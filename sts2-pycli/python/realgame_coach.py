"""Live coach for the real-game advisor: grades the human's decisions after the fact.

The advisor computes advice for every decision whether or not the mod shows
it (Coach mode renders nothing before the player acts). This module keeps
that advice per request (:class:`AdviceCache`) and grades each ``human_action``
against the advice for exactly the position it was taken from
(:class:`CoachGrader`). The grades go to the mod as ``coach`` messages and into
the workbench run log as ``coach`` records (PROTOCOL.md, "coach").

The grader, its cache and its worker run in Rust (``sts2_advisor::coach_grader``,
GH #1426 step 4); this module keeps their Python names and the searches the
grader asks for (:class:`CoachEngine`), on the advisor's native positions.

Combat is graded PER TURN, never per card. Each action is diffed with plan 1's
:func:`workbench.regret_review.assess_decision` (HP and win chance together),
after a forced-first-move top-up when the played move's row is too thin to
trust. A turn is graded when it ends, from its actions:

.. code-block:: text

    graded = per-action DecisionRegret, after top-up
    if any action verdict == "uncertain" or ungraded -> turn ungraded, silent
    otherwise regret_review.grade_turn: count only actions with
    severity >= TURN_ACTION_FLOOR, severity = max(summed points, summed HP),
    verdict best | tie | slip | blunder (the browser workbench uses its twin)

Silence beats a wrong number: a turn with any action that stays uncertain, or
that cannot be graded, emits nothing to the mod (the log records it as
``ungraded``). In-combat card selects carry no per-option stats and are
skipped (counted in ``ungraded_actions``), not guessed.

Run-level choices (map, card reward, shop, ...) are graded by policy
AGREEMENT (:func:`choice_grade.grade_choice`), never by a win-chance cost.

Two summaries ride on those grades (coach plan 4, ``capabilities.coach >= 2``):
a ``recap`` after each won fight (the mod's ``combat_ended``: HP lost against
the AI line's expectation, plus the costliest turns with the position each
started from) and an ``autopsy`` on death (the run's costliest decisions,
:func:`rank_autopsy`). Both are built on the executor behind the grading still
in flight, and only from records still on the active path.
"""

from __future__ import annotations

import dataclasses
import logging
import time
from collections.abc import Callable, Iterable
from typing import Any

import sts2_rs
from workbench.reasoning import rows_from_mcts

logger = logging.getLogger(__name__)

#: Budget of each step of the background AI-line search.
AI_LINE_SIMS = 1000
#: Defensive cap on an AI line's length; the stop rules end it far sooner.
AI_LINE_MAX_STEPS = 12
#: Root-parallel workers for a catch-up search (kept off the main search's cores).
CATCH_UP_THREADS = 2
#: How many decisions the death autopsy shows (a recap shows the
#: ``sts2_rs.COACH_RECAP_COSTLIEST`` costliest turns).
AUTOPSY_ITEMS: int = sts2_rs.COACH_AUTOPSY_ITEMS


#: One advice request's raw result, as the grader needs it (``kind``
#: ``"combat"``: ``rows`` / ``cs`` / ``cfg``; ``"screen"``: ``options``; ``msg``
#: is the wire ``advice_request`` the advice answered).
Entry = sts2_rs.CoachEntry
#: Bounded LRU of :data:`Entry` keyed by ``request_id``. Thread-safe.
AdviceCache = sts2_rs.CoachAdviceCache


# ---------------------------------------------------------------------------
# Pure helpers: in Rust (`sts2_advisor::coach`, GH #1426 step 3)
# ---------------------------------------------------------------------------


def _match_row_index(rows: list[dict[str, Any]], native: dict[str, Any]) -> int | None:
    """``regret_review._match_row``'s exact-then-``stable_idx``-only rule, as an index.

    Exact includes the teammate of an AnyAlly play (``ally_index``): with three
    or four players one card has a row per teammate (GH #1017)."""
    return sts2_rs.coach_match_row_index(rows, native)


def played_needs_top_up(rows: list[dict[str, Any]], native: dict[str, Any]) -> bool:
    """The played move's row exists, is not the recommended one, and has fewer
    than ``PLAYED_MIN_VISITS`` visits (twin of ``advisors.rs::played_needs_top_up``)."""
    return sts2_rs.coach_played_needs_top_up(rows, native)


def native_from_row(row: dict[str, Any]) -> dict[str, Any] | None:
    """A regret row's move as the engine's action dict."""
    return sts2_rs.coach_native_from_row(row)


def intent_text(intents: list[dict[str, Any]] | None) -> str:
    """One enemy's telegraphed move as the recap prints it: ``"attack 6"``,
    ``"attack 6×2"``, else the lower-cased ``intent_type``, joined with
    ``" + "``; ``"unknown"`` when nothing reads."""
    return sts2_rs.coach_intent_text(intents)


def turn_context(msg: dict[str, Any] | None) -> dict[str, Any] | None:
    """The position a combat turn started from, for the recap and autopsy.

    ``msg`` is the turn's first combat ``advice_request``; ``None`` without
    one (a turn without its position is never shown)."""
    return sts2_rs.coach_turn_context(msg)


def board_events(start: dict[str, Any], end: dict[str, Any]) -> list[tuple[str, int]]:
    """What a line visibly did to the enemies between two engine boards:
    ``("wakes", i)`` / ``("kills", i)`` per encounter slot, or a lone
    ``("wins", -1)`` when the line won the fight."""
    return sts2_rs.coach_board_events(start, end)


def why_text(
    you: list[tuple[str, int]],
    ai: list[tuple[str, int]],
    names: list[str],
    turn: Any,
) -> str | None:
    """The recap's reason line, e.g. ``"Your line wakes Lagavulin Matriarch on
    turn 1"`` (GH #888); ``None`` when both lines did the same."""
    return sts2_rs.coach_why_text(you, ai, names, turn)


def rank_autopsy(
    turns: list[dict[str, Any]], choices: list[dict[str, Any]], k: int = AUTOPSY_ITEMS
) -> list[dict[str, Any]]:
    """The run's costliest decisions, for the death autopsy: turns by expected
    HP lost (win-chance points when HP is unknown), choices by how far the AI's
    top pick out-weighed the player's, interleaved turns first."""
    return sts2_rs.coach_rank_autopsy(turns, choices, k)


def native_action(msg: dict[str, Any], info: dict[str, Any]) -> dict[str, Any] | None:
    """The engine action a combat ``human_action`` stands for at a position
    (``info``: :meth:`CoachEngine.position_info`): a card located in hand by
    identity, a potion in its dense belt; ``None`` when neither resolves."""
    return sts2_rs.coach_native_action(msg, info)


# ---------------------------------------------------------------------------
# Engine operations (stubbed in tests)
# ---------------------------------------------------------------------------


def _cfg_for(cfg: Any, *, sims: int, threads: int) -> Any:
    """``cfg`` re-budgeted, with the mode-resolved constants re-derived for it."""
    from sts2_hint.recommender import RecommendConfig

    base = cfg if cfg is not None else RecommendConfig()
    return dataclasses.replace(base, sims=sims, threads=threads, c_puct=None)


class CoachEngine:
    """The searches the coach runs, on native ``sts2_rs`` combat states."""

    def position_info(self, cs: Any, msg: dict[str, Any] | None = None) -> dict[str, Any]:
        """``{"hand": [(id, upgraded)], "potions": [id | None], "round": int | None}``.

        From the native state when there is one (its hand order is the frame
        the rows index); from the wire board otherwise.
        """
        if cs is not None:
            import sts2_rs

            d = sts2_rs.state_to_dict(cs)
            hand = [
                (str(c.get("id") or ""), bool(c.get("upgraded")))
                for c in (d.get("deck") or {}).get("hand") or []
            ]
            potions = [
                (str(p) if p else None) for p in ((d.get("player") or {}).get("potions") or [])
            ]
            rnd = d.get("round")
            return {
                "hand": hand,
                "potions": potions,
                "round": rnd if isinstance(rnd, int) else None,
            }
        msg = msg or {}
        hand = [
            (str(c.get("id") or ""), bool(c.get("upgraded") or c.get("upgrade_level")))
            for c in msg.get("hand") or []
            if isinstance(c, dict)
        ]
        potions = [
            p.get("id")
            for p in msg.get("potions") or []
            if isinstance(p, dict) and not p.get("empty") and p.get("id")
        ]
        rnd = msg.get("round")
        return {"hand": hand, "potions": potions, "round": rnd if isinstance(rnd, int) else None}

    def top_up(self, cs: Any, native: dict[str, Any], cfg: Any, sims: int) -> dict[str, Any] | None:
        """A forced-root search pricing ``native`` from ``cs``."""
        import sts2_rs

        c = _cfg_for(cfg, sims=sims, threads=1)
        return sts2_rs.mcts_score_action_forced(
            cs,
            native,
            sims=sims,
            c_puct=c.resolved_c_puct(),
            depth=c.depth,
            seed=c.seed,
            rollout_policy=c.rollout_policy,
            hidden_info=c.hidden_info,
            draw_widening_c=c.resolved_draw_widening_c(),
            endturn_widening_c=c.endturn_widening_c,
            endturn_widening_alpha=c.endturn_widening_alpha,
            win_hp_carry=c.win_hp_carry,
            win_potion_carry=c.win_potion_carry,
        )

    def _recommend(self, cs: Any, cfg: Any, sims: int, threads: int) -> dict[str, Any]:
        import sts2_rs

        c = _cfg_for(cfg, sims=sims, threads=threads)
        return sts2_rs.mcts_recommend(
            cs,
            sims=sims,
            c_puct=c.resolved_c_puct(),
            depth=c.depth,
            seed=c.seed,
            threads=threads,
            rollout_policy=c.rollout_policy,
            hidden_info=c.hidden_info,
            draw_widening_c=c.resolved_draw_widening_c(),
            endturn_widening_c=c.endturn_widening_c,
            endturn_widening_alpha=c.endturn_widening_alpha,
            win_hp_carry=c.win_hp_carry,
            win_potion_carry=c.win_potion_carry,
        )

    def catch_up(self, cs: Any, cfg: Any, sims: int) -> list[dict[str, Any]]:
        """Search a position the advisor never searched (the player was faster)."""
        return rows_from_mcts(self._recommend(cs, cfg, sims, CATCH_UP_THREADS), {})

    def board(self, cs: Any) -> dict[str, Any] | None:
        """The engine board of ``cs`` (``None`` without a position)."""
        import sts2_rs

        return sts2_rs.state_to_dict(cs) if cs is not None else None

    def ai_line(
        self,
        cs: Any,
        first: dict[str, Any] | None,
        cfg: Any,
        label: Callable[[dict[str, Any], dict[str, Any]], str],
    ) -> list[str]:
        """The labels of :meth:`ai_turn`."""
        return self.ai_turn(cs, first, cfg, label)[0]

    def ai_turn(
        self,
        cs: Any,
        first: dict[str, Any] | None,
        cfg: Any,
        label: Callable[[dict[str, Any], dict[str, Any]], str],
    ) -> tuple[list[str], dict[str, Any] | None]:
        """The AI's line for the turn from ``cs``: ``first``, then the best move
        of a short single-tree search, repeated on a clone.

        Also returns the board the line ended the turn from (the position its
        end turn was played in, or the won / lost board), for
        :func:`board_events`; ``None`` when the line stopped short of that.

        Stops after end turn, when combat ends, or after a move that put a new
        card into the hand -- past a draw the line would name cards the search
        dealt itself (``advisors.rs::label_line``'s rule). Each step is labelled
        from the position it is played in.
        """
        import sts2_rs

        state = cs.clone()
        out: list[str] = []
        end: dict[str, Any] | None = None
        for step in range(AI_LINE_MAX_STEPS):
            if step == 0 and first is not None:
                action = first
            else:
                result = self._recommend(state, cfg, AI_LINE_SIMS, 1)
                best = next((a for a in result.get("actions") or [] if a.get("is_best")), None)
                if best is None:
                    break
                action = best["action"]
            before = sts2_rs.state_to_dict(state)
            out.append(label(before, action))
            hand_before = {
                c.get("stable_idx") for c in (before.get("deck") or {}).get("hand") or []
            }
            outcome = state.apply_step(action)
            if action.get("action") == "end_turn":
                end = before
                break
            after = sts2_rs.state_to_dict(state)
            if outcome in ("combat_won", "combat_lost"):
                end = after
                break
            if any(
                c.get("stable_idx") not in hand_before
                for c in (after.get("deck") or {}).get("hand") or []
            ):
                break
        return out, end


class InlineExecutor:
    """Runs a grader's jobs before the call that queued them returns (tests,
    and a coach with no grader thread)."""


#: The coach's one background worker: catch-up searches, played-row top-ups
#: and AI lines run there, during the enemy turn, while the advisor loop goes
#: on serving messages -- the loop never joins or waits on it. ``cancel``
#: drops every job queued so far (an undo made them moot); a job already
#: running finishes and its result is discarded by the grader.
GraderThread = sts2_rs.CoachGraderThread


class CoachGrader:
    """Per-connection live coach (Rust: ``sts2_advisor::coach_grader``).

    ``send(record)`` puts one record on the wire (the advisor wraps it in the
    ``coach`` envelope); it is only called while the mod advertised
    ``capabilities.coach >= 1``. ``log(record)`` writes the run log and is
    always called, including for turns that stay ungraded. Both may be called
    from the grader's thread. The positions the grader hands ``engine`` are
    the ones it was given (the advisor's native ``CombatState``).
    """

    def __init__(
        self,
        *,
        send: Callable[[dict[str, Any]], None],
        log: Callable[[dict[str, Any]], None],
        engine: Any = None,
        executor: Any = None,
        cache: Any = None,
        catchup_sims: int = 2000,
        budget_s: float = 6.0,
        map_path: Callable[[dict[str, Any], int], list[dict[str, Any]] | None] | None = None,
        clock: Callable[[], float] = time.monotonic,
        locate: Callable[[Any], tuple[str | None, int | None]] | None = None,
    ) -> None:
        from display_names import _names

        self.engine = engine if engine is not None else CoachEngine()
        self.executor = executor if executor is not None else InlineExecutor()
        self.cache = cache if cache is not None else AdviceCache()
        self.catchup_sims = catchup_sims
        self.budget_s = budget_s
        self._core = sts2_rs.CoachGraderCore(
            send=send,
            log=log,
            engine=self.engine,
            names=_names(),
            cache=self.cache,
            executor=self.executor if isinstance(self.executor, GraderThread) else None,
            catchup_sims=catchup_sims,
            budget_s=budget_s,
            map_path=map_path,
            # The grader's own monotonic clock unless a test steers one.
            clock=None if clock is time.monotonic else clock,
            locate=locate,
        )

    @property
    def mod_coach(self) -> int:
        """The mod's advertised ``capabilities.coach`` (0 = never send)."""
        return self._core.mod_coach

    def on_advice_request(self, msg: dict[str, Any]) -> None:
        """Every ``advice_request``: track the mod's capability and the floor,
        and preempt grading that the next turn has overtaken (dropped, never
        waited on: the advice is served at once)."""
        self._core.on_advice_request(msg)

    def on_rewind(self, msg: dict[str, Any], active_path: Iterable[Any]) -> list[dict[str, Any]]:
        """An undo / snapshot restore: abandon grading, withdraw every emitted
        record whose ``node_id`` is not on the root -> active node path, with
        one ``{"kind": "withdraw", "ids": [...]}`` record."""
        return self._core.on_rewind(msg, list(active_path))

    def on_advice(self, entry: Any) -> None:
        """Advice was served and cached; remember what labelling needs."""
        self._core.on_advice(entry)

    def on_combat_ended(self, msg: dict[str, Any]) -> None:
        """The mod's ``combat_ended``: queue a won fight's recap behind the
        fight's turns still being graded."""
        self._core.on_combat_ended(msg)

    def on_run_end(self) -> None:
        """A run ended with nothing to summarize: cancel the grading still
        queued and forget the run."""
        self._core.on_run_end()

    def on_game_over(self, msg: dict[str, Any]) -> None:
        """Death: queue the run's autopsy behind the grading in flight, then
        forget the run."""
        self._core.on_game_over(msg)

    def on_human_action(
        self, msg: dict[str, Any], pre_state: Any = None, node_id: Any = None
    ) -> list[dict[str, Any]]:
        """Grade one ``human_action``; returns the records emitted before it
        returned. ``pre_state`` is the mirror's native state taken BEFORE the
        action was followed: a catch-up search starts there when the advice on
        screen was not for this exact position."""
        try:
            return self._core.on_human_action(msg, pre_state, node_id)
        except Exception as exc:  # grading is bookkeeping: never take the loop down
            logger.warning("coach: grading failed: %s", exc, exc_info=True)
            return []

    def _label_human(self, msg: dict[str, Any]) -> str:
        return self._core.label_human(msg)

    def _label_engine(self, state: dict[str, Any], action: dict[str, Any]) -> str:
        return self._core.label_engine(state, action)

    @property
    def _emitted(self) -> dict[str, Any]:
        return self._core.emitted()

    @property
    def _emitted_records(self) -> dict[str, dict[str, Any]]:
        return self._core.emitted_records()

    @property
    def _grading(self) -> list[None]:
        return [None] * self._core.grading()
