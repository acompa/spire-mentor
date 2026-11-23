"""Human starts (`ppo.e2e.human`): the bank, the start, and the worker, replay and learner paths."""

from __future__ import annotations

import dataclasses
import gzip
import json
import random
from collections import Counter
from pathlib import Path

import pytest
import sts2_client
import test_learner as TL  # make_cfg, make_learner: the fake source and teachers
from ppo.e2e import actors, factory, human
from ppo.e2e import env as E
from ppo.e2e import learner as L
from ppo.e2e import practice as P

SEED = 1_300_000_900  # the throwaway block (plan §3.1)

_win32 = pytest.mark.skipif(
    'sys.platform == "win32"',
    reason="the learner ships rows through POSIX shared memory; the lane runs on Linux only",
)

# A Silent player entering floor 24 (act 2, standing on row 4): what a bank record holds.
START = {
    "key": "test-run:24",
    "character": "Silent",
    "build": "v0.104.0",
    "ascension": 10,
    "won": False,
    "act": 2,
    "row": 4,
    "floor": 24,
    "hp": 41,
    "max_hp": 73,
    "gold": 152,
    "deck": [
        *["STRIKE_SILENT"] * 4,
        *["DEFEND_SILENT+1"] * 5,
        "NEUTRALIZE",
        "SURVIVOR",
        "BLADE_DANCE+1",
        "ACCURACY",
        "FOOTWORK",
        "BACKFLIP@SWIFT:1",
    ],
    "relics": ["RING_OF_THE_SNAKE", "ANCHOR", "VAJRA"],
    "potions": ["FIRE_POTION"],
}


@pytest.fixture(autouse=True, scope="module")
def _campaign_process():
    """The factory needs the process to select v0.111.0 (`STS2_GAME_VERSION`)."""
    with pytest.MonkeyPatch.context() as mp:
        mp.setattr(sts2_client, "GAME_VERSION", factory.GAME_VERSION)
        yield


@pytest.fixture
def bank(tmp_path: Path) -> Path:
    """A bank of two Silent starts and one Defect start."""
    p = tmp_path / "bank.jsonl.gz"
    other = {**START, "key": "test-run:30", "floor": 30, "row": 10}
    defect = {
        **START,
        "key": "d:5",
        "character": "Defect",
        "act": 1,
        "row": 2,
        "floor": 5,
        "deck": [*["STRIKE_DEFECT"] * 4, *["DEFEND_DEFECT"] * 4, "ZAP", "DUALCAST"],
        "relics": ["CRACKED_CORE"],
        "potions": [],
    }
    with gzip.open(p, "wt") as f:
        for s in (START, other, defect):
            f.write(json.dumps(s) + "\n")
    human.clear_bank_cache()
    return p


def _norm(card: str) -> str:
    return str(card).replace("_", "").upper().removesuffix("CARD")


# ── the config and the draw ──────────────────────────────────────────────────


def test_human_cfg_round_trips_and_refuses_bad_input() -> None:
    cfg = human.HumanCfg.from_dict({"p": 0.2, "bank": "/x/bank.jsonl.gz"})
    assert cfg.to_dict() == {"p": 0.2, "bank": "/x/bank.jsonl.gz"}
    assert human.HumanCfg.from_dict(cfg.to_dict()) == cfg
    assert human.HumanCfg.from_dict({}).to_dict() == {"p": 0.1, "bank": ""}
    assert human.HumanCfg().path == human.DEFAULT_BANK
    with pytest.raises(ValueError, match="unknown keys"):
        human.HumanCfg.from_dict({"share": 0.1})
    with pytest.raises(ValueError, match=r"outside \[0, 1\]"):
        human.HumanCfg(p=1.5)


def test_draw_start_is_a_function_of_the_seed_at_share_p(bank: Path) -> None:
    cfg = human.HumanCfg(p=0.2, bank=str(bank))
    seeds = range(SEED, SEED + 4000)
    got = [human.draw_start(s, "Silent", cfg) for s in seeds]
    assert got == [human.draw_start(s, "Silent", cfg) for s in seeds]
    hit = [s for s in got if s is not None]
    assert 0.17 < len(hit) / len(got) < 0.23
    assert {s["key"] for s in hit} == {"test-run:24", "test-run:30"}  # its own character's
    pool = human.load_bank(str(bank))["Silent"]  # the parsed bank picks the same starts
    assert all(g is None or g in pool for g in got)
    one = dataclasses.replace(cfg, p=1.0)  # each draw is a fresh dict: callers may mutate it
    assert human.draw_start(SEED, "Silent", one) is not human.draw_start(SEED, "Silent", one)
    low = [human.draw_start(s, "Silent", dataclasses.replace(cfg, p=0.05)) for s in seeds]
    assert all(lo is None or lo == hi for lo, hi in zip(low, got, strict=True))
    assert human.draw_start(SEED, "Silent", None) is None
    assert human.draw_start(SEED, "Regent", human.HumanCfg(p=1.0, bank=str(bank))) is None
    with pytest.raises(FileNotFoundError, match=r"ppo\.e2e\.human build"):
        human.draw_start(SEED, "Silent", human.HumanCfg(p=1.0, bank=str(bank) + ".missing"))


# ── the start ────────────────────────────────────────────────────────────────


def test_apply_start_restores_the_players_state_on_the_map() -> None:
    rm = factory.new_run(SEED, character="Silent")
    assert human.apply_start(rm, START, SEED) == []
    assert rm.phase() == "map_choice" and rm.act_index == 2
    assert rm.current_room()["map_coord"][1] == START["row"]
    assert rm.available_actions() and all(a["action"] == "move" for a in rm.available_actions())
    deck = Counter(_norm(c) for c in rm.master_deck())
    want = Counter(_norm(human.card_spec(c)["id"]) for c in START["deck"])
    assert deck == want
    assert [r.removeprefix("RELIC.") for r in rm.relics()] == START["relics"]
    assert (rm.current_hp, rm.max_hp, rm.gold) == (41, 73, 152)
    assert [_norm(x) for x in rm.potions() if x] == [_norm(x) for x in START["potions"]]
    # The same seed parks on the same node.
    rm2 = factory.new_run(SEED, character="Silent")
    human.apply_start(rm2, START, SEED)
    assert rm2.current_room() == rm.current_room()
    with pytest.raises(ValueError, match="Silent"):
        human.apply_start(factory.new_run(SEED, character="Defect"), START, SEED)


def test_card_spec_reads_upgrades_and_enchantments() -> None:
    assert human.card_spec("BASH") == {"id": "BASH", "upgraded": False}
    assert human.card_spec("BASH+1") == {"id": "BASH", "upgraded": True}
    assert human.card_spec("BACKFLIP+1@SWIFT:2") == {
        "id": "BACKFLIP",
        "upgraded": True,
        "enchantment": "SWIFT",
        "enchantment_amount": 2,
    }


# ── the env and the worker ───────────────────────────────────────────────────


@pytest.mark.parametrize("rust", [False, True])
def test_a_segment_source_starts_runs_from_the_bank(bank: Path, rust: bool) -> None:
    """At p=1 every full run plays from a drawn start (no forced card) and captures
    nothing; the character comes from the start."""
    from ppo.e2e import forced

    seeds = list(range(SEED, SEED + 3))
    q = P.PracticeQueues(worker=0, enabled=True)
    cfg = human.HumanCfg(p=1.0, bank=str(bank))
    src = P.SegmentSource(
        q,
        iter(seeds),
        n_slots=1,
        cfg=E.EnvConfig(rust=rust, character="Silent"),
        human=cfg,
        forced=forced.ForcedCfg(p=1.0),
    )
    for s in seeds:
        env = src.next(0, 0)
        start = human.draw_start(s, "Silent", cfg)
        assert env.human_start == start and env.seed == s
        assert env.forced_card is None
        assert env.cfg.on_capture is None
        assert env.rm.act_index == start["act"]
    assert src.stats["run/human"] == len(seeds) and src.stats["run/forced"] == 0


def test_a_human_start_needs_a_run_the_env_builds() -> None:
    rm = factory.new_run(SEED, character="Silent")
    with pytest.raises(ValueError, match="human_start"):
        E.E2EEnv(SEED, rm=rm, cfg=E.EnvConfig(human_start=START))
    with pytest.raises(ValueError, match="for a Defect run"):
        E.E2EEnv(SEED, cfg=E.EnvConfig(human_start=START, character="Defect"))


def test_an_action_log_replays_a_human_start_run() -> None:
    """The record's `extra` carries the whole start (`actors._char`); the replay rebuilds it."""
    from ppo.e2e import actionlog as AL

    env = E.E2EEnv(SEED, cfg=E.EnvConfig(human_start=START, character="Silent"))
    rng = random.Random(SEED)
    d = env.reset()
    while isinstance(d, E.E2EDecision):
        d = env.step(rng.randrange(d.n_classes))
    extra = actors._char(env)
    assert extra["human_start"] == START
    rec = AL.terminal_record(env.terminal, reason="test", extra=extra)
    assert AL.replay(rec).fingerprint == env.terminal.fingerprint


def test_end_records_carry_the_start_and_the_learner_counts_them() -> None:
    term = dataclasses.make_dataclass("T", [])()
    for k, v in {
        "reward": 0.0, "outcome": "won", "won": True, "cap": None, "floor": 1, "act": 1,
        "bosses": 0, "reached_boss2": False, "rows": 1, "steps": 1, "seed": SEED,
        "error": None, "total_reward": 0.0, "unassigned_reward": 0.0, "counts": {},
    }.items():  # fmt: skip
        setattr(term, k, v)
    rec = actors.end_record(1, 0, 0, term, "run", None, {}, human_start=START)
    assert (rec["human_key"], rec["human_act"]) == ("test-run:24", 2)
    plain = actors.end_record(1, 0, 0, term, "run", None, {})
    assert plain["human_key"] is None and plain["human_act"] is None
    ends = [
        {"human_key": "a:24", "human_act": 2, "won": True, "floor": 50},
        {"human_key": "b:40", "human_act": 3, "won": False, "floor": 44},
        {"human_key": "c:20", "human_act": 2, "won": False, "floor": 30},
        {"human_key": None, "human_act": None, "won": True, "floor": 49},
    ]
    st = human.human_stats(ends)
    assert st["all"] == {"runs": 3, "won": 1, "floor_mean": pytest.approx(124 / 3)}
    assert st["acts"]["act2"] == {"runs": 2, "won": 1, "floor_mean": 40.0}
    assert st["acts"]["act3"]["runs"] == 1
    assert human.human_stats(ends[3:]) == {}


# ── the bank ─────────────────────────────────────────────────────────────────


def test_bank_build_from_the_corpus(tmp_path: Path) -> None:
    """A few corpus runs give starts in every act they reached, each of which builds."""
    if not human.CORPUS.is_dir():
        pytest.skip("no unpacked run corpus (`just unpack-corpus` in sts2-rs)")
    root = tmp_path / "corpus"
    root.mkdir()
    for f in sorted(human.CORPUS.rglob("*.run"))[:12]:
        (root / f.name).write_bytes(f.read_bytes())
    out = tmp_path / "bank.jsonl.gz"
    why = human.build_bank(root, out, per_run=6)
    assert not [k for k in why if k.startswith("start/")], why
    human.clear_bank_cache()
    starts = [s for v in human.load_bank(str(out)).values() for s in v]
    assert starts and len({s["key"] for s in starts}) == len(starts)
    for s in starts:
        assert s["act"] in (1, 2, 3) and s["row"] >= 0 and 0 < s["hp"] <= s["max_hp"]


# ── the learner ──────────────────────────────────────────────────────────────


@_win32
def test_learner_settles_keeps_and_replaces_human(tmp_path: Path, bank: Path) -> None:
    """`--human` reaches the workers normalized, a resume without it keeps the state's,
    and a bad one (or a missing bank) is refused at the learner."""
    with pytest.raises(L.UsageError, match="--human"):
        TL.make_learner(TL.make_cfg(tmp_path / "bad", human={"p": 2.0}))
    with pytest.raises(L.UsageError, match="missing"):
        TL.make_learner(TL.make_cfg(tmp_path / "nobank", human={"bank": str(bank) + ".x"}))
    want = human.HumanCfg(p=0.5, bank=str(bank)).to_dict()
    lr = TL.make_learner(TL.make_cfg(tmp_path, human=want, max_iters=2))
    assert lr.worker_cfg()["human"] == want
    assert lr.train() == "max_iters"
    (tmp_path / "DONE").unlink(missing_ok=True)
    lr2 = TL.make_learner(TL.make_cfg(tmp_path, max_iters=3, resume=str(tmp_path / "state.pt")))
    assert lr2.worker_cfg()["human"] == want and lr2.cfg.human == want
    assert TL.make_learner(TL.make_cfg(tmp_path / "off")).worker_cfg()["human"] is None


def test_the_eval_split_holds_out_whole_runs(tmp_path: Path) -> None:
    """About EVAL_SHARE of the runs are held out, every start of a run on one side, and
    training draws only held-in starts."""
    runs = [f"run-{i}" for i in range(2000)]
    sides = {r: human.split_of(f"{r}:5") for r in runs}
    assert all(human.split_of(f"{r}:{f}") == sides[r] for r in runs[:50] for f in (3, 20, 40))
    held = sum(v == "eval" for v in sides.values()) / len(runs)
    assert abs(held - human.EVAL_SHARE) < 0.03
    p = tmp_path / "split.jsonl.gz"
    with gzip.open(p, "wt") as f:
        for r in runs[:200]:
            f.write(json.dumps({**START, "key": f"{r}:24"}) + "\n")
    human.clear_bank_cache()
    train = {s["key"] for s in human.load_bank(str(p))["Silent"]}
    ev = {s["key"] for s in human.load_bank(str(p), "eval")["Silent"]}
    assert not train & ev and len(train | ev) == 200
    assert len(human.load_bank(str(p), "all")["Silent"]) == 200
    cfg = human.HumanCfg(p=1.0, bank=str(p))
    drawn = {human.draw_start(s, "Silent", cfg)["key"] for s in range(SEED, SEED + 500)}
    assert drawn <= train


# ── the held-out eval ────────────────────────────────────────────────────────


@pytest.fixture
def split_bank(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """200 Silent starts over 200 runs (both splits), the eval's bank."""
    p = tmp_path / "split.jsonl.gz"
    with gzip.open(p, "wt") as f:
        for i in range(200):
            won = i % 3 == 0
            f.write(json.dumps({**START, "key": f"r{i}:24", "won": won, "act": 2}) + "\n")
    monkeypatch.setenv(human.BANK_ENV, str(p))
    human.clear_bank_cache()
    human.eval_starts.cache_clear()
    return p


def test_dev_seed_i_plays_held_out_start_i(split_bank: Path) -> None:
    from ppo.e2e.panels import DEV_2000

    pool = human.eval_starts("Silent", str(split_bank))
    assert pool and all(human.split_of(s["key"]) == "eval" for s in pool)
    assert [human.eval_start(DEV_2000.start + i, "Silent") for i in range(len(pool))] == list(pool)
    with pytest.raises(ValueError, match="outside"):
        human.eval_start(DEV_2000.start + len(pool), "Silent")
    with pytest.raises(ValueError, match="no held-out"):
        human.eval_start(DEV_2000.start, "Regent")


def test_the_held_out_eval_plays_through_evaluate(
    split_bank: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """`evaluate`'s Player on `eval_env` plays a held-out start to the end, as Silent."""
    from ppo.e2e import evaluate as EV
    from ppo.e2e.panels import DEV_2000

    monkeypatch.setattr(factory, "CHARACTER", "Silent")
    row = EV.Player(human.EVAL_ENV, "random", None, "cpu").play(DEV_2000.start)
    assert row["character"] == "Silent" and "won" in row, row
    assert row["end"]["act"] >= 2  # it started in act 2


def test_eval_report_compares_with_the_humans(split_bank: Path) -> None:
    from ppo.e2e.panels import DEV_2000

    n = len(human.eval_starts("Silent", str(split_bank)))
    rows = [{"seed": DEV_2000.start + i, "character": "Silent", "won": i < 2} for i in range(n)]
    rows.append({"seed": DEV_2000.start, "character": "Silent", "outcome": "error"})
    rep = human.eval_report(rows)
    assert rep["all"]["runs"] == n and rep["all"]["wins"] == 2 and set(rep) == {"all", "act2"}
    want = sum(human.eval_start(r["seed"], "Silent")["won"] for r in rows[:n]) / n
    assert rep["all"]["human_win_rate"] == pytest.approx(want)
    assert rep["all"]["ci_lo"] <= rep["all"]["win_rate"] <= rep["all"]["ci_hi"]


def test_a_start_never_breaks_the_f9_invariants() -> None:
    """A human's belt can be bigger than the run's slots (a lower ascension than the
    lane's A10, or a potion use the scrape missed): the restore keeps the oldest potions
    up to `max_potion_slots`, and clamps HP and gold, so F9 sees a legal state (the
    allchar smoke at it 7025 stopped on `run.potion_overflow`, 3 potions in 2 slots)."""
    from ppo.e2e.invariants import check

    big = {
        **START,
        "potions": ["FIRE_POTION", "BLOCK_POTION", "SWIFT_POTION", "ENERGY_POTION", "FEAR_POTION"],
        "hp": 90,
        "gold": -5,
    }
    rm = factory.new_run(SEED, character="Silent")
    human.apply_start(rm, big, SEED)
    slots = int(rm.max_potion_slots())
    held = [x for x in rm.potions() if x]
    assert len(held) == min(slots, 5) and slots < 5
    assert [_norm(x) for x in held] == [_norm(x) for x in big["potions"][:slots]]
    assert (rm.current_hp, rm.max_hp, rm.gold) == (73, 73, 0)
    assert check(rm) == []
    assert human.check_start(big, SEED) is None


def test_an_edited_run_gives_no_starts_and_a_start_never_restores_its_gold() -> None:
    """A corpus run with ~960M gold (an edited save) put inf into the tokenizer's float16
    gold feature and NaN into the policy (allchar lane, it 7806): the build drops runs
    past `STAT_MAX`, and the restore clamps gold to it."""
    pt = {"player_stats": [{"current_gold": 99, "current_hp": 50, "max_hp": 80}]}
    run = {
        "game_mode": "standard",
        "players": [{"character": next(iter(human.CHARACTER_OF)), "deck": []}],
        "map_point_history": [[pt, pt]],
    }
    assert human.run_rejection(run) is None
    rich = {"player_stats": [{"current_gold": 959_999_000, "current_hp": 50, "max_hp": 80}]}
    assert human.run_rejection({**run, "map_point_history": [[pt, rich]]}) == "implausible_stats"
    rm = factory.new_run(SEED, character="Silent")
    human.apply_start(rm, {**START, "gold": 959_999_000}, SEED)
    assert rm.gold == human.STAT_MAX
