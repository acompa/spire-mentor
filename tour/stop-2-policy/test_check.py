import math

import numpy as np
import pytest

import check
import tourlib


def test_softmax_sums_to_one_and_keeps_order():
    p = check.softmax([2.0, 1.0, -3.0])
    assert p.sum() == pytest.approx(1.0)
    assert list(np.argsort(-p)) == [0, 1, 2]


def test_softmax_survives_large_logits():
    p = check.softmax([1000.0, 999.0])
    assert p == pytest.approx([1 / (1 + math.exp(-1)), 1 / (1 + math.e)])


def test_entropy_bounds():
    assert check.entropy([1.0, 0.0, 0.0]) == 0.0
    assert check.entropy([0.25] * 4) == pytest.approx(math.log(4))


def _run(seed, won):
    return check.Run(seed, "x", won, "won" if won else "lost", [])


def test_paired_table():
    g = [_run(1, True), _run(2, True), _run(3, False), _run(4, False)]
    s = [_run(1, True), _run(2, False), _run(3, True), _run(4, False)]
    assert check.paired(g, s) == {"both": 1, "only_greedy": 1, "only_sampled": 1, "neither": 1}


@pytest.fixture(scope="module")
def student():
    sts2_rs = pytest.importorskip("sts2_rs")
    return sts2_rs.E2eStudent(str(tourlib.weight(check.NET)))


@pytest.mark.parametrize("seed", list(tourlib.PANEL_A[:3]))
def test_greedy_loop_plays_what_selfplay_plays(student, seed):
    """The peek/decide/apply loop, played greedily, chooses exactly the classes
    the native `e2e_selfplay` chooses and reaches the same result."""
    import sts2_rs

    native = sts2_rs.e2e_selfplay(student, seed)
    ours = check.play(student, seed, "greedy")
    assert [s.played for s in ours.steps] == native["chosen"]
    assert ours.won == native["won"]


def test_sampled_play_is_reproducible(student):
    seed = tourlib.PANEL_A[0]
    a = check.play(student, seed, "sample", max_decisions=150)
    b = check.play(student, seed, "sample", max_decisions=150)
    assert [s.played for s in a.steps] == [s.played for s in b.steps]


def test_low_temperature_sampling_is_greedy(student):
    """As T -> 0 the tempered softmax puts all its mass on the top logit."""
    seed = tourlib.PANEL_A[1]
    greedy = check.play(student, seed, "greedy", max_decisions=120)
    cold = check.play(student, seed, "sample", max_decisions=120, temperature=1e-3)
    assert [s.played for s in cold.steps] == [s.played for s in greedy.steps]
