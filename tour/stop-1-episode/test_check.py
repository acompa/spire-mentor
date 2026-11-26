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


def _rec(T: int, forced: int, outcome: str, g0: float) -> dict:
    return {
        "T": T,
        "forced": forced,
        "fight_decisions": T // 2,
        "floor": 46 if outcome == "won" else 3,
        "outcome": outcome,
        "classes": {"2": T // 2, "4": T - T // 2},
        "members": {"3": T // 2, "4": T - T // 2},
        "forced_actions": {"fight end_turn": forced},
        "env": {"G0": g0},
    }


def test_figures_summary_uses_runs_as_the_sampling_unit() -> None:
    import figures

    s = figures.summary([_rec(10, 2, "won", 1.242), _rec(4, 0, "lost", 0.006)])
    assert s["T_mean"] == pytest.approx(7.0)
    assert s["forced_share"] == pytest.approx(2 / 16)
    assert s["J_hat"] == pytest.approx((1.242 + 0.006) / 2)
    # standard error over 2 runs, not over 14 decisions
    assert s["J_se"] == pytest.approx(abs(1.242 - 0.006) / 2)
    assert s["classes_mean"] == pytest.approx(3.0)
    assert s["merge_ratio"] == pytest.approx(3.5 / 3.0)
    assert s["forced_fight_share"] == pytest.approx(1.0)
