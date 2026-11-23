//! GH #1738: C# hides Toy Box's counter once it is used up (15 combats on
//! v0.111.0). The rebuild reads a missing counter as a spent Toy Box instead
//! of one that has seen no combat.

use serde_json::json;
use sts2_core::relics::RelicKind;

#[test]
fn a_toy_box_with_no_shown_counter_is_used_up() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/combat_action_ironclad.json"),
    )
    .expect("fixture");
    let mut v: serde_json::Value = serde_json::from_str(&text).expect("fixture parses");
    v["game_version"] = json!("v0.111.0");
    v["player"]["relics"] = json!([{"id": "TOY_BOX", "counter": null}]);
    let state = sts2_wire::state_from_json(&v).expect("parses");
    let toy = state
        .relics
        .iter()
        .find(|r| r.kind == RelicKind::ToyBox)
        .expect("toy box");
    assert_eq!(toy.counter, 15);
}
