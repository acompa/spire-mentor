//! A card the engine builds with no base block (Second Wind keeps its block
//! per exhausted card in a var) reaches the board with the block a C# view
//! reads. The rebuild keeps the engine's 0, as a native card has.

use sts2_core::cards::CardId;
use sts2_wire::emit::combat::combat_state_to_value;

#[test]
fn a_second_wind_with_a_shown_block_rebuilds_as_the_native_card() {
    let state = sts2_wire::scenario::build_scenario_state_for_version(
        sts2_core::GameVersion::V0_111_0,
        7,
        "Ironclad",
        &["SECOND_WIND".to_owned(), "STRIKE_IRONCLAD".to_owned()],
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
    let native = state
        .cards
        .iter()
        .find(|c| c.card_id == CardId::SecondWind)
        .expect("native Second Wind")
        .base_block;
    let mut wire = combat_state_to_value(&state).expect("serializes");
    let mut found = false;
    for pile in ["hand", "draw_pile", "discard_pile"] {
        if let Some(cards) = wire["deck"][pile].as_array_mut() {
            for card in cards.iter_mut().filter(|c| c["id"] == "SECOND_WIND") {
                card["base_block"] = serde_json::json!(5);
                found = true;
            }
        }
    }
    assert!(found, "Second Wind is on the board");
    let back = sts2_wire::state_from_json(&wire).expect("parses");
    let rebuilt = back
        .cards
        .iter()
        .find(|c| c.card_id == CardId::SecondWind)
        .expect("rebuilt Second Wind");
    assert_eq!(native, 0);
    assert_eq!(rebuilt.base_block, native);
}
