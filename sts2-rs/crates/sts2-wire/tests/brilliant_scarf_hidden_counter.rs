//! GH #1735: C# hides Brilliant Scarf's counter once the free fifth card of
//! the turn has been played. The rebuild takes the count from the turn's
//! card plays then, instead of restarting it at 0.

use serde_json::json;
use sts2_core::relics::RelicKind;

#[test]
fn a_scarf_with_no_shown_counter_counts_the_turns_plays() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/combat_action_ironclad.json"),
    )
    .expect("fixture");
    let mut v: serde_json::Value = serde_json::from_str(&text).expect("fixture parses");
    v["player"]["relics"] = json!([{"id": "BRILLIANT_SCARF", "counter": null}]);
    v["player"]["cards_played_this_turn"] = json!([3, 2, 0, 0, 0]);
    let state = sts2_wire::state_from_json(&v).expect("parses");
    let scarf = state
        .relics
        .iter()
        .find(|r| r.kind == RelicKind::BrilliantScarf)
        .expect("scarf");
    assert_eq!(scarf.counter, 5);
}
