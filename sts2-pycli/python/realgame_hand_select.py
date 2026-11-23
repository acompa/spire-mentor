"""Rank the in-combat card selections no trained head covers.

Why this exists
---------------
``card_select`` is answered by the run-policy net via
:func:`realgame_ppo._build_card_select`, whose surface table maps only the
*deck-mutation* prompts (``TO_REMOVE`` / ``TO_TRANSFORM`` / ``TO_ENCHANT`` /
``TO_UPGRADE`` / ``TO_OBTAIN``). An in-combat selection -- Burning Pact asking
which card to Exhaust, Gambler's Brew which to replace -- matches none of them,
so the builder returns ``None`` and the advisor fell through to
``_unranked``, whose documented contract is *"defaulting to the first option,
NOT a recommendation"*.

That default is not neutral: it exhausts whatever sits leftmost in hand.
Measured over the 2026-08-06 ascension climb, **8 of 8** in-hand picks were
index 0, including a Burning Pact that exhausted a ``Strike+`` while an
unupgraded ``Strike`` sat next to it. See
``docs/experiments/2026-08-06-ascension-climb-issues.md`` issue 5.

This module is deliberately NOT a net. It is a small, explicit heuristic that
answers the one question the leftmost default gets wrong most often: *does the
chosen card get hurt or helped?* -- and then picks the worst or the best card
accordingly.

Direction is data, not taste
----------------------------
Every per-card prompt ships its English text in
``localization_eng`` as ``"<ID>.selectionScreenPrompt"``. The tables below are
transcribed from those strings, so "Choose a card to Copy" (Dual Wield) is
classified BOOST because the game says so, not because someone guessed. A
prompt that is not in the tables stays UNKNOWN and keeps the old leftmost
behaviour -- with the note saying so, which is the point: a guess in the wrong
direction is worse than an admitted arbitrary pick.

This lives beside :mod:`realgame_ppo` rather than inside it on purpose.
``_build_card_select`` is shared with the PPO driver that the engine-world and
rung-3.5 arms are measured with; changing it would move a comparator mid-ladder.

The tables and the ranking live in Rust (``sts2_advisor::hand_select``, GH
#1426 step 3), with the prompt text each direction was transcribed from; this
module keeps the Python names.
"""

from __future__ import annotations

from typing import Any

import sts2_rs

#: The chosen card SUFFERS -- exhausted, discarded, removed, transformed away.
#: Pick the worst card.
DISPOSE = "dispose"

#: The chosen card BENEFITS -- copied, retained, drawn, made free. Pick the
#: best card.
BOOST = "boost"

#: No idea which way this prompt points. Keep the leftmost default and say so.
UNKNOWN = "unknown"

#: ``CardSelectorPrefs``' shared prompts, and the source model ids (equally
#: the stem of a ``"<ID>.selectionScreenPrompt"`` prompt key): ``{key:
#: direction}``. Read-only copies of the Rust tables.
_PROMPT_DIRECTION, _SOURCE_DIRECTION = sts2_rs.hand_select_tables()


def direction_of(state: dict[str, Any]) -> tuple[str, str]:
    """Classify a ``card_select`` state as DISPOSE / BOOST / UNKNOWN.

    Returns ``(direction, key)`` where ``key`` is whatever named it, for the
    note. ``prompt_key`` is checked before ``source_model``.
    """
    return sts2_rs.hand_select_direction(state)


def card_value(card: dict[str, Any]) -> tuple[int, int, int]:
    """Rank a card from worthless to precious, worst first: class (curses,
    statuses, basics, then rarity), then upgrade level, then cost."""
    return sts2_rs.hand_select_card_value(card)


def rank(state: dict[str, Any]) -> dict[str, Any] | None:
    """Rank a ``card_select`` the nets do not cover, or ``None`` if unknown.

    ``None`` means "I still do not know which way this points" -- the caller
    keeps its existing arbitrary-but-honest fallback. Ties go to the leftmost
    card either way.
    """
    return sts2_rs.hand_select_rank(state)
