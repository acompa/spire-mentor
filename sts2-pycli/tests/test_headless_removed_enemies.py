"""Enemies that died and left the board (GH #1716).

The game takes an ordinary corpse out of ``CombatState.Enemies``; only a
reviver's stays (``dead_enemies``). The engine keeps every corpse in its
place, so a fight rebuilt from the board alone has one enemy too few. A
player watched the enemy die, so the host lists it in ``removed_enemies``.
"""

from __future__ import annotations

from test_referee_rng import _project


def test_a_killed_enemy_is_listed_as_removed(game):
    state = game.start(character="Ironclad", seed="REMOVED1")
    game.skip_neow(state)
    r = game.set_player(hp=70, max_hp=80, deck=["BLUDGEON"] * 6, relics=[], potions=[])
    assert r.get("type") != "error", r
    before = game.enter_room("combat", encounter="TOADPOLES_WEAK")
    assert not before.get("removed_enemies"), before.get("removed_enemies")
    ids = [e["combat_id"] for e in before["enemies"]]
    assert len(ids) >= 2, before["enemies"]
    victim = before["enemies"][0]

    after = game.play_card(0, 0)
    assert victim["combat_id"] not in [e["combat_id"] for e in after["enemies"]], after["enemies"]
    removed = after.get("removed_enemies") or []
    assert [(e["combat_id"], e["id"], e["max_hp"]) for e in removed] == [
        (victim["combat_id"], victim["id"], victim["max_hp"])
    ]
    assert removed[0]["hp"] == 0 and removed[0]["is_dead"] is True


def test_the_referee_view_keeps_removed_enemies(game):
    state = game.start(character="Ironclad", seed="REMOVED2")
    game.skip_neow(state)
    r = game.set_player(hp=70, max_hp=80, deck=["BLUDGEON"] * 6, relics=[], potions=[])
    assert r.get("type") != "error", r
    game.enter_room("combat", encounter="TOADPOLES_WEAK")
    view = game.play_card(0, 0)
    projected = _project(view)
    assert projected["_unclassified"] == []
    kept = projected["removed_enemies"]
    assert [(e["combat_id"], e["id"]) for e in kept] == [
        (e["combat_id"], e["id"]) for e in view["removed_enemies"]
    ]
    assert all("move_state" not in e and "ai" not in e for e in kept)


def test_every_enemy_names_its_encounter_slot(game):
    """GH #1838: the board is sorted by encounter slot; a corpse keeps its
    slot. The host sends each enemy's slot index, the removed ones too. The
    rats stand in slots 2-4 and summon into the free ones in front."""
    state = game.start(character="Ironclad", seed="REMOVED3")
    game.skip_neow(state)
    r = game.set_player(hp=70, max_hp=80, deck=["BLUDGEON"] * 6, relics=[], potions=[])
    assert r.get("type") != "error", r
    before = game.enter_room("combat", encounter="TWO_TAILED_RATS_NORMAL")
    assert [e.get("slot") for e in before["enemies"]] == [2, 3, 4], before["enemies"]
    victim = before["enemies"][0]
    after = game.play_card(0, 0)
    (removed,) = after["removed_enemies"]
    assert (removed["combat_id"], removed["slot"]) == (victim["combat_id"], 2)
