"""GH #1791: with STS2_E2E_CORPSE_POWERS=strip a corpse keeps only what C# keeps."""

from __future__ import annotations

import pytest
from ppo.combat_ctx import snapshot as S


def _names(e: dict) -> list[str]:
    return [p["name"] for p in e["powers"]]


def test_a_corpse_shows_only_the_powers_the_game_keeps(monkeypatch: pytest.MonkeyPatch) -> None:
    corpse = {
        "kind": "X",
        "is_dead": True,
        "powers": [{"name": "VULNERABLE", "amount": 2}, {"name": "MINION", "amount": 1}],
    }
    monkeypatch.setattr(S, "STRIP_CORPSE_POWERS", False)
    assert _names(S._enemy(corpse)) == ["VULNERABLE", "MINION"]
    monkeypatch.setattr(S, "STRIP_CORPSE_POWERS", True)
    assert _names(S._enemy(corpse)) == ["MINION"]
    alive = {"kind": "X", "is_dead": False, "powers": [{"name": "VULNERABLE", "amount": 2}]}
    assert _names(S._enemy(alive)) == ["VULNERABLE"]


@pytest.mark.parametrize(("value", "strip"), [(None, True), ("keep", False), ("strip", True)])
def test_the_strip_is_on_unless_kept(
    monkeypatch: pytest.MonkeyPatch, value: str | None, strip: bool
) -> None:
    """GH #1791: on by default after a neutral paired eval; `keep` restores the engine form."""
    import importlib

    if value is None:
        monkeypatch.delenv("STS2_E2E_CORPSE_POWERS", raising=False)
    else:
        monkeypatch.setenv("STS2_E2E_CORPSE_POWERS", value)
    try:
        assert importlib.reload(S).STRIP_CORPSE_POWERS is strip
    finally:
        monkeypatch.undo()
        importlib.reload(S)
