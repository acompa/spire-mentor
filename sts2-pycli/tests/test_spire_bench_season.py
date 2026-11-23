"""Spire Bench milestone 3: season commit-reveal and the static leaderboard."""

from __future__ import annotations

import hashlib
import io
import json
import random

import pytest
from spire_bench import season as season_mod
from spire_bench.client import SpireBench, VecEnv
from spire_bench.gateway import Config
from spire_bench.leaderboard import build, judge
from spire_bench.season import (
    SeasonError,
    check_reveal,
    episode_counts,
    fetch_randomness,
    make_commitment,
    make_reveal,
    open_season,
    season_id,
)
from spire_bench.verify import _against_commitment
from test_spire_bench_gateway import TOKENS, _play_out, gateway

LIMITS = {"decision_deadline": 600.0, "run_deadline": 3600.0, "max_invalid": 20}
RANDOMNESS = hashlib.sha256(b"round").digest()


def _commit() -> tuple[dict, bytes]:
    return make_commitment("s-test", "chain", 1000, "v0.111.0", LIMITS, engine="e" * 64)


# ── game-free ───────────────────────────────────────────────────────────────


def test_the_season_key_needs_the_committed_k_and_depends_on_the_beacon():
    commitment, k = _commit()
    key, sid = open_season(commitment, k, RANDOMNESS)
    assert len(key) == 32 and sid == season_id(commitment)
    other, _ = open_season(commitment, k, hashlib.sha256(b"other round").digest())
    assert other != key
    with pytest.raises(SeasonError, match="does not match"):
        open_season(commitment, bytes(32), RANDOMNESS)
    # Changing anything in the commitment changes the season id.
    assert season_id({**commitment, "limits": {**LIMITS, "max_invalid": 21}}) != sid


def test_a_reveal_with_another_k_does_not_check():
    commitment, k = _commit()
    reveal = make_reveal(commitment, k, RANDOMNESS, {"alpha": 1})
    key, sid, _ = check_reveal(reveal)
    assert (key, sid) == open_season(commitment, k, RANDOMNESS)
    with pytest.raises(SeasonError):
        check_reveal({**reveal, "k": bytes(32).hex()})
    with pytest.raises(SeasonError):
        make_reveal(commitment, bytes(32), RANDOMNESS, {})


def test_drand_randomness_must_be_the_hash_of_the_rounds_signature(monkeypatch):
    sig = bytes(range(48))
    beacon = {"round": 7, "signature": sig.hex(), "randomness": hashlib.sha256(sig).hexdigest()}
    urls = []

    def fake(url: str, timeout: float) -> io.BytesIO:
        urls.append(url)
        return io.BytesIO(json.dumps(beacon).encode())

    monkeypatch.setattr(season_mod.urllib.request, "urlopen", fake)
    assert fetch_randomness("abc", 7) == hashlib.sha256(sig).digest()
    assert urls == [f"{season_mod.DRAND_API}/abc/public/7"]
    beacon["randomness"] = "00" * 32
    with pytest.raises(SeasonError, match="sha256"):
        fetch_randomness("abc", 7)
    beacon["round"] = 8
    with pytest.raises(SeasonError, match="round 8"):
        fetch_randomness("abc", 7)


def _result(agent: str, no: int, *, ok: bool = True, status: str = "won") -> dict:
    out = {"agent_id": agent, "episode_no": no, "character": "Silent", "ascension": 5, "ok": ok}
    return {**out, "status": status} if ok else {**out, "why": "fingerprint differs"}


def test_an_agent_is_scored_only_when_every_episode_is_there_and_verifies():
    rows, flagged = judge(
        [
            _result("alpha", 0),
            _result("alpha", 1, status="lost"),
            _result("alpha", 2, status="void"),
            _result("beta", 0),
            _result("beta", 2),  # episode 1 left out
            _result("gamma", 0),
            _result("gamma", 1, ok=False),
            _result("delta", 0),  # episode 1, the last one, left out
            _result("eps", 0),
            _result("eps", 1),  # beyond the reveal's count
            _result("zeta", 0),  # not in the reveal
            {"ok": False, "why": "no header", "path": "x.jsonl"},
        ],
        {"alpha": 3, "beta": 3, "gamma": 2, "delta": 2, "eps": 1, "eta": 1},
    )
    assert [(r["agent_id"], r["won"], r["finished"], r["void"]) for r in rows] == [
        ("alpha", 1, 2, 1)
    ]
    assert flagged["beta"] == ["1 episode(s) missing: 1"]
    assert flagged["gamma"] == ["episode 1: fingerprint differs"]
    assert flagged["delta"] == ["1 episode(s) missing: 1"]
    assert flagged["eps"] == ["1 episode(s) beyond the reveal's count 1"]
    assert flagged["zeta"] == ["the reveal does not count this agent's episodes"]
    assert flagged["eta"] == ["1 episode(s) missing: 0"]
    assert flagged["(unreadable)"] == ["x.jsonl: no header"]


def test_the_reveal_counts_each_agents_episodes_from_the_gateway_index(tmp_path):
    rows = [
        {"type": "start", "agent_id": "a", "episode_no": 0},
        {"type": "start", "agent_id": "b", "episode_no": 0},
        {"type": "end", "episode_id": "SB-a-000000"},
        {"type": "start", "agent_id": "a", "episode_no": 1},
    ]
    (tmp_path / "episodes.jsonl").write_text(
        "".join(json.dumps(r) + "\n" for r in rows), encoding="utf-8"
    )
    assert episode_counts(tmp_path) == {"a": 2, "b": 1}
    rows.append({"type": "start", "agent_id": "b", "episode_no": 2})
    (tmp_path / "episodes.jsonl").write_text(
        "".join(json.dumps(r) + "\n" for r in rows), encoding="utf-8"
    )
    with pytest.raises(SeasonError, match="gap"):
        episode_counts(tmp_path)


def test_a_header_must_run_under_the_committed_parameters():
    commitment, _ = _commit()
    header = {"game_version": "v0.111.0", "engine_hash": "e" * 64, "limits": dict(LIMITS)}
    assert _against_commitment(header, commitment) is None
    assert _against_commitment({**header, "game_version": "v0.113.0"}, commitment)
    assert _against_commitment({**header, "engine_hash": "f" * 64}, commitment)
    looser = {**header, "limits": {**LIMITS, "decision_deadline": 6000.0}}
    assert "limits" in _against_commitment(looser, commitment)


# ── end to end: commit → season server → bots → reveal → leaderboard ────────


@pytest.mark.oracle
def test_a_season_runs_from_commitment_to_a_verified_leaderboard(tmp_path):
    import sts2_client

    sdir = tmp_path / "season"
    sdir.mkdir()
    commitment, k = make_commitment("s-e2e", "chain", 1000, sts2_client.GAME_VERSION, LIMITS)
    (sdir / "K.secret").write_text(k.hex() + "\n", encoding="utf-8")
    (sdir / "commitment.json").write_text(json.dumps(commitment), encoding="utf-8")
    bench_json = tmp_path / "bench.json"
    bench_json.write_text(
        json.dumps(
            {
                "season": {"dir": str(sdir), "drand_randomness": RANDOMNESS.hex()},
                "agents": TOKENS,
                "state_dir": str(tmp_path / "state"),
            }
        ),
        encoding="utf-8",
    )
    config = Config.load(bench_json)
    assert config.season_id == season_id(commitment) and config.limits.max_invalid == 20

    with gateway(tmp_path, config=config) as (url, _):
        receipts = {}
        for token, n in (("tok-alpha", 2), ("tok-beta", 2)):
            bench = SpireBench(url, token)
            assert bench.info()["season_id"] == config.season_id
            env = VecEnv(bench, "Silent", 0, n)
            env.reset()
            _play_out(env, random.Random(n))
            receipts |= env.receipts()

    transcripts = sorted((tmp_path / "state" / "transcripts").glob("*.jsonl"))
    assert len(transcripts) == 4
    reveal = tmp_path / "reveal.json"
    counts = episode_counts(tmp_path / "state")
    assert counts == {"alpha": 2, "beta": 2}
    reveal.write_text(json.dumps(make_reveal(commitment, k, RANDOMNESS, counts)), encoding="utf-8")

    site = tmp_path / "site"
    board = build(reveal, site, transcripts)
    assert {r["agent_id"]: r["finished"] for r in board["scores"]} == {"alpha": 2, "beta": 2}
    assert board["not_scored"] == {}
    assert {e["episode_id"]: e["fingerprint"] for e in board["episodes"]} == receipts
    page = (site / "index.html").read_text(encoding="utf-8")
    assert "s-e2e" in page and "alpha" in page
    assert {p.name for p in site.iterdir()} >= {"index.html", "leaderboard.json", "reveal.json"}

    # Leaving out beta's last episode keeps beta off the board.
    kept = [t for t in transcripts if t.name != "SB-beta-000001.jsonl"]
    board = build(reveal, tmp_path / "site2", kept)
    assert [r["agent_id"] for r in board["scores"]] == ["alpha"]
    assert board["not_scored"]["beta"] == ["1 episode(s) missing: 1"]

    # A reveal with another beacon value gives another key: nothing replays.
    other = tmp_path / "other.json"
    bad = make_reveal(commitment, k, hashlib.sha256(b"x").digest(), counts)
    other.write_text(json.dumps(bad), encoding="utf-8")
    board = build(other, tmp_path / "site3", transcripts)
    assert board["scores"] == [] and set(board["not_scored"]) == {"alpha", "beta"}
