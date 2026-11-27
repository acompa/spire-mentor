"""Stop 6: how far importance weights can correct off-policy data.

Network A (the published Ironclad net) plays whole runs, sampling each
decision from its own probabilities: A is the behaviour policy. At every
decision the check also asks network B (the all-character net) for its
probabilities on the same state. From those it measures how well importance
weights correct A's data towards two target policies:

- a small gap: A's log-probabilities moved a step `eps` of the way towards
  B's (a stand-in for the trainer's few-updates-old actors);
- a large gap: B itself.

Then B plays the same seeds, sampling from its own probabilities, so the
importance-sampling estimate of B's win rate can be compared with the truth.

    tour/run stop-6-off-policy/check.py [--quick] [--runs N] [--workers N]
"""

from __future__ import annotations

import argparse
import math
import time
from collections import Counter
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
from pathlib import Path

import numpy as np

import tourlib

STOP = Path(__file__).resolve().parent
NET_A = "ironclad_e2e_d9000_ema0999.e2ew"
NET_B = "allchar_e2e_d4500_ema0999.e2ew"
#: The trainer's clamp on the importance weight (`algo.LossCfg.is_clamp`).
CLAMP = 2.0
#: The small gap: how far the target moves from A towards B.
SMALL_EPS = 0.1
SWEEP_EPS = (0.01, 0.03, 0.1, 0.3, 1.0)
WINDOWS = (1, 10, 100)


# --- arithmetic (tested in test_check.py) -----------------------------------


def log_softmax(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, np.float64)
    m = x.max()
    return x - (m + math.log(np.exp(x - m).sum()))


def mix_logp(logp_a: np.ndarray, logp_b: np.ndarray, eps: float) -> np.ndarray:
    """log π_eps: the softmax of (1 - eps)·log π_A + eps·log π_B.
    eps = 0 is A itself, eps = 1 is B."""
    return log_softmax((1.0 - eps) * np.asarray(logp_a) + eps * np.asarray(logp_b))


def logsumexp(x: np.ndarray) -> float:
    x = np.asarray(x, np.float64)
    if x.size == 0:
        return -math.inf
    m = x.max()
    if not math.isfinite(m):
        return float(m)
    return float(m + math.log(np.exp(x - m).sum()))


def ess_share(log_w: np.ndarray) -> float:
    """Effective sample size (Σw)² / Σw², as a share of the n weights.

    Computed from log-weights, so products of hundreds of ratios neither
    overflow nor underflow. Equal weights give 1; one weight that dwarfs the
    rest gives 1/n."""
    log_w = np.asarray(log_w, np.float64)
    if log_w.size == 0:
        return math.nan
    return math.exp(2.0 * logsumexp(log_w) - logsumexp(2.0 * log_w)) / log_w.size


def clamped_share(log_w: np.ndarray, cap: float = CLAMP) -> float:
    """Share of weights the trainer's clamp would cut (w > cap), as in
    `ppo.e2e.update.is_tails`."""
    return float((np.exp(np.asarray(log_w, np.float64)) > cap).mean())


def window_log_weights(per_run: list[np.ndarray], k: int) -> np.ndarray:
    """Sum log-ratios over consecutive, non-overlapping windows of k decisions
    inside each run (a run's leftover tail is dropped): the log of the product
    of k per-decision ratios."""
    out = []
    for lr in per_run:
        n = len(lr) // k
        if n:
            out.append(np.asarray(lr[: n * k], np.float64).reshape(n, k).sum(1))
    return np.concatenate(out) if out else np.zeros(0)


def measured_log10(x: float) -> str:
    return f"{math.log10(x):.1f}" if x > 0 else "-inf"


def is_estimates(log_w: np.ndarray, won: np.ndarray) -> tuple[float, float]:
    """(plain, self-normalised) importance-sampling estimates of the target's
    win rate from per-run log-weights and outcomes.

    plain = mean(w·won); self-normalised = Σ w·won / Σ w. The plain estimate
    can overflow, so it is returned as a float that may be inf."""
    log_w = np.asarray(log_w, np.float64)
    won = np.asarray(won, bool)
    lw_won = logsumexp(log_w[won])
    plain_log = lw_won - math.log(len(log_w))
    plain = math.exp(plain_log) if plain_log < 700 else math.inf
    snis = math.exp(lw_won - logsumexp(log_w)) if won.any() else 0.0
    return plain, snis


# --- playing ----------------------------------------------------------------

_NETS: dict[str, object] = {}


def _init(paths: dict[str, str]) -> None:
    import sts2_rs

    for k, p in paths.items():
        _NETS[k] = sts2_rs.E2eStudent(p)


@dataclass
class Played:
    seed: int
    won: bool
    outcome: str
    chosen: list[int]
    logp_a: list[np.ndarray]  # A's log-probs over the classes, per decision
    logp_b: list[np.ndarray]  # B's, on the same state


def play(seed: int, actor: str, score_other: bool) -> Played:
    """`actor` ("A" or "B") plays `seed` from the start, sampling every
    decision from its own probabilities, through the same peek/decide/apply
    loop the advisor uses (`sts2-py/src/e2e.rs`). With `score_other`, the
    other net is asked for its probabilities on every state too; it never
    changes what is played."""
    import sts2_rs

    rng = np.random.default_rng(seed)
    me = _NETS[actor]
    other = _NETS["B" if actor == "A" else "A"]
    rm = sts2_rs.e2e_new_run(seed)
    drv = sts2_rs.E2eDriver(seed)
    out = Played(seed, False, "", [], [], [])
    while True:
        p = drv.peek(rm)
        if p.kind == "terminal":
            t = p.terminal
            out.won, out.outcome = bool(t["won"]), str(t["outcome"])
            return out
        if p.kind == "forced":
            f = p.forced
            drv.apply(rm, f["action"], f["kind"])
            continue
        lp_me = log_softmax(me.decide(drv, p, rm)["logits"])
        c = int(rng.choice(len(lp_me), p=np.exp(lp_me) / np.exp(lp_me).sum()))
        if score_other:
            lp_other = log_softmax(other.decide(drv, p, rm)["logits"])
            if len(lp_other) != len(lp_me):
                raise RuntimeError(f"seed {seed}: the nets see different class counts")
            a, b = (lp_me, lp_other) if actor == "A" else (lp_other, lp_me)
            out.logp_a.append(a.astype(np.float32))
            out.logp_b.append(b.astype(np.float32))
        out.chosen.append(c)
        act = p.class_action(c)
        drv.apply(rm, act["action"], act["kind"])


def _play_a(seed: int) -> Played:
    return play(seed, "A", score_other=True)


def _play_b(seed: int) -> Played:
    return play(seed, "B", score_other=False)


# --- the check --------------------------------------------------------------


def per_run_log_ratios(runs: list[Played], eps: float) -> list[np.ndarray]:
    """log(π_target / π_A) of the action A took, per decision, per run."""
    out = []
    for r in runs:
        lr = np.empty(len(r.chosen))
        for i, (c, la, lb) in enumerate(zip(r.chosen, r.logp_a, r.logp_b)):
            la = la.astype(np.float64)
            lt = mix_logp(la, lb, eps) if eps < 1.0 else lb.astype(np.float64)
            lr[i] = lt[c] - la[c]
        out.append(lr)
    return out


def describe(name: str, per_run: list[np.ndarray]) -> dict[str, float]:
    flat = np.concatenate(per_run)
    w = np.exp(flat)
    run_lw = np.array([lr.sum() for lr in per_run])
    q = np.quantile(w, [0.01, 0.5, 0.99])
    d = {
        "clamped": clamped_share(flat),
        "ess_decision": ess_share(flat),
        "ess_run": ess_share(run_lw),
    }
    print(f"\n{name}")
    print(f"  per-decision weight: p01 {q[0]:.3g}  median {q[1]:.3g}  p99 {q[2]:.3g}  "
          f"max {w.max():.3g}")
    print(f"  share of per-decision weights above the clamp ({CLAMP:g}): {d['clamped']:.2%}")
    print(f"  ESS of per-decision weights: {d['ess_decision']:.1%} of {len(flat)} decisions")
    print(f"  per-run log10 weight: min {run_lw.min() / math.log(10):.1f}  "
          f"median {np.median(run_lw) / math.log(10):.1f}  max {run_lw.max() / math.log(10):.1f}")
    print(f"  ESS of per-run weights: {d['ess_run']:.1%} of {len(run_lw)} runs")
    return d


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true", help="8 runs per net, about two minutes")
    ap.add_argument("--runs", type=int, default=None, help="runs per net (default 48)")
    ap.add_argument("--workers", type=int, default=4)
    args = ap.parse_args()
    tourlib.load_predictions(STOP)  # the gate: nothing slow runs without predictions
    n = args.runs or (8 if args.quick else 48)
    seeds = list(tourlib.PANEL_A[:n])
    paths = {"A": str(tourlib.weight(NET_A)), "B": str(tourlib.weight(NET_B))}

    t0 = time.time()
    print(f"A plays {n} runs sampling from its own probabilities; B scores every state.")
    with ProcessPoolExecutor(args.workers, initializer=_init, initargs=(paths,)) as pool:
        runs_a = list(pool.map(_play_a, seeds))
        t1 = time.time()
        print(f"  done in {t1 - t0:.0f} s. B now plays the same {n} seeds, sampling.")
        runs_b = list(pool.map(_play_b, seeds))
    t2 = time.time()
    print(f"  done in {t2 - t1:.0f} s.")

    n_dec = sum(len(r.chosen) for r in runs_a)
    agree = np.mean([int(np.argmax(la)) == int(np.argmax(lb))
                     for r in runs_a for la, lb in zip(r.logp_a, r.logp_b)])
    print(f"\n{n_dec} decisions by A ({n_dec / n:.0f} per run).")
    print(f"A and B rank the same action first on {agree:.1%} of them.")
    for who, rs in (("A", runs_a), ("B", runs_b)):
        ends = Counter(r.outcome for r in rs)
        print(f"{who} won {sum(r.won for r in rs)} of {n} sampled runs; outcomes {dict(ends)}")

    small = describe(f"Small gap: target = A moved {SMALL_EPS:g} of the way to B",
                     per_run_log_ratios(runs_a, SMALL_EPS))
    lr_large = per_run_log_ratios(runs_a, 1.0)
    large = describe("Large gap: target = B", lr_large)

    print("\nHow the gap and the horizon break the correction.")
    print("ESS (as a share) of products of k consecutive per-decision ratios:")
    head = "".join(f"{f'k={k}':>9}" for k in WINDOWS) + f"{'whole run':>11}{'clamped':>9}"
    print(f"  {'eps':>5}{head}")
    for eps in SWEEP_EPS:
        lr = per_run_log_ratios(runs_a, eps)
        cells = "".join(f"{ess_share(window_log_weights(lr, k)):>9.1%}" for k in WINDOWS)
        whole = ess_share(np.array([x.sum() for x in lr]))
        print(f"  {eps:>5g}{cells}{whole:>11.1%}{clamped_share(np.concatenate(lr)):>9.2%}")

    won_a = np.array([r.won for r in runs_a])
    plain, snis = is_estimates(np.array([x.sum() for x in lr_large]), won_a)
    truth = float(np.mean([r.won for r in runs_b]))
    print("\nOff-policy evaluation of B from A's runs (per-run weights):")
    print(f"  A's own win rate (no correction): {won_a.mean():.1%}")
    print(f"  plain IS estimate:                {plain:.3g}  (log10 {measured_log10(plain)})")
    print(f"  self-normalised IS estimate:      {snis:.1%}")
    print(f"  B's measured win rate (truth):    {truth:.1%}  on the same {n} seeds")
    print(f"\nWall time: {time.time() - t0:.0f} s")

    measured = {
        "small_gap_clamped_pct": 100 * small["clamped"],
        "large_gap_clamped_pct": 100 * large["clamped"],
        "top_action_agreement_pct": 100 * agree,
        "small_gap_run_ess_pct": 100 * small["ess_run"],
        "large_gap_run_ess_pct": 100 * large["ess_run"],
        "plain_is_log10": math.log10(plain) if plain > 0 else -math.inf,
        "snis_error_pp": 100 * abs(snis - truth),
    }
    tourlib.score(STOP, {k: float(v) for k, v in measured.items()})


if __name__ == "__main__":
    main()
