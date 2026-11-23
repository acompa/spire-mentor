//! v0.113.0 `Thumbscrew` (GH #1495): `ModifyAttackHitCount` gives an attack
//! one more hit when its `ModelSource` is an Attack card the owner owns and
//! its hit count is already above 1 (`Models.Relics/Thumbscrew.cs`). C# folds
//! the hook once per `AttackCommand.Execute` (`AttackCommand.cs:557`), on the
//! final `WithHitCount` value, so X-cost and calculated counts get it too.
//! Expected values are read off the decomp, not off the engine.
#![allow(dead_code)]

use smallvec::SmallVec;
use std::num::NonZeroU32;

use sts2_content::cards::dispatch as content_dispatch;
use sts2_content::cards::v0111::card_pod_for;
use sts2_core::cards::CardId;
use sts2_core::potions::CharacterId;
use sts2_core::{
    CombatPhase, CombatState, Creature, CreatureUid, GameVersion, IntentHistory, MonsterId,
    MonsterKind, Power, PowerKind, PowerUid, RelicKind,
};

const V111: GameVersion = GameVersion::V0_111_0;
const V113: GameVersion = GameVersion::V0_113_0;
const ENEMY_HP: i32 = 5000;

fn add_enemy(state: &mut CombatState, raw: u32) -> CreatureUid {
    let id = NonZeroU32::new(raw).expect("nz");
    state.creatures.push(Creature {
        id,
        current_hp: ENEMY_HP,
        max_hp: ENEMY_HP,
        block: 0,
        powers: SmallVec::new(),
        species: Some(MonsterKind::Nibbit),
        intent: None,
        intent_history: IntentHistory::default(),
        monster_id: MonsterId("Nibbit"),
        ai: None,
    });
    state.enemy_uids.push(CreatureUid(id));
    CreatureUid(id)
}

fn fresh(version: GameVersion, character: CharacterId, enemies: u32) -> CombatState {
    content_dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    let mut state = CombatState {
        run_rng: sts2_core::run_rng_set::RunRngSet::new(4242_i32),
        game_version: version,
        ..CombatState::default()
    };
    for i in 0..enemies {
        let _ = add_enemy(&mut state, 1 + i);
    }
    state.player.current_hp = 80;
    state.player.max_hp = 80;
    state.player.energy = 3;
    state.player.max_energy = 3;
    state.player.character = Some(character);
    let _ = state.init_player_creature();
    state.round = 1;
    state.phase = CombatPhase::PlayerPlayPhase;
    state
}

fn give_thumbscrew(state: &mut CombatState) {
    let raw = u32::try_from(state.relics.len() + 1).expect("u32");
    state.relics.push(sts2_core::Relic {
        id: NonZeroU32::new(raw).expect("nz"),
        kind: RelicKind::Thumbscrew,
        counter: 0,
        is_wax: false,
        is_melted: false,
    });
}

fn push_hand(state: &mut CombatState, id: CardId, upgraded: bool) -> u8 {
    let raw = u32::try_from(state.cards.len() + 1).expect("u32");
    let uid = sts2_core::CardUid::new(raw).expect("nz");
    let mut card = card_pod_for(id, upgraded, state.game_version).expect("spec");
    card.uid = uid;
    state.cards.push(card);
    state.player.hand.push(uid);
    u8::try_from(state.player.hand.len() - 1).expect("u8")
}

fn add_enemy_power(state: &mut CombatState, uid: CreatureUid, kind: PowerKind, amount: i32) {
    let pid = NonZeroU32::new(900 + u32::try_from(state.powers.len()).expect("u32")).expect("nz");
    state.powers.push(Power::new(pid, kind, amount));
    state
        .creatures
        .iter_mut()
        .find(|c| c.id == uid.0)
        .expect("creature")
        .powers
        .push(PowerUid(pid));
}

fn total_enemy_hp_lost(state: &CombatState) -> i32 {
    state
        .enemy_uids
        .iter()
        .map(|u| {
            let c = state.creatures.iter().find(|c| c.id == u.0).expect("enemy");
            ENEMY_HP - c.current_hp
        })
        .sum()
}

/// Play `card` once against enemy 0 (after `setup`) and return the HP that
/// play took off.
fn damage_of(
    version: GameVersion,
    character: CharacterId,
    card: CardId,
    thumbscrew: bool,
    setup: fn(&mut CombatState),
) -> i32 {
    let mut state = fresh(version, character, 1);
    if thumbscrew {
        give_thumbscrew(&mut state);
    }
    setup(&mut state);
    let before = total_enemy_hp_lost(&state);
    let i = push_hand(&mut state, card, false);
    let _ = sts2_core::combat_loop::play_card(&mut state, i, Some(0));
    total_enemy_hp_lost(&state) - before
}

fn no_setup(_: &mut CombatState) {}

fn vulnerable_enemy(state: &mut CombatState) {
    let t = state.enemy_uids[0];
    add_enemy_power(state, t, PowerKind::Vulnerable, 3);
}

fn lost_hp_this_turn(state: &mut CombatState) {
    state.combat_log.hp_lost_this_turn = 1;
}

fn three_cards_in_hand(state: &mut CombatState) {
    for _ in 0..3 {
        let _ = push_hand(state, CardId::DefendIronclad, false);
    }
}

fn two_attacks_played(state: &mut CombatState) {
    for _ in 0..2 {
        let i = push_hand(state, CardId::StrikeSilent, false);
        let _ = sts2_core::combat_loop::play_card(state, i, Some(0));
    }
    state.player.energy = 3;
}

/// `(card, character, base hits C# passes to WithHitCount, setup)`.
type Row = (CardId, CharacterId, i32, fn(&mut CombatState));

/// Every multi-hit Attack card whose C# `OnPlay` builds ONE `AttackCommand`
/// with `WithHitCount(n)`, n > 1 here (v0.113.1 decomp `Models.Cards/`).
const MULTI_HIT: &[Row] = &[
    (CardId::TwinStrike, CharacterId::Ironclad, 2, no_setup),
    (CardId::SwordBoomerang, CharacterId::Ironclad, 3, no_setup),
    // X = 3 energy.
    (CardId::Whirlwind, CharacterId::Ironclad, 3, no_setup),
    (CardId::Conflagration, CharacterId::Ironclad, 4, no_setup),
    (CardId::FightMe, CharacterId::Ironclad, 2, no_setup),
    (CardId::Thrash, CharacterId::Ironclad, 2, no_setup),
    (
        CardId::Dismantle,
        CharacterId::Ironclad,
        2,
        vulnerable_enemy,
    ),
    (CardId::Spite, CharacterId::Ironclad, 2, lost_hp_this_turn),
    (
        CardId::FiendFire,
        CharacterId::Ironclad,
        3,
        three_cards_in_hand,
    ),
    (CardId::DaggerSpray, CharacterId::Silent, 2, no_setup),
    (CardId::Ricochet, CharacterId::Silent, 4, no_setup),
    (CardId::Skewer, CharacterId::Silent, 3, no_setup),
    (CardId::Finisher, CharacterId::Silent, 2, two_attacks_played),
    (CardId::GunkUp, CharacterId::Defect, 3, no_setup),
    (CardId::Refract, CharacterId::Defect, 2, no_setup),
    (CardId::Uproar, CharacterId::Defect, 2, no_setup),
    (CardId::Eradicate, CharacterId::Necrobinder, 3, no_setup),
    (CardId::SevenStars, CharacterId::Regent, 7, no_setup),
    (CardId::AstralPulse, CharacterId::Regent, 2, no_setup),
    (CardId::CelestialMight, CharacterId::Regent, 3, no_setup),
    (CardId::HeavenlyDrill, CharacterId::Regent, 3, no_setup),
    (CardId::Exterminate, CharacterId::Ironclad, 4, no_setup),
    (CardId::Maul, CharacterId::Ironclad, 2, no_setup),
    (CardId::Peck, CharacterId::Ironclad, 3, no_setup),
    (CardId::Volley, CharacterId::Ironclad, 3, no_setup),
    (CardId::RipAndTear, CharacterId::Ironclad, 2, no_setup),
    (CardId::WahKi, CharacterId::Fool, 2, no_setup),
    (CardId::RapidShot, CharacterId::Fool, 3, no_setup),
    (CardId::RepeatShot, CharacterId::Fool, 3, no_setup),
];

#[test]
fn thumbscrew_adds_one_hit_to_every_multi_hit_attack_card() {
    let mut failures = Vec::new();
    for &(card, character, base_hits, setup) in MULTI_HIT {
        let without = damage_of(V113, character, card, false, setup);
        let with = damage_of(V113, character, card, true, setup);
        // Every hit of one command deals the same damage here (no Strength,
        // no block, an enemy that cannot die), so the extra hit is exactly
        // one hit's worth.
        let per_hit = without / base_hits;
        if without == 0 || without % base_hits != 0 || with - without != per_hit {
            failures.push(format!(
                "{card:?}: {base_hits} hits dealt {without} without Thumbscrew, {with} with \
                 (want {})",
                without + per_hit
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The relic is v0.113.0-only: owning one on an older build (a hand-built
/// scenario) changes nothing.
#[test]
fn thumbscrew_is_inert_before_v0113() {
    for &(card, character, _, setup) in &MULTI_HIT[..4] {
        let without = damage_of(V111, character, card, false, setup);
        let with = damage_of(V111, character, card, true, setup);
        assert_eq!(with, without, "{card:?} on v0.111.0");
    }
}

/// `hitCount <= 1` returns unchanged: a single-hit attack, a multi-hit card
/// whose condition left it at one hit, and an X-cost card at X = 1 / X = 0.
#[test]
fn thumbscrew_leaves_single_hit_attacks_alone() {
    fn x_one(state: &mut CombatState) {
        state.player.energy = 1;
    }
    fn x_zero(state: &mut CombatState) {
        state.player.energy = 0;
    }
    let cases: &[(CardId, CharacterId, fn(&mut CombatState))] = &[
        (CardId::StrikeIronclad, CharacterId::Ironclad, no_setup),
        (CardId::Bash, CharacterId::Ironclad, no_setup),
        // Spite with no HP lost this turn: WithHitCount(1).
        (CardId::Spite, CharacterId::Ironclad, no_setup),
        // Dismantle on a non-Vulnerable target: WithHitCount(1).
        (CardId::Dismantle, CharacterId::Ironclad, no_setup),
        (CardId::Whirlwind, CharacterId::Ironclad, x_one),
        (CardId::Whirlwind, CharacterId::Ironclad, x_zero),
        // Finisher with no attacks played: WithHitCount(0).
        (CardId::Finisher, CharacterId::Silent, no_setup),
    ];
    for &(card, character, setup) in cases {
        let without = damage_of(V113, character, card, false, setup);
        let with = damage_of(V113, character, card, true, setup);
        assert_eq!(with, without, "{card:?}");
    }
}

/// A random-target multi-hit card draws one more `CombatTargets` value for
/// the extra hit (`AttackCommand` picks a target per hit).
#[test]
fn thumbscrew_extra_random_hit_draws_one_more_target() {
    let run = |thumbscrew: bool| {
        let mut state = fresh(V113, CharacterId::Ironclad, 2);
        if thumbscrew {
            give_thumbscrew(&mut state);
        }
        let before = state.run_rng.combat_targets.counter();
        let i = push_hand(&mut state, CardId::SwordBoomerang, false);
        let _ = sts2_core::combat_loop::play_card(&mut state, i, None);
        (
            state.run_rng.combat_targets.counter() - before,
            total_enemy_hp_lost(&state),
        )
    };
    let (draws_without, hp_without) = run(false);
    let (draws_with, hp_with) = run(true);
    assert_eq!(draws_without, 3);
    assert_eq!(draws_with, 4);
    assert_eq!(hp_with - hp_without, hp_without / 3);
}

/// Monster attacks are not card-sourced: the hook leaves them alone. A
/// Skill card's attack (none multi-hit in v0.113) would be skipped by the
/// same `Type: Attack` check.
#[test]
fn thumbscrew_ignores_non_card_attacks() {
    let state = {
        let mut s = fresh(V113, CharacterId::Ironclad, 1);
        give_thumbscrew(&mut s);
        s
    };
    assert_eq!(
        sts2_core::card_effects::modify_attack_hit_count(&state, None, 3),
        3
    );
    assert_eq!(
        sts2_core::card_effects::modify_attack_hit_count(
            &state,
            Some(sts2_core::cards::CardType::Skill),
            3
        ),
        3
    );
    assert_eq!(
        sts2_core::card_effects::modify_attack_hit_count(
            &state,
            Some(sts2_core::cards::CardType::Attack),
            3
        ),
        4
    );
}
