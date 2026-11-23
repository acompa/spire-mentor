"""Tactical columns: what this turn can still do (WP3, plan §4).

Three numbers a combat net would otherwise have to add up itself, from the
honest snapshot only (`combat_ctx.snapshot.honest_snapshot`, so they are a
function of what the player sees):

* per enemy, `max_damage`: the most damage this turn's hand can deal to that
  enemy under the current energy. A 0/1 knapsack over the playable hand
  cards' `card_damage_preview()` numbers (per hit, with Strength, Vulnerable
  and the like already applied), times each card's hit count from the
  content prior; an X-cost card spends whatever energy is left. A random-
  target card counts all its hits on the enemy (an upper bound).
* per enemy, `killable`: `max_damage >= hp + block`.
* `hp_loss`: the HP the player loses if the turn ended now, the displayed
  attack intents of every living enemy (`effective_amount x times`) less the
  player's block. On another character's row (the "chars" fill event, GH
  #1373) two more terms come off, both zero for Ironclad: the block the Frost
  orbs' passives add before the enemies act (`end_block`), and the HP of a
  living Osty, which takes the unblocked attack damage first (`ally_hp`).
* per enemy, `doomed`: Doom >= hp, so the enemy dies at the end of its turn.

It ignores draw, energy gain and every other effect of the cards it plays, so
`max_damage` is a lower bound on a combo turn and an upper bound when a random
hit can miss its enemy. Pure Python: about 5-30 us on a typical hand.
"""

from __future__ import annotations

from functools import lru_cache
from typing import Any

from . import content_prior as P
from . import vocab as V

# Divisors that put the columns near [0, 1] (the net applies symlog anyway).
DMG_SCALE = 50.0


@lru_cache(maxsize=4096)
def hits_of(card_id: str) -> int:
    """A card's printed hit count (1 if it has no repeat), from the prior."""
    vid = V.load().id("card", card_id)
    h = float(P.table()[vid, P.C["HITS"]]) * 5.0
    return max(1, round(h))


def _attack_total(i: dict[str, Any]) -> float:
    if i.get("kind") not in ("Attack", "DeathBlow"):
        return 0.0
    eff = i.get("effective_amount", i.get("amount", 0)) or 0
    return float(eff) * float(i.get("times") or 1)


def hp_loss(
    snap: dict[str, Any],
    intents: list[list[dict[str, Any]]] | None = None,
    *,
    end_block: float = 0.0,
    ally_hp: float = 0.0,
) -> float:
    """The HP the player loses to attack intents if the turn ended now.

    `intents` (aligned with `snap["enemies"]`) replaces the snapshot's
    displayed intents, e.g. with the full telegraph once the getter exists.
    `end_block` and `ally_hp` are `end_turn_block` and `ally_hp_of` of the
    player (0 for Ironclad).
    """
    inc = 0.0
    for j, m in enumerate(snap["enemies"]):
        if m.get("is_dead"):
            continue
        ints = intents[j] if intents is not None else m.get("intents") or []
        inc += sum(_attack_total(i) for i in ints)
    unblocked = max(0.0, inc - float(snap["player"].get("block") or 0) - end_block)
    return max(0.0, unblocked - ally_hp)


def end_turn_block(player: dict[str, Any]) -> float:
    """The block the orb belt adds at the end of the player's turn, before the
    enemies act: each Frost orb's passive (`OrbQueue.BeforeTurnEnd`,
    `FrostOrb.Passive`: unpowered block, Focus already in `passive`).

    `player` is `state_to_dict(bv)["player"]`.
    """
    return float(
        sum(
            int(o.get("passive") or 0) for o in player.get("orbs") or [] if o.get("type") == "Frost"
        )
    )


def ally_hp_of(player: dict[str, Any]) -> float:
    """The HP of the player's living Osty, else 0. Osty takes the unblocked
    damage of an attack on its owner, and the overkill passes on to the owner
    (`DieForYouPower.ModifyUnblockedDamageTarget`, `CreatureCmd.Damage`)."""
    o = player.get("osty") or {}
    return float(o.get("hp") or 0) if o.get("alive") else 0.0


DOOM = "DOOM"


def doomed(snap: dict[str, Any]) -> list[float]:
    """Per enemy of `snap["enemies"]`: 1.0 if it is alive and its Doom is at
    least its HP (`DoomPower.IsOwnerDoomed`: it dies at the end of its turn)."""
    out = []
    for m in snap["enemies"]:
        doom = sum(
            float(p.get("amount") or 0) for p in m.get("powers") or [] if p.get("name") == DOOM
        )
        hp = float(m.get("hp") or 0)
        out.append(float(not m.get("is_dead") and hp > 0 and doom >= hp))
    return out


def max_damage(snap: dict[str, Any], playable: set[int]) -> list[float]:
    """Per enemy of `snap["enemies"]`, the knapsack damage bound (see module doc)."""
    n = len(snap["enemies"])
    energy = max(0, int(snap["player"].get("energy") or 0))
    fixed = [0.0] * n  # zero-cost cards: always played
    items: list[tuple[int, list[float]]] = []  # (cost, damage per enemy)
    xcards: list[list[float]] = []  # X cost: damage per enemy per energy
    for i, c in enumerate(snap["hand"]):
        if i not in playable:
            continue
        dmg = c.get("dmg")
        if not dmg:
            continue
        h = hits_of(c["id"])
        row = [0.0] * n
        for k, v in dmg.items():
            j = int(k)
            if 0 <= j < n and v > 0:
                row[j] = float(v) * h
        if not any(row):
            continue
        if int(c.get("base_cost") or 0) < 0:
            xcards.append([x / h for x in row])  # per hit; hits = energy spent
            continue
        cost = max(0, int(c.get("cost") or 0))
        if cost == 0:
            fixed = [a + b for a, b in zip(fixed, row, strict=True)]
        elif cost <= energy:
            items.append((cost, row))
    out = []
    for j in range(n):
        dp = [0.0] * (energy + 1)
        for cost, row in items:
            v = row[j]
            if v <= 0.0:
                continue
            for e in range(energy, cost - 1, -1):
                t = dp[e - cost] + v
                if t > dp[e]:
                    dp[e] = t
        best = dp[energy]
        for xc in xcards:
            per = xc[j]
            if per > 0.0:
                for e in range(energy + 1):
                    best = max(best, dp[e] + per * (energy - e))
        out.append(fixed[j] + best)
    return out


def live_snapshot(snap: dict[str, Any], bv: Any, d_bv: dict[str, Any]) -> dict[str, Any]:
    """A copy of `snap` with the live keys (`add_live`); `snap` stays as it was.

    The tokenizer makes it on a row that carries the "chars" event, and only there
    (GH #1197): a row without the event pays for none of the engine walks. Twin of
    `snapshot::live_snapshot` in `sts2-e2e`."""
    out = dict(snap)
    out["hand"] = [dict(c) for c in snap["hand"]]
    return add_live(out, bv, d_bv)


_PILE_KEYS = ("draw_pile", "discard_pile", "exhaust_pile", "play_pile")


def _live_piles(snap: dict[str, Any], d_bv: dict[str, Any]) -> None:
    """The piles of `snap` rebuilt with each card's `pile_flags` (a pile card's Sly /
    Sly this turn / Retain this turn, GH #1197) in its `flags`, as a hand card carries
    them. A multiset entry splits where the flags differ. No-op when no pile card has
    `pile_flags` (the rebuilt piles would equal the honest ones)."""
    from ppo.combat_ctx.snapshot import _card, multiset

    deck = d_bv.get("deck") or {}
    if not any(c.get("pile_flags") for k in _PILE_KEYS for c in deck.get(k) or []):
        return

    def card(c: dict[str, Any]) -> dict[str, Any]:
        pf = c.get("pile_flags")
        return _card({**c, "flags": [*(c.get("flags") or []), *pf]} if pf else c)

    draw = deck.get("draw_pile", [])
    top_idx = list(deck.get("pinned_top", []))
    bot_idx = list(deck.get("pinned_bottom", []))
    pinned = set(top_idx) | set(bot_idx)
    snap["draw_unknown"] = multiset([card(c) for i, c in enumerate(draw) if i not in pinned])
    snap["draw_top"] = [card(draw[i]) for i in top_idx]
    snap["draw_bottom"] = [card(draw[i]) for i in bot_idx]
    snap["discard"] = multiset([card(c) for c in deck.get("discard_pile", [])])
    snap["exhaust"] = multiset([card(c) for c in deck.get("exhaust_pile", [])])
    snap["play"] = [card(c) for c in deck.get("play_pile", [])]


def add_live(snap: dict[str, Any], bv: Any, d_bv: dict[str, Any]) -> dict[str, Any]:
    """The live keys of a "chars" row (GH #1197), set in place on `snap`.

    Per hand card: `dmg_live` (`{"<enemy index>": [damage per hit, hits]}`,
    `bv.card_damage_live()`), `gen_live` (`{"flat", "per_energy", "kills"}` when the
    play generates damage, plus `"by_x"`, a list by the X spent, for an X card whose
    damage is not linear in X;
    `bv.card_gen_live()`), `blk_live` (`bv.card_block_live()`,
    None = no block) and `star_cost_live` (copied from `d_bv`'s hand card when it has
    one). The piles carry each card's `pile_flags` in its `flags` (`_live_piles`).
    Only on a "chars" row, through `live_snapshot`: `snapshot_from_parts`, the merge
    key (`choose_outcome_key`) and the other honest-snapshot consumers stay as they
    were. Twin of `snapshot::add_live` in `sts2-e2e`.

    Also the top-level `end_live` (`{"block", "dies", "self_loss", "buffer",
    "remnant"}`, `bv.end_turn_live()`) for `dies_before_acting` / `hp_loss_live`: the
    change in the player's block before the enemies act, the enemy indexes certain to
    die before their move, the HP the turn end itself costs the player, its Buffer
    stacks left and Beating Remnant's HP-loss budget left (None without the relic).
    """
    dmg = bv.card_damage_live()
    gen = bv.card_gen_live()
    blk = bv.card_block_live()
    d_hand = d_bv["deck"].get("hand", [])
    for i, hc in enumerate(snap["hand"]):
        pv = dmg[i] if i < len(dmg) else {}
        hc["dmg_live"] = {str(int(k)): [int(d), float(h)] for k, (d, h) in sorted(pv.items())}
        g = gen[i] if i < len(gen) else None
        if g is not None:
            flat, per, kills, by_x = g
            hc["gen_live"] = {
                "flat": {str(int(k)): float(v) for k, v in sorted(flat.items())},
                "per_energy": {str(int(k)): float(v) for k, v in sorted(per.items())},
                "kills": [int(j) for j in kills],
            }
            if by_x:
                hc["gen_live"]["by_x"] = [
                    {str(int(k)): float(v) for k, v in sorted(r.items())} for r in by_x
                ]
        if i < len(d_hand) and "star_cost_live" in d_hand[i]:
            hc["star_cost_live"] = d_hand[i]["star_cost_live"]
        hc["blk_live"] = blk[i] if i < len(blk) else None
    _live_piles(snap, d_bv)
    block, dies, self_loss, buffer, remnant = bv.end_turn_live()
    snap["end_live"] = {
        "block": int(block),
        "dies": [int(j) for j in dies],
        "self_loss": int(self_loss),
        "buffer": int(buffer),
        "remnant": None if remnant is None else int(remnant),
    }
    return snap


def _live_rows(
    c: dict[str, Any], enemies: list[dict[str, Any]]
) -> tuple[list[float], list[float], list[float], list[float], list[list[float]]]:
    """A hand card's `(attack, flat, per_energy, kill, by_x)` rows per enemy, from its
    live keys.

    `attack` is `dmg_live` (`dmg x hits`, per energy on an X-cost card, all the
    stars a star-X card would spend now). `flat` is the damage the play adds
    beyond its own attack (`gen_live.flat`: Shivs, Knife Trap, orb evokes).
    `per_energy` is the generated damage per energy of an X-cost card (Multi-Cast's
    evokes). `kill` is the enemy's HP + block where the play's Doom kills it
    outright (`gen_live.kills`, End of Days), else 0. `by_x` is, by the energy an
    X-cost play spends, the damage it adds per enemy where that is not linear in X
    (`gen_live.by_x`: Tempest's overflow evokes); empty for every other card.
    """
    n = len(enemies)
    att = [0.0] * n
    for k, v in (c.get("dmg_live") or {}).items():
        j = int(k)
        if 0 <= j < n and v[0] > 0:
            att[j] = float(v[0]) * float(v[1])
    gen = c.get("gen_live") or {}
    flat = [0.0] * n
    for k, v in (gen.get("flat") or {}).items():
        j = int(k)
        if 0 <= j < n and v > 0:
            flat[j] = float(v)
    per = [0.0] * n
    for k, v in (gen.get("per_energy") or {}).items():
        j = int(k)
        if 0 <= j < n and v > 0:
            per[j] = float(v)
    kill = [0.0] * n
    for j in gen.get("kills") or []:
        if 0 <= j < n and not enemies[j].get("is_dead"):
            m = enemies[j]
            kill[j] = float(m.get("hp") or 0) + float(m.get("block") or 0)
    by_x = []
    for r in gen.get("by_x") or []:
        row = [0.0] * n
        for k, v in r.items():
            j = int(k)
            if 0 <= j < n and v > 0:
                row[j] = float(v)
        by_x.append(row)
    return att, flat, per, kill, by_x


def _x_live(p: float, b: float, by_x: list[list[float]], j: int) -> bool:
    """Whether an X card hits enemy `j` at some X (its table, if any, replaces `p`)."""
    if by_x:
        return b > 0.0 or any(r[j] > 0.0 for r in by_x)
    return p > 0.0 or b > 0.0


def _x_term(p: float, by_x: list[list[float]], j: int, x: int) -> float:
    """An X card's X damage on enemy `j` at `x` spent: `by_x[x][j]` (0 past the
    table) when it has one, else `p * x`."""
    if by_x:
        return by_x[x][j] if x < len(by_x) else 0.0
    return p * x


def max_damage_live(snap: dict[str, Any], playable: set[int]) -> list[float]:
    """Per enemy of `snap["enemies"]`, the live knapsack damage bound (GH #1197).

    The hand cards' live keys (`dmg_live`, `star_cost_live`, `gen_live`; see
    `add_live`) under a 2-D budget of energy and stars. An item costs its energy
    and its star cost and is worth `attack + flat`, or the target's HP + block if
    that is more and its Doom kills it (`_live_rows`); a card free in both is
    always played. One X card at most: an X-energy card spends the energy left at
    `attack + per_energy` per energy plus its flat damage, a star-X card the stars
    left at `attack / stars` per star plus its flat damage (no X card kills with
    Doom). A card with a `by_x` table (indexed by the energy spent, or the stars
    for a star-X card) takes its entry at that X instead of the linear term:
    Tempest, Chemical X, Heavenly Drill's doubling, Multi-Cast. An enemy already doomed keeps its bound: the `doomed` column carries it.
    `max_damage` is untouched.
    """
    enemies = snap["enemies"]
    n = len(enemies)
    energy = max(0, int(snap["player"].get("energy") or 0))
    stars = max(0, int(snap["player"].get("stars") or 0))
    fixed = [0.0] * n  # free in energy and stars: always played
    items: list[tuple[int, int, list[float]]] = []  # (energy, stars, damage per enemy)
    # X energy: (per energy, flat, by the energy spent)
    xe: list[tuple[list[float], list[float], list[list[float]]]] = []
    # star-X: (energy, per star, flat, by the stars spent)
    xs: list[tuple[int, list[float], list[float], list[list[float]]]] = []
    for i, c in enumerate(snap["hand"]):
        if i not in playable:
            continue
        att, flat, per, kill, by_x = _live_rows(c, enemies)
        cost = max(0, int(c.get("cost") or 0))
        star_cost = int(c.get("star_cost_live") or 0)
        if int(c.get("base_cost") or 0) < 0:
            pe = [a + b for a, b in zip(att, per, strict=True)]
            if any(pe) or any(flat) or any(any(r) for r in by_x):
                xe.append((pe, flat, by_x))
            continue
        if star_cost < 0:
            ps = [a / stars for a in att] if stars > 0 else [0.0] * n
            if cost <= energy and (any(ps) or any(flat) or any(any(r) for r in by_x)):
                xs.append((cost, ps, flat, by_x))
            continue
        row = [max(a + b, k) for a, b, k in zip(att, flat, kill, strict=True)]
        if not any(row):
            continue
        star_cost = max(0, star_cost)
        if cost == 0 and star_cost == 0:
            fixed = [a + b for a, b in zip(fixed, row, strict=True)]
        elif cost <= energy and star_cost <= stars:
            items.append((cost, star_cost, row))
    w = stars + 1
    out = []
    for j in range(n):
        dp = [0.0] * ((energy + 1) * w)  # dp[e * w + s]: best with e energy, s stars
        for cost, star_cost, row in items:
            v = row[j]
            if v <= 0.0:
                continue
            for e in range(energy, cost - 1, -1):
                for s in range(stars, star_cost - 1, -1):
                    t = dp[(e - cost) * w + s - star_cost] + v
                    if t > dp[e * w + s]:
                        dp[e * w + s] = t
        best = dp[energy * w + stars]
        for pe, flat, by_x in xe:
            p, b = pe[j], flat[j]
            if _x_live(p, b, by_x, j):
                for e in range(energy + 1):
                    cand = dp[e * w + stars] + _x_term(p, by_x, j, energy - e) + b
                    if cand > best:
                        best = cand
        for cost, ps, flat, by_x in xs:
            p, b = ps[j], flat[j]
            if _x_live(p, b, by_x, j):
                for s in range(stars + 1):
                    cand = dp[(energy - cost) * w + s] + _x_term(p, by_x, j, stars - s) + b
                    if cand > best:
                        best = cand
        out.append(fixed[j] + best)
    return out


def playable_from(actions: list[dict[str, Any]]) -> set[int]:
    """Hand indices some legal `play` action plays (combat action dicts)."""
    return {int(a["card_index"]) for a in actions if a.get("action") == "play"}


def tactical(
    snap: dict[str, Any],
    actions: list[dict[str, Any]],
    intents: list[list[dict[str, Any]]] | None = None,
    *,
    end_block: float = 0.0,
    ally_hp: float = 0.0,
) -> tuple[list[float], list[float], float]:
    """`(max_damage per enemy, killable per enemy, hp_loss)`, unscaled."""
    md = max_damage(snap, playable_from(actions))
    kill = []
    for j, m in enumerate(snap["enemies"]):
        alive = not m.get("is_dead")
        ehp = float(m.get("hp") or 0) + float(m.get("block") or 0)
        kill.append(float(alive and ehp > 0 and md[j] >= ehp))
    return md, kill, hp_loss(snap, intents, end_block=end_block, ally_hp=ally_hp)


def tactical_live(
    snap: dict[str, Any], actions: list[dict[str, Any]], player: dict[str, Any]
) -> tuple[list[float], list[float], float]:
    """`tactical` on a "chars" row (GH #1197): `(max_damage_live, killable, hp_loss_live)`
    of a `live_snapshot`, unscaled. Twin of `tactical::tactical_live`."""
    md = max_damage_live(snap, playable_from(actions))
    kill = []
    for j, m in enumerate(snap["enemies"]):
        alive = not m.get("is_dead")
        ehp = float(m.get("hp") or 0) + float(m.get("block") or 0)
        kill.append(float(alive and ehp > 0 and md[j] >= ehp))
    return md, kill, hp_loss_live(snap, player)


# ---- end of turn (GH #1197): `dies_before_acting`, `player_doomed`, `hp_loss_live`.
# The engine walks the deterministic end of the turn (`end_turn_live` in `sts2-wire`,
# the snapshot's `end_live`); these do the per-hit arithmetic on it. The old `hp_loss`
# stays as it was for the rows without the "chars" event.


def _power_amount(powers: list[dict[str, Any]] | None, name: str) -> float:
    return sum(float(p.get("amount") or 0) for p in powers or [] if p.get("name") == name)


def dies_before_acting(snap: dict[str, Any], player: dict[str, Any]) -> list[float]:
    """Per enemy of `snap["enemies"]`: 1.0 if it is alive and certain to die before
    its move resolves (`end_live.dies`): the deterministic end-of-turn damage (Glass,
    Lightning with one hittable enemy, Hailstorm, Consuming Shadow's evokes), then
    Poison at its turn start (`PoisonPower.Trigger`, Accelerant, block ignored).
    `player` is unused; the engine read the orbs."""
    del player
    dies = set((snap.get("end_live") or {}).get("dies") or [])
    return [
        float(j in dies and not m.get("is_dead") and float(m.get("hp") or 0) > 0)
        for j, m in enumerate(snap["enemies"])
    ]


def player_doomed(snap: dict[str, Any]) -> float:
    """1.0 if the player's Doom is at least its HP: it dies at the end of its own turn
    (`DoomPower.AfterSideTurnEnd`, `IsOwnerDoomed`: `CurrentHp <= Amount`)."""
    hp = float(snap["player"].get("hp") or 0)
    return float(hp > 0 and _power_amount(snap.get("powers"), DOOM) >= hp)


def _relic_index(snap: dict[str, Any], rid: str) -> int | None:
    for k, r in enumerate(snap.get("relics") or []):
        if r.get("id") == rid and not r.get("is_melted"):
            return k
    return None


def hp_loss_live(snap: dict[str, Any], player: dict[str, Any]) -> float:
    """The HP the player loses if the turn ended now, on a "chars" row.

    `end_live.self_loss` (the turn end's own self-damage: Burn-type cards, Constrict),
    then the attack intents of the living enemies not in `dies_before_acting`, hit by
    hit (enemy order, intent order, `times`), as `CreatureCmd.Damage` resolves each:
    the block (the player's plus `end_live.block`, its change before the enemies act)
    first, then a living Osty soaks the unblocked damage (`DieForYouPower`) and only
    the overkill passes on, then on the player Intangible caps it at 1; then the
    `ModifyHpLostAfterOsty` relics in the player's relic order (powers before relics
    in `IterateHookListeners`): Tungsten Rod takes 1 off, Beating Remnant caps it at
    the budget left (`end_live.remnant`, less each hit's HP lost); then a Buffer
    stack (`end_live.buffer`, what the self-damage left) zeroes a hit that still
    loses HP (`ModifyHpLostAfterOstyLate`).
    """
    end = snap.get("end_live") or {}
    block = float(snap["player"].get("block") or 0)
    block += float(end.get("block") or 0)
    osty = ally_hp_of(player)
    powers = snap.get("powers")
    buffer = float(end["buffer"]) if "buffer" in end else _power_amount(powers, "BUFFER")
    intangible = _power_amount(powers, "INTANGIBLE") > 0
    rod = _relic_index(snap, "TUNGSTEN_ROD")
    rem_at = _relic_index(snap, "BEATING_REMNANT")
    remnant = end.get("remnant")
    left = float(remnant) if remnant is not None and rem_at is not None else None
    rod_first = rod is not None and (rem_at is None or rod < rem_at)
    dies = dies_before_acting(snap, player)
    loss = float(end.get("self_loss") or 0)
    for j, m in enumerate(snap["enemies"]):
        if m.get("is_dead") or dies[j]:
            continue
        for i in m.get("intents") or []:
            if i.get("kind") not in ("Attack", "DeathBlow"):
                continue
            d = float(i.get("effective_amount", i.get("amount", 0)) or 0)
            for _ in range(int(i.get("times") or 1)):
                b = min(block, d)
                block -= b
                u = d - b
                if osty > 0.0:
                    s = min(u, osty)
                    osty -= s
                    u -= s
                if intangible and u >= 1.0:
                    u = 1.0
                if rod_first:
                    u = max(0.0, u - 1.0)
                if left is not None:
                    u = min(u, left)
                if rod is not None and not rod_first:
                    u = max(0.0, u - 1.0)
                if buffer > 0.0 and u > 0.0:
                    u = 0.0
                    buffer -= 1.0
                if left is not None:
                    left = max(0.0, left - u)
                loss += u
    return loss
