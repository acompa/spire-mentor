"""Ghost Seed's Ethereal on cards off the hand reaches the rebuild (GH #1737).

Ghost Seed makes every basic Strike and Defend Ethereal for the fight, in
every pile. The headless host names the keyword on each card; the adapter
used to pass keyword flags for the hand only, so the draw pile rebuilt plain.
"""

from __future__ import annotations

from sts2_hint.adapter import _translate_card


def test_a_draw_pile_card_keeps_its_fight_ethereal() -> None:
    raw = {"index": 0, "id": "CARD.STRIKE_IRONCLAD", "cost": 1, "keywords": ["Ethereal", "Retain"]}
    assert _translate_card(raw, "$.draw_pile[0]")["flags"] == ["ethereal"]
    assert _translate_card(raw, "$.hand[0]", hand=True)["flags"] == ["ethereal", "retain"]
