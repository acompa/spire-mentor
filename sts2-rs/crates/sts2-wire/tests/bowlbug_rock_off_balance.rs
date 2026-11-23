//! A Bowlbug Rock telegraphing DIZZY is off balance (C# `IsOffBalance`,
//! set by a fully blocked headbutt, cleared by `DizzyMove`). The engine keeps
//! the flag on the AI counter; a rebuilt board must carry it as a native one
//! does, or the forecast after the dizzy turn differs.

use sts2_core::MonsterKind;
use sts2_wire::emit::combat::combat_state_to_value;

#[test]
fn a_dizzy_rock_rebuilds_off_balance() {
    let mut state = sts2_wire::scenario::build_scenario_state_for_version(
        sts2_core::GameVersion::V0_111_0,
        7,
        "Ironclad",
        &vec!["STRIKE_IRONCLAD".to_owned(); 10],
        80,
        80,
        &[],
        &[],
        "BOWLBUGS_WEAK",
        10,
        None,
        None,
    )
    .expect("scenario");
    let slot = state
        .enemy_uids
        .iter()
        .position(|u| {
            state
                .creatures
                .iter()
                .any(|c| c.id == u.0 && c.species == Some(MonsterKind::BowlbugRock))
        })
        .expect("a Bowlbug Rock");
    let uid = state.enemy_uids[slot].0;
    {
        let rock = state
            .creatures
            .iter_mut()
            .find(|c| c.id == uid)
            .expect("rock");
        let ai = rock.ai.as_mut().expect("ai");
        ai.force_current_state("DIZZY");
        ai.set_counter(1);
        rock.intent = Some(sts2_core::Intent::Stun);
    }
    let mut wire = combat_state_to_value(&state).expect("serializes");
    // The referee hides the move machine's state; only the telegraph shows.
    wire["encounter"][slot]
        .as_object_mut()
        .expect("enemy")
        .remove("ai");
    let back = sts2_wire::state_from_json(&wire).expect("parses");
    let rebuilt = back
        .creatures
        .iter()
        .find(|c| c.id == back.enemy_uids[slot].0)
        .and_then(|c| c.ai.as_ref())
        .expect("rebuilt rock AI");
    assert_eq!(rebuilt.current_state_id(), "DIZZY");
    assert_eq!(rebuilt.counter(), 1, "off balance while dizzy");
}
