"""The serving guard (`Learner._serve_bad`): a round with a non-finite or all-masked
row is dumped to the save dir and raises, instead of a nameless device assert."""

from __future__ import annotations

import pickle
import types
from pathlib import Path

import numpy as np
import pytest
import torch
from ppo.e2e.actors import META_COLS
from ppo.e2e.learner import Learner


def test_serve_bad_dumps_the_round_and_raises(tmp_path: Path) -> None:
    beh = torch.tensor([[0.0, -torch.inf], [-torch.inf, -torch.inf], [float("nan"), 0.0]])
    bad = Learner._bad_rows(beh).numpy()
    assert bad.tolist() == [False, True, True]
    fake = types.SimpleNamespace(
        algo=types.SimpleNamespace(clocks=types.SimpleNamespace(it=42)),
        segs={7: types.SimpleNamespace(kind="run")},
        sd=types.SimpleNamespace(p=lambda rel: tmp_path / rel),
    )
    ch = types.SimpleNamespace(rb={"n_tok": np.array([5, 5, 5])}, it=9)
    metas = np.zeros((3, len(META_COLS)), np.int64)
    metas[:, META_COLS.index("uid")] = 7
    with pytest.raises(RuntimeError, match="2 rows with non-finite or all-masked"):
        Learner._serve_bad(fake, ch, metas, bad, beh, beh, np.zeros(3, np.int64), [2])  # type: ignore[arg-type]
    (dump,) = tmp_path.glob("serve_bad_it42_*.pkl")
    d = pickle.loads(dump.read_bytes())
    assert [r["i"] for r in d["bad_rows"]] == [1, 2]
    assert d["bad_rows"][1]["teacher_acted"] and d["bad_rows"][0]["seg_kind"] == "run"
    assert d["beh"].shape == (2, 2) and "n_tok" in d["chunk_rb"]
