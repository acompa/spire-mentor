//! GH #1708: the board carries an attack's base damage (C#
//! `AttackIntent.DamageCalc()`, a public move-table number). A rebuild takes
//! it as the intent's base instead of inverting the shown number, which
//! floors at 0 under heavy Strength loss.

use sts2_core::Intent;
use sts2_wire::emit::combat::combat_state_to_value;

#[test]
fn a_shown_zero_attack_keeps_the_boards_base() {
    let state = sts2_wire::scenario::build_scenario_state_for_version(
        sts2_core::GameVersion::V0_111_0,
        7,
        "Ironclad",
        &vec!["STRIKE_IRONCLAD".to_owned(); 10],
        80,
        80,
        &[],
        &[],
        "AXEBOTS_NORMAL",
        10,
        None,
        None,
    )
    .expect("scenario");
    let mut wire = combat_state_to_value(&state).expect("serializes");
    // As the host's board says it: shown 0, base 12.
    wire.as_object_mut().expect("obj").remove("intent_amounts");
    let enemy = &mut wire["encounter"][0];
    enemy["intents"] = serde_json::json!([{"kind": "Attack", "amount": 0, "base_amount": 12}]);
    let back = sts2_wire::state_from_json(&wire).expect("parses");
    let first = back.enemy_uids[0];
    let intent = back
        .creatures
        .iter()
        .find(|c| c.id == first.0)
        .and_then(|c| c.intent);
    assert_eq!(intent, Some(Intent::Attack { damage: 12 }));
}
