"""The check's arithmetic on cases small enough to work by hand."""

import math

import numpy as np
import pytest

from check import (
    clamped_share,
    ess_share,
    is_estimates,
    log_softmax,
    mix_logp,
    window_log_weights,
)


def test_ratio_of_a_hand_made_pair():
    # Behaviour gives the taken action 0.5, the target 0.25: weight 0.5.
    behav = np.log([0.5, 0.5])
    target = np.log([0.25, 0.75])
    assert math.exp(target[0] - behav[0]) == pytest.approx(0.5)
    assert math.exp(target[1] - behav[1]) == pytest.approx(1.5)


def test_mix_ends_are_the_two_policies():
    a = log_softmax(np.array([2.0, 0.0, -1.0]))
    b = log_softmax(np.array([-1.0, 3.0, 0.0]))
    np.testing.assert_allclose(mix_logp(a, b, 0.0), a, atol=1e-12)
    np.testing.assert_allclose(mix_logp(a, b, 1.0), b, atol=1e-12)
    m = mix_logp(a, b, 0.5)
    assert np.exp(m).sum() == pytest.approx(1.0)
    # Halfway in log space is the normalised geometric mean.
    g = np.sqrt(np.exp(a) * np.exp(b))
    np.testing.assert_allclose(np.exp(m), g / g.sum())


def test_ess_equal_weights_is_everyone():
    assert ess_share(np.log([3.0, 3.0, 3.0, 3.0])) == pytest.approx(1.0)


def test_ess_by_hand():
    # Weights 1, 1, 2: (Σw)²/Σw² = 16/6, a share of 16/18.
    assert ess_share(np.log([1.0, 1.0, 2.0])) == pytest.approx(16 / 18)


def test_ess_one_giant_weight_is_one_run():
    # A product of 700 ratios can be e^300; the arithmetic must not overflow.
    lw = np.array([0.0, -5.0, 300.0, 2.0])
    assert ess_share(lw) == pytest.approx(1 / 4)


def test_clamped_share():
    assert clamped_share(np.log([0.5, 1.0, 1.9, 2.5, 10.0])) == pytest.approx(2 / 5)


def test_windows_multiply_consecutive_ratios():
    run1 = np.log([2.0, 0.5, 4.0, 1.0, 3.0])  # windows of 2: [2·0.5, 4·1], tail dropped
    run2 = np.log([0.25, 0.25])
    got = np.exp(window_log_weights([run1, run2], 2))
    np.testing.assert_allclose(got, [1.0, 4.0, 1 / 16])
    np.testing.assert_allclose(np.exp(window_log_weights([run1], 1)), np.exp(run1))


def test_is_estimates_by_hand():
    # Weights 1, 3 on a win and a loss: plain = (1·1 + 3·0)/2, SNIS = 1/4.
    plain, snis = is_estimates(np.log([1.0, 3.0]), np.array([True, False]))
    assert plain == pytest.approx(0.5)
    assert snis == pytest.approx(0.25)


def test_is_estimates_survive_huge_weights():
    plain, snis = is_estimates(np.array([800.0, 0.0]), np.array([True, False]))
    assert plain == math.inf
    assert snis == pytest.approx(1.0)
    plain, snis = is_estimates(np.array([800.0, 0.0]), np.array([False, False]))
    assert plain == 0.0 and snis == 0.0
