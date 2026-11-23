"""A reward card's enchantment reaches the bench (Silken Tress, GH #1733).

Silken Tress glams the cards of the first card reward after it is obtained.
The host names the enchantment on each reward card, and the bench bridge
passes it on, so the rebuilt reward offers the glammed cards.
"""

from __future__ import annotations

import pytest
from headless_eval import to_bridge_card_reward
from test_card_reward_alternatives import _to_card_reward
from test_referee_rng import Host


def test_the_bridge_keeps_a_reward_cards_enchantment() -> None:
    state = {
        "type": "decision",
        "decision": "card_reward",
        "cards": [
            {"index": 0, "id": "IRON_WAVE", "name": "IRON_WAVE.title", "enchantment": "Glam"},
            {"index": 1, "id": "TREMBLE", "name": "TREMBLE.title"},
        ],
        "can_skip": True,
    }
    cards = to_bridge_card_reward(state, "Ironclad")["cards"]
    assert cards[0]["enchantment"] == "Glam"
    assert cards[0]["enchantment_amount"] == 1
    assert "enchantment" not in cards[1]


@pytest.mark.oracle
def test_silken_tress_glams_the_rewards_cards_on_the_wire() -> None:
    with Host(referee=False) as host:
        state = _to_card_reward(host, "SILKEN_TRESS", "GLAM1")
        assert state["cards"]
        assert all("Glam" in str(c.get("enchantment")) for c in state["cards"]), state["cards"]


def test_the_bridge_keeps_a_card_select_cards_enchantment() -> None:
    """Dolly's Mirror offering a Kifuda card copies its Adroit 3 (Spire Bench corpus)."""
    from headless_eval import to_bridge_card_select

    state = {
        "type": "decision",
        "decision": "card_select",
        "cards": [
            {
                "index": 0,
                "id": "CARD.ANGER",
                "name": "Anger",
                "enchantment": "Adroit",
                "enchantment_amount": 3,
            },
            {"index": 1, "id": "CARD.BASH", "name": "Bash"},
        ],
        "min_select": 1,
        "max_select": 1,
    }
    cards = to_bridge_card_select(state, "Ironclad")["cards"]
    assert (cards[0]["enchantment"], cards[0]["enchantment_amount"]) == ("Adroit", 3)
    assert "enchantment" not in cards[1]


@pytest.mark.oracle
def test_a_deck_card_select_names_each_cards_enchantment() -> None:
    """The host's card_select shows an enchanted deck card's enchantment, as its
    card_reward and deck do: a glammed reward card, then a rest site's smith."""
    with Host(referee=False) as host:
        state = _to_card_reward(host, "SILKEN_TRESS", "GLAM1")
        glam = state["cards"][0]["id"]
        host.send(cmd="action", action="select_card_reward", args={"card_index": 0})
        state = host.send(cmd="enter_room", type="rest")
        smith = [
            o["index"]
            for o in state["options"]
            if "SMITH" in str(o.get("id") or o.get("text_key") or o).upper()
        ]
        assert smith, state["options"]
        state = host.send(cmd="action", action="choose_option", args={"option_index": smith[0]})
        assert state.get("decision") == "card_select", state.get("decision")
        hit = [c for c in state["cards"] if c["id"] == glam]
        assert hit, [c["id"] for c in state["cards"]]
        assert "Glam" in str(hit[0].get("enchantment")), hit[0]
