"""``sts2_rs.RunManager(mode=...)``: one definition of the deck-decision bundle (GH #981).

Without the flags a run auto-resolves every deck mutation the real game asks the
player about (front-of-deck removal, ``master_deck[0]`` transform, rewards-stream
upgrade) -- right for the parity suite, silently wrong for an agent host. The
``mode`` kwarg names the whole bundle so a host cannot set three flags and forget
the fourth; these tests pin that it IS the four flags, and that mixing the two
spellings is refused rather than resolved by a silent precedence rule.
"""

from __future__ import annotations

import pytest

sts2_rs = pytest.importorskip("sts2_rs")

AGENT_FLAGS = {
    "interactive_card_removal": True,
    "interactive_card_upgrade": True,
    "interactive_card_transform": True,
    "direct_event_relic_grant": True,
}


def _rm(**kwargs):
    return sts2_rs.RunManager(seed=7, character="Ironclad", neow_start=True, **kwargs)


def test_default_is_parity_mode() -> None:
    assert _rm().mode == "parity"
    assert _rm(mode="parity").mode == "parity"


def test_agent_mode_sets_the_whole_bundle() -> None:
    assert _rm(mode="agent").mode == "agent"


def test_the_four_deck_flags_alone_read_custom() -> None:
    # Agent mode also plays the Crystal Sphere with the digger, which has no
    # kwarg: the four flags spelled out (what `run_parity`'s C# lockstep
    # passes) keep the oracle's sweep, so they are not agent mode.
    assert _rm(**AGENT_FLAGS).mode == "custom"


def test_a_partial_mix_reads_custom() -> None:
    assert _rm(interactive_card_removal=True).mode == "custom"


def test_agent_mode_plays_like_the_four_deck_flags() -> None:
    """Same engine state either way: the deck-decision part of the bundle is
    exactly the four flags; the bundle's fifth, the Crystal Sphere digger, only
    matters inside a Crystal Sphere."""

    def observe(rm):
        names = ("phase", "neow_offers", "master_deck", "relics", "gold")
        # Some of these are getters, some methods; read each either way.
        return [v() if callable(v := getattr(rm, n)) else v for n in names]

    assert observe(_rm(mode="agent")) == observe(_rm(**AGENT_FLAGS))


def test_potion_swap_is_not_in_the_bundle() -> None:
    # It stays its own kwarg (hosts disagree on it on purpose), so it combines
    # with a mode instead of conflicting.
    assert _rm(mode="agent", interactive_potion_swap=True).mode == "agent"


@pytest.mark.parametrize("mode", ["agent", "parity"])
@pytest.mark.parametrize("flag", sorted(AGENT_FLAGS))
def test_mode_and_an_individual_flag_are_exclusive(mode: str, flag: str) -> None:
    with pytest.raises(ValueError, match="not both"):
        _rm(mode=mode, **{flag: True})


def test_unknown_mode_is_rejected() -> None:
    with pytest.raises(ValueError, match="unknown mode"):
        _rm(mode="interactive")


def test_the_backend_shadowing_the_oracle_keeps_its_crystal_sphere_sweep() -> None:
    """``RustGame(target_convention="csharp")`` is the Rust half of every C#
    lockstep (``act23_state_parity``, ``run_parity``, ``parity_walk``, the
    shadow replays). The oracle plays the Crystal Sphere with its fixed sweep
    (``RunSimulator.PlayCrystalSphereHeadless``), so that backend must not
    build an agent-mode run: the digger clicks other cells and the two sides
    reveal different items (act23 Fool:1007 v0.113.0: C# 50 gold, Rust a rare
    potion). The engine's own lanes stay in agent mode."""
    from workbench.backends.rust_game import RustGame

    shadow = RustGame(target_convention="csharp")
    shadow.start(character="Ironclad", seed="1000", ascension=0)
    # The four deck flags without the digger (see
    # `test_the_four_deck_flags_alone_read_custom`).
    assert shadow.rm.mode == "custom"

    native = RustGame()
    native.start(character="Ironclad", seed="1000", ascension=0)
    assert native.rm.mode == "agent"
