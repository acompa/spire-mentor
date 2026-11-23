"""The host sends what Possess Speed stole (GH #1836).

``PossessSpeedPower`` keeps the Dexterity it took in private data
(``Data.stolenDexterity``) and gives it back when its owner dies. The engine
keeps the same total on ``Power.counter``. The host sent only the power's
amount, so a rebuilt Forgotten had stolen nothing and gave nothing back.
"""

from __future__ import annotations


def _possess(view: dict) -> dict | None:
    for e in view.get("enemies") or []:
        for p in e.get("powers") or []:
            if "POSSESS_SPEED" in str(p.get("id")):
                return p
    return None


def test_the_forgotten_sends_the_dexterity_it_stole(game) -> None:
    state = game.start(character="Ironclad", seed="POSSESS1836")
    game.skip_neow(state)
    deck = [*["DEFEND_IRONCLAD"] * 10]
    r = game.set_player(hp=300, max_hp=300, deck=deck, relics=[], potions=[])
    assert r.get("type") != "error", r
    view = game.enter_room("combat", encounter="THE_LOST_AND_FORGOTTEN_NORMAL")
    assert view.get("decision") == "combat_play", view
    power = _possess(view)
    assert power is not None, view.get("enemies")
    assert (power.get("vars") or {}).get("Stolen", 0) == 0
    # The Forgotten opens with Miasma: Dexterity -2 on the player.
    view = game.end_turn()
    assert view.get("decision") == "combat_play", view
    power = _possess(view)
    assert power is not None
    assert (power.get("vars") or {}).get("Stolen") == 2, power
