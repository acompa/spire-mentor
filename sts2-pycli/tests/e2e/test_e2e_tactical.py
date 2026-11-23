"""WP3b: tactical columns (`ppo.e2e.tactical`): knapsack damage, killable, HP loss."""

from __future__ import annotations

import time

import pytest
import sts2_client
from ppo.e2e import factory
from ppo.e2e import tactical as TC


@pytest.fixture(autouse=True, scope="module")
def _campaign_process():
    """The factory needs the process to select v0.111.0 (`STS2_GAME_VERSION`)."""
    with pytest.MonkeyPatch.context() as mp:
        mp.setattr(sts2_client, "GAME_VERSION", factory.GAME_VERSION)
        yield


def _snap(hand, energy, enemies, block=0):
    return {
        "player": {"energy": energy, "block": block, "hp": 50},
        "hand": hand,
        "enemies": enemies,
    }


def _card(cid, cost, dmg, base_cost=None):
    return {
        "id": cid,
        "cost": cost,
        "base_cost": cost if base_cost is None else base_cost,
        "dmg": dmg,
    }


def _enemy(hp, block=0, intents=(), dead=False):
    return {"hp": hp, "block": block, "intents": list(intents), "is_dead": dead}


def test_knapsack_picks_the_best_cards_under_energy() -> None:
    hand = [
        _card("STRIKE_IRONCLAD", 1, {"0": 6, "1": 6}),
        _card("BASH", 2, {"0": 8, "1": 8}),
        _card("STRIKE_IRONCLAD", 1, {"0": 6, "1": 6}),
        _card("DEFEND_IRONCLAD", 1, {}),
    ]
    s = _snap(hand, 3, [_enemy(12), _enemy(30)])
    md, kill, loss = TC.tactical(s, [{"action": "play", "card_index": i} for i in range(4)])
    assert md == [14.0, 14.0]  # Bash + a Strike beats two Strikes (12)
    assert kill == [1.0, 0.0]
    assert loss == pytest.approx(0.0)
    # Two energy: Bash alone (8) loses to two Strikes (12).
    assert TC.max_damage(_snap(hand, 2, [_enemy(12)]), {0, 1, 2}) == [12.0]


def test_hit_counts_x_cost_zero_cost_and_unplayable() -> None:
    hand = [
        _card("TWIN_STRIKE", 1, {"0": 5}),  # 2 hits
        _card("WHIRLWIND", 0, {"0": 5}, base_cost=-1),  # X: hits = energy left
        _card("ANGER", 0, {"0": 6}),  # free: always played
        _card("STRIKE_IRONCLAD", 1, {"0": 6}),  # not playable below
    ]
    s = _snap(hand, 3, [_enemy(100)])
    # Twin Strike (10) + Whirlwind x2 (10) + Anger (6) = 26 > Whirlwind x3 (15) + 6.
    assert TC.max_damage(s, {0, 1, 2}) == [26.0]
    assert TC.hits_of("TWIN_STRIKE") == 2 and TC.hits_of("STRIKE_IRONCLAD") == 1


def test_block_and_dead_enemies_and_hp_loss() -> None:
    atk = {"kind": "Attack", "amount": 5, "effective_amount": 7, "times": 2}
    s = _snap(
        [_card("STRIKE_IRONCLAD", 1, {"0": 6})],
        1,
        [_enemy(5, block=2, intents=[atk]), _enemy(0, dead=True, intents=[atk])],
        block=4,
    )
    md, kill, loss = TC.tactical(s, [{"action": "play", "card_index": 0, "target_index": 0}])
    assert md == [6.0, 0.0]
    assert kill == [0.0, 0.0]  # 6 < 5 HP + 2 block; the dead enemy is never killable
    assert loss == pytest.approx(10.0)  # the dead enemy's intent does not count
    # A full telegraph overrides the displayed head intent.
    two = [[atk, atk], []]
    assert TC.hp_loss(s, two) == pytest.approx(24.0)


def test_fast_enough_for_the_worker() -> None:
    hand = [_card("STRIKE_IRONCLAD", 1, {"0": 6, "1": 6, "2": 6}) for _ in range(10)]
    s = _snap(hand, 5, [_enemy(40), _enemy(40), _enemy(40)])
    acts = [{"action": "play", "card_index": i} for i in range(10)]
    TC.tactical(s, acts)
    n = 2000
    t0 = time.perf_counter()
    for _ in range(n):
        TC.tactical(s, acts)
    us = (time.perf_counter() - t0) / n * 1e6
    # Plan target 20-50 us; a 10-card, 5-energy, 3-enemy hand is a heavy case
    # and the laptop is shared, so the bound is loose.
    assert us < 400, us


@pytest.mark.parametrize("seed", [5, 6])
def test_on_a_real_fight(seed: int) -> None:
    from ppo.combat_ctx.snapshot import honest_snapshot

    cs = factory.scenario_state(
        seed=seed,
        character="Ironclad",
        deck=["TWIN_STRIKE"] * 3 + ["BASH"] * 2 + ["STRIKE_IRONCLAD"] * 5,
        hp=70,
        max_hp=80,
        relics=["BURNING_BLOOD"],
        encounter="SLIMES_WEAK",
        ascension=10,
    )
    snap = honest_snapshot(cs.belief_view(0), "monster")
    md, kill, loss = TC.tactical(snap, cs.legal_actions())
    assert len(md) == len(kill) == len(snap["enemies"])
    assert all(x >= 0 for x in md) and loss >= 0
    assert max(md) > 0


# ---- `max_damage_live` (GH #1197). The cases mirror the `bound_` tests of
# `sts2-e2e/src/tactical.rs` one for one.


def _live_snap(energy, stars, hand, enemies):
    return {
        "player": {"energy": energy, "stars": stars, "block": 0},
        "hand": hand,
        "enemies": enemies,
    }


def _all(n):
    return set(range(n))


def test_bound_body_slam_counts() -> None:
    hand = [
        {"id": "BODY_SLAM", "cost": 1, "base_cost": 1, "dmg": {}, "dmg_live": {"0": [12, 1.0]}},
    ]
    s = _live_snap(1, 0, hand, [_enemy(40)])
    assert TC.max_damage(s, _all(1)) == [0.0]
    assert TC.max_damage_live(s, _all(1)) == [12.0]


def test_bound_fiend_fire_hits() -> None:
    hand = [
        {
            "id": "FIEND_FIRE",
            "cost": 2,
            "base_cost": 2,
            "dmg": {"0": 7},
            "dmg_live": {"0": [7, 4.0]},
        },
        {
            "id": "STRIKE_IRONCLAD",
            "cost": 1,
            "base_cost": 1,
            "dmg": {"0": 6},
            "dmg_live": {"0": [6, 1.0]},
        },
    ]
    s = _live_snap(2, 0, hand, [_enemy(40)])
    assert TC.max_damage_live(s, _all(2)) == [28.0]
    assert TC.max_damage_live(s, {1}) == [6.0]


def test_bound_star_budget_limits() -> None:
    hand = [
        {"id": "FALLING_STAR", "cost": 0, "base_cost": 0, "star_cost_live": 2, "base_star_cost": 2,
         "dmg": {"0": 8}, "dmg_live": {"0": [8, 1.0]}},
        {"id": "METEOR_SHOWER", "cost": 0, "base_cost": 0, "star_cost_live": 2, "base_star_cost": 2,
         "dmg": {"0": 14}, "dmg_live": {"0": [14, 1.0]}},
    ]  # fmt: skip
    enemies = [_enemy(99)]
    # Three stars pay for one of the two: the bigger one.
    assert TC.max_damage_live(_live_snap(3, 3, hand, enemies), _all(2)) == [14.0]
    assert TC.max_damage_live(_live_snap(3, 4, hand, enemies), _all(2)) == [22.0]
    # Star-X (Stardust) spends whatever stars are left, 5 per star here.
    hand = [
        *hand,
        {"id": "STARDUST", "cost": 0, "base_cost": 0, "star_cost_live": -1, "base_star_cost": -1,
         "dmg": {"0": 5}, "dmg_live": {"0": [5, 3.0]}},
    ]  # fmt: skip
    # Meteor Shower (14) + one star of Stardust (5) beats Stardust x3 (15).
    assert TC.max_damage_live(_live_snap(0, 3, hand, enemies), _all(3)) == [19.0]
    # No stars: neither the star cards nor Stardust.
    assert TC.max_damage_live(_live_snap(3, 0, hand, enemies), _all(3)) == [0.0]


def test_bound_osty_dead_zero() -> None:
    hand = [{"id": "POKE", "cost": 1, "base_cost": 1, "dmg": {"0": 9}, "dmg_live": {}}]
    s = _live_snap(1, 0, hand, [_enemy(40)])
    assert TC.max_damage(s, _all(1)) == [9.0]
    assert TC.max_damage_live(s, _all(1)) == [0.0]


def test_bound_barrage_orbs() -> None:
    hand = [
        {"id": "BARRAGE", "cost": 1, "base_cost": 1, "dmg": {"0": 5, "1": 5},
         "dmg_live": {"0": [5, 3.0], "1": [5, 3.0]}},
        {"id": "WHIRLWIND", "cost": 0, "base_cost": -1, "dmg": {"0": 5, "1": 5},
         "dmg_live": {"0": [5, 1.0], "1": [5, 1.0]}},
    ]  # fmt: skip
    s = _live_snap(3, 0, hand, [_enemy(40), _enemy(40)])
    # Barrage x3 (15) + Whirlwind x2 (10) beats Whirlwind x3 (15).
    assert TC.max_damage_live(s, _all(2)) == [25.0, 25.0]


def test_bound_end_of_days_doom_kill() -> None:
    hand = [
        {"id": "END_OF_DAYS", "cost": 3, "base_cost": 3, "dmg": {}, "dmg_live": {},
         "gen_live": {"flat": {}, "per_energy": {}, "kills": [0]}},
        {"id": "STRIKE_NECROBINDER", "cost": 1, "base_cost": 1, "dmg": {"0": 6, "1": 6},
         "dmg_live": {"0": [6, 1.0], "1": [6, 1.0]}},
    ]  # fmt: skip
    enemies = [_enemy(20, block=5), _enemy(50)]
    # Lethal on the enemy its Doom kills: HP + block.
    assert TC.max_damage_live(_live_snap(3, 0, hand, enemies), _all(2)) == [25.0, 6.0]
    # Unaffordable: no kill.
    assert TC.max_damage_live(_live_snap(2, 0, hand, enemies), _all(2)) == [6.0, 6.0]


def test_bound_blade_dance_shivs() -> None:
    hand = [
        {"id": "BLADE_DANCE", "cost": 1, "base_cost": 1, "dmg": {}, "dmg_live": {},
         "gen_live": {"flat": {"0": 12.0, "1": 12.0}, "per_energy": {}, "kills": []}},
        {"id": "STRIKE_SILENT", "cost": 1, "base_cost": 1, "dmg": {"0": 6, "1": 6},
         "dmg_live": {"0": [6, 1.0], "1": [6, 1.0]}},
    ]  # fmt: skip
    enemies = [_enemy(40), _enemy(40)]
    assert TC.max_damage_live(_live_snap(1, 0, hand, enemies), _all(2)) == [12.0, 12.0]
    assert TC.max_damage_live(_live_snap(2, 0, hand, enemies), _all(2)) == [18.0, 18.0]


def test_bound_dualcast_evoke() -> None:
    hand = [
        {"id": "DUALCAST", "cost": 1, "base_cost": 1, "dmg": {}, "dmg_live": {},
         "gen_live": {"flat": {"0": 16.0}, "per_energy": {}, "kills": []}},
    ]  # fmt: skip
    assert TC.max_damage_live(_live_snap(1, 0, hand, [_enemy(40)]), _all(1)) == [16.0]
    # Multi-Cast+: X evokes plus one, so per energy and a flat evoke.
    hand = [
        {"id": "MULTI_CAST", "cost": 0, "base_cost": -1, "upgraded": True, "dmg": {}, "dmg_live": {},
         "gen_live": {"flat": {"0": 8.0}, "per_energy": {"0": 8.0}, "kills": []}},
    ]  # fmt: skip
    assert TC.max_damage_live(_live_snap(3, 0, hand, [_enemy(40)]), _all(1)) == [32.0]


def test_bound_tempest_by_x() -> None:
    """Tempest: its overflow evokes by the energy spent (`gen_live.by_x`), not linear
    in X; the knapsack reads the table at the energy the X card gets."""

    def tempest(by_x):
        return {"id": "TEMPEST", "cost": 0, "base_cost": -1, "dmg": {}, "dmg_live": {},
                "gen_live": {"flat": {}, "per_energy": {}, "kills": [], "by_x": by_x}}  # fmt: skip

    def strike(d):
        return {"id": "STRIKE_DEFECT", "cost": 1, "base_cost": 1, "dmg": {}, "dmg_live": {"0": [d, 1.0]}}  # fmt: skip

    table = [{}, {}, {"0": 8.0}, {"0": 16.0}]
    # X=3 (16) beats Strike + X=2 (7 + 8).
    s = _live_snap(3, 0, [strike(7), tempest(table)], [_enemy(40)])
    assert TC.max_damage_live(s, _all(2)) == [16.0]
    # Strike + X=2 (9 + 8) beats X=3.
    s = _live_snap(3, 0, [strike(9), tempest(table)], [_enemy(40)])
    assert TC.max_damage_live(s, _all(2)) == [17.0]
    # Alone, at X=0 the table's first entry counts.
    s = _live_snap(0, 0, [tempest([{"0": 8.0}])], [_enemy(40)])
    assert TC.max_damage_live(s, _all(1)) == [8.0]


def test_bound_x_table_replaces_the_linear_term() -> None:
    """An X card with a `by_x` table (Chemical X, Heavenly Drill's doubling): the table
    is its whole X damage, replacing `dmg_live x X`; a star-X card indexes it by the
    stars spent."""
    table = [{"0": 10.0}, {"0": 15.0}, {"0": 20.0}]
    gen = {"flat": {}, "per_energy": {}, "kills": [], "by_x": table}
    ww = {"id": "WHIRLWIND", "cost": 0, "base_cost": -1, "dmg": {}, "dmg_live": {"0": [5, 1.0]},
          "gen_live": gen}  # fmt: skip
    assert TC.max_damage_live(_live_snap(2, 0, [ww], [_enemy(40)]), _all(1)) == [20.0]
    # A 1-cost Strike and X=1 (6 + 15) beat X=2 (20).
    strike = {"id": "STRIKE_IRONCLAD", "cost": 1, "base_cost": 1, "dmg": {}, "dmg_live": {"0": [6, 1.0]}}  # fmt: skip
    assert TC.max_damage_live(_live_snap(2, 0, [strike, ww], [_enemy(40)]), _all(2)) == [21.0]
    # Star-X: by the stars spent.
    sd = {"id": "STARDUST", "cost": 0, "base_cost": 0, "star_cost_live": -1, "dmg": {},
          "dmg_live": {"0": [5, 2.0]}, "gen_live": gen}  # fmt: skip
    assert TC.max_damage_live(_live_snap(0, 2, [sd], [_enemy(40)]), _all(1)) == [20.0]


def test_bound_undeclared_unchanged() -> None:
    # The old bound on a fixed hand, with and without the live keys.
    plain = [
        {"id": "TWIN_STRIKE", "cost": 1, "base_cost": 1, "dmg": {"0": 5}},
        {"id": "WHIRLWIND", "cost": 0, "base_cost": -1, "dmg": {"0": 5}},
        {"id": "ANGER", "cost": 0, "base_cost": 0, "dmg": {"0": 6}},
        {"id": "STRIKE_IRONCLAD", "cost": 1, "base_cost": 1, "dmg": {"0": 6}},
    ]
    s = _live_snap(3, 0, plain, [_enemy(100)])
    assert TC.max_damage(s, {0, 1, 2}) == [26.0]
    live = [
        {
            **c,
            "dmg_live": {"0": [50, 9.0]},
            "star_cost_live": 0,
            "gen_live": {"flat": {"0": 99.0}, "per_energy": {}, "kills": [0]},
        }
        for c in plain
    ]
    s = _live_snap(3, 0, live, [_enemy(100)])
    assert TC.max_damage(s, {0, 1, 2}) == [26.0]


def _live_fight(character: str, card: str, relics: list[str]):
    from ppo.e2e.env import combat_view

    cs = factory.scenario_state(
        seed=5,
        character=character,
        deck=[card] * 10,
        hp=70,
        max_hp=80,
        relics=relics,
        encounter="SLIMES_WEAK",
        ascension=10,
    )
    bv, d, snap = combat_view(cs, "monster")
    return cs, TC.live_snapshot(snap, bv, d)


def test_bound_live_keys_on_a_real_fight() -> None:
    """A "chars" row's snapshot (`live_snapshot`) carries the live keys, read through
    the new bindings; the env's own snapshot does not."""
    from ppo.e2e.env import combat_view

    cs, snap = _live_fight("Ironclad", "FIEND_FIRE", ["BURNING_BLOOD"])
    assert "end_live" not in combat_view(cs, "monster")[2]
    hand = snap["hand"]
    living = [str(j) for j, m in enumerate(snap["enemies"]) if not m.get("is_dead")]
    assert len(living) >= 2 and len(hand) >= 3
    # Fiend Fire hits once per other card in the hand.
    for c in hand:
        assert sorted(c["dmg_live"]) == sorted(living)
        assert [h for _d, h in c["dmg_live"].values()] == [float(len(hand) - 1)] * len(living)
        assert "gen_live" not in c and "star_cost_live" not in c
    md = TC.max_damage_live(snap, TC.playable_from(cs.legal_actions()))
    for j in map(int, living):
        d, h = hand[0]["dmg_live"][str(j)]
        assert md[j] == pytest.approx(float(d) * h)  # 3 energy: one Fiend Fire

    cs, snap = _live_fight("Silent", "BLADE_DANCE", ["RING_OF_THE_SNAKE"])
    for c in snap["hand"]:
        assert c["dmg_live"] == {}
        assert c["gen_live"]["flat"] == {j: 12.0 for j in living}  # three 4-damage Shivs

    cs, snap = _live_fight("Necrobinder", "END_OF_DAYS", ["BOUND_PHYLACTERY"])
    doomable = [j for j, m in enumerate(snap["enemies"]) if not m.get("is_dead") and m["hp"] <= 29]
    assert doomable  # the weak slimes all sit under 29 HP
    md = TC.max_damage_live(snap, TC.playable_from(cs.legal_actions()))
    for c in snap["hand"]:
        assert c["gen_live"]["kills"] == doomable
    for j in doomable:
        m = snap["enemies"][j]
        assert md[j] == pytest.approx(float(m["hp"] + (m.get("block") or 0)))

    _cs, snap = _live_fight("Regent", "FALLING_STAR", ["DIVINE_RIGHT"])
    for c in snap["hand"]:
        assert c["star_cost_live"] == 2

    # Tempest: Cracked Core's Lightning in a 3-slot belt, 3 energy. Only X=3
    # overflows, evoking that Lightning (`gen_live.by_x`, by energy spent).
    cs, snap = _live_fight("Defect", "TEMPEST", ["CRACKED_CORE"])
    assert snap["player"]["energy"] == 3
    md = TC.max_damage_live(snap, TC.playable_from(cs.legal_actions()))
    for c in snap["hand"]:
        by_x = c["gen_live"]["by_x"]
        assert len(by_x) == 4 and by_x[:3] == [{}, {}, {}]
        assert sorted(by_x[3]) == sorted(living)
        for j, v in by_x[3].items():
            assert md[int(j)] == v


# ---- end of turn: `dies_before_acting`, `player_doomed`, `hp_loss_live` (GH #1197).
# The cases mirror the Rust twins in `sts2-e2e/src/tactical.rs` one for one. The
# mechanics cases build a real engine state (a v0.111.0 Defect fight edited through
# the wire dict), so `end_live` comes from `CombatState.end_turn_live()`.


def _end_fight(
    hp=None,
    block=None,
    enemy_powers=None,
    dead=(),
    orbs=(),
    relics=(),
    powers=(),
    player_block=0,
):
    """A v0.111.0 Defect fight (SLIMES_WEAK, enemies at 12/11/36 HP), edited: the
    snapshot the env builds of it (`combat_view`) and the player dict."""
    import sts2_rs
    from ppo.e2e.env import combat_view

    cs = factory.scenario_state(
        seed=5,
        character="Defect",
        deck=["ZAP"] * 10,
        hp=70,
        max_hp=80,
        relics=["CRACKED_CORE"],
        encounter="SLIMES_WEAK",
        ascension=10,
    )
    d = sts2_rs.state_to_dict(cs)
    for j, m in enumerate(d["encounter"]):
        if hp is not None:
            m["hp"] = hp[j]
        if block is not None:
            m["block"] = block[j]
        m["powers"] = [{"name": n, "amount": a} for n, a in (enemy_powers or {}).get(j, [])]
        if j in dead:
            m["hp"] = 0
            m["is_dead"] = True
    d["player"]["orbs"] = [{"type": t, "passive": p, "evoke": e} for t, p, e in orbs]
    d["player"]["relics"] = [{"id": r} for r in relics]
    d["player"]["powers"] = [{"name": n, "amount": a} for n, a in powers]
    d["player"]["block"] = player_block
    bv, d_bv, snap = combat_view(sts2_rs.state_from_dict(d), "monster")
    return TC.live_snapshot(snap, bv, d_bv), d_bv["player"]


def _attack(dmg, times=1):
    return {"kind": "Attack", "amount": dmg, "effective_amount": dmg, "times": times}


def _end_snap(enemies, block=0, hp=50, powers=(), relics=(), end_live=None):
    s = {
        "player": {"energy": 0, "block": block, "hp": hp},
        "powers": [{"name": n, "amount": a} for n, a in powers],
        "relics": [{"id": r} for r in relics],
        "hand": [],
        "enemies": enemies,
    }
    if end_live is not None:
        s["end_live"] = end_live
    return s


def test_poison_lethal_flags_enemy() -> None:
    # Unblockable: the 5 block does not save the 12-HP enemy from 12 Poison.
    snap, p = _end_fight(block=[5, 0, 0], enemy_powers={0: [("POISON", 12)]})
    assert TC.dies_before_acting(snap, p) == [1.0, 0.0, 0.0]
    snap, p = _end_fight(enemy_powers={0: [("POISON", 11)]})
    assert TC.dies_before_acting(snap, p) == [0.0, 0.0, 0.0]


def test_poison_accelerant_counts() -> None:
    snap, p = _end_fight(enemy_powers={0: [("POISON", 7)]})
    assert TC.dies_before_acting(snap, p) == [0.0, 0.0, 0.0]
    # Accelerant 1: two ticks, 7 + 6 = 13 >= 12.
    snap, p = _end_fight(enemy_powers={0: [("POISON", 7)]}, powers=[("ACCELERANT", 1)])
    assert TC.dies_before_acting(snap, p) == [1.0, 0.0, 0.0]


def test_poisoned_enemy_attack_dropped() -> None:
    enemies = [_enemy(5, intents=[_attack(6)]), _enemy(30, intents=[_attack(5)])]
    s = _end_snap(enemies, end_live={"block": 0, "dies": [0]})
    assert TC.dies_before_acting(s, {}) == [1.0, 0.0]
    assert TC.hp_loss_live(s, {}) == pytest.approx(5.0)
    assert TC.hp_loss(s) == pytest.approx(11.0)


def test_glass_kills_all() -> None:
    orbs = [("Glass", 4, 8)]
    snap, p = _end_fight(hp=[4, 4, 36], block=[0, 1, 0], orbs=orbs)
    assert TC.dies_before_acting(snap, p) == [1.0, 0.0, 0.0]
    # Gold-Plated Cables: the front Glass again, one lower (4 + 3).
    snap, p = _end_fight(hp=[4, 4, 36], block=[0, 1, 0], orbs=orbs, relics=["GOLD_PLATED_CABLES"])
    assert TC.dies_before_acting(snap, p) == [1.0, 1.0, 0.0]


def test_lightning_single_enemy() -> None:
    orbs = [("Lightning", 3, 8)]
    snap, p = _end_fight(hp=[3, 3, 3], orbs=orbs)
    assert TC.dies_before_acting(snap, p) == [0.0, 0.0, 0.0]  # a random target
    snap, p = _end_fight(hp=[3, 3, 3], dead=(0, 1), orbs=orbs)
    assert TC.dies_before_acting(snap, p) == [0.0, 0.0, 1.0]


def test_gold_plated_cables_double_frost() -> None:
    frost = [("Frost", 2, 5), ("Frost", 2, 5)]
    snap, p = _end_fight(orbs=frost)
    assert snap["end_live"]["block"] == 4
    snap, p = _end_fight(orbs=frost, relics=["GOLD_PLATED_CABLES"])
    assert snap["end_live"]["block"] == 6
    for m in snap["enemies"]:
        m["intents"] = [_attack(4, 2)]
    # 24 incoming, 6 block before the enemies act.
    assert TC.hp_loss_live(snap, p) == pytest.approx(18.0)


def test_plating_orichalcum_cloak_clasp_block() -> None:
    relics = ["ORICHALCUM", "FAKE_ORICHALCUM", "CLOAK_CLASP", "RIPPLE_BASIN"]
    snap, p = _end_fight(relics=relics, powers=[("PLATING", 4)])
    n = len(snap["hand"])
    assert n > 0
    assert snap["end_live"]["block"] == 4 + 6 + 3 + n + 4
    # Orichalcum and its fake latch only on 0 block.
    snap, p = _end_fight(relics=relics, powers=[("PLATING", 4)], player_block=5)
    assert snap["end_live"]["block"] == 4 + n + 4
    snap["enemies"] = [_enemy(30, intents=[_attack(20)])]
    assert TC.hp_loss_live(snap, p) == pytest.approx(float(max(0, 20 - 5 - (4 + n + 4))))


def test_buffer_prevents_loss() -> None:
    enemies = [_enemy(30, intents=[_attack(6, 2)])]
    assert TC.hp_loss_live(_end_snap(enemies, powers=[("BUFFER", 1)]), {}) == pytest.approx(6.0)
    # A fully blocked hit does not spend the Buffer.
    assert TC.hp_loss_live(
        _end_snap(enemies, block=6, powers=[("BUFFER", 1)]), {}
    ) == pytest.approx(0.0)
    assert TC.hp_loss_live(_end_snap(enemies, block=6), {}) == pytest.approx(6.0)


def test_osty_then_intangible_order() -> None:
    osty = {"osty": {"alive": True, "hp": 2, "max_hp": 10}}
    # Intangible caps each telegraphed hit at 1: the Osty soaks two, the
    # player loses 1 a hit after it.
    enemies = [_enemy(30, intents=[_attack(1, 4)])]
    assert TC.hp_loss_live(_end_snap(enemies, powers=[("INTANGIBLE", 1)]), osty) == pytest.approx(
        2.0
    )
    # An uncapped hit: the Osty takes 4, the overkill 6 is capped to 1 on the
    # player (after the Osty), then Tungsten Rod takes 1 off.
    osty = {"osty": {"alive": True, "hp": 4, "max_hp": 10}}
    enemies = [_enemy(30, intents=[_attack(10)])]
    s = _end_snap(enemies, powers=[("INTANGIBLE", 1)])
    assert TC.hp_loss_live(s, osty) == pytest.approx(1.0)
    s = _end_snap(enemies, powers=[("INTANGIBLE", 1)], relics=["TUNGSTEN_ROD"])
    assert TC.hp_loss_live(s, osty) == pytest.approx(0.0)
    s = _end_snap(enemies, relics=["TUNGSTEN_ROD"])
    assert TC.hp_loss_live(s, osty) == pytest.approx(5.0)
    assert TC.hp_loss_live(s, {}) == pytest.approx(9.0)


def test_self_damage_buffer_and_block() -> None:
    # Constrict (`AfterSideTurnEnd`) hits the player before the enemies act.
    snap, p = _end_fight(powers=[("CONSTRICT", 3)])
    assert (snap["end_live"]["block"], snap["end_live"]["self_loss"]) == (0, 3)
    snap["enemies"] = []
    assert TC.hp_loss_live(snap, p) == pytest.approx(3.0)
    # Through block first: the enemies then see 5 - 3.
    snap, p = _end_fight(powers=[("CONSTRICT", 3)], player_block=5)
    assert (snap["end_live"]["block"], snap["end_live"]["self_loss"]) == (-3, 0)
    snap["enemies"] = [_enemy(30, intents=[_attack(6)])]
    assert TC.hp_loss_live(snap, p) == pytest.approx(4.0)
    # It spends the Buffer stack, so the attack lands in full.
    snap, p = _end_fight(powers=[("CONSTRICT", 3), ("BUFFER", 1)])
    assert (snap["end_live"]["self_loss"], snap["end_live"]["buffer"]) == (0, 0)
    snap["enemies"] = [_enemy(30, intents=[_attack(6)])]
    assert TC.hp_loss_live(snap, p) == pytest.approx(6.0)


def test_beating_remnant_relic_order() -> None:
    snap, _ = _end_fight(relics=["BEATING_REMNANT"])
    assert snap["end_live"]["remnant"] == 20
    enemies = [_enemy(30, intents=[_attack(10, 3)])]
    live = {"block": 0, "dies": [], "self_loss": 0, "buffer": 0, "remnant": 12}
    # The budget caps the HP lost: 10, 2, 0.
    s = _end_snap(enemies, relics=["BEATING_REMNANT"], end_live=live)
    assert TC.hp_loss_live(s, {}) == pytest.approx(12.0)
    # Remnant before Tungsten Rod: 10 - 1, 3 - 1, 1 - 1.
    s = _end_snap(enemies, relics=["BEATING_REMNANT", "TUNGSTEN_ROD"], end_live=live)
    assert TC.hp_loss_live(s, {}) == pytest.approx(11.0)
    # Rod first: 9, then min(9, 3), then 0.
    s = _end_snap(enemies, relics=["TUNGSTEN_ROD", "BEATING_REMNANT"], end_live=live)
    assert TC.hp_loss_live(s, {}) == pytest.approx(12.0)
    # The self-damage counts on top of the hits.
    s = _end_snap(enemies, end_live={**live, "self_loss": 4, "remnant": None})
    assert TC.hp_loss_live(s, {}) == pytest.approx(34.0)


def test_player_doom_flag() -> None:
    assert TC.player_doomed(_end_snap([], hp=10, powers=[("DOOM", 10)])) == pytest.approx(1.0)
    assert TC.player_doomed(_end_snap([], hp=11, powers=[("DOOM", 10)])) == pytest.approx(0.0)
    assert TC.player_doomed(_end_snap([], hp=10)) == pytest.approx(0.0)


def test_old_hp_loss_unchanged() -> None:
    enemies = [_enemy(5, intents=[_attack(6, 3)]), _enemy(0, dead=True, intents=[_attack(9)])]
    plain = _end_snap(enemies, block=5)
    assert TC.hp_loss(plain, end_block=2.0, ally_hp=3.0) == pytest.approx(8.0)
    live = _end_snap(
        enemies,
        block=5,
        powers=[("BUFFER", 3), ("INTANGIBLE", 1)],
        relics=["TUNGSTEN_ROD"],
        end_live={"block": 9, "dies": [0]},
    )
    assert TC.hp_loss(live, end_block=2.0, ally_hp=3.0) == pytest.approx(8.0)
