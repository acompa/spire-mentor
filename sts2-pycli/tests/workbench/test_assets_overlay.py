"""A game build's art overlay (GH #801): ``images/<version>/…`` is the art that
build added or redrew. Its resolver tries the overlay first; the default
build's resolver and manifest never see it."""

from __future__ import annotations

import sys
from pathlib import Path

from workbench.assets import AssetResolver

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
from build_asset_manifest import added_audio_of, build_manifest, overlay_versions


def _tree(root: Path) -> Path:
    for rel in (
        "card_atlas/defect/beta/voltaic.png",
        "v0.111.0/card_atlas/defect/voltaic.png",
        "v0.111.0/relic_atlas/dowsing_rod.png",
    ):
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        (root / rel).write_bytes(b"png")
    return root


def test_the_overlay_wins_only_for_its_build(tmp_path: Path) -> None:
    root = _tree(tmp_path)
    base = AssetResolver(root)
    beta = AssetResolver(root, overlay="v0.111.0")
    assert base.card("Defect", "CARD.VOLTAIC", "Voltaic") == "card_atlas/defect/beta/voltaic.png"
    assert (
        beta.card("Defect", "CARD.VOLTAIC", "Voltaic") == "v0.111.0/card_atlas/defect/voltaic.png"
    )
    assert base.relic("Dowsing Rod") is None
    assert beta.relic("Dowsing Rod") == "v0.111.0/relic_atlas/dowsing_rod.png"


def test_manifest_mode_matches_filesystem_mode(tmp_path: Path) -> None:
    root = _tree(tmp_path)
    manifest = set(build_manifest(root, overlays=["v0.111.0"]))
    beta = AssetResolver(root, manifest=manifest, overlay="v0.111.0")
    assert (
        beta.card("Defect", "CARD.VOLTAIC", "Voltaic") == "v0.111.0/card_atlas/defect/voltaic.png"
    )


def test_the_default_manifest_lists_no_overlay(tmp_path: Path) -> None:
    root = _tree(tmp_path)
    assert overlay_versions(root) == ["v0.111.0"]
    assert build_manifest(root) == ["card_atlas/defect/beta/voltaic.png"]
    assert "v0.111.0/relic_atlas/dowsing_rod.png" in build_manifest(root, overlays=["v0.111.0"])


def test_a_rig_only_the_overlay_has_resolves_to_the_overlay(tmp_path: Path) -> None:
    """The Fool and the v0.113.0 monsters ship their rigs in the overlay only.

    Mutation caught: the rig lookups returning the bare ``character_rigs/…``
    path after finding the ``.skel`` under the overlay — a URL that 404s.
    """
    for rel in (
        "v0.113.0/character_rigs/fool/fool.skel",
        "v0.113.0/monster_rigs/wuppy/wuppy.skel",
        "monster_rigs/nibbit/nibbit.skel",
    ):
        (tmp_path / rel).parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / rel).write_bytes(b"skel")
    (tmp_path / "creature_scales.json").write_text('{"nibbit": {"scale": 0.2}}', encoding="utf-8")
    (tmp_path / "v0.113.0/creature_scales.json").write_text(
        '{"fool": {"scale": 0.34}, "nibbit": {"scale": 9.0}}', encoding="utf-8"
    )
    base = AssetResolver(tmp_path)
    new = AssetResolver(tmp_path, overlay="v0.113.0")
    assert base.character_rig("Fool") is None
    assert new.character_rig("Fool") == "v0.113.0/character_rigs/fool/fool"
    assert new.monster_rig("Wuppy") == "v0.113.0/monster_rigs/wuppy/wuppy"
    assert new.monster_rig("Nibbit") == "monster_rigs/nibbit/nibbit"
    # The overlay adds the new creatures' scales; a shared row is never replaced.
    assert new.character_rig_scale("Fool") == {"scale": 0.34}
    assert new.monster_rig_scale("Nibbit") == {"scale": 0.2}
    assert base.character_rig_scale("Fool") is None


def test_a_later_builds_sound_stays_out_of_the_default_manifest(tmp_path: Path) -> None:
    """The Fool's voice lives in the shared audio tree (there is no audio
    overlay), so the default build's manifest leaves out what the v0.113.0
    import recorded as its own; that build's manifest lists it.

    Mutation caught: ``exclude_audio`` ignored, so importing v0.113.0's sound
    moved the default bundle and a default-only import read as incomplete.
    """
    images, audio, wiki = tmp_path / "images", tmp_path / "audio", tmp_path / "data"
    images.mkdir()
    for rel in ("sfx/block_gain.webm", "sfx/characters/fool/fool_shot.webm"):
        (audio / rel).parent.mkdir(parents=True, exist_ok=True)
        (audio / rel).write_bytes(b"webm")
    (wiki / "v0.113.0").mkdir(parents=True)
    (wiki / "v0.113.0/added_audio.json").write_text(
        '["sfx/characters/fool/fool_shot.webm"]', encoding="utf-8"
    )

    added = added_audio_of(wiki, ["v0.111.0", "v0.113.0"])
    assert added == {"sfx/characters/fool/fool_shot.webm"}
    assert build_manifest(images, audio, exclude_audio=added) == ["audio/sfx/block_gain.webm"]
    assert "audio/sfx/characters/fool/fool_shot.webm" in build_manifest(images, audio)
