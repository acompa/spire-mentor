"""Unit tests for the agent-mode parity lane's offer normaliser and diff (GH #1610).

Pure Python: views are hand-built dicts in the C# decision shape, so these
need neither the game process nor the wheel.
"""

from __future__ import annotations

import agent_parity as ap


def _keys(offers: list[ap.Offer]) -> list[tuple]:
    return [o.key for o in offers]


def test_canon_strips_prefixes_and_loc_suffix() -> None:
    assert ap.canon("CARD.STRIKE_IRONCLAD") == "STRIKEIRONCLAD"
    assert ap.canon("RELIC.ANCHOR") == "ANCHOR"
    assert ap.canon("BURNING_BLOOD.title") == "BURNINGBLOOD"
    assert ap.canon(None) == ""


def test_card_reward_keys_carry_id_upgrade_and_alternatives() -> None:
    view = {
        "decision": "card_reward",
        "cards": [
            {"index": 0, "id": "CARD.BASH", "upgraded": True},
            {"index": 1, "id": "CARD.ANGER", "upgraded": False},
        ],
        "alternatives": [{"index": 0, "id": "REROLL"}],
        "can_skip": True,
    }
    offers = ap.offers_from_view(view)
    assert _keys(offers) == [
        ("card_reward", "BASH", True),
        ("card_reward", "ANGER", False),
        ("card_reward_alternative", "REROLL"),
        ("skip_card_reward",),
    ]
    assert offers[1].verb == "select_card_reward"
    assert offers[1].kw == {"card_index": 1}


def test_shop_keys_carry_price_and_drop_unaffordable_and_sold_out() -> None:
    view = {
        "decision": "shop",
        "player": {"gold": 100},
        "cards": [
            {"index": 0, "id": "CARD.ANGER", "cost": 50, "is_stocked": True},
            {"index": 1, "id": "CARD.BASH", "cost": 150, "is_stocked": True},
            {"index": 2, "id": "CARD.CLASH", "cost": 40, "is_stocked": False},
        ],
        "relics": [{"index": 0, "id": "RELIC.ANCHOR", "cost": 99, "is_stocked": True}],
        "potions": [],
        "card_removal_cost": 75,
    }
    assert _keys(ap.offers_from_view(view)) == [
        ("buy_card", "ANGER", False, 50),
        ("buy_relic", "ANCHOR", False, 99),
        ("remove_card", 75),
        ("leave_room",),
    ]


def test_rewards_screen_offers_take_and_skip_per_reward() -> None:
    view = {
        "decision": "rewards",
        "rewards": [
            {"index": 0, "type": "gold", "amount": 17},
            {"index": 1, "type": "relic", "id": "ANCHOR"},
        ],
    }
    assert _keys(ap.offers_from_view(view)) == [
        ("take_reward", "gold", "17"),
        ("take_reward", "relic", "ANCHOR"),
        ("skip_reward", "gold", "17"),
        ("skip_reward", "relic", "ANCHOR"),
        ("proceed_rewards",),
    ]
    # A set built WithSkippingDisallowed (Neow's Bones, #1641): takes only.
    assert _keys(ap.offers_from_view({**view, "can_skip": False})) == [
        ("take_reward", "gold", "17"),
        ("take_reward", "relic", "ANCHOR"),
    ]


def test_card_select_offers_bounds_cards_and_optional_skip() -> None:
    view = {
        "decision": "card_select",
        "min_select": 0,
        "max_select": 1,
        "cards": [{"index": 0, "id": "CARD.STRIKE_IRONCLAD"}],
    }
    assert _keys(ap.offers_from_view(view)) == [
        ("select_bounds", 0, 1),
        ("select_card", "STRIKEIRONCLAD", False),
        ("skip_select",),
    ]


def test_combat_offers_key_on_card_identity_not_hand_index() -> None:
    view = {
        "decision": "combat_play",
        "hand": [{"index": 0, "id": "CARD.BASH"}, {"index": 1, "id": "CARD.DEFEND_IRONCLAD"}],
        "player": {"potions": [{"index": 0, "entry": "FIRE_POTION"}]},
    }
    legal = [
        {"action": "play_card", "card_index": 0, "target_index": 1},
        {"action": "play_card", "card_index": 1, "target_index": None},
        {"action": "use_potion", "potion_index": 0, "target_index": 0},
        {"action": "end_turn"},
    ]
    assert _keys(ap.combat_offers(view, legal)) == [
        ("play_card", "BASH", False, 1),
        ("play_card", "DEFENDIRONCLAD", False, None),
        ("use_potion", "FIREPOTION", 0),
        ("end_turn",),
    ]


def test_diff_offers_is_a_multiset_difference() -> None:
    a = [ap.Offer(("play_card", "STRIKE", False, 0), "play_card")] * 2 + [
        ap.Offer(("end_turn",), "end_turn")
    ]
    b = [
        ap.Offer(("play_card", "STRIKE", False, 0), "play_card"),
        ap.Offer(("end_turn",), "end_turn"),
        ap.Offer(("discard_potion", "X"), ""),
    ]
    only_cs, only_rs = ap.diff_offers(a, b)
    assert only_cs == [("play_card", "STRIKE", False, 0)]
    assert only_rs == [("discard_potion", "X")]


def test_identical_offers_diff_empty() -> None:
    view = {"decision": "map_select", "choices": [{"col": 1, "row": 0}, {"col": 3, "row": 0}]}
    assert ap.diff_offers(ap.offers_from_view(view), ap.offers_from_view(view)) == ([], [])


def test_classify_names_the_known_gaps_and_leaves_the_rest_new() -> None:
    # Every reward screen, an event's or relic's own included, is on both
    # sides since #1620 / #1641: a one-sided reward offer is NEW.
    assert ap.classify("rewards", "cs", ("take_reward", "gold", "10")) is None
    assert ap.classify("rewards", "cs", ("use_potion_ooc", "X")) is None
    assert "event_reward_screen" not in ap.KNOWN_GAPS
    assert (
        ap.classify("card_reward", "cs", ("card_reward_alternative", "REROLL"))
        == "driftwood_reroll"
    )
    assert (
        ap.classify("crystal_sphere", "cs", ("crystal_sphere_click", 1, 2, "big"))
        == "crystal_sphere"
    )
    assert (
        ap.classify("event_choice", "cs", ("event_option", 1), "DOUBLEDOWN")
        == "double_down_withheld"
    )
    assert ap.classify("card_reward", "rs", ("discard_potion", "X")) == "host_overlay_belt"
    # The headless host has no merchant node for Foul Potion to be thrown at.
    assert (
        ap.classify("shop", "rs", ("use_potion_ooc", "FOULPOTION")) == "host_foul_potion_merchant"
    )
    assert ap.classify("map_select", "rs", ("use_potion_ooc", "FOULPOTION")) is None
    # Rust's combat select stops at 255 cards (#1635).
    cap = frozenset({"option_cap"})
    key = ("select_card", "WOUND", False)
    assert ap.classify("card_select", "cs", key, ctx=cap) == "select_option_cap"
    assert ap.classify("card_select", "cs", key) is None
    # A Rust-only extra and anything unknown are NEW.
    assert ap.classify("rewards", "rs", ("take_reward", "gold", "10")) is None
    assert ap.classify("combat_play", "rs", ("play_card", "BASH", False, 0)) is None
    for gap in (
        "driftwood_reroll",
        "crystal_sphere",
        "double_down_withheld",
    ):
        assert gap in ap.KNOWN_GAPS


def test_closed_reward_screen_gaps_are_new_again() -> None:
    # GH #1620 closed: a combat room's or chest's reward screen is on both
    # sides, so a one-sided take / skip / proceed, or the potion belt on the
    # screen, is a NEW difference.
    for gap in ("reward_screen", "ooc_potion_screen"):
        assert gap not in ap.KNOWN_GAPS
    for key in (
        ("take_reward", "gold", "10"),
        ("skip_reward", "relic", "GORGET"),
        ("proceed_rewards",),
        ("use_potion_ooc", "BLOODPOTION"),
    ):
        assert ap.classify("rewards", "cs", key) is None


def test_closed_select_gaps_are_new_again() -> None:
    # GH #1619 / #1621 closed: a C#-only Smith / Cook skip or a pick-N mismatch
    # is a NEW difference now, whatever the walk context.
    for gap in ("smith_not_cancelable", "batched_select"):
        assert gap not in ap.KNOWN_GAPS
    assert ap.classify("card_select", "cs", ("skip_select",)) is None
    assert ap.classify("card_select", "cs", ("select_first_n", 4)) is None
    assert ap.classify("card_select", "rs", ("select_bounds", 1, 1)) is None


def test_skippable_select_bounds_compare_equal_across_representations() -> None:
    # C#: min 0 / max 1. Rust: min 1 / max 1 with can_skip (Attack Potion's pick).
    cards = [{"index": 0, "id": "CARD.ANGER"}]
    cs = {"decision": "card_select", "min_select": 0, "max_select": 1, "cards": cards}
    rs = {
        "decision": "card_select",
        "min_select": 1,
        "max_select": 1,
        "can_skip": True,
        "cards": cards,
    }
    assert ap.diff_offers(ap.offers_from_view(cs), ap.offers_from_view(rs)) == ([], [])


def test_pick_n_select_offers_no_single_pick() -> None:
    view = {
        "decision": "card_select",
        "min_select": 2,
        "max_select": 2,
        "cards": [{"index": i, "id": "CARD.STRIKE_IRONCLAD"} for i in range(3)],
    }
    playable = [o for o in ap.offers_from_view(view) if o.verb]
    assert [o.key for o in playable] == [("select_first_n", 2)]
    assert playable[0].kw == {"indices": "0,1"}


def test_one_sided_selects_name_their_gap() -> None:
    assert ap.classify_cs_alone_select("CARD.ENTROPY") == "turn_start_pick"
    assert ap.classify_cs_alone_select("CARD.DUAL_WIELD") == "turn_start_pick"
    assert ap.classify_cs_alone_select("CARD.HAVOC") is None
    # #1623 / #1624 closed: a Rust-only select has no known gap any more.
    assert "select_after_combat_end" not in ap.KNOWN_GAPS
    assert "forced_deck_select" not in ap.KNOWN_GAPS
    assert not hasattr(ap, "classify_rs_alone_select")


def test_parasol_removal_is_a_host_gap_only_with_the_relic() -> None:
    key = ("remove_card", 75)
    assert ap.classify("shop", "cs", key, ctx=frozenset({"parasol"})) == "host_parasol_removal"
    assert ap.classify("shop", "cs", key) is None
