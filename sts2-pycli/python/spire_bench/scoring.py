"""Scores: win rate with a Wilson interval, pooled over every episode started.

An agent cannot drop bad episodes: every episode it starts counts, and one it
abandons is lost at the deadline. Void episodes (the referee failed) are left
out and reported separately.
"""

from __future__ import annotations

import math
from collections import defaultdict
from collections.abc import Iterable
from typing import Any


def wilson(wins: int, n: int, z: float = 1.96) -> tuple[float, float]:
    """95% Wilson score interval for ``wins`` out of ``n``."""
    if n == 0:
        return (0.0, 1.0)
    p = wins / n
    denom = 1 + z * z / n
    centre = (p + z * z / (2 * n)) / denom
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / denom
    return (max(0.0, centre - half), min(1.0, centre + half))


def score(episodes: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    """One row per (agent, character, ascension), from episode records.

    Each record has ``agent_id``, ``character``, ``ascension`` and ``status``
    (``running``, ``won``, ``lost`` or ``void``).
    """
    cells: dict[tuple[str, str, int], dict[str, int]] = defaultdict(
        lambda: {"won": 0, "lost": 0, "void": 0, "running": 0}
    )
    for ep in episodes:
        cells[ep["agent_id"], ep["character"], int(ep["ascension"])][ep["status"]] += 1
    rows = []
    for (agent, character, ascension), c in sorted(cells.items()):
        n = c["won"] + c["lost"]
        lo, hi = wilson(c["won"], n)
        rows.append(
            {
                "agent_id": agent,
                "character": character,
                "ascension": ascension,
                "finished": n,
                "won": c["won"],
                "win_rate": c["won"] / n if n else None,
                "wilson_95": [lo, hi],
                "running": c["running"],
                "void": c["void"],
            }
        )
    return rows
