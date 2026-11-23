//! The e2e observation's ascension in a Blitz run (GH #1347).
//!
//! A Blitz run's `ascension_level` is the Blitz level 0..=5, a SET of
//! standard levels (`AscensionHelper.GetLevelForBlitz`,
//! `Helpers/AscensionHelper.cs:66-78`, v0.113.0). The tokenizer's `R_ASC`
//! feature and `bosses_left` read a standard-ladder number, so the
//! observation carries the standard level with the same fight scaling
//! (`RunState::combat_ascension`). Standard and Dread keep the plain number.

use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character,
    new_run_manager_for_character_with_ascension,
};
use sts2_core::run_manager::RunManager;
use sts2_core::{CharacterId, GameMode, GameVersion};
use sts2_e2e::env::{obs_ascension, observe};

const V113: GameVersion = GameVersion::V0_113_0;

fn run(mode: GameMode, level: u8) -> RunManager {
    let master = V113.run_seed_hash(&sts2_core::canonicalize_seed("BLITZAI"));
    let rm = if level > 0 {
        new_run_manager_for_character_with_ascension(master as i32, CharacterId::Ironclad, level)
    } else {
        new_run_manager_for_character(master as i32, CharacterId::Ironclad)
    };
    let mut rm = rm
        .with_game_version(V113)
        .with_game_mode(mode)
        .with_run_seed(master)
        .with_neow_start();
    rm.run_state.set_agent_decisions();
    let mut rm = rm.with_rolled_act1();
    initialize_run_setup(&mut rm);
    rm
}

/// The `ascension` key of the run observation.
fn observed(rm: &mut RunManager) -> i64 {
    let asc = obs_ascension(rm);
    observe(rm, "event", None, 0, asc)["ascension"]
        .as_i64()
        .expect("an int")
}

#[test]
fn observation_ascension_is_the_fight_ascension_in_blitz() {
    for (level, want) in [0i64, 0, 0, 8, 9, 9].into_iter().enumerate() {
        let mut rm = run(GameMode::Blitz, level as u8);
        assert_eq!(obs_ascension(&rm), want, "Blitz {level}");
        assert_eq!(observed(&mut rm), want, "Blitz {level}");
    }
}

#[test]
fn standard_and_dread_keep_the_plain_ascension() {
    for level in 0u8..=10 {
        let mut rm = run(GameMode::Standard, level);
        assert_eq!(observed(&mut rm), i64::from(level));
    }
    let mut rm = run(GameMode::Dread, 10);
    assert_eq!(observed(&mut rm), 10);
}
