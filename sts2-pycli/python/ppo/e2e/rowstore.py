"""Row batches the learner keeps, ships to the update ranks and saves (WP4c).

Every row is a row of a `ppo.e2e.contracts` row batch (`ROW_FIELDS`,
SCHEMA.md section 9): per-row fields, then the ragged token, member, class,
PRIV and pool axes. A `Chunk` is one row batch with its axis offsets computed
once; the learner keeps one chunk per serving round and every stored row
points into one (`Row.chunk`, `Row.i`), so a chunk is freed once no stored,
held or in-flight row points into it. Once an iteration's joined table is in
its `SharedMemory` block, the learner points the shipped rows into that block
(a `ShmChunk`), so their rounds and the last update's held block go. Rank 0
ends the update by copying the rows its hold-back keeps out of the block into
a block of their own (`take_to_shm`, `update.Rank`); the learner maps it and
points those rows there, and the update's block goes with the cut's other
rows. A held row would otherwise keep its whole round (or the whole block)
alive, and the held rows of the last `algo.HOLD_ITERS` iterations point into
nearly every round of them.

* `take(chunk, idx)`: rows `idx` of one chunk as a new row batch (copies),
  one vectorised gather per axis.
* `gather(refs)`: rows from any chunks, in the order given (the joined
  table): one `take` per chunk, then one reordering `take`.
* `batch_to_shm` / `batch_from_shm`: a row batch in one `SharedMemory` block
  (the learner writes it, the ranks read views of it); `take_to_shm`: rows
  of a chunk straight into a new block (`batch_to_shm(take(...))` in one
  copy); `gather_to_shm`: rows from any chunks straight into a new block
  (`batch_to_shm(gather(...))` in one copy, the learner's cut);
  `ShmChunk`: a chunk of a block's views that owns the block.
* Cleanup: every block's name carries its owner's pid (`PREFIX`, `shm_name`):
  the learner's, also for the held rows' block rank 0 makes for it. Normal
  paths unlink the names; a learner killed by SIGKILL cannot, and `sweep` (at
  each learner start) removes the blocks of owners that are gone (GH #1152).

Dense rows cost 486 bytes per token (SCHEMA.md "Size"). The compact ROW layout
(`contracts.compact_rows`, `Cfg.compact_rows`, #1550) cuts that about tenfold: a
row batch whose `cat`, `cont`, `pcat` and `pcont` are their sections' live
columns only, on ragged axes of their own. Every function here takes either
layout (`contracts.row_fields`) and gives back the one it got; a chunk's batch
(`RowBatch`) still answers a read of a dense matrix of a compact batch, by
building it (`contracts.dense_rows`: correct, and as large as the dense batch,
so the serving and update paths upload the compact arrays and build the
matrices on the device, `net.dense_tokens`).
"""

from __future__ import annotations

import contextlib
import os
import queue
import secrets
import threading
from collections.abc import Mapping, Sequence
from concurrent.futures import ThreadPoolExecutor
from multiprocessing import shared_memory
from pathlib import Path
from typing import Any, NamedTuple

import numpy as np

from ppo.combat_ppo.ranks import shm_view, to_shm

from . import contracts as C

FIELDS = C.ROW_FIELDS
#: Ragged axis -> its per-row count field.
AXIS_COUNT = {"tok": "n_tok", "mem": "n_mem", "cls": "n_classes", "priv": "n_priv"}
AXIS_COUNT |= {"pool": "n_pool", "ppool": "n_ppool"}
#: The fields the actor fills at acting time (the worker leaves them at their defaults).
ACTING = ("actor", "logp_behav", "action", "iter", "v_behav", "t_src")

#: The extra array of a placed block (`place_to_shm`): each table row's row in it.
ROW_OF = "row_of"
#: Row blocks' name prefix: `PREFIX<owner pid>_<random>` (`shm_name`, `sweep`).
PREFIX = "sts2psm_"
SHM_DIR = Path("/dev/shm")


def shm_name(owner: int | None = None) -> str:
    """A new row block's name. `owner` (default: this process) is the pid whose
    death makes the block garbage: the learner's."""
    return f"{PREFIX}{os.getpid() if owner is None else int(owner)}_{secrets.token_hex(6)}"


def _new_block(size: int, owner: int | None) -> shared_memory.SharedMemory:
    return shared_memory.SharedMemory(
        name=shm_name(owner), create=True, size=max(size, 64), track=False
    )


def _owner_of(name: str) -> int | None:
    try:
        return int(name[len(PREFIX) :].split("_", 1)[0])
    except ValueError:
        return None


def _alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:  # another user's process
        return True
    return True


def sweep(shm_dir: Path = SHM_DIR) -> list[str]:
    """Unlink the row blocks whose owner is gone (a learner killed by SIGKILL, which
    unlinks nothing) and return their names. A live learner's blocks stay."""
    gone: list[str] = []
    # No /dev/shm (not Linux): nothing to sweep. On Windows `os.kill(pid, 0)` would
    # terminate `pid`, and a block goes with its last handle anyway.
    if os.name == "nt" or not shm_dir.is_dir():
        return gone
    for p in shm_dir.glob(PREFIX + "*"):
        pid = _owner_of(p.name)
        if pid is None or pid <= 0 or _alive(pid):
            continue
        with contextlib.suppress(FileNotFoundError):
            p.unlink()
            gone.append(p.name)
    return gone


def offsets(rb: Mapping[str, np.ndarray]) -> dict[str, np.ndarray]:
    """Start offsets (length B + 1) of every ragged axis (a compact batch's
    compact axes too)."""
    out = {}
    counts = {axis: np.asarray(rb[cnt], np.int64) for axis, cnt in AXIS_COUNT.items()}
    if C.is_compact(rb):
        counts |= C.compact_counts(rb)
    for axis, n in counts.items():
        o = np.zeros(len(n) + 1, np.int64)
        np.cumsum(n, out=o[1:])
        out[axis] = o
    return out


class RowBatch(dict):
    """A row batch (either layout). A compact one answers a read of `cat`, `cont`,
    `pcat` or `pcont` with the dense matrix, built on every read and not kept: it
    is the size the compact layout saves. Hot paths read the compact arrays."""

    __slots__ = ()

    def __missing__(self, key: str) -> np.ndarray:
        if key in C.DENSE_TOKEN_FIELDS and "ccont" in self:
            return C.dense_rows(self)[key]
        raise KeyError(key)


def dense(rb: Mapping[str, np.ndarray]) -> dict[str, np.ndarray]:
    """`rb` in the dense layout (`contracts.ROW_FIELDS`), as plain arrays: for a
    reader of the dense matrices that reads them more than once."""
    return C.dense_rows(rb)


def _ranges(starts: np.ndarray, lens: np.ndarray) -> np.ndarray:
    lens = np.asarray(lens, np.int64)
    total = int(lens.sum())
    if total == 0:
        return np.zeros(0, np.int64)
    cum = np.cumsum(lens) - lens
    return np.repeat(np.asarray(starts, np.int64) - cum, lens) + np.arange(total)


#: The most bytes one scattered copy step (`gather_to_shm`) holds in a temporary.
#: 16 MiB, not the first 1 MiB: at 1 MiB a lane table (~170k rows, ~280 serving
#: rounds) takes ~170k small numpy calls, each re-taking the GIL from the serving
#: loop, and the lane's pack went 2.6 -> 4.7 s per iteration (a4000, 2.07B, -12%
#: decisions/s). Bench, 4 threads, a 5.3 GB lane-shaped table: 1 MiB 3.2 s quiet /
#: 109 s against a busy Python thread; 16 MiB 2.2 s / 11-20 s; the pre-block code
#: 2.6 s / 13-31 s. A 16 MiB block is past numpy's 4 MiB huge-page threshold, so
#: on a host with THP defrag=madvise and fragmented memory it can stall in direct
#: compaction again (the a4000 runs defrag=defer).
COPY_BLOCK_BYTES = 16 << 20


def _blocks(
    ch: Chunk, idx: np.ndarray, pos: np.ndarray, like: Mapping[str, np.ndarray]
) -> list[tuple[np.ndarray, np.ndarray]]:
    """`idx`/`pos` cut into row blocks whose widest field's entries fit in
    `COPY_BLOCK_BYTES` (on the chunk's average entries per row)."""
    n = len(idx)
    if n <= 64:  # a serving round's rows: small whatever the fields
        return [(idx, pos)]
    per_row = 8.0  # a row field's bytes, and the row index itself
    for f in C.row_fields(like):
        x = like[f.name]
        width = x.itemsize * int(np.prod(x.shape[1:], dtype=np.int64))
        if f.axis == "row":
            per_row = max(per_row, float(width))
        else:
            o = ch.off[f.axis]
            mean = float(o[-1] - o[0]) / max(1, len(ch.rb["n_tok"]))
            per_row = max(per_row, mean * max(width, 8))  # 8: the entry index
    rows = max(1, int(COPY_BLOCK_BYTES // per_row))
    return [(idx[b : b + rows], pos[b : b + rows]) for b in range(0, n, rows)]


def _places(si: Any, di: Any, o: np.ndarray | None, oo: np.ndarray | None) -> tuple[Any, Any]:
    """Source rows `si` and destination rows `di` as entries on one axis (`o`/`oo`:
    the source's and the block's offsets on it; None for the row axis): slices stay
    slices, and index blocks whose destination is one span become a slice there."""
    if isinstance(si, slice):
        if o is None:
            return si, di
        assert oo is not None
        return slice(int(o[si.start]), int(o[si.stop])), slice(int(oo[di.start]), int(oo[di.stop]))
    if o is None:
        s, d = si, di
    else:
        assert oo is not None
        lens = o[si + 1] - o[si]
        s, d = _ranges(o[si], lens), _ranges(oo[di], lens)
    if len(d) and int(d[-1]) - int(d[0]) + 1 == len(d) and bool((np.diff(d) == 1).all()):
        return s, slice(int(d[0]), int(d[-1]) + 1)
    return s, d


def _copy(v: np.ndarray, x: np.ndarray, s: Any, d: Any) -> None:
    """`v[d] = x[s]` without a large temporary: slice to slice directly, an index
    block into one destination span with `take(out=)`, else through a temporary of
    the block's size."""
    if isinstance(s, slice):
        v[d] = x[s]
    elif not len(s):
        return
    elif isinstance(d, slice):
        np.take(x, s, axis=0, out=v[d], mode="clip")  # in range; "raise" would buffer
    else:
        v[d] = np.take(x, s, axis=0)


def row_chars(rb: Mapping[str, np.ndarray]) -> np.ndarray:
    """Each row's character as an index into `contracts.CHARACTERS` (int64): its
    RUNCTX token's X_CHAR minus 1. X_CHAR 0, a row without "chars" data, is an
    Ironclad row (`tokens.run_char`: only Ironclad leaves it undeclared). Of a
    placed block's batch (`ROW_OF`): its table rows', in the table's order."""
    out = _row_chars(rb)
    row_of = rb.get(ROW_OF)
    return out if row_of is None else out[row_of]


def _row_chars(rb: Mapping[str, np.ndarray]) -> np.ndarray:
    n_tok = np.asarray(rb["n_tok"], np.int64)
    if not len(n_tok):
        return np.zeros(0, np.int64)
    first = np.concatenate([[0], np.cumsum(n_tok)[:-1]])
    col = C.XC["X_CHAR"]
    if C.is_compact(rb):
        # The first token's X_CHAR among its section's live columns, when it is one.
        sec = np.asarray(rb["sec"], np.int64)
        w = C.COMPACT_AXES["ccat"][2][sec]
        s0 = sec[first]
        live = C.LIVE_CAT_TABLE[s0]
        at = (live == col) & (np.arange(live.shape[1])[None, :] < w[first][:, None])
        start = (np.cumsum(w) - w)[first]
        x = np.zeros(len(first), np.int64)
        has = at.any(1)
        x[has] = rb["ccat"][start[has] + at[has].argmax(1)]
        return np.maximum(x - 1, 0)
    x = np.asarray(rb["cat"][first, col], np.int64)
    return np.maximum(x - 1, 0)


class Chunk:
    """One row batch and its axis offsets.

    A placed block's chunk (`place_to_shm`: the batch carries `ROW_OF`) numbers its
    rows as the table does: row `i` is row `row_of[i]` of the batch, `n` is the
    table's length, and every function here that takes rows of a chunk maps them.
    Its `rb` and `off` are in the block's own order."""

    __slots__ = ("_off", "n", "rb", "row_of")

    def __init__(self, rb: Mapping[str, np.ndarray]) -> None:
        self.rb = RowBatch(rb)
        self.row_of = self.rb.get(ROW_OF)
        self.n = len(self.rb["n_tok"]) if self.row_of is None else len(self.row_of)
        self._off: dict[str, np.ndarray] | None = None

    @property
    def off(self) -> dict[str, np.ndarray]:
        """The batch's axis offsets (`offsets`), computed on first use: a serving
        round that goes into the update's block whole never needs them, and the
        learner never reads its table block's (the ranks compute their own)."""
        o = self._off
        if o is None:
            o = self._off = offsets(self.rb)
        return o

    def at(self, idx: Any) -> Any:
        """Rows `idx` (an index or an int64 array) as rows of `rb`."""
        return idx if self.row_of is None else self.row_of[idx]

    def span(self, axis: str, i: int) -> slice:
        o = self.off[axis]
        i = int(self.at(i))
        return slice(int(o[i]), int(o[i + 1]))

    def classes(self, i: int) -> np.ndarray:
        """Row `i`'s class of each member (`mem_class`), int64."""
        return np.asarray(self.rb["mem_class"][self.span("mem", i)], np.int64)


def take(ch: Chunk, idx: Sequence[int] | np.ndarray) -> dict[str, np.ndarray]:
    """Rows `idx` of `ch` as a new row batch (within-row indices carry over)."""
    idx = ch.at(np.asarray(idx, np.int64))
    sel = {axis: _ranges(o[idx], o[idx + 1] - o[idx]) for axis, o in ch.off.items()}
    return RowBatch(
        (f.name, ch.rb[f.name][idx] if f.axis == "row" else ch.rb[f.name][sel[f.axis]])
        for f in C.row_fields(ch.rb)
    )


def axis_rows(ch: Chunk, name: str, idx: Sequence[int] | np.ndarray) -> np.ndarray:
    """Field `name` (a ragged-axis field) of rows `idx`, concatenated in that order."""
    f = next(f for f in C.row_fields(ch.rb) if f.name == name)
    idx = ch.at(np.asarray(idx, np.int64))
    o = ch.off[f.axis]
    return ch.rb[name][_ranges(o[idx], o[idx + 1] - o[idx])]


def concat(batches: Sequence[Mapping[str, np.ndarray]]) -> dict[str, np.ndarray]:
    """The row batches as one (all of one layout, which the result keeps)."""
    if not batches:
        return C.empty_row_batch()
    fields = C.row_fields(batches[0])
    if len(batches) == 1:
        return RowBatch((f.name, np.asarray(batches[0][f.name])) for f in fields)
    return RowBatch((f.name, np.concatenate([b[f.name] for b in batches])) for f in fields)


def _grouped(refs: Sequence[tuple[Chunk, int]]) -> tuple[dict[str, np.ndarray], np.ndarray, int]:
    """The rows `refs` grouped by chunk (one `take` each): the row batch, each of its
    rows' position in `refs`, and the number of chunks."""
    groups: dict[int, tuple[Chunk, list[int], list[int]]] = {}
    for pos, (ch, i) in enumerate(refs):
        g = groups.get(id(ch))
        if g is None:
            g = groups[id(ch)] = (ch, [], [])
        g[1].append(int(i))
        g[2].append(pos)
    parts = [take(ch, idx) for ch, idx, _ in groups.values()]  # `take` maps a placed chunk's rows
    pos = np.concatenate([np.asarray(p, np.int64) for _, _, p in groups.values()])
    return concat(parts), pos, len(groups)


def gather(refs: Sequence[tuple[Chunk, int]]) -> dict[str, np.ndarray]:
    """The rows `refs` (`(chunk, row)` pairs) as one row batch, in that order."""
    if not refs:
        return C.empty_row_batch()
    rb, pos, n_chunks = _grouped(refs)
    if n_chunks == 1 and bool((np.diff(pos) > 0).all()):
        return rb
    return take(Chunk(rb), np.argsort(pos, kind="stable"))


def _layout(
    n_rows: int, totals: Mapping[str, int], like: Mapping[str, np.ndarray]
) -> dict[str, Any]:
    """`ranks.to_shm`'s layout (in `FIELDS` order, 64-byte aligned) of a row batch of
    `n_rows` rows with `totals[axis]` entries per ragged axis, each field shaped and
    typed as `like`'s; `"_size"` is the block's size."""
    layout: dict[str, Any] = {}
    off = 0
    for f in C.row_fields(like):
        x = like[f.name]
        shape = (n_rows if f.axis == "row" else int(totals[f.axis]), *x.shape[1:])
        layout[f.name] = (off, tuple(int(s) for s in shape), x.dtype.str)
        off += -(-int(np.prod(shape, dtype=np.int64)) * x.dtype.itemsize // 64) * 64
    layout["_size"] = off
    return layout


def _same_layout(chunks: Sequence[Chunk], fields: Sequence[Any]) -> None:
    like = chunks[0].rb
    for ch in chunks[1:]:
        if C.is_compact(ch.rb) != C.is_compact(like):
            raise ValueError("chunks of the dense and the compact row layout")
        for f in fields:
            a, b = ch.rb[f.name], like[f.name]
            if a.dtype != b.dtype or a.shape[1:] != b.shape[1:]:
                raise ValueError(f"field {f.name}: chunks of different dtypes or widths")


class RowRefs(NamedTuple):
    """Rows of chunks as arrays, for a table too long to list as `(chunk, row)` pairs:
    row `row[j]` of `chunks[chunk[j]]`. `gather_to_shm` and `place_to_shm` take either."""

    chunks: Sequence[Chunk]
    chunk: np.ndarray
    row: np.ndarray


def _by_chunk(
    refs: Sequence[tuple[Chunk, int]] | RowRefs,
) -> tuple[list[Chunk], np.ndarray, np.ndarray]:
    """`refs` as `(the chunks it uses, each ref's chunk, each ref's row of that
    chunk's batch)`. The chunks keep a `RowRefs`' order, and are in order of first
    use for pairs."""
    if isinstance(refs, RowRefs):
        used = np.flatnonzero(np.bincount(refs.chunk, minlength=len(refs.chunks)))
        rank = np.zeros(len(refs.chunks), np.int64)
        rank[used] = np.arange(len(used))
        chunks: list[Chunk] = [refs.chunks[int(j)] for j in used]
        inv = rank[refs.chunk]
        ri = np.array(refs.row, np.int64)
    else:
        n = len(refs)
        chs, rows_i = zip(*refs, strict=True)
        ids = np.fromiter(map(id, chs), np.int64, n)
        ri = np.fromiter(rows_i, np.int64, n)
        _ids, first, inv = np.unique(ids, return_index=True, return_inverse=True)
        by_first = np.argsort(first, kind="stable")
        rank = np.empty(len(first), np.int64)
        rank[by_first] = np.arange(len(first))
        chunks = [chs[int(first[j])] for j in by_first]
        inv = rank[inv]
    for c, ch in enumerate(chunks):
        if ch.row_of is not None:  # a placed block's rows, as rows of its batch
            m = inv == c
            ri[m] = ch.row_of[ri[m]]
    return chunks, inv, ri


#: A chunk goes into a placed block whole when the table uses this share of its rows.
PLACE_WHOLE = 0.9


def place_to_shm(
    refs: Sequence[tuple[Chunk, int]] | RowRefs,
    *,
    threads: int = 1,
    owner: int | None = None,
) -> tuple[shared_memory.SharedMemory, dict[str, Any]]:
    """The rows `refs` in a new block, in the CHUNKS' order, not the table's: each
    chunk the table uses nearly whole (`PLACE_WHOLE`) is copied as it is, any other
    chunk's used rows are taken, and the block's extra array `ROW_OF` gives each
    table row its row in the block. A `Chunk` of the block numbers its rows as the
    table does (`Chunk.row_of`).

    The copy is one `np.concatenate` per field: a serving round's rows scatter over
    the whole table (it is ordered by segment), so `gather_to_shm` needs an index
    gather per chunk or block and field, tens of thousands of small calls for a
    lane table, each retaking the GIL from the serving loop (#1550). A whole
    chunk's rows the table does not use (a dropped segment's) ride along, unread."""
    n = len(refs.chunk) if isinstance(refs, RowRefs) else len(refs)
    if not n:
        return batch_to_shm(C.empty_row_batch(), owner=owner)
    chunks, inv, ri = _by_chunk(refs)
    like = chunks[0].rb
    fields = C.row_fields(like)
    _same_layout(chunks, fields)
    order = np.argsort(inv, kind="stable")
    cuts = np.searchsorted(inv[order], np.arange(len(chunks) + 1))
    row_of = np.empty(n, np.int64)
    parts: list[Mapping[str, np.ndarray]] = []
    at = 0
    for c, ch in enumerate(chunks):
        pos = order[cuts[c] : cuts[c + 1]]
        idx = ri[pos]
        rows = len(ch.rb["n_tok"])
        if len(pos) >= PLACE_WHOLE * rows:
            parts.append(ch.rb)
            row_of[pos] = at + idx
            at += rows
        else:
            sel = np.unique(idx)
            plain = Chunk({k: v for k, v in ch.rb.items() if k != ROW_OF})
            parts.append(take(plain, sel))
            row_of[pos] = at + np.searchsorted(sel, idx)
            at += len(sel)
    axis_of = {f.axis: f.name for f in fields if f.axis != "row"}
    totals = {axis: sum(len(p[nm]) for p in parts) for axis, nm in axis_of.items()}
    layout = _layout(at, totals, like)
    size = layout.pop("_size")
    layout[ROW_OF] = (size, (n,), row_of.dtype.str)
    size += -(-row_of.nbytes // 64) * 64
    shm = (_POOL.take(size) if owner is None else None) or _new_block(_POOL.room(size), owner)
    views = {k: shm_view(shm, ent) for k, ent in layout.items()}

    def copy(names: list[str]) -> None:
        for nm in names:
            if len(views[nm]):
                np.concatenate([p[nm] for p in parts], out=views[nm])

    try:
        views[ROW_OF][...] = row_of
        # The widest fields first, one thread each.
        names = sorted((f.name for f in fields), key=lambda nm: -views[nm].nbytes)
        k = max(1, min(int(threads), len(names)))
        if k == 1:
            copy(names)
        else:
            with ThreadPoolExecutor(k, thread_name_prefix="e2e-place") as ex:
                list(ex.map(copy, [names[j::k] for j in range(k)]))
    except BaseException:
        views.clear()
        shm.unlink()
        with contextlib.suppress(BufferError):
            shm.close()
        raise
    views.clear()
    return shm, layout


#: The most bytes of one axis' fields that `gather_to_shm` joins into one array
#: before it gathers them (its joined path); a larger axis copies chunk by chunk.
JOIN_AXIS_BYTES = 2 << 30


def gather_to_shm(
    refs: Sequence[tuple[Chunk, int]] | RowRefs,
    *,
    threads: int = 1,
    owner: int | None = None,
) -> tuple[shared_memory.SharedMemory, dict[str, Any]]:
    """`batch_to_shm(gather(refs))` in one copy: the chunks' rows go straight to
    their places in a new block (the same bytes, the same layout), with no row
    batch in between, on `threads` threads. It only reads the chunks, so it can run
    in a thread while the chunks' owner goes on (the learner's cut, S1-cut). The
    caller owns the block (`close`, `unlink`); its name carries `owner`
    (`shm_name`).

    An axis whose fields are at most `JOIN_AXIS_BYTES` over all chunks (every axis
    of a compact table) is joined first: its fields are concatenated over the
    chunks (a transient of that size) and the table's rows gathered from them in
    row blocks, a few thousand large numpy calls that drop the GIL. A larger axis
    (a dense table's tokens) copies chunk by chunk: one small call per chunk,
    block and field, which for a lane table of ~1,800 serving rounds is ~100,000
    calls that each retake the GIL from the serving loop (#1550: with compact rows
    the copy took as long as the dense one, 8.5 s, and slowed the collect by 3 s)."""
    n = len(refs.chunk) if isinstance(refs, RowRefs) else len(refs)
    if not n:
        return batch_to_shm(C.empty_row_batch(), owner=owner)
    chunks, inv, ri = _by_chunk(refs)
    like = chunks[0].rb
    fields = C.row_fields(like)
    _same_layout(chunks, fields)
    # The chunks as one virtual batch: each table row's row in it, and its axes'
    # offsets there. Then every table row's entry count and the block's offsets.
    ns = np.fromiter((len(ch.rb["n_tok"]) for ch in chunks), np.int64, len(chunks))
    base = np.concatenate([[0], np.cumsum(ns)])
    g = base[inv] + ri
    goff: dict[str, np.ndarray] = {}
    cnts: dict[str, np.ndarray] = {}
    out_off: dict[str, np.ndarray] = {}
    for axis in chunks[0].off:
        tot = np.fromiter((ch.off[axis][-1] for ch in chunks), np.int64, len(chunks))
        shift = np.concatenate([[0], np.cumsum(tot)])
        o = np.concatenate([ch.off[axis][:-1] for ch in chunks] + [shift[-1:]])
        o[:-1] += np.repeat(shift[:-1], ns)
        goff[axis] = o
        cnts[axis] = o[g + 1] - o[g]
        oo = np.zeros(n + 1, np.int64)
        np.cumsum(cnts[axis], out=oo[1:])
        out_off[axis] = oo
    layout = _layout(n, {a: int(o[-1]) for a, o in out_off.items()}, like)
    size = layout.pop("_size")
    by_axis: dict[str, list[str]] = {}
    for f in fields:
        by_axis.setdefault(f.axis, []).append(f.name)
    joined = {
        axis: names
        for axis, names in by_axis.items()
        if len(chunks) > 1
        and sum(ch.rb[nm].nbytes for nm in names for ch in chunks) <= JOIN_AXIS_BYTES
    }
    rest = [f for f in fields if f.axis not in joined]
    shm = (_POOL.take(size) if owner is None else None) or _new_block(_POOL.room(size), owner)
    views = {f.name: shm_view(shm, layout[f.name]) for f in fields}
    k = max(1, int(threads))
    try:
        with ThreadPoolExecutor(k, thread_name_prefix="e2e-gather") as ex:
            for axis, names in joined.items():
                src = {nm: np.concatenate([ch.rb[nm] for ch in chunks]) for nm in names}
                # Row blocks whose entry indices stay under `COPY_BLOCK_BYTES`.
                per_row = 8.0 if axis == "row" else 8.0 * float(out_off[axis][-1]) / n
                step = max(1, int(COPY_BLOCK_BYTES // max(per_row, 8.0)))

                def job(
                    b0: int, axis: str = axis, src: dict[str, np.ndarray] = src, step: int = step
                ) -> None:
                    b1 = min(n, b0 + step)
                    if axis == "row":
                        sel, dst = g[b0:b1], slice(b0, b1)
                    else:
                        oo = out_off[axis]
                        sel = _ranges(goff[axis][g[b0:b1]], cnts[axis][b0:b1])
                        dst = slice(int(oo[b0]), int(oo[b1]))
                    if len(sel):
                        for nm, x in src.items():
                            np.take(x, sel, axis=0, out=views[nm][dst], mode="clip")

                list(ex.map(job, range(0, n, step)))
                del src
            if rest:
                # Per chunk, its rows' places. A chunk whose rows land in order on one
                # span (the held rows' block, which leads the table) copies slices. Any
                # other chunk copies in row blocks (`_blocks`): their index arrays and
                # copies stay under `COPY_BLOCK_BYTES`, which bounds each copy's
                # allocation but, at 16 MiB, NOT below numpy's 4 MiB huge-page
                # threshold: on a host with THP defrag=madvise and fragmented memory a
                # block copy can still stall in direct compaction (see
                # `COPY_BLOCK_BYTES`).
                order = np.argsort(inv, kind="stable")
                cuts = np.searchsorted(inv[order], np.arange(len(chunks) + 1))
                plans = []
                for c, ch in enumerate(chunks):
                    pos = order[cuts[c] : cuts[c + 1]]
                    idx = ri[pos]
                    if bool((np.diff(pos) == 1).all() and (np.diff(idx) == 1).all()):
                        i0, i1 = int(idx[0]), int(idx[-1]) + 1
                        plans.append((ch, [(slice(i0, i1), slice(int(pos[0]), int(pos[-1]) + 1))]))
                    else:
                        plans.append((ch, _blocks(ch, idx, pos, like)))

                def copy(group: list[tuple[Chunk, list[tuple[Any, Any]]]]) -> None:
                    for ch, spans in group:
                        for si, di in spans:
                            places = {"row": _places(si, di, None, None)}
                            for axis in {f.axis for f in rest} - {"row"}:
                                places[axis] = _places(si, di, ch.off[axis], out_off[axis])
                            for f in rest:
                                s_, d_ = places[f.axis]
                                _copy(views[f.name], ch.rb[f.name], s_, d_)

                kk = min(k, len(plans))
                list(ex.map(copy, [plans[j::kk] for j in range(kk)]))
    except BaseException:
        views.clear()
        shm.unlink()
        with contextlib.suppress(BufferError):
            shm.close()
        raise
    views.clear()
    return shm, layout


def batch_to_shm(
    rb: Mapping[str, np.ndarray], *, owner: int | None = None
) -> tuple[shared_memory.SharedMemory, dict[str, Any]]:
    """`rb` in one `SharedMemory` block; the caller owns it (`close`, `unlink`); its
    name carries `owner` (`shm_name`)."""
    spec = {}
    for f in C.row_fields(rb):
        x = np.asarray(rb[f.name])
        spec[f.name] = (x.shape, x.dtype, x)
    return to_shm(spec, name=shm_name(owner))


def take_to_shm(
    ch: Chunk, idx: Sequence[int] | np.ndarray, *, owner: int | None = None
) -> tuple[shared_memory.SharedMemory, dict[str, Any]]:
    """Rows `idx` of `ch` in a new `SharedMemory` block, laid out as `batch_to_shm`
    lays out `take(ch, idx)`: each field's rows go straight into the block, with
    no row batch in between. The caller owns the block (`close`, `unlink`); its
    name carries `owner` (`shm_name`)."""
    idx = ch.at(np.asarray(idx, np.int64))
    sel = {axis: _ranges(o[idx], o[idx + 1] - o[idx]) for axis, o in ch.off.items()}
    src = {
        f.name: (ch.rb[f.name], idx if f.axis == "row" else sel[f.axis])
        for f in C.row_fields(ch.rb)
    }
    layout: dict[str, Any] = {}
    off = 0
    for name, (x, ix) in src.items():  # `ranks.to_shm`'s layout: 64-byte aligned, in order
        shape = (len(ix), *x.shape[1:])
        layout[name] = (off, shape, x.dtype.str)
        off += -(-int(np.prod(shape, dtype=np.int64)) * x.dtype.itemsize // 64) * 64
    shm = _new_block(off, owner)
    try:
        for name, (x, ix) in src.items():
            v = shm_view(shm, layout[name])
            np.take(x, ix, axis=0, out=v, mode="clip")  # in range; "raise" would buffer
            del v
    except BaseException:
        shm.unlink()
        with contextlib.suppress(BufferError):
            shm.close()
        raise
    return shm, layout


def batch_from_shm(shm: shared_memory.SharedMemory, layout: Mapping[str, Any]) -> dict[str, Any]:
    """Views of a `batch_to_shm` block (valid while `shm` is open)."""
    return {k: shm_view(shm, ent) for k, ent in layout.items()}


class _Closer:
    """Unmaps shared-memory blocks in one daemon thread (#1550).

    An update's row block holds its whole table (15-25 GB in the lane). Once its
    name is gone, the unmap of its last mapping frees every page, which took the
    serving loop ~7.8 s an iteration. The unmap releases the GIL, so here it
    overlaps serving. `drain` waits for every queued close. A forked child starts
    its own thread on its first close.

    `submit` takes the block's mapping and file descriptor OUT of the
    `SharedMemory` object, on the caller's thread, and the thread releases those.
    Had the thread called `SharedMemory.close` instead, a second close of the same
    object (its `__del__`, a caller's own) could run between that close's
    `os.close(fd)` and its `fd = -1`, and close the descriptor again: by then
    another thread's new block may own that number (EBADF in an unrelated open)."""

    def __init__(self) -> None:
        self._cv = threading.Condition()
        self._pending = 0
        self._q: queue.SimpleQueue[tuple[Any, Any, int]] = queue.SimpleQueue()
        self._pid = -1

    def submit(self, shm: shared_memory.SharedMemory) -> None:
        with self._cv:
            if self._pid != os.getpid():  # first close here (or in a forked child)
                self._pid, self._pending = os.getpid(), 0
                self._q = queue.SimpleQueue()
                threading.Thread(
                    target=self._run, args=(self._q,), daemon=True, name="e2e-shm-close"
                ).start()
            parts: Any = shm  # its private state: the object is closed from here on
            item = (parts._buf, parts._mmap, parts._fd)
            parts._buf, parts._mmap, parts._fd = None, None, -1
            self._pending += 1
            self._q.put(item)

    def _run(self, q: queue.SimpleQueue[tuple[Any, Any, int]]) -> None:
        while True:
            buf, mm, fd = q.get()
            try:
                # A view left keeps the mapping (it goes with the view); the
                # descriptor goes either way (the mapping has its own).
                with contextlib.suppress(BufferError):
                    if buf is not None:
                        buf.release()
                    if mm is not None:
                        mm.close()
                if fd >= 0:
                    with contextlib.suppress(OSError):
                        os.close(fd)
            finally:
                del buf, mm
                with self._cv:
                    self._pending -= 1
                    self._cv.notify_all()

    def drain(self, timeout: float | None = None) -> bool:
        """Wait until every queued block is closed; False on `timeout`."""
        with self._cv:
            if self._pid != os.getpid():
                return True
            return self._cv.wait_for(lambda: self._pending == 0, timeout)


_CLOSER = _Closer()


class _Pool:
    """One retired row block, kept mapped and named for the next `gather_to_shm`
    (#1550, `Cfg.recycle_blocks`).

    A new block's pages are allocated and zeroed as the copy first touches them,
    and freed again when it is unmapped: copying 16 GiB into a new block took
    2.95 s on an idle host and 0.45 s into one that had been written before. The
    learner's tables are about the same size every iteration, so the block of the
    update that just landed takes the next cut's rows. A block too small for them
    is dropped, and its successor gets `ROOM` to spare."""

    ROOM = 1.1

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._block: shared_memory.SharedMemory | None = None
        self._open = False

    def open(self) -> None:
        with self._lock:
            self._open = True

    def close(self) -> None:
        """Stop pooling and drop the pooled block (its name and mapping)."""
        with self._lock:
            self._open = False
            old, self._block = self._block, None
        if old is not None:
            _drop(old)

    def room(self, size: int) -> int:
        return int(size * self.ROOM) if self._open else size

    def put(self, shm: shared_memory.SharedMemory) -> bool:
        """Keep `shm` for the next gather; False when the pool is closed."""
        with self._lock:
            if not self._open:
                return False
            old, self._block = self._block, shm
        if old is not None:
            _drop(old)
        return True

    def take(self, size: int) -> shared_memory.SharedMemory | None:
        with self._lock:
            shm, self._block = self._block, None
        if shm is not None and shm.size < size:
            _drop(shm)
            return None
        return shm


def _drop(shm: shared_memory.SharedMemory) -> None:
    with contextlib.suppress(FileNotFoundError):
        shm.unlink()
    _CLOSER.submit(shm)


_POOL = _Pool()


def pool_open() -> None:
    """Retired row blocks (`ShmChunk.retire`) are kept for the next gather."""
    _POOL.open()


def pool_close() -> None:
    """They are not, and the kept one goes (`drain_closes` waits for its unmap)."""
    _POOL.close()


def drain_closes(timeout: float | None = None) -> bool:
    """Wait for the `ShmChunk` blocks whose chunks went to be unmapped."""
    return _CLOSER.drain(timeout)


class ShmChunk(Chunk):
    """A chunk of views of a `batch_to_shm` block, which it owns: the block is
    closed (unmapped) when the chunk goes, that is with the last row pointing
    into it, by `_CLOSER`'s thread (`drain_closes` waits for it). `unlink`
    removes the block's name once the ranks are done with it; the chunk's
    views stay valid until it goes."""

    __slots__ = ("retired", "shm")

    def __init__(self, shm: shared_memory.SharedMemory, layout: Mapping[str, Any]) -> None:
        self.retired = False
        super().__init__(batch_from_shm(shm, layout))
        self.shm = shm

    def retire(self) -> None:
        """The ranks are done with the block: when the chunk goes it is offered
        to the pool, name and mapping (`_Pool`), and unlinked only if refused."""
        self.retired = True

    def unlink(self) -> None:
        with contextlib.suppress(FileNotFoundError):  # already unlinked
            self.shm.unlink()

    def __del__(self) -> None:
        shm = getattr(self, "shm", None)
        if shm is None:  # __init__ failed
            return
        self.rb = {}  # the views first: a block with views left cannot close
        if getattr(self, "retired", False):
            if _POOL.put(shm):
                return
            with contextlib.suppress(FileNotFoundError):
                shm.unlink()
        try:
            _CLOSER.submit(shm)
        except Exception:  # no thread at interpreter shutdown: close here
            with contextlib.suppress(BufferError):
                shm.close()
