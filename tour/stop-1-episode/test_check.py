"""Tests for stop 1's check: the episode loop and the panel summary."""

from __future__ import annotations

import pytest
import tourlib

import check

SEED = tourlib.PANEL_A[0]


def _step(forced: bool, combat: bool, dtype: str = "", n: int = 1) -> check.Step:
    return check.Step(forced, combat, dtype, n, None if forced else 0, None if forced else 1)


def test_summarize_counts_decisions_and_forced_steps() -> None:
    a = check.Episode(
        1,
        [
            _step(False, False, "NEOW", 3),
            _step(True, True),
            _step(False, True, "COMBAT_TURN", 4),
            _step(False, True, "COMBAT_TURN", 2),
        ],
        {"floor": 2, "outcome": "lost"},
    )
    b = check.Episode(
        2, [_step(False, False, "MAP", 5), _step(True, False)], {"floor": 6, "outcome": "won"}
    )
    s = check.summarize([a, b])
    assert s["decisions_per_run"] == pytest.approx(2.0)  # (3 + 1) / 2
    assert s["combat_share"] == pytest.approx(0.5)  # 2 of 4 decisions
    assert s["forced_share"] == pytest.approx(2 / 6)
    assert s["forced_combat_share"] == pytest.approx(0.5)
    assert s["median_classes"] == pytest.approx(3.5)  # of 2, 3, 4, 5
    assert s["by_type"] == {"COMBAT_TURN": 2, "NEOW": 1, "MAP": 1}
    assert s["mean_floor"] == pytest.approx(4.0)
    assert s["outcomes"] == {"lost": 1, "won": 1}


def test_describe_names_cards_targets_and_skips() -> None:
    hand = [{"id": "STRIKE"}, {"id": "DEFEND"}]
    enemies = [{"id": "Seapunk"}]
    assert (
        check.describe({"action": "play", "card_index": 0, "target_index": 0}, hand, enemies)
        == "play STRIKE at Seapunk"
    )
    assert (
        check.describe({"action": "play", "card_index": 1, "target_index": None}, hand, enemies)
        == "play DEFEND"
    )
    assert check.describe({"action": "pick_card_reward", "idx": None}) == "pick_card_reward: skip"
    assert (
        check.describe({"action": "pick_card_reward", "idx": 1}, offered=["A", "B"])
        == "pick_card_reward: B"
    )
    assert check.describe({"action": "end_turn"}) == "end turn"


def test_random_play_is_a_whole_run_and_repeats() -> None:
    ep = check.play(SEED, check.random_chooser(SEED))
    assert ep.end["outcome"] in {"won", "lost", "capped", "error"}
    assert ep.decisions and all(s.n_classes >= 2 for s in ep.decisions)
    assert all(0 <= s.chosen < s.n_classes for s in ep.decisions)
    again = check.play(SEED, check.scripted_chooser(ep.chosen))
    assert again.chosen == ep.chosen and again.end == ep.end
    assert again.n_forced == ep.n_forced


@pytest.mark.skipif(
    not (tourlib.weights_dir() / check.NET).is_file(), reason="published weights not fetched"
)
def test_logged_loop_plays_what_selfplay_plays() -> None:
    import sts2_rs

    student = sts2_rs.E2eStudent(str(tourlib.weight(check.NET)))
    ref = sts2_rs.e2e_selfplay(student, SEED)
    ep = check.play(SEED, check.net_chooser(student))
    assert ep.chosen == list(ref["chosen"])
    assert ep.end["outcome"] == ref["outcome"] and ep.end["floor"] == ref["floor"]


def test_check_refuses_blank_predictions(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv(tourlib.DEV_ENV, raising=False)
    with pytest.raises(SystemExit, match="not complete"):
        check.main(["--seeds", "1"])


def test_sample_predictions_cover_every_measured_key() -> None:
    import tomllib

    keys = {
        "net_decisions_per_run",
        "random_decisions_per_run",
        "net_fight_share",
        "net_median_classes",
        "random_mean_floor",
        "net_forced_share",
    }
    for name in ("predictions.toml", "predictions.sample.toml"):
        raw = tomllib.loads((check.HERE / name).read_text(encoding="utf-8"))
        assert set(raw) == keys, name
