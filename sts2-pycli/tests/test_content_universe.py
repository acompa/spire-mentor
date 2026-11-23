"""The Rust-side content universe matches the C# decompilation (GH #1500).

`content_universe` is what `tests/test_id_partitions.py` checks the parity id
lists against; this checks the universe itself, name for name, against the
concrete model classes of each build's decomp, so it cannot go stale either
(a new ``V0115_ONLY`` the parser does not know about fails here).

Reads ``sts2-wiki/decompiled/``, which the public export leaves out
(`conftest._NEEDS`).
"""

from __future__ import annotations

import content_universe as cu
import pytest
from event_parity import CONTEXT_DEPENDENT_EVENTS

#: Decomp classes in a content folder that are not content, with the reason.
_NOT_CONTENT: dict[str, dict[str, str]] = {
    "Relics": {"VAKUU_CARD_SELECTOR": "an ICardSelector helper for Vakuu's auto-play"},
    "Potions": {"DEPRECATED_POTION": "the placeholder for retired potion ids"},
    "Encounters": {"DEPRECATED_ENCOUNTER": "the placeholder for retired encounter ids"},
}

#: Event combats. The engine runs them through `sts2_core::events::EventEncounter`,
#: not the encounter registry, so they are not `content_universe.encounters()`.
_EVENT_ENCOUNTERS = frozenset(
    {
        "BATTLEWORN_DUMMY_EVENT_ENCOUNTER",
        "BATTLEWORN_DUMMY_EVENT_V1_ENCOUNTER",
        "BATTLEWORN_DUMMY_EVENT_V2_ENCOUNTER",
        "BATTLEWORN_DUMMY_EVENT_V3_ENCOUNTER",
        "DENSE_VEGETATION_EVENT_ENCOUNTER",
        "FAKE_MERCHANT_EVENT_ENCOUNTER",
        "MYSTERIOUS_KNIGHT_EVENT_ENCOUNTER",
        "PUNCH_OFF_EVENT_ENCOUNTER",
        "THE_ARCHITECT_EVENT_ENCOUNTER",
    }
)

#: Engine events the C# has no class for: the engine's run-start Neow boon flow
#: (the C# runs Neow as an Ancient event).
_ENGINE_ONLY_EVENTS = frozenset({"NeowsBlessing"})


@pytest.mark.parametrize("version", cu.BUILDS)
@pytest.mark.parametrize(
    ("kind", "universe"),
    [("Relics", cu.relics), ("Potions", cu.potions)],
    ids=["relics", "potions"],
)
def test_universe_is_the_decomps_model_classes(kind: str, universe, version: str) -> None:
    decomp = set(cu.decomp_classes(version, kind).values()) - set(_NOT_CONTENT[kind])
    ours = set(universe(version))
    assert sorted(decomp - ours) == [], f"{version} {kind} the engine universe lacks"
    assert sorted(ours - decomp) == [], f"{version} {kind} the decomp does not have"


@pytest.mark.parametrize("version", cu.BUILDS)
def test_events_are_the_decomps_event_classes(version: str) -> None:
    decomp = set(cu.decomp_classes(version, "Events"))
    ours = set(cu.events(version))
    assert sorted(decomp - ours) == []
    assert ours - decomp == _ENGINE_ONLY_EVENTS


@pytest.mark.parametrize("version", cu.BUILDS)
def test_encounters_are_the_decomps_encounter_classes(version: str) -> None:
    decomp = set(cu.decomp_classes(version, "Encounters").values()) - set(
        _NOT_CONTENT["Encounters"]
    )
    ours = set(cu.encounters())
    assert sorted(ours - decomp) == [], f"engine encounters {version} does not have"
    assert sorted(decomp - ours - _EVENT_ENCOUNTERS) == []


@pytest.mark.parametrize("version", cu.BUILDS)
def test_context_dependent_events_are_gated_by_is_allowed(version: str) -> None:
    """`event_parity.CONTEXT_DEPENDENT_EVENTS` is the events whose `IsAllowed`
    precondition force-entry bypasses; one without an `IsAllowed` override is
    there for no reason and hides a real option diff."""
    folder = cu._DECOMP_ROOT / cu.DECOMP_DIR[version] / "MegaCrit.Sts2.Core.Models.Events"
    for event in sorted(CONTEXT_DEPENDENT_EVENTS):
        text = (folder / f"{event}.cs").read_text(encoding="utf-8")
        assert "IsAllowed" in text, f"{event} has no IsAllowed override in {version}"


def test_slugify_matches_the_csharp() -> None:
    assert cu.slugify("BurningBlood") == "BURNING_BLOOD"
    assert cu.slugify("WongoCustomerAppreciationBadge") == "WONGO_CUSTOMER_APPRECIATION_BADGE"
    assert cu.slugify("BattlewornDummyEventV1Encounter") == "BATTLEWORN_DUMMY_EVENT_V1_ENCOUNTER"
