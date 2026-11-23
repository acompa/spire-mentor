"""The host sends the text the game shows, not a key or an unfilled template (GH #1324).

The live same-seed diff (#1287) found the host sending ``Insert {Rarity} Potion``
for both *The Future of Potions?* options: the agent could not tell which potion
each one costs. The cause was the host's ``LocTable.GetRawText`` patch, which
returned the key for every entry, so the game's own ``GetFormattedText`` never
saw a template. Names had a second cause: the host built ``<id>.title`` keys
instead of using the model's own ``Title`` (potion powers are named after their
potion, Decimillipede segments after ``DECIMILLIPEDE_SEGMENT``).
"""

from __future__ import annotations

import re

import pytest
import sts2_client

pytestmark = pytest.mark.oracle

UNLOCALIZED = re.compile(r"^[A-Z0-9_]+\.(?:title|name)$")
PLACEHOLDER = re.compile(r"\{[A-Za-z]\w*\}")


def test_the_future_of_potions_names_each_options_potion(game):
    game.send({"cmd": "start_run", "character": "Ironclad", "seed": "FUTURE1", "ascension": 0})
    game.send({"cmd": "set_player", "potions": ["STRENGTH_POTION", "ENERGY_POTION"]})
    state = game.send({"cmd": "enter_room", "type": "event", "event": "THE_FUTURE_OF_POTIONS"})

    assert state["decision"] == "event_choice"
    options = state["options"]
    assert [o["title"] for o in options] == ["Insert Common Potion"] * 2
    descriptions = [o["description"] for o in options]
    assert descriptions[0].startswith("Lose Strength Potion. Obtain an Upgraded Common ")
    assert descriptions[1].startswith("Lose Energy Potion. Obtain an Upgraded Common ")
    for text in [state["event_name"], *(o["title"] for o in options), *descriptions]:
        assert not PLACEHOLDER.search(text), text


def test_decimillipede_segments_and_a_potion_power_have_display_names(game):
    game.send({"cmd": "start_run", "character": "Ironclad", "seed": "NAMES1", "ascension": 0})
    game.send({"cmd": "set_player", "potions": ["SPEED_POTION"]})
    state = game.send({"cmd": "enter_room", "type": "monster", "encounter": "DECIMILLIPEDE_ELITE"})
    assert state["decision"] == "combat_play"
    assert {e["name"] for e in state["enemies"]} == {"Decimillipede"}

    state = game.send({"cmd": "action", "action": "use_potion", "args": {"potion_index": 0}})
    names = {p["id"]: p["name"] for p in state["player_powers"]}
    assert names.get("SPEED_POTION_POWER") == "Speed Potion", names
    for name in [*names.values(), *(e["name"] for e in state["enemies"])]:
        assert not UNLOCALIZED.match(name), name


def test_cards_relics_potions_and_powers_carry_their_filled_text(game):
    """Descriptions are the game's own rendering (`CardModel.GetDescriptionForPile`,
    `DynamicDescription`, the power's hover tip), not the raw template: Beckon's
    ``lose {HpLoss} HP`` reached the agent unfilled. Energy icons read ``[E]``."""
    game.send({"cmd": "start_run", "character": "Ironclad", "seed": "TEXT1", "ascension": 0})
    game.send(
        {
            "cmd": "set_player",
            "deck": ["BECKON", "OFFERING", "BASH"],
            "relics": ["BURNING_BLOOD", "LANTERN"],
            "potions": ["SPEED_POTION"],
        }
    )
    state = game.send({"cmd": "enter_room", "type": "monster", "encounter": "TOADPOLES_WEAK"})
    cards = {c["id"]: c["description"] for c in state["hand"] + state.get("draw_pile", [])}
    assert cards["CARD.BECKON"].endswith("lose 6 HP.")
    assert cards["CARD.OFFERING"].startswith("Lose 6 HP.\nGain [E][E].\nDraw 3 cards.")
    assert cards["CARD.BASH"] == "Deal 8 damage.\nApply 2 Vulnerable."
    relics = {r["id"]: r["description"] for r in state["player"]["relics"]}
    assert relics["BURNING_BLOOD"] == "At the end of combat, heal 6 HP."
    assert relics["LANTERN"] == "Start each combat with an additional [E]."
    potion = state["player"]["potions"][0]["description"]
    assert potion == "Gain 5 Dexterity. At the end of your turn, lose 5 Dexterity."

    state = game.send({"cmd": "action", "action": "use_potion", "args": {"potion_index": 0}})
    powers = {p["id"]: p["description"] for p in state["player_powers"]}
    assert powers["SPEED_POTION_POWER"] == "Gain 5 Dexterity until the end of this turn."
    texts = [*cards.values(), *relics.values(), potion, *powers.values()]
    assert not [t for t in texts if PLACEHOLDER.search(t) or "res://" in t]


def test_an_ancients_relic_options_use_the_relics_own_text(game):
    """Orobas's relic options fall back to the relic's text (`EventOption.FromRelic`).

    The host's loc patch said every key existed, so the fallback never ran:
    Sea Glass came out as "Sea Glass" with ``{Character.StringValue:...}``
    unfilled where the game names it after the character it shows ("Noble
    Glass" for the Regent's cards), and Touch of Orobas the same (#1287).
    The seed offers both options and differs by build (v0.107.1 seeds its RNG
    differently; the character Sea Glass shows differs by build too).
    """
    seed = "OROBAS47" if sts2_client.GAME_VERSION == "v0.107.1" else "OROBAS6"
    glass = {
        "Demon Glass": "The Ironclad",
        "Venom Glass": "The Silent",
        "Gear Glass": "The Defect",
        "Lich Glass": "The Necrobinder",
        "Noble Glass": "The Regent",
    }
    game.send({"cmd": "start_run", "character": "Ironclad", "seed": seed, "ascension": 0})
    state = game.send({"cmd": "enter_room", "type": "event", "event": "OROBAS"})
    assert state["decision"] == "event_choice"
    options = {o["text_key"].rsplit(".", 1)[-1]: o for o in state["options"]}
    title = options["SEA_GLASS"]["title"]
    assert title in glass, title
    assert options["SEA_GLASS"]["description"] == (
        f"See 15 cards from {glass[title]}. Choose any number of them to add to your Deck."
    )
    assert options["TOUCH_OF_OROBAS"]["description"] == "Replace Burning Blood with Black Blood."
    for option in state["options"]:
        assert not PLACEHOLDER.search(option["title"] + option["description"]), option

    # Once owned, the relic keeps that name (`RelicModel.Title`, not `<id>.title`).
    state = game.send(
        {
            "cmd": "action",
            "action": "choose_option",
            "args": {"option_index": options["SEA_GLASS"]["index"]},
        }
    )
    names = {r["id"]: r["name"] for r in state["player"]["relics"]}
    assert names["SEA_GLASS"] == title, names


def test_event_option_titles_carry_the_events_own_values(game):
    """Option titles are filled with the event's DynamicVars, as
    `NEventOptionButton._Ready` does (`Event.DynamicVars.AddTo(Option.Title)`).

    The host formatted the title without them, so Ranwid the Elder offered
    ``Give {Gold} Gold`` / ``Give {Potion}`` / ``Give {Relic}`` where the game
    names the potion and relic it takes, and Grave of the Forgotten
    ``Accept the {Relic}`` (#1287 sweep, counted as host-unformatted).
    """
    game.send({"cmd": "start_run", "character": "Ironclad", "seed": "RANWID1", "ascension": 0})
    game.send(
        {
            "cmd": "set_player",
            "gold": 150,
            "relics": ["BURNING_BLOOD", "LANTERN"],
            "potions": ["STRENGTH_POTION"],
        }
    )
    state = game.send({"cmd": "enter_room", "type": "event", "event": "RANWID_THE_ELDER"})
    assert state["decision"] == "event_choice"
    titles = {o["text_key"].rsplit(".", 1)[-1]: o["title"] for o in state["options"]}
    assert titles["GOLD"] == "Give 100 Gold"
    assert titles["POTION"] == "Give Strength Potion"
    assert titles["RELIC"] in {"Give Burning Blood", "Give Lantern"}, titles


def test_grave_of_the_forgotten_names_the_relic_it_gives(game):
    """Same cause as above: ``Accept the {Relic}`` where the game names Forgotten Soul."""
    game.send({"cmd": "start_run", "character": "Ironclad", "seed": "GRAVE1", "ascension": 0})
    state = game.send({"cmd": "enter_room", "type": "event", "event": "GRAVE_OF_THE_FORGOTTEN"})
    assert state["decision"] == "event_choice"
    titles = {o["text_key"].rsplit(".", 1)[-1]: o["title"] for o in state["options"]}
    assert titles["ACCEPT"] == "Accept the Forgotten Soul"
    for option in state["options"]:
        assert not PLACEHOLDER.search(option["title"] + option["description"]), option
