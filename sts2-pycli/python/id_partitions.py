"""The parity harness's hand-kept id lists, as checked partitions (GH #1500).

Each list below must cover its build's universe (`content_universe`): every id
of the build is either IN the list or in one of the list's named exclusions,
and each exclusion carries the reason the list leaves those ids out. Three
things fail `tests/test_id_partitions.py`:

* **missing** — an id of the build that is neither listed nor excluded. A new
  game build's content lands here on day one, so somebody has to decide
  whether the list exercises it.
* **unknown** — a listed id the build does not have (a typo, a rename, a
  removed id): such an entry exercises nothing and still reads as coverage.
* **stale exclusion** — an excluded id no build has.

The lists themselves stay where their consumers import them
(`parity_fuzz_scenario`); this module only names each list's exclusions.
"""

from __future__ import annotations

from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass

import content_universe
import parity_fuzz_scenario as pfs


@dataclass(frozen=True)
class Partition:
    """One hand-kept list, the universe it partitions and its exclusions."""

    name: str
    listed: frozenset[str]
    universe: Callable[[str], Iterable[str]]
    #: reason -> ids the list deliberately leaves out.
    excluded: Mapping[str, frozenset[str]]


def problems(part: Partition, version: str) -> list[str]:
    """Every way ``part`` fails to partition ``version``'s universe (empty == ok)."""
    universe = set(part.universe(version))
    known = {i for b in content_universe.BUILDS for i in part.universe(b)}
    excluded = frozenset().union(*part.excluded.values()) if part.excluded else frozenset()
    out: list[str] = []
    if unknown := sorted(part.listed - universe):
        out.append(f"{part.name} lists ids {version} does not have: {unknown}")
    if missing := sorted(universe - part.listed - excluded):
        out.append(
            f"{part.name} neither lists nor excludes {len(missing)} id(s) of {version}: "
            f"{missing} — add each to the list, or to an exclusion with its reason"
        )
    for reason, ids in part.excluded.items():
        if stale := sorted(ids - known):
            out.append(f"{part.name} exclusion {reason!r} names ids no build has: {stale}")
    return out


_EVENT_GRANTED = (
    "an event or Ancient hands it out by name (EVENT_RELICS); a grafted copy would become a second"
)

PARTITIONS: tuple[Partition, ...] = (
    Partition(
        name="ALL_RELICS",
        listed=frozenset(pfs.ALL_RELICS),
        universe=content_universe.relics,
        excluded={
            # ALL_RELICS keeps the 35 event relics it always swept; the rest of
            # the event pool is out for the reason the loadout leaves it out.
            _EVENT_GRANTED: pfs.EVENT_RELICS - set(pfs.ALL_RELICS),
            "frozen single-fight axis (generate_v2's rng keys the trace cache); "
            "generate_loadout grafts it (LOADOUT_ONLY_RELICS)": pfs.LOADOUT_ONLY_RELICS,
            "nothing to exercise (PLACEHOLDER_RELICS)": pfs.PLACEHOLDER_RELICS,
        },
    ),
    Partition(
        name="ALL_POTIONS",
        listed=frozenset(pfs.ALL_POTIONS),
        universe=content_universe.potions,
        excluded={
            "class potion: only its character drinks it; generate_loadout grafts it onto "
            "that character (CLASS_POTIONS)": pfs.CLASS_POTIONS,
            "newer than the frozen single-fight list; generate_loadout grafts it "
            "(LOADOUT_ONLY_POTIONS)": pfs.LOADOUT_ONLY_POTIONS,
        },
    ),
    Partition(
        name="ALL_ENCOUNTERS",
        listed=frozenset(pfs.ALL_ENCOUNTERS),
        universe=lambda _version: content_universe.encounters(),
        excluded={},
    ),
)


def loadout_partition(version: str) -> Partition:
    """The loadout relic pool at ``version`` as a partition. It is derived, so
    this proves the derivation and its exclusions agree."""
    return Partition(
        name=f"loadout_relics({version})",
        listed=frozenset(pfs.loadout_relics(version)),
        universe=content_universe.relics,
        excluded={
            _EVENT_GRANTED: pfs.EVENT_RELICS,
            "nothing to exercise (PLACEHOLDER_RELICS)": pfs.PLACEHOLDER_RELICS,
        },
    )
