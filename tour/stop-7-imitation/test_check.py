"""Tests for the stop's bookkeeping, and one live check that `play` is the
same game loop as `sts2_rs.e2e_selfplay`."""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import tourlib  # noqa: E402

import imitation  # noqa: E402


def test_agreement_is_percent_of_equal_choices():
    assert imitation.agreement(np.array([1, 2, 3, 0]), np.array([1, 2, 0, 0])) == 75.0
    assert np.isnan(imitation.agreement(np.array([]), np.array([])))


@pytest.mark.parametrize("plan", [imitation.FULL, imitation.QUICK])
def test_seed_roles_do_not_overlap(plan):
    train = set(plan.train_seeds(max(plan.sizes)))
    held, dag, ev = set(plan.heldout_seeds()), set(plan.dagger_seeds()), set(plan.eval_seeds())
    assert not train & held and not train & dag and not held & dag
    assert ev <= set(tourlib.PANEL_A) and not ev & (train | held | dag)
    sizes = [plan.train_seeds(n) for n in plan.sizes]
    assert all(a == b[: len(a)] for a, b in zip(sizes, sizes[1:]))  # nested


def test_quantities_pick_the_largest_student_and_its_half():
    res = {
        "bc025": {"heldout_agreement": 50.0},
        "bc050": {"heldout_agreement": 60.0},
        "bc100": {"heldout_agreement": 64.0, "own_agreement": 55.0,
                  "mean_floor": 9.0, "win_rate": 1.5},
        "dagger": {"mean_floor": 10.5},
    }
    q = imitation.quantities(imitation.FULL, res)
    assert q == {
        "heldout_agreement": 64.0,
        "own_state_agreement": 55.0,
        "student_mean_floor": 9.0,
        "student_win_rate": 1.5,
        "agreement_gain_from_doubling": 4.0,
        "dagger_floor_change": 1.5,
    }


def test_play_matches_selfplay_on_one_seed():
    import sts2_rs

    teacher = imitation.teacher_paths()[1]
    seed = tourlib.PANEL_B[0]
    ref = sts2_rs.e2e_selfplay(sts2_rs.E2eStudent(str(teacher)), seed)
    g = imitation.play((seed, str(teacher)))
    assert g.chosen.tolist() == ref["chosen"]
    assert (g.won, g.floor) == (ref["won"], ref["floor"])
    assert len(g.rows["n_tok"]) == g.decisions == ref["decisions"]
    rb, y = imitation.runs(g.rows, g.chosen, [seed])
    assert len(y) == g.decisions
    assert imitation.agreement(imitation.choices(imitation.teacher_paths()[0], rb), y) == 100.0


def test_soft_cross_entropy_is_hard_cross_entropy_for_one_hot_teacher():
    import torch

    student = torch.tensor([[2.0, 0.5, -float("inf")], [0.0, 1.0, 3.0]])
    hard = torch.tensor([[50.0, -50.0, -float("inf")], [-50.0, -50.0, 50.0]])
    expected = torch.nn.functional.cross_entropy(student, torch.tensor([0, 2]))
    assert torch.isclose(imitation.soft_cross_entropy(student, hard), expected, atol=1e-5)


def test_soft_cross_entropy_is_teacher_entropy_when_student_matches():
    import torch

    logits = torch.tensor([[1.0, 0.0, -1.0]])
    p = torch.softmax(logits, -1)
    entropy = -(p * p.log()).sum()
    assert torch.isclose(imitation.soft_cross_entropy(logits, logits), entropy, atol=1e-6)


def test_render_fills_measured_values_and_differences():
    import figures

    d = {"teacher": {"mean_floor": 45.07}, "students": {"bc100": {"mean_floor": 17.0}}}
    html = ('<span class="v" data-k="teacher.mean_floor" data-f=".1f">?</span> '
            '<span class="v" data-k="teacher.mean_floor - students.bc100.mean_floor" data-f="+.1f">old</span>')
    assert figures.fill_values(html, d) == (
        '<span class="v" data-k="teacher.mean_floor" data-f=".1f">45.1</span> '
        '<span class="v" data-k="teacher.mean_floor - students.bc100.mean_floor" data-f="+.1f">+28.1</span>')
