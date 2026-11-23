"""Forced deck inclusion (GH #1541): a share of training runs start with a never-picked card.

A card the policy is offered hundreds of times and never takes gets no gradient: the
card-reward head's low opinion of it is assumed, not learned, and the combat policy
never learns to play it. A forced run adds one such card to the starting deck (the
character's list in `forced_cards.toml`, or `ForcedCfg.cards`), so the value head
sees runs that hold it and the combat policy plays it. Unlike #1445's forced pick,
no decision is taken off-policy: the run simply has the card, so every row stays
on-policy and nothing needs excluding from the loss.

Which runs, and which card, is a function of the run seed only::

    card = draw_forced(seed, character, cfg)   # None: an ordinary run

The seed hashes to u in [0, 1); u < p forces a card, which a second hash picks
evenly from the character's list. So a resumed lane forces what the lost session
would have, and a forced run's action-log record replays (its `extra` names the
card; `actionlog._segment_env`).

Forced runs feed no practice queue: a practice start replays its capture from the
seed and the action log, which does not know the card (`practice.SegmentSource`).
The learner counts their outcomes separately (stats row `forced`), and keeps them
in the lane's ordinary run statistics too: they are real runs of the lane.

The learner takes it as `--forced` (JSON, `ForcedCfg.from_dict`), keeps it in its
state and hands it to its workers (`worker_cfg()["forced"]`).
"""

from __future__ import annotations

import hashlib
import tomllib
from collections.abc import Mapping
from dataclasses import dataclass, field
from functools import cache
from pathlib import Path
from typing import Any

from . import factory

CARDS_FILE = Path(__file__).with_name("forced_cards.toml")


@cache
def default_cards() -> dict[str, tuple[str, ...]]:
    """`forced_cards.toml`: per character, its never-picked card ids."""
    with CARDS_FILE.open("rb") as f:
        raw = tomllib.load(f)
    return {str(c): tuple(str(x) for x in ids) for c, ids in raw.items()}


@dataclass(frozen=True)
class ForcedCfg:
    p: float = 0.03  # share of full runs that start with a forced card
    # Per character, the card ids to draw from; empty: `default_cards()`. A character
    # without a list (or with an empty one) is never forced.
    cards: Mapping[str, tuple[str, ...]] = field(default_factory=dict)

    def __post_init__(self) -> None:
        if not 0.0 <= float(self.p) <= 1.0:
            raise ValueError(f"forced: p {self.p} outside [0, 1]")
        cards = {str(c): tuple(str(x) for x in ids) for c, ids in self.cards.items()}
        if unknown := [c for c in cards if c not in factory.CHARACTERS]:
            raise ValueError(f"forced cards: characters {unknown} not in {factory.CHARACTERS}")
        object.__setattr__(self, "p", float(self.p))
        object.__setattr__(self, "cards", cards)

    def pool(self, character: str) -> tuple[str, ...]:
        """The card ids `character`'s forced runs draw from."""
        src = self.cards or default_cards()
        return tuple(src.get(str(character), ()))

    def to_dict(self) -> dict[str, Any]:
        """Plain JSON types (the learner state loads with `weights_only=True`)."""
        return {"p": self.p, "cards": {c: list(ids) for c, ids in self.cards.items()}}

    @classmethod
    def from_dict(cls, d: Mapping[str, Any]) -> ForcedCfg:
        """`--forced`'s JSON; unset keys take the defaults, unknown keys raise."""
        known = {"p", "cards"}
        if unknown := sorted(set(d) - known):
            raise ValueError(f"forced: unknown keys {unknown} (known: {sorted(known)})")
        cards = d.get("cards") or {}
        if not isinstance(cards, Mapping):
            raise ValueError(f"forced: cards must map a character to card ids, got {cards!r}")
        return cls(p=float(d.get("p", cls.p)), cards={c: tuple(v) for c, v in cards.items()})


def _hash64(seed: int, salt: bytes) -> int:
    h = hashlib.blake2b(int(seed).to_bytes(8, "little", signed=True), digest_size=8, person=salt)
    return int.from_bytes(h.digest(), "little")


def draw_forced(seed: int, character: str, cfg: ForcedCfg | None) -> str | None:
    """The card the run with `seed` of `character` starts with, or None (module doc)."""
    if cfg is None or cfg.p <= 0.0:
        return None
    pool = cfg.pool(character)
    if not pool:
        return None
    if _hash64(seed, b"e2e-forced-u") / 2.0**64 >= cfg.p:
        return None
    return pool[_hash64(seed, b"e2e-forced-c") % len(pool)]


def forced_stats(run_ends: list[Mapping[str, Any]]) -> dict[str, Any]:
    """A stats row's `forced`: the forced runs among `run_ends` (end records), overall
    and per card (`character:card`): runs, wins and mean floor. Empty when none."""
    by: dict[str, list[Mapping[str, Any]]] = {}
    for e in run_ends:
        card = e.get("forced_card")
        if card:
            by.setdefault(f"{e.get('character') or factory.CHARACTER}:{card}", []).append(e)
    if not by:
        return {}

    def agg(es: list[Mapping[str, Any]]) -> dict[str, Any]:
        n = len(es)
        return {
            "runs": n,
            "won": sum(bool(e.get("won")) for e in es),
            "floor_mean": sum(float(e.get("floor", 0)) for e in es) / n,
        }

    every = [e for es in by.values() for e in es]
    return {"all": agg(every), "cards": {k: agg(v) for k, v in sorted(by.items())}}
