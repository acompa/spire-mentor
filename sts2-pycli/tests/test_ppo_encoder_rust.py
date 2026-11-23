"""GH #1426 step 2: `ppo.encoder.featurize_row` runs the Rust encoder.

Training, the arena and the advisor share that one implementation, so the
net inputs must be bit-identical to the Python reference
(`featurize_row_py`) it replaced: every array, `aux_room` (the trainer's
aux-head label, which the Rust parity fixture does not carry) included.
Checked on the committed encoder fixtures (every feature schema they were
recorded at) and on fresh env decisions, with `metrics` passed separately
the way the collectors pass it.
"""

from __future__ import annotations

import json
import random
from pathlib import Path

import numpy as np
import pytest
import sts2_rs
from ppo import encoder, schema
from ppo.env import Decision, PpoRunEnv

FIXTURES = Path(__file__).resolve().parents[2] / "sts2-rs/crates/sts2-runai/tests/fixtures"
ARRAYS = ("obs_cat", "obs_cont", "obs_sec", "act_cat", "act_cont", "act_sec", "aux_room")
ALL_FEATURES = frozenset(schema.KNOWN_FEATURES)


def _assert_identical(obs, actions, metrics, features) -> None:
    want = encoder.featurize_row_py(obs, actions, metrics, features)
    got = encoder.featurize_row(obs, actions, metrics, features)
    assert got.n_actions == want.n_actions
    for name in ARRAYS:
        w, g = getattr(want, name), getattr(got, name)
        assert g.dtype == w.dtype, name
        assert g.shape == w.shape or (g.size == 0 and w.size == 0), name
        # Bytes, not values: a float that rounds differently is a different input.
        assert g.tobytes() == w.tobytes(), name


def _fixture_rows():
    for path in sorted(FIXTURES.glob("ppo_encoder_parity*.jsonl")):
        with path.open(encoding="utf-8") as f:
            for line in f:
                yield path.name, json.loads(line)


def test_fixture_rows_featurize_identically() -> None:
    n = 0
    for name, row in _fixture_rows():
        features = schema.parse_features(",".join(row.get("features") or []))
        # A fixture row's `obs` already carries the metrics (`env.observe`).
        _assert_identical(row["obs"], row["actions"], row["obs"], features)
        _assert_identical(row["obs"], row["actions"], row["obs"], ALL_FEATURES)
        n += 1
    assert n > 500, f"only {n} fixture rows under {FIXTURES}"


@pytest.mark.parametrize("character", ["Ironclad", "Silent", "Defect", "Necrobinder", "Regent"])
def test_env_decisions_featurize_identically(character: str) -> None:
    rng = random.Random(1426)
    surfaces: set[str] = set()
    for seed in range(2):
        env = PpoRunEnv(seed=7000 + seed, ascension=10, character=character, sims=8, boss_sims=8)
        out = env.reset()
        while isinstance(out, Decision):
            surfaces.add(out.surface)
            for features in (frozenset(), ALL_FEATURES):
                _assert_identical(out.obs, out.actions, out.metrics, features)
            out = env.step(rng.randrange(len(out.actions)))
    assert {"path", "card-pick"} <= surfaces


def test_sparse_rows_read_missing_keys_as_python_does() -> None:
    """Hand-built rows (tests, `honest_obs`) omit most keys; `featurize_row_py`
    reads each with `.get(k) or <default>`, and so must the Rust binding."""
    _assert_identical({}, [], {}, frozenset())
    _assert_identical(
        {"deck": [["Strike", False]], "max_hp": None}, [{"action": "skip"}], {}, ALL_FEATURES
    )
    _assert_identical({"hp": 70}, [{"action": "rest"}], {"hp": 33, "gold": None}, frozenset())
    _assert_identical(
        {}, [{"kind": "skip"}, {"action": None}, {"action": "no_such_kind"}], {}, frozenset()
    )
    _assert_identical({"map": {}}, [{"action": "move", "col": 1}], {}, frozenset())
    boss_only = {"map": {"boss": {"col": 2, "row": 15, "id": "KAISER_CRAB_BOSS"}, "nodes": []}}
    _assert_identical(boss_only, [{"action": "move", "col": 2}], {}, frozenset())


def test_arrays_are_writable() -> None:
    feat = encoder.featurize_row({}, [{"action": "skip"}], {})
    feat.act_cat[0, 0] = 0  # collate and padding helpers write in place


def test_unknown_feature_raises() -> None:
    with pytest.raises(ValueError, match="unknown STS2_PPO_FEATURES"):
        sts2_rs.ppo_featurize({}, [], {}, "no_such_feature")


def test_frombuffer_layout_matches_reference_dtypes() -> None:
    feat = encoder.featurize_row({}, [], {})
    assert feat.obs_cat.dtype == np.int64 and feat.obs_cont.dtype == np.float32
