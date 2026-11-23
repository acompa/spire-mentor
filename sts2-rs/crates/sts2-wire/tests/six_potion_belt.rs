//! A belt of more than five potions (Alchemical Coffer, Potion Belt) keeps
//! every potion through a rebuild. The ingest stopped at the potion list's
//! inline capacity of five, so the sixth potion was gone from the rebuilt
//! fight (Spire Bench seed 170000054).

use serde_json::json;

#[test]
fn a_sixth_potion_survives_the_rebuild() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/combat_action_ironclad.json"),
    )
    .expect("fixture");
    let mut v: serde_json::Value = serde_json::from_str(&text).expect("fixture parses");
    v["player"]["potion_slots"] = json!(6);
    v["player"]["potions"] = json!([
        "SNECKO_OIL",
        "BLOOD_POTION",
        "ATTACK_POTION",
        "EXPLOSIVE_AMPOULE",
        "COLORLESS_POTION",
        "BLOCK_POTION"
    ]);
    let state = sts2_wire::state_from_json(&v).expect("parses");
    assert_eq!(state.player.potions.len(), 6);
    assert_eq!(
        state.player.potions.last().copied(),
        Some(sts2_core::PotionKind::BlockPotion)
    );
}
