"""A Blitz run saved mid-run comes back as the same run (GH #1347).

A workbench save is the active line (the run identity plus every action from
the root) and loading replays it, so it is only as good as the engine's
determinism over the Blitz-only state: the merchant's rolled wares and
remaining picks, and the chest's second round. Three positions are saved --
mid-fight, at the merchant with picks left, and in the chest between its two
picks -- and each must come back identical through a named save and through
a seek to the root and back, then play on identically.

Twin of the Rust port's ``tests/blitz_saves.rs``.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import pytest
from fastapi.testclient import TestClient
from workbench.server import create_app

V113 = "v0.113.0"
#: Actions played on after each rebuild.
PLAY_ON = 12


def _client(tmp_path: Path) -> TestClient:
    images = tmp_path / "images"
    images.mkdir(exist_ok=True)
    logs = tmp_path / "logs"
    logs.mkdir(exist_ok=True)
    return TestClient(create_app(images_root=images, logs_dir=logs))


class _Player:
    """The test's player. It needs no AI: it only has to get through two
    hallway fights and an elite on some seed, and to be a pure function of
    the step, so the same step always yields the same action."""

    def __init__(self) -> None:
        # Wares already asked for at a floor's merchant: a refused one stays
        # on offer and must not be asked for again.
        self.tried: set[tuple[int, int]] = set()

    def choose(self, step: dict[str, Any]) -> tuple[str, dict[str, Any]]:
        state = step["state"]
        options = [o for o in state.get("options") or [] if o.get("enabled", True)]
        decision = step["decision"]
        if decision == "combat_play":
            playable = [c for c in state.get("hand") or [] if c.get("can_play")]
            aimed = [c for c in playable if c.get("target_type") == "AnyEnemy"]
            if not playable:
                return "end_turn", {}
            card = (aimed or playable)[0]
            args: dict[str, Any] = {"card_index": card["index"]}
            if card.get("target_type") == "AnyEnemy":
                weakest = min(state["enemies"], key=lambda e: e.get("hp") or 0)
                args["target_index"] = weakest["index"]
            return "play_card", args
        if decision == "card_select":
            n = max(1, state.get("min_select") or 1)
            return "select_cards", {"indices": ",".join(str(i) for i in range(n))}
        if decision == "shop":
            floor = state.get("floor") or 0
            if (state.get("blitz_choices_remaining") or 0) > 0:
                for o in options:
                    key = (floor, (o.get("args") or {}).get("index", -1))
                    if o["action"] == "blitz_choice" and key not in self.tried:
                        self.tried.add(key)
                        return o["action"], o["args"]
            return "leave_room", {}
        if options:
            return options[0]["action"], options[0]["args"]
        return "proceed", {}


def _act(c: TestClient, action: str, args: dict[str, Any]) -> dict[str, Any]:
    r = c.post("/api/live/action", json={"action": action, "args": args})
    assert r.status_code == 200, r.text
    assert r.json()["error"] is None, (action, args, r.json()["error"])
    return r.json()["step"]


def _live_state(c: TestClient) -> dict[str, Any]:
    r = c.get("/api/live/state")
    assert r.status_code == 200, r.text
    return r.json()["state"]


def _engine(c: TestClient) -> dict[str, Any]:
    """The engine's own state, under the enrichment."""
    (session,) = list(c.app.state.sessions._sessions.values())
    return session.live.game.get_state()


def _check_position(
    c: TestClient, player: _Player, step: dict[str, Any], where: str
) -> dict[str, Any]:
    """Save where the run stands, play on, then rebuild the position twice
    (a named save, a seek to the root and back) and replay the same actions:
    every state must match the first pass."""
    at_save = _live_state(c)
    engine_at_save = _engine(c)
    assert at_save["game_mode"] == "blitz"
    assert at_save["decision"] == step["decision"], where
    name = where.replace(" ", "-")
    assert c.post("/api/live/save", json={"name": name}).status_code == 200
    leaf = c.get("/api/live/timeline").json()["active_leaf"]

    # First pass: play on, remembering every action and the state it gave.
    tried = set(player.tried)
    played: list[tuple[str, dict[str, Any], dict[str, Any], dict[str, Any]]] = []
    cur = step
    for _ in range(PLAY_ON):
        if cur["decision"] == "game_over":
            break
        action, args = player.choose(cur)
        cur = _act(c, action, args)
        played.append((action, args, cur["state"], _engine(c)))
    after = cur

    def replay(how: str) -> None:
        assert _live_state(c) == at_save, f"{where}: {how}"
        assert _engine(c) == engine_at_save, f"{where}: {how} (engine)"
        for i, (action, args, want, want_engine) in enumerate(played):
            got = _act(c, action, args)
            at = f"{where}: {how}, {i} actions on ({action} {args})"
            assert got["state"] == want, at
            assert _engine(c) == want_engine, f"{at} (engine)"

    # A checkpoint: seek to the root, then back to the saved node.
    tl = c.get("/api/live/timeline").json()
    assert c.post("/api/live/seek", json={"node_id": tl["nodes"][0]["id"]}).status_code == 200
    r = c.post("/api/live/seek", json={"node_id": leaf})
    assert r.status_code == 200, r.text
    replay("seek")

    # A named save, loaded over a standard run.
    r = c.post(
        "/api/live/start",
        json={"character": "Ironclad", "seed": "OTHER", "backend": "rust", "game_version": V113},
    )
    assert r.status_code == 200, r.text
    r = c.post("/api/saves/load", json={"name": name})
    assert r.status_code == 200, r.text
    assert r.json()["state"]["game_mode"] == "blitz"
    replay("save")

    player.tried = tried | player.tried
    return after


def _walk(tmp_path: Path, seed: str, level: int) -> set[str]:
    """Walk one run to the chest's second round, checking each position on
    the way; returns the positions it reached (fewer if the player dies)."""
    saved: set[str] = set()
    player = _Player()
    c = _client(tmp_path / seed)
    r = c.post(
        "/api/live/start",
        json={
            "character": "Ironclad",
            "seed": seed,
            "backend": "rust",
            "act1": "roll",
            "ascension": level,
            "game_version": V113,
            "game_mode": "blitz",
        },
    )
    assert r.status_code == 200, r.text
    step = r.json()
    fight_actions = 0
    for _ in range(2000):
        state = step["state"]
        decision = step["decision"]
        if decision == "game_over":
            return saved
        if decision == "combat_play" and "fight" not in saved:
            # Mid-fight and mid-turn: two cards into the first fight.
            if fight_actions == 2:
                saved.add("fight")
                step = _check_position(c, player, step, f"{seed} fight")
                continue
            fight_actions += 1
        elif (
            decision == "shop"
            and "merchant" not in saved
            and (state.get("blitz_choices_remaining") or 0) > 0
            and any(w["status"] == "Used" for w in state.get("blitz_wares") or [])
        ):
            # The merchant with a ware taken and picks left.
            saved.add("merchant")
            step = _check_position(c, player, step, f"{seed} merchant")
            continue
        elif decision == "rewards" and "chest" not in saved and state.get("chest_picks_left") == 1:
            # The chest between its two picks.
            saved.add("chest")
            _check_position(c, player, step, f"{seed} chest")
            return saved
        action, args = player.choose(step)
        step = _act(c, action, args)
    pytest.fail(f"{seed}: the walk never reached the chest: {saved}")


def test_a_blitz_run_saved_mid_run_reloads_and_plays_on_identically(tmp_path):
    pytest.importorskip("sts2_rs")
    reached: set[str] = set()
    # The player is no strategist: take the first seeds it survives on.
    for seed, level in (
        ("BLITZSAVE1", 0),
        ("BLITZSAVE2", 3),
        ("BLITZSAVE3", 5),
        ("BLITZSAVE4", 0),
        ("BLITZSAVE5", 0),
        ("BLITZSAVE6", 0),
    ):
        (tmp_path / seed).mkdir()
        reached |= _walk(tmp_path, seed, level)
        if reached == {"fight", "merchant", "chest"} and level > 0:
            break
    assert reached == {"fight", "merchant", "chest"}, reached
