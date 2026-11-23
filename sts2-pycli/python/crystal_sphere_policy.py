"""Crystal Sphere: which cell to divine.

The policy is the digger (``sts2_content::events::crystal_sphere_digger``,
through the wheel's ``sts2_rs.crystal_sphere_pick``). It needs what the screen
shows: the fogged cells, the item class of every cleared cell showing a piece
(``revealed``) and the divinations left and at the start
(``divinations_left`` / ``divinations_total``). :func:`choose` asks it when a
message carries all three; an older mod's message, or anything the digger
cannot answer, falls back to the fixed block order below (GH #872), and the
note says so.

The fallback rule (GH #872). The minigame (``CrystalSphereMinigame.cs``)
hides fifteen items on an 11x11 grid and reveals an item only once EVERY cell
it covers is clear. The mod never changes the tool, so each ``divine_cell`` is
the default Big tool: the clicked cell and its eight neighbours. The game enables proceed only once the
divinations are spent (``NCrystalSphereScreen``: ``if (_entity.IsFinished)``),
so stopping early is never a choice -- only which cells to clear. A cell click
after that is a no-op, which is why proceed always wins when it is offered:
picking a cell there just loops the screen.

An older bridge sends only the hidden cells' coordinates, not the board's
contents, so the rule is geometric. The relic is 4x4: only adjacent, non-overlapping 3x3
blocks can uncover it. :data:`CENTERS` clears a 3-wide column first (the
3-divination event option) and then the column beside it (the 6-divination
option, a 6x9 rectangle). A Monte Carlo over the decomp's placement rules
(n=4000; item values relic 150, rare card 45, rare potion 40, ..., curse -60)
put this order at EV ~46 / ~167 for 3 / 6 divinations, against ~34 / ~69 for a
uniformly random hidden cell (what ``realgame_random`` did) and ~21 / ~50 for
the first hidden cell (what F7 clicked).

A centre already cleared (the player divined by hand first) is skipped; past
the list, the cell clearing the most still-hidden cells wins, nearest the grid
centre on a tie.

Twin of ``spire-mentor-mod/CrystalSpherePick.cs`` (F7's lane, where the mod
answers the screen itself). Keep the two orders identical.
"""

from __future__ import annotations

import logging
from typing import Any

logger = logging.getLogger(__name__)

#: ``_defaultWidth`` / ``_defaultHeight`` (``CrystalSphereMinigame.cs``).
GRID_SIZE = 11

#: Big-tool centres, in order: column x 2..4, then x 5..7.
CENTERS: tuple[tuple[int, int], ...] = ((3, 2), (3, 5), (3, 8), (6, 2), (6, 5), (6, 8))

#: Names the deciding policy in the action's ``annotations`` (and so in the
#: driver's JSONL step record): the #872 rule, and the digger.
POLICY_NAME = "crystal_sphere_rule"
DIGGER_POLICY_NAME = "crystal_sphere_digger"


def pick_cell(hidden: list[tuple[int, int]]) -> int | None:
    """Index into ``hidden`` of the cell to divine, or ``None`` if none is hidden."""
    import sts2_rs

    return sts2_rs.crystal_sphere_pick_cell(hidden)


def choose(state: dict[str, Any]) -> dict[str, Any] | None:
    """The bridge answer to a ``crystal_sphere`` message, or ``None`` when no
    enabled option is usable (the caller falls back).

    The message shape is ``SimCrystalSphereScreenHandler``'s (PROTOCOL.md
    ``crystal_sphere``): ``{index, action: "divine_cell", x, y, enabled}`` per
    hidden cell plus a trailing ``{index, action: "proceed", enabled}``, and,
    from the digger's mod on, ``divinations_left``, ``divinations_total`` and
    ``revealed: [{x, y, item}]``. Proceed wins when offered; then the digger,
    when all three fields are there; then :func:`pick_cell`.
    """
    import sts2_rs

    # Rust: `sts2_advisor::crystal_sphere::choose`. A rule pick where the
    # digger's fields were there must never pass for the digger's: the note
    # says why, and so does the log.
    action, warning = sts2_rs.crystal_sphere_choose(state)
    if warning is not None:
        logger.warning("%s", warning)
    return action


#: The headless host names a cleared cell's item by its texture
#: (``RunSimulator.CrystalSphereItemKind``: ``crystal_sphere_relic``,
#: ``crystal_sphere_rare_potion``, ``crystal_sphere_big_gold``, ...). The
#: digger wants the class; the word that decides it is in the name.
_HOST_KIND_CLASSES = ("relic", "potion", "card", "curse", "gold")


def _host_item_class(kind: str) -> str:
    for cls in _HOST_KIND_CLASSES:
        if cls in kind:
            return cls
    return kind  # unknown: the digger refuses it and the rule answers


def host_click(state: dict[str, Any]) -> dict[str, Any] | None:
    """The click for the headless host's ``crystal_sphere`` decision: the
    arguments of a ``crystal_sphere_click``, or ``None`` when no cell is fogged.

    The host (referee mode, GH #1325) sends the whole grid as
    ``cells: [{x, y, hidden, item?: {kind, ...}}]`` and takes any fogged cell
    with either tool. This plays the same policy as :func:`choose`, always
    with the Big tool; the Small tool is the client's to pick, not ours.
    """
    options: list[dict[str, Any]] = []
    revealed: list[dict[str, Any]] = []
    for cell in state.get("cells") or []:
        if cell.get("hidden"):
            options.append({"index": len(options), "action": "divine_cell", **_xy(cell)})
        elif isinstance(cell.get("item"), dict):
            kind = str(cell["item"].get("kind", ""))
            revealed.append({**_xy(cell), "item": _host_item_class(kind)})
    message = {
        "options": options,
        "divinations_left": state.get("divinations_left"),
        "divinations_total": state.get("divinations_total"),
        "revealed": revealed,
    }
    answer = choose(message)
    if answer is None:
        return None
    picked = options[answer["index"]]
    return {"x": picked["x"], "y": picked["y"], "tool": "big"}


def _xy(cell: dict[str, Any]) -> dict[str, int]:
    return {"x": int(cell["x"]), "y": int(cell["y"])}
