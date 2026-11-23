"""The reply chain behind every fingerprint (the agent's receipt).

The chain starts from a digest of the episode's identity (the header fields
that decide what the episode is and how it is judged), then takes the sha256
of every reply, then, for a loss by a host failure during an action, the
digest of the triggering command. Changing any of them changes the
fingerprint the agent holds as its receipt.
"""

from __future__ import annotations

import hashlib
import json
from typing import Any

IDENTITY_FIELDS = (
    "protocol",
    "episode_id",
    "agent_id",
    "episode_no",
    "character",
    "ascension",
    "label",
    "game_version",
    "engine_hash",
    "limits",
    "season_id",
)


def digest(obj: Any) -> str:
    return hashlib.sha256(json.dumps(obj, sort_keys=True).encode()).hexdigest()


def new_chain(header: dict[str, Any]) -> Any:
    """A chain seeded with the header's identity."""
    chain = hashlib.sha256()
    identity = {k: header.get(k) for k in IDENTITY_FIELDS}
    chain.update(("identity:" + digest(identity)).encode())
    return chain


def add_reply(chain: Any, reply_sha256: str) -> None:
    chain.update(reply_sha256.encode())


def add_trigger(chain: Any, trigger: dict[str, Any]) -> None:
    chain.update(("trigger:" + digest(trigger)).encode())
