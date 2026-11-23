"""Learner-side distillation (GH #1197): a frozen teacher net on the distill characters'
trained rows, fed through `ppo_losses`' kickstarting term (`ScheduleCfg.distill_*`)."""

from __future__ import annotations

import dataclasses
import math
import shutil
from pathlib import Path
from typing import Any

import numpy as np
import pytest
import test_learner as TL  # make_cfg, make_learner, CharSource: the scripted lane
import torch
from ppo.e2e import algo as A
from ppo.e2e import learner as L
from ppo.e2e import ops_contract as OC
from ppo.e2e import schedules as S
from ppo.e2e import update as U
from ppo.e2e.net import file_sha256
from ppo.e2e.update import export_variant

# These drive the scripted learner lane, which is Linux-only (see test_learner.py).
pytestmark = pytest.mark.skipif(
    'sys.platform == "win32"',
    reason="the learner ships rows through POSIX shared memory; the lane runs on Linux only.",
)

_v111 = TL._v111  # the autouse fixture: the stamps need v0.111.0


# ── the config ───────────────────────────────────────────────────────────


def test_distill_cfg_defaults_off_and_validates() -> None:
    d = S.DEFAULT
    assert d.distill_ckpt is None and not d.distill_lambda and not d.distill_on
    assert d.distill_chars == ("Ironclad",)
    with pytest.raises(ValueError, match="distill_sha256"):
        S.ScheduleCfg(distill_ckpt="/x.pt")
    with pytest.raises(ValueError, match="distill_lambda"):
        S.ScheduleCfg(distill_lambda=-0.1)
    with pytest.raises(ValueError, match="distill_lambda"):
        S.ScheduleCfg(distill_lambda=math.nan)
    on = S.ScheduleCfg(distill_ckpt="/x.pt", distill_sha256="ab", distill_lambda=0.1)
    assert on.distill_on
    assert not S.ScheduleCfg(distill_ckpt="/x.pt", distill_sha256="ab").distill_on
    with pytest.raises(ValueError, match="distill_chars"):
        U.distill_char_ids(["Ironclad", "Watcher"])
    np.testing.assert_array_equal(U.distill_char_ids(["Ironclad", "Regent"]), [0, 3])


def test_distill_cfg_round_trips_and_an_older_state_loads_with_it_off() -> None:
    cfg = S.ScheduleCfg(
        distill_ckpt="/t.pt",
        distill_sha256="ab" * 32,
        distill_lambda=0.25,
        distill_chars=("Ironclad", "Silent"),
    )
    st = A.AlgoState(cfg, n_types=A.N_TYPES).to_dict()
    back = A.AlgoState.from_dict(st).cfg
    assert back == cfg and back.distill_chars == ("Ironclad", "Silent")
    # A c47d40201 state has no distill_* keys: they take their defaults (off).
    for k in [k for k in st["cfg"] if k.startswith("distill_")]:
        del st["cfg"][k]
    old = A.AlgoState.from_dict(st).cfg
    assert old.distill_ckpt is None and not old.distill_on


# ── the teacher probabilities and the loss ───────────────────────────────


def _rows(b: int, nc: torch.Tensor, ks: np.ndarray, seed: int) -> A.LossRows:
    g = np.random.default_rng(seed)
    return A.LossRows.from_numpy(
        n_classes=nc.numpy(),
        action=np.zeros(b, np.int64),
        lp_prox=np.full(b, -1.0),
        lp_behav=np.full(b, -1.0),
        adv=g.normal(size=b),
        target=g.normal(size=b),
        pi=np.ones(b, bool),
        v=np.ones(b, bool),
        beta=np.zeros(b),
        ks=ks,
        dtype=g.integers(0, 4, b),
    )


def _loss_and_grad(logits: torch.Tensor, rows: A.LossRows) -> tuple[Any, Any, Any]:
    lg = logits.clone().requires_grad_()
    out = A.ppo_losses(lg, torch.zeros(len(logits)), rows, n_types=4)
    out.loss.backward()
    return out.loss.detach(), lg.grad, out.stats


def test_teacher_probs_mask_like_the_student_and_other_rows_are_inert() -> None:
    """Selected rows get the teacher's softmax over their merged classes; the rest are NaN
    with ks 0, so whatever the teacher says about them changes neither loss nor gradient."""
    g = torch.Generator().manual_seed(3)
    b, w = 12, 7
    nc = torch.randint(2, w + 1, (b,), generator=g)
    logits = torch.randn(b, w, generator=g)
    t_full = torch.randn(b, w, generator=g)
    sel = torch.rand(b, generator=g) < 0.5
    sel[0], sel[1] = True, False
    lam = 0.3
    ks = np.where(sel.numpy(), lam, 0.0)
    tp = U.distill_teacher_probs(t_full[sel], nc, sel, w)
    assert torch.isnan(tp[~sel]).all()
    for i in torch.nonzero(sel).flatten().tolist():
        k = int(nc[i])
        np.testing.assert_allclose(tp[i, :k], torch.softmax(t_full[i, :k], 0), rtol=1e-6)
        assert not tp[i, k:].any()
    rows = _rows(b, nc, ks, 1)
    l1, g1, st = _loss_and_grad(logits, dataclasses.replace(rows, teacher=tp))
    # The term is lam * KL(pi_T || pi_S) on the selected rows, over the trained-row denom.
    want = 0.0
    for i in torch.nonzero(sel).flatten().tolist():
        k = int(nc[i])
        pt = torch.softmax(t_full[i, :k].double(), 0)
        ls = torch.log_softmax(logits[i, :k].double(), 0)
        want += lam * float((pt * (pt.log() - ls)).sum())
    assert float(st["ks_loss"]) == pytest.approx(want, rel=1e-5)
    assert int(st["n_ks"]) == int(sel.sum()) and int(st["n_ks_missing"]) == 0
    assert int(st["n_ks_t"].sum()) == int(sel.sum())
    assert float(st["ks_kl_t"].sum()) == pytest.approx(want / lam, rel=1e-5)
    # Another teacher for the unselected rows: identical loss and gradient.
    t2 = t_full.clone()
    t2[~sel] = 50 * torch.randn(int((~sel).sum()), w, generator=g)
    tp2 = U.distill_teacher_probs(t2[sel], nc, sel, w)
    assert torch.equal(torch.nan_to_num(tp2), torch.nan_to_num(tp))
    full2 = torch.where(sel[:, None], tp2, torch.softmax(t2, -1))  # even finite probs there
    l2, g2, _ = _loss_and_grad(logits, dataclasses.replace(rows, teacher=full2))
    assert torch.equal(l1, l2) and torch.equal(g1, g2)
    # Without the term (teacher None, ks 0) the result is the plain PPO loss.
    rows0 = dataclasses.replace(rows, ks=torch.zeros(b))
    l0, g0, _ = _loss_and_grad(logits, rows0)
    l0b, g0b, _ = _loss_and_grad(logits, dataclasses.replace(rows0, teacher=tp))
    assert torch.equal(l0, l0b) and torch.equal(g0, g0b)
    assert not torch.equal(l0, l1)


def test_teacher_probs_take_a_narrower_sub_batch_and_identical_logits_give_zero_kl() -> None:
    g = torch.Generator().manual_seed(5)
    b, w = 8, 9
    nc = torch.tensor([2, 3, 4, 5, 2, 3, 9, 4])
    sel = torch.tensor([True, True, False, True, False, True, False, True])
    logits = torch.randn(b, w, generator=g)
    # The teacher's own sub-batch pads to its widest selected row (5 classes).
    sub = logits[sel][:, :5]
    tp = U.distill_teacher_probs(sub, nc, sel, w)
    rows = _rows(b, nc, np.where(sel.numpy(), 1.0, 0.0), 2)
    _, _, st = _loss_and_grad(logits, dataclasses.replace(rows, teacher=tp))
    assert int(st["n_ks"]) == int(sel.sum())
    assert abs(float(st["ks_kl"])) < 1e-5


# ── the lane ─────────────────────────────────────────────────────────────

ITER = TL.ITER


def _base(tmp_path: Path) -> tuple[Path, Path, str]:
    """A 2-iteration Ironclad + Regent lane (V-warm only), its state and its raw weights
    exported as the teacher (and that file's sha256)."""
    src = tmp_path / "src"
    lr = _learner(TL.make_cfg(src, max_iters=2))
    assert lr.train() == "max_iters"
    teacher = tmp_path / "teacher.pt"
    sha = export_variant(str(src / "state.pt"), "raw", str(teacher))
    assert sha == file_sha256(teacher)
    return src, teacher, sha


def _learner(cfg: L.Cfg) -> L.Learner:
    """A teacherless learner (lambda_KS 0) on Ironclad + Regent scripted rows."""
    clock = TL.Clock()
    lr = L.Learner(cfg, source=lambda wcfg: TL.CharSource(wcfg), updater="local", clock=clock)
    clock.learner = lr
    return lr


def _resume(
    tmp_path: Path, src: Path, name: str, sched: dict[str, Any], max_iters: int = 7
) -> L.Learner:
    d = tmp_path / name
    shutil.copytree(src, d)
    (d / OC.DONE).unlink(missing_ok=True)
    lr = _learner(TL.make_cfg(d, max_iters=max_iters, resume=str(d / "state.pt"), sched=sched))
    assert lr.train() == "max_iters"
    return lr


def _weights(lr: L.Learner) -> dict[str, torch.Tensor]:
    return {k: v.detach().clone() for k, v in lr.updater.rank.net.state_dict().items()}


def test_lane_distills_ironclad_rows_only_and_off_is_bit_identical(tmp_path: Path) -> None:
    """Through V-warm and into U0 (the policy trains): distill off, distill at lambda 0
    and distill on a character with no rows (Silent) all end bit-identical; on Ironclad
    the weights move, the teacher ran on Ironclad rows only, and the stats carry it."""
    src, teacher, sha = _base(tmp_path)
    d = {"distill_ckpt": str(teacher), "distill_sha256": sha}
    off = _weights(_resume(tmp_path, src, "off", dict(TL.SCHED)))
    lam0 = _resume(tmp_path, src, "lam0", dict(TL.SCHED, **d, distill_lambda=0.0))
    assert lam0.updater.rank.distill is not None  # loaded (sha checked), never run
    silent = _resume(
        tmp_path, src, "silent", dict(TL.SCHED, **d, distill_lambda=0.5, distill_chars=["Silent"])
    )
    for arm in (lam0, silent):
        w = _weights(arm)
        assert all(torch.equal(w[k], off[k]) for k in off)
    assert all(r["update"]["distill_rows"] == 0 for r in silent.stats_rows)
    ic = _resume(tmp_path, src, "ic", dict(TL.SCHED, **d, distill_lambda=0.5))
    w = _weights(ic)
    assert any(not torch.equal(w[k], off[k]) for k in off)
    # The teacher is no part of the student: not in its params, state or EMA.
    rank = ic.updater.rank
    assert not any(p is q for p in rank.params for q in rank.distill.parameters())
    assert all(not p.requires_grad for p in rank.distill.parameters())
    assert not rank.distill.training
    on = [r for r in ic.stats_rows if r["update"]["distill_rows"] > 0]
    assert on, "U0 trains the policy: the teacher must have run"
    for r in on:
        u = r["update"]
        assert set(u["distill_kl_ct"]) == {"Ironclad"}
        assert (
            u["distill_kl_t"] and math.isfinite(u["distill_kl"]) and u["distill_kl"] >= -1e-6
        )  # teacher == student here: KL ~ 0, compiled-path rounding
        assert u["distill_loss"] == pytest.approx(0.5 * u["distill_kl"], rel=1e-5)
        # The term's share of the minibatch mean: the trained rows outnumber the distilled.
        assert -1e-6 <= u["distill_loss_per_row"] <= u["distill_loss"] + 1e-6
        assert u["distill_fwd_s"] >= 0 and u["n_ks_missing"] == 0
        assert r["losses"]["ks"] == u["ks_loss"]  # it is the teacher term's loss
    assert max(r["update"]["distill_kl"] for r in on) > 0  # the student drifts off it
    # V-warm (policy off) runs no teacher.
    assert all(
        r["update"]["distill_rows"] == 0 for r in ic.stats_rows if r["losses"]["policy"] is None
    )
    # A resume with no --sched keeps the saved distill settings.
    (tmp_path / "ic" / OC.DONE).unlink()
    st = torch.load(tmp_path / "ic" / "state.pt", map_location="cpu", weights_only=True)
    assert st["algo"]["cfg"]["distill_lambda"] == pytest.approx(0.5)
    assert "distill" not in str(list(st["net"]["state_dict"]))
    again = _learner(
        TL.make_cfg(
            tmp_path / "ic", max_iters=8, resume=str(tmp_path / "ic" / "state.pt"), sched={}
        )
    )
    assert again.algo.cfg.distill_ckpt == str(teacher)
    assert again.algo.cfg.distill_chars == ("Ironclad",) and again.algo.cfg.distill_on


def test_identical_teacher_gives_zero_kl(tmp_path: Path) -> None:
    """The teacher is the resumed state's own weights and the lr holds at 0: the student
    never moves, so the distillation KL is ~0 on every update that runs it."""
    src, teacher, sha = _base(tmp_path)
    sched = dict(
        TL.SCHED,
        lr_hold_decisions=10**9,
        distill_ckpt=str(teacher),
        distill_sha256=sha,
        distill_lambda=1.0,
    )
    lr = _resume(tmp_path, src, "same", sched)
    on = [r["update"] for r in lr.stats_rows if r["update"]["distill_rows"] > 0]
    assert on
    for u in on:
        assert abs(u["distill_kl"]) < 1e-5, u["distill_kl"]
        assert all(abs(v) < 1e-5 for v in u["distill_kl_t"].values())


def test_a_wrong_sha_refuses_before_the_lane_starts(tmp_path: Path) -> None:
    src, teacher, _ = _base(tmp_path)
    bad = "0" * 64
    with pytest.raises(ValueError, match="sha256"):
        U.check_distill_cfg(S.ScheduleCfg(distill_ckpt=str(teacher), distill_sha256=bad))
    d = tmp_path / "bad"
    shutil.copytree(src, d)
    (d / OC.DONE).unlink(missing_ok=True)
    sched = dict(TL.SCHED, distill_ckpt=str(teacher), distill_sha256=bad, distill_lambda=0.1)
    with pytest.raises(L.UsageError, match=r"distill.*sha256"):
        _learner(TL.make_cfg(d, max_iters=4, resume=str(d / "state.pt"), sched=sched))


def test_distill_and_the_stored_row_ks_teacher_refuse_to_run_together(tmp_path: Path) -> None:
    """The fake teacher pair puts lambda_KS at 1 from the start: the first update refuses."""
    _, teacher, sha = _base(tmp_path)
    sched = dict(TL.SCHED, distill_ckpt=str(teacher), distill_sha256=sha, distill_lambda=0.1)
    lr = TL.make_learner(TL.make_cfg(tmp_path / "both", max_iters=3, sched=sched))
    with pytest.raises(RuntimeError, match="both on"):
        lr.train()
