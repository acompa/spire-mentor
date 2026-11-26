import math

import pytest
from ppo.e2e.stats import PairedTable, newcombe_paired

import check


def test_mde80_matches_the_plans_power_figure():
    # Plan §1.5: paired SE 0.79 pp at d = 0.25 on 4,000 seeds; MDE80 ~2 pp (§13).
    se = math.sqrt(0.25 / 4000)
    assert se == pytest.approx(0.0079, abs=5e-5)
    assert check.mde80(4000, 0.25) == pytest.approx((check.Z95 + check.Z80) * se)
    assert 0.020 < check.mde80(4000, 0.25) < 0.023


def test_mde80_shrinks_with_root_n_and_pairing_helps_when_d_is_small():
    assert check.mde80(4000, 0.2) == pytest.approx(check.mde80(1000, 0.2) / 2)
    assert check.mde80(1000, 0.2) < check.mde80_unpaired(1000, 0.7)


def test_unpaired_newcombe_is_paired_newcombe_at_zero_correlation():
    # a*d == b*c, so phi = 0 and the paired interval reduces to the unpaired one.
    a, b, c, d = 30, 20, 15, 10
    assert a * d - b * c == 0
    assert check.newcombe_unpaired(a + b, 75, a + c, 75) == pytest.approx(newcombe_paired(a, b, c, d))


def test_paired_se_is_smaller_when_the_arms_agree():
    t = PairedTable(a=280, b=30, c=10, d=80)
    assert check.paired_se(t) < check.unpaired_se(t)
    assert check.paired_se(t) == pytest.approx(math.sqrt((0.1 - 0.05**2) / 400))


def test_unpaired_z_p():
    assert check.unpaired_z_p(50, 100, 50, 100) == pytest.approx(1.0)
    assert check.unpaired_z_p(60, 100, 40, 100) == pytest.approx(check.unpaired_z_p(40, 100, 60, 100))
    assert check.unpaired_z_p(60, 100, 40, 100) < 0.01


def test_slice_rates_are_disjoint_and_consecutive():
    wins = [True] * 4 + [False] * 4 + [True, False] * 2
    assert check.slice_rates(wins, 3) == [1.0, 0.0, 0.5]


def test_hours_for():
    # 1,000 seeds x 2 networks x 700 decisions at 800 decisions/s
    assert check.hours_for(1000, 700, 800) == pytest.approx(1000 * 2 * 700 / 800 / 3600)
