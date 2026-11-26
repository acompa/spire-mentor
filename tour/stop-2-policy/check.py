"""Stop 2: what the policy network outputs at a decision, and what greedy play discards.

Three parts:

1. One decision in a fight and one between fights, with each offered class's
   logit and probability.
2. Along greedy runs of the published Ironclad net on `PANEL_A`: the
   probability of the top class at every decision, entropy by decision type,
   and how often the top class holds more than 0.9.
3. The same seeds played again, sampling each action from the probabilities
   (temperature 1). A paired win table, and how often the sampled class
   differed from the greedy one.

The loop is `E2eDriver.peek` -> `E2eStudent.decide` -> `E2ePeek.class_action`
-> `E2eDriver.apply` (sts2-rs/crates/sts2-py/src/e2e.rs). `decide` returns the
real network's logits; `e2e_selfplay` does not. `test_check.py` pins that the
greedy loop plays exactly the classes `e2e_selfplay` plays.

    tour/run stop-2-policy/check.py [--quick] [--seeds N] [--workers K]
"""

from __future__ import annotations

import argparse
import math
import statistics
import time
from collections import defaultdict
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
from pathlib import Path

import numpy as np

import tourlib

STOP = Path(__file__).resolve().parent
NET = "ironclad_e2e_d9000_ema0999.e2ew"
MAX_DECISIONS = 6000  # e2e_selfplay's default cap
TOP_SURE = 0.9


# ── the maths of one decision ──


def softmax(logits) -> np.ndarray:
    """Probabilities from scores: exp(z - max) / sum. Subtracting the max
    changes nothing in the result and keeps exp from overflowing."""
    z = np.asarray(logits, dtype=np.float64)
    e = np.exp(z - z.max())
    return e / e.sum()


def entropy(p) -> float:
    """-sum p log p, in nats. 0 when one class has all the mass; log(n) when
    all n classes are equally likely. Classes with p = 0 add nothing."""
    p = np.asarray(p, dtype=np.float64)
    nz = p[p > 0]
    return float(-(nz * np.log(nz)).sum())


# ── one run ──


@dataclass(frozen=True)
class Step:
    """One decision along a run."""

    dtype: str
    is_combat: bool
    n_classes: int
    top_p: float
    entropy: float
    greedy: int
    played: int


@dataclass(frozen=True)
class Run:
    seed: int
    mode: str
    won: bool
    outcome: str
    steps: list[Step]


_student = None


def _init_worker(path: str) -> None:
    global _student
    import sts2_rs

    _student = sts2_rs.E2eStudent(path)


def play(student, seed: int, mode: str = "greedy", on_decision=None,
         max_decisions: int = MAX_DECISIONS) -> Run:
    """Play `seed` from the start with `student`.

    `mode` "greedy" plays the top class; "sample" draws a class from the
    softmax of the logits (temperature 1) with a generator seeded by `seed`.
    `on_decision(decision, out, rm)` sees every decision before it is applied.
    """
    import sts2_rs

    rng = np.random.default_rng([seed, 2])
    rm = sts2_rs.e2e_new_run(seed)
    drv = sts2_rs.E2eDriver(seed)
    steps: list[Step] = []
    while True:
        peek = drv.peek(rm)
        if peek.kind == "terminal":
            t = peek.terminal
            return Run(seed, mode, bool(t["won"]), t["outcome"], steps)
        if peek.kind == "forced":
            f = peek.forced
            drv.apply(rm, f["action"], f["kind"], strict=False)
            continue
        if len(steps) >= max_decisions:
            return Run(seed, mode, False, "stopped", steps)
        d = peek.decision
        try:
            out = student.decide(drv, peek, rm)
        except RuntimeError as e:
            return Run(seed, mode, False, f"error: {e}", steps)
        p = softmax(out["logits"])
        greedy = int(out["class"])
        played = greedy if mode == "greedy" else int(rng.choice(len(p), p=p))
        if on_decision is not None:
            on_decision(d, out, rm)
        steps.append(Step(d["decision_type"], bool(d["is_combat"]), len(p),
                          float(p.max()), entropy(p), greedy, played))
        ca = peek.class_action(played)
        drv.apply(rm, ca["action"], ca["kind"])


def _play_job(job: tuple[int, str]) -> Run:
    seed, mode = job
    return play(_student, seed, mode)


# ── part 1: two decisions up close ──


def _card(hand: list[dict], i) -> str:
    return hand[i]["id"] if i is not None and 0 <= i < len(hand) else f"card {i}"


def label(d: dict, cls: dict, rm) -> str:
    """A readable name for one offered class: its first member action, with
    card and enemy names in a fight, plus how many identical actions it merges."""
    a = cls["actions"][0]
    kind = cls["kind"]
    text = kind
    if d["is_combat"]:
        try:
            st = rm.combat_state().get_state()
            hand, foes = st.get("hand", []), st.get("enemies", [])
        except Exception:
            hand, foes = [], []
        if a.get("action") == "play":
            text = f"play {_card(hand, a.get('card_index'))}"
            ti = a.get("target_index")
            if ti is not None and ti < len(foes):
                text += f" -> {foes[ti]['id']}"
        elif a.get("action") == "end_turn":
            text = "end turn"
        else:
            text = f"{kind} {_args(a)}"
    elif d["decision_type"] == "CARD_REWARD":
        try:
            ids = rm.card_reward_option_ids()
            text = (f"take {ids[a['idx']].removeprefix('CARD.')}"
                    if a.get("idx") is not None else "skip the reward")
        except Exception:
            text = f"{kind} {_args(a)}"
    else:
        text = f"{kind} {_args(a)}"
    n = len(cls["actions"])
    return text + (f"  (x{n} merged)" if n > 1 else "")


def _args(a: dict) -> str:
    return " ".join(f"{k}={v}" for k, v in a.items() if k != "action")


def show_two(student, seed: int) -> None:
    """Print the first fight decision with at least three classes and the
    first card reward of the greedy run on `seed`."""
    want = {"fight": None, "between": None}

    def grab(d, out, rm):
        if want["fight"] is None and d["is_combat"] and d["n_classes"] >= 3:
            want["fight"] = (d, out, [label(d, c, rm) for c in d["classes"]])
        if want["between"] is None and d["decision_type"] == "CARD_REWARD":
            want["between"] = (d, out, [label(d, c, rm) for c in d["classes"]])

    play(student, seed, "greedy", on_decision=grab, max_decisions=200)
    for name, got in want.items():
        if got is None:
            print(f"\n(no {name} decision found in the first 200 decisions)")
            continue
        d, out, labels = got
        z = np.asarray(out["logits"], dtype=np.float64)
        p = softmax(z)
        head = "combat" if d["is_combat"] else "out-of-combat"
        print(f"\n{d['decision_type']} at act {d['act']}, floor {d['floor']}: "
              f"{len(p)} offered classes, read by the {head} head")
        print(f"  {'class':>5}  {'logit':>9}  {'probability':>11}  action")
        for i in np.argsort(-z):
            mark = "  <- greedy" if i == out["class"] else ""
            print(f"  {i:>5}  {z[i]:>9.3f}  {p[i]:>11.4g}  {labels[i]}{mark}")
        print(f"  entropy {entropy(p):.3f} nats (uniform over {len(p)} would be "
              f"{math.log(len(p)):.3f})")


# ── parts 2 and 3: summaries ──


def distribution(runs: list[Run]) -> dict:
    steps = [s for r in runs for s in r.steps]
    fight = [s.top_p for s in steps if s.is_combat]
    between = [s.top_p for s in steps if not s.is_combat]
    by_type: dict[str, list[Step]] = defaultdict(list)
    for s in steps:
        by_type[s.dtype].append(s)
    return {
        "n": len(steps),
        "median_top_fight": statistics.median(fight) if fight else float("nan"),
        "median_top_between": statistics.median(between) if between else float("nan"),
        "share_sure": sum(s.top_p > TOP_SURE for s in steps) / len(steps),
        "by_type": by_type,
    }


def paired(greedy: list[Run], sampled: list[Run]) -> dict[str, int]:
    """Win table over seeds played both ways."""
    g = {r.seed: r.won for r in greedy}
    s = {r.seed: r.won for r in sampled}
    seeds = sorted(set(g) & set(s))
    t = {"both": 0, "only_greedy": 0, "only_sampled": 0, "neither": 0}
    for k in seeds:
        key = ("both" if s[k] else "only_greedy") if g[k] else ("only_sampled" if s[k] else "neither")
        t[key] += 1
    return t


def print_types(by_type: dict[str, list[Step]]) -> None:
    print(f"\n  {'decision type':<14} {'count':>7} {'combat':>6} {'classes':>7} "
          f"{'top p':>7} {'top>0.9':>8} {'H nats':>7} {'H/log n':>8}")
    for dt, ss in sorted(by_type.items(), key=lambda kv: -len(kv[1])):
        n = len(ss)
        norm = [s.entropy / math.log(s.n_classes) for s in ss if s.n_classes > 1]
        print(f"  {dt:<14} {n:>7} {'yes' if ss[0].is_combat else 'no':>6} "
              f"{statistics.mean(s.n_classes for s in ss):>7.1f} "
              f"{statistics.median(s.top_p for s in ss):>7.3f} "
              f"{sum(s.top_p > TOP_SURE for s in ss) / n:>8.1%} "
              f"{statistics.mean(s.entropy for s in ss):>7.3f} "
              f"{(statistics.mean(norm) if norm else 0.0):>8.3f}")
    print("  (top p is the median; H is the mean entropy; H/log n divides by the"
          " uniform entropy for that many classes)")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--quick", action="store_true", help="20 seeds instead of 150")
    ap.add_argument("--seeds", type=int, help="number of PANEL_A seeds")
    ap.add_argument("--workers", type=int, default=4)
    args = ap.parse_args()

    tourlib.load_predictions(STOP)  # refuse before any slow work
    n_seeds = args.seeds or (20 if args.quick else 150)
    seeds = list(tourlib.PANEL_A[:n_seeds])
    path = str(tourlib.weight(NET))

    import sts2_rs

    t0 = time.time()
    print(f"Part 1. Two decisions from the greedy run on seed {seeds[0]}.")
    show_two(sts2_rs.E2eStudent(path), seeds[0])

    print(f"\nPlaying {n_seeds} seeds twice (greedy, then sampled) on "
          f"{args.workers} processes...")
    jobs = [(s, "greedy") for s in seeds] + [(s, "sample") for s in seeds]
    with ProcessPoolExecutor(args.workers, initializer=_init_worker, initargs=(path,)) as pool:
        runs = list(pool.map(_play_job, jobs))
    greedy = [r for r in runs if r.mode == "greedy"]
    sampled = [r for r in runs if r.mode == "sample"]
    for r in runs:
        if r.outcome not in ("won", "lost") and not r.outcome.startswith("died"):
            print(f"  note: seed {r.seed} ({r.mode}) ended '{r.outcome}', counted as a loss")

    dist = distribution(greedy)
    print(f"\nPart 2. The policy's probabilities at {dist['n']} greedy decisions.")
    print(f"  median top-class probability, fights:          {dist['median_top_fight']:.4f}")
    print(f"  median top-class probability, between fights: {dist['median_top_between']:.4f}")
    print(f"  share of decisions with top class above {TOP_SURE}:  {dist['share_sure']:.1%}")
    print_types(dist["by_type"])

    ss = [s for r in sampled for s in r.steps]
    differ = sum(s.played != s.greedy for s in ss) / len(ss)
    t = paired(greedy, sampled)
    wr_g = sum(r.won for r in greedy) / len(greedy)
    wr_s = sum(r.won for r in sampled) / len(sampled)
    print(f"\nPart 3. Greedy against sampled (temperature 1) on the same {n_seeds} seeds.")
    print(f"  {'':>16}  {'sampled won':>11}  {'sampled lost':>12}")
    print(f"  {'greedy won':>16}  {t['both']:>11}  {t['only_greedy']:>12}")
    print(f"  {'greedy lost':>16}  {t['only_sampled']:>11}  {t['neither']:>12}")
    print(f"  win rate: greedy {wr_g:.1%}, sampled {wr_s:.1%}")
    print(f"  sampled decisions: {len(ss)}; the sampled class differed from the "
          f"greedy one at {differ:.2%}")
    print(f"  decisions per run: greedy {len([s for r in greedy for s in r.steps]) / n_seeds:.0f}, "
          f"sampled {len(ss) / n_seeds:.0f}")
    print(f"\n({time.time() - t0:.0f} s)")

    tourlib.score(STOP, {
        "median_top_p_fight": dist["median_top_fight"],
        "median_top_p_between": dist["median_top_between"],
        "share_top_over_0_9": dist["share_sure"],
        "share_sampled_differs": differ,
        "winrate_sampled_minus_greedy_pp": 100 * (wr_s - wr_g),
        "share_seeds_disagree": (t["only_greedy"] + t["only_sampled"]) / n_seeds,
    })


if __name__ == "__main__":
    main()
