"""The hand-kept parity id lists partition each build's universe (GH #1500).

Pure Python: the universe is parsed from the Rust source (`content_universe`),
so these run in the per-PR gate without a wheel or the C# oracle, on every
build at once (the gate's pytest runs only at the default build).
"""

from __future__ import annotations

import content_universe as cu
import id_partitions as ip
import parity_fuzz_scenario as pfs
import pytest
from card_inventory import engine_cards, sweepable_cards, v0111_only_cards, v0113_only_cards
from event_parity import CONTEXT_DEPENDENT_EVENTS

_CHARACTERS = ("Ironclad", "Silent", "Defect", "Necrobinder", "Regent")


def _characters(version: str) -> tuple[str, ...]:
    return (*_CHARACTERS, "Fool") if cu.version_tuple(version) >= (0, 113, 0) else _CHARACTERS


@pytest.mark.parametrize("version", cu.BUILDS)
@pytest.mark.parametrize("part", ip.PARTITIONS, ids=lambda p: p.name)
def test_list_partitions_the_build_universe(part: ip.Partition, version: str) -> None:
    assert ip.problems(part, version) == []


@pytest.mark.parametrize("version", cu.BUILDS)
def test_loadout_relics_are_the_universe_minus_named_exclusions(version: str) -> None:
    part = ip.loadout_partition(version)
    assert ip.problems(part, version) == []
    pool = pfs.loadout_relics(version)
    assert len(pool) == len(set(pool))
    assert not set(pool) & (pfs.EVENT_RELICS | pfs.PLACEHOLDER_RELICS)


def test_loadout_only_relics_never_overlap_the_swept_ones() -> None:
    assert not pfs.LOADOUT_ONLY_RELICS & set(pfs.ALL_RELICS)
    assert not pfs.LOADOUT_ONLY_RELICS & pfs.EVENT_RELICS


def test_class_potions_are_the_rust_character_locked_ones() -> None:
    assert set(cu.potion_character_locks()) == pfs.CLASS_POTIONS


@pytest.mark.parametrize("version", cu.BUILDS)
def test_loadout_potions_hold_only_the_characters_own_class_potions(version: str) -> None:
    locks = cu.potion_character_locks()
    universe = set(cu.potions(version))
    for character in _characters(version):
        pool = pfs.loadout_potions(character, version)
        own = {p for p, c in locks.items() if c == character} & universe
        foreign = {p for p, c in locks.items() if c != character}
        assert set(pool) <= universe
        assert own <= set(pool), f"{character} cannot be grafted its own class potions"
        assert not set(pool) & foreign
        assert universe - set(pool) == foreign & universe


@pytest.mark.parametrize("version", cu.BUILDS)
def test_generate_loadout_draws_from_the_build_pools(version: str) -> None:
    relics = set(pfs.loadout_relics(version))
    for character in _characters(version):
        potions = set(pfs.loadout_potions(character, version))
        for seed in range(30):
            lo = pfs.generate_loadout(str(seed), character, version)
            assert set(lo.relics) <= relics
            assert set(lo.potions) <= potions


def test_a_new_relic_fails_the_partition() -> None:
    """Mutation check: an id the list neither has nor excludes is reported,
    and so is a listed id the build lacks."""
    part = ip.PARTITIONS[0]
    grown = ip.Partition(
        name=part.name,
        listed=part.listed - {"AKABEKO"} | {"NOT_A_RELIC"},
        universe=part.universe,
        excluded=part.excluded,
    )
    found = "\n".join(ip.problems(grown, "v0.111.0"))
    assert "AKABEKO" in found
    assert "NOT_A_RELIC" in found


@pytest.mark.parametrize("version", cu.BUILDS)
def test_context_dependent_events_are_events_of_every_build(version: str) -> None:
    assert set(cu.events(version)) >= CONTEXT_DEPENDENT_EVENTS


@pytest.mark.parametrize("version", cu.BUILDS)
def test_card_packs_partition_the_builds_sweepable_cards(version: str) -> None:
    """Every character's class-card list (``ALL_IC_CARDS``, ...), the colorless
    and event lists: together they cover the build's sweepable cards, and name
    none the build lacks. Exclusions: ``card_inventory.UNSWEEPABLE_RARITIES``."""
    covered: set[str] = set()
    for character in _characters(version):
        covered |= {
            s.coverage_subject.split(":", 1)[1]
            for s in pfs.coverage_scenarios(character, version)
            if s.coverage_subject and s.coverage_subject.startswith("card:")
        }
    expected = sweepable_cards(version)
    assert not expected - covered, sorted(expected - covered)
    lacking = set(engine_cards()) - expected
    if cu.version_tuple(version) < (0, 111, 0):
        lacking |= v0111_only_cards()
    if cu.version_tuple(version) < (0, 113, 0):
        lacking |= v0113_only_cards()
    assert not covered & lacking, sorted(covered & lacking)
    assert not covered - set(engine_cards()), sorted(covered - set(engine_cards()))


@pytest.mark.parametrize("version", ["v0.111.0", "v0.113.0"])
def test_the_engine_grafts_every_loadout_relic_of_the_build(version: str) -> None:
    # The random-loadout harness grafts through `debug_set_loadout`, which once
    # resolved names against the unversioned roster: every v0.113-only relic
    # the build-aware pool drew failed with "unknown relic id".
    import sts2_rs

    rm = sts2_rs.RunManager(7, "Ironclad", game_version=version)
    pool = pfs.loadout_relics(version)
    rm.debug_set_loadout(relics=pool)
    assert set(pool) <= {r.removeprefix("RELIC.") for r in rm.relics()}
