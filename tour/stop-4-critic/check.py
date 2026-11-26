"""Stop 4: the critic and the advantage.

Plays runs of `PANEL_A` with the Ironclad checkpoint, reads the critic's
value at every decision, and measures:

1. one run's value trace, with fights, elites, bosses and floors marked;
2. calibration: states grouped by predicted value against what followed;
3. one fight's surprises and GAE by hand, against `algo.compute_returns`;
4. the elite fork: value just before and just after each fight, by room
   type and HP.

    tour/run stop-4-critic/check.py [--quick] [--runs N] [--workers N]
"""

from __future__ import annotations

import argparse
import sys
import time
from concurrent.futures import ProcessPoolExecutor
from functools import partial
from pathlib import Path

STOP = Path(__file__).resolve().parent
sys.path.insert(0, str(STOP))

import numpy as np  # noqa: E402
import tourlib  # noqa: E402
from critic import (  # noqa: E402
    ROOM_NAMES,
    Fight,
    Trace,
    fights_of,
    hand_two_chain,
    trace_run,
    weight_of_later_surprise,
)

CKPT = "ironclad_e2e_d9000_ema0999.pt"
HP_BINS = ((0.0, 0.5, "hp < 50%"), (0.5, 0.8, "hp 50-80%"), (0.8, 1.01, "hp >= 80%"))
V_BINS = (-1.0, 0.2, 0.4, 0.6, 0.8, 0.9, 1.0, 1.1, 9.0)


def play(seeds: list[int], workers: int) -> list[Trace]:
    ckpt = str(tourlib.weight(CKPT))
    t0 = time.time()
    with ProcessPoolExecutor(max_workers=workers) as pool:
        out = []
        for i, tr in enumerate(pool.map(partial(trace_run, ckpt=ckpt), seeds), 1):
            out.append(tr)
            if i % 25 == 0 or i == len(seeds):
                print(f"  {i}/{len(seeds)} runs traced ({time.time() - t0:.0f} s)", flush=True)
    return out


# ── 1. one run's value trace ──────────────────────────────────────────────


def show_trace(tr: Trace) -> None:
    print(f"\n== 1. Value trace of seed {tr.seed} ({'won' if tr.won else tr.outcome}, "
          f"{len(tr)} decisions) ==")
    print("Run rows show V_run. A fight is one line: V_combat at its first and last")
    print("decision, its lowest V_combat, and V_run at the run row that follows.\n")
    print(f"{'rows':>9}  {'act':>3} {'floor':>5}  {'event':<22} {'hp':>5}  "
          f"{'V first':>8} {'V last':>8} {'V min':>8}  {'V next':>8}  reward")
    n, t = len(tr), 0
    while t < n:
        start = t
        if tr.is_combat[t]:
            while t + 1 < n and tr.is_combat[t + 1] and tr.fight[t + 1] == tr.fight[start]:
                t += 1
            seg = tr.value[start : t + 1]
            room = ROOM_NAMES[tr.room[start]]
            nxt = f"{tr.value[t + 1]:8.3f}" if t + 1 < n else "     end"
            label = f"fight {tr.fight[start]} ({room})"
            if room in ("elite", "boss"):
                label = f"** {label}"
        else:
            while t + 1 < n and not tr.is_combat[t + 1]:
                t += 1
            seg = tr.value[start : t + 1]
            nxt = f"{tr.value[t + 1]:8.3f}" if t + 1 < n else "     end"
            label = f"{t - start + 1} run decision(s)"
        r = tr.reward[start : t + 1].sum()
        print(f"{start:>4}-{t:<4}  {tr.act[start]:>3} {tr.floor[start]:>5}  {label:<22} "
              f"{tr.hp_frac[start]:5.2f}  {seg[0]:8.3f} {seg[-1]:8.3f} {seg.min():8.3f}  "
              f"{nxt}  {r:.3f}")
        t += 1
    print(f"\nTotal reward collected: {tr.reward.sum():.3f}")


# ── 2. calibration ────────────────────────────────────────────────────────


def calibration(traces: list[Trace]) -> float:
    v = np.concatenate([tr.value for tr in traces])
    g = np.concatenate([tr.returns_to_go() for tr in traces])
    won = np.concatenate([np.full(len(tr), tr.won) for tr in traces])
    run = np.concatenate([np.full(len(tr), i) for i, tr in enumerate(traces)])
    print(f"\n== 2. Calibration over {len(traces)} runs, {len(v)} decisions ==")
    print("Each decision is grouped by the critic's value V. G is the reward that")
    print("actually followed that decision, to the end of its run.\n")
    print(f"{'V from':>7} {'to':>6}  {'decisions':>9} {'runs':>5}  {'mean V':>7} "
          f"{'mean G':>7}  {'won':>6}")
    for lo, hi in zip(V_BINS[:-1], V_BINS[1:]):
        m = (v >= lo) & (v < hi)
        if not m.any():
            continue
        print(f"{lo:7.1f} {hi:6.1f}  {int(m.sum()):9d} {len(np.unique(run[m])):5d}  "
              f"{v[m].mean():7.3f} {g[m].mean():7.3f}  {won[m].mean():6.1%}")
    mid = (v >= 0.4) & (v < 0.6)
    print(f"\nV between 0.4 and 0.6: {int(mid.sum())} decisions from "
          f"{len(np.unique(run[mid]))} runs.")
    return float(won[mid].mean()) if mid.any() else float("nan")


def explained_in_fights(traces: list[Trace]) -> float:
    from ppo.e2e import algo

    v = np.concatenate([tr.value for tr in traces])
    g = np.concatenate([tr.returns_to_go() for tr in traces])
    comb = np.concatenate([tr.is_combat for tr in traces])
    ev = algo.explained_variance(g, v, comb)
    print(f"Inside fights, 1 - Var(G - V) / Var(G) = {ev:.3f} "
          f"(`algo.explained_variance`, target = return to go).")
    return ev


# ── 3. GAE by hand ────────────────────────────────────────────────────────


def gae_by_hand(tr: Trace) -> None:
    from ppo.e2e import algo

    lc, lr = algo.LAM_C, algo.LAM_R
    repo = algo.compute_returns(tr.table(), tr.value, hold_iters=0).adv
    hand = hand_two_chain(tr, lc, lr)
    fights = [f for f in range(tr.fight.max() + 1) if (tr.fight == f).any()]
    elites = [f for f in fights if ROOM_NAMES[tr.room[np.argmax(tr.fight == f)]] == "elite"]
    f = (elites or fights)[0]
    rows = np.flatnonzero(tr.fight == f)
    end = int(rows[-1])
    show = list(range(max(int(rows[0]), end - 7), end + 1))
    print(f"\n== 3. Surprises and GAE by hand: the last {len(show)} decisions of fight {f} "
          f"({ROOM_NAMES[tr.room[rows[0]]]}), seed {tr.seed} ==")
    print(f"delta_t = r_t + V_(t+1) - V_t.  A_t = delta_t + {lc} * A_(t+1) while the next "
          "row is a combat row.")
    print("The last row's V_(t+1) is the run row after the fight (V_run).\n")
    print(f"{'row':>5} {'r':>7} {'V':>8} {'V next':>8} {'delta':>8} {'A hand':>9} "
          f"{'A repo':>9}")
    for t in show:
        vn = tr.value[t + 1] if t + 1 < len(tr) else 0.0
        delta = tr.reward[t] + vn - tr.value[t]
        print(f"{t:>5} {tr.reward[t]:7.3f} {tr.value[t]:8.4f} {vn:8.4f} {delta:+8.4f} "
              f"{hand[t]:+9.5f} {repo[t]:+9.5f}")
    before = int(rows[0]) - 1
    if before >= 0 and not tr.is_combat[before]:
        after = end + 1
        va = tr.value[after] if after < len(tr) else 0.0
        r = tr.reward[before:after].sum()
        print(f"\nThe run chain steps over the fight: run row {before} -> "
              f"{'row ' + str(after) if after < len(tr) else 'end'}.")
        print(f"delta = R {r:.3f} + V_after {va:.4f} - V_before {tr.value[before]:.4f} "
              f"= {r + va - tr.value[before]:+.4f};  A hand {hand[before]:+.5f}, "
              f"A repo {repo[before]:+.5f}  (lambda_r = {lr})")
    worst = float(np.abs(hand - repo).max())
    print(f"\nWhole run, every row: largest |A hand - A repo| = {worst:.2e}")


# ── 4. the elite fork ─────────────────────────────────────────────────────


def elite_fork(fights: list[Fight]) -> tuple[float, float]:
    print("\n== 4. The elite fork: V_run just before and just after each fight ==")
    print("dV = V_after - V_before. delta adds the reward collected across the fight")
    print("(floors and boss bonuses): the run chain's surprise for that step.\n")
    print(f"{'room':<8} {'hp at start':<11} {'fights':>6} {'died':>5} {'mean V_before':>13} "
          f"{'mean dV':>8} {'mean delta':>10}")
    for room in ("monster", "elite", "boss"):
        for lo, hi, name in ((0.0, 1.01, "all"), *HP_BINS):
            sel = [f for f in fights if f.room == room and lo <= f.hp_frac < hi]
            if not sel:
                continue
            print(f"{room:<8} {name:<11} {len(sel):6d} {sum(f.run_ended for f in sel):5d} "
                  f"{np.mean([f.v_before for f in sel]):13.3f} "
                  f"{np.mean([f.dv for f in sel]):+8.4f} {np.mean([f.delta for f in sel]):+10.4f}")
    elite = [f.dv for f in fights if f.room == "elite"]
    ordinary = [f.dv for f in fights if f.room == "monster"]
    gap = float(np.mean(elite) - np.mean(ordinary)) if elite and ordinary else float("nan")
    boss = [f.dv for f in fights if f.room == "boss" and f.act < 3 and not f.run_ended]
    print(f"\nWon act 1 and act 2 boss fights: {len(boss)}, median dV {np.median(boss):+.4f}"
          if boss else "\nNo won act 1 or act 2 boss fights.")
    return gap, float(np.median(boss)) if boss else float("nan")


def main() -> None:
    preds, dry = tourlib.load_predictions(STOP)
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--quick", action="store_true", help="40 runs instead of 300")
    ap.add_argument("--runs", type=int, default=None)
    ap.add_argument("--workers", type=int, default=3)
    args = ap.parse_args()
    n = args.runs or (40 if args.quick else 300)
    seeds = list(tourlib.PANEL_A[:n])
    print(f"Tracing {n} runs of PANEL_A with {CKPT} on {args.workers} processes ...")
    traces = play(seeds, args.workers)
    bad = [tr for tr in traces if tr.outcome not in ("won", "lost")]
    if bad:
        print(f"  {len(bad)} run(s) ended by error or cap; kept as losses: "
              f"{[(tr.seed, tr.outcome) for tr in bad]}")
    print(f"  won {sum(tr.won for tr in traces)} of {len(traces)}")

    show_trace(traces[0])
    win_at_half = calibration(traces)
    ev = explained_in_fights(traces)
    gae_by_hand(traces[0])
    fights = [f for tr in traces for f in fights_of(tr)]
    gap, boss_dv = elite_fork(fights)

    from ppo.e2e import algo

    w20 = weight_of_later_surprise(20, algo.LAM_C)
    print(f"\nA unit surprise 20 decisions later, through `compute_returns` with "
          f"lambda {algo.LAM_C}: weight {w20:.4f}")

    tourlib.score(
        STOP,
        {
            "start_value": float(np.mean([tr.value[0] for tr in traces])),
            "boss_won_dv": boss_dv,
            "elite_minus_ordinary_dv": gap,
            "win_rate_at_half": win_at_half,
            "weight_20_later": w20,
            "explained_in_fights": ev,
        },
    )


if __name__ == "__main__":
    main()
