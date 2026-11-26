"""Stop 3: reward and return.

Plays runs in the training environment (`ppo.e2e.rsenv.RsE2EEnv`, the Rust
port of `ppo.e2e.env.E2EEnv` that the rollout workers drive) and reads the
reward that environment assigns. Nothing here recomputes reward on the way:
the ledger is the env's own increments, split into floor, boss and win parts
only for display, and every run is checked against the env's `total_reward`.

    tour/run stop-3-reward/check.py            # 300 runs per policy
    tour/run stop-3-reward/check.py --quick    # 24 runs per policy
"""

from __future__ import annotations

import argparse
import random
import statistics
import sys
import time
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

import tourlib

STOP = Path(__file__).resolve().parent
NET = "ironclad_e2e_d9000_ema0999.pt"


# ── the reward rule, as the env's constants ─────────────────────────────────


def constants() -> tuple[float, float, float]:
    """`(R_WIN, R_BOSS, R_FLOOR)` from the training env (env.py:117-119)."""
    from ppo.e2e import env

    return env.R_WIN, env.R_BOSS, env.R_FLOOR


def return_from_outcome(floors: int, bosses: int, won: bool) -> float:
    """The return the reward rule gives a run that reached floor `floors`,
    beat `bosses` bosses before the last fight, and won or not.

    This is for scoring runs that were never played. `test_check.py` holds it
    equal to the env's own total on real runs."""
    r_win, r_boss, r_floor = constants()
    return r_floor * floors + r_boss * bosses + (r_win if won else 0.0)


# ── one run, with its reward ledger ─────────────────────────────────────────


@dataclass
class Entry:
    """One reward increment the env assigned, split by what triggered it."""

    row: int  # the stored row the env credited it to
    act: int
    floor: int
    amount: float
    floors: float  # the part paid for new floors
    boss: float  # the part paid for a boss beaten before the last fight
    win: float  # the part paid for winning
    running: float  # the return so far


@dataclass
class Run:
    seed: int
    outcome: str  # won | lost | capped | error
    won: bool
    floor: int
    bosses: int
    total_reward: float  # the env's own sum
    unassigned_reward: float
    decisions: int
    ledger: list[Entry] = field(default_factory=list)
    hp: int = 0  # at the end of the run: facts the reward never reads
    max_hp: int = 0
    relics: list[str] = field(default_factory=list)

    @property
    def ret(self) -> float:
        return sum(e.amount for e in self.ledger)

    def part(self, name: str) -> float:
        return sum(getattr(e, name) for e in self.ledger)


def split(amount: float, new_floors: int, terminal_won: bool) -> tuple[float, float, float]:
    """Split one increment into (floors, boss, win) parts.

    The env adds `R_FLOOR` per new floor, `R_BOSS` per boss won before the
    run's end and `R_WIN` on a win into one pending sum, then credits it at
    the next stored row or the end (env.py:1431-1446). The floor part is
    known from the floor counter; the rest must be a whole number of bosses
    plus the win. Raises when it is not, so the ledger never hides reward it
    cannot explain."""
    r_win, r_boss, r_floor = constants()
    floors = r_floor * new_floors
    rest = amount - floors
    win = r_win if terminal_won else 0.0
    rest -= win
    n_boss = round(rest / r_boss)
    if n_boss < 0 or abs(rest - n_boss * r_boss) > 1e-7:
        raise AssertionError(
            f"unexplained reward {amount!r}: {new_floors} new floors, won={terminal_won}"
        )
    return floors, n_boss * r_boss, win


def play(seed: int, choose) -> Run:
    """Play `seed` in the training env, choosing each decision's merged class
    with `choose(decision)`, and keep every reward increment."""
    from ppo.e2e.rsenv import RsDecision, RsE2EEnv

    env = RsE2EEnv(seed, detail=True)
    d = env.reset()
    ledger: list[Entry] = []
    max_floor = 0
    running = 0.0
    decisions = 0

    def credit(amount: float, row: int, act: int, floor: int, won: bool) -> None:
        nonlocal max_floor, running
        new = max(0, floor - max_floor)
        max_floor = max(max_floor, floor)
        if amount == 0.0:
            return
        f, b, w = split(amount, new, won)
        running += amount
        ledger.append(Entry(row, act, floor, amount, f, b, w, running))

    while isinstance(d, RsDecision):
        # `d.reward` is what the env just credited to the previous row.
        credit(d.reward, d.row - 1, int(d.act), int(d.floor), won=False)
        d = env.step(choose(d))
        decisions += 1
    t = d
    end = env.rm
    credit(t.reward, t.rows - 1, int(t.act), int(t.floor), won=bool(t.won))
    return Run(
        seed=seed,
        outcome=t.outcome,
        won=bool(t.won),
        floor=int(t.floor),
        bosses=int(t.bosses),
        total_reward=float(t.total_reward),
        unassigned_reward=float(t.unassigned_reward),
        decisions=decisions,
        ledger=ledger,
        hp=int(end.current_hp),
        max_hp=int(end.max_hp),
        relics=list(end.relics()),
    )


# ── the two policies ────────────────────────────────────────────────────────

_NET = None


def _select_build() -> None:
    from ppo.e2e import evaluate

    evaluate.select_campaign_build()


def net_chooser():
    """Greedy play with the published Ironclad checkpoint, on the env's own
    token rows (`StudentPolicy.act`, net.py)."""
    global _NET
    if _NET is None:
        import torch

        from ppo.e2e import net, tokens

        torch.set_num_threads(1)
        _NET = net.StudentPolicy.from_checkpoint(
            tourlib.weight(NET),
            encode_row=lambda d: tokens.row_batch([d.tok], run_uid=[0], t=[d.row]),
        )
    pol = _NET
    return lambda d: pol.act_decisions([d])[0]


def random_chooser(seed: int):
    """A uniform pick among the merged classes, as `evaluate.RandomOption`."""
    rng = random.Random(f"tour.stop-3:{seed}")
    return lambda d: rng.randrange(d.n_classes)


def play_net(seed: int) -> Run:
    _select_build()
    return play(seed, net_chooser())


def play_random(seed: int) -> Run:
    _select_build()
    return play(seed, random_chooser(seed))


def play_many(fn, seeds, workers: int) -> list[Run]:
    with ProcessPoolExecutor(max_workers=workers) as pool:
        return list(pool.map(fn, seeds, chunksize=2))


# ── checks and summaries ────────────────────────────────────────────────────


def verify(run: Run) -> None:
    """The ledger, the env's total and the rule applied to the run's outcome
    facts must all give the same number."""
    if run.outcome == "error":
        return
    rule = return_from_outcome(run.floor, run.bosses, run.won)
    got = (run.ret, run.total_reward + run.unassigned_reward, rule)
    if max(got) - min(got) > 1e-7:
        raise AssertionError(f"seed {run.seed}: ledger, env total, rule = {got}")


@dataclass
class Summary:
    n: int
    errors: int
    wins: int
    mean: float
    floors: float  # mean return from floors
    boss: float
    win: float


def summarize(runs: list[Run]) -> Summary:
    ok = [r for r in runs if r.outcome != "error"]
    n = len(ok)
    if n == 0:
        raise SystemExit("every run ended in an engine error")
    return Summary(
        n=n,
        errors=len(runs) - n,
        wins=sum(r.won for r in ok),
        mean=sum(r.ret for r in ok) / n,
        floors=sum(r.part("floors") for r in ok) / n,
        boss=sum(r.part("boss") for r in ok) / n,
        win=sum(r.part("win") for r in ok) / n,
    )


def print_ledger(run: Run) -> None:
    print(f"\nReward ledger: seed {run.seed}, network, greedy ({run.decisions} decisions)")
    print(f"{'row':>5} {'act':>3} {'floor':>5} {'reward':>8}  {'trigger':<28} {'return so far':>13}")
    for e in run.ledger:
        why = []
        if e.floors:
            why.append(f"{round(e.floors / constants()[2])} new floor(s)")
        if e.boss:
            why.append(f"{round(e.boss / constants()[1])} boss beaten")
        if e.win:
            why.append("run won")
        print(f"{e.row:>5} {e.act:>3} {e.floor:>5} {e.amount:>8.3f}  {', '.join(why):<28} {e.running:>13.3f}")
    print(f"outcome {run.outcome}, floor {run.floor}, bosses paid {run.bosses}")
    print(f"ledger sum {run.ret:.6f}   env total_reward {run.total_reward:.6f}"
          f"   unassigned {run.unassigned_reward:.6f}")


def print_summary(name: str, s: Summary) -> None:
    def share(x: float) -> str:
        return f"{x:.4f} ({x / s.mean:.0%})" if s.mean else f"{x:.4f}"

    print(f"\n{name}: {s.n} runs ({s.errors} engine errors left out), {s.wins} won")
    print(f"  mean return   {s.mean:.4f}")
    print(f"  from floors   {share(s.floors)}")
    print(f"  from bosses   {share(s.boss)}")
    print(f"  from the win  {share(s.win)}")


HYPOTHETICAL = {
    # key: (description, floors reached, bosses paid, won)
    "death_floor_20_return": ("dies on floor 20, after beating the act-1 boss", 20, 1, False),
    "death_floor_40_return": ("dies on floor 40, after beating the act-1 and act-2 bosses", 40, 2, False),
}


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--quick", action="store_true", help="24 runs per policy")
    ap.add_argument("--runs", type=int, default=None, help="runs per policy (default 300)")
    ap.add_argument("--workers", type=int, default=3)
    args = ap.parse_args(argv)
    n = args.runs or (24 if args.quick else 300)
    seeds = list(tourlib.PANEL_A[:n])
    t0 = time.time()

    _select_build()
    ledger_run = play_net(seeds[0])
    verify(ledger_run)
    print_ledger(ledger_run)

    net_runs = play_many(play_net, seeds, args.workers)
    rnd_runs = play_many(play_random, seeds, args.workers)
    for r in net_runs + rnd_runs:
        verify(r)
    net, rnd = summarize(net_runs), summarize(rnd_runs)
    print_summary("Network (greedy)", net)
    print_summary("Random play", rnd)

    won = [r.ret for r in net_runs if r.won]
    if not won:
        raise SystemExit("the network won no run: use more runs")
    print(f"\nWon runs: return from {min(won):.3f} to {max(won):.3f}, median {statistics.median(won):.3f}")
    lost = [r.ret for r in net_runs + rnd_runs if not r.won and r.outcome != "error"]
    if lost:
        print(f"Largest return among the losing runs played here: {max(lost):.3f}")
    # A won run with its win bonus taken away is exactly what a run that died in
    # the final fight would have earned: the same floors and the same bosses
    # (the final boss pays only the win, env.py:1596-1601).
    r_win = constants()[0]
    best_loss = max(w - r_win for w in won)

    print("\nHypothetical runs, scored by the same rule:")
    hypo = {}
    for key, (desc, floors, bosses, w) in HYPOTHETICAL.items():
        hypo[key] = return_from_outcome(floors, bosses, w)
        print(f"  {desc}: {hypo[key]:.3f}")
    print(f"  dies in the final boss fight (a won run minus the win): {best_loss:.3f}")
    print(f"\n{len(net_runs) + len(rnd_runs) + 1} runs in {time.time() - t0:.0f} s; "
          "every ledger matched the env's total and the rule.")


if __name__ == "__main__":
    sys.exit(main())
