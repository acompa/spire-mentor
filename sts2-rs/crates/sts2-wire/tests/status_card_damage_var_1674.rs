//! GH #1674: a status the engine gives no attack (Infection) keeps 0 damage
//! when a C# view's `stats.damage` names its end-of-turn hit on the player.

use serde_json::json;
use sts2_wire::state_from_json;

fn install() {
    sts2_content::cards::dispatch::install_default_registry();
    sts2_core::register_card_factory(sts2_content::cards::spec_index::card_pod_from_id);
    sts2_core::relic_effects::install_default_registry();
}

fn board(discard: serde_json::Value) -> serde_json::Value {
    install();
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/combat_action_ironclad.json"),
    )
    .expect("fixture");
    let mut v: serde_json::Value = serde_json::from_str(&text).expect("fixture parses");
    v["game_version"] = json!("0.111.0");
    v["deck"]["discard_pile"] = discard;
    let st = state_from_json(&v).expect("builds");
    sts2_wire::emit::combat_state_to_value(&st).expect("emits")
}

#[test]
fn an_infection_keeps_no_attack_while_a_strike_keeps_its_damage() {
    let out = board(json!([
        {"id": "INFECTION", "stable_idx": 90, "kind": "card", "upgraded": false, "base_damage": 3},
        {"id": "STRIKE_IRONCLAD", "stable_idx": 91, "kind": "card", "upgraded": false, "base_damage": 8},
    ]));
    let dmg: Vec<(String, i64)> = out["deck"]["discard_pile"]
        .as_array()
        .expect("pile")
        .iter()
        .map(|c| {
            (
                c["id"].as_str().unwrap_or("").to_owned(),
                c["base_damage"].as_i64().unwrap_or(-1),
            )
        })
        .collect();
    assert!(dmg.contains(&("INFECTION".into(), 0)), "{dmg:?}");
    assert!(dmg.contains(&("STRIKE_IRONCLAD".into(), 8)), "{dmg:?}");
}
