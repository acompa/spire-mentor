"""GH #872: the Crystal Sphere is played by a geometric rule, not at random.

The messages here are shaped exactly like ``SimCrystalSphereScreenHandler``'s
(PROTOCOL.md ``crystal_sphere``): one ``divine_cell`` option per hidden cell in
the container's x-major order, plus a trailing ``proceed`` once the screen
enables it.
"""

from __future__ import annotations

import logging
import random
import re
from pathlib import Path
from typing import Any

import crystal_sphere_policy as csp
import pytest
import realgame_ppo as rp

_MOD_TWIN = Path(__file__).resolve().parents[2] / "spire-mentor-mod" / "CrystalSpherePick.cs"


def _fresh_hidden() -> list[tuple[int, int]]:
    """The 11x11 grid minus the four Manhattan-radius-2 corner diamonds the
    minigame clears before placing items (``CrystalSphereMinigame`` ctor)."""
    n = csp.GRID_SIZE
    return [(x, y) for x in range(n) for y in range(n) if min(x, n - 1 - x) + min(y, n - 1 - y) > 2]


def _message(
    hidden: list[tuple[int, int]],
    *,
    proceed: bool = False,
    left: int | None = None,
    total: int | None = None,
    revealed: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    """Old shape by default; ``left`` / ``total`` / ``revealed`` add the
    digger's wire fields (``divinations_left``, ``divinations_total``,
    ``revealed``)."""
    options: list[dict[str, Any]] = [
        {"index": i, "action": "divine_cell", "x": x, "y": y, "enabled": True}
        for i, (x, y) in enumerate(hidden)
    ]
    if proceed:
        options.append({"index": len(hidden), "action": "proceed", "enabled": True})
    msg: dict[str, Any] = {"type": "crystal_sphere", "options": options}
    if left is not None:
        msg["divinations_left"] = left
    if total is not None:
        msg["divinations_total"] = total
    if revealed is not None:
        msg["revealed"] = revealed
    return msg


def _divine(hidden: list[tuple[int, int]], x: int, y: int) -> list[tuple[int, int]]:
    """The Big tool: clear (x, y) and its eight neighbours."""
    return [(a, b) for a, b in hidden if abs(a - x) > 1 or abs(b - y) > 1]


def _cell(action: dict[str, Any], hidden: list[tuple[int, int]]) -> tuple[int, int]:
    return hidden[action["index"]]


class TestRule:
    def test_a_fresh_board_starts_the_first_column_not_the_first_cell(self) -> None:
        hidden = _fresh_hidden()
        action = csp.choose(_message(hidden))
        assert action is not None
        assert _cell(action, hidden) == (3, 2)
        assert hidden[0] == (0, 3), "the old F7 pick: an edge cell"

    def test_six_divinations_clear_a_six_by_nine_rectangle(self) -> None:
        hidden = _fresh_hidden()
        order = []
        for _ in range(6):
            action = csp.choose(_message(hidden))
            assert action is not None
            x, y = _cell(action, hidden)
            order.append((x, y))
            hidden = _divine(hidden, x, y)
        assert order == list(csp.CENTERS)
        assert len(_fresh_hidden()) - len(hidden) == 54, "the blocks never overlap"

    def test_proceed_wins_once_offered(self) -> None:
        # Proceed only enables when the divinations are spent; a cell click
        # then is a no-op that would loop the screen.
        hidden = _fresh_hidden()
        action = csp.choose(_message(hidden, proceed=True))
        assert action is not None
        assert action["index"] == len(hidden)
        assert action["annotations"]["policy"] == csp.POLICY_NAME

    def test_a_centre_cleared_by_hand_is_skipped(self) -> None:
        hidden = _divine(_fresh_hidden(), 3, 2)
        action = csp.choose(_message(hidden))
        assert action is not None
        assert _cell(action, hidden) == (3, 5)

    def test_past_the_list_the_densest_block_wins(self) -> None:
        hidden = [(0, 5), (9, 9), (9, 10), (10, 9), (10, 10), (5, 0)]
        assert hidden[csp.pick_cell(hidden)] == (9, 9)
        lone = [(0, 5), (5, 4), (10, 5)]
        assert lone[csp.pick_cell(lone)] == (5, 4), "tie: nearest the centre"

    def test_disabled_and_malformed_options_are_ignored(self) -> None:
        msg = {
            "type": "crystal_sphere",
            "options": [
                {"index": 0, "action": "divine_cell", "x": 3, "y": 2, "enabled": False},
                {"index": 1, "action": "divine_cell", "x": "?", "y": 5},
                {"index": 2, "action": "divine_cell", "x": 7, "y": 7, "enabled": True},
                {"index": 3, "action": "proceed", "enabled": False},
            ],
        }
        action = csp.choose(msg)
        assert action is not None
        assert action["index"] == 2

    def test_nothing_usable_returns_none(self) -> None:
        assert csp.choose({"type": "crystal_sphere", "options": []}) is None
        assert csp.pick_cell([]) is None

    def test_beats_the_old_picks_on_simulated_boards(self) -> None:
        """A cheap reproduction of the measurement behind the order: on random
        boards (placement per the decomp), the rule reveals more items than
        the first hidden cell or a random one at both divination counts."""
        items = [(4, 4)] + [(1, 3)] * 2 + [(2, 2)] * 5 + [(1, 1)] * 5 + [(2, 1)] * 2

        def revealed(pick: Any, divs: int, rng: random.Random) -> int:
            hidden = set(_fresh_hidden())
            placed: list[set[tuple[int, int]]] = []
            free = set(hidden)
            for w, h in items:
                cand = [
                    (x, y)
                    for x in range(csp.GRID_SIZE - w + 1)
                    for y in range(csp.GRID_SIZE - h + 1)
                    if all((x + i, y + j) in free for i in range(w) for j in range(h))
                ]
                x, y = rng.choice(cand)
                cells = {(x + i, y + j) for i in range(w) for j in range(h)}
                free -= cells
                placed.append(cells)
            for _ in range(divs):
                order = sorted(hidden)
                cx, cy = order[pick(order, rng)]
                hidden -= {(cx + dx, cy + dy) for dx in (-1, 0, 1) for dy in (-1, 0, 1)}
            return sum(not (cells & hidden) for cells in placed)

        def rule(order: list[tuple[int, int]], _rng: random.Random) -> int:
            i = csp.pick_cell(order)
            assert i is not None
            return i

        def first(_order: list[tuple[int, int]], _rng: random.Random) -> int:
            return 0

        def uniform(order: list[tuple[int, int]], rng: random.Random) -> int:
            return rng.randrange(len(order))

        for divs in (3, 6):
            totals = {}
            for name, pick in (("rule", rule), ("first", first), ("random", uniform)):
                rng = random.Random(872)
                totals[name] = sum(revealed(pick, divs, rng) for _ in range(300))
            assert totals["rule"] > totals["random"] > totals["first"], (divs, totals)


class TestDigger:
    """With the digger's wire fields, the wheel's ``crystal_sphere_pick``
    decides; an old-shape message, or anything the digger cannot answer,
    gets the #872 rule (``TestRule``) instead."""

    def test_new_shape_message_gets_the_digger(self) -> None:
        hidden = _fresh_hidden()
        action = csp.choose(_message(hidden, left=6, total=6, revealed=[]))
        assert action is not None
        assert action["annotations"]["policy"] == csp.DIGGER_POLICY_NAME == "crystal_sphere_digger"
        assert _cell(action, hidden) in hidden

    def test_digger_matches_the_wheel(self) -> None:
        sts2_rs = pytest.importorskip("sts2_rs")
        hidden = _fresh_hidden()
        action = csp.choose(_message(hidden, left=6, total=6, revealed=[]))
        assert action is not None
        assert _cell(action, hidden) == sts2_rs.crystal_sphere_pick(hidden, [], 6, 6)

    def test_old_shape_message_gets_the_rule(self) -> None:
        hidden = _fresh_hidden()
        action = csp.choose(_message(hidden))
        assert action is not None
        assert action["annotations"]["policy"] == csp.POLICY_NAME == "crystal_sphere_rule"
        assert _cell(action, hidden) == (3, 2)

    def test_proceed_wins_when_offered(self) -> None:
        hidden = _fresh_hidden()
        action = csp.choose(_message(hidden, proceed=True, left=0, total=6, revealed=[]))
        assert action is not None
        assert action["index"] == len(hidden)

    # A digger cell that is not offered (the digger misbehaving) falls back to
    # the rule: `sts2_advisor::crystal_sphere`'s
    # `a_digger_cell_that_is_not_offered_falls_back_to_the_rule`, which can
    # stand a misbehaving digger in.

    def test_bad_revealed_item_falls_back(self) -> None:
        hidden = _fresh_hidden()
        msg = _message(hidden, left=6, total=6, revealed=[{"x": 5, "y": 5, "item": "banana"}])
        action = csp.choose(msg)
        assert action is not None
        assert _cell(action, hidden) == (3, 2)
        assert action["annotations"]["policy"] == csp.POLICY_NAME
        assert "fell back" in action["annotations"]["note"]

    def test_digger_fallback_warns(self, caplog: pytest.LogCaptureFixture) -> None:
        # The fields were there, the digger did not answer: a warning, so a
        # rule pick never passes silently for the digger's.
        hidden = _fresh_hidden()
        msg = _message(hidden, left=6, total=6, revealed=[{"x": 5, "y": 5, "item": "banana"}])
        with caplog.at_level(logging.WARNING, logger=csp.__name__):
            action = csp.choose(msg)
        assert action is not None
        assert action["annotations"]["policy"] == csp.POLICY_NAME
        warnings = [r for r in caplog.records if r.levelno == logging.WARNING]
        assert len(warnings) == 1
        assert "digger did not answer" in warnings[0].getMessage()
        assert "fell back" in warnings[0].getMessage()

    def test_no_warning_without_the_fields_or_when_the_digger_answers(
        self, caplog: pytest.LogCaptureFixture
    ) -> None:
        # An older mod's message is the rule's by design, not a failure.
        hidden = _fresh_hidden()
        with caplog.at_level(logging.WARNING, logger=csp.__name__):
            assert csp.choose(_message(hidden)) is not None
            assert csp.choose(_message(hidden, left=6, total=6, revealed=[])) is not None
        assert not [r for r in caplog.records if r.levelno >= logging.WARNING]

    def test_zero_left_without_proceed_returns_none_or_rule(self) -> None:
        hidden = _fresh_hidden()
        action = csp.choose(_message(hidden, left=0, total=6, revealed=[]))
        assert action is not None
        assert _cell(action, hidden) == (3, 2)
        assert action["annotations"]["policy"] == csp.POLICY_NAME


class TestDriverLane:
    def test_the_driver_answers_with_the_rule_and_names_it(self) -> None:
        hidden = _fresh_hidden()
        action, log_extra = rp.pick_noncombat_action_ppo(
            _message(hidden), random.Random(0), policies=None, session={}
        )
        assert log_extra is None, "a rule answer is not a PPO fallback"
        assert _cell(action, hidden) == (3, 2)
        assert action["annotations"]["policy"] == csp.POLICY_NAME

    def test_an_unusable_message_still_gets_the_random_fallback(self) -> None:
        msg = {"type": "crystal_sphere", "options": [{"index": 0, "label": "?"}]}
        action, log_extra = rp.pick_noncombat_action_ppo(
            msg, random.Random(0), policies=None, session={}
        )
        assert log_extra is None
        assert action == {"action": "choose", "index": 0}


def test_the_mod_twin_plays_the_same_order() -> None:
    """``CrystalSpherePick.cs`` (F7's lane) and this module must agree."""
    if not _MOD_TWIN.is_file():
        pytest.skip(f"mod source not in this checkout: {_MOD_TWIN}")
    text = _MOD_TWIN.read_text(encoding="utf-8")
    block = re.search(r"Centers\s*=\s*\{(.*?)\};", text, re.S)
    assert block, "Centers array not found"
    pairs = [(int(a), int(b)) for a, b in re.findall(r"\((\d+),\s*(\d+)\)", block.group(1))]
    assert tuple(pairs) == csp.CENTERS


# ── The headless host's shape (referee mode, GH #1325 / #1364) ──────────────


def _host_state(
    clear: dict[tuple[int, int], str | None] | None = None, left: int = 3, total: int | None = 3
) -> dict[str, Any]:
    """A host ``crystal_sphere`` decision: all fogged but ``clear`` (cell -> kind)."""
    clear = clear or {}
    cells: list[dict[str, Any]] = []
    for y in range(csp.GRID_SIZE):
        for x in range(csp.GRID_SIZE):
            cell: dict[str, Any] = {"x": x, "y": y, "hidden": (x, y) not in clear}
            if clear.get((x, y)):
                cell["item"] = {"kind": clear[x, y], "part_x": 0, "part_y": 0}
            cells.append(cell)
    state: dict[str, Any] = {"decision": "crystal_sphere", "divinations_left": left, "cells": cells}
    if total is not None:
        state["divinations_total"] = total
    return state


class TestHostClick:
    def test_the_host_shape_gets_the_same_cell_as_the_mod_shape(self) -> None:
        clear = {(5, 5): "crystal_sphere_relic", (5, 6): None, (0, 0): "crystal_sphere_big_gold"}
        state = _host_state(clear, left=5, total=6)
        hidden = [(c["x"], c["y"]) for c in state["cells"] if c["hidden"]]
        mod_answer = csp.choose(
            {
                "options": [
                    {"index": i, "action": "divine_cell", "x": x, "y": y}
                    for i, (x, y) in enumerate(hidden)
                ],
                "divinations_left": 5,
                "divinations_total": 6,
                "revealed": [
                    {"x": 5, "y": 5, "item": "relic"},
                    {"x": 0, "y": 0, "item": "gold"},
                ],
            }
        )
        assert mod_answer is not None
        assert mod_answer["annotations"]["policy"] == csp.DIGGER_POLICY_NAME
        click = csp.host_click(state)
        assert click is not None
        assert (click["x"], click["y"]) == hidden[mod_answer["index"]]
        assert click["tool"] == "big"

    @pytest.mark.parametrize(
        ("kind", "cls"),
        [
            ("crystal_sphere_relic", "relic"),
            ("crystal_sphere_rare_potion", "potion"),
            ("crystal_sphere_common_potion", "potion"),
            ("crystal_sphere_rare_card_reward", "card"),
            ("crystal_sphere_curse", "curse"),
            ("crystal_sphere_gold", "gold"),
            ("crystal_sphere_big_gold", "gold"),
        ],
    )
    def test_every_texture_name_maps_to_a_digger_class(self, kind: str, cls: str) -> None:
        assert csp._host_item_class(kind) == cls

    def test_without_the_starting_count_the_rule_answers(self) -> None:
        click = csp.host_click(_host_state(total=None))
        assert click == {"x": csp.CENTERS[0][0], "y": csp.CENTERS[0][1], "tool": "big"}

    def test_the_click_is_always_a_fogged_cell(self) -> None:
        state = _host_state({c: None for c in csp.CENTERS}, left=2, total=3)
        click = csp.host_click(state)
        assert click is not None
        fogged = {(c["x"], c["y"]) for c in state["cells"] if c["hidden"]}
        assert (click["x"], click["y"]) in fogged

    def test_no_fogged_cell_is_no_click(self) -> None:
        clear = {(x, y): None for x in range(csp.GRID_SIZE) for y in range(csp.GRID_SIZE)}
        assert csp.host_click(_host_state(clear, left=1)) is None

    def test_the_reference_driver_clicks_the_sphere(self) -> None:
        import referee_spike

        state = _host_state()
        cmd = referee_spike.choose(state, random.Random(0))
        assert cmd["action"] == "crystal_sphere_click"
        assert cmd["args"] == csp.host_click(state)
