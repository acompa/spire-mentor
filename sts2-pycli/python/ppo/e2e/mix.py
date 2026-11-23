"""The character mix of an all-character lane (GH #1197): which character a new run plays.

One shared net trains on every character, warm-started from an Ironclad net. The
lane plays Ironclad only until `settle_end` decisions, then the share of
non-Ironclad runs ramps linearly to `target_new` at `ramp_end` and holds there::

    share = mix_share(decisions, cfg)          # 0 at or before settle_end
    character = draw_character(seed, decisions, cfg)

`draw_character` is a function of the run seed and that share only: the seed hashes
to u in [0, 1); u >= share plays Ironclad, else a second hash of the seed picks one
of the other characters evenly. So a run's character is reproducible from its seed
and the decision clock it started at, a seed that plays Ironclad at a share plays it
at every lower share, and a resumed lane (the same clock from its `AlgoState`, the
same `MixCfg` from its state) draws what the lost session would have.

**Fixed weights** (GH #1535): with `weights` set (character -> weight, e.g. the
Fool lane's `{"Fool": 0.8, "Ironclad": 0.04, ...}`), the ramp is off: every run draws
its character from its seed alone, by the normalized weights in `characters` order,
at every decision clock. `characters` is then the weights' keys (in the order given)
and needs no Ironclad; `mix_share` is the share of runs that are not Ironclad.

The learner takes the mix as `--mix` (JSON, `MixCfg.from_dict`), keeps it in its
state and hands it to its workers (`worker_cfg()["mix"]`); a worker draws each new
full run's character at its decision clock (`practice.SegmentSource`), which the
learner sends with every round (`info["decisions"]`).
"""

from __future__ import annotations

import hashlib
from collections.abc import Mapping
from dataclasses import asdict, dataclass, fields
from typing import Any

from . import factory

BASE = factory.CHARACTERS[0]  # Ironclad: the warm start's character


@dataclass(frozen=True)
class MixCfg:
    characters: tuple[str, ...] = ("Ironclad", "Silent", "Defect", "Regent", "Necrobinder")
    settle_end: int = 5_000_000  # Ironclad only before this
    ramp_end: int = 205_000_000  # linear ramp settle_end -> ramp_end
    target_new: float = 0.8  # share of non-Ironclad runs after the ramp
    # Fixed per-character weights (module doc); None is the Ironclad-based ramp.
    weights: tuple[tuple[str, float], ...] | None = None

    def __post_init__(self) -> None:
        if self.weights is not None:
            w = tuple((str(c), float(x)) for c, x in dict(self.weights).items())
            if len(w) != len(self.weights):
                raise ValueError(f"mix weights {self.weights}: a character is listed twice")
            if any(not x > 0.0 for _, x in w):
                raise ValueError(f"mix weights {dict(w)}: every weight must be > 0")
            object.__setattr__(self, "weights", w)
            object.__setattr__(self, "characters", tuple(c for c, _ in w))
        chars = tuple(str(c) for c in self.characters)
        object.__setattr__(self, "characters", chars)
        if unknown := [c for c in chars if c not in factory.CHARACTERS]:
            raise ValueError(f"mix characters {unknown}: not in {factory.CHARACTERS}")
        if len(set(chars)) != len(chars):
            raise ValueError(f"mix characters {list(chars)}: a character is listed twice")
        if self.weights is not None:
            if len(chars) < 2:
                raise ValueError(f"mix weights {list(chars)}: need two characters or more")
        elif BASE not in chars or len(chars) < 2:
            raise ValueError(f"mix characters {list(chars)}: need {BASE} and another")
        if not 0 <= int(self.settle_end) < int(self.ramp_end):
            raise ValueError(
                f"mix: need 0 <= settle_end < ramp_end, got {self.settle_end}, {self.ramp_end}"
            )
        if not 0.0 <= float(self.target_new) <= 1.0:
            raise ValueError(f"mix: target_new {self.target_new} outside [0, 1]")

    @property
    def others(self) -> tuple[str, ...]:
        """The lane's characters but Ironclad, in the order given."""
        return tuple(c for c in self.characters if c != BASE)

    def to_dict(self) -> dict[str, Any]:
        """Plain JSON types (the learner state loads with `weights_only=True`)."""
        d = asdict(self)
        d["characters"] = list(self.characters)
        if self.weights is None:
            d.pop("weights")  # a ramp mix's dict is the one it always was
        else:
            d["weights"] = dict(self.weights)
        return d

    def shares(self) -> dict[str, float]:
        """A weights mix's normalized share per character (`characters` order)."""
        if self.weights is None:
            raise ValueError("a ramp mix has no fixed shares")
        total = sum(x for _, x in self.weights)
        return {c: x / total for c, x in self.weights}

    @classmethod
    def from_dict(cls, d: Mapping[str, Any]) -> MixCfg:
        """`--mix`'s JSON; unset keys take the defaults, unknown keys raise."""
        names = {f.name for f in fields(cls)}
        if unknown := sorted(set(d) - names):
            raise ValueError(f"mix: unknown keys {unknown} (known: {sorted(names)})")
        kw: dict[str, Any] = dict(d)
        if "characters" in kw:
            kw["characters"] = tuple(kw["characters"])
        if kw.get("weights") is not None:
            kw["weights"] = tuple((str(c), float(x)) for c, x in dict(kw["weights"]).items())
            if "characters" in kw and tuple(kw["characters"]) != tuple(c for c, _ in kw["weights"]):
                raise ValueError(
                    f"mix: characters {list(kw['characters'])} differ from the weights' keys"
                )
        for k in ("settle_end", "ramp_end"):
            if k in kw:
                kw[k] = int(kw[k])
        if "target_new" in kw:
            kw["target_new"] = float(kw["target_new"])
        return cls(**kw)


def mix_share(decisions: int, cfg: MixCfg) -> float:
    """The share of non-Ironclad runs at `decisions` (the learner's clock): 0 at or
    before `settle_end`, linear to `target_new` at `ramp_end`, held after."""
    if cfg.weights is not None:
        return 1.0 - cfg.shares().get(BASE, 0.0)
    d = int(decisions)
    if d <= cfg.settle_end:
        return 0.0
    if d >= cfg.ramp_end:
        return float(cfg.target_new)
    return float(cfg.target_new) * (d - cfg.settle_end) / (cfg.ramp_end - cfg.settle_end)


def _hash64(seed: int, salt: bytes) -> int:
    h = hashlib.blake2b(int(seed).to_bytes(8, "little", signed=True), digest_size=8, person=salt)
    return int.from_bytes(h.digest(), "little")


def draw_character(seed: int, decisions: int, cfg: MixCfg) -> str:
    """The character of the run with `seed` started at `decisions` (module doc)."""
    u = _hash64(seed, b"e2e-mix-u") / 2.0**64
    if cfg.weights is not None:
        acc = 0.0
        for c, share in cfg.shares().items():
            acc += share
            if u < acc:
                return c
        return cfg.characters[-1]  # float rounding at u ~ 1
    share = mix_share(decisions, cfg)
    if u >= share:
        return BASE
    others = cfg.others
    return others[_hash64(seed, b"e2e-mix-c") % len(others)]
