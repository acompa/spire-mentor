//! A FakeUpgraded Wither reaches the board with its grown damage (C#
//! `Wither.FakeUpgrade`, +3 per level). The rebuild turns it into the level
//! and keeps the canonical base 3, as a native Wither does (GH #1713).

use sts2_core::cards::{CardId, VarKey};
use sts2_wire::emit::combat::combat_state_to_value;

#[test]
fn a_grown_wither_rebuilds_as_the_native_card() {
    let state = sts2_wire::scenario::build_scenario_state_for_version(
        sts2_core::GameVersion::V0_111_0,
        7,
        "Ironclad",
        &["WITHER".to_owned(), "STRIKE_IRONCLAD".to_owned()],
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
    let mut found = false;
    for pile in ["hand", "draw_pile", "discard_pile"] {
        if let Some(cards) = wire["deck"][pile].as_array_mut() {
            for card in cards.iter_mut().filter(|c| c["id"] == "WITHER") {
                card["base_damage"] = serde_json::json!(6);
                found = true;
            }
        }
    }
    assert!(found, "the Wither is on the board");
    let back = sts2_wire::state_from_json(&wire).expect("parses");
    let wither = back
        .cards
        .iter()
        .find(|c| c.card_id == CardId::Wither)
        .expect("rebuilt Wither");
    assert_eq!(wither.combat_var(VarKey::WitherFakeUpgradeLevel), 1);
    assert_eq!(wither.base_damage, 3);
}
