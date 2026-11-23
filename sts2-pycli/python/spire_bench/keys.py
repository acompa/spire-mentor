"""Episode keys.

Each episode's randomness is keyed by ``HMAC-SHA256(season_key, "<agent>:<n>")``.
Keys are per agent on purpose: if two agents shared dice, a second account
could scout the seeds for the first.
"""

from __future__ import annotations

import hashlib
import hmac


def episode_key(season_key: bytes, agent_id: str, episode_no: int) -> str:
    """The 32-byte episode key, as the 64 hex chars ``start_run`` takes."""
    if len(season_key) < 32:
        raise ValueError("season key must be at least 32 bytes")
    msg = f"{agent_id}:{episode_no}".encode()
    return hmac.new(season_key, msg, hashlib.sha256).hexdigest()


def episode_label(agent_id: str, episode_no: int) -> str:
    """The public seed label of an episode. It decides nothing by itself."""
    return f"SB-{agent_id}-{episode_no:06d}"
