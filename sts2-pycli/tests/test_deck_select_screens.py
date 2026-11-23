"""The game's deck select screens on the Rust backend (GH #1619, #1621).

The agent-parity lane builds its Rust run with ``deck_select_screens``: Smith's
and Cook's selects are cancelable (``skip_select``) and a pick-N deck select is
one ``card_select`` whose ``select_cards`` takes every index at once.
"""

from __future__ import annotations

import sts2_rs
from workbench.backends.base import make_backend


def _lane_game():
    g = make_backend("rust", target_convention="csharp", agent_lane=True)()
    g.start(character="Ironclad", seed="DSS1", ascension=0, interactive_potion_swap=True)
    assert g.rm.deck_select_screens
    return g


def _smith_option(view):
    return next(o["index"] for o in view["options"] if o["option_id"] == "SMITH")


def test_smith_select_cancels_back_to_the_campfire():
    g = _lane_game()
    g.rm.debug_clear_pending_choices()
    g.rm.debug_enter_rest_site([])
    view = g._serialize()
    assert view["decision"] == "rest_site"
    deck = list(g.rm.master_deck_detailed())
    sel = g.act("choose_option", option_index=_smith_option(view))
    assert sel["decision"] == "card_select"
    assert sel["can_skip"] and sel["cancelable"]
    assert (sel["min_select"], sel["max_select"]) == (1, 1)
    back = g.act("skip_select")
    assert back["decision"] == "rest_site"
    assert [o["option_id"] for o in back["options"]] == [o["option_id"] for o in view["options"]]
    assert list(g.rm.master_deck_detailed()) == deck


def test_pick_n_upgrade_is_one_select():
    g = _lane_game()
    g.rm.debug_clear_pending_choices()
    g.rm.debug_queue_deck_upgrades(2)
    view = g._serialize()
    assert view["decision"] == "card_select"
    assert (view["min_select"], view["max_select"]) == (2, 2)
    assert not view["can_skip"]
    before = sum(up for _, up in g.rm.master_deck_detailed())
    g.act("select_cards", indices="0,1")
    assert g.rm.pending_deck_upgrades == 0
    assert sum(up for _, up in g.rm.master_deck_detailed()) == before + 2


def test_parity_mode_keeps_the_oracle_select():
    rm = sts2_rs.RunManager(seed=1, character="Ironclad", mode="agent")
    assert not rm.deck_select_screens
    rm.debug_enter_rest_site([])
    rm.take_action({"action": "rest_option", "idx": 1})
    assert {"action": "confirm_deck_selection"} not in rm.available_actions()
    assert not rm.deck_select_cancelable
