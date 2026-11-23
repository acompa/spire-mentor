"""GH #1286: Crystal Sphere clicks planned from the rewards a log shows.

A replay log holds what the player dug up in the Crystal Sphere, not where they
dug. `host_replay_sphere.plans` turns the host's list of the items under the
fog into click sequences that reveal exactly those rewards.
"""

from __future__ import annotations

from collections import Counter
from typing import Any

import host_replay_sphere as sphere
import pytest
from sts2_client import Game


def _state(items: list[dict[str, Any]], divinations: int, size: int = 7) -> dict[str, Any]:
    return {
        "grid": {"width": size, "height": size},
        "divinations_left": divinations,
        "cells": [{"x": x, "y": y, "hidden": True} for y in range(size) for x in range(size)],
        "items": items,
    }


def _item(kind: str, x: int, y: int, w: int = 1, h: int = 1) -> dict[str, Any]:
    return {"kind": kind, "x": x, "y": y, "width": w, "height": h}


def _revealed(state: dict[str, Any], clicks: list[sphere.Click]) -> list[str]:
    out = sphere._Grid(state).play(clicks)
    assert out is not None
    return [state["items"][k]["kind"] for k in out]


def test_a_plan_reveals_the_wanted_item_and_no_other():
    state = _state(
        [
            _item("crystal_sphere_common_card_reward", 0, 0, 2, 2),
            _item("crystal_sphere_gold", 2, 0),  # next to the card: a careless dig takes it
            _item("crystal_sphere_relic", 3, 3, 4, 4),
        ],
        divinations=3,
    )
    found = sphere.plans(state, Counter(card=1))
    assert found
    for clicks in found:
        assert len(clicks) == 3  # every divination is spent
        assert _revealed(state, clicks) == ["crystal_sphere_common_card_reward"]


def test_nothing_wanted_is_a_plan_too():
    state = _state([_item("crystal_sphere_gold", 3, 3)], divinations=2)
    (clicks,) = sphere.plans(state, Counter())
    assert len(clicks) == 2
    assert _revealed(state, clicks) == []


def test_one_plan_for_each_order_that_rolls_differently():
    # A common and a rare potion roll off one stream: which is revealed first
    # decides what each is. Potions are drawn before the gold, wherever it lies.
    state = _state(
        [
            _item("crystal_sphere_common_potion", 0, 0, 1, 3),
            _item("crystal_sphere_rare_potion", 4, 4, 2, 2),
            _item("crystal_sphere_gold", 6, 0),
        ],
        divinations=3,
    )
    found = sphere.plans(state, Counter(potion=2, gold=1))
    orders = {tuple(k for k in _revealed(state, clicks) if "potion" in k) for clicks in found}
    assert orders == {
        ("crystal_sphere_common_potion", "crystal_sphere_rare_potion"),
        ("crystal_sphere_rare_potion", "crystal_sphere_common_potion"),
    }
    assert len(found) == 2


def test_gold_dug_up_before_a_card_reward_moves_its_draw():
    # Gold draws a number as the reward screen fills, like the cards: a plan
    # for the gold first and one for the card reward first.
    state = _state(
        [
            _item("crystal_sphere_rare_card_reward", 0, 0, 2, 2),
            _item("crystal_sphere_big_gold", 4, 4, 2, 1),
        ],
        divinations=2,
    )
    found = sphere.plans(state, Counter(rare_card=1, big_gold=1))
    assert {tuple(sphere.item_class(k) for k in _revealed(state, c)) for c in found} == {
        ("card", "big_gold"),
        ("big_gold", "card"),
    }
    assert len(found) == 2


def test_a_card_reward_the_host_places_decides_the_first_plan():
    # The host tells that the log's rare cards come after one draw: the gold
    # was dug up first.
    state = _state(
        [
            _item("crystal_sphere_rare_card_reward", 0, 0, 2, 2),
            _item("crystal_sphere_big_gold", 4, 4, 2, 1),
        ],
        divinations=2,
    )
    want = Counter(rare_card=1, big_gold=1)
    for at, first in ((1, "big_gold"), (0, "card")):
        found = sphere.plans(state, want, ["rare_card"], {"rare_card": at})
        assert sphere.item_class(_revealed(state, found[0])[0]) == first
        assert len(found) == 2


def test_a_relic_or_a_potion_draws_an_unknown_count():
    card, gold, relic = "crystal_sphere_rare_card_reward", "gold", "crystal_sphere_relic"
    at = {"rare_card": 2}
    assert sphere._misplaced([gold, gold, card], at) == 0
    assert sphere._misplaced([gold, card, gold], at) == 1
    assert sphere._misplaced([relic, gold, card], at) == 0
    assert sphere._misplaced(["crystal_sphere_common_potion", card], at) == 0
    # A second reward is counted from the first: six draws a card reward.
    two = {"rare_card": 0, "common_card": 7}
    common = "crystal_sphere_common_card_reward"
    assert sphere._misplaced([card, gold, common], two) == 0
    assert sphere._misplaced([card, common, gold], two) == 1


def test_a_wanted_rarity_picks_the_item_of_that_rarity():
    # A log's card reward tells its rarity; the plans dig up that item only.
    state = _state(
        [
            _item("crystal_sphere_common_card_reward", 0, 0, 2, 2),
            _item("crystal_sphere_rare_card_reward", 4, 0, 2, 2),
            _item("crystal_sphere_uncommon_card_reward", 0, 4, 2, 2),
        ],
        divinations=2,
    )
    found = sphere.plans(state, Counter(rare_card=1))
    assert found
    for clicks in found:
        assert _revealed(state, clicks) == ["crystal_sphere_rare_card_reward"]
    # A potion nothing is known of may be of any rarity, but is not counted twice.
    potions = _state(
        [
            _item("crystal_sphere_common_potion", 0, 0, 1, 3),
            _item("crystal_sphere_rare_potion", 4, 0, 2, 2),
        ],
        divinations=2,
    )
    for clicks in sphere.plans(potions, Counter(rare_potion=1, potion=1)):
        assert len(_revealed(potions, clicks)) == 2
    assert sphere.plans(potions, Counter(rare_potion=1, potion=2)) == []


def test_the_plan_in_the_logs_order_comes_first():
    state = _state(
        [
            _item("crystal_sphere_common_card_reward", 0, 0, 2, 2),
            _item("crystal_sphere_rare_card_reward", 4, 0, 2, 2),
        ],
        divinations=2,
    )
    want = Counter(rare_card=1, common_card=1)
    for order in (["rare_card", "common_card"], ["common_card", "rare_card"]):
        first = sphere.plans(state, want, order)[0]
        assert [sphere.item_name(k) for k in _revealed(state, first)] == order


def test_no_plan_when_the_divinations_cannot_reach():
    state = _state(
        [_item("crystal_sphere_gold", 0, 0), _item("crystal_sphere_gold", 6, 6)], divinations=1
    )
    assert sphere.plans(state, Counter(gold=2)) == []
    assert sphere.plans(state, Counter(relic=1)) == []


@pytest.mark.parametrize(
    ("kind", "cls"),
    [
        ("crystal_sphere_relic", "relic"),
        ("crystal_sphere_common_potion", "potion"),
        ("crystal_sphere_rare_potion", "potion"),
        ("crystal_sphere_uncommon_card_reward", "card"),
        ("crystal_sphere_gold", "gold"),
        ("crystal_sphere_big_gold", "big_gold"),
        ("crystal_sphere_curse", "curse"),
    ],
)
def test_item_classes(kind, cls):
    assert sphere.item_class(kind) == cls


def _enter_sphere(game: Any) -> dict[str, Any]:
    game.start(seed="SPHERE1")
    state = game.enter_room("event", event="CRYSTAL_SPHERE")
    return game.act("choose_option", option_index=state["options"][0]["index"])


def test_the_host_lists_the_items_only_when_asked(game, monkeypatch):
    monkeypatch.setenv("STS2_CRYSTAL_SPHERE_INTERACTIVE", "1")
    monkeypatch.delenv("STS2_CRYSTAL_SPHERE_REVEAL", raising=False)
    g = Game()
    try:
        state = _enter_sphere(g)
        assert state["decision"] == "crystal_sphere"
        assert "items" not in state  # a player does not see under the fog
    finally:
        g.close()


def test_a_plan_played_on_the_host_gives_the_wanted_rewards(game, monkeypatch):
    monkeypatch.setenv("STS2_CRYSTAL_SPHERE_INTERACTIVE", "1")
    monkeypatch.setenv("STS2_CRYSTAL_SPHERE_REVEAL", "1")
    monkeypatch.setenv("STS2_INTERACTIVE_REWARDS", "1")
    g = Game()
    try:
        state = _enter_sphere(g)
        assert state["decision"] == "crystal_sphere"
        kinds = Counter(sphere.item_class(i["kind"]) for i in state["items"])
        # CrystalSphereMinigame.PopulateItems.
        assert kinds == Counter(relic=1, potion=3, card=3, curse=1, gold=5, big_gold=2)
        gold = state["player"]["gold"]
        want = Counter(card=1, big_gold=1)
        clicks = sphere.plans(state, want)[0]
        assert len(clicks) == state["divinations_left"]
        for x, y, tool in clicks:
            assert state["decision"] == "crystal_sphere"
            state = g.act("crystal_sphere_click", x=x, y=y, tool=tool)
        assert state["decision"] == "rewards"
        assert Counter(r["type"] for r in state["rewards"]) == Counter(card=1, gold=1)
        assert state["player"]["gold"] == gold  # nothing is taken yet
    finally:
        g.close()
