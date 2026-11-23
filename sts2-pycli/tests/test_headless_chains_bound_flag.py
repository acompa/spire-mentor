"""The host's Chains of Binding flag is the power's own (GH #1826).

``ChainsOfBindingPower.BeforeCardPlayed`` marks a play only for a card Bound
when its play starts. Hellraiser auto-plays a drawn Strike in
``AfterCardDrawnEarly``, before Chains afflicts it in ``AfterCardDrawn``, so
that play does not use up the turn's one Bound card. The host rebuilt the
flag from history with each card's affliction *now* and reported a Bound
card played, so the Instinct mirror treated the hand's Bound cards as
unplayable while the game let them be played.
"""

from __future__ import annotations

from typing import Any


def _index(view: dict[str, Any], card: str) -> int | None:
    hand = view.get("hand") or []
    return next((i for i, c in enumerate(hand) if c["id"] == card), None)


def test_a_hellraiser_auto_play_does_not_use_up_the_bound_card(game) -> None:
    state = game.start(character="Ironclad", seed="CHAINS1826")
    game.skip_neow(state)
    deck = ["HELLRAISER", *["STRIKE_IRONCLAD"] * 5, *["DEFEND_IRONCLAD"] * 6]
    r = game.set_player(hp=300, max_hp=300, deck=deck, relics=[], potions=[])
    assert r.get("type") != "error", r
    view = game.enter_room("combat", encounter="QUEEN_BOSS")
    if _index(view, "CARD.HELLRAISER") is None:
        assert game.set_draw_order(["HELLRAISER"]).get("type") != "error"
        view = game.end_turn()
    idx = _index(view, "CARD.HELLRAISER")
    assert idx is not None, view.get("hand")
    view = game.play_card(idx, None)
    assert view.get("decision") == "combat_play", view
    # Queen opens with Puppet Strings (Chains of Binding 3). Next turn's draw
    # starts with two Strikes (Hellraiser plays them) and then Defends.
    order = ["STRIKE_IRONCLAD", "STRIKE_IRONCLAD", *["DEFEND_IRONCLAD"] * 5]
    assert game.set_draw_order(order).get("type") != "error"
    view = game.end_turn()
    assert view.get("decision") == "combat_play", view
    powers = [p.get("id") for p in (view.get("player_powers") or [])]
    assert any("CHAINS_OF_BINDING" in str(p) for p in powers), powers
    bound = [c for c in view.get("hand") or [] if c.get("affliction") == "Bound"]
    assert bound, view.get("hand")
    assert (view.get("player") or {}).get("cards_played_this_turn", [0])[0] >= 1
    # The game lets the Bound cards be played, and the flag agrees.
    assert all(c.get("can_play") for c in bound), bound
    assert not (view.get("player") or {}).get("bound_card_played_this_turn"), view["player"]
