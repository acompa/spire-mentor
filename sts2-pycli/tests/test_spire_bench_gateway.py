"""Spire Bench milestone 2: the gateway, end to end over HTTP on real hosts."""

from __future__ import annotations

import asyncio
import json
import random
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any

import pytest
import uvicorn
from spire_bench.client import BenchError, SpireBench, VecEnv, random_action
from spire_bench.episode import Episode, Limits
from spire_bench.gateway import Bench, Config, create_app
from spire_bench.keys import episode_key
from spire_bench.scoring import score, wilson
from spire_bench.verify import _check_outcome, _trigger_shape, verify, verify_all
from spire_bench.worker import WorkerError

SEASON_KEY = bytes(range(32))
TOKENS = {"tok-alpha": "alpha", "tok-beta": "beta"}


@contextmanager
def gateway(tmp_path: Path, **overrides: Any) -> Iterator[tuple[str, Config]]:
    limits = Limits(
        decision_deadline=overrides.pop("decision_deadline", 600.0),
        run_deadline=overrides.pop("run_deadline", 3600.0),
        max_invalid=overrides.pop("max_invalid", 20),
    )
    config = overrides.pop("config", None) or Config(
        season_key=SEASON_KEY,
        agents=dict(TOKENS),
        state_dir=tmp_path / "state",
        limits=limits,
        reaper_interval=0.2,
        **overrides,
    )
    server = uvicorn.Server(
        uvicorn.Config(create_app(config), host="127.0.0.1", port=0, log_level="warning", ws="none")
    )
    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    deadline = time.time() + 30
    while not server.started:
        if time.time() > deadline or not thread.is_alive():
            raise RuntimeError("gateway did not start")
        time.sleep(0.05)
    port = server.servers[0].sockets[0].getsockname()[1]
    try:
        yield f"http://127.0.0.1:{port}", config
    finally:
        server.should_exit = True
        thread.join(30)


# ── game-free ───────────────────────────────────────────────────────────────


def test_episode_keys_are_per_agent_and_per_episode():
    a0 = episode_key(SEASON_KEY, "alpha", 0)
    assert a0 == episode_key(SEASON_KEY, "alpha", 0)
    assert len({a0, episode_key(SEASON_KEY, "alpha", 1), episode_key(SEASON_KEY, "beta", 0)}) == 3
    assert len(bytes.fromhex(a0)) == 32
    with pytest.raises(ValueError):
        episode_key(b"short", "alpha", 0)


def test_scores_pool_every_finished_episode_and_leave_out_void_ones():
    eps = [
        {"agent_id": "a", "character": "Ironclad", "ascension": 10, "status": s}
        for s in ("won", "lost", "lost", "lost", "void", "running")
    ]
    (row,) = score(eps)
    assert (row["finished"], row["won"], row["void"], row["running"]) == (4, 1, 1, 1)
    assert row["win_rate"] == pytest.approx(0.25)
    lo, hi = row["wilson_95"]
    assert 0 < lo < 0.25 < hi < 1
    assert wilson(0, 0) == (0.0, 1.0)


# ── with the referee host ───────────────────────────────────────────────────

pytestmark_oracle = pytest.mark.oracle


def _play_out(env: VecEnv, rnd: random.Random, max_steps: int = 5000) -> None:
    obs = env.observations()
    for _ in range(max_steps):
        if env.all_done():
            return
        obs = env.step([random_action(o, rnd) if o else None for o in obs])
    raise AssertionError("episodes did not finish")


@pytestmark_oracle
def test_a_random_bot_plays_episodes_to_the_end_and_each_is_scored_and_verifiable(tmp_path):
    with gateway(tmp_path) as (url, config):
        bench = SpireBench(url, "tok-alpha")
        info = bench.info()
        assert info["protocol"] and len(info["engine_hash"]) == 64
        env = VecEnv(bench, "Ironclad", 0, 3)
        first = env.reset()
        assert all(o is not None and o["legal_actions"] for o in first)
        _play_out(env, random.Random(1))
        assert {s["status"] for s in env.states} <= {"won", "lost"}
        assert all(s["outcome"]["reason"] == "game_over" for s in env.states)
        receipts = env.receipts()
        (row,) = bench.scores()
        assert (row["agent_id"], row["finished"], row["running"]) == ("alpha", 3, 0)
    transcripts = sorted((config.state_dir / "transcripts").glob("*.jsonl"))
    assert len(transcripts) == 3
    rows = [json.loads(line) for line in transcripts[0].read_text(encoding="utf-8").splitlines()]
    assert rows[0]["type"] == "header" and rows[-1]["type"] == "end"
    assert all("rng_key" not in json.dumps(r) for r in rows)
    # Once the season key is out, anyone can replay the episode.
    results = asyncio.run(verify_all(transcripts, SEASON_KEY))
    assert all(r["ok"] and r["status"] in ("won", "lost") for r in results), results
    # Each agent's last receipt is the replayed fingerprint of its episode.
    assert {r["episode_id"]: r["fingerprint"] for r in results} == receipts
    # A wrong key does not verify.
    assert not asyncio.run(verify(transcripts[0], bytes(32)))["ok"]

    def forged(name: str, edit: Any) -> Path:
        copy = [json.loads(json.dumps(r)) for r in rows]
        edit(copy)
        path = tmp_path / f"{name}.jsonl"
        path.write_text("".join(json.dumps(r) + "\n" for r in copy), encoding="utf-8")
        return path

    def why(path: Path) -> str:
        result = asyncio.run(verify(path, SEASON_KEY))
        assert not result["ok"], result
        return result["why"]

    # An edited reply hash.
    assert "differs" in why(forged("reply", lambda r: r[-2].update(reply_sha256="0" * 64)))
    # A loss claimed as a win (the fingerprint is untouched).
    assert "won without" in why(forged("won", lambda r: r[-1].update(status="won")))
    # A header that names another character, or another agent's episode.
    assert "character" in why(forged("char", lambda r: r[0].update(character="Defect")))
    assert "label" in why(forged("agent", lambda r: r[0].update(agent_id="beta")))
    # A second start_run in the middle.
    assert "start_run" in why(forged("restart", lambda r: r.insert(2, r[1])))
    # Two transcripts of one episode: a publisher could keep the better run.
    # Relabelling the finished run as a restart void does not verify.
    assert "after the game ended" in why(
        forged("void", lambda r: r[-1].update(status="void", reason="server_restart"))
    )

    # Cutting it short and calling it a restart void does: the transcript alone
    # cannot tell. The agent's receipt can.
    def truncate(r: list) -> None:
        del r[-6:-1]
        r[-1] = {"type": "end", "status": "void", "reason": "server_restart"}

    cut = asyncio.run(verify(forged("cut", truncate), SEASON_KEY))
    assert cut["ok"] and cut["fingerprint"] != receipts[rows[0]["episode_id"]]
    dup = asyncio.run(verify_all([transcripts[0], transcripts[0]], SEASON_KEY))
    assert dup[0]["ok"] and not dup[1]["ok"] and "same episode" in dup[1]["why"]


def _first_legal(state: dict) -> dict:
    return random_action(state["observation"], random.Random(0))


@pytestmark_oracle
def test_a_resent_action_gets_the_same_reply_and_a_stale_seq_is_refused(tmp_path):
    with gateway(tmp_path) as (url, _):
        bench = SpireBench(url, "tok-alpha")
        (ep,) = bench.start("Silent", 0)
        act = {"episode_id": ep["episode_id"], "seq": ep["seq"], **_first_legal(ep)}
        (once,) = bench.step([act])
        assert "error" not in once and once["seq"] == ep["seq"] + 1
        (again,) = bench.step([act])
        assert again == once
        stale = {**act, "seq": ep["seq"] + 5}
        (refused,) = bench.step([stale])
        assert refused["error"]["code"] == "stale_seq" and refused["seq"] == once["seq"]
        (_dup, dup2) = bench.step([{**act, "seq": once["seq"]}, {**act, "seq": once["seq"]}])
        assert dup2["error"]["code"] == "duplicate_in_batch"


@pytestmark_oracle
def test_a_refused_action_keeps_the_decision_and_too_many_lose_the_episode(tmp_path):
    with gateway(tmp_path, max_invalid=3) as (url, config):
        bench = SpireBench(url, "tok-alpha")
        (ep,) = bench.start("Defect", 0)
        bad = {"episode_id": ep["episode_id"], "seq": ep["seq"], "action": "no_such_action"}
        for _ in range(2):
            (r,) = bench.step([bad])
            assert r["status"] == "running" and r["error"]["code"]
            assert r["seq"] == ep["seq"] and r["observation"] == ep["observation"]
        (r,) = bench.step([bad])
        assert r["status"] == "lost" and r["outcome"]["reason"] == "invalid_actions"
        assert bench.scores()[0]["finished"] == 1
    (path,) = (config.state_dir / "transcripts").glob("*.jsonl")
    got = asyncio.run(verify(path, SEASON_KEY))
    assert got["ok"] and got["fingerprint"] == r["fingerprint"], got
    # The limit is in the header; claiming a looser one does not verify.
    rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
    # The season's published limit is enforced when given.
    assert asyncio.run(verify(path, SEASON_KEY, max_invalid=3))["ok"]
    assert "season" in asyncio.run(verify(path, SEASON_KEY, max_invalid=5))["why"]
    # The header is in the chain: claiming a looser limit breaks the
    # fingerprint, and recomputing that breaks the agent's receipt.
    rows[0]["limits"]["max_invalid"] = 4
    path.write_text("".join(json.dumps(x) + "\n" for x in rows), encoding="utf-8")
    assert asyncio.run(verify(path, SEASON_KEY))["why"] == "fingerprint differs"
    rows[-1]["fingerprint"] = asyncio.run(verify(path, SEASON_KEY))["fingerprint"]
    path.write_text("".join(json.dumps(x) + "\n" for x in rows), encoding="utf-8")
    regenerated = asyncio.run(verify(path, SEASON_KEY))
    assert regenerated["fingerprint"] != r["fingerprint"]


@pytestmark_oracle
def test_an_agent_that_goes_silent_loses_at_the_decision_deadline(tmp_path):
    with gateway(tmp_path, decision_deadline=1.0) as (url, _):
        bench = SpireBench(url, "tok-alpha")
        (ep,) = bench.start("Regent", 0)
        deadline = time.time() + 15
        while bench.episode(ep["episode_id"])["status"] == "running":
            assert time.time() < deadline
            time.sleep(0.2)
        got = bench.episode(ep["episode_id"])
        assert got["status"] == "lost" and got["outcome"]["reason"] == "decision_deadline"


@pytestmark_oracle
def test_tokens_episodes_and_worker_slots_are_enforced(tmp_path):
    with gateway(tmp_path, max_workers=2) as (url, _):
        with pytest.raises(BenchError) as exc:
            SpireBench(url, "nope").start("Ironclad", 0)
        assert exc.value.status == 401
        alpha, beta = SpireBench(url, "tok-alpha"), SpireBench(url, "tok-beta")
        (ep,) = alpha.start("Necrobinder", 0)
        # Another agent can neither see nor play it.
        with pytest.raises(BenchError) as exc:
            beta.episode(ep["episode_id"])
        assert exc.value.status == 404
        (r,) = beta.step([{"episode_id": ep["episode_id"], "seq": 0, "action": "end_turn"}])
        assert r["error"]["code"] == "unknown_episode"
        with pytest.raises(BenchError) as exc:
            beta.start("Ironclad", 0, count=2)
        assert exc.value.status == 429
        with pytest.raises(BenchError) as exc:
            alpha.start("Watcher", 0)
        assert exc.value.status == 400


def test_episodes_a_previous_server_left_running_are_void(tmp_path):
    state = tmp_path / "state"
    (state / "transcripts").mkdir(parents=True)
    start = {"type": "start", "episode_id": "SB-alpha-000000", "agent_id": "alpha", "episode_no": 0}
    start |= {"character": "Ironclad", "ascension": 0, "t": 0}
    (state / "episodes.jsonl").write_text(json.dumps(start) + "\n", encoding="utf-8")
    config = Config(season_key=SEASON_KEY, agents=dict(TOKENS), state_dir=state)
    try:
        bench = Bench(config)
    except FileNotFoundError:
        pytest.skip("host not built")
    assert bench.records["SB-alpha-000000"]["status"] == "void"
    assert bench.next_no["alpha"] == 1
    # The void is written down, so a second restart reads it back.
    assert Bench(config).records["SB-alpha-000000"]["status"] == "void"


class _CrashingWorker:
    """Answers start_run, then dies on the first action."""

    world_errors = 0

    def __init__(self) -> None:
        self.closed = False

    async def send(self, cmd: dict) -> dict:
        if cmd["cmd"] == "start_run":
            return {"type": "decision", "decision": "map_select", "legal_actions": []}
        raise WorkerError("host exited", "exited")

    async def close(self) -> None:
        self.closed = True


def test_an_action_that_crashes_the_host_loses_the_episode_and_names_the_action(tmp_path):
    # Voiding it would let an agent that found a crashing action wipe a run it
    # is losing.
    path = tmp_path / "t.jsonl"
    ep = Episode("SB-a-000000", "a", 0, "Ironclad", 0, "SB-a-000000", Limits(), path)

    async def run() -> tuple[str, dict]:
        ep.open_transcript("v0.111.0", "e" * 64)
        await ep.start(_CrashingWorker(), "00" * 32)  # type: ignore[arg-type]
        before = ep.fingerprint
        return before, await ep.step(0, "select_map_node", {"col": 1, "row": 0})

    before, out = asyncio.run(run())
    assert out["status"] == "lost" and out["outcome"]["reason"] == "worker_error_during_action"
    end = json.loads(path.read_text(encoding="utf-8").splitlines()[-1])
    assert end["trigger"] == {
        "cmd": "action",
        "action": "select_map_node",
        "args": {"col": 1, "row": 0},
    }
    assert (end["detail"], end["failure"]) == ("host exited", "exited")
    # The trigger is in the chain: the agent's last receipt covers it.
    assert out["fingerprint"] == end["fingerprint"] != before


def test_a_trigger_must_be_a_plain_agent_action():
    assert _trigger_shape({"cmd": "action", "action": "end_turn"}) is None
    assert (
        _trigger_shape({"cmd": "action", "action": "play_card", "args": {"card_index": 0}}) is None
    )
    for bad in (
        None,
        {"cmd": "start_run", "character": "Ironclad"},
        {"cmd": "action", "action": "end_turn", "rng_key": "00"},
        {"cmd": "action", "action": 3},
        {"cmd": "action", "action": "end_turn", "args": "x"},
    ):
        assert _trigger_shape(bad) is not None, bad


def test_a_claimed_outcome_must_follow_from_the_replay():
    over_won = {"type": "decision", "decision": "game_over", "victory": True}
    over_lost = {"type": "decision", "decision": "game_over", "victory": False}
    mid = {"type": "decision", "decision": "combat_play"}
    err = {"type": "error", "code": "invalid"}
    fault = {"type": "referee_fault"}
    header = {"limits": {"max_invalid": 2}}
    start = {"cmd": {"cmd": "start_run"}}
    act = {"cmd": {"cmd": "action", "action": "end_turn"}}

    def check(status, reason, replies, *, trigger=None, failed=None, commands=None):
        end = {"status": status, "reason": reason}
        if trigger is not None:
            end["trigger"] = trigger
        cmds = commands if commands is not None else [start] + [act] * (len(replies) - 1)
        return _check_outcome(end, header, cmds, replies, failed)

    ok = [
        check("won", "game_over", [mid, over_won]),
        check("lost", "game_over", [mid, over_lost]),
        check("lost", "decision_deadline", [mid, mid]),
        check("lost", "invalid_actions", [mid, mid, err, err]),
        check("lost", "referee_fault_during_action", [mid, fault], trigger=act["cmd"]),
        check("lost", "worker_error_during_action", [mid], trigger=act["cmd"], failed=True),
        check("void", "worker_error", [], commands=[]),
        check("void", "referee_fault", [fault]),
        check("void", "server_restart", [mid, mid]),
        check("void", "server_shutdown", []),
    ]
    assert ok == [None] * len(ok), ok
    bad = [
        check("won", "game_over", [mid, over_lost]),
        check("won", "game_over", [mid, mid]),
        check("lost", "game_over", [mid, mid]),
        check("lost", "decision_deadline", [mid, over_won]),
        check("lost", "invalid_actions", [mid, mid, err]),
        check("lost", "invalid_actions", [mid, err, err, err]),
        check("lost", "referee_fault_during_action", [mid, mid], trigger=act["cmd"]),
        check("lost", "referee_fault_during_action", [mid, fault]),
        check("lost", "worker_error_during_action", [mid], trigger=act["cmd"], failed=False),
        check(
            "lost", "worker_error_during_action", [mid, over_won], trigger=act["cmd"], failed=True
        ),
        check("lost", "made_up", [mid]),
        check("void", "worker_error", [mid]),
        check("void", "referee_fault", [mid, fault]),
        check("void", "server_restart", [mid, over_won]),
        check("void", "made_up", [mid]),
    ]
    assert None not in bad, bad


def test_a_restart_keeps_an_outcome_the_transcript_already_holds(tmp_path):
    state = tmp_path / "state"
    (state / "transcripts").mkdir(parents=True)
    start = {"type": "start", "episode_id": "SB-alpha-000000", "agent_id": "alpha", "episode_no": 0}
    start |= {"character": "Ironclad", "ascension": 0, "t": 0}
    (state / "episodes.jsonl").write_text(json.dumps(start) + "\n", encoding="utf-8")
    end = {"type": "end", "status": "won", "reason": "game_over", "fingerprint": "f"}
    (state / "transcripts" / "SB-alpha-000000.jsonl").write_text(
        json.dumps({"type": "header"}) + "\n" + json.dumps(end) + "\n", encoding="utf-8"
    )
    config = Config(season_key=SEASON_KEY, agents=dict(TOKENS), state_dir=state)
    try:
        bench = Bench(config)
    except FileNotFoundError:
        pytest.skip("host not built")
    assert bench.records["SB-alpha-000000"]["status"] == "won"
    lines = (
        (state / "transcripts" / "SB-alpha-000000.jsonl").read_text(encoding="utf-8").splitlines()
    )
    assert len(lines) == 2  # no second end row
