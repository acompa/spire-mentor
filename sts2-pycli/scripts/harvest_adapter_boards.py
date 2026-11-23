#!/usr/bin/env python3
"""Collect every live fight board the Python tests feed the adapter (GH #1426).

The live advisor rebuilds a fight in Rust (``sts2_rs.combat_wire_from_bridge``,
``sts2-workbench``'s ``combat_import``); the Python two-stage translation
(``realgame_adapter.to_headless_combat_play`` + ``sts2_hint.adapter.translate``)
is its reference. ``tests/test_board_import_parity.py`` holds the two to the
same wire on every board this script collects, so what the Python tests cover
(co-op seats, Dread, Blitz, game builds, History counters, ...) is covered for
the Rust twin too.

It runs the adapter-facing test modules with ``to_headless_combat_play``
wrapped, and writes each distinct board once, sorted, to
``tests/fixtures/board_import/boards.jsonl.gz``. Re-run it after adding a test
that builds a new kind of board::

    cd sts2-pycli && PYTHONPATH=python .venv/bin/python scripts/harvest_adapter_boards.py
"""

from __future__ import annotations

import gzip
import json
import sys
from pathlib import Path
from typing import Any

import pytest

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "tests" / "fixtures" / "board_import" / "boards.jsonl.gz"

#: The test modules that build bridge ``combat_action`` boards.
MODULES = (
    "tests/test_realgame_adapter.py",
    "tests/test_realgame_game_version.py",
    "tests/test_realgame_turn_plan.py",
    "tests/test_realgame_ppo.py",
    "tests/test_realgame_eval.py",
    "tests/test_realgame_advisor.py",
    "tests/test_advisor_mirror.py",
    "tests/test_coop_replay.py",
    "tests/test_coop_any_ally.py",
    "tests/test_coop_potion_throw.py",
    "tests/test_coop_teammate_turn.py",
    "tests/workbench/test_state_import.py",
)


class _Harvest:
    """Pytest plugin: record each distinct board ``to_headless_combat_play`` sees."""

    def __init__(self) -> None:
        self.boards: set[str] = set()

    def pytest_configure(self, config: Any) -> None:
        import realgame_adapter

        original = realgame_adapter.to_headless_combat_play
        boards = self.boards

        def recording(state: dict[str, Any], *args: Any, **kwargs: Any) -> dict[str, Any]:
            try:
                boards.add(json.dumps(state, sort_keys=True, ensure_ascii=False))
            except (TypeError, ValueError):
                pass
            return original(state, *args, **kwargs)

        # Before collection, so a test module's `from realgame_adapter import
        # to_headless_combat_play` binds the recording one.
        setattr(realgame_adapter, "to_headless_combat_play", recording)  # noqa: B010


def main() -> int:
    harvest = _Harvest()
    code = pytest.main(["-q", "-n0", "-p", "no:cacheprovider", *MODULES], plugins=[harvest])
    if code != 0:
        print(f"the harvested tests failed (exit {code}); nothing written", file=sys.stderr)
        return int(code)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    text = "".join(f"{b}\n" for b in sorted(harvest.boards))
    # mtime 0: the same boards give the same bytes.
    OUT.write_bytes(gzip.compress(text.encode("utf-8"), mtime=0))
    print(f"{len(harvest.boards)} boards -> {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
