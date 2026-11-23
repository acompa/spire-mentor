//! GH #1346: the final boss's deferred win needs nothing on the combat wire.
//!
//! v0.113.0 offers the last boss room's `ExtraRewards` before `WinRun`
//! (`RewardsSet.cs:96-103`), so a Forbidden Grimoire's card removal is picked
//! first and the run manager holds the win back until then
//! (`RunManager::pending_run_win`). That flag is run-level and lives only
//! between the end of the fight and the removal pick, when there is no combat
//! to put on a wire. What a combat wire has to keep is its cause: the
//! player's `ForbiddenGrimoire` power, which the combat exit reads
//! (`run_bridge::exit_combat`). A final-boss fight rebuilt from the wire and
//! then won defers the win like the live one.

use sts2_content::monster_dispatch::MonsterSpawnArgs;
use sts2_content::run_bridge::enter_combat_from_roster;
use sts2_content::run_callbacks::{initialize_run_setup, new_run_manager_for_character};
use sts2_core::run_manager::{Phase, RunAction, RunManager};
use sts2_core::runstate::{Room, RoomType};
use sts2_core::{CharacterId, CombatState, GameVersion, MonsterKind, PowerKind};
use sts2_wire::emit::combat_state_to_value;
use sts2_wire::state_from_json;

/// A v0.113.0 run standing in its last boss room, and the fight entered
/// there with one Forbidden Grimoire already played.
fn final_boss_fight() -> (RunManager, CombatState) {
    let mut rm = new_run_manager_for_character(7, CharacterId::Ironclad)
        .with_game_version(GameVersion::V0_113_0);
    initialize_run_setup(&mut rm);
    rm.run_state.interactive_card_removal = true;
    rm.run_state.act_index = 3;
    rm.run_state.current_room = Some(Room::new(RoomType::Boss));
    let mut fight = enter_combat_from_roster(
        &rm.run_state,
        &[(MonsterKind::Nibbit, MonsterSpawnArgs::None, 1)],
        RoomType::Boss,
    );
    let player = fight.player.creature_uid.expect("player creature");
    sts2_core::card_effects::apply_power_to_creature(
        &mut fight,
        player,
        PowerKind::ForbiddenGrimoire,
        1,
    );
    (rm, fight)
}

/// Play the fight out with the first legal action each step (the enemy has
/// 1 HP, so the first attack played wins it).
fn win_the_fight(rm: &mut RunManager) {
    for _ in 0..200 {
        if rm.phase() != Phase::Combat {
            return;
        }
        let action = rm
            .get_available_actions()
            .into_iter()
            .next()
            .expect("a combat action");
        let _ = rm.take_action(action);
    }
    panic!("the fight did not end");
}

#[test]
fn a_rebuilt_final_boss_fight_defers_the_win_until_the_removal() {
    for rebuilt in [false, true] {
        let (mut rm, fight) = final_boss_fight();
        let fight = if rebuilt {
            let v = combat_state_to_value(&fight).expect("emits");
            let back = state_from_json(&v).expect("re-parses");
            assert_eq!(back.player_power_amount(PowerKind::ForbiddenGrimoire), 1);
            back
        } else {
            fight
        };
        let deck_before = rm.run_state.master_deck.len();
        rm.debug_enter_combat(fight, RoomType::Boss);
        win_the_fight(&mut rm);

        assert!(
            !rm.run_state.is_over,
            "rebuilt={rebuilt}: the removal is still to pick"
        );
        let pick = rm
            .get_available_actions()
            .into_iter()
            .find(|a| matches!(a, RunAction::RemoveDeckCard { .. }))
            .expect("a deck removal pick");
        let _ = rm.take_action(pick);
        assert_eq!(rm.run_state.master_deck.len(), deck_before - 1);
        assert_eq!(rm.phase(), Phase::RunOver, "rebuilt={rebuilt}");
        assert!(rm.run_state.player_won, "rebuilt={rebuilt}");
    }
}
