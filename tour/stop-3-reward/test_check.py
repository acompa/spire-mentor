"""The reward ledger must account for every unit of reward the training env
assigns, and the rule used for hypothetical runs must be the env's rule."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest

_spec = importlib.util.spec_from_file_location("stop3_check", Path(__file__).with_name("check.py"))
check = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = check  # dataclasses look their module up
_spec.loader.exec_module(check)

R_WIN, R_BOSS, R_FLOOR = check.constants()


def test_constants_are_the_preregistered_rule():
    assert (R_WIN, R_BOSS, R_FLOOR) == (1.0, 0.05, 0.002)


def test_split_names_each_part():
    assert check.split(R_FLOOR, 1, False) == pytest.approx((R_FLOOR, 0.0, 0.0))
    assert check.split(R_FLOOR + R_BOSS, 1, False) == pytest.approx((R_FLOOR, R_BOSS, 0.0))
    assert check.split(R_WIN, 0, True) == pytest.approx((0.0, 0.0, R_WIN))


def test_split_refuses_reward_it_cannot_explain():
    with pytest.raises(AssertionError):
        check.split(0.01, 1, False)


def test_rule_on_outcome_facts():
    assert check.return_from_outcome(0, 0, False) == 0.0
    assert check.return_from_outcome(46, 3, True) == pytest.approx(1.0 + 0.15 + 0.092)


@pytest.fixture(scope="module", autouse=True)
def _build():
    check._select_build()


@pytest.mark.parametrize("seed", list(check.tourlib.PANEL_A[:3]))
def test_random_ledger_matches_env_total(seed):
    run = check.play_random(seed)
    check.verify(run)  # ledger == env total == rule(outcome facts)
    assert run.ret == pytest.approx(run.total_reward, abs=1e-9)
    assert run.unassigned_reward == 0.0  # a run from Neow starts on floor 0


def test_network_ledger_matches_env_total():
    run = check.play_net(check.tourlib.PANEL_A[0])
    check.verify(run)
    assert run.ret == pytest.approx(run.total_reward, abs=1e-9)
    assert run.ledger[-1].running == pytest.approx(run.ret)
    if run.won:
        assert run.ledger[-1].win == R_WIN
