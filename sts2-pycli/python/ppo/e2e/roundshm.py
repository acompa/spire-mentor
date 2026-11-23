"""A worker's round of rows in shared memory (GH #1550).

A worker's message carried its round's row batch (~0.6 MB of arrays) through
the pipe. The learner's reader thread then held the GIL to assemble and
unpickle it, about a fifth of the wall time, while the serving thread drops and
retakes the GIL around every copy and every device call.

With `Cfg.shm_rounds` the worker writes the batch into a block it reuses every
round (`RoundSlot`) and sends its name and layout instead (`REF`, a few hundred
bytes). The learner maps the block once (`RoundMaps`) and reads the rows there.
The worker writes its next round only after the learner's reply, and the learner
copies the rows out (the round's `concat`) before it replies.

This module imports no torch: the workers load it.
"""

from __future__ import annotations

import contextlib
import secrets
from collections.abc import Mapping
from multiprocessing import shared_memory
from typing import Any

import numpy as np

from . import contracts as C

#: `rowstore.PREFIX`: the blocks are named `PREFIX<owner pid>_<random>`, so
#: `rowstore.sweep` removes those of a learner that died.
PREFIX = "sts2psm_"
#: The first item of a row batch sent by reference: `(REF, name, layout)`.
REF = "shm"
Layout = dict[str, tuple[int, tuple[int, ...], str]]


def _view(shm: shared_memory.SharedMemory, ent: tuple[int, tuple[int, ...], str]) -> np.ndarray:
    off, shape, dt = ent
    return np.ndarray(shape, np.dtype(dt), buffer=shm.buf, offset=off)


class RoundSlot:
    """The worker's block: `write` puts a row batch in it and returns its reference.
    The block grows (a new block, a new name) when a round does not fit."""

    def __init__(self, owner: int) -> None:
        self.owner = int(owner)
        self.shm: shared_memory.SharedMemory | None = None

    def write(self, rb: Mapping[str, np.ndarray]) -> tuple[str, str, Layout]:
        layout: Layout = {}
        arrs = {}
        off = 0
        for f in C.row_fields(rb):  # 64-byte aligned, in field order
            x = arrs[f.name] = np.asarray(rb[f.name])
            layout[f.name] = (off, tuple(int(s) for s in x.shape), x.dtype.str)
            off += -(-x.nbytes // 64) * 64
        if self.shm is None or self.shm.size < off:
            self.close()
            name = f"{PREFIX}{self.owner}_{secrets.token_hex(6)}"
            size = max(off * 3 // 2, 1 << 16)
            self.shm = shared_memory.SharedMemory(name=name, create=True, size=size, track=False)
        for name, x in arrs.items():
            if x.size:
                _view(self.shm, layout[name])[...] = x
        return (REF, self.shm.name, layout)

    def close(self) -> None:
        shm, self.shm = self.shm, None
        if shm is not None:
            with contextlib.suppress(FileNotFoundError):
                shm.unlink()
            with contextlib.suppress(BufferError):
                shm.close()


def is_ref(rb: Any) -> bool:
    return isinstance(rb, tuple) and len(rb) == 3 and rb[0] == REF


class RoundMaps:
    """The learner's mappings of its workers' blocks, one per worker."""

    def __init__(self) -> None:
        self._maps: dict[Any, shared_memory.SharedMemory] = {}
        self._names: set[str] = set()

    def rows(self, key: Any, ref: tuple[str, str, Layout]) -> dict[str, np.ndarray]:
        """The referenced row batch as views of worker `key`'s block: valid until
        the worker is answered (it writes its next round over them)."""
        _tag, name, layout = ref
        shm = self._maps.get(key)
        if shm is None or shm.name != name:
            if shm is not None:  # the worker's block grew: its old one is gone
                with contextlib.suppress(BufferError):
                    shm.close()
            shm = self._maps[key] = shared_memory.SharedMemory(name=name, track=False)
            self._names.add(name)
        return {k: _view(shm, ent) for k, ent in layout.items()}

    def close(self) -> None:
        """Unmap every block, and unlink those a worker left (a killed worker)."""
        for shm in self._maps.values():
            with contextlib.suppress(BufferError):
                shm.close()
        self._maps = {}
        for name in self._names:
            with contextlib.suppress(FileNotFoundError):
                shared_memory.SharedMemory(name=name, track=False).unlink()
        self._names = set()
