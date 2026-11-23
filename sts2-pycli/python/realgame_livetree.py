"""Live-game branch tree for the advisor's control plane (rewind lane).

A thin tree over the events the advisor already sees — one node per distinct
decision the mod requested advice for, stamped with the mod's snapshot ids
(``snapshot_saved``) so the workbench's ``BranchTree.tsx`` can offer the
seekable ones as rewind targets. A ``snapshot_restored`` moves the active
pointer back to the node holding that snapshot; the next decision then forks
a child there, which is exactly the fork the tidy-tree draws.

Deliberately NOT :class:`workbench.branching.BranchingLiveSession`: that
class owns and drives a deterministic engine backend (its ``seek`` is
replay-from-scratch), while this one only *observes* a human-driven live
game and delegates seeks to the mod's in-process snapshot restore.

``timeline()`` returns the exact ``{nodes, active_path, active_leaf}`` wire
shape the workbench's live views consume (``LiveNode``/``LiveTimeline`` in
``web/src/api.ts``), with two additive fields per node: ``snapshot_id``
(str | None) and ``seekable`` (bool).

Rust: ``sts2_advisor::livetree`` (GH #1426 step 4); this class keeps its
Python name over ``sts2_rs.LiveRunTreeCore``.
"""

from __future__ import annotations

from typing import Any

import sts2_rs


class LiveRunTree:
    """Thread-safe branch tree of one live run. All entry points may be
    called from the advisor's socket loop or the control plane's HTTP
    threads."""

    def __init__(self) -> None:
        self._core = sts2_rs.LiveRunTreeCore()

    # -- events (called from the advisor loop) -------------------------

    def on_decision(self, msg: dict[str, Any], advice_for: str, log_idx: int | None = None) -> None:
        """A new advised decision: append a node under the active pointer.

        ``log_idx`` is the replay step index of the decision's ``state`` line
        in the run log (``WorkbenchRunLogger.log_decision``): the node -> step
        join the live coach's "open in workbench" links need, since node ids
        and step indices diverge (root, room entries, rewind forks).
        """
        self._core.on_decision(msg, advice_for, log_idx)

    def on_human_action(self, msg: dict[str, Any]) -> int | None:
        """Refine a decision node's label with what the human actually did.

        The target is the node the action ANSWERED (its ``advice_request_id``),
        not whichever node is active: the mod's history poll can deliver the
        action for decision N after the advice request for decision N+1 has
        already appended a node. Reports without an id fall back to the newest
        node still awaiting its label.

        Returns the id of the node the action answered (the live coach keys
        its grades by it), or ``None`` when no decision node took it.
        """
        return self._core.on_human_action(msg)

    def on_snapshot_saved(self, msg: dict[str, Any]) -> None:
        """Stamp the mod's snapshot onto the tree: a room-entry snapshot is
        its own node ("floor N" waypoint); a post-combat snapshot stamps the
        active node (the fight it concluded)."""
        self._core.on_snapshot_saved(msg)

    def on_snapshot_restored(self, msg: dict[str, Any]) -> None:
        """The game rewound: move the active pointer to the snapshot's node.
        The original subtree stays; the next decision forks a sibling branch."""
        self._core.on_snapshot_restored(msg)

    def on_combat_undone(self, msg: dict[str, Any]) -> None:
        """The game replayed a combat tape to the start of a turn: move the
        active pointer to the last node BEFORE that turn, so the next decision
        forks there."""
        self._core.on_combat_undone(msg)

    def on_run_end(self, msg: dict[str, Any]) -> None:
        self._core.on_run_end(msg)

    def snapshot_id_of(self, node_id: int) -> str | None:
        return self._core.snapshot_id_of(node_id)

    def seek_plan(self, node_id: int) -> dict[str, Any] | None:
        """The bridge verb that lands the game where *node_id* says
        (``undo_turn`` for a combat node with a turn, ``restore_snapshot``
        for a room-entry / post-combat node); ``None`` = nothing exact."""
        return self._core.seek_plan(node_id)

    # -- projection ----------------------------------------------------

    def log_idx(self, node_id: Any) -> int | None:
        """The run-log replay step of ``node_id`` (``None`` when unknown)."""
        return self._core.log_idx(node_id)

    def active_path(self) -> list[int]:
        """Node ids from the root to the active node (the live coach withdraws
        grades for nodes off this path after a rewind)."""
        return self._core.active_path()

    def timeline(self) -> dict[str, Any]:
        """The ``LiveTimeline`` wire shape (plus snapshot_id/seekable)."""
        return self._core.timeline()
