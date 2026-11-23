"""A chest the AI walks into shows what it held (GH #1438).

The engine opens a treasure room on entry and jumps to the map, so the only
trace of the chest is ``room_reveal`` on the next state. It used to be computed
only for hand-played actions (``RustGame.act`` takes the pre-action snapshot):
every AI move (Let AI pick, AI plays, autobattle, native replay) went straight
to ``rm.take_action``, so a run the AI played passed its chests in silence.

The Python twin of ``sts2-rs/crates/sts2-workbench/tests/treasure_reveal_ai.rs``.
"""

from __future__ import annotations

from typing import Any

import pytest

pytest.importorskip("sts2_rs")

from fastapi.testclient import TestClient
from workbench.branching import BranchingLiveSession
from workbench.server import create_app

#: Act 1's chest stands mid-act; floor 12 is past it on every path.
LAST_FLOOR = 12
#: The Rust test's seed. Small search budgets keep the walk to a few seconds;
#: the agent is deterministic at a fixed budget, so the walk is too.
SEED = 7
SIMS, BOSS_SIMS = 48, 96


def _client(tmp_path) -> TestClient:
    images = tmp_path / "images"
    images.mkdir()
    logs = tmp_path / "logs"
    logs.mkdir()
    return TestClient(create_app(images_root=images, logs_dir=logs))


def _assert_a_chest_with_a_relic(reveal: dict[str, Any]) -> None:
    assert reveal["kind"] == "treasure"
    assert isinstance((reveal.get("relic") or {}).get("name"), str), f"no relic: {reveal}"


def test_an_ai_played_run_reveals_its_act_one_chest(tmp_path) -> None:
    c = _client(tmp_path)
    r = c.post(
        "/api/live/start-faithful",
        json={
            "character": "Ironclad",
            "seed": str(SEED),
            "ascension": 0,
            "sims": SIMS,
            "boss_sims": BOSS_SIMS,
        },
    )
    assert r.status_code == 200, r.text
    step = r.json()
    for _ in range(3000):
        state = step["state"]
        reveal = state.get("room_reveal")
        if reveal is not None:
            _assert_a_chest_with_a_relic(reveal)
            return
        assert step.get("decision") != "game_over", "the run ended before its chest"
        assert (state.get("floor") or 0) <= LAST_FLOOR, (
            f"the AI passed floor {LAST_FLOOR} without a chest reveal"
        )
        r = c.post("/api/live/run-mode-step", json={})
        assert r.status_code == 200, r.text
        body = r.json()
        assert body["error"] is None, f"run-mode step failed: {body['error']}"
        step = body["step"]
    pytest.fail("no chest reveal in 3000 AI steps")


def _walk_to_the_chest(sess: BranchingLiveSession) -> int:
    """Run-mode-step until a state reveals a chest; return that node's id."""
    for _ in range(3000):
        out = sess.run_mode_step()
        assert out["error"] is None, out["error"]
        assert not out["over"], "the run ended before its chest"
        if out["state"].get("room_reveal") is not None:
            assert sess._active_id is not None
            return sess._active_id
    pytest.fail("no chest reveal in 3000 AI steps")


def test_replaying_an_ai_line_reveals_the_chest_again(tmp_path) -> None:
    # The native replay paths (``seek`` and a save's ``load_line``) re-apply the
    # AI's recorded moves through the backend too, so landing on the chest node
    # shows the same panel the live step did.
    sess = BranchingLiveSession(logs_dir=str(tmp_path), backend="rust", loc={})
    sess.start_faithful("Ironclad", SEED, sims=SIMS, boss_sims=BOSS_SIMS, threads=1)
    chest = _walk_to_the_chest(sess)
    live = sess._nodes[chest].state["room_reveal"]
    _assert_a_chest_with_a_relic(live)
    line, _ = sess.active_line()
    # Step past the chest so the seek has somewhere to come back from.
    sess.run_mode_step()
    assert sess.seek(chest).get("room_reveal") == live

    restored = BranchingLiveSession(logs_dir=str(tmp_path / "restore"), backend="rust", loc={})
    state = restored.load_line("Ironclad", str(SEED), 0, line, run_mode=True)
    assert state.get("room_reveal") == live
