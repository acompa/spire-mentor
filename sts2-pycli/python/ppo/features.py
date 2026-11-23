"""The run-policy encoder's optional features, importable without numpy.

Split out of :mod:`ppo.schema` (which imports numpy at module scope) for the
same reason :mod:`ppo.errors` was: the advisor's live path asks which
features a champion was trained with (``realgame_advisor._trained_on_packs``,
the Scroll Boxes ``card_bundle`` screen), and the packaged advisor ships no
numpy. Found by the advisor replay trace (GH #1426): that screen imported
``ppo.schema`` and so could only answer with an error in the frozen exe.

``ppo.schema`` re-exports everything here, so every existing import site
stays valid.
"""

from __future__ import annotations

#: Candidate encoder inputs, off by default. See :mod:`ppo.schema`'s docstring.
REWARD_UPGRADE = "reward_upgrade"
CARD_MODS = "card_mods"
CARD_BUNDLES = "card_bundles"
CARD_KIND = "card_kind"
KNOWN_FEATURES: frozenset[str] = frozenset({REWARD_UPGRADE, CARD_MODS, CARD_BUNDLES, CARD_KIND})


def parse_features(spec: str) -> frozenset[str]:
    """``"foo,bar"`` → ``{"foo", "bar"}``; raises on an unknown name.

    Failing loud on a typo is the point: a misspelled feature would otherwise
    screen as a null and be recorded as "this idea does not help".
    """
    names = frozenset(t.strip() for t in spec.split(",") if t.strip())
    unknown = names - KNOWN_FEATURES
    if unknown:
        raise ValueError(f"unknown STS2_PPO_FEATURES entries {sorted(unknown)}")
    return names
