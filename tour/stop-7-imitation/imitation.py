"""Behaviour cloning of the published Ironclad net, and one DAgger round.

Every game here goes through `play`: one policy (the actor) chooses each
move, and the loop keeps each decision's tokenized row. Which policy acts
decides whose states the rows are:

* teacher rows: the teacher acts (the training data and the held-out states);
* student rows: a student acts (its own states, and the DAgger data).

Labels and agreement come afterwards, from a batched forward pass of a
network over stored rows (`choices`): the teacher's choice on a student
row is the DAgger label, and agreement is the share of rows where two
networks pick the same class.

The loop is `sts2_rs.E2eDriver` peek / apply, the loop `sts2_rs.e2e_selfplay`
runs inside Rust (`sts2-rs/crates/sts2-e2e/src/env.rs`,
`E2eEnv::run_to_decision`); with the teacher acting it plays the same moves
as `e2e_selfplay` on the same seed.
"""

from __future__ import annotations

import json
import multiprocessing as mp
import time
from dataclasses import dataclass
from pathlib import Path

import numpy as np

import tourlib

TEACHER = "ironclad_e2e_d9000_ema0999"
#: The same cap `sts2_rs.e2e_selfplay` uses.
MAX_DECISIONS = 6000


# ── playing ───────────────────────────────────────────────────────────────

_NETS: dict[str, object] = {}


def _net(path: str):
    """One `sts2_rs.E2eStudent` per weights file per process."""
    import sts2_rs

    if path not in _NETS:
        _NETS[path] = sts2_rs.E2eStudent(path)
    return _NETS[path]


@dataclass
class Game:
    seed: int
    won: bool
    outcome: str
    floor: int
    decisions: int
    #: The tokenized row of each decision, as a row batch (`run_uid` = seed).
    rows: dict | None
    #: The class the actor chose at each decision.
    chosen: np.ndarray


def play(job: tuple[int, str]) -> Game:
    """Play seed `job[0]` to the end with the net at `job[1]` choosing every
    move greedily, keeping each decision's row."""
    import sts2_rs
    from ppo.e2e import rsenv, tokens

    seed, path = job
    actor = _net(path)
    rm = sts2_rs.e2e_new_run(seed)
    drv = sts2_rs.E2eDriver(seed)
    toks, chosen = [], []
    while True:
        peek = drv.peek(rm)
        if peek.kind == "terminal":
            t = peek.terminal
            break
        if peek.kind == "forced":
            f = peek.forced
            drv.apply(rm, f["action"], f["kind"])
            continue
        if len(toks) >= MAX_DECISIONS:
            t = {"won": False, "outcome": "stopped", "floor": 0}
            break
        toks.append(rsenv.tok_of(actor.encode(drv, peek, rm)))
        chosen.append(actor.decide(drv, peek, rm)["class"])
        a = peek.class_action(chosen[-1])
        drv.apply(rm, a["action"], a["kind"])
    rows = tokens.row_batch(toks, run_uid=[seed] * len(toks)) if toks else None
    return Game(seed, bool(t["won"]), str(t["outcome"]), int(t["floor"] or 0), len(toks),
                rows, np.asarray(chosen, np.int64))


def play_many(seeds: list[int], path: Path, workers: int) -> list[Game]:
    """`play` each seed in a pool of processes (the binding's `decide` holds
    the GIL, so threads would not help). Results come back in seed order."""
    jobs = [(int(s), str(path)) for s in seeds]
    if workers <= 1:
        return [play(j) for j in jobs]
    with mp.get_context("spawn").Pool(workers) as pool:
        return pool.map(play, jobs, chunksize=1)


# ── datasets ──────────────────────────────────────────────────────────────


def gather(games: list[Game]) -> tuple[dict, np.ndarray]:
    """The rows of `games` as one row batch, and the class the actor chose at each."""
    from ppo.e2e.net import concat_rows

    kept = [g for g in games if g.rows is not None]
    return concat_rows([g.rows for g in kept]), np.concatenate([g.chosen for g in kept])


def save_rows(path: Path, rb: dict, y: np.ndarray) -> None:
    np.savez(path, labels=y, **rb)


def load_rows(path: Path) -> tuple[dict, np.ndarray]:
    with np.load(path) as z:
        y = z["labels"]
        rb = {k: z[k] for k in z.files if k != "labels"}
    return rb, y


def runs(rb: dict, y: np.ndarray, seeds: list[int]) -> tuple[dict, np.ndarray]:
    """The rows of the runs whose seed is in `seeds` (each row's `run_uid` is
    its seed). Nested seed lists give nested datasets."""
    from ppo.e2e.net import select_rows

    idx = np.flatnonzero(np.isin(rb["run_uid"], np.asarray(seeds)))
    return select_rows(rb, idx), y[idx]


def agreement(a: np.ndarray, b: np.ndarray) -> float:
    """Percent of decisions where two policies chose the same class."""
    return 100.0 * float(np.mean(a == b)) if len(a) else float("nan")


# ── networks in torch ─────────────────────────────────────────────────────


def teacher_paths() -> tuple[Path, Path]:
    """The teacher's checkpoint (`.pt`, for torch) and its runnable export (`.e2ew`)."""
    return tourlib.weight(TEACHER + ".pt"), tourlib.weight(TEACHER + ".e2ew")


def device() -> str:
    import torch

    return "mps" if torch.backends.mps.is_available() else "cpu"


def _to(b, dev):
    """`TokenBatch.to` with blocking copies: its non-blocking host-to-MPS copy
    of a batch that is freed right after can read garbage (NaN logits)."""
    import dataclasses

    kw = {f.name: getattr(b, f.name) for f in dataclasses.fields(b)}
    return type(b)(**{k: (v if k == "n_real" else v.to(dev)) for k, v in kw.items()})


def choices(pt: Path, rb: dict, batch: int = 512) -> np.ndarray:
    """The greedy class the checkpoint `pt` picks on each row of `rb`."""
    import torch

    from ppo.e2e import net as N

    dev = device()
    net = N.load_checkpoint(pt)[0].to(dev).eval()
    n = len(rb["n_tok"])
    out = []
    with torch.no_grad():
        for i in range(0, n, batch):
            b = _to(N.to_batch(N.select_rows(rb, np.arange(i, min(n, i + batch))), "cpu"), dev)
            out.append(net.policy(b).argmax(-1).cpu().numpy())
    return np.concatenate(out) if out else np.zeros(0, np.int64)


def fresh_student(seed: int = 0):
    """A new `E2ENet` with the teacher's exact `NetConfig` (6M parameters:
    dim 256, 4 transformer layers) and random weights. It copies one thing from
    the teacher: the `prior` buffer, a fixed table of card features that is an
    input, not a learned weight."""
    from ppo.e2e import net as N

    tnet, tcfg = N.load_checkpoint(teacher_paths()[0])
    s = N.E2ENet(N.NetConfig(**tcfg["net"]), seed=seed, filled=tcfg["stamps"]["filled"])
    s.set_prior(tnet.prior, tnet.prior_sha256)
    return s, tcfg


def train(rb: dict, y: np.ndarray, out: Path, *, epochs: float, batch: int = 256,
          lr: float = 3e-4, seed: int = 0, soft: bool = False, log=print) -> None:
    """Behaviour cloning: minimise the cross-entropy between the student's
    policy and the teacher's chosen class (hard labels), or with `soft` the
    teacher's whole distribution over classes (distillation, the teacher's
    logits computed per batch), over `epochs` passes of `rb`.
    Writes `out.pt` (checkpoint, for torch) and `out.e2ew` (what Rust plays)."""
    import torch

    from ppo.e2e import net as N
    from ppo.e2e.export_wb import export_weights

    dev = device()
    net, tcfg = fresh_student(seed)
    net.to(dev).train()
    teacher = N.load_checkpoint(teacher_paths()[0])[0].to(dev).eval() if soft else None
    opt = torch.optim.AdamW(net.parameters(), lr=lr, weight_decay=0.01)
    n = len(y)
    steps = max(1, round(epochs * n / batch))
    warm = min(100, steps // 10 + 1)
    sched = torch.optim.lr_scheduler.LambdaLR(
        opt, lambda i: min(1.0, (i + 1) / warm) * 0.5 * (1 + np.cos(np.pi * min(i, steps) / steps))
    )
    rng = np.random.default_rng(seed)
    order, at = rng.permutation(n), 0
    t0, losses = time.time(), []
    for step in range(steps):
        if at + batch > n:
            order, at = rng.permutation(n), 0
        idx = order[at:at + batch]
        at += batch
        b = _to(N.to_batch(N.select_rows(rb, idx), "cpu"), dev)
        logits = net.policy(b)
        if teacher is None:
            loss = torch.nn.functional.cross_entropy(logits, torch.as_tensor(y[idx], device=dev))
        else:
            loss = soft_cross_entropy(logits, teacher_logits(teacher, b))
        opt.zero_grad(set_to_none=True)
        loss.backward()
        torch.nn.utils.clip_grad_norm_(net.parameters(), 1.0)
        opt.step()
        sched.step()
        losses.append(loss.item())
        if (step + 1) % 200 == 0 or step + 1 == steps:
            log(f"    step {step + 1}/{steps}  loss {np.mean(losses[-200:]):.3f}  "
                f"{time.time() - t0:.0f}s")
    net.cpu().eval()
    N.save_checkpoint(out.with_suffix(".pt"), net,
                      stamps=dict(tcfg["stamps"], variant="tour-bc-student"))
    export_weights(str(out.with_suffix(".pt")), str(out.with_suffix(".e2ew")))


def teacher_logits(teacher, b):
    import torch

    with torch.no_grad():
        return teacher.policy(b)


def soft_cross_entropy(student_logits, teacher_logits):
    """Mean over rows of H(p_T, p_S) = -sum_a p_T(a) log p_S(a). Classes off a
    row are -inf in both; their p_T is 0, so they are dropped, not 0 * -inf."""
    import torch

    p_t = torch.softmax(teacher_logits.float(), -1)
    logp_s = torch.log_softmax(student_logits.float(), -1)
    return -(p_t * torch.where(p_t > 0, logp_s, torch.zeros_like(logp_s))).sum(-1).mean()


# ── the plan of one preparation ───────────────────────────────────────────


@dataclass(frozen=True)
class Plan:
    """Which seeds play which role, and how long the students train."""

    name: str
    #: Teacher runs per dataset; each is a prefix of the next (nested datasets).
    sizes: tuple[int, ...]
    #: `PANEL_B` runs whose teacher states nobody trains on (held-out agreement).
    heldout: int
    #: `PANEL_B` runs the student plays in the DAgger round.
    dagger: int
    #: `PANEL_A` runs per policy for win rate, mean floor and own-state agreement.
    evaluate: int
    epochs: float

    def train_seeds(self, n: int) -> list[int]:
        return list(tourlib.PANEL_B[:n])

    def heldout_seeds(self) -> list[int]:
        return list(tourlib.PANEL_B[900:900 + self.heldout])

    def dagger_seeds(self) -> list[int]:
        return list(tourlib.PANEL_B[500:500 + self.dagger])

    def eval_seeds(self) -> list[int]:
        return list(tourlib.PANEL_A[:self.evaluate])

    def names(self) -> list[str]:
        """The students, smallest dataset first, DAgger last."""
        return [f"bc{n:03d}" for n in self.sizes] + ["dagger"]


FULL = Plan("full", sizes=(25, 50, 100), heldout=10, dagger=50, evaluate=150, epochs=6)
QUICK = Plan("quick", sizes=(2, 4), heldout=1, dagger=3, evaluate=4, epochs=2)


def prepare(plan: Plan, out: Path, workers: int, log=print) -> None:
    """Teacher data, held-out teacher states, one student per dataset size,
    the DAgger round, and the teacher's own score. Each step is skipped when
    its output exists, so a stopped run resumes."""
    out.mkdir(parents=True, exist_ok=True)
    teacher_pt, teacher = teacher_paths()
    names = plan.names()
    t0 = time.time()

    def teacher_rows(path: Path, seeds: list[int], what: str) -> None:
        if path.exists():
            return
        log(f"teacher plays {len(seeds)} {what} PANEL_B runs ({workers} processes)")
        games = play_many(seeds, teacher, workers)
        save_rows(path, *gather(games))
        log(f"  {sum(g.decisions for g in games)} decisions, {time.time() - t0:.0f}s")

    teacher_rows(out / "teacher_rows.npz", plan.train_seeds(max(plan.sizes)), "training")
    teacher_rows(out / "heldout_rows.npz", plan.heldout_seeds(), "held-out")
    rb, y = load_rows(out / "teacher_rows.npz")

    for n, name in zip(plan.sizes, names):
        if not (out / f"{name}.e2ew").exists():
            sub_rb, sub_y = runs(rb, y, plan.train_seeds(n))
            log(f"train {name} on {len(sub_y)} teacher decisions from {n} runs ({device()})")
            train(sub_rb, sub_y, out / name, epochs=plan.epochs, log=log)

    big = names[-2]
    ddata = out / "dagger_rows.npz"
    if not ddata.exists():
        log(f"DAgger: {big} plays {plan.dagger} PANEL_B runs; the teacher labels its states")
        drb, _ = gather(play_many(plan.dagger_seeds(), out / f"{big}.e2ew", workers))
        save_rows(ddata, drb, choices(teacher_pt, drb))
        log(f"  {len(drb['n_tok'])} decisions, {time.time() - t0:.0f}s")
    if not (out / "dagger.e2ew").exists():
        from ppo.e2e.net import concat_rows

        drb, dy = load_rows(ddata)
        sub_rb, sub_y = runs(rb, y, plan.train_seeds(max(plan.sizes)))
        log(f"train dagger on {len(sub_y)} teacher + {len(dy)} DAgger decisions")
        train(concat_rows([sub_rb, drb]), np.concatenate([sub_y, dy]), out / "dagger",
              epochs=plan.epochs, log=log)

    ref = out / "teacher_eval.json"
    if not ref.exists():
        import sts2_rs

        log(f"teacher's own score on {plan.evaluate} PANEL_A runs")
        res = tourlib.selfplay_many(sts2_rs.E2eStudent(str(teacher)), plan.eval_seeds(), workers)
        ref.write_text(json.dumps({
            "win_rate": 100.0 * float(np.mean([r["won"] for r in res])),
            "mean_floor": float(np.mean([r["floor"] or 0 for r in res])),
            "decisions_per_run": float(np.mean([r["decisions"] for r in res])),
        }))
    log(f"prepared {out} in {time.time() - t0:.0f}s")


# ── measuring ─────────────────────────────────────────────────────────────


def measure(plan: Plan, out: Path, workers: int, log=print) -> dict[str, dict]:
    """Per student: held-out agreement, own-state agreement, win rate (percent)
    and mean floor. `heldout_early_agreement` is held-out agreement on only
    the first K decisions of each teacher run, K the student's mean decisions
    per run: teacher states from the same stage of a run the student reaches."""
    paths = {k: out / f"{k}.pt" for k in plan.names()}
    need = [out / "heldout_rows.npz", *paths.values(), *(p.with_suffix(".e2ew") for p in paths.values())]
    missing = [str(p) for p in need if not p.exists()]
    if missing:
        raise SystemExit("not prepared: run stop-7-imitation/prepare.py first. Missing:\n  "
                         + "\n  ".join(missing))
    teacher_pt = teacher_paths()[0]
    t0 = time.time()
    hrb, hy = load_rows(out / "heldout_rows.npz")
    res: dict[str, dict] = {}
    for k, pt in paths.items():
        log(f"{k}: held-out agreement on {len(hy)} teacher decisions; "
            f"plays {plan.evaluate} PANEL_A runs")
        games = play_many(plan.eval_seeds(), pt.with_suffix(".e2ew"), workers)
        srb, chosen = gather(games)
        held = choices(pt, hrb)
        early = hrb["t"] < len(chosen) / len(games)
        res[k] = {
            "heldout_agreement": agreement(held, hy),
            "heldout_early_agreement": agreement(held[early], hy[early]),
            "own_agreement": agreement(choices(teacher_pt, srb), chosen),
            "win_rate": 100.0 * float(np.mean([g.won for g in games])),
            "mean_floor": float(np.mean([g.floor for g in games])),
            "decisions_per_run": len(chosen) / len(games),
        }
        log(f"  {time.time() - t0:.0f}s")
    return res


def quantities(plan: Plan, res: dict[str, dict]) -> dict[str, float]:
    """The six predicted quantities from `measure`'s results. "The student" is
    the one trained on the largest teacher dataset."""
    *sizes, dagger = plan.names()
    s, half = res[sizes[-1]], res[sizes[-2]]
    return {
        "heldout_agreement": s["heldout_agreement"],
        "own_state_agreement": s["own_agreement"],
        "student_mean_floor": s["mean_floor"],
        "student_win_rate": s["win_rate"],
        "agreement_gain_from_doubling": s["heldout_agreement"] - half["heldout_agreement"],
        "dagger_floor_change": res[dagger]["mean_floor"] - s["mean_floor"],
    }
