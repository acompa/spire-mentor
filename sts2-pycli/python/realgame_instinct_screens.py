"""Instinct's out-of-combat screens: park the rebuilt run AT the screen, map back.

:mod:`realgame_instinct` rebuilds the live run from the mod's envelope
(:func:`realgame_instinct.graft_run`); this module puts that run on the screen
the player is looking at and turns the student's pick into the screen's own
option index.

**Inject, never re-roll.** The envelope's ``rng`` block carries every stream's
cursor but none of the history-dependent state (card rarity odds, unknown-room
odds, visited events, grab bags), so re-entering a room could roll a different
shop / event / reward than the screen shows. Each screen's options go into the
engine as shown, through the ``debug_*`` graft setters (``sts2_wire::graft``),
and the engine's offer is checked against the screen before the student's
answer is trusted.

**Map back by identity, not by index.** The engine's index spaces differ from
the screen's in ways that are easy to get subtly wrong (curse-first removal
order, upgradable-only upgrade lists, a shop's per-kind lists vs the screen's
flat one), so a deck pick is resolved by APPLYING it to a copy of the run and
seeing which card left the deck (removed, upgraded, transformed: the picked
card is the one missing afterwards), and a shop / rest / event / relic pick by
the item it names. A class that names nothing on the screen is dropped and the
crown passes to the student's next choice.

Every handler raises :class:`realgame_instinct.InstinctUnavailable` with a
player-facing reason when it cannot stand on the screen; the advisor then
answers with the search.
"""

from __future__ import annotations

import logging
import re
from collections import Counter
from typing import TYPE_CHECKING, Any

from display_names import card_name, display_name, potion_swap_label, shop_option_label
from realgame_instinct import (
    InstinctPick,
    InstinctUnavailable,
    engine_game_version,
    graft_run,
    is_coop,
    note_for,
)

if TYPE_CHECKING:
    from realgame_instinct import InstinctAdvisor

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def norm_id(raw: Any) -> str:
    """An id in any spelling (``STRIKE_IRONCLAD``, ``StrikeIronclad``,
    ``CARD.STRIKE_IRONCLAD``, ``RELIC.ANCHOR``) as one comparable key."""
    tail = str(raw or "").split(".")[-1]
    return re.sub(r"[^A-Z0-9]", "", tail.upper())


def screen_advice(
    pick: InstinctPick,
    rows: dict[int, tuple[dict[str, Any], str] | None],
    *,
    note_extra: str = "",
) -> dict[str, Any]:
    """The wire ``advice`` from per-class screen responses.

    ``rows[class] = (response, label)`` where ``response`` is the screen's
    ``{"action": "choose", "index": i}`` or ``{"action": "skip"}``; ``None``
    when the class names nothing on this screen (dropped). Classes sharing a
    response are shown once with their probabilities summed. Shaped in Rust
    (``sts2_advisor::instinct::screen_advice``, GH #1426).
    """
    import sts2_rs

    advice, why = sts2_rs.instinct_screen_advice(
        pick.decision, pick.probs, pick.best, pick.ms, rows, note_extra
    )
    if advice is None:
        raise InstinctUnavailable(why)
    return advice


def lone_option(o: dict[str, Any]) -> dict[str, Any]:
    """A screen with one button: no decision, the button at probability 1."""
    import sts2_rs

    return sts2_rs.instinct_lone_option(o)


def _first_action(pick: InstinctPick, ci: int) -> dict[str, Any]:
    return (pick.decision["classes"][ci].get("actions") or [{}])[0] or {}


# ---------------------------------------------------------------------------
# Card offers
# ---------------------------------------------------------------------------


def _rust_screen(adv: InstinctAdvisor | None, screen: str, state: dict[str, Any]) -> dict[str, Any]:
    """A screen answered in Rust; with no advisor, only by rule (Claws, the
    Blitz merchant), and anything that needs the student declines."""
    if adv is not None:
        return adv.rust_screen(screen, state)
    import sts2_rs
    from display_names import _names

    out = sts2_rs.instinct_screen(screen, None, None, state, engine_game_version(state), _names())
    if out["advice"] is None:
        raise InstinctUnavailable(out["why"])
    return out["advice"]


def card_reward(adv: InstinctAdvisor, state: dict[str, Any], session: Any) -> dict[str, Any]:
    """A card reward, or an out-of-fight choose-a-card screen (Rust)."""
    return _rust_screen(adv, "card_reward", state)


def card_bundle(adv: InstinctAdvisor, state: dict[str, Any], session: Any) -> dict[str, Any]:
    """A Scroll Boxes pack choice (Rust)."""
    return _rust_screen(adv, "card_bundle", state)


# ---------------------------------------------------------------------------
# Rest site + smith
# ---------------------------------------------------------------------------


def rest_site(adv: InstinctAdvisor, state: dict[str, Any], session: Any) -> dict[str, Any]:
    """The rest site (Rust)."""
    return _rust_screen(adv, "rest_site", state)


# ---------------------------------------------------------------------------
# Deck screens (card_select out of combat)
# ---------------------------------------------------------------------------


def deck_select(adv: InstinctAdvisor, state: dict[str, Any], session: Any) -> dict[str, Any]:
    """An out-of-fight card select: remove, transform, enchant, upgrade (a
    smith after a rest site) or a card grant (Rust)."""
    return _rust_screen(adv, "card_select", state)


# ---------------------------------------------------------------------------
# Shop
# ---------------------------------------------------------------------------


def shop(adv: InstinctAdvisor, state: dict[str, Any], session: Any) -> dict[str, Any]:
    """The shop (Rust); a removal pick is carried to the next card screen."""
    return _rust_screen(adv, "shop", state)


# ---------------------------------------------------------------------------
# Events (Neow included) and relic choices
# ---------------------------------------------------------------------------


def event(adv: InstinctAdvisor, state: dict[str, Any], session: Any) -> dict[str, Any]:
    """An event page, Neow and the Ancients' relic offers included (Rust).

    Later pages are replayed from the first through the advisor's event
    trail (``adv.event_track``), which the handler updates."""
    return _rust_screen(adv, "event", state)


def note_event_choice(adv: InstinctAdvisor, msg: dict[str, Any]) -> None:
    """A ``human_action`` on an event screen: the page's taken option."""
    if adv.event_track is not None:
        import sts2_rs

        adv.event_track = sts2_rs.instinct_note_event_choice(adv.event_track, msg)


def boss_relic(adv: InstinctAdvisor, state: dict[str, Any], session: Any) -> dict[str, Any]:
    """The boss relic screen (Rust)."""
    return _rust_screen(adv, "boss_relic", state)


def potion_swap(adv: InstinctAdvisor, state: dict[str, Any], session: Any) -> dict[str, Any]:
    """The full-belt potion swap (Rust)."""
    return _rust_screen(adv, "potion_swap", state)


#: ``advice_for`` -> handler.
HANDLERS = {
    "card_reward": card_reward,
    "card_bundle": card_bundle,
    "rest_site": rest_site,
    "card_select": deck_select,
    "shop": shop,
    "event": event,
    "boss_relic": boss_relic,
    "potion_swap": potion_swap,
}
