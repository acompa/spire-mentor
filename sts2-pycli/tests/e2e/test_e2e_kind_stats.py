"""`learner._kind_stats`: the new option kinds (GH #1604/#1606), offered and chosen."""

from collections import Counter

import numpy as np
from ppo.e2e.actors import META_COLS
from ppo.e2e.learner import _kind_stats


def test_kind_stats_counts_offered_and_chosen_on_live_rows() -> None:
    # Row 0: kinds [1, 54, 55], picks 54. Row 1: [5, 5], picks 1 (kind 5).
    # Row 2: [55, 2], picks 1, but is a bootstrap row (not counted).
    rb = {
        "n_tok": np.ones(3, np.int64),
        "n_classes": np.array([3, 2, 2], np.int64),
        "cls_kind": np.array([1, 54, 55, 5, 5, 55, 2], np.uint8),
    }
    metas = np.zeros((3, len(META_COLS)), np.int64)
    metas[2, META_COLS.index("boot")] = 1
    out: Counter = Counter()
    _kind_stats(rb, metas, np.array([1, 1, 1]), out)
    assert out["use_potion_ooc_offered"] == 1
    assert out["use_potion_ooc_chosen"] == 1
    assert out["discard_potion_offered"] == 1
    assert out["discard_potion_chosen"] == 0
    assert out["potion_target_offered"] == 1
    assert out["potion_target_chosen"] == 1
