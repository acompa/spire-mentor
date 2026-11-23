"""Hold a Python PPO decision builder to its Rust port (GH #1426 step 2).

The advisor builds its observations with ``sts2_rs.ppo_wire_obs`` when the net
is the Rust one; the builder tests call these so every board they build is
checked through both, field for field (``diag_ppo_wire_parity``'s rules).
"""

from __future__ import annotations

from typing import Any

import realgame_ppo
import sts2_rs
from diag_ppo_wire_parity import compare_decisions


def build_card_select_both(state: dict[str, Any], prev_msg_type: str | None) -> Any:
    """``realgame_ppo._build_card_select``, asserted equal to the Rust builder."""
    decision = realgame_ppo._build_card_select(state, prev_msg_type)
    rust = sts2_rs.ppo_wire_obs(state, "card_select", prev_msg_type)
    diffs = compare_decisions(decision, rust)
    assert not diffs, "the Rust card_select builder differs:\n" + "\n".join(diffs)
    return decision


def build_both(builder: Any, state: dict[str, Any], surface: str) -> Any:
    """``builder(state)``, asserted equal to the Rust builder for ``surface``."""
    decision = builder(state)
    rust = sts2_rs.ppo_wire_obs(state, surface)
    diffs = compare_decisions(decision, rust)
    assert not diffs, f"the Rust {surface} builder differs:\n" + "\n".join(diffs)
    return decision
