"""`ppo.e2e.solve`: the sampling seed solver and its replayable proofs."""

from __future__ import annotations

import json

import pytest
import sts2_client
from ppo.e2e import factory
from ppo.e2e import solve as S

try:
    import sts2_rs  # noqa: F401

    HAVE_ENGINE = True
except ImportError:  # pragma: no cover - depends on the venv
    HAVE_ENGINE = False

needs_engine = pytest.mark.skipif(not HAVE_ENGINE, reason="sts2_rs wheel not importable")
SMOKE = 162_000_000


@pytest.fixture(autouse=True)
def _campaign_process():
    with pytest.MonkeyPatch.context() as mp:
        mp.setattr(sts2_client, "GAME_VERSION", factory.GAME_VERSION)
        yield


@pytest.fixture(scope="module")
def tiny_ckpt(tmp_path_factory):
    """A small random-init student: it loses fast, which keeps runs cheap."""
    import torch
    from ppo.e2e import net as N

    torch.set_num_threads(2)
    cfg = N.NetConfig(num_hidden=64, dim=64, n_layers=2, ffn=176, head_hidden=64)
    path = tmp_path_factory.mktemp("solve") / "tiny.pt"
    N.save_checkpoint(path, N.E2ENet(cfg, seed=0).eval())
    return str(path)


def test_parse_seed():
    assert S.parse_seed("123").value == 123
    assert S.parse_seed(" 7 ").value == 7
    assert S.parse_seed("BK9KTUB5Z1").value == "BK9KTUB5Z1"
    assert S.parse_seed("123", game_seed=True).value == "123"
    with pytest.raises(ValueError):
        S.parse_seed(str(2**31))
    with pytest.raises(ValueError):
        S.parse_seed("  ")
    for spec in (S.SeedSpec(5), S.SeedSpec("ABC")):
        assert S.SeedSpec.from_json(spec.to_json()) == spec


def test_attempt_plan_greedy_first_then_cycles():
    plan = [S.attempt_plan(k, 2, [1.0, 0.5], True) for k in range(7)]
    assert plan == [(0, 0.0), (1, 0.0), (0, 1.0), (1, 1.0), (0, 0.5), (1, 0.5), (0, 1.0)]
    assert [S.attempt_plan(k, 1, [0.7], False) for k in range(3)] == [(0, 0.7)] * 3


def test_upper_bound_is_about_three_over_n():
    assert S.upper_bound(0) == pytest.approx(1.0)
    assert S.upper_bound(1) == pytest.approx(0.95)
    assert S.upper_bound(1000) == pytest.approx(3 / 1000, rel=0.01)


def test_stream_seed_distinct_per_attempt():
    assert len({S.stream_seed(SMOKE, k) for k in range(1000)}) == 1000
    assert S.stream_seed(SMOKE, 3) != S.stream_seed(SMOKE + 1, 3)


@needs_engine
def test_attempt_deterministic_and_log_replays(tiny_ckpt):
    """An attempt is a function of (seed, k, ckpt, T), and its action log replays
    with no net to the same terminal; a lost log is not a proof."""
    from ppo.e2e.env import E2EDecision

    spec = S.SeedSpec(SMOKE)
    a = S.play_attempt(spec, 3, tiny_ckpt, 1.0)
    b = S.play_attempt(spec, 3, tiny_ckpt, 1.0)
    assert a["outcome"] != "error", a
    assert (a["outcome"], a["floor"], a["rows"]) == (b["outcome"], b["floor"], b["rows"])

    s = S._sampler(tiny_ckpt)
    env = S.make_env(spec)
    s.begin(env.seed, 3, 1.0)
    d = env.reset()
    while isinstance(d, E2EDecision) and not d.bootstrap:
        d = env.step(s.act(d))
    t = env.terminal
    term, steps = S.replay(spec, list(t.log_idx), readable=True)
    assert (term.outcome, term.fingerprint) == (t.outcome, t.fingerprint)
    assert len(steps) == len(t.log_idx) and all("action" in x for x in steps)
    ok, why = S.check_proof(
        {"seed": spec.to_json(), "log_idx": list(t.log_idx), "fingerprint": t.fingerprint}
    )
    assert ok == t.won and (ok or "ended" in why)
    ok, why = S.check_proof(
        {"seed": spec.to_json(), "log_idx": list(t.log_idx)[:-1], "fingerprint": t.fingerprint}
    )
    assert not ok and "diverged" in why


@needs_engine
def test_game_seed_string_builds_a_campaign_run():
    rm = S.make_run(S.SeedSpec("SOLVETEST1"))
    rm2 = S.make_run(S.SeedSpec("SOLVETEST1"))
    assert int(rm.seed) == int(rm2.seed)
    assert int(rm.seed) != int(S.make_run(S.SeedSpec("SOLVETEST2")).seed)


@needs_engine
def test_cli_unsolved_reports_bound_and_verify_refuses(tiny_ckpt, tmp_path, capsys):
    rc = S.main(
        [
            "solve",
            str(SMOKE),
            "--ckpt",
            tiny_ckpt,
            "--samples",
            "2",
            "--procs",
            "1",
            "--out",
            str(tmp_path),
        ]
    )
    assert rc == 0
    res = json.loads((tmp_path / f"{SMOKE}.json").read_text(encoding="utf-8"))
    assert res["verdict"] == "unsolved", res
    assert res["attempts"] == 2 and res["wins"] == 0
    assert res["win_chance_upper_95"] == pytest.approx(1 - 0.05**0.5, abs=1e-6)
    assert res["config"]["temps"] == [1.0] and res["engine"]["game_version"] == "v0.111.0"
    assert "unsolved" in capsys.readouterr().out
    assert S.main(["verify", str(tmp_path / f"{SMOKE}.json")]) == 2


@needs_engine
def test_redeal_hidden_rooms_keeps_the_seen_and_rerolls_the_hidden():
    """`RunManager.redeal_hidden_rooms` (path probe): fought rooms and the visible
    act boss stay; the unfought tail and later acts follow the reseed."""
    from ppo.e2e.env import E2EDecision, E2EEnv

    env = E2EEnv(160_000_000)
    d = env.reset()
    while isinstance(d, E2EDecision) and d.floor < 4:
        d = env.step(0)
    rm = env.rm
    fought = env.fights  # act-1 monster fights so far (no elite before floor 6)
    assert fought >= 1
    base = rm.generated_rooms()

    def redealt(s):
        c = rm.clone()
        c.reseed_run(s)
        c.redeal_hidden_rooms()
        return c.generated_rooms()

    a, a2, b = redealt(12345), redealt(12345), redealt(999)
    assert a == a2
    assert a[0]["boss"] == base[0]["boss"]
    assert a[0]["normal"][:fought] == base[0]["normal"][:fought]
    assert a[0]["normal"] != b[0]["normal"] or a[1] != b[1]
    assert a[1] != base[1] and a[2] != base[2]
