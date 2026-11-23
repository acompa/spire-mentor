"""Stars on the wire for a character other than Regent (GH #1734).

Big Bang gains a star for whoever plays it. The host used to emit ``stars``
for Regent only, so an Ironclad's star read as 0 in the rebuilt fight.
"""

from __future__ import annotations


def test_an_ironclad_who_plays_big_bang_shows_one_star(game):
    state = game.start(character="Ironclad", seed="STARS1")
    game.skip_neow(state)
    r = game.set_player(hp=70, max_hp=80, deck=["BIG_BANG"] * 5, relics=[], potions=[])
    assert r.get("type") != "error", r
    before = game.enter_room("combat", encounter="SHRINKER_BEETLE_WEAK")
    assert not before.get("stars"), before.get("stars")
    after = game.play_card(0, None)
    assert after.get("stars") == 1, {k: after.get(k) for k in ("decision", "stars", "energy")}
