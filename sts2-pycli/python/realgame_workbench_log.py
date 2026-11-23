"""Workbench-schema run logging for the live-game advisor (phase 9).

Writes each advised live run as a ``game_log.GameLogger``-format JSONL file —
``{step, ts, type: meta|state|action|mcts|coach, data}`` — in the *headless
state dialect*, so the workbench loads it with zero changes: ``/api/runs``
lists it, ``/api/load`` parses it, the Board renders combat steps, the
EventLog labels the human's moves, and the mcts records carry the advice that
was on screen for each decision.

This rides alongside (never replaces) the raw-wire ``AdvisorLog``: that one
is the debugging tape (``tools/ascension_climb.py`` byte-slices it), this
one is the review surface.

Rust: ``sts2_advisor::workbench_log`` (GH #1426 step 4) maps each surface,
dedupes repeated boards and writes the file; this module keeps the Python
names over ``sts2_rs.WorkbenchRunLoggerCore``. Mapping notes live there.
"""

from __future__ import annotations

import logging
from datetime import datetime
from typing import Any

import sts2_rs

logger = logging.getLogger(__name__)


def run_log_name(path: str | None) -> str | None:
    """A run log's name as the control plane's ``GET /log`` serves it.

    The bare filename with anything outside ``[A-Za-z0-9._-]`` replaced: the
    bundled page's run store keys by it (``engineRoutes.ts`` RUN_FILENAME),
    and coach records carry it so the mod can link a decision to its file.
    """
    return sts2_rs.workbench_run_log_name(path)


def _root_outcomes(advice: dict[str, Any]) -> list[dict[str, Any]]:
    """The advice's ranked options as the ``root_outcomes`` rows
    ``TimelineStore.load_jsonl`` turns into per-step ``scores``."""
    return sts2_rs.workbench_root_outcomes(advice)


def human_action_args(msg: dict[str, Any], state: dict[str, Any] | None) -> dict[str, Any]:
    """The native action dict a ``human_action`` stands for, or ``{}``.

    Combat moves come out in the engine's frame against ``state`` (a card
    found in its hand by identity, a potion's ``slot`` among the HELD
    potions); non-combat choices log ``{"index": i | None}``.
    """
    return sts2_rs.workbench_human_action_args(msg, state)


def _now() -> str:
    return datetime.now().isoformat()


class WorkbenchRunLogger:
    """One workbench-schema JSONL per live run, opened lazily.

    A run is identified by ``(character, seed)`` from the message envelope; a
    change (or a terminal message) closes the current file and the next
    decision opens a fresh one. Never raises into the advisor loop.
    """

    def __init__(self, log_dir: str | None) -> None:
        #: Falsy dir disables the whole logger (``--workbench-logs ''``).
        self._core = sts2_rs.WorkbenchRunLoggerCore(
            log_dir or None, _now, logger.info, logger.warning
        )

    @property
    def path(self) -> str | None:
        return self._core.path

    @property
    def log_name(self) -> str | None:
        """The open run log's name as ``GET /log`` serves it, else the name of
        the file closed last (:func:`run_log_name`)."""
        return self._core.log_name

    def close(self) -> None:
        self._core.close()

    def note_reconnect(self) -> None:
        """The connection to the mod dropped. Requests it wrote that this
        process never read are gone with their combat-log rows, so the next
        logged board says so (``coop_replay`` skips a pair across it)."""
        self._core.note_reconnect()

    def log_decision(self, msg: dict[str, Any], advice_for: str) -> int | None:
        """Append the decision's ``state`` record.

        Returns the new step's replay index -- the index the workbench's replay
        view gives this ``state`` line -- or ``None`` when nothing was written
        (duplicate board, unknown surface, or logging disabled).
        """
        return self._core.log_decision(msg, advice_for)

    def log_advice(
        self,
        msg: dict[str, Any],
        advice: dict[str, Any],
        elapsed_s: float,
        tree: dict[str, Any] | None = None,
    ) -> None:
        """Attach the advice to the current step as its ``mcts`` record."""
        self._core.log_advice(msg, advice, elapsed_s, tree)

    def log_human_action(self, msg: dict[str, Any]) -> None:
        """Append the human's move as the current step's ``action`` record
        (``args`` is its machine form, :func:`human_action_args`)."""
        self._core.log_human_action(msg)

    def log_coach(self, record: dict[str, Any]) -> None:
        """Append one coach record (a graded turn / choice, or a withdrawal).

        The death autopsy (and a withdrawal racing it) is built after the run
        end closed the file: those go into the file closed last, re-opened
        for the one line.
        """
        self._core.log_coach(record)

    def log_run_end(self, msg: dict[str, Any]) -> None:
        """Terminal ``game_over``/``run_complete``: final state + summary."""
        self._core.log_run_end(msg)
