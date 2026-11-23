"""`experiments/snapshot-pool/card_probe.py` and its analysis: the honest
card-reward headroom probe (common re-deals per option, held-out scoring)."""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

import pytest
import sts2_client
from ppo.e2e import factory

try:
    import sts2_rs

    HAVE_ENGINE = hasattr(sts2_rs.RunManager, "redeal_hidden_rooms")
except ImportError:  # pragma: no cover - depends on the venv
    HAVE_ENGINE = False

needs_engine = pytest.mark.skipif(not HAVE_ENGINE, reason="sts2_rs wheel lacks redeal_hidden_rooms")
EXP = Path(__file__).resolve().parents[2] / "experiments" / "snapshot-pool"


def _load(name: str):
    spec = importlib.util.spec_from_file_location(name, EXP / f"{name}.py")
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod  # dataclasses resolve their module by name
    spec.loader.exec_module(mod)
    return mod


# The public export leaves experiments/ out (GH #1531). The scripts load at
# import, so a `_NEEDS` marker in conftest would come too late: skip here.
if not (EXP / "analyze_card_probe.py").is_file() or not (EXP / "card_probe.py").is_file():
    pytest.skip("experiments/snapshot-pool is not exported", allow_module_level=True)

A = _load("analyze_card_probe")
P = _load("card_probe")


def _rows(vals, *, seed=1, nth=1, pick=0, keys=None):
    """Probe-shaped rows from `vals[r][opt]`."""
    n_opt = len(vals[0])
    keys = keys or [f"k{o}" for o in range(n_opt)]
    return [
        {
            "seed": seed,
            "nth": nth,
            "r": r,
            "opt": o,
            "n_opt": n_opt,
            "keys": keys,
            "net_pick": pick,
            "net_probs": [1 / n_opt] * n_opt,
            "G": v,
        }
        for r, row in enumerate(vals)
        for o, v in enumerate(row)
    ]


def test_held_out_gain_finds_a_better_option():
    """Option 1 beats the pick by 0.2 on every re-deal: the gain is 0.2, held out."""
    vals = [[0.3 + 0.01 * r, 0.5 + 0.01 * r, 0.1] for r in range(8)]
    (c,), problems = A.load(_rows(vals))
    assert not problems
    gain, switched = A.held_out_gain(c)
    assert gain == pytest.approx(0.2)
    assert switched


def test_held_out_gain_has_no_winners_curse():
    """Pure noise that favours a different option on each half scores negative
    or zero, never the in-sample gap."""
    vals = [[0.5, 1.0, 0.0], [0.5, 0.0, 1.0]] * 4  # opt 1 wins evens, opt 2 odds
    (c,), _ = A.load(_rows(vals))
    gain, _ = A.held_out_gain(c)
    assert gain == pytest.approx(-0.5)
    s = A.summarize([c], min_rollouts=8)
    assert s["in_sample_best_minus_pick_pp"] == pytest.approx(0.0)


def test_incomplete_rollouts_are_dropped_and_mismatch_flagged():
    rows = _rows([[0.1, 0.2]] * 6)
    rows = [x for x in rows if not (x["r"] == 5 and x["opt"] == 1)]
    rows.append({**rows[0], "r": 9, "keys": ["other", "k1"]})
    (c,), problems = A.load(rows)
    assert A.complete(c) == [0, 1, 2, 3, 4]
    assert problems


def test_clustered_se_averages_within_seed_first():
    m, se, n = A.clustered([(1, 0.0), (1, 1.0), (2, 1.0)])
    assert (m, n) == (0.75, 2)
    assert se == pytest.approx(0.25)


def test_horizon_and_reseed_helpers():
    assert P.parse_horizon("act") == "act"
    assert P.parse_horizon("3") == 3
    with pytest.raises(ValueError):
        P.parse_horizon("0")
    assert P.reseed_for(5, 1, 0) != P.reseed_for(5, 1, 1) != P.reseed_for(5, 2, 0)
    assert 0 <= P.reseed_for(2**31 - 1, 99, 10**6) < 2**31


@pytest.fixture(scope="module")
def tiny_ckpt(tmp_path_factory):
    import torch
    from ppo.e2e import net as N

    torch.set_num_threads(2)
    cfg = N.NetConfig(num_hidden=64, dim=64, n_layers=2, ffn=176, head_hidden=64)
    path = tmp_path_factory.mktemp("cardprobe") / "tiny.pt"
    N.save_checkpoint(path, N.E2ENet(cfg, seed=0).eval())
    return str(path)


@needs_engine
def test_probe_task_shares_redeals_and_is_deterministic(tiny_ckpt, monkeypatch):
    """A task probes the first card reward with every option on the same
    re-deal; the same task twice gives the same rows, and the analysis reads them."""
    monkeypatch.setattr(sts2_client, "GAME_VERSION", factory.GAME_VERSION)
    seed = None
    for s in range(162_000_000, 162_000_040):  # a random net may die before any reward
        rows = P.task((s, 0, 2, tiny_ckpt, 2, (1,)))
        if any("opt" in x for x in rows):
            seed = s
            break
    assert seed is not None, "no seed reached a card reward"
    again = P.task((seed, 0, 2, tiny_ckpt, 2, (1,)))
    strip = lambda xs: [{k: v for k, v in x.items() if k != "sec"} for x in xs]  # noqa: E731
    assert strip(rows) == strip(again)
    opts = [x for x in rows if "opt" in x]
    assert {x["r"] for x in opts} == {0, 1}
    assert all(x["G"] is not None for x in opts), [x["end"] for x in opts]
    n_opt = opts[0]["n_opt"]
    assert n_opt >= 2 and len(opts) == 2 * n_opt
    assert len(opts[0]["net_probs"]) == n_opt
    assert sum(opts[0]["net_probs"]) == pytest.approx(1.0, abs=1e-3)
    (c,), problems = A.load(json.loads(json.dumps(x)) for x in rows)
    assert not problems and A.complete(c) == [0, 1]


@needs_engine
def test_cut_rollout_bootstraps_with_the_privileged_run_value(tiny_ckpt, monkeypatch):
    """A rollout cut at its first OOC decision returns reward so far plus the
    critic's `v_run` on that decision's privileged encoding, and is repeatable."""
    import math

    monkeypatch.setattr(sts2_client, "GAME_VERSION", factory.GAME_VERSION)
    pol = P._policy(tiny_ckpt)
    for seed in range(162_000_000, 162_000_040):
        choices, _ = P.find_choices(seed, pol, {1})
        if choices:
            break
    (ch,) = choices
    # horizon 1 from one floor below the choice: stop at the first OOC decision after it
    args = (pol, seed, ch["snap"], ch["actions"][0], P.reseed_for(seed, 1, 0), 1)
    a = P.rollout(*args, ch["act"], ch["floor"] - 1)
    b = P.rollout(*args, ch["act"], ch["floor"] - 1)
    assert a == b
    # a map move always follows a card reward before the next fight
    assert a["end"] == "cut", a
    assert a["won"] is None and math.isfinite(a["v_leaf"])
    assert 0.0 <= a["G"] - a["v_leaf"] < 0.01  # at most a floor's reward before the cut
    from ppo.e2e.env import E2EEnv

    pol.begin_run(seed)
    d = E2EEnv(seed).reset()  # Neow: an OOC decision
    assert d.kind == "ooc"
    assert math.isfinite(P.priv_value(pol, d))
