"""The turn plan: this turn's moves, in order, in the CURRENT hand's indices.

``advice.plan`` (PROTOCOL.md §5) lets the HUD number the cards to play this
turn 1, 2, 3. Both brains produce a line of engine actions -- the search its
principal variation, Instinct a greedy rollout of itself -- but every action
after the first indexes a hand that exists only after the earlier ones were
played. This module walks such a line on a copy of the fight and maps each
action back to the hand the player is looking at now, keeping only what it
can map with certainty:

* a play's ``card_index`` is translated through the engine->mod hand map of
  the request (``wire.deck.hand[i].stable_idx``), which is shifted by every
  card the line has already played;
* after every step the engine's hand must be exactly the previous hand minus
  the played card (same cards, same order). A card that draws, generates,
  discards, upgrades or reorders ends the plan AFTER itself: it is still
  certain, the next one is not;
* a step that opens a choice, wins or loses the fight ends the plan after
  itself; a move the walk cannot map (a choice, an orb verb, an index the
  map does not cover, an engine refusal) ends it before itself;
* ``end_turn`` is the last step when the line reaches it;
* the enemy roster must keep its length (``target_index`` addresses it).

Potions use the remap ``best`` uses: the engine's potion slot is dense over
the held potions, the mod's is the belt slot.

The walk runs in Rust (``sts2_advisor::turn_plan``, GH #1426 step 3); this
module keeps the Python names.
"""

from __future__ import annotations

import logging
from collections.abc import Callable
from typing import Any

import sts2_rs

logger = logging.getLogger(__name__)

#: A defensive bound on one turn's plan (a real turn ends long before).
MAX_PLAN_STEPS: int = sts2_rs.TURN_PLAN_MAX_STEPS

#: Wall-clock cap on the re-searches that extend a plan past its PV. A plan
#: is a drawing: once spent, the plan simply ends where it got to.
PLAN_BUDGET_S: float = sts2_rs.TURN_PLAN_BUDGET_S

#: Maps one line of engine actions onto the request's hand, step by step:
#: ``PlanWalk(wire, state)``, then ``to_step`` / ``push`` / ``advance`` /
#: ``pop_step``, with ``steps`` and ``done``.
PlanWalk = sts2_rs.TurnPlanWalk


def step_label(state: dict[str, Any], step: dict[str, Any]) -> str:
    """A plan step named against the request's board (``state``: the mod's)."""
    return sts2_rs.turn_step_label(state, step)


def same_move(step: dict[str, Any], best: dict[str, Any]) -> bool:
    """Whether a plan step is the move ``advice.best`` names."""
    return sts2_rs.turn_same_move(step, best)


def first_step(best: dict[str, Any] | None, state: dict[str, Any]) -> dict[str, Any] | None:
    """``best`` as a plan step (when the line cannot follow it further)."""
    return sts2_rs.turn_first_step(best, state)


def search_plan(
    cs: Any,
    pv: list[Any],
    wire: dict[str, Any],
    state: dict[str, Any],
    best: dict[str, Any] | None,
    research: Callable[[Any], list[Any]] | None = None,
    budget_s: float = PLAN_BUDGET_S,
) -> list[dict[str, Any]]:
    """The search's plan for this turn.

    Follows the principal variation, then -- the honest search's PV is one
    ply deep -- ``research(fight)``: a search from the fight as the plan has
    left it, returning its line (its pick first). Steps are applied to a
    clone of ``cs``, never ``cs`` itself. When the PV does not open with
    ``best``, the plan is ``best`` alone. A failed re-search ends the plan
    where it got to.
    """

    def logged(fight: Any) -> list[Any]:
        assert research is not None
        try:
            return research(fight)
        except Exception as exc:
            logger.debug("plan: re-search failed: %s", exc)
            raise

    return sts2_rs.turn_search_plan(
        cs, list(pv), wire, state, best, logged if research is not None else None, budget_s
    )
