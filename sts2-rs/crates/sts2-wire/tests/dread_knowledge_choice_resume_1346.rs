//! GH #1346: a board rebuilt from the wire in the middle of
//! `KnowledgeDemonDominion`'s turn-1 Sloth-or-Amnesia choice
//! (`KnowledgeDemonDominion.cs:26-41`) resumes like the live one: the screen
//! is open, and answering it runs the hand draw it parked.
//!
//! The wire carries no open choice and no parked continuation for any other
//! select. This one is engine-derived (two fixed options, no skip), so
//! `$.dread_knowledge_choice` names all of it: the parked draw's count and
//! whether the pause already fired `AfterSideTurnStart`.

use smallvec::smallvec;
use sts2_content::monster_dispatch::MonsterSpawnArgs;
use sts2_content::run_bridge::enter_combat_from_roster;
use sts2_content::run_callbacks::{initialize_run_setup, new_run_manager_for_character};
use sts2_core::cards::CardId;
use sts2_core::runstate::RoomType;
use sts2_core::{
    apply, Action, CharacterId, ChoiceKind, CombatPhase, CombatState, Dominion, GameMode,
    GameVersion, MonsterKind, PowerKind, StepResult,
};
use sts2_wire::emit::combat_state_to_value;
use sts2_wire::state_from_json;

const KEY: &str = "dread_knowledge_choice";

/// The first hallway fight of a v0.113.0 run in `mode` whose act boss is the
/// Knowledge Demon, as the run layer enters it. In Dread that parks on the
/// dominion's choice.
fn hallway_fight(mode: GameMode) -> CombatState {
    let mut rm = new_run_manager_for_character(7, CharacterId::Ironclad)
        .with_game_version(GameVersion::V0_113_0);
    rm.run_state.ascension_level = 10;
    rm.run_state.game_mode = mode;
    initialize_run_setup(&mut rm);
    let rooms = &mut rm.run_state.generated_rooms[0];
    rooms.boss_id = Dominion::KnowledgeDemon.boss_id();
    rooms.second_boss_id = None;
    enter_combat_from_roster(
        &rm.run_state,
        &[(MonsterKind::LagavulinMatriarch, MonsterSpawnArgs::None, 200)],
        RoomType::Monster,
    )
}

fn at_knowledge_choice() -> CombatState {
    let state = hallway_fight(GameMode::Dread);
    assert_eq!(state.dominion, Some(Dominion::KnowledgeDemon));
    let pc = state.pending_choice.as_ref().expect("the choice is open");
    assert_eq!(pc.kind, ChoiceKind::MonsterCurseSelect);
    assert!(state.player.hand.is_empty(), "the hand draw waits");
    assert!(state.pending_relic_hand_draw_resume.is_some());
    state
}

fn round_trip(state: &CombatState) -> CombatState {
    let v = combat_state_to_value(state).expect("emits");
    state_from_json(&v).expect("re-parses")
}

fn choice_shape(state: &CombatState) -> Option<(ChoiceKind, Vec<CardId>, u8, u8, bool)> {
    state.pending_choice.as_ref().map(|pc| {
        (
            pc.kind,
            pc.options.iter().map(|o| o.card_id).collect(),
            pc.min_select,
            pc.max_select,
            pc.can_skip,
        )
    })
}

fn player_powers(state: &CombatState) -> Vec<(PowerKind, i32)> {
    state
        .player_power_uids()
        .iter()
        .filter_map(|p| state.powers.get_by_id(p.0))
        .map(|p| (p.kind, p.amount))
        .collect()
}

fn hand(state: &CombatState) -> Vec<CardId> {
    state
        .player
        .hand
        .iter()
        .map(|u| state.cards[u.0.get() as usize - 1].card_id)
        .collect()
}

/// What the player sees of a board, for comparing a rebuilt one with the
/// live one it was taken from.
fn view(state: &CombatState) -> impl PartialEq + std::fmt::Debug {
    (
        state.phase,
        state.round,
        hand(state),
        state.player.draw.len(),
        state.player.discard.len(),
        state.player.energy,
        state.player.current_hp,
        player_powers(state),
        choice_shape(state),
    )
}

#[test]
fn the_parked_choice_rides_the_wire() {
    let live = at_knowledge_choice();
    let v = combat_state_to_value(&live).expect("emits");
    assert!(v.get(KEY).is_some(), "the parked choice is on the wire");
    let back = state_from_json(&v).expect("re-parses");
    assert_eq!(choice_shape(&back), choice_shape(&live));
    assert_eq!(
        choice_shape(&back),
        Some((
            ChoiceKind::MonsterCurseSelect,
            vec![CardId::SlothStatus, CardId::Amnesia],
            1,
            1,
            false
        ))
    );
    assert_eq!(
        back.pending_relic_hand_draw_resume,
        live.pending_relic_hand_draw_resume
    );
    assert!(back.combat_log.dread_knowledge_choice_opened);
    assert!(back.player.hand.is_empty());
    // And a second trip carries the same key.
    let again = combat_state_to_value(&back).expect("emits");
    assert_eq!(again[KEY], v[KEY]);
    assert_eq!(
        v[KEY],
        serde_json::json!({"draw_count": 5, "side_fired": true})
    );
}

/// Answering the choice on the rebuilt board runs the parked hand draw, as on
/// the live one: Sloth keeps the full hand, Amnesia draws two fewer and
/// costs one energy (`AmnesiaPower.cs:25-49`). The turn after matches too.
#[test]
fn a_rebuilt_board_resumes_the_hand_draw() {
    for (pick, power, hand_len, energy) in [
        (0_u8, (PowerKind::Sloth, 3), 5, 3),
        (1_u8, (PowerKind::Amnesia, 1), 3, 2),
    ] {
        let mut live = at_knowledge_choice();
        let mut back = round_trip(&live);
        for state in [&mut live, &mut back] {
            let r = apply(
                state,
                Action::ResolveChoice {
                    selection: smallvec![pick],
                },
            );
            assert_eq!(r, StepResult::Advanced, "pick {pick}");
        }
        assert_eq!(back.phase, CombatPhase::PlayerPlayPhase, "pick {pick}");
        assert!(back.pending_choice.is_none(), "pick {pick}");
        assert_eq!(player_powers(&back), [power], "pick {pick}");
        assert_eq!(back.player.hand.len(), hand_len, "pick {pick}");
        assert_eq!(back.player.energy, energy, "pick {pick}");
        assert_eq!(view(&back), view(&live), "pick {pick}");
        assert!(back.pending_relic_hand_draw_resume.is_none());
        // Past the choice nothing is parked, so nothing rides the wire.
        let after = combat_state_to_value(&back).expect("emits");
        assert!(after.get(KEY).is_none(), "pick {pick}");

        let _ = apply(&mut live, Action::EndTurn);
        let _ = apply(&mut back, Action::EndTurn);
        assert_eq!(back.round, 2, "pick {pick}");
        assert!(back.pending_choice.is_none(), "turn 1 only");
        assert_eq!(view(&back), view(&live), "pick {pick}, turn 2");
    }
}

/// Only a board parked on the dominion's choice carries the key: a standard
/// fight of the same seed and a Dread fight under another dominion do not.
#[test]
fn no_other_board_carries_it() {
    let standard = hallway_fight(GameMode::Standard);
    assert!(standard.pending_choice.is_none());
    let v = combat_state_to_value(&standard).expect("emits");
    assert!(v.get(KEY).is_none());
    assert!(v.get("game_mode").is_none() && v.get("dominion").is_none());

    let mut other = at_knowledge_choice();
    let _ = apply(
        &mut other,
        Action::ResolveChoice {
            selection: smallvec![0],
        },
    );
    let v = combat_state_to_value(&other).expect("emits");
    assert!(v.get(KEY).is_none());
}

#[test]
fn a_malformed_or_misplaced_key_is_refused() {
    let good = combat_state_to_value(&at_knowledge_choice()).expect("emits");
    assert!(state_from_json(&good).is_ok());
    for bad in [
        serde_json::json!(true),
        serde_json::json!({}),
        serde_json::json!({"draw_count": "five", "side_fired": false}),
        serde_json::json!({"draw_count": -1, "side_fired": false}),
        serde_json::json!({"draw_count": 5, "side_fired": "no"}),
    ] {
        let mut v = good.clone();
        v[KEY] = bad.clone();
        assert!(state_from_json(&v).is_err(), "{bad}");
    }
    // The choice belongs to the Knowledge Demon dominion alone.
    for dominion in [None, Some("QUEEN_DOMINION")] {
        let mut v = good.clone();
        match dominion {
            Some(name) => v["dominion"] = serde_json::json!(name),
            None => {
                let _ = v.as_object_mut().expect("object").remove("dominion");
            }
        }
        assert!(state_from_json(&v).is_err(), "{dominion:?}");
    }
}
