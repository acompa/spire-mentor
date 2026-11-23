//! A C# `CardDebuffIntent` (Queen's opening PUPPET_STRINGS) reaches the
//! board as kind `CardDebuff`. The engine declares those moves `Debuff`, so a
//! rebuilt board must read the same telegraph a native one shows.

use sts2_wire::emit::combat::combat_state_to_value;

#[test]
fn a_card_debuff_telegraph_rebuilds_as_the_native_debuff() {
    let state = sts2_wire::scenario::build_scenario_state_for_version(
        sts2_core::GameVersion::V0_111_0,
        7,
        "Ironclad",
        &vec!["STRIKE_IRONCLAD".to_owned(); 10],
        80,
        80,
        &[],
        &[],
        "QUEEN_BOSS",
        10,
        None,
        None,
    )
    .expect("scenario");
    let queen = state
        .creatures
        .iter()
        .find(|c| {
            c.intent
                .is_some_and(|i| matches!(i, sts2_core::Intent::Debuff { .. }))
        })
        .expect("the Queen opens on a Debuff telegraph natively");
    let native = queen.intent;
    let uid = queen.id;

    let mut wire = combat_state_to_value(&state).expect("serializes");
    wire.as_object_mut().expect("obj").remove("intent_amounts");
    let slot = state
        .enemy_uids
        .iter()
        .position(|u| u.0 == uid)
        .expect("enemy slot");
    // As the host's board says it.
    wire["encounter"][slot]["intents"] = serde_json::json!([{"kind": "CardDebuff"}]);
    let back = sts2_wire::state_from_json(&wire).expect("parses");
    let rebuilt_uid = back.enemy_uids[slot].0;
    let rebuilt = back
        .creatures
        .iter()
        .find(|c| c.id == rebuilt_uid)
        .and_then(|c| c.intent);
    assert_eq!(
        rebuilt.map(|i| std::mem::discriminant(&i)),
        native.map(|i| std::mem::discriminant(&i))
    );
}
