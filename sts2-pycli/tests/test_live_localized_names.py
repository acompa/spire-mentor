"""GH #1423: the live path resolves combat entities by id, not by English title.

The mod sends every ``name`` in the game's language (``card.Title``,
``orb.Title.GetFormattedText()``, ...). On a Chinese game those are Chinese,
so any lookup keyed on the English title misses. These tests feed real mod
captures (``tests/fixtures/realgame_adapter/``) through the adapter twice:
as recorded, and with every ``name`` replaced by Chinese text. The engine
board built from both must be identical.
"""

from __future__ import annotations

import copy
import json
import sys
from pathlib import Path
from typing import Any

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "python"))

from realgame_adapter import to_headless_combat_play
from sts2_hint import adapter as hint_adapter
from sts2_hint.adapter import translate

FIXTURE_DIR = Path(__file__).resolve().parent / "fixtures" / "realgame_adapter"

try:
    import sts2_rs  # type: ignore[import-not-found]

    _HAS_ENGINE = True
except ImportError:
    _HAS_ENGINE = False

needs_engine = pytest.mark.skipif(not _HAS_ENGINE, reason="sts2_rs not importable")

FIXTURES = sorted(p.name for p in FIXTURE_DIR.glob("*.json"))

#: A potion in the belt, as ``SimStateExtractor.SerializePotionSlot`` sends it.
#: The fixtures were all recorded with an empty belt.
_FIRE_POTION = {
    "slot": 0,
    "empty": False,
    "id": "FIRE_POTION",
    "name": "Fire Potion",
    "rarity": "Common",
    "usage": "CombatOnly",
    "can_use": True,
    "target_type": "AnyEnemy",
    "requires_target": True,
}


def _load(name: str) -> dict[str, Any]:
    with (FIXTURE_DIR / name).open(encoding="utf-8") as fh:
        state = json.load(fh)["state"]
    state["potions"] = [copy.deepcopy(_FIRE_POTION), *(state.get("potions") or [])[1:]]
    return state


def _localize(node: Any, counter: list[int] | None = None) -> Any:
    """A copy of *node* with every ``name`` replaced by distinct Chinese text."""
    counter = counter if counter is not None else [0]
    if isinstance(node, dict):
        out = {}
        for k, v in node.items():
            if k == "name" and isinstance(v, str):
                counter[0] += 1
                out[k] = f"闪电{counter[0]}号"
            else:
                out[k] = _localize(v, counter)
        return out
    if isinstance(node, list):
        return [_localize(v, counter) for v in node]
    return node


def _wire(state: dict[str, Any]) -> dict[str, Any]:
    hint_adapter.reset_drop_counts()
    wire = translate(to_headless_combat_play(state))
    assert hint_adapter.drop_counts() == {}, "the board soft-dropped an entity"
    return wire


@needs_engine
@pytest.mark.parametrize("fixture", FIXTURES)
def test_localized_names_build_the_same_board(fixture: str) -> None:
    state = _load(fixture)
    english = _wire(state)
    chinese = _wire(_localize(state))
    assert chinese == english
    sts2_rs.state_from_dict(chinese)  # and the engine builds it


@needs_engine
def test_the_fixtures_cover_what_the_test_resolves() -> None:
    """Guards the test above against fixtures that hold nothing to resolve."""
    wire = _wire(_load("orbs_defect.json"))
    assert [o["type"] for o in wire["player"]["orbs"]] == ["LIGHTNING", "LIGHTNING"]
    assert wire["player"]["potions"][0] == "FIRE_POTION"
    assert wire["encounter"]
    wire = _wire(_load("dead_osty_ironclad.json"))
    assert "curse:Decay" in {c["id"] for c in wire["deck"]["draw_pile"]}


@needs_engine
@pytest.mark.parametrize(
    ("raw_id", "kind"),
    [
        ("SHRINKER_BEETLE", "ShrinkerBeetle"),
        ("BATTLE_FRIEND_V1", "BattleFriendV1"),
        ("TWIG_SLIME_M", "TwigSlimeM"),
        ("AXE_RUBY_RAIDER", "AxeRubyRaider"),
        # One title ("Decimillipede") for three kinds; only the id tells them apart.
        ("DECIMILLIPEDE_SEGMENT_FRONT", "DecimillipedeSegmentFront"),
        ("DECIMILLIPEDE_SEGMENT_BACK", "DecimillipedeSegmentBack"),
    ],
)
def test_monster_kind_from_the_mod_id(raw_id: str, kind: str) -> None:
    assert hint_adapter._monster_kind_from_wire({"id": raw_id, "name": "某怪物"}) == kind


def test_an_unknown_monster_id_falls_back_to_the_title() -> None:
    assert hint_adapter._monster_kind_from_wire({"id": "NOT_A_MONSTER"}) is None
    assert hint_adapter._monster_kind_from_wire({"name": "Shrinker Beetle"}) is None
    assert hint_adapter._monster_kind_from_title("Shrinker Beetle", "$") == "ShrinkerBeetle"


@pytest.mark.parametrize(
    ("orb", "stem"),
    [
        ({"id": "LIGHTNING_ORB", "name": "闪电"}, "Lightning"),
        ({"id": "LightningOrb", "name": "闪电"}, "Lightning"),
        ({"id": "FROST_ORB", "name": "冰霜"}, "Frost"),
        ({"id": "PLASMA_ORB", "name": "等离子"}, "Plasma"),
        ({"id": "DARK_ORB", "name": "黑暗"}, "Dark"),
        ({"id": "GLASS_ORB", "name": "玻璃"}, "Glass"),
        ({"id": "EMPTY_ORB", "name": "充能球栏位"}, "Orb Slot"),
        # No id (an older producer): the name is all there is.
        ({"name": "Lightning"}, "Lightning"),
    ],
)
def test_orb_type_stem_prefers_the_id(orb: dict[str, Any], stem: str) -> None:
    board = {"type": "combat_action", "orbs": [orb]}
    assert to_headless_combat_play(board)["orbs"][0]["type"] == stem


@needs_engine
def test_potion_resolves_by_id_with_the_title_as_fallback() -> None:
    assert hint_adapter._translate_potion({"id": "FIRE_POTION", "name": "火焰药水"}, "$") == (
        "FIRE_POTION"
    )
    assert hint_adapter._translate_potion({"name": "Fire Potion"}, "$") == "FIRE_POTION"
    assert hint_adapter._translate_potion("Fire Potion", "$") == "FIRE_POTION"
    assert hint_adapter._translate_potion({"slot": 0, "empty": True}, "$") is None


@needs_engine
@pytest.mark.parametrize(
    ("encounter", "kinds"),
    [
        ("SHRINKER_BEETLE_WEAK", ["ShrinkerBeetle"]),
        # One English title, three kinds: only the id tells the segments apart.
        (
            "DECIMILLIPEDE_ELITE",
            [
                "DecimillipedeSegmentFront",
                "DecimillipedeSegmentMiddle",
                "DecimillipedeSegmentBack",
            ],
        ),
    ],
)
def test_headless_boards_resolve_by_id_too(game: Any, encounter: str, kinds: list[str]) -> None:
    """The headless host sends the enemy ``id`` as well, and its board builds the
    same with every name localized (the GH #1423 comment's headless half)."""
    game.skip_neow(game.start(seed="loc1423"))
    state = game.enter_room("combat", encounter=encounter)
    assert state["decision"] == "combat_play"
    assert all(e.get("id") for e in state["enemies"]), state["enemies"]
    hint_adapter.reset_drop_counts()
    english = translate(state)
    chinese = translate(_localize(state))
    assert hint_adapter.drop_counts() == {}
    assert sorted(e["kind"] for e in english["encounter"]) == sorted(kinds)
    assert chinese == english


def test_a_headless_potion_resolves_by_its_entry() -> None:
    """The headless host names a potion's registry id ``entry`` (``id`` is the
    mod's). Ambergris has no title in the v0.107.1 title map, so read by title
    it was soft-dropped from every fight (Spire Bench seeds 170000047, 058)."""
    raw = {"slot": 0, "entry": "AMBERGRIS", "name": "Ambergris"}
    assert hint_adapter._translate_potion(raw, "$") == "AMBERGRIS"
