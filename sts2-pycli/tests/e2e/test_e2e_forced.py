"""GH #1541: forced deck inclusion (`ppo.e2e.forced`) and its worker, replay and learner paths."""

from __future__ import annotations

import dataclasses
import random
from collections import Counter
from pathlib import Path

import pytest
import sts2_client
import test_learner as TL  # make_cfg, make_learner: the fake source and teachers
from ppo.e2e import actors, factory, forced
from ppo.e2e import env as E
from ppo.e2e import learner as L
from ppo.e2e import practice as P

SEED = 1_300_000_900  # the throwaway block (plan §3.1)

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


def _norm(card: str) -> str:
    """`master_deck` names (`Whirlwind`, `OneTwoPunchCard`: a few enum names carry a
    `Card` suffix) and ids (`ONE_TWO_PUNCH`) alike."""
    return str(card).replace("_", "").upper().removesuffix("CARD")


def _deck_ids(rm) -> Counter:
    return Counter(_norm(c) for c in rm.master_deck())


# ── the config and the draw ──────────────────────────────────────────────────


def test_default_cards_cover_every_character_and_are_engine_ids() -> None:
    """Every listed id is a card `force_add_card` accepts, for its own character."""
    cards = forced.default_cards()
    assert set(cards) == set(factory.CHARACTERS)
    for ch, ids in cards.items():
        assert ids and len(set(ids)) == len(ids), ch
        rm = factory.new_run(SEED, character=ch)
        before = _deck_ids(rm)
        for cid in ids:
            rm.force_add_card(cid)
        assert _deck_ids(rm) - before == Counter(_norm(c) for c in ids), ch


def test_forced_cfg_round_trips_and_refuses_bad_input() -> None:
    cfg = forced.ForcedCfg.from_dict({"p": 0.05, "cards": {"Silent": ["BURST"]}})
    assert cfg.to_dict() == {"p": 0.05, "cards": {"Silent": ["BURST"]}}
    assert forced.ForcedCfg.from_dict(cfg.to_dict()) == cfg
    assert forced.ForcedCfg.from_dict({}).to_dict() == {"p": 0.03, "cards": {}}
    with pytest.raises(ValueError, match="unknown keys"):
        forced.ForcedCfg.from_dict({"share": 0.1})
    with pytest.raises(ValueError, match=r"outside \[0, 1\]"):
        forced.ForcedCfg(p=1.5)
    with pytest.raises(ValueError, match="not in"):
        forced.ForcedCfg.from_dict({"cards": {"Watcher": ["X"]}})


def test_draw_forced_is_a_function_of_the_seed_at_share_p() -> None:
    cfg = forced.ForcedCfg(p=0.2)
    seeds = range(SEED, SEED + 4000)
    got = [forced.draw_forced(s, "Defect", cfg) for s in seeds]
    assert got == [forced.draw_forced(s, "Defect", cfg) for s in seeds]
    hit = [c for c in got if c is not None]
    assert 0.17 < len(hit) / len(got) < 0.23
    pool = forced.default_cards()["Defect"]
    assert set(hit) == set(pool)  # every card drawn, none outside the list
    # The same seeds force at a lower p only where they forced at the higher one.
    low = [forced.draw_forced(s, "Defect", forced.ForcedCfg(p=0.05)) for s in seeds]
    assert all(lo is None or lo == hi for lo, hi in zip(low, got, strict=True))
    assert forced.draw_forced(SEED, "Defect", None) is None
    assert forced.draw_forced(SEED, "Defect", forced.ForcedCfg(p=0.0)) is None
    only = forced.ForcedCfg(p=1.0, cards={"Silent": ("BURST",)})
    assert forced.draw_forced(SEED, "Silent", only) == "BURST"
    assert forced.draw_forced(SEED, "Ironclad", only) is None  # no list: never forced


# ── the env and the worker ───────────────────────────────────────────────────


@pytest.mark.parametrize("rust", [False, True])
def test_a_segment_source_forces_a_card_into_the_starting_deck(rust: bool) -> None:
    """At p=1 every full run starts with its drawn card and captures nothing; at p=0
    the runs are as before."""
    seeds = list(range(SEED, SEED + 4))
    q = P.PracticeQueues(worker=0, enabled=True)
    cfg = forced.ForcedCfg(p=1.0)
    src = P.SegmentSource(q, iter(seeds), n_slots=1, cfg=E.EnvConfig(rust=rust), forced=cfg)
    plain = P.SegmentSource(q, iter(seeds), n_slots=1, cfg=E.EnvConfig(rust=rust))
    for s in seeds:
        env, base = src.next(0, 0), plain.next(0, 0)
        card = forced.draw_forced(s, "Ironclad", cfg)
        assert card is not None and env.forced_card == card and env.seed == s
        assert _deck_ids(env.rm) - _deck_ids(base.rm) == Counter({_norm(card): 1})
        assert env.cfg.on_capture is None and base.cfg.on_capture is not None
        assert base.forced_card is None
    assert src.stats["run/forced"] == len(seeds) and plain.stats["run/forced"] == 0


def test_a_forced_card_needs_a_run_the_env_builds() -> None:
    rm = factory.new_run(SEED)
    with pytest.raises(ValueError, match="forced_card"):
        E.E2EEnv(SEED, rm=rm, cfg=E.EnvConfig(forced_card="WHIRLWIND"))


def test_an_action_log_replays_a_forced_run() -> None:
    """The record's `extra` names the card (`actors._char`); the replay adds it."""
    from ppo.e2e import actionlog as AL

    env = E.E2EEnv(SEED, cfg=E.EnvConfig(forced_card="BREAKTHROUGH"))
    rng = random.Random(SEED)
    d = env.reset()
    while isinstance(d, E.E2EDecision):
        d = env.step(rng.randrange(d.n_classes))
    assert actors._char(env) == {"forced_card": "BREAKTHROUGH"}
    rec = AL.terminal_record(env.terminal, reason="test", extra=actors._char(env))
    assert AL.replay(rec).fingerprint == env.terminal.fingerprint
    with pytest.raises((AssertionError, IndexError)):  # without the card: diverges
        AL.replay(AL.terminal_record(env.terminal, reason="test"))


def test_end_records_carry_the_forced_card_and_the_learner_counts_them() -> None:
    term = dataclasses.make_dataclass("T", [])()
    for k, v in {
        "reward": 0.0, "outcome": "won", "won": True, "cap": None, "floor": 1, "act": 1,
        "bosses": 0, "reached_boss2": False, "rows": 1, "steps": 1, "seed": SEED,
        "error": None, "total_reward": 0.0, "unassigned_reward": 0.0, "counts": {},
    }.items():  # fmt: skip
        setattr(term, k, v)
    rec = actors.end_record(1, 0, 0, term, "run", None, {}, forced_card="CLAW", character="Defect")
    assert rec["forced_card"] == "CLAW"
    assert actors.end_record(1, 0, 0, term, "run", None, {})["forced_card"] is None
    ends = [
        {"forced_card": "CLAW", "character": "Defect", "won": True, "floor": 50},
        {"forced_card": "CLAW", "character": "Defect", "won": False, "floor": 20},
        {"forced_card": "WHIRLWIND", "character": None, "won": False, "floor": 10},
        {"forced_card": None, "character": "Silent", "won": True, "floor": 49},
    ]
    st = forced.forced_stats(ends)
    assert st["all"] == {"runs": 3, "won": 1, "floor_mean": pytest.approx(80 / 3)}
    assert st["cards"]["Defect:CLAW"] == {"runs": 2, "won": 1, "floor_mean": 35.0}
    assert st["cards"]["Ironclad:WHIRLWIND"]["runs"] == 1
    assert forced.forced_stats(ends[3:]) == {}


# ── the learner ──────────────────────────────────────────────────────────────


@_win32
def test_learner_settles_keeps_and_replaces_forced(tmp_path: Path) -> None:
    """`--forced` reaches the workers normalized, a resume without it keeps the state's,
    and a bad one is refused."""
    with pytest.raises(L.UsageError, match="--forced"):
        TL.make_learner(TL.make_cfg(tmp_path / "bad", forced={"p": 2.0}))
    lr = TL.make_learner(TL.make_cfg(tmp_path, forced={"p": 0.5}, max_iters=2))
    want = forced.ForcedCfg(p=0.5).to_dict()
    assert lr.worker_cfg()["forced"] == want
    assert lr.train() == "max_iters"
    (tmp_path / "DONE").unlink(missing_ok=True)
    lr2 = TL.make_learner(TL.make_cfg(tmp_path, max_iters=3, resume=str(tmp_path / "state.pt")))
    assert lr2.worker_cfg()["forced"] == want and lr2.cfg.forced == want
    assert lr2.train() == "max_iters"  # and releases the lane's lock
    (tmp_path / "DONE").unlink(missing_ok=True)
    lr3 = TL.make_learner(
        TL.make_cfg(tmp_path, forced={"p": 0.1}, max_iters=3, resume=str(tmp_path / "state.pt"))
    )
    assert lr3.worker_cfg()["forced"]["p"] == pytest.approx(0.1)
    assert TL.make_learner(TL.make_cfg(tmp_path / "off")).worker_cfg()["forced"] is None


@_win32
def test_learner_trains_with_the_probability_floor(tmp_path: Path) -> None:
    """A lane with the floor on trains, and its stats rows carry the floor settings."""
    sched = {**TL.SCHED, "floor_coef": 0.1, "floor_p": 0.05, "floor_start": 1, "floor_ramp": 0}
    lr = TL.make_learner(TL.make_cfg(tmp_path, sched=sched, max_iters=5))
    assert lr.train() == "max_iters"
    rows = lr.stats_rows
    assert rows and all(r["update"]["floor_coef"] == pytest.approx(0.1) for r in rows)
    trained = [r["update"] for r in rows if r["update"]["policy_on"]]
    assert trained and all("n_fl" in u for u in trained)
