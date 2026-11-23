"""The per-run character and the ramped character mix (GH #1197, `ppo.e2e.mix`).

A mix lane plays every character from one process: each new run draws its
character from the run seed and the learner's decision clock (`mix.draw_character`),
Ironclad only until `settle_end`, then a linear ramp of the non-Ironclad share to
`target_new` at `ramp_end`. The factory builds and checks a run of any lane
character; the envs, practice starts and end records carry the run's own.
"""

from __future__ import annotations

import collections
import dataclasses
import hashlib
import json
import os
import random
import signal
from pathlib import Path

import numpy as np
import pytest
import sts2_client
import test_learner as TL  # make_cfg, make_learner: the fake source and teachers
from ppo.e2e import actors, factory, mix
from ppo.e2e import algo as A
from ppo.e2e import env as E
from ppo.e2e import learner as L
from ppo.e2e import practice as P
from ppo.e2e.rsenv_lockstep import Lockstep, run_seeds

SEED = 1_300_000_700  # the throwaway block (plan §3.1)
LANE = ("Ironclad", "Silent", "Defect", "Regent", "Necrobinder")
MIX = {
    "characters": list(LANE),
    "settle_end": 5_000_000,
    "ramp_end": 205_000_000,
    "target_new": 0.8,
}

_win32 = pytest.mark.skipif(
    'sys.platform == "win32"',
    reason="the learner ships rows through POSIX shared memory; the lane runs on Linux only",
)


@pytest.fixture(autouse=True, scope="module")
def _campaign_process():
    """The factory needs the process to select v0.111.0 (`STS2_GAME_VERSION`)."""
    with pytest.MonkeyPatch.context() as mp:
        mp.setattr(sts2_client, "GAME_VERSION", factory.GAME_VERSION)
        yield


# ── the schedule ─────────────────────────────────────────────────────────────


def test_mix_share_settle_ramp_hold() -> None:
    cfg = mix.MixCfg()
    assert cfg.characters == LANE
    assert mix.mix_share(0, cfg) == pytest.approx(0.0, abs=0.0)
    assert mix.mix_share(4_900_000, cfg) == pytest.approx(0.0, abs=0.0)
    assert mix.mix_share(5_000_000, cfg) == pytest.approx(0.0, abs=0.0)
    assert mix.mix_share(105_000_000, cfg) == pytest.approx(0.4)
    assert mix.mix_share(205_000_000, cfg) == pytest.approx(0.8)
    assert mix.mix_share(1_000_000_000, cfg) == pytest.approx(0.8)
    xs = [mix.mix_share(d, cfg) for d in range(0, 300_000_000, 1_000_000)]
    assert xs == sorted(xs) and max(xs) == pytest.approx(0.8)


def test_mix_cfg_refuses_a_bad_schedule() -> None:
    for bad in (
        {"ramp_end": 5_000_000},  # ramp_end <= settle_end
        {"target_new": 1.5},
        {"characters": ("Silent", "Defect")},  # no Ironclad
        {"characters": ("Ironclad",)},  # no other character
        {"characters": ("Ironclad", "Watcher")},
        {"characters": ("Ironclad", "Silent", "Silent")},
    ):
        with pytest.raises(ValueError):
            mix.MixCfg(**bad)
    with pytest.raises(ValueError, match="unknown"):
        mix.MixCfg.from_dict({**MIX, "ramp": 3})
    assert mix.MixCfg.from_dict(MIX) == mix.MixCfg()
    assert mix.MixCfg.from_dict(mix.MixCfg().to_dict()) == mix.MixCfg()


def test_draw_character_deterministic() -> None:
    cfg = mix.MixCfg()
    late = cfg.ramp_end
    a = [mix.draw_character(s, late, cfg) for s in range(SEED, SEED + 200)]
    b = [mix.draw_character(s, late, cfg) for s in range(SEED, SEED + 200)]
    assert a == b and set(a) == set(LANE)
    # Ironclad only before the ramp; a seed that draws Ironclad late draws it early too.
    assert {mix.draw_character(s, 4_900_000, cfg) for s in range(SEED, SEED + 200)} == {"Ironclad"}
    mid = [mix.draw_character(s, 105_000_000, cfg) for s in range(SEED, SEED + 200)]
    assert all(m == "Ironclad" for m, x in zip(mid, a, strict=True) if x == "Ironclad")


def test_draw_character_shares() -> None:
    cfg = mix.MixCfg()
    n = 100_000
    got = collections.Counter(
        mix.draw_character(s, cfg.ramp_end, cfg) for s in range(1_000_000_000, 1_000_000_000 + n)
    )
    assert set(got) == set(LANE)
    for c in LANE:
        assert abs(got[c] / n - 0.2) <= 0.01, (c, got)


# ── the factory ──────────────────────────────────────────────────────────────


WEIGHTS = {
    "Ironclad": 0.5,
    "Silent": 0.25,
    "Defect": 0.125,
    "Regent": 0.0625,
    "Necrobinder": 0.0625,
}


def test_a_weights_mix_draws_its_shares_at_every_clock() -> None:
    """GH #1535: fixed weights replace the ramp; the draw is the seed's alone."""
    cfg = mix.MixCfg.from_dict({"weights": WEIGHTS})
    assert cfg.characters == tuple(WEIGHTS)
    assert mix.mix_share(0, cfg) == mix.mix_share(10**10, cfg) == pytest.approx(0.5)
    seeds = range(SEED, SEED + 20_000)
    got = collections.Counter(mix.draw_character(s, 0, cfg) for s in seeds)
    for c, w in WEIGHTS.items():
        assert got[c] / 20_000 == pytest.approx(w, abs=0.015), (c, got)
    assert all(mix.draw_character(s, 0, cfg) == mix.draw_character(s, 10**10, cfg) for s in seeds)
    assert mix.MixCfg.from_dict(cfg.to_dict()) == cfg
    assert json.loads(json.dumps(cfg.to_dict()))["weights"] == WEIGHTS


def test_a_weights_mix_needs_no_ironclad_and_refuses_bad_weights() -> None:
    cfg = mix.MixCfg.from_dict({"weights": {"Silent": 3.0, "Defect": 1.0}})
    assert cfg.characters == ("Silent", "Defect") and mix.mix_share(0, cfg) == pytest.approx(1.0)
    for bad in ({"Silent": 1.0}, {"Silent": 1.0, "Defect": 0.0}, {"Silent": 1.0, "Nobody": 1.0}):
        with pytest.raises(ValueError):
            mix.MixCfg.from_dict({"weights": bad})
    with pytest.raises(ValueError, match="differ"):
        mix.MixCfg.from_dict(
            {"weights": {"Silent": 1.0, "Defect": 1.0}, "characters": ["Defect", "Silent"]}
        )


def test_a_ramp_mix_dict_has_no_weights_key() -> None:
    """A saved ramp mix (the #1197 lane's state) round-trips to the dict it was."""
    assert mix.MixCfg.from_dict(MIX).to_dict() == {**MIX, "characters": list(LANE)}


def test_new_run_character_override(monkeypatch: pytest.MonkeyPatch) -> None:
    rm = factory.new_run(SEED, character="Defect")
    assert rm.character == "Defect"
    assert factory.new_run(SEED).character == factory.CHARACTER == "Ironclad"
    monkeypatch.setattr(factory, "CHARACTER", "Regent")  # read at call time
    assert factory.new_run(SEED).character == "Regent"
    with pytest.raises(AssertionError, match="character"):
        factory.new_run(SEED, character="Watcher")


def test_check_run_accepts_lane_characters() -> None:
    rm = factory.new_run(SEED, character="Silent")
    assert factory.check_run(rm, characters=LANE) is rm
    with pytest.raises(AssertionError, match="not a campaign run"):
        factory.check_run(rm, characters={"Ironclad"})
    with pytest.raises(AssertionError, match="not a campaign run"):
        factory.check_run(rm)  # None: the process character, Ironclad


# ── the workers ──────────────────────────────────────────────────────────────


@pytest.mark.parametrize("rust", [False, True])
def test_a_segment_source_draws_each_runs_character(
    rust: bool, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A worker's full runs take `draw_character(seed, t)`; without a mix they are the
    process's character, as before."""
    monkeypatch.setenv(factory.CHARS_ENV, "1")
    cfg = mix.MixCfg()
    seeds = list(range(SEED, SEED + 12))
    q = P.PracticeQueues(worker=0, enabled=False)
    src = P.SegmentSource(q, iter(seeds), n_slots=1, cfg=E.EnvConfig(rust=rust), mix=cfg)
    want = [mix.draw_character(s, cfg.ramp_end, cfg) for s in seeds]
    assert len(set(want)) > 2
    for s, w in zip(seeds, want, strict=True):
        env = src.next(0, cfg.ramp_end)
        assert (env.seed, env.character, env.rm.character) == (s, w, w)
        assert env.cfg.characters == LANE
    plain = P.SegmentSource(q, iter(seeds), n_slots=1, cfg=E.EnvConfig(rust=rust))
    env = plain.next(0, cfg.ramp_end)
    assert env.character == env.rm.character == "Ironclad" and env.cfg.characters is None


def test_a_practice_start_keeps_its_runs_character() -> None:
    """A capture of a Silent run starts a Silent practice segment; the start's run check
    takes the lane's set (an Ironclad-only check refuses it)."""
    rm = factory.new_run(SEED, character="Silent")
    rm.set_player(hp=100_000, max_hp=100_000)
    caps: list[E.Capture] = []
    lane = E.EnvConfig(characters=LANE)
    env = E.E2EEnv(SEED, rm=rm, cfg=dataclasses.replace(lane, on_capture=caps.append))
    rng = random.Random(SEED)
    d = env.reset()
    while isinstance(d, E.E2EDecision) and not caps:
        d = env.step(rng.randrange(d.n_classes))
    assert caps, "no elite or boss room reached"
    st = P.start_env(caps[0], P.RESEED_LO, cfg=lane)
    assert st is not None and st.character == st.rm.character == "Silent"
    with pytest.raises(AssertionError, match="not a campaign run"):
        P.start_env(caps[0], P.RESEED_LO, cfg=E.EnvConfig())


def test_end_records_carry_the_character() -> None:
    term = dataclasses.make_dataclass("T", [])()
    for k, v in {
        "reward": 0.0, "outcome": "won", "won": True, "cap": None, "floor": 1, "act": 1,
        "bosses": 0, "reached_boss2": False, "rows": 1, "steps": 1, "seed": SEED,
        "error": None, "total_reward": 0.0, "unassigned_reward": 0.0, "counts": {},
    }.items():  # fmt: skip
        setattr(term, k, v)
    rec = actors.end_record(1, 0, 0, term, "run", None, {}, character="Silent")
    assert rec["character"] == "Silent"
    assert actors.end_record(1, 0, 0, term, "run", None, {})["character"] is None
    err = actors.error_record(1, 0, 0, "run", None, "boom", {}, character="Defect")
    assert err["character"] == "Defect"


def test_lockstep_on_mix_drawn_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    """An Ironclad process with "chars" declared (a mix lane's workers) plays another
    character's run per run: the Rust env's rows equal the Python env's."""
    monkeypatch.setenv(factory.CHARS_ENV, "1")
    ls = Lockstep(f9_p=0.0)
    run_seeds(ls, SEED, 2, pinned=False, practice=0, character="Silent", characters=LANE)
    # A pinned run reaches its elites and bosses: its captures start practice segments.
    run_seeds(ls, SEED + 9, 1, pinned=True, practice=1, character="Necrobinder", characters=LANE)
    assert ls.stats["rows"] > 100 and ls.stats["captures"] > 0
    assert ls.mismatches == 0, {"stats": dict(ls.stats), "examples": ls.examples[:5]}


def test_an_action_log_replays_a_mix_run_as_its_character() -> None:
    """A mix lane's worker logs its run's character in the record's `extra`
    (`actors._char`); the replay builds the run as that character."""
    from ppo.e2e import actionlog as AL

    env = E.E2EEnv(SEED, cfg=E.EnvConfig(character="Defect"))
    rng = random.Random(SEED)
    d = env.reset()
    while isinstance(d, E.E2EDecision):
        d = env.step(rng.randrange(d.n_classes))
    assert actors._char(env) == {"character": "Defect"}
    rec = AL.terminal_record(env.terminal, reason="test", extra=actors._char(env))
    assert AL.replay(rec).fingerprint == env.terminal.fingerprint
    with pytest.raises((AssertionError, IndexError)):  # as the process's Ironclad: diverges
        AL.replay(AL.terminal_record(env.terminal, reason="test"))
    assert actors._char(E.E2EEnv(SEED)) == {}  # a one-character lane's records: unchanged


class _Conn:
    """A learner stub for an in-process `actors.worker_main`: random choices, the clock
    past the ramp, every message hashed and every end kept."""

    def __init__(self, rounds: int, t0: int) -> None:
        self.rounds, self.t0, self.r = rounds, t0, 0
        self.h = hashlib.sha256()
        self.rng = np.random.default_rng(0)
        self.n: np.ndarray = np.zeros(0)
        self.ends: list[dict] = []

    def send(self, msg) -> None:
        _, _wid, metas, r_prev, rb, _extra, ends = msg
        for k in sorted(rb):
            v = rb[k]
            if isinstance(v, np.ndarray):
                self.h.update(k.encode() + np.ascontiguousarray(v).tobytes())
        self.h.update(metas.tobytes() + np.asarray(r_prev, np.float64).tobytes())
        skip = {"wheel", "py_src", "error", "f9_samples"}
        for e in ends:
            self.ends.append(e)
            self.h.update(
                json.dumps({k: v for k, v in e.items() if k not in skip}, sort_keys=True).encode()
            )
        self.n = metas[:, 8]

    def recv(self):
        self.r += 1
        if self.r > self.rounds:
            return ("stop",)
        acts = [int(self.rng.integers(max(1, int(n)))) for n in self.n]
        return ("act", acts, {"decisions": self.t0 + self.r})

    def close(self) -> None:
        pass


def _mix_worker(rust: bool, monkeypatch: pytest.MonkeyPatch) -> _Conn:
    # As test_rsenv's worker test: keep `worker_main`'s process settings in this test.
    monkeypatch.setattr(signal, "signal", lambda *a: None)
    for k in ("RAYON_NUM_THREADS", "OMP_NUM_THREADS"):
        monkeypatch.setenv(k, os.environ.get(k, "1"))
    t0 = mix.MixCfg().ramp_end
    conn = _Conn(60, t0)
    cfg = {
        "n_envs": 8,
        "run_seed_base": SEED,
        "start_it": 0,
        "start_decisions": t0,
        "seed": 0,
        "practice": False,
        "f9_tolerate": [],
        "teacher_rare": None,
        "run_teacher_honest": False,
        "action_log_dir": None,
        "rust_env": rust,
        "mix": mix.MixCfg().to_dict(),
    }
    actors.worker_main(0, conn, cfg)
    return conn


def test_a_mix_worker_plays_each_runs_drawn_character(monkeypatch: pytest.MonkeyPatch) -> None:
    """`actors.worker_main` with a mix (the learner's `worker_cfg`): every run ends with
    the character drawn for its seed, and the Rust env's messages equal the Python
    env's byte for byte."""
    monkeypatch.setenv(factory.CHARS_ENV, "1")
    py = _mix_worker(False, monkeypatch)
    rs = _mix_worker(True, monkeypatch)
    assert rs.h.hexdigest() == py.h.hexdigest() and len(rs.ends) == len(py.ends)
    runs = [e for e in rs.ends if e["kind"] == "run" and e["outcome"] != "error"]
    assert len(runs) >= 4
    for e in runs:
        assert e["character"] == mix.draw_character(e["seed"], mix.MixCfg().ramp_end, mix.MixCfg())
    assert len({e["character"] for e in runs}) >= 2


# ── the learner ──────────────────────────────────────────────────────────────


@_win32
def test_learner_mix_requires_chars(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv(factory.CHARS_ENV, raising=False)
    with pytest.raises(L.UsageError, match='"chars"'):
        TL.make_learner(TL.make_cfg(tmp_path / "a", mix=MIX))
    with pytest.raises(L.UsageError, match="--mix"):
        TL.make_learner(TL.make_cfg(tmp_path / "b", mix={**MIX, "ramp_end": 1}, filled=["chars"]))
    # --character and the process check are skipped with --mix.
    lr = TL.make_learner(
        TL.make_cfg(tmp_path / "c", mix=MIX, filled=["chars"], character="Necrobinder")
    )
    assert factory.fill_chars() and lr.worker_cfg()["mix"] == mix.MixCfg.from_dict(MIX).to_dict()
    lr = TL.make_learner(TL.make_cfg(tmp_path / "d"))
    assert lr.worker_cfg()["mix"] is None
    factory.set_fill_chars(False)


@_win32
@pytest.mark.parametrize(
    "teacher",
    [{"teacher_pair": "R3m"}, {"combat_teacher": "c.pt"}, {"run_teacher": "r.pt"}],
)
def test_learner_mix_refuses_a_teacher(tmp_path: Path, teacher: dict[str, str]) -> None:
    """The teachers are Ironclad champions: a mixed lane would distil them onto every
    character's rows. Refused before any teacher loads."""
    with pytest.raises(L.UsageError, match=r"--mix.*teacher"):
        TL.make_learner(TL.make_cfg(tmp_path, mix=MIX, filled=["chars"], **teacher))
    factory.set_fill_chars(False)


@_win32
def test_mix_share_resume_invariant(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """A resumed lane keeps its mix without --mix (the state carries it) and computes
    the same share from the same decisions (the AlgoState's clock)."""
    monkeypatch.delenv(factory.CHARS_ENV, raising=False)
    m = {**MIX, "settle_end": 0, "ramp_end": 10**6, "target_new": 0.6}
    lr = TL.make_learner(TL.make_cfg(tmp_path, mix=m, filled=["chars"], max_iters=2))
    assert lr.train() == "max_iters"
    cfg = mix.MixCfg.from_dict(m)
    dec = int(lr.algo.clocks.decisions)
    assert 0 < mix.mix_share(dec, cfg) < 0.6
    algo = A.AlgoState.from_dict(lr.algo.to_dict())
    assert algo.clocks.decisions == dec
    (tmp_path / "DONE").unlink(missing_ok=True)
    lr2 = TL.make_learner(
        TL.make_cfg(tmp_path, filled=["chars"], max_iters=4, resume=str(tmp_path / "state.pt"))
    )
    w = lr2.worker_cfg()
    assert w["mix"] == cfg.to_dict() and lr2.cfg.mix == cfg.to_dict()
    assert w["start_decisions"] == dec
    assert mix.mix_share(w["start_decisions"], mix.MixCfg.from_dict(w["mix"])) == mix.mix_share(
        dec, cfg
    )
    assert lr2.train() == "max_iters"  # and the state it saves keeps the mix
    # A new --mix on a resume replaces the saved one (the `sched` rule).
    (tmp_path / "DONE").unlink(missing_ok=True)
    m3 = {**m, "target_new": 0.5}
    lr3 = TL.make_learner(
        TL.make_cfg(
            tmp_path, mix=m3, filled=["chars"], max_iters=4, resume=str(tmp_path / "state.pt")
        )
    )
    assert lr3.worker_cfg()["mix"] == mix.MixCfg.from_dict(m3).to_dict()
    factory.set_fill_chars(False)


@_win32
@pytest.mark.slow
def test_a_mix_lane_on_real_workers(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Spawned `actors.worker_main` processes on the Rust env with a mix and the fill
    check on (the launch path): a fresh lane's first runs start at decision 0
    (Ironclad); the resumed lane's workers start past the ramp and draw every
    character from their first run. Every shipped table passes the schema's
    validator and the fill check, with no error segment."""
    from ppo.e2e import contracts as C
    from ppo.e2e import fillcheck as FC
    from ppo.e2e import schedules as S

    pytest.importorskip("sts2_rs")
    monkeypatch.setenv("STS2_GAME_VERSION", TL.V111)
    monkeypatch.setenv("OMP_NUM_THREADS", "1")
    monkeypatch.delenv(factory.CHARS_ENV, raising=False)
    shipped = TL.spy_shipped(monkeypatch)
    m = {**MIX, "settle_end": 0, "ramp_end": 1}
    kw = {
        "workers": 2,
        "n_envs": 3,
        "iter_decisions": 96,
        "rust_env": True,
        "fill_check": True,
        "filled": sorted({*FC.wheel_filled(), "chars"}),
    }
    clock = lambda: S.ANNEAL_START - 86400.0  # noqa: E731
    lr = L.Learner(TL.make_cfg(tmp_path, max_iters=2, mix=m, **kw), updater="local", clock=clock)
    assert lr.train() == "max_iters"
    assert lr.stats_rows[-1]["errors"] == 0
    (tmp_path / "DONE").unlink(missing_ok=True)
    lr2 = L.Learner(
        TL.make_cfg(tmp_path, max_iters=3, resume=str(tmp_path / "state.pt"), **kw),
        updater="local",
        clock=clock,
    )
    assert lr2.train() == "max_iters" and lr2.cfg.mix == mix.MixCfg.from_dict(m).to_dict()
    rows = lr.stats_rows + lr2.stats_rows
    assert all(r["errors"] == 0 for r in rows)
    assert any(r.get("fill_check") is not None for r in rows)
    for rb in shipped:
        C.validate_row_batch(rb)
    runs: collections.Counter = collections.Counter()
    for r in lr2.stats_rows:
        for w in (r.get("workers") or {}).values():
            runs.update({k: v for k, v in w["sources"].items() if k.startswith("run/")})
    assert len(runs) >= 2, runs  # Ironclad and at least one other character
    # GH #1197: the stats rows split by the real rows' X_CHAR and the runs' character.
    shares: collections.Counter = collections.Counter()
    for r in lr2.stats_rows:
        assert r["mix_share"] == pytest.approx(m["target_new"])
        assert sum(d["runs"] for d in r["by_char"].values()) == r["runs"]
        shares.update({c: d["share"] or 0.0 for c, d in r["by_char"].items()})
    assert any(c != "Ironclad" and s > 0 for c, s in shares.items()), shares
    factory.set_fill_chars(False)
