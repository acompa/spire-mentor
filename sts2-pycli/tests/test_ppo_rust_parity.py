"""Phase-2 gates A2 + C: Rust obs builder and Rust PPO agent vs Python.

A2: sts2_rs.ppo_observe_json == ppo.env.observe on every yielded decision.
C:  Rust PpoRunAgent.evaluate matches Python NetPolicy.distribution
    (same argmax; |Δπ|, |ΔV| < 1e-3) along Python-greedy trajectories.
Plus two checks on the same states: `evaluate_with_actions` agrees with
`evaluate`, and the workbench's rust backend with its python one.

All four run in ONE walk per seed (`test_rust_matches_python_along_the_greedy_walk`).
They used to be four tests, each re-simulating the same seed's run, and the
simulation is the whole cost: on one seed, `env.step` took 72.5 s of an 80 s
walk; the net, the Rust obs builder and the Rust agent together took under
7 s (GH #1375).
"""

from __future__ import annotations

import json
import os
from pathlib import Path

import numpy as np
import pytest
import sts2_rs
from ppo.env import PpoRunEnv, Terminal
from ppo.net_policy import NetPolicy

CKPT = "data/ppo/ppo_fullrun_it350.pt"
ONNX = Path("data/ppo/ppo_fullrun_it350.onnx")
# `PARITY_SEEDS` caps both sweeps (the knob tests/test_ppo_env_parity.py reads
# too). Unset is the full sweep; the per-PR gate (`just test-slow-pr`) sets a
# smaller one, because every seed checks the same invariant.
_MAX_SEEDS = int(os.environ.get("PARITY_SEEDS", "20"))
A2_SEEDS = list(range(min(12, _MAX_SEEDS)))
C_SEEDS = list(range(min(20, _MAX_SEEDS)))
# Fixed sets, not capped by PARITY_SEEDS (as before the merge).
WITH_ACTIONS_SEEDS = [0, 1, 2]
WORKBENCH_SEEDS = [0, 3, 7]
WALK_SEEDS = sorted({*A2_SEEDS, *C_SEEDS, *WITH_ACTIONS_SEEDS, *WORKBENCH_SEEDS})

# Every walk is a full PPO run per seed (the env's combat search is the cost),
# so this file is still the largest share of the slow suite. Gated behind
# `slow` (deselected by the default addopts); run it with `just test-slow` /
# `pytest -m slow`.
pytestmark = [
    pytest.mark.skipif(not ONNX.exists(), reason="no ONNX snapshot"),
    pytest.mark.slow,
]


@pytest.fixture(autouse=True)
def _env(monkeypatch):
    # Policy-mode `PpoRunEnv` drives combat with the combat-only
    # `MctsCombatAgent` (GH #220) — no net files or env vars needed.
    monkeypatch.delenv("STS2_RUNAI_VALUE", raising=False)
    # These gates assert Rust argmax == numpy argmax over the RAW
    # per-candidate probs. The Rust side's group-mass decode (A8 audit
    # §8, default ON) deliberately diverges from that on split-mass
    # screens — e.g. pooled ShopRemoveCard mass beating the single best
    # buy — so parity runs pin the plain per-candidate argmax. The
    # grouped decode itself is covered by Rust-side unit tests
    # (sts2-runai ppo::policy group_decode_tests).
    monkeypatch.setenv("STS2_RUNAI_PPO_GROUP_DECODE", "0")


def _assert_near_tie(gate, seed, step, probs, rs_idx, py_idx):
    """The argmax may differ only at a genuine near-exact tie.

    Candidate lists and per-option probabilities agree (asserted by the
    callers); the ONLY way the argmax itself can differ is a near-exact tie
    between two candidates (e.g. two identical duplicate-card options at a
    smith/reward pick, which the net scores equally). tract (Rust) and torch
    (Python) resolve such ties differently because their floating-point
    reduction order isn't bit-identical — inherent to comparing two
    independent numerical runtimes at an exact tie, not a masking/order bug
    (verified by hand for seeds 4/6: candidate lists matched exactly, and the
    "wrong" index's probability differs from the "right" one by ~1e-7).
    1e-5 is ~100x the measured tie gap yet 100x tighter than the blanket
    |Δπ| < 1e-3 tolerance, so a quiet ranking regression in the 1e-4..1e-3
    band still fails loudly instead of hiding in the tie carve-out.
    """
    assert abs(probs[rs_idx] - probs[py_idx]) < 1e-5, (
        f"{gate}: argmax drift @ seed {seed} step {step}: rust picked "
        f"{rs_idx} ({probs[rs_idx]!r}), python picked {py_idx} "
        f"({probs[py_idx]!r}) — not a near-tie"
    )


@pytest.mark.parametrize("seed", WALK_SEEDS)
def test_rust_matches_python_along_the_greedy_walk(seed):
    """One Python-greedy walk per seed, checking at every decision:

    - A2 (`A2_SEEDS`): `sts2_rs.ppo_observe_json` == `ppo.env.observe`.
    - C (`C_SEEDS`): `PpoRunAgent.evaluate` matches `NetPolicy.distribution`
      (same argmax up to a near-tie; |Δπ|, |ΔV| < 1e-3).
    - with-actions (`WITH_ACTIONS_SEEDS`): `evaluate_with_actions` returns a
      masked action list aligned with probs, and its (probs, value, argmax)
      equal `evaluate()`'s.
    - workbench (`WORKBENCH_SEEDS`): `workbench.ppo_policy.evaluate`
      (impl='rust') agrees with impl='python' on argmax, per-option prob,
      surface and the resolved candidate labels.

    The with-actions check used to walk the Rust argmax rather than the
    Python one; the two differ only at the near-ties C allows, so the states
    it sees are the same.
    """
    # Policy-mode walk (reset/step), NOT expert_run(): expert mode yields the
    # Decision AFTER the expert has already acted (env.py:287-301), so env.rm
    # would no longer be at the observed decision state.
    a2, c = seed in A2_SEEDS, seed in C_SEEDS
    with_actions, workbench = seed in WITH_ACTIONS_SEEDS, seed in WORKBENCH_SEEDS
    os.environ.setdefault("STS2_RUNAI_PPO", str(ONNX.resolve()))
    from workbench import ppo_policy

    ppo_policy._reset_for_tests()
    env = PpoRunEnv(seed=seed)
    pol = NetPolicy(CKPT)
    agent = sts2_rs.PpoRunAgent(seed=seed) if (c or with_actions) else None
    out = env.reset()
    checked = {"C": 0, "with-actions": 0, "workbench": 0}
    while not isinstance(out, Terminal):
        step = out.step_index
        py_probs, py_value = pol.distribution(out.obs, out.actions, out.metrics)
        py_idx = int(np.argmax(py_probs))

        if a2:
            rust = json.loads(sts2_rs.ppo_observe_json(env.rm, out.phase, out.surface, seed, 0))
            # `out.obs` is a native Python dict straight from the PyO3 getters
            # (e.g. `deck` is a list of *tuples*, not lists — JSON has no tuple
            # type), so a bare `==` against `rust` (which came out of
            # `json.loads`) would spuriously fail on every nested sequence even
            # when every value matches. Round-tripping `out.obs` through
            # `json.dumps`/`json.loads` normalizes it to the same tuple-free
            # shape `rust` already has, without relaxing any VALUE comparison —
            # `obs.rs`'s own `round_trips_every_fixture_obs_row` test uses the
            # same `serde_json::Value`-shaped equality for the identical reason.
            py_obs = json.loads(json.dumps(out.obs))
            assert rust == py_obs, f"A2: obs drift @ seed {seed} step {step}"

        base = agent.evaluate(env.rm) if agent is not None else None
        if c:
            assert base is not None, f"C: rust declined a multi-option decision @ seed {seed}"
            rs_probs, rs_value, rs_idx = base
            if rs_idx != py_idx:
                _assert_near_tie("C", seed, step, rs_probs, rs_idx, py_idx)
            assert max(abs(a - b) for a, b in zip(rs_probs, py_probs)) < 1e-3, f"C @ step {step}"
            assert abs(rs_value - py_value) < 1e-3, f"C: value drift @ seed {seed} step {step}"
            checked["C"] += 1

        if with_actions:
            assert agent is not None
            wa = agent.evaluate_with_actions(env.rm)
            # Both decline (single-option) or both engage, in lockstep.
            assert (wa is None) == (base is None), f"with-actions @ step {step}"
            if wa is not None and base is not None:
                actions, probs, value, argmax = wa
                assert len(actions) == len(probs)
                assert 0 <= argmax < len(actions)
                b_probs, b_value, b_argmax = base
                assert probs == pytest.approx(b_probs, abs=0.0)
                assert value == pytest.approx(b_value, abs=0.0)
                assert argmax == b_argmax
                checked["with-actions"] += 1

        if workbench:
            py = ppo_policy.evaluate(env.rm, impl="python")
            rs = ppo_policy.evaluate(env.rm, impl="rust")
            assert (py is None) == (rs is None), f"workbench @ step {step}"
            if py is not None and py.probs is not None and rs is not None:
                assert rs.probs is not None
                assert rs.surface == py.surface
                assert [a["action"] for a in rs.actions] == [a["action"] for a in py.actions]
                assert max(abs(a - b) for a, b in zip(rs.probs, py.probs)) < 1e-3
                if rs.chosen != py.chosen:
                    _assert_near_tie("workbench", seed, step, rs.probs, rs.chosen, py.chosen)
                checked["workbench"] += 1

        out = env.step(py_idx)
    ppo_policy._reset_for_tests()
    for gate, on in (("C", c), ("with-actions", with_actions), ("workbench", workbench)):
        assert not on or checked[gate] > 0, f"{gate}: no decision checked on seed {seed}"


# Seed 3 is `soak`: its run is twice as long as seed 0's (measured with the
# reference walk, since dropped), and seed 0 already walks the benchmark path per PR.
@pytest.mark.parametrize("seed", [0, pytest.param(3, marks=pytest.mark.soak)])
def test_ppo_brain_benchmark_run_completes(seed, tmp_path):
    """The workbench run-mode stepper with brain=ppo + Benchmark budget runs a
    PPO 'as trained' session to completion without error.

    This does not assert per-decision trajectory equality against a
    PpoRunEnv rollout on the same seed; that per-decision PPO-net equality is
    gated separately by the workbench check of
    ``test_rust_matches_python_along_the_greedy_walk``. (It used to simulate
    that rollout anyway, at 1000/4000 sims, and assert only that it had
    decisions: half the test's time for a check the walk makes. GH #1375.)"""
    os.environ.setdefault("STS2_RUNAI_PPO", str(ONNX.resolve()))
    from workbench import branching, ppo_policy

    ppo_policy._reset_for_tests()
    # Workbench stepper: brain=ppo (rust ONNX), Benchmark budget.
    sess = branching.BranchingLiveSession(str(tmp_path), backend="rust")
    sess.start_run_mode(
        "Ironclad",
        seed,
        off_combat_brain="ppo",
        ppo_impl="rust",
        combat_sims=1000,
        combat_boss_sims=4000,
    )
    for _ in range(20_000):
        step = sess.run_mode_step()
        assert step["error"] is None
        if step["done"] or step["over"]:
            break
    ppo_policy._reset_for_tests()
    # The run reaches a terminal state without error under the PPO brain.
    assert step["over"] or step["done"]
