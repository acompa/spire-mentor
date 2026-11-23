"""The enemy move inferrer against the host's own move ids (v0.111.0).

The fixture holds full-state host observations of long fights (bosses and
elites included) with ``move_state``/``ai`` stripped, and the true move ids
beside them (``bench_agents.move_infer``).
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

sts2_rs = pytest.importorskip("sts2_rs")

from bench_agents.move_infer import MoveTracker  # noqa: E402

FIXTURE = Path(__file__).parent / "fixtures" / "bench_agents" / "move_truth_v0111.json"
ALIASES = Path(__file__).parents[2] / "sts2-rs" / "crates" / "sts2-wire" / "src" / "state_dict.rs"


def _aliases() -> dict[tuple[str, str], str]:
    text = ALIASES.read_text(encoding="utf-8")
    pat = r'\(\s*MonsterKind::(\w+),\s*"(\w+)",\s*"(\w+)",?\s*\)'
    return {(m[1], m[2]): m[3] for m in re.finditer(pat, text)}


def _norm(aliases: dict, kind: str, move: str | None) -> str | None:
    if move is None:
        return None
    # The engine's own relaxed matcher (`force_current_state_by_name_relaxed`).
    move = aliases.get((kind, move), move)
    return move.removesuffix("_MOVE").replace("_MOVE_", "_")


def test_moves_are_inferred_from_the_shown_intents_alone():
    from sts2_hint import adapter

    aliases = _aliases()
    right = total = 0
    wrong = []
    for fight in json.loads(FIXTURE.read_text(encoding="utf-8")):
        tracker = MoveTracker(fight["character"], fight["ascension"], "v0.111.0")
        for obs, truth in zip(fight["observations"], fight["truth"], strict=True):
            assert all("move_state" not in e for e in obs["enemies"])
            got = tracker.infer(obs)
            wire = adapter.translate(
                {**obs, "game_version": "v0.111.0"},
                character=fight["character"],
                ascension=fight["ascension"],
            )
            live = wire["encounter"][: len(truth)]
            for enc, t, g in zip(live, truth, got, strict=True):
                if t is None:
                    continue
                total += 1
                if _norm(aliases, enc["kind"], t) == _norm(aliases, enc["kind"], g["move_state"]):
                    right += 1
                else:
                    wrong.append((fight["encounter"], t, g["move_state"]))
    # 98% over every v0.111.0 encounter (2026-11-20); these long fights are
    # the hard end of it.
    assert total > 150
    assert right / total >= 0.9, (right, total, wrong[:10])


def test_an_enemy_is_tracked_by_its_combat_id_through_another_ones_death():
    # GH #1651: `index` is the position among the living, so it shifts when
    # an enemy before it dies; `combat_id` does not.
    fight = next(
        f
        for f in json.loads(FIXTURE.read_text(encoding="utf-8"))
        if len(f["observations"][0]["enemies"]) >= 3
    )
    obs = json.loads(json.dumps(fight["observations"][0]))
    for e in obs["enemies"]:
        e["combat_id"] = 10 + e["index"]
    tracker = MoveTracker(fight["character"], fight["ascension"], "v0.111.0")
    tracker.infer(obs)
    survivor = tracker.enemies[12]
    # The first enemy dies: the others move up a slot.
    later = {**obs, "enemies": obs["enemies"][1:]}
    for i, e in enumerate(later["enemies"]):
        e["index"] = i
    tracker.infer(later)
    assert tracker.enemies[12] is survivor
