//! v0.113.0 Wuppy (GH #1496): the relic (`Relics/Wuppy.cs`), its pet
//! (`Monsters/Wuppy.cs`), `WuppysLoyaltyPower` and `RestoreRestSiteOption`.
//!
//! - `BeforeCombatStart` summons the pet with `WuppysLoyaltyPower(1)` at the
//!   relic's saved `MaxHp` / `CurrentHp`.
//! - `ModifyPreBlockDamageTarget`: a powered attack on the owner goes to a
//!   living Wuppy BEFORE block; only the overkill reaches the player's block.
//! - `AfterCurrentHpChanged`: the relic's `CurrentHp` (its counter) follows the
//!   pet, so the next combat starts at the HP this one ended on.
//! - `TryModifyRestSiteOptions`: one Restore option, enabled while
//!   `CurrentHp < MaxHp`; picking it sets `CurrentHp = MaxHp`.

use sts2_content::run_bridge::{enter_combat, exit_combat};
use sts2_core::combat_loop::apply_damage_to_player;
use sts2_core::encounters::{act1_normal_pool, select_first};
use sts2_core::hooks::{CreatureRef, ValueProps};
use sts2_core::runstate::{RestSiteActionOutcome, RestSiteOption, RunState};
use sts2_core::{
    CombatState, CreatureUid, GameVersion, MonsterKind, PowerKind, RelicId, RelicKind,
};

fn install() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
}

/// A v0.113.0 run holding Wuppy at `current_hp` of `max_hp`.
fn run_with_wuppy(version: GameVersion, max_hp: i32, current_hp: i32) -> RunState {
    let mut rs = RunState {
        current_hp: 72,
        max_hp: 80,
        game_version: version,
        ..RunState::default()
    };
    rs.relics.push(RelicId::Wuppy);
    rs.wuppy_max_hp = max_hp;
    rs.relic_counters.insert(RelicId::Wuppy, current_hp);
    rs
}

fn wuppy(state: &CombatState) -> Option<&sts2_core::Creature> {
    state
        .creatures
        .iter()
        .find(|c| c.species == Some(MonsterKind::Wuppy))
}

fn has_loyalty(state: &CombatState) -> bool {
    wuppy(state).is_some_and(|c| {
        c.powers.iter().any(|u| {
            state
                .powers
                .get_by_id(u.0)
                .is_some_and(|p| p.kind == PowerKind::WuppysLoyalty && p.amount == 1)
        })
    })
}

fn relic_counter(state: &CombatState) -> i32 {
    state
        .relics
        .iter()
        .find(|r| r.kind == RelicKind::Wuppy)
        .expect("Wuppy relic in combat")
        .counter
}

fn enemy(state: &CombatState) -> CreatureRef {
    CreatureRef::Enemy(state.enemy_uids[0])
}

fn hit(state: &mut CombatState, amount: i32, props: ValueProps) {
    let dealer = enemy(state);
    let _ = apply_damage_to_player(state, dealer, amount, props, None);
}

#[test]
#[serial_test::serial(global_registry)]
fn combat_start_summons_the_pet_at_the_relics_hp() {
    install();
    let rs = run_with_wuppy(GameVersion::V0_113_0, 30, 20);
    let state = enter_combat(&rs, select_first(act1_normal_pool()));
    let pet = wuppy(&state).expect("Wuppy summoned");
    assert_eq!((pet.current_hp, pet.max_hp), (20, 30), "CurrentHp of MaxHp");
    assert!(has_loyalty(&state), "WuppysLoyaltyPower(1) on the pet");
    assert!(
        !state.enemy_uids.contains(&CreatureUid(pet.id)),
        "the pet is player-side"
    );
    assert_eq!(state.player.pet_uid, None, "not the Osty slot");
    assert_eq!(relic_counter(&state), 20, "counter = CurrentHp");
}

#[test]
#[serial_test::serial(global_registry)]
fn a_strong_wuppy_above_the_default_keeps_its_max() {
    install();
    // MaxHp 60 at full HP: WuppyInTrouble's HelpImmediately.
    let rs = run_with_wuppy(GameVersion::V0_113_0, 60, 60);
    let state = enter_combat(&rs, select_first(act1_normal_pool()));
    let pet = wuppy(&state).expect("Wuppy summoned");
    assert_eq!((pet.current_hp, pet.max_hp), (60, 60));
}

#[test]
#[serial_test::serial(global_registry)]
fn a_relic_at_zero_hp_summons_a_corpse_that_keeps_its_power() {
    install();
    let rs = run_with_wuppy(GameVersion::V0_113_0, 30, 0);
    let mut state = enter_combat(&rs, select_first(act1_normal_pool()));
    let pet = wuppy(&state).expect("a dead Wuppy stays in the fight");
    assert_eq!((pet.current_hp, pet.max_hp), (0, 30));
    assert!(has_loyalty(&state), "the power survives its owner's death");
    assert!(!state.is_over, "a dead pet does not end the fight");
    // A dead Wuppy takes nothing.
    state.player.block = 0;
    let hp = state.player.current_hp;
    hit(&mut state, 6, ValueProps::MOVE);
    assert_eq!(state.player.current_hp, hp - 6);
}

#[test]
#[serial_test::serial(global_registry)]
fn a_powered_attack_on_the_player_hits_living_wuppy_before_block() {
    install();
    let rs = run_with_wuppy(GameVersion::V0_113_0, 30, 20);
    let mut state = enter_combat(&rs, select_first(act1_normal_pool()));
    state.player.block = 10;
    let hp = state.player.current_hp;
    hit(&mut state, 12, ValueProps::MOVE);
    assert_eq!(
        wuppy(&state).expect("pet").current_hp,
        8,
        "Wuppy took all 12"
    );
    assert_eq!(
        state.player.block, 10,
        "block untouched: the redirect is pre-block"
    );
    assert_eq!(state.player.current_hp, hp);
    assert_eq!(
        relic_counter(&state),
        8,
        "AfterCurrentHpChanged synced the relic"
    );

    // Overkill: 15 into 8 HP leaves 7 for the player's block.
    hit(&mut state, 15, ValueProps::MOVE);
    let pet = wuppy(&state).expect("the dead pet stays in the fight");
    assert_eq!(pet.current_hp, 0);
    assert!(has_loyalty(&state));
    assert_eq!(state.player.block, 3, "the overkill 7 hit block");
    assert_eq!(state.player.current_hp, hp);
    assert_eq!(relic_counter(&state), 0);
    assert!(!state.is_over, "Wuppy's death does not end the fight");

    // Dead Wuppy: no redirect.
    hit(&mut state, 5, ValueProps::MOVE);
    assert_eq!(state.player.block, 0);
    assert_eq!(state.player.current_hp, hp - 2);
}

#[test]
#[serial_test::serial(global_registry)]
fn unpowered_damage_is_not_redirected() {
    install();
    let rs = run_with_wuppy(GameVersion::V0_113_0, 30, 20);
    let mut state = enter_combat(&rs, select_first(act1_normal_pool()));
    state.player.block = 0;
    let hp = state.player.current_hp;
    hit(&mut state, 4, ValueProps::MOVE.union(ValueProps::UNPOWERED));
    hit(&mut state, 3, ValueProps::UNPOWERED);
    assert_eq!(wuppy(&state).expect("pet").current_hp, 20);
    assert_eq!(state.player.current_hp, hp - 7);
}

#[test]
#[serial_test::serial(global_registry)]
fn the_pets_hp_carries_into_the_next_combat() {
    install();
    let mut rs = run_with_wuppy(GameVersion::V0_113_0, 30, 20);
    let mut state = enter_combat(&rs, select_first(act1_normal_pool()));
    state.player.block = 0;
    hit(&mut state, 9, ValueProps::MOVE);
    exit_combat(&mut rs, &state);
    assert_eq!(rs.relic_counters.get(&RelicId::Wuppy), Some(&11));
    assert_eq!(rs.wuppy_max_hp, 30, "MaxHp unchanged");
    let next = enter_combat(&rs, select_first(act1_normal_pool()));
    let pet = wuppy(&next).expect("summoned again");
    assert_eq!((pet.current_hp, pet.max_hp), (11, 30));
}

fn restore_options(rs: &RunState) -> Vec<RestSiteOption> {
    rs.rest_site_options()
        .into_iter()
        .filter(|o| matches!(o, RestSiteOption::Restore { .. }))
        .collect()
}

#[test]
#[serial_test::serial(global_registry)]
fn restore_is_offered_once_enabled_below_max_and_heals_the_relic() {
    install();
    let mut rs = run_with_wuppy(GameVersion::V0_113_0, 30, 11);
    assert_eq!(
        restore_options(&rs),
        [RestSiteOption::Restore { enabled: true }],
        "one Restore, enabled below MaxHp"
    );
    let ids: Vec<&str> = rs
        .rest_site_options()
        .iter()
        .map(RestSiteOption::option_id)
        .collect();
    assert_eq!(ids, ["HEAL", "SMITH", "RESTORE"]);
    let outcome = rs.do_rest_site_action(RestSiteOption::Restore { enabled: true });
    assert_eq!(outcome, RestSiteActionOutcome::Restored);
    assert_eq!(rs.relic_counters.get(&RelicId::Wuppy), Some(&30));
    // At full HP it is still offered, disabled.
    assert_eq!(
        restore_options(&rs),
        [RestSiteOption::Restore { enabled: false }]
    );
    assert_eq!(
        rs.do_rest_site_action(RestSiteOption::Restore { enabled: false }),
        RestSiteActionOutcome::Disabled
    );
}

#[test]
#[serial_test::serial(global_registry)]
fn a_dead_wuppy_can_be_restored() {
    install();
    let mut rs = run_with_wuppy(GameVersion::V0_113_0, 60, 0);
    assert_eq!(
        restore_options(&rs),
        [RestSiteOption::Restore { enabled: true }]
    );
    let _ = rs.do_rest_site_action(RestSiteOption::Restore { enabled: true });
    let state = enter_combat(&rs, select_first(act1_normal_pool()));
    assert_eq!(wuppy(&state).expect("pet").current_hp, 60);
}

#[test]
#[serial_test::serial(global_registry)]
fn without_wuppy_or_before_v0113_nothing_changes() {
    install();
    // No relic: no pet, no Restore.
    let plain = RunState {
        game_version: GameVersion::V0_113_0,
        ..RunState::default()
    };
    assert!(restore_options(&plain).is_empty());
    let state = enter_combat(&plain, select_first(act1_normal_pool()));
    assert!(wuppy(&state).is_none());

    // A v0.111.0 run (no Wuppy in that build): neither surface fires.
    let old = run_with_wuppy(GameVersion::V0_111_0, 30, 20);
    assert!(restore_options(&old).is_empty());
    let mut state = enter_combat(&old, select_first(act1_normal_pool()));
    assert!(wuppy(&state).is_none());
    state.player.block = 0;
    let hp = state.player.current_hp;
    hit(&mut state, 6, ValueProps::MOVE);
    assert_eq!(state.player.current_hp, hp - 6);
}

#[test]
#[serial_test::serial(global_registry)]
fn obtaining_wuppy_without_hp_fills_in_the_defaults() {
    install();
    // C# `Wuppy.AfterObtained`: MaxHp 0 -> 60, unset CurrentHp -> MaxHp.
    let mut rs = RunState {
        game_version: GameVersion::V0_113_0,
        ..RunState::default()
    };
    rs.obtain_relic(RelicId::Wuppy);
    assert_eq!(rs.wuppy_max_hp, 60);
    assert_eq!(rs.relic_counters.get(&RelicId::Wuppy), Some(&60));
}
