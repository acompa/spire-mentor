"""Throwing Axe's spent bit on the wire.

Throwing Axe plays the first card of a combat twice and then stops glowing
(``RelicStatus.Active`` -> ``Normal``). The host sent no counter for it, so a
fight rebuilt after that first card re-armed the relic and the mirror played
the next card twice too.
"""

from __future__ import annotations


def _axe(state: dict) -> dict:
    relics = (state.get("player") or {}).get("relics") or []
    return next(r for r in relics if r.get("id") == "THROWING_AXE")


def test_throwing_axe_reads_spent_after_the_first_card(game):
    state = game.start(character="Ironclad", seed="AXE1")
    game.skip_neow(state)
    r = game.set_player(
        hp=70, max_hp=80, deck=["DEFEND_IRONCLAD"] * 6, relics=["THROWING_AXE"], potions=[]
    )
    assert r.get("type") != "error", r
    before = game.enter_room("combat", encounter="SHRINKER_BEETLE_WEAK")
    assert _axe(before).get("counter") == 0, _axe(before)
    after = game.play_card(0, None)
    assert _axe(after).get("counter") == 1, _axe(after)
