"""Seed solver: is this seed winnable? Sample the e2e student until it wins.

    python -m ppo.e2e.solve solve SEED [SEED ...] --ckpt CKPT [--ckpt CKPT2] \\
        --samples 2000 --temps 1.0 --procs 16 --out DIR
    python -m ppo.e2e.solve verify DIR/SEED.json

The campaign's run: Ironclad, A10, v0.111.0, from Neow, act 1 rolled from the
seed (`factory.new_run`). SEED is a run seed in [0, 2**31) or, with letters in
it (or `--game-seed`), the seed string the game shows (`RunManager(string_seed=)`).

Each attempt `k` plays one whole run. Attempt 0 is greedy (with
`--greedy-first`, the default); every other attempt samples each decision from
the student's softmax at a temperature from `--temps`, on a torch stream seeded
by (seed, k), cycling through `--ckpt` and `--temps`. An attempt is therefore a
deterministic function of (seed, k, ckpt, temperature): it can be re-run.

The verdict, one JSON file per seed:

* `solved`: some attempt won. The PROOF is that attempt's action log: the
  index of the action taken at every decision (`E2EEnv.step_action`), with
  the run's terminal fingerprint. `verify` replays it on a fresh run with no
  net at all and checks it ends `won` on the same fingerprint, so the proof
  holds for anyone with the same engine build (`engine` in the file). The
  file also lists every step's action in readable form.
* `unsolved`: no win in N attempts. That is NOT a proof the seed is
  impossible, only evidence: the file gives the one-sided 95% upper bound on
  this sampler's per-attempt win chance, `1 - 0.05 ** (1 / N)` (about 3/N),
  and the floors the attempts reached. More samples, more nets and other
  temperatures narrow it.

Why sampling works (2026-11-08 replication, `experiments/snapshot-pool/`):
greedy snapshots of one lane disagree on which seeds they win, and 16 sampled
runs of ONE net at T=1.0 win 95.5% of DEV seeds between them, each run as
strong as greedy play (52.6% vs 52.5%).

The proof is a simulator proof. Whether the same line wins in the real game
depends on whole-run parity with the game (`run_parity.py`), and the real
game must be on the same build (v0.111.0) with the same character and
ascension.
"""

from __future__ import annotations

import argparse
import collections
import json
import multiprocessing as mp
import os
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import factory

SCHEMA = "e2e-solve/1"
ALPHA = 0.05


# ── seeds and runs ───────────────────────────────────────────────────────────


@dataclass(frozen=True)
class SeedSpec:
    """A run seed (`int`) or the game's seed string (`str`)."""

    value: int | str

    @property
    def label(self) -> str:
        return str(self.value)

    def to_json(self) -> dict[str, Any]:
        if isinstance(self.value, int):
            return {"seed": self.value}
        return {"game_seed": self.value}

    @classmethod
    def from_json(cls, d: dict[str, Any]) -> SeedSpec:
        return cls(int(d["seed"])) if "seed" in d else cls(str(d["game_seed"]))


def parse_seed(text: str, *, game_seed: bool = False) -> SeedSpec:
    """`123` is a run seed; anything with a non-digit, or `game_seed`, a seed string."""
    text = text.strip()
    if not text:
        raise ValueError("empty seed")
    if not game_seed and text.isdigit():
        n = int(text)
        if not 0 <= n < factory.SEED_LIMIT:
            raise ValueError(f"run seed {n} outside [0, 2**31)")
        return SeedSpec(n)
    return SeedSpec(text)


def make_run(spec: SeedSpec) -> Any:
    """A fresh campaign run for `spec`, checked by the factory."""
    if isinstance(spec.value, int):
        return factory.new_run(spec.value)
    import sts2_rs

    factory.require_selection()
    rm = sts2_rs.RunManager(
        string_seed=spec.value,
        character=factory.CHARACTER,
        ascension_level=factory.ASCENSION,
        neow_start=True,
        act1=factory.ACT1,
        game_version=factory.GAME_VERSION,
        mode=factory.MODE,
        interactive_potion_swap=True,
    )
    return factory.check_run(rm)


def make_env(spec: SeedSpec) -> Any:
    from .env import E2EEnv

    rm = make_run(spec)
    return E2EEnv(int(rm.seed), rm=rm)


# ── the sampler ──────────────────────────────────────────────────────────────


def attempt_plan(k: int, n_ckpts: int, temps: list[float], greedy_first: bool) -> tuple[int, float]:
    """(checkpoint index, temperature) of attempt `k`; temperature 0 is greedy."""
    if greedy_first:
        if k < n_ckpts:
            return k, 0.0
        j = k - n_ckpts
    else:
        j = k
    return j % n_ckpts, float(temps[(j // n_ckpts) % len(temps)])


def stream_seed(run_seed: int, k: int) -> int:
    """The torch generator seed of attempt `k` on run seed `run_seed`."""
    return (int(run_seed) * 1_000_003 + int(k) * 7_919 + 17) % (2**63 - 1)


class Sampler:
    """One checkpoint's student, sampling at temperature `T` (0 = greedy argmax)."""

    def __init__(self, ckpt: str) -> None:
        import torch

        from . import net as N

        self.N = N
        self.torch = torch
        self.student = N.make_student(ckpt)
        self.gen = torch.Generator()
        self.T = 0.0

    def begin(self, run_seed: int, k: int, T: float) -> None:
        self.student.begin_run(run_seed)
        self.gen.manual_seed(stream_seed(run_seed, k))
        self.T = float(T)

    def act(self, d: Any) -> int:
        torch, N = self.torch, self.N
        with torch.no_grad():
            b = N.to_batch(self.student.row(d), self.student.device)
            logits = self.student.net.policy(b, bf16=False)[: b.n_real].float().cpu()[0]
        if self.T <= 0:
            return int(logits.argmax())
        p = torch.softmax(logits / self.T, -1)
        return int(torch.multinomial(p, 1, generator=self.gen))


# ── one attempt ──────────────────────────────────────────────────────────────

_SAMPLERS: dict[str, Sampler] = {}


def _sampler(ckpt: str) -> Sampler:
    if ckpt not in _SAMPLERS:
        import torch

        torch.set_num_threads(1)
        _SAMPLERS[ckpt] = Sampler(ckpt)
    return _SAMPLERS[ckpt]


def play_attempt(spec: SeedSpec, k: int, ckpt: str, T: float) -> dict[str, Any]:
    """Play attempt `k`; the run's outcome and, for `won`, its action log."""
    from .env import E2EDecision

    t0 = time.time()
    s = _sampler(ckpt)
    try:
        env = make_env(spec)
        s.begin(env.seed, k, T)
        d = env.reset()
        while isinstance(d, E2EDecision) and not d.bootstrap:
            d = env.step(s.act(d))
        t = env.terminal
        out = {
            "k": k,
            "outcome": t.outcome,
            "won": bool(t.won),
            "act": int(t.act),
            "floor": int(t.floor),
            "rows": int(t.rows),
        }
        if t.won:
            out.update(log_idx=[int(i) for i in t.log_idx], fingerprint=t.fingerprint)
    except Exception as ex:  # one bad attempt must not stop the search
        out = {
            "k": k,
            "outcome": "error",
            "won": False,
            "act": 0,
            "floor": 0,
            "rows": 0,
            "error": f"{type(ex).__name__}: {ex}"[:300],
        }
    out.update(ckpt=ckpt, T=T, sec=round(time.time() - t0, 2))
    return out


def _task(a: tuple[SeedSpec, int, str, float]) -> dict[str, Any]:
    return play_attempt(*a)


# ── proofs ───────────────────────────────────────────────────────────────────


def _readable(d: Any, ai: int) -> dict[str, Any]:
    """One step for people: the engine action plus best-effort names (`label`)."""
    a = d.actions[ai]
    act = {k: v for k, v in a.items() if isinstance(v, (str, int, float, bool)) or v is None}
    out: dict[str, Any] = {"act": int(d.act), "floor": int(d.floor), "kind": d.kind, "action": act}
    b = d.d_bv if d.kind == "combat" else None
    if b:
        label = []
        hand = (b.get("deck") or {}).get("hand") or []
        ci = a.get("card_index")
        if isinstance(ci, int) and 0 <= ci < len(hand):
            c = hand[ci]
            label.append(f"{c.get('id')}{'+' if c.get('upgraded') else ''}")
        foes = [e for e in b.get("encounter") or [] if not e.get("is_dead")]
        ti = a.get("target_index")
        if isinstance(ti, int) and 0 <= ti < len(foes):
            label.append(f"-> {foes[ti].get('kind')}")
        if label:
            out["label"] = " ".join(label)
    return out


def replay(spec: SeedSpec, log_idx: list[int], *, readable: bool = False) -> tuple[Any, list[dict]]:
    """Replay `log_idx` on a fresh run; the terminal and (optionally) each step."""
    from .env import E2EDecision

    env = make_env(spec)
    d = env.reset()
    steps: list[dict] = []
    for n, ai in enumerate(log_idx):
        if not isinstance(d, E2EDecision) or d.bootstrap:
            raise AssertionError(f"the run ended at step {n} of {len(log_idx)}")
        if not 0 <= ai < len(d.actions):
            raise AssertionError(f"step {n}: action {ai} out of range 0..{len(d.actions) - 1}")
        if readable:
            steps.append(_readable(d, ai))
        d = env.step_action(int(ai))
    if isinstance(d, E2EDecision) and not d.bootstrap:
        raise AssertionError(f"the log ended after {len(log_idx)} steps, the run did not")
    return env.terminal, steps


def check_proof(proof: dict[str, Any]) -> tuple[bool, str]:
    """(ok, why) for a `solved` file's proof, replayed with no net."""
    spec = SeedSpec.from_json(proof["seed"])
    try:
        term, _ = replay(spec, [int(i) for i in proof["log_idx"]])
    except AssertionError as ex:
        return False, f"replay diverged: {ex}"
    if not term.won:
        return False, f"replay ended {term.outcome} on floor {term.floor}"
    if term.fingerprint != proof["fingerprint"]:
        return False, f"fingerprint {term.fingerprint} != {proof['fingerprint']}"
    return True, f"won on floor {term.floor}, fingerprint {term.fingerprint}"


def upper_bound(n: int, alpha: float = ALPHA) -> float:
    """One-sided (1 - alpha) upper bound on a win chance after 0 wins in `n` tries."""
    return 1.0 - alpha ** (1.0 / n) if n > 0 else 1.0


def engine_identity() -> dict[str, Any]:
    out: dict[str, Any] = {
        "game_version": factory.GAME_VERSION,
        "character": factory.CHARACTER,
        "ascension": factory.ASCENSION,
    }
    try:
        from ppo import wheel_guard

        out["wheel"] = wheel_guard.wheel_identity()
    except Exception as ex:  # identity is provenance, not a precondition
        out["wheel_error"] = str(ex)[:200]
    return out


# ── solve ────────────────────────────────────────────────────────────────────


def solve_seed(spec: SeedSpec, a: argparse.Namespace, pool: Any, log: Any) -> dict[str, Any]:
    plans = [
        (spec, k, a.ckpt[ci], T)
        for k in range(a.samples)
        for ci, T in [attempt_plan(k, len(a.ckpt), a.temps, a.greedy_first)]
    ]
    t0 = time.time()
    rows: list[dict[str, Any]] = []
    win = None
    it = pool.imap_unordered(_task, plans) if pool is not None else map(_task, plans)
    for r in it:
        rows.append(r)
        if r["won"] and (win is None or r["k"] < win["k"]):
            win = r
        if len(rows) % a.progress == 0:
            print(
                f"[{spec.label}] {len(rows)}/{a.samples} attempts, "
                f"{sum(x['won'] for x in rows)} won, best floor {max(x['floor'] for x in rows)}, "
                f"{time.time() - t0:.0f}s",
                file=log,
                flush=True,
            )
        if win is not None and not a.keep_going:
            break
    n = len(rows)
    wins = sum(r["won"] for r in rows)
    floors = collections.Counter(r["floor"] for r in rows)
    res: dict[str, Any] = {
        "schema": SCHEMA,
        **spec.to_json(),
        "engine": engine_identity(),
        "config": {
            "samples": a.samples,
            "ckpt": a.ckpt,
            "temps": a.temps,
            "greedy_first": a.greedy_first,
            "keep_going": a.keep_going,
        },
        "attempts": n,
        "wins": wins,
        "errors": sum(r["outcome"] == "error" for r in rows),
        "best_floor": max(floors) if floors else 0,
        "floors": {str(f): c for f, c in sorted(floors.items())},
        "seconds": round(time.time() - t0, 1),
    }
    if win is None:
        res["verdict"] = "unsolved"
        res["win_chance_upper_95"] = round(upper_bound(n), 6)
        res["note"] = (
            "no win in these attempts: evidence, not a proof of impossibility. "
            "The bound is for THIS sampler's per-attempt win chance."
        )
        return res
    proof = {
        "seed": spec.to_json(),
        "k": win["k"],
        "ckpt": win["ckpt"],
        "T": win["T"],
        "log_idx": win["log_idx"],
        "fingerprint": win["fingerprint"],
    }
    term, steps = replay(spec, win["log_idx"], readable=True)
    ok = bool(term.won and term.fingerprint == win["fingerprint"])
    res.update(
        verdict="solved" if ok else "replay_failed",
        proof=proof,
        proof_verified=ok,
        win_rate=round(wins / n, 4),
        steps=steps,
    )
    return res


def cmd_solve(a: argparse.Namespace) -> int:
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    specs = [parse_seed(s, game_seed=a.game_seed) for s in a.seeds]
    for c in a.ckpt:
        if not Path(c).is_file():
            raise SystemExit(f"--ckpt {c}: no such file")
    a.ckpt = [str(Path(c).resolve()) for c in a.ckpt]
    status = 0
    ctx = mp.get_context("spawn")
    pool = ctx.Pool(a.procs) if a.procs > 1 else None
    try:
        for spec in specs:
            res = solve_seed(spec, a, pool, sys.stderr)
            path = out / f"{spec.label}.json"
            path.write_text(json.dumps(res, indent=1) + "\n", encoding="utf-8")
            line = f"{spec.label}: {res['verdict']}, {res['attempts']} attempts finished"
            if res["verdict"] == "solved":
                line += (
                    f"; attempt {res['proof']['k']} won (T={res['proof']['T']}), proof replayed OK"
                )
            elif res["verdict"] == "unsolved":
                line += (
                    f", best floor {res['best_floor']}, per-attempt win chance "
                    f"< {res['win_chance_upper_95']:.4f} (95%)"
                )
            else:
                status = 1
            print(f"{line} -> {path}", flush=True)
            if pool is not None and res["verdict"] != "unsolved" and not a.keep_going:
                pool.terminate()  # drop the attempts still queued for this seed
                pool = ctx.Pool(a.procs)
    finally:
        if pool is not None:
            pool.terminate()
    return status


def cmd_verify(a: argparse.Namespace) -> int:
    res = json.loads(Path(a.file).read_text(encoding="utf-8"))
    if res.get("verdict") != "solved" or "proof" not in res:
        print(f"{a.file}: verdict {res.get('verdict')!r}, no proof to verify")
        return 2
    ok, why = check_proof(res["proof"])
    print(f"{a.file}: {'PROOF OK' if ok else 'PROOF FAILED'}: {why}")
    return 0 if ok else 1


def main(argv: list[str] | None = None) -> int:
    os.environ.setdefault("STS2_GAME_VERSION", factory.GAME_VERSION)
    import sts2_client

    if sts2_client.GAME_VERSION != factory.GAME_VERSION:
        sts2_client.set_game_version(factory.GAME_VERSION)
    ap = argparse.ArgumentParser(
        prog="python -m ppo.e2e.solve", description=(__doc__ or "").split("\n\n")[0]
    )
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("solve", help="search for a winning line on each seed")
    s.add_argument("seeds", nargs="+", help="run seed (int) or the game's seed string")
    s.add_argument("--ckpt", action="append", required=True, help="student checkpoint (repeatable)")
    s.add_argument("--samples", type=int, default=1000, help="attempts per seed (default 1000)")
    s.add_argument(
        "--temps",
        type=lambda t: [float(x) for x in t.split(",")],
        default=[1.0],
        help="comma-separated sampling temperatures, cycled (default 1.0)",
    )
    s.add_argument(
        "--no-greedy-first",
        dest="greedy_first",
        action="store_false",
        help="skip the greedy attempt(s) per checkpoint",
    )
    s.add_argument(
        "--keep-going",
        action="store_true",
        help="play all --samples even after a win (gives a win-rate estimate)",
    )
    s.add_argument("--game-seed", action="store_true", help="read every SEED as a game seed string")
    s.add_argument("--procs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    s.add_argument("--progress", type=int, default=50, help="log every N attempts")
    s.add_argument("--out", required=True, help="directory for <seed>.json verdicts")
    s.set_defaults(fn=cmd_solve)
    v = sub.add_parser("verify", help="replay a solved file's proof with no net")
    v.add_argument("file")
    v.set_defaults(fn=cmd_verify)
    a = ap.parse_args(argv)
    if a.cmd == "solve" and a.samples < 1:
        ap.error("--samples must be >= 1")
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
