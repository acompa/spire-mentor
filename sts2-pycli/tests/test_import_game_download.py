"""scripts/import_game.py: `--download` and the non-default build's art overlay.

The download itself needs a Steam login, so these pin what can be checked
offline: every supported build has a pinned manifest, the DepotDownloader
command, which builds `--download` imports in which order, and which files
the art overlay takes.
"""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest

_SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "import_game.py"


@pytest.fixture(scope="module")
def ig():
    spec = importlib.util.spec_from_file_location("import_game", _SCRIPT)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_every_supported_build_has_a_pinned_manifest(ig):
    assert set(ig.DEPOT_MANIFESTS) == set(ig.SUPPORTED_VERSIONS)
    assert set(ig.DEPOT_BRANCHES) == set(ig.SUPPORTED_VERSIONS)
    for version, manifest in ig.DEPOT_MANIFESTS.items():
        assert manifest.isdigit(), f"{version}: {manifest!r} is not a depot manifest id"


def test_depot_command_pins_the_manifest_and_never_carries_a_password(ig, tmp_path):
    cmd = ig.depot_command("dd", "v0.111.0", tmp_path, tmp_path / "f.txt", "someone", qr=False)
    assert cmd[cmd.index("-manifest") + 1] == ig.DEPOT_MANIFESTS["v0.111.0"]
    assert cmd[cmd.index("-depot") + 1] == ig.STEAM_DEPOT
    assert cmd[cmd.index("-branch") + 1] == ig.DEPOT_BRANCHES["v0.111.0"]
    assert cmd[cmd.index("-username") + 1] == "someone"
    assert "-password" not in cmd
    qr = ig.depot_command("dd", "v0.111.0", tmp_path, tmp_path / "f.txt", None, qr=True)
    assert "-qr" in qr and "-username" not in qr


def test_download_defaults_to_every_build_default_first(ig, monkeypatch, tmp_path):
    fetched, imported = [], []
    monkeypatch.setattr(ig, "find_depotdownloader", lambda explicit: "dd")
    monkeypatch.setattr(ig, "download", lambda v, *a: fetched.append(v) or tmp_path / v)
    monkeypatch.setattr(
        ig, "import_build", lambda game_dir, tier, args: imported.append(Path(game_dir).name)
    )
    assert ig.main(["--download", "--steam-user", "someone", "--tier", "workbench"]) == 0
    assert fetched == list(ig.SUPPORTED_VERSIONS)
    assert imported == list(ig.SUPPORTED_VERSIONS)
    assert ig.SUPPORTED_VERSIONS[0] == ig.DEFAULT_VERSION


def test_download_needs_a_login(ig):
    with pytest.raises(SystemExit):
        ig.main(["--download"])


def test_a_present_download_is_recognised(ig, tmp_path):
    (tmp_path / "SlayTheSpire2.pck").write_bytes(b"")
    (tmp_path / "data_sts2_windows_x86_64").mkdir()
    (tmp_path / "data_sts2_windows_x86_64" / "sts2.dll").write_bytes(b"")
    (tmp_path / "release_info.json").write_text(json.dumps({"version": "v0.111.0"}))
    assert ig.downloaded(tmp_path, "v0.111.0")
    assert not ig.downloaded(tmp_path, "v0.107.1")


def test_the_overlay_takes_only_what_the_shared_tree_lacks(ig, tmp_path):
    extracted, shared, overlay = (
        tmp_path / "x",
        tmp_path / "shared",
        tmp_path / "shared" / "v0.111.0",
    )
    for rel in (
        "cards/ironclad/bash.png",
        "cards/defect/voltaic.png",
        "relic_atlas/dowsing_rod.png",
    ):
        (extracted / rel).parent.mkdir(parents=True, exist_ok=True)
        (extracted / rel).write_bytes(rel.encode())
    (shared / "cards/ironclad").mkdir(parents=True)
    (shared / "cards/ironclad/bash.png").write_bytes(b"0.107")
    (shared / "cards/defect/beta").mkdir(parents=True)
    (shared / "cards/defect/beta/voltaic.png").write_bytes(b"sketch")

    assert ig.copy_new_art(extracted, shared, overlay) == 2
    assert sorted(p.relative_to(overlay).as_posix() for p in overlay.rglob("*.png")) == [
        "cards/defect/voltaic.png",
        "relic_atlas/dowsing_rod.png",
    ]
    assert (shared / "cards/ironclad/bash.png").read_bytes() == b"0.107"
    assert ig.copy_new_art(extracted, shared, overlay) == 0  # idempotent


def test_only_a_non_default_build_gets_an_overlay_step(ig, tmp_path):
    def names(version):
        return [s.name for s in ig.build_steps(tmp_path, version)]

    assert not any("art overlay" in n for n in names(ig.DEFAULT_VERSION))
    other = [v for v in ig.SUPPORTED_VERSIONS if v != ig.DEFAULT_VERSION]
    for version in other:
        steps = ig.build_steps(tmp_path, version)
        i = next(i for i, s in enumerate(steps) if "art overlay" in s.name)
        assert steps[i].versioned and steps[i].tier == "workbench"
        # Before the bundle step, which lists the overlay in that build's art manifest.
        assert i < next(j for j, s in enumerate(steps) if s.name.startswith("workbench bundle"))


def test_new_creature_scales_keep_every_shared_row(ig, tmp_path: Path) -> None:
    """A later build's creature_scales.json carries only the creatures it added.

    The shared table's rows win even where the build's scene differs: three of
    them hold hand-overridden scales the scene does not author.
    """
    fresh, shared, out = (
        tmp_path / "fresh.json",
        tmp_path / "shared.json",
        tmp_path / "v/creature_scales.json",
    )
    fresh.write_text(
        json.dumps(
            {
                "fool": {"scale": 0.34, "bounds_w": 285.0, "bounds_h": 327.0},
                "mawler": {"scale": 1.0, "bounds_w": 1.0, "bounds_h": 1.0},
            }
        ),
        encoding="utf-8",
    )
    shared.write_text(
        json.dumps({"mawler": {"scale": 0.5, "bounds_w": 9.0, "bounds_h": 9.0}}), encoding="utf-8"
    )

    assert ig.write_new_creature_scales(fresh, shared, out) == 1
    assert json.loads(out.read_text(encoding="utf-8")) == {
        "fool": {"scale": 0.34, "bounds_w": 285.0, "bounds_h": 327.0},
    }


def test_added_audio_records_what_the_shared_tree_lacked_and_only_grows(ig, tmp_path: Path) -> None:
    """A later build's sound goes into the shared audio tree; the record is
    what still says which files that build added (the default build's asset
    manifest leaves them out).

    Mutation caught: writing the record after the copy (it comes out empty),
    or overwriting it on a re-run (the merged files drop off it).
    """
    encoded, shared, record = tmp_path / "enc", tmp_path / "audio", tmp_path / "v/added_audio.json"
    for rel in ("sfx/block_gain.webm", "sfx/characters/fool/fool_shot.webm"):
        (encoded / rel).parent.mkdir(parents=True, exist_ok=True)
        (encoded / rel).write_bytes(rel.encode())
    (shared / "sfx").mkdir(parents=True)
    (shared / "sfx/block_gain.webm").write_bytes(b"0.107")

    assert ig.record_added_audio(encoded, shared, record) == ["sfx/characters/fool/fool_shot.webm"]
    ig.copy_new_art(encoded, shared, shared)
    # Re-run after the merge: nothing is new any more, the record keeps it.
    assert ig.record_added_audio(encoded, shared, record) == ["sfx/characters/fool/fool_shot.webm"]
    assert json.loads(record.read_text(encoding="utf-8")) == ["sfx/characters/fool/fool_shot.webm"]


def test_a_moved_arrow_anchor_brings_the_shared_row_along(ig, tmp_path: Path) -> None:
    """v0.113.0 gave the Mecha Knight an `ArrowPosition` marker. The bundle
    replaces whole rows, so the overlay carries the shared row with the new
    anchor; a creature whose anchor did not move stays out of it."""
    fresh, shared, out = (
        tmp_path / "fresh.json",
        tmp_path / "shared.json",
        tmp_path / "v/creature_scales.json",
    )
    fresh.write_text(
        json.dumps(
            {
                "mecha_knight": {
                    "scale": 0.15,
                    "bounds_w": 660.0,
                    "bounds_h": 502.0,
                    "arrow_x": -118.0,
                    "arrow_y": -178.0,
                },
                "nibbit": {
                    "scale": 0.28,
                    "bounds_w": 270.0,
                    "bounds_h": 146.0,
                    "arrow_x": 0.0,
                    "arrow_y": -72.0,
                },
            }
        ),
        encoding="utf-8",
    )
    shared.write_text(
        json.dumps(
            {
                "mecha_knight": {
                    "scale": 0.2,
                    "bounds_w": 660.0,
                    "bounds_h": 502.0,
                    "arrow_x": 23.0,
                    "arrow_y": -355.0,
                },
                "nibbit": {
                    "scale": 0.28,
                    "bounds_w": 270.0,
                    "bounds_h": 146.0,
                    "arrow_x": 0.0,
                    "arrow_y": -72.0,
                },
            }
        ),
        encoding="utf-8",
    )
    assert ig.write_new_creature_scales(fresh, shared, out) == 1
    assert json.loads(out.read_text(encoding="utf-8")) == {
        "mecha_knight": {
            "scale": 0.2,
            "bounds_w": 660.0,
            "bounds_h": 502.0,
            "arrow_x": -118.0,
            "arrow_y": -178.0,
        },
    }


def test_a_creature_whose_authored_scale_moved_gets_this_builds_scale(ig, tmp_path: Path) -> None:
    """v0.111.1 swapped the Aeonglass's static hourglass (scene scale 0.29) for
    a Spine rig drawn at 0.2; the shared row is the default build's. A creature
    whose scene kept its scale keeps its shared, hand-set row."""
    fresh, shared, default, out = (
        tmp_path / n for n in ("fresh.json", "shared.json", "default.json", "out.json")
    )
    row = {"bounds_w": 428.0, "bounds_h": 527.0, "arrow_x": 5.0, "arrow_y": -304.0}
    fresh.write_text(
        json.dumps(
            {"aeonglass": {**row, "scale": 0.2}, "test_subject": {**row, "scale": 0.90000004}}
        ),
        encoding="utf-8",
    )
    default.write_text(
        json.dumps(
            {"aeonglass": {**row, "scale": 0.29032356}, "test_subject": {**row, "scale": 0.9}}
        ),
        encoding="utf-8",
    )
    shared.write_text(
        json.dumps(
            {"aeonglass": {**row, "scale": 0.29032356}, "test_subject": {**row, "scale": 0.3}}
        ),
        encoding="utf-8",
    )
    assert ig.write_new_creature_scales(fresh, shared, out, default) == 1
    assert json.loads(out.read_text(encoding="utf-8")) == {"aeonglass": {**row, "scale": 0.2}}


def test_a_hand_set_overlay_row_survives_a_reimport(ig, tmp_path: Path) -> None:
    """The v0.113.0 Mawler's 0.25 was set by hand for its re-exported rig (no
    rule derives it); re-running the import must not drop it."""
    fresh, shared, out = (tmp_path / n for n in ("fresh.json", "shared.json", "out.json"))
    fresh.write_text(
        json.dumps({"mawler": {"scale": 0.5, "bounds_w": 510.0, "bounds_h": 327.0}}),
        encoding="utf-8",
    )
    shared.write_text(
        json.dumps({"mawler": {"scale": 0.5, "bounds_w": 510.0, "bounds_h": 327.0}}),
        encoding="utf-8",
    )
    out.write_text(
        json.dumps({"mawler": {"scale": 0.25, "bounds_w": 510.0, "bounds_h": 327.0}}),
        encoding="utf-8",
    )
    assert ig.write_new_creature_scales(fresh, shared, out) == 1
    assert json.loads(out.read_text(encoding="utf-8"))["mawler"]["scale"] == pytest.approx(0.25)


def test_rigs_go_over_the_shared_copy(ig, tmp_path: Path) -> None:
    """`extract_rig --new-since` keeps the rigs a build redrew; the shared tree
    has the default build's copy, which `copy_new_art` skips. A rig that only
    gained the arrow slots renders the default build's still, which stays shared."""
    extracted, baseline = tmp_path / "x", tmp_path / "baseline"
    overlay = tmp_path / "shared" / "v0.113.0"
    rig = extracted / "monster_rigs/nibbit/nibbit.skel"
    rig.parent.mkdir(parents=True)
    rig.write_bytes(b"ARROW_HIT_FRONT")
    (extracted / "monsters").mkdir()
    (extracted / "monsters/nibbit.png").write_bytes(b"portrait")
    (baseline / "monsters").mkdir(parents=True)
    (baseline / "monsters/nibbit.png").write_bytes(b"portrait")
    assert ig.copy_rigs(extracted, baseline, overlay) == 1
    assert (overlay / "monster_rigs/nibbit/nibbit.skel").read_bytes() == b"ARROW_HIT_FRONT"
    assert not (overlay / "monsters").exists()
    assert ig.copy_rigs(extracted, baseline, overlay) == 0  # idempotent


def test_a_redrawn_rig_brings_its_portrait(ig, tmp_path: Path) -> None:
    """v0.111.1 redrew the Ironclad: its overlay needs the rig AND the
    character-select still rendered from it, or the build shows the old one.
    The baseline is the default build's rig rendered by the same renderer."""
    extracted, baseline = tmp_path / "x", tmp_path / "baseline"
    overlay = tmp_path / "shared" / "v0.111.0"
    for rel, data in (
        ("character_rigs/ironclad/ironclad.skel", b"new rig"),
        ("character_sprites/ironclad.png", b"new still"),
        ("room_rigs/shop_merchant_top/shop_merchant_top.skel", b"new room"),
    ):
        (extracted / rel).parent.mkdir(parents=True, exist_ok=True)
        (extracted / rel).write_bytes(data)
    (baseline / "character_sprites").mkdir(parents=True)
    (baseline / "character_sprites/ironclad.png").write_bytes(b"old still")
    assert ig.copy_rigs(extracted, baseline, overlay) == 3
    assert (overlay / "character_sprites/ironclad.png").read_bytes() == b"new still"
    assert (overlay / "room_rigs/shop_merchant_top/shop_merchant_top.skel").is_file()
