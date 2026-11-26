"""Stop 5: the PPO update.

Runs the repository's own learning check (`python -m ppo.e2e.learner train
--mode learning_check`): a network from random weights, no teacher, trained
by PPO on its own play. Two arms run side by side:

- `clipped`: the learner's settings, scaled down to fit this Mac (see the README).
- `unclipped`: the same, with the ratio clip set out of reach, a learning
  rate 10 times larger and 4 epochs per batch. The dual clip (c = 3) and
  the gradient-norm clip stay on.

Each arm logs one row per iteration. This script prints them as tables and
scores your predictions.

    tour/run stop-5-ppo-update/check.py            # about 12 minutes
    tour/run stop-5-ppo-update/check.py --quick    # about 90 seconds, plumbing only
"""

from __future__ import annotations

import argparse
import json
import math
import os
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import tourlib

STOP = Path(__file__).resolve().parent

#: A smaller network than plan §5's 6 x 256, so the update runs on a CPU.
#: Same architecture (`ppo.e2e.net.NetConfig`), fewer and narrower layers.
SMALL_NET = {
    "dim": 128,
    "n_layers": 2,
    "n_heads": 4,
    "ffn": 352,
    "num_hidden": 256,
    "head_hidden": 128,
}

#: Moves the learning-rate anneal (schedules.anneal, from Sun Nov 8 2026)
#: past today, as it was when the author ran the check before the campaign.
NO_ANNEAL = 1_900_000_000


@dataclass(frozen=True)
class Size:
    wall_s: int  # the learner's --max-wall-s: it starts no iteration after this
    iter_decisions: int
    minibatch: int
    lc_window: int
    workers: int
    threads: int
    value_only_iters: int
    lr_warmup_iters: int


DEFAULT = Size(
    wall_s=690,
    iter_decisions=8192,
    minibatch=1024,
    lc_window=1000,
    workers=4,
    threads=2,
    value_only_iters=1,
    lr_warmup_iters=3,
)
QUICK = Size(
    wall_s=75,
    iter_decisions=2048,
    minibatch=512,
    lc_window=100,
    workers=3,
    threads=2,
    value_only_iters=1,
    lr_warmup_iters=1,
)

#: What each arm changes. `lr_scale` multiplies both base learning rates.
ARMS: dict[str, dict] = {
    "clipped": {"clip": 0.2, "epochs": 2, "lr_scale": 1.0},
    "unclipped": {"clip": 1e6, "epochs": 4, "lr_scale": 10.0},
    # Not run by default: the unclipped arm's step with the clip back on.
    "clipped_fast": {"clip": 0.2, "epochs": 4, "lr_scale": 10.0},
}
DEFAULT_ARMS = ["clipped", "unclipped"]


def learner_argv(arm: str, size: Size, save_dir: Path, seed: int) -> list[str]:
    """The command line of one arm: `python -m ppo.e2e.learner train ...`."""
    from ppo.e2e import schedules as S

    a = ARMS[arm]
    d = S.DEFAULT
    sched = {
        "anneal_start": NO_ANNEAL,
        "kl_freeze_at": NO_ANNEAL,
        "value_only_iters": size.value_only_iters,
        "lr_warmup_iters": size.lr_warmup_iters,
        "lr_trunk": d.lr_trunk * a["lr_scale"],
        "lr_value": d.lr_value * a["lr_scale"],
    }
    flags = {
        "save-dir": save_dir,
        "updater": "local",
        "mode": "learning_check",
        "seed": seed,
        # This Mac: no CUDA, and MPS fails in the sampler (torch.multinomial)
        # and in the token densifier, so serving and the update run on the CPU.
        "infer-device": "cpu",
        "update-devices": "cpu",
        "bf16": "false",
        "compile-serve": "false",
        "rank-threads": size.threads,
        "workers": size.workers,
        "n-envs": 32,
        "rust-env": "true",
        "filled": json.dumps(["WP-ENG-1"]),
        "net": json.dumps(SMALL_NET),
        "iter-decisions": size.iter_decisions,
        "minibatch": size.minibatch,
        "strata-per-mb": 8,
        "probe-strata": 8,
        "fwd-chunk": 1024,
        "epochs": a["epochs"],
        "clip": a["clip"],
        "lc-window": size.lc_window,
        "max-wall-s": size.wall_s,
        "eval-cmd": "",
        "action-logs": "false",
        "sched": json.dumps(sched),
    }
    argv = [sys.executable, "-m", "ppo.e2e.learner", "train"]
    for k, v in flags.items():
        argv += [f"--{k}", str(v)]
    return argv


# ── reading the learner's stats ─────────────────────────────────────────────


@dataclass(frozen=True)
class Row:
    """One iteration of one arm, from the learner's `stats.jsonl`."""

    it: int
    phase: str  # V-warm (value only), then U0: PPO on the policy
    decisions: int
    floor: float  # mean floor of the runs that ended in this iteration's batch
    kl: float  # exact KL(old ‖ new) on the probe rows after the last epoch
    clipfrac: float  # share of policy rows whose ratio left [1 - clip, 1 + clip]
    entropy: float  # policy entropy / log(number of choices), in [0, 1]
    ev: float  # explained variance of the critic on combat rows
    dps: float  # decisions per second of this iteration's collection
    time: float


def _num(x: object) -> float:
    return float(x) if isinstance(x, (int, float)) and math.isfinite(x) else math.nan


def parse_row(r: dict) -> Row:
    u = r["update"]
    return Row(
        it=int(r["it"]),
        phase=str(u.get("phase", "")),
        decisions=int(r["decisions"]),
        floor=_num(r.get("floor_mean")),
        kl=_num(u.get("probe_kl")),
        clipfrac=_num(u.get("clipfrac")),
        entropy=_num(u.get("ent_n")),
        ev=_num(u.get("ev_combat")),
        dps=_num(r.get("dec_per_s")),
        time=_num(r.get("time")),
    )


def read_rows(save_dir: Path) -> list[Row]:
    p = save_dir / "stats.jsonl"
    if not p.exists():
        return []
    return [parse_row(json.loads(x)) for x in p.read_text().splitlines() if x.strip()]


def policy_rows(rows: list[Row]) -> list[Row]:
    """The iterations that trained the policy (V-warm trains the value only)."""
    return [r for r in rows if r.phase != "V-warm"]


def _median(xs: list[float]) -> float:
    xs = [x for x in xs if math.isfinite(x)]
    return statistics.median(xs) if xs else math.nan


@dataclass(frozen=True)
class Summary:
    floor_start: float
    floor_end: float
    complete: bool
    kl_median: float
    clipfrac_median: float
    entropy_change: float
    dps: float


def summarize(rows: list[Row], verdict: dict, t_start: float) -> Summary:
    """Per-arm numbers the predictions are scored on.

    `floor_start` and `floor_end` are the learner's own learning-check windows
    (the first and the last `lc_window` finished runs). `entropy_change` is the
    mean entropy of the last three policy iterations minus that of the first.
    `dps` counts every decision the arm stored over its whole wall time,
    start-up included."""
    pol = policy_rows(rows)
    ent = [r.entropy for r in pol if math.isfinite(r.entropy)]
    change = (statistics.fmean(ent[-3:]) - ent[0]) if len(ent) >= 2 else math.nan
    last = rows[-1] if rows else None
    dps = last.decisions / (last.time - t_start) if last else math.nan
    return Summary(
        floor_start=_num(verdict.get("first_mean")),
        floor_end=_num(verdict.get("last_mean")),
        complete=bool(verdict.get("complete")),
        kl_median=_median([r.kl for r in pol]),
        clipfrac_median=_median([r.clipfrac for r in pol]),
        entropy_change=change,
        dps=dps,
    )


def table(rows: list[Row]) -> str:
    head = f"{'it':>3} {'phase':<6} {'decisions':>9} {'floor':>6} {'KL':>9} {'clip%':>6} {'entropy':>7} {'EV':>6} {'dec/s':>6}"
    out = [head]
    for r in rows:
        out.append(
            f"{r.it:>3} {r.phase:<6} {r.decisions:>9,} {r.floor:>6.2f} {r.kl:>9.2e} "
            f"{100 * r.clipfrac:>6.1f} {r.entropy:>7.4f} {r.ev:>6.2f} {r.dps:>6.0f}"
        )
    return "\n".join(out)


# ── running the arms ────────────────────────────────────────────────────────


def run_arms(size: Size, root: Path, seed: int, arms: list[str]) -> dict[str, tuple[Path, float]]:
    """Start every arm's learner at once and wait for all of them."""
    env = dict(os.environ, STS2_GAME_VERSION="v0.111.0", OMP_NUM_THREADS="1")
    procs = {}
    for arm in arms:
        sd = root / arm
        log = open(root / f"{arm}.log", "w")
        t0 = time.time()
        p = subprocess.Popen(
            learner_argv(arm, size, sd, seed), env=env, stdout=log, stderr=subprocess.STDOUT
        )
        procs[arm] = (p, sd, t0, log)
    for arm, (p, sd, _t0, log) in procs.items():
        rc = p.wait()
        log.close()
        if rc != 0:
            tail = (root / f"{arm}.log").read_text().splitlines()[-15:]
            # A non-finite update stops the learner (ops contract, exit 86): that is a
            # result for the unclipped arm, so keep its rows. Anything else is a failure.
            if not read_rows(sd):
                raise SystemExit(f"the {arm} learner exited {rc}:\n" + "\n".join(tail))
            print(f"the {arm} learner exited {rc}; its last lines:\n" + "\n".join(tail[-3:]))
    return {arm: (sd, t0) for arm, (_p, sd, t0, _l) in procs.items()}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--quick", action="store_true", help="about two minutes; checks the plumbing")
    ap.add_argument("--seed", type=int, default=0, help="the learners' seed")
    ap.add_argument("--workers", type=int, help="rollout workers per arm")
    ap.add_argument("--keep", type=Path, help="keep the learners' save directories here")
    ap.add_argument(
        "--arms", default=",".join(DEFAULT_ARMS), help=f"comma-separated, from {sorted(ARMS)}"
    )
    args = ap.parse_args()

    tourlib.load_predictions(STOP)  # refuse before any slow work

    size = QUICK if args.quick else DEFAULT
    if args.workers:
        size = Size(**{**size.__dict__, "workers": args.workers})
    arms = [a for a in args.arms.split(",") if a]
    unknown = sorted(set(arms) - set(ARMS))
    if unknown:
        raise SystemExit(f"unknown arms {unknown}; choose from {sorted(ARMS)}")
    if args.keep:
        args.keep.mkdir(parents=True, exist_ok=False)
        root = args.keep
    else:
        root = Path(tempfile.mkdtemp(prefix="tour-stop5-"))
    print(
        f"Training {len(arms)} arms side by side for about {size.wall_s // 60 + 1} minutes"
        f" ({size.workers} workers and {size.threads} update threads each).\n"
        f"Logs: {root}",
        flush=True,
    )
    t = time.time()
    dirs = run_arms(size, root, args.seed, arms)
    print(f"Done in {time.time() - t:.0f} s.\n")

    sums: dict[str, Summary] = {}
    for arm, (sd, t0) in dirs.items():
        rows = read_rows(sd)
        vp = sd / "learning_check.json"
        verdict = json.loads(vp.read_text()) if vp.exists() else {}
        s = summarize(rows, verdict, t0)
        sums[arm] = s
        print(f"── {arm}: {ARMS[arm]}")
        print(table(rows))
        print(
            f"learning check: first {verdict.get('first_runs', 0)} runs' mean floor"
            f" {s.floor_start:.2f}, last {verdict.get('last_runs', 0)} runs' {s.floor_end:.2f}"
            f" (windows {'complete' if s.complete else 'NOT complete: too few runs'});"
            f" verdict pass={verdict.get('pass')}\n"
        )

    if not {"clipped", "unclipped"} <= set(sums):
        return  # the predictions compare the two default arms
    c, u = sums["clipped"], sums["unclipped"]
    tourlib.score(
        STOP,
        {
            "floor_start": c.floor_start,
            "floor_end": c.floor_end,
            "kl_per_update": c.kl_median,
            "clip_fraction": c.clipfrac_median,
            "entropy_change": c.entropy_change,
            "decisions_per_second": c.dps,
            "unclipped_kl_per_update": u.kl_median,
        },
    )


if __name__ == "__main__":
    main()
