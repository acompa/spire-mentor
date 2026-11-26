"""The data behind Chapter 4's plots and tables, written to `chapter_data.json`.

Plays the first `--runs` seeds of `PANEL_A` with the Ironclad checkpoint
(`critic.trace_run`, privileged tokens on, and a second value read with them
removed), caches the traces in `.cache/traces.npz`, and reduces them to the
numbers the chapter prints.

    tour/run stop-4-critic/figures.py [--runs 200] [--workers 3] [--reuse]
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from concurrent.futures import ProcessPoolExecutor
from functools import partial
from pathlib import Path

STOP = Path(__file__).resolve().parent
sys.path.insert(0, str(STOP))

import numpy as np  # noqa: E402
import tourlib  # noqa: E402
from critic import ROOM_NAMES, Trace, fights_of, hand_two_chain, trace_run  # noqa: E402

CKPT = "ironclad_e2e_d9000_ema0999.pt"
CACHE = STOP / ".cache" / "traces.npz"
OUT = STOP / "chapter_data.json"
FIELDS = ("is_combat", "fight", "room", "hp_frac", "act", "floor", "reward", "value",
          "v_combat", "v_run", "value_nopriv")  # fmt: skip
LAMBDAS = (0.0, 0.25, 0.5, 0.75, 0.9, 0.95, 0.97, 0.99, 1.0)
V_BINS = (-1.0, 0.2, 0.4, 0.6, 0.8, 0.9, 1.0, 1.1, 9.0)
HP_BINS = ((0.0, 0.5, "<50%"), (0.5, 0.8, "50-80%"), (0.8, 1.01, ">=80%"))


def save(traces: list[Trace]) -> None:
    CACHE.parent.mkdir(exist_ok=True)
    lens = np.array([len(t) for t in traces])
    cols = {f: np.concatenate([getattr(t, f) for t in traces]) for f in FIELDS}
    np.savez_compressed(CACHE, lens=lens, seeds=[t.seed for t in traces],
                        won=[t.won for t in traces], outcome=[t.outcome for t in traces], **cols)


def load() -> list[Trace]:
    z = np.load(CACHE)
    cuts = np.concatenate([[0], np.cumsum(z["lens"])])
    return [
        Trace(seed=int(z["seeds"][i]), won=bool(z["won"][i]), outcome=str(z["outcome"][i]),
              **{f: z[f][cuts[i]:cuts[i + 1]] for f in FIELDS})
        for i in range(len(z["lens"]))
    ]  # fmt: skip


def r(x: float, k: int = 4) -> float:
    return round(float(x), k)


def trace_view(tr: Trace) -> dict:
    """One run for plotting: the value per row, and one span per fight."""
    spans = []
    t, n = 0, len(tr)
    while t < n:
        if not tr.is_combat[t]:
            t += 1
            continue
        s = t
        while t + 1 < n and tr.is_combat[t + 1] and tr.fight[t + 1] == tr.fight[s]:
            t += 1
        spans.append({"start": s, "end": t, "room": ROOM_NAMES[tr.room[s]], "act": int(tr.act[s]),
                      "floor": int(tr.floor[s])})
        t += 1
    return {
        "seed": tr.seed, "won": tr.won, "n": len(tr),
        "value": [r(v, 3) for v in tr.value],
        "value_nopriv": [r(v, 3) for v in tr.value_nopriv],
        "is_combat": [int(c) for c in tr.is_combat],
        "fights": spans,
    }  # fmt: skip


def lam_sweep(traces: list[Trace]) -> list[dict]:
    """Spread of the advantage estimate as lambda moves from TD(0) to Monte Carlo."""
    from ppo.e2e import algo

    out = []
    for lam in LAMBDAS:
        adv, comb, g, v = [], [], [], []
        for tr in traces:
            a = algo.compute_returns(tr.table(), tr.value, lam_c=lam, lam_r=lam, hold_iters=0).adv
            adv.append(a)
            comb.append(tr.is_combat)
        adv, comb = np.concatenate(adv), np.concatenate(comb)
        out.append({"lam": lam, "std_combat": r(adv[comb].std(), 5),
                    "std_run": r(adv[~comb].std(), 5)})
    return out


def surprise_sites(traces: list[Trace]) -> dict:
    """Mean |delta| at three kinds of row: inside a fight, a fight's last row
    (whose next state is a run row or the end), and run rows (run chain)."""
    inside, last, run = [], [], []
    for tr in traces:
        v, rw, c = tr.value, tr.reward, tr.is_combat
        n = len(tr)
        vn = np.append(v[1:], 0.0)
        d = rw + vn - v
        nxt_c = np.append(c[1:], False)
        same_fight = np.append(tr.fight[1:] == tr.fight[:-1], False)
        inside.append(np.abs(d[c & nxt_c & same_fight]))
        last.append(np.abs(d[c & ~nxt_c]))
        runs = np.flatnonzero(~c)
        ends = np.append(runs[1:], n)
        seg = np.array([rw[i:j].sum() for i, j in zip(runs, ends)])
        vend = np.append(v[runs[1:]], 0.0)
        run.append(np.abs(seg + vend - v[runs]))
    cat = {k: np.concatenate(x) for k, x in (("inside", inside), ("last", last), ("run", run))}
    return {k: {"n": int(len(x)), "mean_abs": r(x.mean(), 5), "sum_abs": r(x.sum(), 2)}
            for k, x in cat.items()}


def gae_stretch(tr: Trace) -> dict:
    from ppo.e2e import algo

    repo = algo.compute_returns(tr.table(), tr.value, hold_iters=0).adv
    hand = hand_two_chain(tr, algo.LAM_C, algo.LAM_R)
    spans = trace_view(tr)["fights"]
    f = next((s for s in spans if s["room"] == "elite"), spans[0])
    rows = []
    for t in range(f["end"] - 5, f["end"] + 1):
        vn = tr.value[t + 1] if t + 1 < len(tr) else 0.0
        rows.append({"t": t, "r": r(tr.reward[t], 3), "v": r(tr.value[t], 5), "v_next": r(vn, 5),
                     "delta": r(tr.reward[t] + vn - tr.value[t], 5), "a_hand": r(hand[t], 6),
                     "a_repo": r(repo[t], 6)})
    b, a = f["start"] - 1, f["end"] + 1
    return {
        "seed": tr.seed, "fight": f, "rows": rows,
        "run_step": {"before": b, "after": a, "v_before": r(tr.value[b], 5),
                     "v_after": r(tr.value[a], 5), "reward": r(tr.reward[b:a].sum(), 4),
                     "a_hand": r(hand[b], 6), "a_repo": r(repo[b], 6)},
        "max_gap_run": float(np.abs(hand - repo).max()),
    }  # fmt: skip


def main() -> None:
    from ppo.e2e import algo

    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--runs", type=int, default=200)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--reuse", action="store_true", help="read .cache/traces.npz")
    args = ap.parse_args()
    if args.reuse and CACHE.is_file():
        traces = load()
    else:
        seeds = list(tourlib.PANEL_A[: args.runs])
        ckpt = str(tourlib.weight(CKPT))
        t0 = time.time()
        traces = []
        with ProcessPoolExecutor(max_workers=args.workers) as pool:
            for i, tr in enumerate(pool.map(partial(trace_run, ckpt=ckpt, no_priv=True), seeds), 1):
                traces.append(tr)
                if i % 20 == 0:
                    print(f"  {i}/{len(seeds)} ({time.time() - t0:.0f} s)", flush=True)
        save(traces)
    print(f"{len(traces)} runs, {sum(len(t) for t in traces)} decisions")

    v = np.concatenate([t.value for t in traces])
    vnp = np.concatenate([t.value_nopriv for t in traces])
    g = np.concatenate([t.returns_to_go() for t in traces])
    won = np.concatenate([np.full(len(t), t.won) for t in traces])
    comb = np.concatenate([t.is_combat for t in traces])
    calib = []
    for lo, hi in zip(V_BINS[:-1], V_BINS[1:]):
        m = (v >= lo) & (v < hi)
        if m.any():
            calib.append({"lo": lo, "hi": hi, "n": int(m.sum()), "mean_v": r(v[m].mean()),
                          "mean_g": r(g[m].mean()), "won": r(won[m].mean())})

    fights = [f for t in traces for f in fights_of(t)]
    fork = []
    for room in ("monster", "elite", "boss"):
        for lo, hi, name in ((0.0, 1.01, "all"), *HP_BINS):
            sel = [f for f in fights if f.room == room and lo <= f.hp_frac < hi]
            if sel:
                fork.append({"room": room, "hp": name, "n": len(sel),
                             "died": sum(f.run_ended for f in sel),
                             "v_before": r(np.mean([f.v_before for f in sel])),
                             "dv": r(np.mean([f.dv for f in sel]), 5),
                             "delta": r(np.mean([f.delta for f in sel]), 5)})
    boss = [f.dv for f in fights if f.room == "boss" and f.act < 3 and not f.run_ended]
    elite = np.mean([f.dv for f in fights if f.room == "elite"])
    ordinary = np.mean([f.dv for f in fights if f.room == "monster"])
    mid = (v >= 0.4) & (v < 0.6)
    lost = next(t for t in traces if not t.won)
    data = {
        "checkpoint": CKPT, "panel": "PANEL_A", "runs": len(traces),
        "decisions": int(len(v)), "won": int(sum(t.won for t in traces)),
        "decisions_per_run": r(len(v) / len(traces), 1),
        "combat_share": r(comb.mean()),
        "start_value": r(np.mean([t.value[0] for t in traces])),
        "mean_total_reward": r(np.mean([t.reward.sum() for t in traces])),
        "boss_won_dv_median": r(np.median(boss), 5), "boss_won_n": len(boss),
        "elite_dv": r(elite, 5), "ordinary_dv": r(ordinary, 5),
        "win_at_half": r(won[mid].mean()), "n_at_half": int(mid.sum()),
        "weight_20": r(algo.LAM_C**20, 4),
        "ev": {
            "combat": r(algo.explained_variance(g, v, comb)),
            "run": r(algo.explained_variance(g, v, ~comb)),
            "combat_nopriv": r(algo.explained_variance(g, vnp, comb)),
            "run_nopriv": r(algo.explained_variance(g, vnp, ~comb)),
        },
        "calibration": calib, "fork": fork, "lam_sweep": lam_sweep(traces),
        "surprise_sites": surprise_sites(traces),
        "gae": gae_stretch(traces[0]),
        "trace_won": trace_view(traces[0]), "trace_lost": trace_view(lost),
    }  # fmt: skip
    OUT.write_text(json.dumps(data, indent=1) + "\n")
    print(json.dumps({k: v for k, v in data.items() if not k.startswith("trace")}, indent=1))


if __name__ == "__main__":
    main()
