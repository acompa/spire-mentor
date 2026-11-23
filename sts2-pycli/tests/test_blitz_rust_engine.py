"""v0.113.0 Blitz mode on the Rust engine (GH #1347), through the workbench
Rust backend in the C# host's dialect, with no oracle process.

The lockstep (``run_parity.py --game-mode blitz``) is what compares the two
engines; these tests pin the decision shapes the Rust backend must emit for
it to line up with the host (``tests/test_blitz_mode.py``): a one-choice
``map_select``, Neow's single option and 10-pick-3 grid, the merchant's
``options`` and the chest's ``rewards`` screen.
"""

from __future__ import annotations

from typing import Any

import pytest
import sts2_client

pytest.importorskip("sts2_rs", reason="the rust backend requires the sts2_rs extension")

from run_parity import enumerate_choices
from workbench.backends.rust_game import RustGame

HAS_BLITZ = sts2_client.GAME_VERSION == "v0.113.0"
pytestmark = pytest.mark.skipif(not HAS_BLITZ, reason="GameMode.Blitz exists from v0.113.0")

LINE = ["Monster", "Monster", "Shop", "Elite", "Treasure", "RestSite"]
CURSED_KINDS = {
    "Glam",
    "TwoMoreChoices",
    "CardRewardOfEachRarity",
    "UpgradedRareCardReward",
    "TwoRelics",
    "FullHealAndMaxHp",
}


def _start(seed: str = "BLITZ1", character: str = "Ironclad", ascension: int = 0) -> RustGame:
    g = RustGame(target_convention="csharp", game_version="v0.113.0")
    state = g.start(
        character=character,
        seed=seed,
        ascension=ascension,
        act1="roll",
        game_mode="blitz",
    )
    assert state.get("type") != "error", state
    return g


def _ok(state: dict[str, Any]) -> dict[str, Any]:
    assert state.get("type") != "error", state
    return state


def _advance(g: RustGame, state: dict[str, Any], *, buy: bool = False) -> dict[str, Any]:
    """One move of a fixed policy; fights are stamped won."""
    dec = state["decision"]
    if dec == "combat_play":
        # The tests are about the rooms, not the fights.
        assert g.rm.debug_win_combat() == "advanced"
        return g.get_state()
    if dec == "map_select":
        (choice,) = state["choices"]
        return _ok(g.act("select_map_node", col=choice["col"], row=choice["row"]))
    if dec == "shop" and not buy:
        return _ok(g.act("leave_room"))
    verb, kwargs = enumerate_choices(g, state)[0]
    return _ok(g.act(verb, **kwargs))


def _until(g: RustGame, decision: str, *, limit: int = 4000) -> dict[str, Any]:
    state = g.get_state()
    for _ in range(limit):
        if state["decision"] == decision:
            return state
        state = _advance(g, state)
    pytest.fail(f"never reached a {decision!r} decision")


def test_the_engine_refuses_a_blitz_level_above_five_and_older_builds():
    import sts2_rs

    with pytest.raises(ValueError, match=r"Blitz level 0\.\.=5"):
        sts2_rs.RunManager(
            string_seed="X", game_version="v0.113.0", game_mode="blitz", ascension_level=6
        )
    with pytest.raises(ValueError, match=r"needs game_version v0\.113\.0"):
        sts2_rs.RunManager(string_seed="X", game_version="v0.111.0", game_mode="blitz")
    rm = sts2_rs.RunManager(
        string_seed="X", game_version="v0.113.0", game_mode="blitz", ascension_level=5
    )
    assert (rm.game_mode, rm.ascension_level) == ("blitz", 5)


def test_neow_is_one_option_then_ten_cards_pick_three():
    g = _start()
    state = g.get_state()
    assert state["decision"] == "event_choice"
    assert state["game_mode"] == "blitz"
    assert [o["index"] for o in state["options"]] == [0]
    deck = len(state["player"]["deck"])
    state = _ok(g.act("choose_option", option_index=0))
    assert state["decision"] == "card_select"
    assert len(state["cards"]) == 10
    assert (state["min_select"], state["max_select"]) == (3, 3)
    state = _ok(g.act("select_cards", indices="0,4,9"))
    assert state["decision"] == "map_select"
    assert len(state["player"]["deck"]) == deck + 3


def test_the_map_is_the_line_with_one_choice_per_step():
    g = _start()
    rows = g.get_map()["rows"]
    # The Ancient start node is row 0 of the engine's map; the oracle omits it.
    line = [row for row in rows if row and row[0]["row"] >= 1]
    assert [[n["type"] for n in row] for row in line] == [[t] for t in LINE]
    assert all(n["col"] == 3 for row in line for n in row)
    seen = []
    state = _until(g, "map_select")
    for _ in range(len(LINE)):
        (choice,) = state["choices"]
        assert choice["col"] == 3
        seen.append(choice["type"])
        state = _ok(g.act("select_map_node", col=3, row=choice["row"]))
        state = _until(g, "map_select")
    assert seen == LINE
    # The boss node is not in the map's ``rows`` (it is ``boss``), so the
    # backend names its coordinate only, as on a standard map's last row.
    assert [(c["col"], c["row"]) for c in state["choices"]] == [(3, 7)]


def test_the_merchant_lists_six_wares_and_three_picks():
    g = _start()
    state = _until(g, "shop")
    assert state["cards"] == state["relics"] == state["potions"] == []
    assert state["card_removal_cost"] is None
    assert state["blitz_choices_remaining"] == 3
    leave, *wares = state["options"]
    assert (leave["index"], leave["action"]) == (0, "leave_shop")
    assert [w["index"] for w in wares] == [1, 2, 3, 4, 5, 6]
    assert all(w["action"] == "blitz_choice" and w["enabled"] for w in wares)
    assert all(w["label"] == f"blitz:{w['item']['blitz_kind']}" for w in wares)
    assert [w["item"]["cursed"] for w in wares] == [False] * 5 + [True]
    assert wares[5]["item"]["blitz_kind"] in CURSED_KINDS
    gold = state["player"]["gold"]
    # Take wares until the picks run out; each returns to the merchant.
    for _ in range(60):
        if state["decision"] == "shop" and not state["blitz_choices_remaining"]:
            break
        state = _advance(g, state, buy=True)
    assert state["decision"] == "shop"
    assert state["blitz_choices_remaining"] == 0
    assert not any(w["enabled"] for w in state["options"][1:])
    assert state["player"]["gold"] == gold
    assert _ok(g.act("leave_room"))["decision"] == "map_select"


def test_the_chest_is_a_two_round_rewards_screen():
    g = _start()
    state = _until(g, "rewards")
    assert state["chest_picks_left"] == 2
    assert [r["index"] for r in state["rewards"]] == [0, 1]
    assert all(r["type"] == "relic" for r in state["rewards"])
    relics = len(state["player"]["relics"])
    state = _ok(g.act("take_reward", reward_index=1))
    if state["decision"] == "rewards":
        assert state["chest_picks_left"] == 1
        assert [r["index"] for r in state["rewards"]] == [0]
        state = _ok(g.act("take_reward", reward_index=0))
        assert len(state["player"]["relics"]) >= relics + 2
    # A skip closes the chest with both relics left in it.
    g = _start(seed="BLITZ2")
    state = _until(g, "rewards")
    relics = state["player"]["relics"]
    state = _ok(g.act("skip_reward", reward_index=0))
    assert state["decision"] == "map_select"
    assert state["player"]["relics"] == relics
