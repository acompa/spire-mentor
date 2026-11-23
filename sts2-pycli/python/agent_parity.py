"""Agent-mode parity lane: compare the OFFERED ACTION SETS, not just states.

GH #1610. The parity drivers (``run_parity.py``, ``act23_state_parity.py``)
run the C# headless host in its auto-collect default and the Rust engine in
parity mode, and pick every move off the C# decision view. Choices the real
game offers but the host auto-resolves (reward take/skip/order, the treasure
skip, Entropy's pick) are never exercised, and an action only Rust offers is
never tested (``docs/experiments/2026-11-20-engine-choice-gap-audit.md``,
"Why parity did not catch these").

This lane runs:

* the C# host with its interactive modes (``STS2_INTERACTIVE_REWARDS``,
  ``STS2_INTERACTIVE_TURN_START_PICKS``, ``STS2_TRIAL_DOUBLE_DOWN_ABANDONS``,
  ``STS2_CRYSTAL_SPHERE_INTERACTIVE``) and ``interactive_potion_swap``;
* Rust in full agent mode (``RunManager(mode="agent")`` +
  ``interactive_potion_swap`` + ``ooc_potion_actions`` +
  ``deck_select_screens`` + ``reward_screen``), the engine the e2e net trains
  on plus the game's cancelable and pick-N deck selects (GH #1619, #1621) and
  its reward screen (GH #1620).

At every decision both sides' offers are normalised to keys that carry the
kind plus the identities a player sees (card / relic / potion ids, upgrade
flags, prices, targets): :func:`offers_from_view` builds them from a decision
view, the same derivation for both sides (the Rust backend serialises the C#
shape, built from the engine's own legal actions), plus the out-of-combat
potion actions (``potion_belt`` on C#, the engine's ``use_potion_ooc`` /
``discard_potion`` on Rust). :func:`diff_offers` reports the keys only one
side offers; :func:`classify` names the known gaps (an allowlist, each entry
citing its issue). A seeded random walk picks from the INTERSECTION, so a
move only one side allows is reported but never played.

Where one side resolves on its own what the other asks about (an event's own
reward screen Rust collects itself, an Entropy pick Rust makes itself), the lane
advances the asking side alone, as the parity drivers do, and counts it under
the gap's name.

A Rust-only select (the game resolved it itself) and a game-only select
(Rust picked itself) are driven on the asking side alone. A game-only one is
named by :func:`classify_cs_alone_select`; a Rust-only one has no known gap
left since #1623 / #1624 closed, so it is always NEW.

Run ``just parity-agent [n] [character|all] [build]`` (``--strict``: exit 1 on
a NEW difference or a broken lockstep). Results of the first sample are in
``docs/experiments/2026-11-20-agent-parity-lane.md``.
"""

from __future__ import annotations

import argparse
import concurrent.futures as _cf
import json
import os
import pathlib
import random
import re
import sys
from collections import Counter
from collections.abc import Iterable
from dataclasses import asdict, dataclass, field
from typing import Any

sys.path.insert(0, "python")

# The C# host reads these once, when the process starts; set them before any
# ``Game`` is spawned (pool workers inherit them).
HOST_ENV = {
    "STS2_INTERACTIVE_REWARDS": "1",
    "STS2_INTERACTIVE_TURN_START_PICKS": "1",
    "STS2_TRIAL_DOUBLE_DOWN_ABANDONS": "1",
    "STS2_CRYSTAL_SPHERE_INTERACTIVE": "1",
}

Key = tuple[Any, ...]

# Decisions on which the potion belt is clickable out of combat
# (``RefereeView.PotionScreens``).
POTION_SCREENS = frozenset({"map_select", "event_choice", "rest_site", "shop", "rewards"})
# Card-pick overlays: the host keeps the belt shut while one is open.
OVERLAY_SCREENS = frozenset({"card_reward", "card_select", "bundle_select", "potion_swap"})


# --------------------------------------------------------------------------- #
# Normalisation
# --------------------------------------------------------------------------- #
def canon(s: Any) -> str:
    """Alnum-uppercase key: ``"CARD.STRIKE_IRONCLAD"`` -> ``"STRIKEIRONCLAD"``."""
    raw = str(s or "")
    for prefix in ("CARD.", "RELIC.", "POTION."):
        if raw.upper().startswith(prefix):
            raw = raw[len(prefix) :]
    raw = re.sub(r"\.title$", "", raw)
    return re.sub(r"[^A-Z0-9]", "", raw.upper())


def _card_key(c: dict[str, Any]) -> tuple[str, bool]:
    return canon(c.get("id") or c.get("name")), bool(c.get("upgraded"))


def _int(v: Any, default: int = 0) -> int:
    try:
        return int(v)
    except (TypeError, ValueError):
        return default


@dataclass
class Offer:
    """One offered action: its comparison key and the verb that plays it."""

    key: Key
    verb: str
    kw: dict[str, Any] = field(default_factory=dict)
    raw: dict[str, Any] | None = None  # a raw Rust action, applied directly
    # What the player reads (an event option's title). Not compared: the
    # engine's titles are developer shorthand ("Eat" for "Consume").
    label: str = ""


def _potion_ids(view: dict[str, Any]) -> dict[int, str]:
    out: dict[int, str] = {}
    pots = (view.get("player") or {}).get("potions") or []
    for i, p in enumerate(pots):
        if not isinstance(p, dict):
            continue
        idx = _int(p.get("index"), i)
        out[idx] = canon(p.get("entry") or p.get("id") or p.get("name"))
    return out


def _hand_ids(view: dict[str, Any]) -> dict[int, tuple[str, bool]]:
    out: dict[int, tuple[str, bool]] = {}
    for i, c in enumerate(view.get("hand") or []):
        if isinstance(c, dict):
            out[_int(c.get("index"), i)] = _card_key(c)
    return out


def combat_offers(view: dict[str, Any], legal: Iterable[dict[str, Any]]) -> list[Offer]:
    """Offers of a ``combat_play`` view from its C#-shaped legal list
    (``{"action": "play_card", "card_index", "target_index"}`` /
    ``use_potion`` / ``end_turn``; target over the alive enemies)."""
    hand, pots = _hand_ids(view), _potion_ids(view)
    out: list[Offer] = []
    for a in legal:
        verb = a.get("action")
        kw = {k: v for k, v in a.items() if k != "action"}
        if verb == "play_card":
            cid, up = hand.get(_int(a.get("card_index")), ("?", False))
            out.append(Offer(("play_card", cid, up, a.get("target_index")), verb, kw))
        elif verb == "use_potion":
            pid = pots.get(_int(a.get("potion_index")), "?")
            out.append(Offer(("use_potion", pid, a.get("target_index")), verb, kw))
        elif verb == "end_turn":
            out.append(Offer(("end_turn",), verb, kw))
        elif verb == "discard_potion":
            pid = pots.get(_int(a.get("potion_index")), "?")
            out.append(Offer(("discard_potion", pid), verb, kw))
        else:
            out.append(Offer(("combat_other", str(verb)), str(verb), kw))
    return out


def offers_from_view(view: dict[str, Any]) -> list[Offer]:
    """Every out-of-combat offer a C#-shaped decision view lists.

    Mirrors ``RefereeView.LegalActions`` (the host's own "what may a client
    send" list), with identities in the key instead of list indices.
    """
    dec = view.get("decision") or view.get("type")
    gold = _int((view.get("player") or {}).get("gold"))
    out: list[Offer] = []

    def items(name: str) -> list[dict[str, Any]]:
        return [x for x in (view.get(name) or []) if isinstance(x, dict)]

    if dec == "map_select":
        for c in items("choices"):
            out.append(
                # Column only: the engine's view leaves ``row`` out, and the
                # row of every choice is the next row anyway.
                Offer(
                    ("map", c.get("col")),
                    "select_map_node",
                    {"col": c.get("col"), "row": c.get("row")},
                )
            )
    elif dec == "event_choice":
        for o in items("options"):
            if o.get("is_locked"):
                continue
            label = canon(o.get("title") or o.get("text_key"))
            out.append(
                Offer(
                    ("event_option", _int(o.get("index"))),
                    "choose_option",
                    {"option_index": o.get("index")},
                    label=label,
                )
            )
        if not out:
            out.append(Offer(("leave_room",), "leave_room", {}))
    elif dec == "rest_site":
        for o in items("options"):
            if not o.get("is_enabled", o.get("enabled", True)):
                continue
            label = canon(o.get("option_id") or o.get("id") or o.get("title"))
            out.append(
                Offer(("rest_option", label), "choose_option", {"option_index": o.get("index")})
            )
        if view.get("can_leave") or not out:
            out.append(Offer(("leave_room",), "leave_room", {}))
    elif dec == "card_reward":
        for c in items("cards"):
            out.append(
                Offer(
                    ("card_reward", *_card_key(c)),
                    "select_card_reward",
                    {"card_index": c.get("index")},
                )
            )
        for a in items("alternatives"):
            label = canon(a.get("id") or a.get("option_id") or a.get("label") or a.get("title"))
            out.append(
                Offer(
                    ("card_reward_alternative", label),
                    "card_reward_alternative",
                    {"alternative_index": a.get("index")},
                )
            )
        if view.get("can_skip", True) or not out:
            out.append(Offer(("skip_card_reward",), "skip_card_reward", {}))
    elif dec == "bundle_select":
        for b in items("bundles"):
            cards = tuple(
                sorted(_card_key(c) for c in (b.get("cards") or []) if isinstance(c, dict))
            )
            out.append(Offer(("bundle", cards), "select_bundle", {"bundle_index": b.get("index")}))
    elif dec == "card_select":
        cards = items("cards")
        lo = _int(view.get("min_select"))
        hi = min(_int(view.get("max_select"), 1), len(cards))
        # The bounds are part of what is offered; a select that can be
        # skipped or cancelled offers zero cards whatever its minimum says.
        skippable = lo == 0 or bool(view.get("cancelable")) or bool(view.get("can_skip"))
        out.append(Offer(("select_bounds", 0 if skippable else lo, hi), "", {}))
        for c in cards:
            # Every candidate is compared; a single pick is playable only
            # while one card completes the select.
            out.append(
                Offer(
                    ("select_card", *_card_key(c)),
                    "select_cards" if lo <= 1 else "",
                    {"indices": str(c.get("index"))},
                )
            )
        if lo >= 2:
            # A pick-N select resolves in one verb: offer the first N.
            idxs = ",".join(str(c.get("index")) for c in cards[:lo])
            out.append(Offer(("select_first_n", lo), "select_cards", {"indices": idxs}))
        if skippable:
            out.append(Offer(("skip_select",), "skip_select", {}))
    elif dec == "shop":
        for kind, verb, arg in (
            ("card", "buy_card", "card_index"),
            ("relic", "buy_relic", "relic_index"),
            ("potion", "buy_potion", "potion_index"),
        ):
            for e in items(kind + "s"):
                stocked = e.get("is_stocked", not e.get("sold_out", False))
                cost = _int(e.get("cost"), 10**9)
                if not stocked or cost > gold or e.get("affordable") is False:
                    continue
                ident = canon(e.get("id") or e.get("entry") or e.get("name"))
                out.append(
                    Offer((verb, ident, bool(e.get("upgraded")), cost), verb, {arg: e.get("index")})
                )
        rc = view.get("card_removal_cost")
        if rc is not None and _int(rc, 10**9) <= gold:
            out.append(Offer(("remove_card", _int(rc)), "remove_card", {}))
        if _int(view.get("blitz_choices_remaining")) > 0:
            for o in items("options"):
                if o.get("action") == "blitz_choice" and o.get("enabled", True):
                    out.append(
                        Offer(
                            ("blitz_choice", _int(o.get("index"))),
                            "blitz_choice",
                            {"index": o.get("index")},
                        )
                    )
        out.append(Offer(("leave_room",), "leave_room", {}))
    elif dec == "rewards":
        for r in items("rewards"):
            rk = _reward_ident(r)
            out.append(Offer(("take_reward", *rk), "take_reward", {"reward_index": r.get("index")}))
        # A set built WithSkippingDisallowed (Neow's Bones) is taken whole (#1641).
        if view.get("can_skip", True) is not False:
            for r in items("rewards"):
                rk = _reward_ident(r)
                out.append(
                    Offer(("skip_reward", *rk), "skip_reward", {"reward_index": r.get("index")})
                )
            out.append(Offer(("proceed_rewards",), "proceed", {}))
    elif dec == "potion_swap":
        for o in items("options"):
            out.append(
                Offer(
                    ("potion_swap", o.get("discard_slot")),
                    "potion_swap",
                    {"discard_slot": o.get("discard_slot")},
                )
            )
    elif dec == "crystal_sphere":
        for c in items("cells"):
            if c.get("hidden"):
                for tool in ("big", "small"):
                    out.append(
                        Offer(
                            ("crystal_sphere_click", c.get("x"), c.get("y"), tool),
                            "crystal_sphere_click",
                            {"x": c.get("x"), "y": c.get("y"), "tool": tool},
                        )
                    )
    elif dec == "game_over":
        pass
    else:
        out.append(Offer(("proceed", str(dec)), "proceed", {}))
    return out


def _reward_ident(r: dict[str, Any]) -> tuple[str, str]:
    kind = str(r.get("type") or "other")
    if kind == "gold":
        return kind, str(_int(r.get("amount")))
    return kind, canon(r.get("id") or r.get("name"))


def multiset(offers: Iterable[Offer]) -> Counter[Key]:
    return Counter(o.key for o in offers)


def diff_offers(cs: Iterable[Offer], rs: Iterable[Offer]) -> tuple[list[Key], list[Key]]:
    """Keys (with multiplicity) only C# offers, and only Rust offers."""
    c, r = multiset(cs), multiset(rs)
    return sorted((c - r).elements(), key=repr), sorted((r - c).elements(), key=repr)


# --------------------------------------------------------------------------- #
# Known gaps (allowlist). Each entry: a class name, the issue, and a matcher
# over (decision, side, key). Anything unmatched is reported as NEW.
# --------------------------------------------------------------------------- #
@dataclass(frozen=True)
class KnownGap:
    name: str
    issue: str
    note: str


KNOWN_GAPS: dict[str, KnownGap] = {
    "driftwood_reroll": KnownGap(
        "driftwood_reroll", "#1608", "card-reward reroll (Driftwood) never offered by Rust"
    ),
    "turn_start_pick": KnownGap(
        "turn_start_pick",
        "#1609",
        "Entropy / Dual Wield: the game asks which cards, Rust takes the first ones",
    ),
    "crystal_sphere": KnownGap(
        "crystal_sphere",
        "by design",
        "agent mode digs the Crystal Sphere with a heuristic (crystal_sphere_digger)",
    ),
    "double_down_withheld": KnownGap(
        "double_down_withheld",
        "#1605 (by design)",
        "Trial Reject -> Double Down abandons the run; agent mode withholds it",
    ),
    "host_overlay_belt": KnownGap(
        "host_overlay_belt",
        "host",
        "the host does not open the potion belt over a card pick (card reward, "
        "card select, bundle): its game task waits on the pick (RefereeView.PotionScreens)",
    ),
    "host_parasol_removal": KnownGap(
        "host_parasol_removal",
        "host",
        "Lord's Parasol buys the card removal; on v0.111.0 only the shop UI marks it "
        "used (OnCardRemovalUsed), so the headless host offers it again",
    ),
    "host_foul_potion_merchant": KnownGap(
        "host_foul_potion_merchant",
        "host",
        "Foul Potion thrown at the merchant: FoulPotion.PassesCustomUsabilityCheck out of "
        "combat needs GetFoulPotionMerchantTarget(room).button, the NMerchantRoom / "
        "NFakeMerchant node's MerchantButton (v0.111.0 FoulPotion.cs:51-80, :139-163); the "
        "headless host has no node, so it never lets the potion be thrown. The game allows it "
        "in a MerchantRoom (or the Fake Merchant) while the inventory is closed",
    ),
    "select_option_cap": KnownGap(
        "select_option_cap",
        "#1635",
        "a combat card select lists at most 255 cards in Rust (u8 choice index); the "
        "game has no cap (a 255+ card pile, reached only in the lane's long invincible fights)",
    ),
}


def classify(
    decision: str, side: str, key: Key, label: str = "", ctx: frozenset[str] = frozenset()
) -> str | None:
    """The known gap a one-sided offer belongs to, or ``None`` (a NEW one).

    ``ctx`` carries what the walk knows beyond the key: ``"parasol"`` (a shop
    with Lord's Parasol owned), ``"option_cap"`` (a C# select listing more
    than 255 cards).
    """
    kind = key[0] if key else ""
    if decision == "card_select" and side == "cs" and kind == "select_card" and "option_cap" in ctx:
        return "select_option_cap"
    if kind == "card_reward_alternative" and side == "cs":
        return "driftwood_reroll"
    if kind == "crystal_sphere_click":
        return "crystal_sphere"
    if (
        side == "cs"
        and decision == "event_choice"
        and kind == "event_option"
        and "DOUBLEDOWN" in label
    ):
        return "double_down_withheld"
    if (
        side == "rs"
        and decision in OVERLAY_SCREENS
        and kind in ("use_potion_ooc", "discard_potion")
    ):
        return "host_overlay_belt"
    if side == "cs" and decision == "shop" and kind == "remove_card" and "parasol" in ctx:
        return "host_parasol_removal"
    if side == "rs" and decision == "shop" and key == ("use_potion_ooc", "FOULPOTION"):
        return "host_foul_potion_merchant"
    return None


# --------------------------------------------------------------------------- #
# Per-side offer collection
# --------------------------------------------------------------------------- #
def cs_offers(g: Any, view: dict[str, Any]) -> list[Offer]:
    dec = view.get("decision") or view.get("type")
    if dec == "combat_play":
        try:
            legal = g.legal_actions().get("actions") or []
        except Exception:
            legal = []
        offers = combat_offers(view, legal)
        # The game's potion popup discards in combat too (NPotionPopup).
        pots = _potion_ids(view)
        offers += [
            Offer(("discard_potion", pid), "discard_potion", {"potion_index": i})
            for i, pid in sorted(pots.items())
        ]
        return offers
    offers = offers_from_view(view)
    if dec in POTION_SCREENS:
        offers += cs_belt_offers(g, view)
    return offers


def cs_belt_offers(g: Any, view: dict[str, Any]) -> list[Offer]:
    try:
        belt = g.potion_belt()
    except Exception:
        return []
    pots = _potion_ids(view)
    out: list[Offer] = []
    for s in belt.get("slots") or []:
        i = _int(s.get("potion_index"))
        pid = pots.get(i, "?")
        if s.get("usable"):
            out.append(Offer(("use_potion_ooc", pid), "use_potion", {"potion_index": i}))
        if s.get("discardable"):
            out.append(Offer(("discard_potion", pid), "discard_potion", {"potion_index": i}))
    return out


def _rs_alive_index(rm: Any) -> dict[int, int]:
    """Full encounter slot -> index among the living enemies (the C# target),
    the inverse of ``RustGame._full_enemy_index``."""
    cs = rm.combat_state()
    if cs is None:
        return {}
    import sts2_rs

    encounter = sts2_rs.state_to_dict(cs).get("encounter") or []
    alive = [pos for pos, c in enumerate(encounter) if not c.get("is_dead")]
    return {pos: i for i, pos in enumerate(alive)}


def rs_offers(r: Any, view: dict[str, Any]) -> list[Offer]:
    """Rust offers: the engine's raw legal actions in combat (so nothing the
    backend's view drops is missed), the C#-shaped view elsewhere, plus the
    engine's out-of-combat potion actions."""
    rm = r.rm
    dec = view.get("decision") or view.get("type")
    raw = list(rm.available_actions())
    pots = _potion_ids(view)
    out: list[Offer] = []
    if dec == "combat_play":
        hand = _hand_ids(view)
        alive = _rs_alive_index(rm)
        for a in raw:
            kind = a.get("action")
            tgt = a.get("target_index")
            tgt_c = None if tgt is None else alive.get(int(tgt), int(tgt))
            if kind == "play_card":
                cid, up = hand.get(_int(a.get("hand_idx")), ("?", False))
                out.append(
                    Offer(
                        ("play_card", cid, up, tgt_c),
                        "play_card",
                        {"card_index": a.get("hand_idx"), "target_index": tgt_c},
                    )
                )
            elif kind == "drink_potion":
                pid = pots.get(_int(a.get("potion_slot")), "?")
                out.append(
                    Offer(
                        ("use_potion", pid, tgt_c),
                        "use_potion",
                        {"potion_index": a.get("potion_slot"), "target_index": tgt_c},
                    )
                )
            elif kind == "end_turn":
                out.append(Offer(("end_turn",), "end_turn", {}))
            elif kind == "discard_potion":
                pid = pots.get(_int(a.get("potion_slot")), "?")
                out.append(Offer(("discard_potion", pid), "", {}, raw=a))
            else:
                out.append(Offer(("rs_raw", str(kind)), "", {}, raw=a))
        return out
    out = offers_from_view(view)
    if dec == "event_choice" and rm.phase() == "boss_relic":
        # The backend lists every legal action as an Ancient's option, the
        # potion popup's too; those are compared from the raw list below.
        out = [
            o
            for o in out
            if not (0 <= _int(o.kw.get("option_index"), -1) < len(r._option_actions))
            or r._option_actions[_int(o.kw.get("option_index"))].get("action")
            not in ("use_potion_ooc", "discard_potion")
        ]
    if dec == "event_choice" and rm.phase() in ("treasure", "card_reward"):
        # Generic per-action screens (treasure, combat relic reward): key them
        # by what the raw action is, as the C# reward screen does.
        out = _rs_generic_offers(r, view)
    if (
        dec == "rest_site"
        and any(a.get("action") == "leave_shop" for a in raw)
        and not any(o.key == ("leave_room",) for o in out)
    ):
        # A MiniatureTent rest site: the engine offers leaving after one
        # option, which the backend view does not flag (no ``can_leave``).
        out.append(Offer(("leave_room",), "leave_room", {}))
    for a in raw:
        kind = a.get("action")
        if kind in ("use_potion_ooc", "discard_potion"):
            pid = pots.get(_int(a.get("potion_slot")), "?")
            out.append(Offer((kind, pid), "", {}, raw=a))
    return out


def _rs_generic_offers(r: Any, view: dict[str, Any]) -> list[Offer]:
    rm = r.rm
    relics = list(rm.relic_reward_ids()) if hasattr(rm, "relic_reward_ids") else []
    head = canon(relics[0]) if relics else "?"
    out: list[Offer] = []
    for o in view.get("options") or []:
        i = _int(o.get("index"))
        a = r._option_actions[i] if i < len(r._option_actions) else {}
        kind = a.get("action")
        if kind == "pick_relic_reward":
            key: Key = ("take_reward", "relic", head)
        elif kind == "skip_relic":
            key = ("skip_reward", "relic", head)
        elif kind == "collect_treasure":
            key = ("collect_treasure",)
        else:
            key = ("rs_option", str(kind))
        out.append(Offer(key, "choose_option", {"option_index": i}))
    return out


# --------------------------------------------------------------------------- #
# The walk
# --------------------------------------------------------------------------- #
def _is_err(v: Any) -> bool:
    return isinstance(v, dict) and bool(v.get("error") or v.get("type") == "error")


@dataclass
class LaneRun:
    seed: str
    character: str
    game_version: str
    steps: int = 0
    terminal: str = "error"
    max_act: int = 0
    decisions: Counter[str] = field(default_factory=Counter)
    divergent_decisions: Counter[str] = field(default_factory=Counter)
    gap_counts: Counter[str] = field(default_factory=Counter)  # known-gap one-sided keys
    new_counts: Counter[str] = field(default_factory=Counter)  # unclassified, by decision:kind
    reconciliations: Counter[str] = field(default_factory=Counter)
    state_divergence: list[str] = field(default_factory=list)
    rows: list[dict[str, Any]] = field(default_factory=list)


def _apply_rs(r: Any, offer: Offer) -> dict[str, Any]:
    if offer.raw is not None:
        res = r.rm.take_action(dict(offer.raw))
        if res not in ("advanced", "run_over"):
            return {"type": "error", "message": f"rust take_action -> {res}"}
        return r._serialize()
    return r.act(offer.verb, **offer.kw)


def run_lane(
    seed: str,
    character: str = "Ironclad",
    ascension: int = 0,
    max_steps: int = 3000,
    invincible: bool = True,
    keep_rows: int = 400,
    trace: bool = False,
    dump_steps: frozenset[int] = frozenset(),
) -> LaneRun:
    """One agent-mode lockstep walk; see the module docstring. ``dump_steps``
    keeps both sides' views at those steps (a ``views`` row), for debugging."""
    for k, v in HOST_ENV.items():
        os.environ[k] = v
    from act23_state_parity import (
        COMBAT_ROUND_CAP,
        INVINCIBLE_HP,
        _cs_select_still_pending,
        _is_architect_event,
        _pin,
        _stale_cs_select_pick,
    )
    from run_parity import (
        Snap,
        diff_fake_merchant_offers,
        diff_maps,
        diff_snaps,
        map_signature,
        rust_finalize_atomic_select,
    )
    from sts2_client import GAME_VERSION
    from workbench.backends.base import make_backend

    rng = random.Random(f"agentparity:{seed}")
    out = LaneRun(seed=seed, character=character, game_version=GAME_VERSION)
    g = make_backend("csharp")()
    r = make_backend("rust", target_convention="csharp", agent_lane=True)()

    recent: list[dict[str, Any]] = []  # the last moves, attached to a terminal row
    terminal_kinds = ("state_divergence", "no_shared_action", "act_reject", "harness_error")

    def row(kind: str, **kw: Any) -> None:
        r_ = {"seed": seed, "character": character, "step": out.steps, "kind": kind, **kw}
        if kind in terminal_kinds:
            r_["recent"] = recent[-16:]
            out.rows.append(r_)
        elif len(out.rows) < keep_rows:
            out.rows.append(r_)

    try:
        cs_view = g.start(
            character=character, seed=seed, ascension=ascension, interactive_potion_swap=True
        )
        rs_view = r.start(
            character=character, seed=seed, ascension=ascension, interactive_potion_swap=True
        )
        for step in range(max_steps):
            out.steps = step
            act = _int((cs_view.get("context") or {}).get("act"), 0)
            out.max_act = max(out.max_act, act)
            cs_dec = cs_view.get("decision") or cs_view.get("type")
            rs_dec = rs_view.get("decision") or rs_view.get("type")
            cs_snap, rs_snap = Snap.of(cs_view, "cs"), Snap.of(rs_view, "rs")
            if step in dump_steps:
                row("views", cs=cs_view, rs=rs_view)

            # -- reconciliations: one side asks what the other decided itself --
            if cs_dec == "crystal_sphere":
                out.gap_counts["crystal_sphere"] += 1
                out.terminal = "allowlisted_stop:crystal_sphere"
                return out
            if cs_snap.phase == "card_select" and rs_snap.phase != "card_select":
                offs = [o for o in cs_offers(g, cs_view) if o.verb]
                prompt = str(cs_view.get("prompt_key") or cs_view.get("context_kind") or "")
                source = str(cs_view.get("source_model") or cs_view.get("source") or "")
                out.reconciliations[f"cs_alone_select:{rs_snap.phase}:{prompt}:{source}"] += 1
                gap = classify_cs_alone_select(source)
                if gap:
                    out.gap_counts[gap] += 1
                else:
                    out.new_counts[f"card_select:cs_alone:{prompt}:{source}"] += 1
                row(
                    "cs_alone_select",
                    rs_phase=rs_snap.phase,
                    prompt=prompt,
                    source=source,
                    cards=[c.get("id") for c in cs_view.get("cards") or []][:8],
                )
                if offs:
                    lo = _int(cs_view.get("min_select"))
                    idxs = ",".join(
                        str(c.get("index")) for c in (cs_view.get("cards") or [])[: max(lo, 1)]
                    )
                    nxt = g.act("select_cards", indices=idxs)
                    if not _is_err(nxt):
                        cs_view = nxt
                        continue
            # The host's view can come back before a parked game task opened
            # its select (a racing ``WaitFor``: Lord's Parasol buying the shop
            # out, a turn-start pick). Probe it, then play the same pick on
            # both sides, as act23's stale-select guard does.
            if rs_snap.phase == "card_select" and cs_snap.phase != "card_select":
                if _cs_select_still_pending(g):
                    verb, kw = _stale_cs_select_pick(rs_view)
                    nxt_cs, nxt_rs = g.act(verb, **kw), r.act(verb, **kw)
                    out.reconciliations[f"stale_cs_select:{cs_snap.phase}"] += 1
                    if not _is_err(nxt_cs) and not _is_err(nxt_rs):
                        cs_view, rs_view = nxt_cs, nxt_rs
                        continue
            guard = 0
            while rs_snap.phase == "card_select" and cs_snap.phase != "card_select" and guard < 8:
                nxt = rust_finalize_atomic_select(r, rs_view)
                if _is_err(nxt):
                    break
                out.reconciliations[f"rs_alone_select:{cs_snap.phase}"] += 1
                # #1623 / #1624 were the last known Rust-only selects.
                out.new_counts[
                    f"card_select:rs_alone:{cs_snap.phase}:{rs_view.get('context') or ''}"
                ] += 1
                row(
                    "rs_alone_select",
                    cs_phase=cs_snap.phase,
                    context=str(rs_view.get("context") or rs_view.get("prompt_key") or ""),
                    cards=[[c.get("id"), c.get("upgraded")] for c in rs_view.get("cards") or []][
                        :10
                    ],
                    min_select=rs_view.get("min_select"),
                    max_select=rs_view.get("max_select"),
                    last=recent[-3:],
                )
                rs_view, rs_snap = nxt, Snap.of(nxt, "rs")
                rs_dec = rs_view.get("decision") or rs_view.get("type")
                guard += 1
            if (
                rs_snap.phase == "game_over"
                and cs_snap.phase == "event"
                and _is_architect_event(cs_view)
            ):
                nxt = g.act("choose_option", option_index=0)
                if not _is_err(nxt):
                    out.reconciliations["architect_victory"] += 1
                    cs_view = nxt
                    continue

            # -- state diff --
            fields = diff_snaps(cs_snap, rs_snap) + diff_fake_merchant_offers(cs_view, rs_view)
            if cs_snap.phase == "map" and rs_snap.phase == "map":
                fields += diff_maps(map_signature(g), map_signature(r))
            if cs_snap.phase == "game_over" and rs_snap.phase == "game_over":
                fields = [f for f in fields if not f.startswith("hp:")]
            if fields:
                out.state_divergence = fields
                out.terminal = "state_diverged"
                row(
                    "state_divergence",
                    phase=cs_snap.phase,
                    cs_dec=cs_dec,
                    rs_dec=rs_dec,
                    fields=fields[:6],
                )
                return out
            if cs_dec == "game_over":
                out.terminal = "game_over_match"
                return out

            # -- offer diff --
            co, ro = cs_offers(g, cs_view), rs_offers(r, rs_view)
            out.decisions[str(cs_dec)] += 1
            n_rows = len(out.rows)
            ctx: set[str] = set()
            if cs_dec == "card_select" and len(cs_view.get("cards") or []) > 255:
                ctx.add("option_cap")
            if cs_dec == "shop" and "LORDSPARASOL" in {
                canon(x.get("id") if isinstance(x, dict) else x)
                for x in (cs_view.get("player") or {}).get("relics") or []
            }:
                ctx.add("parasol")
            _record_offer_diff(
                out,
                row,
                str(cs_dec),
                co,
                ro,
                rs_dec,
                str(cs_view.get("event_name") or cs_view.get("prompt_key") or ""),
                recent[-2:],
                frozenset(ctx),
                keep_known=trace,
            )
            if trace and len(out.rows) > n_rows and out.rows[-1].get("new"):
                out.rows[-1]["cs_view"] = _trim(cs_view)
                out.rows[-1]["rs_view"] = _trim(rs_view)

            if (
                invincible
                and cs_dec == "combat_play"
                and _int(cs_view.get("round")) > COMBAT_ROUND_CAP
            ):
                out.terminal = "combat_round_cap"
                return out
            if invincible and cs_snap.phase not in ("combat", "event") and r.combat_state() is None:
                _pin(g, r, INVINCIBLE_HP)

            # -- pick from the intersection --
            rs_by_key: dict[Key, list[Offer]] = {}
            for o in ro:
                rs_by_key.setdefault(o.key, []).append(o)
            shared = [o for o in co if o.verb and rs_by_key.get(o.key)]
            if not shared:
                out.terminal = "no_shared_action"
                row(
                    "no_shared_action",
                    decision=cs_dec,
                    rs_dec=rs_dec,
                    cs=[list(o.key) for o in co][:12],
                    rs=[list(o.key) for o in ro][:12],
                )
                return out
            advanced = False
            while shared:
                c_off = shared.pop(rng.randrange(len(shared)))
                # Equal keys (two unupgraded Strikes) are still different
                # cards: play the same occurrence on both sides.
                nth = [o for o in co if o.key == c_off.key].index(c_off)
                r_list = rs_by_key[c_off.key]
                r_off = r_list[min(nth, len(r_list) - 1)]
                nxt_cs = g.act(c_off.verb, **c_off.kw)
                nxt_rs = _apply_rs(r, r_off)
                ce, re_ = _is_err(nxt_cs), _is_err(nxt_rs)
                if ce and re_:
                    continue
                if ce or re_:
                    out.terminal = "act_reject"
                    row(
                        "act_reject",
                        decision=cs_dec,
                        key=list(c_off.key),
                        cs_err=nxt_cs.get("message") if ce else None,
                        rs_err=(nxt_rs.get("message") or nxt_rs.get("error")) if re_ else None,
                    )
                    return out
                recent.append(
                    {
                        "step": step,
                        "decision": cs_dec,
                        "key": list(c_off.key),
                        "label": c_off.label,
                        "event": cs_view.get("event_name"),
                    }
                )
                del recent[:-16]
                if trace:
                    row(
                        "step",
                        decision=cs_dec,
                        key=list(c_off.key),
                        label=c_off.label,
                        hp=cs_snap.hp,
                        gold=cs_snap.gold,
                    )
                cs_view, rs_view = nxt_cs, nxt_rs
                advanced = True
                break
            if not advanced:
                out.terminal = "both_reject"
                return out
        out.terminal = "max_steps"
        return out
    except Exception as exc:  # harness failure: report, don't crash the sweep
        out.terminal = "error"
        row("harness_error", error=f"{type(exc).__name__}: {exc}")
        return out
    finally:
        for b in (g, r):
            try:
                b.close()
            except Exception:
                pass


def _trim(view: dict[str, Any]) -> dict[str, Any]:
    drop = ("player", "context", "draw_pile", "discard_pile", "exhaust_pile", "map", "description")
    return {k: v for k, v in view.items() if k not in drop}


def _record_offer_diff(
    out: LaneRun,
    row: Any,
    decision: str,
    co: list[Offer],
    ro: list[Offer],
    rs_dec: Any,
    where: str = "",
    last: list[dict[str, Any]] | None = None,
    ctx: frozenset[str] = frozenset(),
    keep_known: bool = False,
) -> None:
    only_cs, only_rs = diff_offers(co, ro)
    if not (only_cs or only_rs):
        return
    labels = {("cs", o.key): o.label for o in co} | {("rs", o.key): o.label for o in ro}
    out.divergent_decisions[decision] += 1
    new: list[tuple[str, Key]] = []
    known: list[tuple[str, str, Key]] = []
    for side, keys in (("cs", only_cs), ("rs", only_rs)):
        for k in keys:
            gap = classify(decision, side, k, labels.get((side, k), ""), ctx)
            if gap is None:
                new.append((side, k))
                out.new_counts[f"{decision}:{side}:{k[0]}"] += 1
            else:
                known.append((gap, side, k))
                out.gap_counts[gap] += 1
    if not new and not keep_known:
        return
    row(
        "offer_diff",
        decision=decision,
        where=where,
        last=[
            [x.get("decision"), x.get("key"), x.get("label") or x.get("event")]
            for x in (last or [])
        ],
        rs_dec=rs_dec,
        new=[[s, list(k), labels.get((s, k), "")] for s, k in new],
        known=[[g, s, list(k)] for g, s, k in known][:10],
    )


# --------------------------------------------------------------------------- #
# Sweep + report
# --------------------------------------------------------------------------- #
def _worker(args: tuple[str, str, int, int, str]) -> dict[str, Any]:
    seed, character, ascension, max_steps, version = args
    os.environ["STS2_GAME_VERSION"] = version
    import sts2_client

    sts2_client.GAME_VERSION = version
    res = run_lane(seed, character, ascension, max_steps)
    counters = ("decisions", "divergent_decisions", "gap_counts", "new_counts", "reconciliations")
    # Not ``asdict``: it rebuilds a Counter from its items, as (key, n) keys.
    d = {k: v for k, v in vars(res).items() if k not in counters}
    for k in counters:
        d[k] = dict(getattr(res, k))
    return d


# Card selects the game asks and Rust resolves itself (#1609), by the C#
# view's ``source_model``, canonicalised.
TURN_START_PICK_SOURCES = ("ENTROPY", "DUALWIELD")


def classify_cs_alone_select(source: str) -> str | None:
    """The known gap behind a select only the game asks (Rust picked by
    itself): Entropy and Dual Wield (#1609). ``None`` is a NEW missing choice."""
    return "turn_start_pick" if any(t in canon(source) for t in TURN_START_PICK_SOURCES) else None


# Terminals where the lockstep itself broke (versus a run that ended: game
# over, the step / round cap, an allowlisted stop).
BROKEN_TERMINALS = frozenset(
    {"state_diverged", "act_reject", "no_shared_action", "both_reject", "error"}
)


def summarize(results: list[dict[str, Any]]) -> dict[str, Any]:
    tot: dict[str, Counter[str]] = {
        k: Counter()
        for k in ("decisions", "divergent_decisions", "gap_counts", "new_counts", "reconciliations")
    }
    terms: Counter[str] = Counter()
    acts: Counter[int] = Counter()
    for r in results:
        terms[r["terminal"]] += 1
        acts[r["max_act"]] += 1
        for k in tot:
            tot[k].update(r[k])
    return {
        "runs": len(results),
        "terminals": dict(terms.most_common()),
        "max_act": {str(k): v for k, v in sorted(acts.items())},
        "steps": sum(r["steps"] for r in results),
        **{k: dict(v.most_common()) for k, v in tot.items()},
        "known_gaps": {k: asdict(v) for k, v in KNOWN_GAPS.items()},
    }


def main(argv: list[str] | None = None) -> int:
    from sts2_client import add_game_version_arg, apply_game_version_arg

    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--character", default="Ironclad", help="a character, a comma list, or 'all'")
    ap.add_argument("--ascension", type=int, default=0)
    ap.add_argument("--seed", default="AP", help="seed prefix; run i uses <prefix><i>")
    ap.add_argument("-n", type=int, default=10)
    ap.add_argument("--start", type=int, default=1000)
    ap.add_argument("-j", type=int, default=4)
    ap.add_argument("--max-steps", type=int, default=3000)
    ap.add_argument(
        "--out", type=pathlib.Path, default=None, help="dir for runs.jsonl + summary.json"
    )
    ap.add_argument(
        "--strict",
        action="store_true",
        help="exit 1 on any NEW (unallowlisted) offer difference or a broken lockstep",
    )
    add_game_version_arg(ap)
    args = ap.parse_args(argv)
    apply_game_version_arg(args)
    from sts2_client import GAME_VERSION

    chars = (
        ["Ironclad", "Silent", "Defect", "Regent", "Necrobinder"]
        if args.character == "all"
        else [c.strip() for c in args.character.split(",") if c.strip()]
    )
    jobs = [
        (f"{args.seed}{args.start + i}", ch, args.ascension, args.max_steps, GAME_VERSION)
        for ch in chars
        for i in range(args.n)
    ]
    results: list[dict[str, Any]] = []
    out_f = None
    if args.out:
        args.out.mkdir(parents=True, exist_ok=True)
        out_f = (args.out / "runs.jsonl").open("w", encoding="utf-8")
    with _cf.ProcessPoolExecutor(max_workers=args.j) as ex:
        for d in ex.map(_worker, jobs):
            results.append(d)
            print(
                f"{d['character']:>11} {d['seed']:>8} act{d['max_act']} steps={d['steps']:5d} {d['terminal']:<28} "
                f"new={sum(d['new_counts'].values())} known={sum(d['gap_counts'].values())}",
                flush=True,
            )
            if out_f:
                out_f.write(json.dumps(d) + "\n")
                out_f.flush()
    if out_f:
        out_f.close()
    summary = summarize(results)
    text = json.dumps(summary, indent=2)
    print(text)
    if args.out:
        (args.out / "summary.json").write_text(text + "\n", encoding="utf-8")
    broken = {t: n for t, n in summary["terminals"].items() if t in BROKEN_TERMINALS}
    if args.strict and (summary["new_counts"] or broken):
        print(f"agent-parity: FAIL new={summary['new_counts']} broken={broken}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
