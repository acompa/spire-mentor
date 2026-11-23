"""Player-facing names for game ids, and the wire guard that keeps ids off the HUD.

The mod's payloads carry both spellings of most things: a card's ``id``
(``SHRUG_IT_OFF``) and its ``name`` (``Shrug It Off``). The advisor builds its
labels from whichever field it reaches first, and a field that is an id on one
surface (the shop's option ``label`` is the merchant entry's id) put
``SHRUG_IT_OFF`` on the HUD. Labels are built from display names where the
payload has them; this module is the rest:

* :func:`display_name` -- one id (any spelling: ``SHRUG_IT_OFF``,
  ``CARD.SHRUG_IT_OFF``, ``SHRUG_IT_OFF.title``) to the game's English name,
  from the vendored localization tables (the same files
  :mod:`sts2_hint.canonical_maps` reads, gzipped in the packaged advisor);
* :func:`humanize` -- every id-shaped token inside a display string replaced
  by its name (``remove:STRIKE_IRONCLAD`` -> ``Remove Strike``);
* :func:`sanitize_advice` / :func:`sanitize_coach` -- the final guard at the
  wire boundary, applied to every human-facing string the advisor sends, so a
  surface added later cannot leak an id either.

The name tables and the id rewriting run in Rust
(``sts2_rs.DisplayNames``, ``sts2-advisor/src/display_names.rs``, GH #1426
step 3); this module reads the localization tables and hands them over.

The takeover's cross-check (``SimAdvisor.ReconcileIndex``) compares
``advice.best.label`` against the labels the mod built for the screen, so
:func:`sanitize_advice` never rewrites a ``best.label`` that is the mod's own
spelling of an option on a cross-checked surface (see
:data:`CROSS_CHECKED_SURFACES`).
"""

from __future__ import annotations

import functools
from pathlib import Path
from typing import Any

import runtime_paths

#: Localization tables, in lookup order (a card and a relic sharing an id
#: resolve as the card). Their name-key suffixes live with the Rust tables.
_TABLES = (
    "cards",
    "relics",
    "potions",
    "monsters",
    "events",
    "enchantments",
    "powers",
    "orbs",
    "afflictions",
    "encounters",
    "characters",
)

#: ``advice_for`` surfaces whose ``best.label`` the mod cross-checks against
#: its own option labels before clicking (``SimScreenDecision.OptionLabels``,
#: ``SimAdvisor.ReconcileIndex``; a mismatch re-anchors or refuses the click).
#: Shop, map and (non-Fake-Merchant) event screens send no labels to check.
CROSS_CHECKED_SURFACES = frozenset(
    {
        "card_reward",
        "card_select",
        "card_bundle",
        "boss_relic",
        "treasure",
        "rest_site",
        "potion_swap",
        "event",
    }
)


def _loc_dirs() -> list[Path]:
    """The newest vendored build first (dev checkout), the bundled one last."""
    root = runtime_paths.data_root()
    newer = sorted(root.glob("localization_eng_v*"), reverse=True)
    return [*newer, root / "localization_eng"]


@functools.cache
def _names() -> Any:
    """The ``sts2_rs.DisplayNames`` tables over every localization directory."""
    import sts2_rs
    from sts2_hint.canonical_maps import _load_json

    tables = [
        (table, list(_load_json(loc_dir / f"{table}.json").items()))
        for loc_dir in _loc_dirs()
        if loc_dir.is_dir()
        for table in _TABLES
    ]
    return sts2_rs.DisplayNames(tables)


def display_name(raw: Any) -> str:
    """The English name for a game id in any spelling; the id prettified when
    no table knows it (``SOME_NEW_CARD`` -> ``Some New Card``)."""
    if not isinstance(raw, (str, int, float, bool)) and raw is not None:
        raw = str(raw)
    return _names().display_name(raw)


def humanize(text: Any) -> Any:
    """``text`` with every id-shaped token replaced by its display name.

    Non-strings pass through unchanged. Idempotent on display text: a name
    has no ``_`` and is not all caps. ``remove:STRIKE_IRONCLAD`` reads
    ``Remove Strike``, a snake_case verb (``leave_shop``) its words, and a
    lone all-caps word only changes when a table knows it, so prose capitals
    (``NOT a recommendation``, ``MCTS``) stay.
    """
    if not isinstance(text, str) or not text:
        return text
    return _names().humanize(text)


def card_name(card: Any) -> str:
    """A wire Card object's name: its ``name``, else its id's display name."""
    if not isinstance(card, dict):
        return "?"
    return _names().card_name(card)


def shop_option_label(option: dict[str, Any]) -> str:
    """A ``shop`` option as the player reads it: the item's name.

    The option's own ``label`` is the merchant entry's id (``SHRUG_IT_OFF``,
    ``remove_card``); the display name is in ``item`` (PROTOCOL.md ``shop``).
    """
    return _names().shop_option_label(option)


def potion_swap_label(option: dict[str, Any], belt: list[Any] | None = None) -> str:
    """A full-belt potion swap option (PROTOCOL.md v5.2) in words.

    The mod's ``label`` is ``decline`` / ``discard <POTION_ID>``; the reward
    is named in ``reward`` and the held potion in the belt (``run_state.potions``
    by ``discard_slot``).
    """
    return _names().potion_swap_label(option, belt)


def _mod_labels(state: dict[str, Any] | None) -> set[str]:
    """The option labels the mod itself sent (case-folded): on a cross-checked
    surface these are what ``best.label`` is compared against."""
    out: set[str] = set()
    for key in ("options", "cards", "bundles"):
        for o in (state or {}).get(key) or []:
            if isinstance(o, dict):
                for field in ("label", "name"):
                    v = o.get(field)
                    if isinstance(v, str) and v.strip():
                        out.add(v.strip().casefold())
    return out


def sanitize_advice(
    advice: dict[str, Any], advice_for: str | None = None, state: dict[str, Any] | None = None
) -> dict[str, Any]:
    """The final guard on every human-facing string of an ``advice`` object.

    The walk stays here because it rewrites the reply in place; the native
    advisor's twin (``sts2_rs.DisplayNames.sanitize_advice``) is held to it by
    ``tests/test_display_names.py``.

    Rewrites, in place: ``note``, ``hand_back``, ``instinct_unavailable``,
    ``pv[]``, ``options[].label``, ``plan[].label`` and ``best.label`` --
    except a ``best.label`` the takeover cross-checks: on a
    :data:`CROSS_CHECKED_SURFACES` screen a label that IS one of the mod's own
    option labels stays as sent, or F6 / F7 would refuse the click.

    Returns:
        ``advice`` (the same object).
    """
    for key in ("note", "hand_back", "instinct_unavailable"):
        if isinstance(advice.get(key), str):
            advice[key] = humanize(advice[key])
    if isinstance(advice.get("pv"), list):
        advice["pv"] = [humanize(s) for s in advice["pv"]]
    for key in ("options", "plan"):
        for o in advice.get(key) or []:
            if isinstance(o, dict) and isinstance(o.get("label"), str):
                o["label"] = humanize(o["label"])
    best = advice.get("best")
    if isinstance(best, dict) and isinstance(best.get("label"), str):
        label = best["label"]
        keep = advice_for in CROSS_CHECKED_SURFACES and label.strip().casefold() in _mod_labels(
            state
        )
        if not keep:
            best["label"] = humanize(label)
    return advice


#: Coach record keys holding display text (strings or lists of strings).
_COACH_TEXT_KEYS = frozenset({"label", "you", "ai", "why", "hand", "name", "note"})


def sanitize_coach(record: Any) -> Any:
    """The same guard over a ``coach`` record (PROTOCOL.md §5 ``coach``),
    recursively: the turn lines, the choice labels, a recap's context."""
    if isinstance(record, dict):
        for key, value in record.items():
            if key in _COACH_TEXT_KEYS and isinstance(value, str):
                record[key] = humanize(value)
            elif key in _COACH_TEXT_KEYS and isinstance(value, list):
                record[key] = [
                    humanize(v) if isinstance(v, str) else sanitize_coach(v) for v in value
                ]
            else:
                sanitize_coach(value)
    elif isinstance(record, list):
        for v in record:
            sanitize_coach(v)
    return record
