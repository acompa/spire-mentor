"""Serving-time distillation teacher (GH #1197, `ScheduleCfg.distill_source` "acting").

A teacher net trained without the "chars" fill event (the 791 Ironclad net) must read
chars-off rows, but an all-character lane's Ironclad rows carry the event, and on a
combat row it rewrites existing inputs (live damage previews, tactical columns), so
the stored rows cannot be turned back. Instead each worker runs a chars-off shadow
env beside every distill-character segment (`rsenv.RsE2EEnv`, `RsDecision.tok_t`)
and sends its rows with the round (`extra["distill"]`). The learner hands them here
after serving the round: `DistillScorer` buffers them, scores them with the frozen
teacher in packed batches and writes the probabilities into the stored rows' own
`tprob` (with `t_src` 1 on combat rows, 2 out of combat; the cut reads `has_t` from `t_src`), where the
update reads them (`update.Rank`, "acting"). Nothing waits on the teacher to act; the
learner flushes before a collection ends, because the cut copies the rows next.
"""

from __future__ import annotations

import time
from collections import Counter
from typing import Any

import numpy as np
import torch

from . import contracts as C
from .net import E2ENet, to_packed
from .rowstore import Chunk, concat, take

#: Option kinds whose teacher mass the scorer counts: POTION (the reference),
#: POTION_TARGET (GH #1604 made every AnyEnemy drink one) and the potion popup's
#: USE_POTION_OOC and DISCARD_POTION (GH #1606), which the teacher never trained on.
_NEW_KINDS: dict[str, tuple[int, ...]] = {"k4": (4,), "k5": (5,), "ooc": (54, 55)}
#: The kinds whose teacher mass the target drops (the popup's: GH #1606).
_MASK_KINDS: tuple[int, ...] = (54, 55)


class DistillScorer:
    """Buffer the shadow rows of served rows and write the teacher's class probs into them.

    It never fails the lane: a payload that is not the round's (keys, indices, run
    uid, t, classes) is dropped and counted, and a teacher forward that raises (a CUDA
    OOM included) or returns non-finite probs leaves its rows without a teacher
    (`score_errors`, `nonfinite`); the update counts them (`distill_rows_missing`)."""

    #: The row-batch fields `add` checks and the forward reads.
    NEED = ("n_tok", "n_classes", "run_uid", "t", "cat", "cont", "sec")
    #: `C.validate_row_batch` is a per-row Python loop (63% of the learner at 80%
    #: Ironclad on the a4000), so it runs on the first payload and then one in this
    #: many; the shape, index and decision checks below run on every payload.
    VALIDATE_EVERY = 64

    def __init__(self, net: E2ENet, device: torch.device | str, flush_rows: int) -> None:
        self.net = net
        self.dev = torch.device(device)
        self.flush_rows = int(flush_rows)
        #: The buffered rows' (round, row in it), in buffer order.
        self._rows: list[tuple[Chunk, int]] = []
        self._rbs: list[dict[str, np.ndarray]] = []
        self.pending = 0
        self.stats: Counter = Counter()
        self._payloads = 0  # payloads past the shape checks (`VALIDATE_EVERY`)

    def add(self, ch: Chunk, base: int, n: int, payload: Any) -> None:
        """Queue a worker message's shadow payload (`extra["distill"]`: `idx`, the
        message's row indices, and `rb`, their shadow rows) for the served rows
        `base:base + n` of the round `ch`. A payload that does not fit is dropped and
        counted (`bad_payload`); a shadow row that is not its row's decision (run uid,
        t, classes, or a bootstrap row) is dropped and counted (`mismatch`). Those
        rows then train without a teacher."""
        try:
            idx = np.asarray(payload["idx"])
            rb = payload["rb"]
            k = len(idx)
            ok_shape = (
                idx.ndim == 1
                and (k == 0 or idx.dtype.kind in "iu")
                and isinstance(rb, dict)
                and all(f in rb for f in self.NEED)
                and all(len(rb[f]) == k for f in ("n_tok", "n_classes", "run_uid", "t"))
                and (k == 0 or (int(idx.min()) >= 0 and int(idx.max()) < n))
                and len(np.unique(idx)) == k
            )
            if ok_shape and k:
                if self._payloads % self.VALIDATE_EVERY == 0:
                    C.validate_row_batch(rb)
                self._payloads += 1
        except Exception:
            ok_shape = False
            k = 0
        if not ok_shape:
            self.stats["bad_payload"] += 1
            return
        if k == 0:
            return
        gidx = base + idx.astype(np.int64)
        ok = (
            (np.asarray(rb["run_uid"]) == ch.rb["run_uid"][gidx])
            & (np.asarray(rb["t"]) == ch.rb["t"][gidx])
            & (np.asarray(rb["n_classes"]) == ch.rb["n_classes"][gidx])
            & ~ch.rb["bootstrap"][gidx].astype(bool)
        )
        if not ok.all():
            self.stats["mismatch"] += int((~ok).sum())
            rb = take(Chunk(rb), np.flatnonzero(ok))
            gidx = gidx[ok]
        if not len(gidx):
            return
        self._rows.extend((ch, i) for i in gidx.tolist())
        self._rbs.append(rb)
        self.pending += len(gidx)

    def flush(self) -> int:
        """Score every buffered row, at most `flush_rows` per teacher forward, and write
        its probs; returns the rows scored. A forward that raises leaves its rows
        without a teacher and is counted (`score_errors`, `score_error_rows`)."""
        if not self.pending:
            return 0
        t0 = time.time()
        rows, rbs = self._rows, self._rbs
        self._rows, self._rbs, self.pending = [], [], 0
        done = 0
        try:
            rb_all = concat(rbs)
        except Exception as ex:
            self._score_error(ex, len(rows))
            self.stats["serve_s"] += time.time() - t0
            return 0
        whole = Chunk(rb_all)
        for lo in range(0, len(rows), self.flush_rows):
            part = np.arange(lo, min(lo + self.flush_rows, len(rows)))
            try:
                rb = rb_all if len(part) == len(rows) else take(whole, part)
                probs = teacher_probs(self.net, rb, self.dev)
            except Exception as ex:
                self._score_error(ex, len(part))
                continue
            nc = rb["n_classes"].astype(np.int64)
            koff = np.concatenate(([0], np.cumsum(nc)))
            kinds = np.asarray(rb["cls_kind"], np.int64)
            for j, k in enumerate(part):
                p = probs[j, : nc[j]]
                if not np.isfinite(p).all():
                    self.stats["nonfinite"] += 1
                    continue
                kj = kinds[koff[j] : koff[j + 1]]
                for name, ks in _NEW_KINDS.items():
                    m = np.isin(kj, ks)
                    if m.any():
                        self.stats[f"{name}_rows"] += 1
                        self.stats[f"{name}_mass"] += float(p[m].sum())
                # The teacher never trained on the potion popup: its mass there is
                # noise, and a zeroed target would teach "never". Drop those classes
                # and renormalize over the screen's own options.
                m = np.isin(kj, _MASK_KINDS)
                if m.any():
                    p = np.where(m, 0.0, p).astype(np.float32)
                    tot = float(p.sum())
                    if not tot > 0.0:
                        self.stats["masked_empty"] += 1
                        continue
                    p /= tot
                    self.stats["masked_rows"] += 1
                ch, i = rows[int(k)]
                ch.rb["tprob"][ch.span("cls", i)] = p
                ch.rb["t_src"][i] = 1 if ch.rb["is_combat"][i] else 2
                done += 1
        self.stats["rows"] += done
        self.stats["flushes"] += 1
        self.stats["serve_s"] += time.time() - t0
        return done

    def _score_error(self, ex: BaseException, n: int) -> None:
        self.stats["score_errors"] += 1
        self.stats["score_error_rows"] += n
        if self.stats["score_errors"] <= 3:
            print(f"[e2e] distill: teacher scoring failed on {n} rows: {ex!r}", flush=True)
        if isinstance(ex, torch.cuda.OutOfMemoryError):
            torch.cuda.empty_cache()

    def discard(self) -> int:
        """Drop the buffered rows and the counters (a collection that ends without a
        cut: its rows are never trained); returns the rows dropped."""
        n = self.pending
        self._rows, self._rbs, self.pending = [], [], 0
        self.stats = Counter()
        return n

    def take_stats(self) -> dict[str, Any]:
        out = {
            "rows_scored": int(self.stats["rows"]),
            "flushes": int(self.stats["flushes"]),
            "serve_s": float(self.stats["serve_s"]),
            "mismatch": int(self.stats["mismatch"]),
            "bad_payload": int(self.stats["bad_payload"]),
            "score_errors": int(self.stats["score_errors"]),
            "score_error_rows": int(self.stats["score_error_rows"]),
            "nonfinite": int(self.stats["nonfinite"]),
            "masked_rows": int(self.stats["masked_rows"]),
            "masked_empty": int(self.stats["masked_empty"]),
            # The teacher's mass on option kinds it never trained on (GH #1604/#1606).
            **{
                f"{name}_{x}": float(self.stats[f"{name}_{x}"])
                for name in _NEW_KINDS
                for x in ("rows", "mass")
            },
        }
        self.stats = Counter()
        return out


@torch.no_grad()
def teacher_probs(net: E2ENet, rb: dict[str, np.ndarray], device: torch.device | str) -> np.ndarray:
    """[B, A] float32: the teacher's softmax over each row's merged classes (zero past
    them), masked as the update's teacher term masks it (`update.distill_teacher_probs`),
    from one packed policy forward (the serving path: no value module, no PRIV)."""
    from .update import distill_teacher_probs

    dev = torch.device(device)
    lg = net.policy_packed(to_packed(rb, dev))
    n = int(lg.shape[0])
    nc = torch.as_tensor(rb["n_classes"].astype(np.int64), device=dev)
    width = int(nc.max()) if n else 0
    p = distill_teacher_probs(lg, nc, torch.ones(n, dtype=torch.bool, device=dev), width)
    return p.float().cpu().numpy()
