"""Stop 8: evaluation discipline.

Two published networks play Ironclad greedily on the same seeds. The check
reports each win rate with its Wilson interval, the paired comparison (McNemar
and the Newcombe paired interval) beside the unpaired one, how much one
network's win rate moves between disjoint slices of seeds, and what larger
experiments would cost on this Mac.

    tour/run stop-8-evaluation/check.py              # 200 seeds per panel, about 12 min on 10 threads
    tour/run stop-8-evaluation/check.py --quick      # 30 seeds per panel, about 2 min
    tour/run stop-8-evaluation/check.py --cost-only  # the cost table alone, from a short timing sample
"""

from __future__ import annotations

import argparse
import json
import math
import os
import time
from dataclasses import dataclass
from pathlib import Path

import tourlib
from ppo.e2e.stats import (
    Z95,
    PairedTable,
    mcnemar_exact,
    newcombe_paired,
    paired_bootstrap,
    wilson,
)

ARM1 = "ironclad_e2e_d9000_ema0999"
ARM2 = "allchar_e2e_d4500_ema0999"

DEFAULT_PER_PANEL = 200
QUICK_PER_PANEL = 30
COST_SAMPLE = 20
SLICE_SIZE = 100
COST_SEEDS = (300, 1_000, 4_000)
COST_DISCORDANCE = (0.10, 0.20, 0.30)
#: The one-sided normal quantile for 80% power.
Z80 = 0.8416212335729143


# ---------------------------------------------------------------- statistics


def mde80(n: int, discordance: float) -> float:
    """Smallest true paired difference a two-sided 5% McNemar test detects with
    80% power on `n` paired seeds, by the normal approximation:
    (z_0.975 + z_0.80) * sqrt(d / n). Assumes `discordance` (d) does not move
    with the effect, which holds while the effect squared is small beside d."""
    return (Z95 + Z80) * math.sqrt(discordance / n)


def mde80_unpaired(n: int, p: float) -> float:
    """The same for two independent samples of `n` seeds each at rate `p`."""
    return (Z95 + Z80) * math.sqrt(2 * p * (1 - p) / n)


def newcombe_unpaired(k1: int, n1: int, k2: int, n2: int) -> tuple[float, float]:
    """Newcombe's hybrid score 95% interval of p1 - p2 for independent samples.
    It is `newcombe_paired` with the correlation phi set to 0."""
    p1, p2 = k1 / n1, k2 / n2
    l1, u1 = wilson(k1, n1)
    l2, u2 = wilson(k2, n2)
    d = p1 - p2
    return (d - math.hypot(p1 - l1, u2 - p2), d + math.hypot(u1 - p1, p2 - l2))


def unpaired_z_p(k1: int, n1: int, k2: int, n2: int) -> float:
    """Two-sided p of the pooled two-proportion z test (ignores the pairing)."""
    pool = (k1 + k2) / (n1 + n2)
    se = math.sqrt(pool * (1 - pool) * (1 / n1 + 1 / n2))
    if se == 0:
        return 1.0
    z = (k1 / n1 - k2 / n2) / se
    return math.erfc(abs(z) / math.sqrt(2))


def paired_se(t: PairedTable) -> float:
    """Standard error of the paired difference: sqrt((d - diff^2) / n)."""
    return math.sqrt(max(0.0, t.discordance - t.diff**2) / t.n)


def unpaired_se(t: PairedTable) -> float:
    """The standard error the same two rates would have on separate seeds."""
    return math.sqrt((t.p1 * (1 - t.p1) + t.p2 * (1 - t.p2)) / t.n)


def slice_rates(wins: list[bool], size: int) -> list[float]:
    """Win rate on each consecutive, disjoint slice of `size` seeds (a short tail is dropped)."""
    return [sum(wins[i:i + size]) / size for i in range(0, len(wins) - size + 1, size)]


def hours_for(n_paired: int, decisions_per_run: float, rate: float, arms: int = 2) -> float:
    """Wall-clock hours to play `n_paired` seeds with each of `arms` networks
    at `rate` decisions per second."""
    return n_paired * arms * decisions_per_run / rate / 3600


# ------------------------------------------------------------------ playing


@dataclass
class Arm:
    name: str
    wins: dict[str, list[bool]]  # panel name -> one result per seed, in seed order
    decisions: int
    errors: int
    runs: list[dict]  # one record per run: panel, seed, won, decisions, seconds


def _timed_selfplay(student, seed: int) -> tuple[dict, float]:
    import sts2_rs

    t0 = time.perf_counter()
    out = sts2_rs.e2e_selfplay(student, seed)
    return out, time.perf_counter() - t0


def play(name: str, panels: dict[str, range], workers: int) -> tuple[Arm, float]:
    """`sts2_rs.e2e_selfplay` (greedy, whole run) over each panel's seeds, timing every run."""
    from concurrent.futures import ThreadPoolExecutor

    import sts2_rs

    student = sts2_rs.E2eStudent(str(tourlib.weight(name + ".e2ew")))
    wins: dict[str, list[bool]] = {}
    records: list[dict] = []
    decisions = errors = 0
    t0 = time.perf_counter()
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for panel, seeds in panels.items():
            done = list(pool.map(lambda s: _timed_selfplay(student, s), seeds))
            wins[panel] = [bool(r["won"]) for r, _ in done]
            for seed, (r, secs) in zip(seeds, done, strict=True):
                records.append({"panel": panel, "seed": seed, "won": bool(r["won"]),
                                "outcome": r["outcome"], "decisions": r["decisions"],
                                "floor": r.get("floor"), "seconds": round(secs, 4)})
                decisions += r["decisions"]
                errors += r["outcome"] in ("error", "stopped")
    return Arm(name, wins, decisions, errors, records), time.perf_counter() - t0


# ------------------------------------------------------------------ printing


def pct(x: float) -> str:
    return f"{100 * x:5.1f}%"


def interval(lo: float, hi: float) -> str:
    return f"[{100 * lo:+.1f}, {100 * hi:+.1f}] pp"


def print_rates(arms: list[Arm]) -> None:
    print("\n1. Win rates, 95% Wilson interval")
    for arm in arms:
        print(f"  {arm.name}")
        pooled = [w for ws in arm.wins.values() for w in ws]
        for label, ws in [*arm.wins.items(), ("pooled", pooled)]:
            k, n = sum(ws), len(ws)
            lo, hi = wilson(k, n)
            print(f"    {label:<8} {k:>4}/{n:<4} {pct(k / n)}  [{pct(lo)}, {pct(hi)}]"
                  f"  half-width {100 * (hi - lo) / 2:.1f} pp")


def print_paired(t: PairedTable) -> None:
    print(f"\n2. Paired comparison, pooled over both panels ({t.n} seeds)")
    print(f"  net 1 = {ARM1}, net 2 = {ARM2}")
    print("                      net 2 wins   net 2 loses")
    print(f"  net 1 wins        a = {t.a:>6}    b = {t.b:>6}")
    print(f"  net 1 loses       c = {t.c:>6}    d = {t.d:>6}")
    print(f"  difference (net 1 - net 2): {100 * t.diff:+.1f} pp"
          f"   discordant seeds: {t.b + t.c} (b={t.b}, c={t.c}), share d = {t.discordance:.3f}")
    k1, k2 = t.a + t.b, t.a + t.c
    boot = paired_bootstrap([(True, False)] * t.b + [(False, True)] * t.c
                            + [(True, True)] * t.a + [(False, False)] * t.d)
    print("                   paired (same seeds)           unpaired (as if separate seeds)")
    print(f"  p-value          McNemar exact {mcnemar_exact(t.b, t.c):<12.3g}"
          f"   two-proportion z {unpaired_z_p(k1, t.n, k2, t.n):.3g}")
    print(f"  95% interval     {interval(*newcombe_paired(t.a, t.b, t.c, t.d)):<28}"
          f"   {interval(*newcombe_unpaired(k1, t.n, k2, t.n))}")
    print(f"  standard error   {100 * paired_se(t):.2f} pp{'':<21}   {100 * unpaired_se(t):.2f} pp")
    print(f"  paired bootstrap ({boot['resamples']:,} resamples): {interval(boot['lo'], boot['hi'])}")


def print_slices(arms: list[Arm]) -> dict[str, float]:
    size = min(SLICE_SIZE, min(len(ws) for ws in arms[0].wins.values()))
    print(f"\n3. Panel variance: each network on disjoint {size}-seed slices of each panel")
    spreads = {}
    for arm in arms:
        rates = [r for ws in arm.wins.values() for r in slice_rates(ws, size)]
        p = sum(rates) / len(rates)
        spreads[arm.name] = max(rates) - min(rates)
        print(f"  {arm.name:<28} slices of {size}: " + "  ".join(pct(r) for r in rates)
              + f"   spread {100 * spreads[arm.name]:.1f} pp"
              + f"   (binomial SD of one slice {100 * math.sqrt(p * (1 - p) / size):.1f} pp)")
    return spreads


COST_DECISIONS = (10_000_000, 100_000_000, 1_000_000_000)


def print_cost(decisions_per_run: float, rate: float, workers: int,
               discordances: list[float], p_typical: float) -> None:
    print(f"\n4. Cost on this Mac: {rate:,.0f} decisions/s on {workers} threads,"
          f" {decisions_per_run:,.0f} decisions per run, 2 networks per seed")
    for n in COST_DECISIONS:
        print(f"  {n:>13,} decisions: {n / rate / 3600:8.1f} hours")
    head = "  ".join(f"MDE80 d={d:.2f}" for d in discordances)
    print(f"  {'paired seeds':>12}  {'hours':>6}  {head}  unpaired MDE80 (p={p_typical:.2f})")
    for n in COST_SEEDS:
        mdes = "  ".join(f"{100 * mde80(n, d):>8.1f} pp" for d in discordances)
        print(f"  {n:>12,}  {hours_for(n, decisions_per_run, rate):>6.2f}  {mdes}"
              f"  {100 * mde80_unpaired(n, p_typical):>8.1f} pp")
    print("  MDE80: smallest true difference a two-sided 5% McNemar test finds 80% of the time"
          " (normal approximation; see chapter.html, section 4).")


# --------------------------------------------------------------------- main


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--quick", action="store_true", help=f"{QUICK_PER_PANEL} seeds per panel")
    ap.add_argument("--cost-only", action="store_true",
                    help=f"time {COST_SAMPLE} seeds per network and print only the cost table")
    ap.add_argument("--seeds", type=int, help="seeds per panel (overrides the default)")
    ap.add_argument("--workers", type=int, default=10, help="threads (default 10)")
    ap.add_argument("--save", type=Path, help="write every run's result and timing to this JSON file")
    args = ap.parse_args()

    if args.cost_only:
        n = args.seeds or COST_SAMPLE
        arms, secs = zip(*(play(name, {"A": tourlib.PANEL_A[:n]}, args.workers) for name in (ARM1, ARM2)))
        decisions = sum(a.decisions for a in arms)
        p = sum(sum(a.wins["A"]) for a in arms) / (2 * n)
        print(f"timing sample: {n} seeds of PANEL_A per network")
        print_cost(decisions / (2 * n), decisions / sum(secs), args.workers, list(COST_DISCORDANCE), p)
        return

    per_panel = args.seeds or (QUICK_PER_PANEL if args.quick else DEFAULT_PER_PANEL)
    panels = {"A": tourlib.PANEL_A[:per_panel], "B": tourlib.PANEL_B[:per_panel]}
    print(f"Playing {ARM1} and {ARM2} greedily on the same {2 * per_panel} seeds"
          f" ({per_panel} from each panel), {args.workers} threads.")

    load_before = os.getloadavg()
    played = [play(name, panels, args.workers) for name in (ARM1, ARM2)]
    arms = [a for a, _ in played]
    secs = sum(s for _, s in played)
    decisions = sum(a.decisions for a in arms)
    runs = 2 * 2 * per_panel
    print(f"Played {runs} runs, {decisions:,} decisions in {secs / 60:.1f} min"
          f" ({sum(a.errors for a in arms)} errored or capped, counted as losses).")

    print_rates(arms)
    t = PairedTable.from_pairs([
        (w1, w2) for p in panels for w1, w2 in zip(arms[0].wins[p], arms[1].wins[p], strict=True)
    ])
    print_paired(t)
    print_slices(arms)

    rate = decisions / secs
    print_cost(decisions / runs, rate, args.workers, sorted({*COST_DISCORDANCE, round(t.discordance, 3)}),
               (t.p1 + t.p2) / 2)

    if args.save:
        args.save.write_text(json.dumps({
            "arms": [ARM1, ARM2],
            "panels": {p: [r.start, r.stop] for p, r in panels.items()},
            "workers": args.workers,
            "wall_seconds": {a.name: round(s, 2) for a, s in played},
            "loadavg_before": load_before,
            "loadavg_after": os.getloadavg(),
            "runs": {a.name: a.runs for a in arms},
        }, indent=1) + "\n", encoding="utf-8")
        print(f"\nsaved {args.save}")


if __name__ == "__main__":
    main()
