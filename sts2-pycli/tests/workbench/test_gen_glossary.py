"""The checked-in hover-tip glossaries match their generator (GH #1598-#1600).

The page reads ``web/public/[<ver>/]glossary.json`` for enchantment tabs, term
hover tips and the "View upgrades" faces. A localization or wiki-data bump
without a regenerated file would leave the page describing the old build.
"""

from __future__ import annotations

import json

import pytest
from sts2_client import SUPPORTED_GAME_VERSIONS
from workbench import gen_glossary
from workbench.localization import loc_dir_for

_VERSIONS = [v for v in SUPPORTED_GAME_VERSIONS if loc_dir_for(v).is_dir()]


@pytest.mark.parametrize("version", _VERSIONS)
def test_shipped_glossary_is_fresh(version: str) -> None:
    shipped = gen_glossary.out_path_for(version)
    assert shipped.is_file(), (
        f"run: PYTHONPATH=python python -m workbench.gen_glossary --version {version}"
    )
    fresh = gen_glossary.build(version)
    assert json.loads(shipped.read_text(encoding="utf-8")) == fresh, (
        f"{shipped} is stale; regenerate with "
        f"`PYTHONPATH=python uv run python -m workbench.gen_glossary --version {version}`"
    )


def test_glossary_names_what_the_page_reads() -> None:
    g = gen_glossary.build("v0.113.0")
    adroit = g["enchantments"]["ADROIT"]
    assert adroit["title"] == "Adroit"
    assert adroit["image"] == "enchantments/adroit.png"
    assert "{" in adroit["description"]  # a template the page fills with the amount
    assert g["keywords"]["EXHAUST"]["title"] == "Exhaust"
    assert (
        g["upgrades"]["BASH"]["description"] == "Deal 10 damage.\nApply 3 [gold]Vulnerable[/gold]."
    )
    assert g["upgrades"]["BODY_SLAM"]["cost"] == 0


# --- the Chinese tables (``--lang zhs``) ------------------------------------


@pytest.mark.parametrize("version", _VERSIONS)
def test_shipped_zh_glossary_is_fresh(version: str) -> None:
    shipped = gen_glossary.out_path_for(version, "zhs")
    assert shipped.is_file(), (
        f"run: PYTHONPATH=python python -m workbench.gen_glossary --version {version} --lang zhs"
    )
    fresh = gen_glossary.build(version, "zhs")
    assert json.loads(shipped.read_text(encoding="utf-8")) == fresh, (
        f"{shipped} is stale; regenerate with `PYTHONPATH=python uv run python -m "
        f"workbench.gen_glossary --version {version} --lang zhs`"
    )


def test_zh_out_path_sits_under_public_zh() -> None:
    en = gen_glossary.out_path_for(None)
    assert gen_glossary.out_path_for(None, "zhs") == en.parent / "zh" / "glossary.json"
    assert gen_glossary.out_path_for("v0.113.0", "zhs") == (
        en.parent / "zh" / "v0.113.0" / "glossary.json"
    )
    with pytest.raises(ValueError):
        gen_glossary.out_path_for(None, "fr")


def test_zh_glossary_is_the_english_one_in_the_games_chinese() -> None:
    """Same entries as the English table, the game's own Chinese text, and a
    filled template wherever the English one is filled."""
    en = gen_glossary.build("v0.113.0")
    zh = gen_glossary.build("v0.113.0", "zhs")
    for section in en:
        assert set(zh[section]) == set(en[section]), section
    assert zh["enchantments"]["ADROIT"]["title"] == "伶俐"
    assert zh["enchantments"]["ADROIT"]["image"] == en["enchantments"]["ADROIT"]["image"]
    assert zh["keywords"]["EXHAUST"]["title"] == "消耗"
    assert zh["relics"]["AKABEKO"]["title"] == "赤牛"
    # The relic's Chinese template, filled with the wiki's base value.
    assert (
        zh["relics"]["AKABEKO"]["description"]
        == "在每场战斗开始时，获得[blue]8[/blue]点[gold]活力[/gold]。"
    )
    # The upgraded face: Chinese text with the upgraded numbers, the keyword
    # lines in Chinese with the game's full stop, energy as card-face icons.
    assert zh["upgrades"]["BASH"]["description"] == "造成10点伤害。\n给予3层[gold]易伤[/gold]。"
    assert zh["upgrades"]["ADRENALINE"]["description"] == (
        "获得[energy]2[/energy]。\n抽2张牌。\n[gold]消耗[/gold]。"
    )
    assert zh["upgrades"]["BODY_SLAM"]["cost"] == 0
    # A Chinese text is filled or not used: no raw placeholder the English
    # text did not already have.
    for kind in ("relics", "potions"):
        for key, row in zh[kind].items():
            english = en[kind][key]["description"]
            assert "{" not in row["description"] or row["description"] == english, (kind, key)


def test_zh_render_keeps_english_when_a_value_is_missing() -> None:
    assert gen_glossary.zh_render("造成{Damage:diff()}点伤害。", {"damage": 6}.get) == (
        "造成6点伤害。"
    )
    assert gen_glossary.zh_render("造成{Damage:diff()}点伤害。", {}.get) is None
    # Plain text spells the energy out, and does not repeat a number the
    # template already prints before the glyph.
    spelled = gen_glossary.zh_render(
        "获得{Energy:energyIcons()}，再额外获得1点{Energy:energyIcons()}。",
        {"energy": 1}.get,
        icons=False,
    )
    assert spelled == "获得1点能量，再额外获得1点能量。"
    # A chooser an older build has no value for, and whose English text does
    # not read it, takes its first branch ("hits twice" printed outright).
    assert (
        gen_glossary.zh_render(
            "造成{Damage:diff()}点伤害{HitCount:choose(2):两次|{HitCount:diff()}次}。",
            {"damage": 5}.get,
            english="Deal {Damage:diff()} damage twice.",
        )
        == "造成5点伤害两次。"
    )
