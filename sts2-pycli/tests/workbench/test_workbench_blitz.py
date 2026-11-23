"""Blitz mode in the workbench (GH #1347).

The wire contract shared with the Rust ``sts2-workbench`` port
(``tests/blitz_smoke.rs`` is this file's twin):

* ``/api/live/start`` takes ``game_mode: "blitz"``. A Blitz run needs the
  Rust engine on v0.113.0 and an ``ascension`` on the Blitz ladder (0..5);
  any other combination is a 400 that says why. The run-mode and import
  routes build standard runs only.
* The live state carries ``game_mode: "blitz"`` (and no ``dominion``, which
  is Dread's); a standard run's state names neither.
* Every Blitz decision is a state the SPA draws: Neow's one option, the
  three-of-ten card grid, the one-node map, the merchant's six wares
  (``blitz_wares`` once enriched) and the chest's two rounds (``rewards``).
* Saves and the run-log meta carry ``game_mode`` only off standard.
* The AI routes answer 409 in a Blitz run.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

import pytest
from fake_game import FakeGame  # shared stub from tests/workbench/fake_game.py
from fastapi.testclient import TestClient
from workbench import saves as saves_mod
from workbench.live_runs import RunIdentity
from workbench.options import normalize_options
from workbench.render import describe_action
from workbench.server import _live_step, create_app
from workbench.timeline import TimelineStore

V111 = "v0.111.0"
V113 = "v0.113.0"
LINE = ["Monster", "Monster", "Shop", "Elite", "Treasure", "RestSite", "Boss"]
LOC_DIR = Path(__file__).resolve().parents[2] / f"localization_eng_{V113}"


def _client(tmp_path, monkeypatch=None) -> TestClient:
    if monkeypatch is not None:
        from workbench import branching

        monkeypatch.setattr(branching, "Game", FakeGame)
    images = tmp_path / "images"
    images.mkdir(exist_ok=True)
    logs = tmp_path / "logs"
    logs.mkdir(exist_ok=True)
    return TestClient(create_app(images_root=images, logs_dir=logs))


def _rust() -> None:
    pytest.importorskip("sts2_rs")


def _body(**extra) -> dict:
    return {
        "character": "Ironclad",
        "seed": "BLITZ",
        "backend": "rust",
        "act1": "overgrowth",
        "game_version": V113,
        "game_mode": "blitz",
        **extra,
    }


def _start(c: TestClient, **extra) -> dict:
    r = c.post("/api/live/start", json=_body(**extra))
    assert r.status_code == 200, r.text
    return r.json()


def _first_indices(n: int) -> str:
    return ",".join(str(i) for i in range(max(1, n)))


def _first_enabled_action(c: TestClient, step: dict) -> dict:
    """Take the first enabled option (a card select picks the cards it asks
    for), as the Rust twin's ``advance`` does."""
    if step["decision"] == "card_select":
        n = step["state"].get("min_select") or 1
        action, args = "select_cards", {"indices": _first_indices(n)}
    else:
        opts = [o for o in step["state"].get("options") or [] if o.get("enabled", True)]
        action, args = (opts[0]["action"], opts[0]["args"]) if opts else ("proceed", {})
    r = c.post("/api/live/action", json={"action": action, "args": args})
    assert r.status_code == 200, r.text
    assert r.json()["error"] is None, r.json()["error"]
    return r.json()["step"]


# ── validation ──────────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    ("route", "body", "needle"),
    [
        ("/api/live/start", _body(game_version=V111), V113),
        (
            "/api/live/start",
            _body(game_version="v0.107.1"),
            "Blitz mode needs game build v0.113.0",
        ),
        ("/api/live/start", _body(ascension=6), "ascension is the Blitz level 0..=5 (got 6)"),
        ("/api/live/start", _body(ascension=10), "ascension is the Blitz level 0..=5 (got 10)"),
        ("/api/live/start", _body(ascension=-1), "ascension is the Blitz level 0..=5"),
        ("/api/live/start", _body(game_mode="nightmare"), '"standard", "dread" or "blitz"'),
        (
            "/api/live/start-faithful",
            {"character": "Ironclad", "seed": "run_1", "game_version": V113, "game_mode": "blitz"},
            "Blitz mode is not available for a run-mode run",
        ),
        (
            "/api/live/import-state",
            {"payload": "{}", "game_version": V113, "game_mode": "blitz"},
            "Blitz mode is not available for an imported run",
        ),
    ],
)
def test_a_blitz_start_the_engine_cannot_build_is_a_400(tmp_path, route, body, needle):
    c = _client(tmp_path)
    r = c.post(route, json=body)
    assert r.status_code == 400, r.text
    assert needle in r.json()["detail"]
    # Nothing was started.
    assert c.get("/api/live/state").status_code != 200


def test_blitz_on_the_csharp_engine_is_a_400(tmp_path, monkeypatch):
    import sts2_client

    c = _client(tmp_path, monkeypatch)
    r = c.post(
        "/api/live/start",
        json=_body(backend="csharp", game_version=sts2_client.GAME_VERSION),
    )
    assert r.status_code == 400, r.text
    assert "Blitz mode requires the rust backend" in r.json()["detail"]


def test_check_game_mode_knows_the_three_modes():
    assert saves_mod.GAME_MODES == ("standard", "dread", "blitz")
    assert saves_mod.check_game_mode(None, "csharp", "v0.107.1") == "standard"
    assert saves_mod.check_game_mode("blitz", "rust", V113) == "blitz"
    assert saves_mod.check_game_mode("dread", "rust", V113) == "dread"
    with pytest.raises(ValueError, match=r"Blitz mode needs game build v0\.113\.0"):
        saves_mod.check_game_mode("blitz", "rust", V111)
    # Dread's words did not move.
    with pytest.raises(ValueError, match=r"Dread mode needs game build v0\.113\.0"):
        saves_mod.check_game_mode("dread", "rust", V111)
    with pytest.raises(ValueError, match="Dread mode requires the rust backend"):
        saves_mod.check_game_mode("dread", "csharp", V113)
    # Only Blitz has a ladder to fall off here.
    saves_mod.check_mode_ascension("blitz", 0)
    saves_mod.check_mode_ascension("blitz", 5)
    saves_mod.check_mode_ascension("standard", 20)
    saves_mod.check_mode_ascension("dread", 3)
    with pytest.raises(ValueError, match="got 6"):
        saves_mod.check_mode_ascension("blitz", 6)


# ── payload defaults ────────────────────────────────────────────────────────


def test_a_stored_payload_names_blitz():
    base = {"character": "Ironclad", "seed": "S", "ascension": 3}
    assert RunIdentity.from_save({**base, "game_mode": "blitz"}).game_mode == "blitz"
    rec = {**base, "backend": "rust", "act1": "roll", "run_mode": False}
    assert RunIdentity.from_recording({**rec, "game_mode": "blitz"}).game_mode == "blitz"


def test_write_save_names_blitz(tmp_path):
    kwargs = {
        "character": "Ironclad",
        "seed": "S",
        "ascension": 3,
        "backend": "rust",
        "run_mode": False,
        "progress": {},
        "line": [],
        "game_version": V113,
    }
    saves_mod.write_save(tmp_path, name="std", **kwargs)
    assert "game_mode" not in saves_mod.read_save(tmp_path, "std")
    saves_mod.write_save(tmp_path, name="blitz", game_mode="blitz", **kwargs)
    assert saves_mod.read_save(tmp_path, "blitz")["game_mode"] == "blitz"


# ── options and labels (no engine) ──────────────────────────────────────────


def _shop_state() -> dict[str, Any]:
    return {
        "decision": "shop",
        "cards": [],
        "relics": [],
        "potions": [],
        "card_removal_cost": None,
        "blitz_choices_remaining": 2,
        "options": [
            {
                "index": 0,
                "id": "leave_shop",
                "action": "leave_shop",
                "label": "leave_shop",
                "enabled": True,
            },
            {
                "index": 1,
                "action": "blitz_choice",
                "label": "blitz:Relic",
                "enabled": True,
                "status": "Available",
                "item": {"blitz_kind": "Relic", "title": "Relic", "cursed": False, "rare": False},
            },
            {
                "index": 2,
                "action": "blitz_choice",
                "label": "blitz:Potions",
                "enabled": False,
                "status": "Used",
                "item": {"blitz_kind": "Potions"},
            },
        ],
    }


def test_blitz_shop_lists_wares_then_leave():
    assert normalize_options(_shop_state()) == [
        {"label": "Relic", "action": "blitz_choice", "args": {"index": 1}, "enabled": True},
        {
            "label": "blitz:Potions",
            "action": "blitz_choice",
            "args": {"index": 2},
            "enabled": False,
        },
        {"label": "Leave shop", "action": "leave_room", "args": {}, "enabled": True},
    ]


def test_a_standard_shop_is_not_read_as_a_blitz_one():
    state = {"decision": "shop", "cards": [], "relics": [], "potions": []}
    assert normalize_options(state) == [
        {"label": "Leave shop", "action": "leave_room", "args": {}, "enabled": True}
    ]


def test_rewards_lists_each_reward_and_skip():
    state = {
        "decision": "rewards",
        "chest_picks_left": 1,
        "rewards": [{"index": 1, "type": "relic", "name": "Anchor"}],
    }
    assert normalize_options(state) == [
        {"label": "Anchor", "action": "take_reward", "args": {"reward_index": 1}, "enabled": True},
        {"label": "Skip", "action": "skip_reward", "args": {}, "enabled": True},
    ]


def test_a_rewards_screen_without_chest_picks_keeps_the_safety_net():
    state = {"decision": "rewards", "rewards": [{"index": 0, "type": "gold", "name": "Gold"}]}
    assert normalize_options(state) == [
        {"label": "Continue", "action": "proceed", "args": {}, "enabled": True}
    ]


def test_blitz_actions_are_described():
    shop = _shop_state()
    take = {"action": "blitz_choice", "args": {"index": 1}}
    assert describe_action(shop, take) == "Take Relic"
    # The enriched state lists the wares under their own key.
    enriched = {**shop, "options": [], "blitz_wares": [{"index": 1, "title": "Relic"}]}
    assert describe_action(enriched, take) == "Take Relic"
    assert describe_action(shop, {"action": "blitz_choice", "args": {"index": 9}}) == "Take ware 9"
    chest = {"decision": "rewards", "rewards": [{"index": 1, "type": "relic", "name": "Anchor"}]}
    assert (
        describe_action(chest, {"action": "take_reward", "args": {"reward_index": 1}})
        == "Take Anchor"
    )
    assert describe_action(chest, {"action": "skip_reward", "args": {}}) == "Leave the chest"


# ── the engine, over the API ────────────────────────────────────────────────


def test_a_blitz_start_names_its_mode_and_plays_its_level(tmp_path):
    _rust()
    c = _client(tmp_path)
    step = _start(c, ascension=3)
    state = step["state"]
    assert state["game_mode"] == "blitz"
    # Blitz has no dominion (the key is Dread's) and no chest reveal.
    assert "dominion" not in state
    assert "room_reveal" not in state
    # The flame shows the Blitz level, not a standard ascension.
    assert state["ascension"] == 3
    assert state["game_version"] == V113

    # Neow's one Blitz option, then exactly three of ten cards.
    assert step["decision"] == "event_choice"
    assert len(state["event_options"]) == 1
    assert len(state["options"]) == 1
    pick = _first_enabled_action(c, step)
    assert pick["decision"] == "card_select"
    s = pick["state"]
    assert len(s["cards"]) == 10
    assert (s["min_select"], s["max_select"], s["context"]) == (3, 3, "grant")
    deck = len(s["player"]["deck"])
    # The map is the fixed line: one node to go to.
    at_map = _first_enabled_action(c, pick)
    assert at_map["decision"] == "map_select"
    s = at_map["state"]
    assert len(s["player"]["deck"]) == deck + 3
    assert [(ch["col"], ch["row"], ch["type"]) for ch in s["choices"]] == [(3, 1, "Monster")]
    rows = c.get("/api/live/map").json()["graphs"][0]["full_map"]["rows"]
    # Seven rows of one node: the Ancient, then the six rooms (the boss is
    # the map's own `boss`).
    assert [len(r) for r in rows] == [1] * 7


@pytest.mark.parametrize("extra", [{"game_mode": "standard"}, {"game_mode": None}])
def test_a_standard_run_names_no_mode(tmp_path, extra):
    _rust()
    c = _client(tmp_path)
    step = _start(c, ascension=5, **extra)
    assert "game_mode" not in step["state"]
    assert "dominion" not in step["state"]
    assert c.post("/api/live/save", json={"name": "std"}).status_code == 200
    payload = saves_mod.read_save(saves_mod.save_dir(tmp_path / "logs"), "std")
    assert payload is not None and "game_mode" not in payload


def test_the_ai_routes_are_refused_in_a_blitz_run(tmp_path):
    _rust()
    c = _client(tmp_path)
    _start(c)
    for route in (
        "/api/live/hint",
        "/api/live/ppo-policy",
        "/api/live/choice-ranker",
        "/api/live/auto-action",
        "/api/live/play-to-end",
        "/api/live/autobattle/advance",
        "/api/live/run-mode",
        "/api/live/decision-advisor",
        "/api/live/decision-advisor/stream",
    ):
        r = c.post(route, json={})
        assert r.status_code == 409, f"{route}: {r.status_code} {r.text}"
        assert "Blitz" in r.json()["detail"], route
    # The run itself still answers.
    assert c.get("/api/live/state").status_code == 200
    assert c.get("/api/live/timeline").status_code == 200
    # A standard run on the same build is not refused for its mode.
    _start(c, game_mode=None)
    r = c.post("/api/live/choice-ranker", json={})
    assert "Blitz" not in r.text


#: The routes a Blitz fight answers (twin of ``blitz_combat_ai.rs``).
_FIGHT_ROUTES = (
    "/api/live/hint",
    "/api/live/search_tree",
    "/api/live/choice-ranker",
    "/api/live/auto-action",
    "/api/live/last-turn-grade",
)

#: The run-level AI: refused in a Blitz run wherever it stands.
_RUN_ROUTES = (
    ("POST", "/api/live/last-choice-grade"),
    ("POST", "/api/live/regret-review"),
    ("POST", "/api/live/card-advisor"),
    ("POST", "/api/live/card-advisor/stream"),
    ("POST", "/api/live/decision-advisor"),
    ("POST", "/api/live/decision-advisor/stream"),
    ("POST", "/api/live/last-choice-check/stream"),
    ("POST", "/api/live/run-mode"),
    ("POST", "/api/live/run-mode-step"),
    ("POST", "/api/live/faithful-step"),
    ("POST", "/api/live/play-to-end"),
    ("POST", "/api/live/autobattle/advance"),
    ("POST", "/api/live/autobattle/step"),
    ("GET", "/api/live/autobattle/verdict"),
    ("POST", "/api/live/ppo-policy"),
    ("POST", "/api/live/wpa-review"),
    ("POST", "/api/live/wpa-verify"),
)

_SEARCH = {"config": {"sims": 48}}


def _refusal(c: TestClient, method: str, route: str) -> tuple[int, str]:
    r = c.get(route) if method == "GET" else c.post(route, json=_SEARCH)
    return r.status_code, r.text


def _assert_run_ai_is_refused(c: TestClient, where: str) -> None:
    for method, route in _RUN_ROUTES:
        status, text = _refusal(c, method, route)
        assert status == 409, f"{where} {route}: {status} {text}"
        assert "Blitz" in text, f"{where} {route}: {text}"


def _in_fight(c: TestClient) -> bool:
    return _live_game(c).rm.phase() == "combat"


def _hinted_move(c: TestClient) -> tuple[str, dict[str, Any]]:
    """The hint's move as an action (``web/src/hint.ts`` ``bestAction``)."""
    r = c.post("/api/live/hint", json=_SEARCH)
    assert r.status_code == 200, r.text
    (best,) = [row for row in r.json()["scores"] if row["is_best"]]
    idx = best["stable_idx"]
    args: dict[str, Any] = {}
    if best.get("target_index") is not None:
        args["target_index"] = best["target_index"]
    if idx == -1:
        return "end_turn", {}
    if idx <= -100:
        return "use_potion", {**args, "potion_index": -100 - idx}
    return "play_card", {**args, "card_index": idx}


def test_the_fight_ai_answers_in_a_blitz_fight_and_only_there(tmp_path):
    """A fight is the same game in every mode (the combat search reads the
    fight's own ascension, 8 at Blitz 3), so the hint, the search tree, the
    in-fight choice ranker, "play best" on an in-fight choice and the turn
    grade answer in a Blitz fight. The run-level AI stays refused, and so do
    the fight routes outside a fight."""
    _rust()
    c = _client(tmp_path)
    step = _start(c, seed="BLITZAI", ascension=3)

    # Neow: no fight, so no AI at all.
    assert step["decision"] == "event_choice"
    for route in _FIGHT_ROUTES:
        status, text = _refusal(c, "POST", route)
        assert status == 409, f"Neow {route}: {status} {text}"
        assert "Blitz" in text, f"Neow {route}: {text}"
    _assert_run_ai_is_refused(c, "Neow")

    # Neow, the three cards, the first node.
    for _ in range(3):
        step = _first_enabled_action(c, step)
    assert step["decision"] == "combat_play"
    assert _in_fight(c)

    # The hint and the tree answer; the routes with nothing to say here
    # refuse for their own reason, not for the mode.
    r = c.post("/api/live/hint", json=_SEARCH)
    assert r.status_code == 200, r.text
    rows = r.json()["scores"]
    assert rows and sum(1 for row in rows if row["is_best"]) == 1
    r = c.post("/api/live/search_tree", json=_SEARCH)
    assert r.status_code == 200, r.text
    assert r.json()["nodes"] and not r.json().get("error")
    for route in ("/api/live/choice-ranker", "/api/live/auto-action", "/api/live/last-turn-grade"):
        status, text = _refusal(c, "POST", route)
        # The turn grade is the browser build's route: this server has none.
        want = 404 if route.endswith("last-turn-grade") else 409
        assert status == want, f"fight {route}: {status} {text}"
        assert "Blitz" not in text, f"fight {route}: {text}"
    _assert_run_ai_is_refused(c, "fight")

    # The hint plays the fight out; an in-fight choice is "play best"'s.
    for _ in range(400):
        if not _in_fight(c):
            break
        if step["decision"] == "card_select":
            r = c.post("/api/live/auto-action", json={})
            if r.status_code == 200:
                assert r.json()["policy"] == "combat_mcts"
                step = _api_act(c, r.json()["action"], **r.json()["args"])["step"]
            else:  # a multi-pick has no single answer
                assert "Blitz" not in r.text
                step = _first_enabled_action(c, step)
            continue
        assert step["decision"] == "combat_play", step["decision"]
        action, args = _hinted_move(c)
        step = _api_act(c, action, **args)["step"]
        if action == "end_turn" and _in_fight(c):
            r = c.post("/api/live/last-turn-grade", json={})
            assert r.status_code == 200 or "Blitz" not in r.text, r.text
    assert not _in_fight(c), "the fight never ended"
    assert step["decision"] != "game_over", "the hint lost"

    # Out of the fight the AI is gone again.
    for route in _FIGHT_ROUTES:
        status, text = _refusal(c, "POST", route)
        assert status == 409, f"after {route}: {status} {text}"
        assert "Blitz" in text, f"after {route}: {text}"
    _assert_run_ai_is_refused(c, "after the fight")

    # A standard run on the same build keeps its AI.
    _start(c, game_mode=None, ascension=3)
    r = c.post("/api/live/auto-action", json={})
    assert r.status_code == 200, r.text
    status, text = _refusal(c, "POST", "/api/live/hint")
    assert status == 409 and "Blitz" not in text, text


def test_save_seek_and_run_log_keep_a_blitz_run(tmp_path):
    _rust()
    c = _client(tmp_path)
    step = _start(c, ascension=5)
    root_seed = step["state"]["bg_seed"]
    # Neow, the three cards, the first node.
    for _ in range(3):
        step = _first_enabled_action(c, step)
    assert step["decision"] == "combat_play"
    before = c.get("/api/live/state").json()["state"]
    assert before["game_mode"] == "blitz" and before["ascension"] == 5

    # Seek to the root rebuilds a Blitz run.
    tl = c.get("/api/live/timeline").json()
    r = c.post("/api/live/seek", json={"node_id": tl["nodes"][0]["id"]})
    assert r.status_code == 200, r.text
    root = r.json()["step"]["state"]
    assert root["game_mode"] == "blitz" and root["ascension"] == 5
    assert root["bg_seed"] == root_seed
    r = c.post("/api/live/seek", json={"node_id": tl["active_leaf"]})
    assert r.status_code == 200, r.text

    # Save -> restore.
    assert c.post("/api/live/save", json={"name": "blitz mid"}).status_code == 200
    payload = saves_mod.read_save(saves_mod.save_dir(tmp_path / "logs"), "blitz mid")
    assert payload is not None
    assert payload["game_mode"] == "blitz" and payload["ascension"] == 5
    _start(c, game_mode=None)  # replace the live run with a standard one
    r = c.post("/api/saves/load", json={"name": "blitz mid"})
    assert r.status_code == 200, r.text
    restored = r.json()["state"]
    assert restored["game_mode"] == "blitz"
    assert restored["floor"] == before["floor"]
    assert restored["player"] == before["player"]

    # An unknown mode in a stored payload is refused, not replayed standard.
    saves_mod.write_save(
        saves_mod.save_dir(tmp_path / "logs"),
        name="bad mode",
        character=payload["character"],
        seed=payload["seed"],
        ascension=payload["ascension"],
        backend="rust",
        run_mode=False,
        progress={},
        line=payload["line"],
        game_version=V113,
        game_mode="nightmare",
    )
    r = c.post("/api/saves/load", json={"name": "bad mode"})
    assert r.status_code == 409, r.text
    assert "unknown game_mode" in r.json()["detail"]

    # Run log meta -> "play from here".
    log_path = c.post("/api/live/quit").json()["log_path"]
    ident = TimelineStore.load_jsonl(log_path).run_identity()
    assert ident is not None and ident["game_mode"] == "blitz"
    assert c.post("/api/load", json={"path": log_path}).status_code == 200
    recorded = c.get("/api/step/1").json()
    assert recorded["state"]["game_mode"] == "blitz"
    assert recorded["decision"] == "card_select"
    r = c.post("/api/live/from_replay", json={"idx": 1})
    assert r.status_code == 200, r.text
    assert r.json()["state"]["game_mode"] == "blitz"


# ── a whole run, decision by decision ───────────────────────────────────────


def _snake(kind: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", kind).lower()


def _loc(table: str) -> dict[str, str]:
    return json.loads((LOC_DIR / f"{table}.json").read_text(encoding="utf-8"))


def _expected_ware_text(item: dict[str, Any]) -> tuple[str, str]:
    """A ware's title and description as the game's own v0.113.0 loc table
    spells them, with the variables the C# entries add filled in
    (``BlitzMerchantCardEnchantEntry`` / ``GlamEntry`` /
    ``PrismaticCardRewardEntry``)."""
    merchant = _loc("merchant_room")
    stem = f"MERCHANT.blitz.{_snake(item['blitz_kind'])}_entry"
    title, desc = merchant[f"{stem}.title"], merchant[f"{stem}.description"]
    ench, amount = item.get("enchantment"), item.get("enchantment_amount") or 1
    if item["blitz_kind"] == "Glam":
        ench, amount = "GLAM", 1
    if ench:
        name = _loc("enchantments")[f"{ench}.title"]
        title = title.replace("{Enchantment}", name)
        desc = desc.replace("{Enchantment}", name)
    desc = desc.replace("{Amount:plural:| {}}", "" if amount == 1 else f" {amount}")
    if item.get("character"):
        short = _loc("characters")[f"{item['character'].upper()}.shortTitle"]
        title = title.replace("{Character}", short)
        desc = desc.replace("{Character}", short)
    return title, desc


def _view(state: dict[str, Any], tmp_path) -> dict[str, Any]:
    """The enriched live step the SPA is sent for ``state``."""
    import copy

    from workbench.assets import AssetResolver

    return _live_step(copy.deepcopy(state), AssetResolver(tmp_path), {})


def _ok(state: dict[str, Any]) -> dict[str, Any]:
    assert state.get("type") != "error", state
    return state


def _wares(state: dict[str, Any]) -> list[dict[str, Any]]:
    return [o for o in state["options"] if o.get("action") == "blitz_choice"]


def _back_to_the_shop(g, state: dict[str, Any]) -> dict[str, Any]:
    for _ in range(40):
        dec = state["decision"]
        if dec == "shop":
            return state
        if dec == "card_select":
            state = _ok(g.act("select_cards", indices=_first_indices(state.get("min_select") or 1)))
        elif dec == "card_reward":
            state = _ok(g.act("select_card_reward", card_index=0))
        elif dec == "event_choice":
            state = _ok(g.act("choose_option", option_index=0))
        else:
            pytest.fail(f"a Blitz ware opened {dec}: {state}")
    pytest.fail("the merchant never came back")


def _play_the_merchant(g, state: dict[str, Any], seen: dict[str, Any], tmp_path) -> dict[str, Any]:
    assert state["blitz_choices_remaining"] == 3
    assert (state["cards"], state["relics"], state["potions"]) == ([], [], [])
    assert state["card_removal_cost"] is None
    assert state["options"][0] == {
        "index": 0,
        "id": "leave_shop",
        "action": "leave_shop",
        "label": "leave_shop",
        "enabled": True,
    }
    listed = _wares(state)
    assert len(listed) == 6
    for i, w in enumerate(listed):
        item = w["item"]
        assert w["index"] == i + 1
        assert w["label"] == f"blitz:{item['blitz_kind']}"
        assert (w["status"], w["enabled"], w["price"]) == ("Available", True, 0)
        assert item["cursed"] is (i == 5), "the cursed ware is last"
        # The text is the game's own.
        if item["blitz_kind"] == "CardEnchant" and not item.get("enchantment"):
            continue  # no enchantment fits the deck: the game's title is empty
        assert (item["title"], item["description"]) == _expected_ware_text(item), item
        assert "{" not in item["title"] + item["description"]
        seen["kinds"].add(item["blitz_kind"])

    # What the SPA is sent: the wares under their own key, one normalized
    # action per ware, then Leave.
    step = _view(state, tmp_path)
    shown = step["state"]["blitz_wares"]
    assert [w["index"] for w in shown] == [1, 2, 3, 4, 5, 6]
    assert shown[5]["cursed"] is True
    assert shown[0] == {
        "index": 1,
        "kind": listed[0]["item"]["blitz_kind"],
        "title": listed[0]["item"]["title"],
        "description": listed[0]["item"]["description"],
        "cursed": False,
        "rare": listed[0]["item"]["rare"],
        "status": "Available",
        "enabled": True,
    }
    opts = step["state"]["options"]
    assert len(opts) == 7
    assert opts[0] == {
        "label": listed[0]["item"]["title"],
        "action": "blitz_choice",
        "args": {"index": 1},
        "enabled": True,
    }
    assert opts[6]["action"] == "leave_room"

    # A ware with a deck pick can be backed out of: nothing is spent.
    deck_pick = ("CardUpgrade", "CardTransform", "CardRemoval", "CardEnchant")
    for w in listed:
        if w["item"]["blitz_kind"] not in deck_pick:
            continue
        pick = _ok(g.act("blitz_choice", index=w["index"]))
        if pick["decision"] == "card_select":
            assert pick["can_skip"] is True
            assert any(
                o["action"] == "skip_select" for o in _view(pick, tmp_path)["state"]["options"]
            )
            again = _ok(g.act("skip_select"))
            assert again["decision"] == "shop"
            assert again["blitz_choices_remaining"] == 3
            assert _wares(again) == listed, "the ware is still available"
            seen["cancelled"] = True
            state = again
        else:
            state = _back_to_the_shop(g, pick)
        break

    # Spend every pick, the cursed ware first.
    order = [6, 1, 2, 3, 4, 5]
    while state["blitz_choices_remaining"] > 0:
        now = _wares(state)
        index = next((i for i in order if now[i - 1]["enabled"]), None)
        if index is None:
            break
        order.remove(index)
        before = state["blitz_choices_remaining"]
        kind = now[index - 1]["item"]["blitz_kind"]
        deck_before = len(state["player"]["deck"])
        state = _back_to_the_shop(g, _ok(g.act("blitz_choice", index=index)))
        after = state["blitz_choices_remaining"]
        ware = _wares(state)[index - 1]
        if ware["status"] == "Available":
            # RandomUpgrades with nothing to upgrade is refused and free.
            assert kind == "RandomUpgrades" and after == before
            continue
        assert (ware["status"], ware["enabled"]) == ("Used", False), kind
        assert after == before + (1 if kind == "TwoMoreChoices" else -1), kind
        if index == 6:
            seen["cursed"] = True
            assert len(state["player"]["deck"]) > deck_before, f"{kind} adds a Curse"
    assert state["blitz_choices_remaining"] == 0
    assert all(not w["enabled"] for w in _wares(state))
    assert {w["status"] for w in _wares(state)} == {"Used", "Disabled"}
    enabled = [o for o in _view(state, tmp_path)["state"]["options"] if o["enabled"]]
    assert [o["action"] for o in enabled] == ["leave_room"]
    assert g.act("blitz_choice", index=1).get("type") == "error"
    return _ok(g.act("leave_room"))


def _open_the_chest(g, state: dict[str, Any], tmp_path) -> dict[str, Any]:
    assert state["chest_picks_left"] == 2
    rewards = state["rewards"]
    assert [(r["index"], r["type"]) for r in rewards] == [(0, "relic"), (1, "relic")]
    assert "room_reveal" not in state
    step = _view(state, tmp_path)
    assert [o["action"] for o in step["state"]["options"]] == [
        "take_reward",
        "take_reward",
        "skip_reward",
    ]
    assert step["state"]["options"][1]["args"] == {"reward_index": 1}
    assert all("image" in r for r in step["state"]["rewards"])

    had, gold = len(state["player"]["relics"]), state["player"]["gold"]
    # Round one: the second relic; the first keeps its index.
    round2 = _ok(g.act("take_reward", reward_index=1))
    assert round2["decision"] == "rewards"
    assert round2["chest_picks_left"] == 1
    assert round2["rewards"] == [rewards[0]]
    assert len(round2["player"]["relics"]) == had + 1
    after = _ok(g.act("take_reward", reward_index=0))
    assert after["decision"] != "rewards"
    assert len(after["player"]["relics"]) >= had + 2
    assert after["player"]["gold"] == gold, "a Blitz chest holds no gold"
    assert "room_reveal" not in after
    return after


def _walk_a_run(character: str, seed: str, level: int, tmp_path) -> dict[str, Any]:
    """Play a Blitz run to its end with fights stamped won, checking every
    decision the workbench shows on the way."""
    from workbench.backends.rust_game import RustGame

    seen: dict[str, Any] = {"kinds": set(), "cancelled": False, "cursed": False, "acts": set()}
    g = RustGame(game_version=V113)
    state = _ok(
        g.start(character=character, seed=seed, ascension=level, act1="roll", game_mode="blitz")
    )
    gold = state["player"]["gold"]
    rooms: dict[int, list[str]] = {}
    ancients: list[int] = []
    for _ in range(600):
        dec = state["decision"]
        if dec == "game_over":
            assert state["victory"] is True
            assert rooms == {1: LINE, 2: LINE, 3: LINE}
            assert ancients == [1, 2, 3], "every act opens on its Ancient"
            return seen
        assert state["game_mode"] == "blitz"
        assert state["ascension"] == level
        assert "dominion" not in state
        act = state["act"]
        seen["acts"].add(act)
        if dec == "event_choice":
            if state["event_name"] == "NEOW.title":
                assert (act, state["floor"], rooms) == (1, 1, {})
                assert [o["title"] for o in state["options"]] == ["Blitz"]
                ancients.append(1)
            else:
                # The next act's Ancient, straight after the boss with no
                # map decision between: three relics, shown (as in a
                # standard run) from the boss room it follows.
                assert state["event_name"] == ""
                assert rooms[act] == LINE
                assert len(state["options"]) == 3
                ancients.append(act + 1)
            state = _ok(g.act("choose_option", option_index=0))
        elif dec == "card_select":
            state = _ok(g.act("select_cards", indices=_first_indices(state.get("min_select") or 1)))
        elif dec == "map_select":
            (choice,) = state["choices"]  # the fixed line: exactly one node
            here = rooms.setdefault(act, [])
            assert (choice["col"], choice["row"]) == (3, len(here) + 1)
            assert state["floor"] == 8 * (act - 1) + len(here) + 1
            assert ancients[-1] == act, "the act's Ancient came first"
            # The boss is the map's own `boss`, not a row node: its choice
            # carries no type (as in a standard run).
            here.append(choice["type"] or "Boss")
            full = g.get_map()
            assert [len(r) for r in full["rows"]] == [1] * 7
            assert full["boss"]["row"] == 7
            state = _ok(g.act("select_map_node", col=choice["col"], row=choice["row"]))
        elif dec == "combat_play":
            assert g.rm.debug_win_combat() in ("advanced", "run_over")
            state = g.get_state()
        elif dec == "card_reward":
            assert len(state["cards"]) == 3
            state = _ok(g.act("select_card_reward", card_index=0))
        elif dec == "shop":
            state = _play_the_merchant(g, state, seen, tmp_path)
        elif dec == "rewards":
            state = _open_the_chest(g, state, tmp_path)
        elif dec == "rest_site":
            state = _ok(g.act("choose_option", option_index=0))
        else:
            pytest.fail(f"unexpected decision {dec}: {state}")
        # Room rewards carry no gold in Blitz.
        assert state["player"]["gold"] == gold, f"after {dec}"
    pytest.fail("the run never ended")


def test_a_blitz_run_is_playable_decision_by_decision(tmp_path):
    _rust()
    cancelled, kinds = False, set()
    for character, seed, level in [
        ("Ironclad", "BLITZWB1", 0),
        ("Silent", "BLITZWB2", 5),
        ("Defect", "BLITZWB3", 2),
        ("Regent", "BLITZWB4", 0),
        ("Necrobinder", "BLITZWB5", 4),
        ("Fool", "BLITZWB6", 1),
        ("Ironclad", "BLITZWB7", 3),
        ("Silent", "BLITZWB8", 0),
    ]:
        seen = _walk_a_run(character, seed, level, tmp_path)
        assert seen["acts"] == {1, 2, 3}, (character, seed)
        assert seen["cursed"], (character, seed)
        cancelled = cancelled or seen["cancelled"]
        kinds |= seen["kinds"]
    assert cancelled, "no walk met a ware with a deck pick"
    # Twenty-four merchants: every ware's text was checked against the loc.
    assert len(kinds) == 20, sorted(kinds)


def test_skipping_the_chest_ends_it(tmp_path):
    _rust()
    from workbench.backends.rust_game import RustGame

    for verb in ("skip_reward", "proceed"):
        g = RustGame(game_version=V113)
        state = _ok(
            g.start(
                character="Ironclad", seed="BLITZWB1", ascension=0, act1="roll", game_mode="blitz"
            )
        )
        for _ in range(200):
            dec = state["decision"]
            if dec == "rewards":
                break
            if dec in ("event_choice", "rest_site"):
                state = _ok(g.act("choose_option", option_index=0))
            elif dec == "card_select":
                state = _ok(
                    g.act("select_cards", indices=_first_indices(state.get("min_select") or 1))
                )
            elif dec == "map_select":
                (choice,) = state["choices"]
                state = _ok(g.act("select_map_node", col=choice["col"], row=choice["row"]))
            elif dec == "combat_play":
                g.rm.debug_win_combat()
                state = g.get_state()
            elif dec == "card_reward":
                state = _ok(g.act("skip_card_reward"))
            elif dec == "shop":
                state = _ok(g.act("leave_room"))
            else:
                pytest.fail(f"unexpected decision {dec}")
        assert state["decision"] == "rewards"
        had = len(state["player"]["relics"])
        after = _ok(g.act(verb))
        assert after["decision"] == "map_select", verb
        assert len(after["player"]["relics"]) == had
        # Outside a chest the chest verbs are errors.
        assert g.act("take_reward", reward_index=0).get("type") == "error"


# ── the same rooms through the session and the API ─────────────────────────


def _live_game(c: TestClient):
    (session,) = list(c.app.state.sessions._sessions.values())
    return session.live.game


def _api_act(c: TestClient, action: str, **args) -> dict:
    r = c.post("/api/live/action", json={"action": action, "args": args})
    assert r.status_code == 200, r.text
    assert r.json()["error"] is None, (action, args, r.json()["error"])
    return r.json()


def test_the_merchant_and_the_chest_play_through_the_api(tmp_path):
    """Every Blitz verb passes the session's allow-list, is labelled in the
    history, and comes back as an enriched step: the merchant with three
    wares taken, then both rounds of the chest."""
    _rust()
    from workbench.branching import ALLOWED_ACTIONS

    assert {"blitz_choice", "take_reward", "skip_reward"} <= ALLOWED_ACTIONS
    c = _client(tmp_path)
    state = _start(c, seed="BLITZWB1", act1="roll")["state"]
    labels: list[str] = []
    shopped = chest_done = False
    for _ in range(200):
        dec = state["decision"]
        if dec == "combat_play":
            # Stamp the fight won; the next action is chosen off the engine's
            # own state. The win is not on the session's line, so this run is
            # never rebuilt afterwards.
            game = _live_game(c)
            assert game.rm.debug_win_combat() == "advanced"
            state = game.get_state()
            continue
        if dec in ("event_choice", "rest_site"):
            resp = _api_act(c, "choose_option", option_index=0)
        elif dec == "card_select":
            resp = _api_act(c, "select_cards", indices=_first_indices(state.get("min_select") or 1))
        elif dec == "map_select":
            (choice,) = state["choices"]
            resp = _api_act(c, "select_map_node", col=choice["col"], row=choice["row"])
        elif dec == "card_reward":
            resp = _api_act(c, "select_card_reward", card_index=0)
        elif dec == "shop":
            shopped = True
            view = c.get("/api/live/state").json()["state"]
            ware = next((w for w in view["blitz_wares"] if w["enabled"]), None)
            if ware is not None and view["blitz_choices_remaining"] > 0:
                resp = _api_act(c, "blitz_choice", index=ware["index"])
                assert resp["label"] == f"Take {ware['title']}"
                labels.append(resp["label"])
            else:
                resp = _api_act(c, "leave_room")
        elif dec == "rewards":
            view = c.get("/api/live/state").json()["state"]
            relic = view["rewards"][-1]
            assert "image" in relic
            resp = _api_act(c, "take_reward", reward_index=relic["index"])
            assert resp["label"] == f"Take {relic['name']}"
            chest_done = len(view["rewards"]) == 1
        else:
            pytest.fail(f"unexpected decision {dec}: {state}")
        state = resp["step"]["state"]
        if chest_done:
            break
    assert shopped and chest_done
    assert len(labels) >= 3, labels
    text = json.dumps(c.get("/api/live/timeline").json())
    for label in labels:
        assert label in text, f"{label} not in the timeline"
