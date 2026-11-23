"""Path rules in `extract_rig` (atlas page sizes) and `extract_rooms` (outputs)."""

import json
import os
import tempfile
import unittest

from scripts.extract_rig import _page_sizes, new_rigs, rigs_changed
from scripts.extract_rooms import aliases_for, output_for


class TestAtlasPageSizes(unittest.TestCase):
    """The atlas's own page sizes pick the right texture when an unrelated
    imported texture shares the page's name (battleworn_dummy.png is both the
    396x502 rig page and the 3440x1613 event illustration)."""

    def test_reads_each_page_size(self):
        atlas = [
            "battleworn_dummy.png",
            "size:396,502",
            "filter:Linear,Linear",
            "arm",
            "bounds:1,2,3,4",
            "",
            "battleworn_dummy_2.png",
            "size: 80, 186",
            "leg",
        ]
        self.assertEqual(
            _page_sizes(atlas),
            {"battleworn_dummy.png": (396, 502), "battleworn_dummy_2.png": (80, 186)},
        )

    def test_region_sizes_are_not_page_sizes(self):
        atlas = ["a.png", "filter:Linear,Linear", "region", "size:9,9"]
        self.assertEqual(_page_sizes(atlas), {})


class TestRoomOutputs(unittest.TestCase):
    def test_ancient_portraits_are_the_map_node_icons(self):
        rel = output_for("images/packed/map/ancients/ancient_node_neow.png")
        self.assertEqual(rel, "ui/map/ancients/ancient_node_neow.png")
        self.assertEqual(aliases_for(rel), ["ancients/neow.png"])
        self.assertEqual(aliases_for("ui/map/ancients/ancient_node_neow_outline.png"), [])

    def test_ancient_scene_backdrops_are_not_portraits(self):
        self.assertIsNone(output_for("images/ancients/darv_placeholder.png"))

    def test_static_monster_sprites(self):
        self.assertEqual(
            output_for("images/monsters/hourglass_placeholder.png"),
            "monsters/hourglass_placeholder.png",
        )
        self.assertIsNone(output_for("images/monsters/beta/the_spear.png"))

    def test_fool_combat_ui(self):
        # The bow + quiver (fool_combat_ui.tscn) and the impaled-arrow halves
        # (impaled_arrow_vfx.tscn) — and no other images/vfx/ file.
        self.assertEqual(output_for("images/packed/fool/bow.png"), "ui/fool/bow.png")
        self.assertEqual(
            output_for("images/packed/fool/arrow_stroke.png"), "ui/fool/arrow_stroke.png"
        )
        self.assertEqual(output_for("images/vfx/arrow_fg.png"), "ui/fool/arrow_fg.png")
        self.assertEqual(output_for("images/vfx/arrow_bg.png"), "ui/fool/arrow_bg.png")
        self.assertIsNone(output_for("images/vfx/arrow_trail.png"))


class TestNewRigs(unittest.TestCase):
    def test_keeps_only_rigs_whose_skeleton_the_older_build_lacks(self):
        # A later build's overlay carries the rigs it ADDED. `axe_bot` is an
        # alias folder for a skeleton the older build already has: a folder
        # diff would wrongly call it new.
        rigs = {
            ("character_rigs", "fool"): ("fool", "fool"),
            ("monster_rigs", "axe_bot"): ("axebot", "axebot"),
            ("monster_rigs", "axebot"): ("axebot", "axebot"),
        }
        old_index = {".godot/imported/axebot.skel-0123abcd.spskel": (0, 10)}
        self.assertEqual(new_rigs(rigs, old_index), {("character_rigs", "fool"): ("fool", "fool")})

    def test_keeps_rigs_the_later_build_redrew(self):
        # v0.111.1 redrew the Ironclad; that build's overlay needs its copy.
        rigs = {
            ("character_rigs", "ironclad"): ("ironclad", "ironclad"),
            ("monster_rigs", "axebot"): ("axebot", "axebot"),
        }
        old_index = {
            ".godot/imported/ironclad.skel-0123abcd.spskel": (0, 10),
            ".godot/imported/axebot.skel-0123abcd.spskel": (0, 10),
        }
        self.assertEqual(
            new_rigs(rigs, old_index, changed=lambda skel, atlas: skel == "ironclad"),
            {("character_rigs", "ironclad"): ("ironclad", "ironclad")},
        )


class TestRigsChanged(unittest.TestCase):
    """``rigs_changed`` compares the skeleton, the atlas and every texture page."""

    def _pck(self, tmp, name, files):
        # A stand-in pck: the files back to back; the index maps res paths to them.
        path, index, blob = os.path.join(tmp, name), {}, b""
        for res, data in files.items():
            index[res] = (len(blob), len(data))
            blob += data
        with open(path, "wb") as f:
            f.write(blob)
        return path, index

    def _rig(self, skel=b"skel", page2=b"page-2"):
        atlas = json.dumps({"atlas_data": "rig.png\nsize:4,4\nrig_2.png\nsize:4,4\n"}).encode()
        return {
            ".godot/imported/rig.skel-1.spskel": skel,
            ".godot/imported/rig.atlas-1.spatlas": atlas,
            ".godot/imported/rig.png-1.ctex": b"page-1",
            ".godot/imported/rig_2.png-1.ctex": page2,
        }

    def test_same_bytes_are_unchanged_and_any_part_is_a_change(self):
        with tempfile.TemporaryDirectory() as tmp:
            old, old_index = self._pck(tmp, "old", self._rig())
            for label, files, want in (
                ("identical", self._rig(), False),
                ("new skeleton", self._rig(skel=b"skel ARROW_HIT_FRONT"), True),
                ("second page redrawn", self._rig(page2=b"page-2 redrawn"), True),
            ):
                new, index = self._pck(tmp, label, files)
                with self.subTest(label):
                    self.assertIs(rigs_changed(new, index, old, old_index)("rig", "rig"), want)

    def test_a_rig_the_older_build_lacks_is_not_a_change(self):
        # new_rigs keeps it anyway, as an added rig.
        with tempfile.TemporaryDirectory() as tmp:
            old, old_index = self._pck(tmp, "old", {})
            new, index = self._pck(tmp, "new", self._rig())
            self.assertFalse(rigs_changed(new, index, old, old_index)("rig", "rig"))


if __name__ == "__main__":
    unittest.main()
