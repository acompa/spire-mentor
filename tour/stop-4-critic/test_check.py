"""The hand-written advantage arithmetic of stop 4 against the repository's.

    tour/run -m pytest tour/stop-4-critic
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import tourlib  # noqa: E402
from critic import (  # noqa: E402
    Trace,
    fights_of,
    hand_gae,
    hand_two_chain,
    trace_run,
    weight_of_later_surprise,
)
from ppo.e2e import algo  # noqa: E402


def synthetic(kind: str, rng: np.random.Generator) -> Trace:
    """A run of "R" (run) and "C" (combat) rows with random values and the
    campaign's kinds of reward: floors, boss bonuses, a win at the end."""
    n = len(kind)
    comb = np.array([k == "C" for k in kind])
    fight = np.where(comb, np.cumsum(np.r_[True, comb[1:] & ~comb[:-1]]) - 1, -1)
    reward = np.zeros(n)
    hit = rng.random(n) < 0.05
    reward[hit] = rng.choice([0.002, 0.05], int(hit.sum()))
    reward[-1] += 1.0
    z = np.zeros(n)
    return Trace(
        seed=0, won=True, outcome="won", is_combat=comb, fight=fight,
        room=np.full(n, 3), hp_frac=np.ones(n), act=np.ones(n, int), floor=z.astype(int),
        reward=reward, value=rng.uniform(0, 1.2, n), v_combat=z, v_run=z,
    )  # fmt: skip


def test_hand_gae_is_the_textbook_recursion() -> None:
    v, r = [0.5, 0.6, 0.4], [0.0, 0.0, 1.0]
    lam = 0.9
    d = [0.0 + 0.6 - 0.5, 0.0 + 0.4 - 0.6, 1.0 + 0.0 - 0.4]
    want = [d[0] + lam * (d[1] + lam * d[2]), d[1] + lam * d[2], d[2]]
    assert hand_gae(v, r, 0.0, lam) == pytest.approx(want, abs=1e-15)


@pytest.mark.parametrize("seed", range(20))
def test_hand_two_chain_matches_compute_returns(seed: int) -> None:
    rng = np.random.default_rng(seed)
    kind = ""
    while len(kind) < 300:
        kind += "R" * int(rng.integers(1, 7)) + "C" * int(rng.integers(5, 40))
    tr = synthetic(kind, rng)
    repo = algo.compute_returns(tr.table(), tr.value, hold_iters=0)
    np.testing.assert_allclose(hand_two_chain(tr, algo.LAM_C, algo.LAM_R), repo.adv, atol=1e-12)


def test_weight_of_a_later_surprise_is_lambda_to_the_k() -> None:
    for k in (0, 1, 5, 20):
        assert weight_of_later_surprise(k, 0.95) == pytest.approx(0.95**k, rel=1e-12)


def test_a_fight_scores_against_the_run_row_after_it() -> None:
    """A fight's last row bootstraps from V_run of the next run row, and the
    run chain steps over the whole fight."""
    rng = np.random.default_rng(7)
    tr = synthetic("RRCCCCRR", rng)
    adv = hand_two_chain(tr, algo.LAM_C, algo.LAM_R)
    v, r = tr.value, tr.reward
    assert adv[5] == pytest.approx(r[5] + v[6] - v[5], abs=1e-12)
    f = fights_of(tr)[0]
    assert (f.v_before, f.v_after) == (v[1], v[6])
    assert f.delta == pytest.approx(r[1:6].sum() + v[6] - v[1], abs=1e-12)


@pytest.mark.skipif(
    not (tourlib.weights_dir() / "ironclad_e2e_d9000_ema0999.pt").is_file(),
    reason="needs the published weights (TOUR_WEIGHTS)",
)
def test_real_run_hand_gae_matches_the_repository() -> None:
    tr = trace_run(tourlib.PANEL_A[0], str(tourlib.weight("ironclad_e2e_d9000_ema0999.pt")))
    assert len(tr) > 100 and np.isfinite(tr.value).all()
    repo = algo.compute_returns(tr.table(), tr.value, hold_iters=0)
    np.testing.assert_allclose(hand_two_chain(tr, algo.LAM_C, algo.LAM_R), repo.adv, atol=1e-12)
    # The value training uses: V_combat on combat rows, V_run elsewhere.
    want = np.where(tr.is_combat, tr.v_combat, tr.v_run)
    np.testing.assert_allclose(tr.value, want, atol=1e-6)
    assert {f.room for f in fights_of(tr)} >= {"monster", "boss"}
