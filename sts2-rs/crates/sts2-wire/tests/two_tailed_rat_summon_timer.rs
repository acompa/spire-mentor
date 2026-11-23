//! A Two-Tailed Rat may CALL_FOR_BACKUP once it has performed two moves (C#
//! `TwoTailedRat.TurnsUntilSummonable`, 2 at spawn, decremented by SCRATCH /
//! DISEASE_BITE / SCREECH). The engine keeps that timer on the AI counter,
//! which the game's producers do not send; a rebuilt board must recover it
//! from the move log, or the rat never forecasts its summon.

mod common;

use common::{end_turn, fight};
use sts2_core::{CombatState, MonsterKind, StepResult};
use sts2_wire::emit::combat::{combat_state_to_value, enemy_intent_forecast};

fn rat_counters(state: &CombatState) -> Vec<i32> {
    state
        .enemy_uids
        .iter()
        .filter_map(|u| state.creatures.iter().find(|c| c.id == u.0))
        .filter(|c| c.species == Some(MonsterKind::TwoTailedRat))
        .map(|c| c.ai.as_ref().expect("rat AI").counter())
        .collect()
}

#[test]
fn a_rebuilt_rat_two_moves_in_can_call_for_backup() {
    let mut state = fight(
        "Ironclad",
        &["DEFEND_IRONCLAD"; 10],
        80,
        &[],
        &[],
        "TWO_TAILED_RATS_NORMAL",
        10,
    );
    for _ in 0..2 {
        assert_eq!(end_turn(&mut state), StepResult::Advanced);
    }
    let native = rat_counters(&state);
    assert_eq!(
        native,
        vec![0, 0, 0],
        "two moves in, every seeded rat may summon"
    );

    let mut wire = combat_state_to_value(&state).expect("serializes");
    // The game sends the move log and the performed flags, not the counter.
    for e in wire["encounter"].as_array_mut().expect("encounter") {
        if let Some(ai) = e.get_mut("ai").and_then(|a| a.as_object_mut()) {
            ai.remove("counter");
        }
    }
    let back = sts2_wire::state_from_json(&wire).expect("parses");
    assert_eq!(rat_counters(&back), native);
    assert_eq!(
        enemy_intent_forecast(&back, 3).expect("forecast"),
        enemy_intent_forecast(&state, 3).expect("forecast"),
    );
}
