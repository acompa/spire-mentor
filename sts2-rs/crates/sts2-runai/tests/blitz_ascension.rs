//! The run AI's ascension inputs in a Blitz run (GH #1347).
//!
//! A Blitz run's `ascension_level` is the Blitz level 0..=5, a SET of
//! standard levels (`AscensionHelper.GetLevelForBlitz`,
//! `Helpers/AscensionHelper.cs:66-78`, v0.113.0), not a rung of the standard
//! ladder the nets and features were fit on. They are fed the standard level
//! with the same fight scaling (`RunState::combat_ascension`: 0, 8 for
//! ToughEnemies, 9 with DeadlyEnemies). Standard and Dread runs keep the
//! plain number.

use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character,
    new_run_manager_for_character_with_ascension,
};
use sts2_core::run_manager::RunManager;
use sts2_core::{CharacterId, GameMode, GameVersion};
use sts2_runai::ppo::policy::{build_obs, classify_surface, phase_str};
use sts2_runai::view::VisibleRunView;

const V113: GameVersion = GameVersion::V0_113_0;

/// A v0.113.0 run of `mode` at `level`, standing at Neow (the builder chain
/// of `sts2-content/tests/v0113_blitz.rs`).
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

/// The PPO observation's ascension at the run's current decision.
fn obs_ascension(rm: &RunManager) -> i64 {
    let phase = rm.phase();
    let surface = classify_surface(phase, &rm.get_available_actions());
    build_obs(rm, phase_str(phase), surface).ascension
}

/// Blitz level -> the standard ascension with the same fights.
const BLITZ_FIGHT_ASCENSION: [u8; 6] = [0, 0, 0, 8, 9, 9];

#[test]
fn ppo_obs_ascension_is_the_fight_ascension_in_blitz() {
    for (level, want) in BLITZ_FIGHT_ASCENSION.into_iter().enumerate() {
        let rm = run(GameMode::Blitz, level as u8);
        assert_eq!(obs_ascension(&rm), i64::from(want), "Blitz {level}");
    }
}

#[test]
fn run_view_ascension_is_the_fight_ascension_in_blitz() {
    for (level, want) in BLITZ_FIGHT_ASCENSION.into_iter().enumerate() {
        let rm = run(GameMode::Blitz, level as u8);
        let view = VisibleRunView::from_run_manager(&rm);
        assert_eq!(view.ascension, want, "Blitz {level}");
    }
}

#[test]
fn standard_and_dread_keep_the_plain_ascension() {
    for level in 0u8..=10 {
        let rm = run(GameMode::Standard, level);
        assert_eq!(obs_ascension(&rm), i64::from(level));
        assert_eq!(VisibleRunView::from_run_manager(&rm).ascension, level);
    }
    let rm = run(GameMode::Dread, 10);
    assert_eq!(obs_ascension(&rm), 10);
    assert_eq!(VisibleRunView::from_run_manager(&rm).ascension, 10);
}
