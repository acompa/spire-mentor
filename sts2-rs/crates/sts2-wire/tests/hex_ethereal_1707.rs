//! GH #1707: C# HexPower adds Ethereal to every Hexed card's keywords for as
//! long as it lasts, so the board flags them `ethereal`. The engine reads
//! that off the affliction; a rebuild must not also stamp the card's own
//! Ethereal-this-combat grant, which outlives the Hex and shows in the
//! card's features.

use sts2_wire::emit::combat::combat_state_to_value;

fn base() -> serde_json::Value {
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
    combat_state_to_value(&state).expect("serializes")
}

fn hand_flags(wire: &serde_json::Value) -> Vec<serde_json::Value> {
    let back = sts2_wire::state_from_json(wire).expect("parses");
    let again = combat_state_to_value(&back).expect("serializes");
    again["deck"]["hand"]
        .as_array()
        .expect("hand")
        .iter()
        .map(|c| c.get("flags").cloned().unwrap_or(serde_json::Value::Null))
        .collect()
}

#[test]
fn a_hexed_cards_ethereal_flag_stays_the_hexs() {
    let mut wire = base();
    let hand = wire["deck"]["hand"].as_array_mut().expect("hand");
    assert!(!hand.is_empty());
    for c in hand.iter_mut() {
        c["flags"] = serde_json::json!(["ethereal"]);
        c["affliction"] = serde_json::json!("Hexed");
    }
    for f in hand_flags(&wire) {
        assert!(
            !f.to_string().contains("ethereal"),
            "Hexed card kept a grant: {f}"
        );
    }
}

#[test]
fn an_unhexed_cards_ethereal_flag_is_its_own() {
    let mut wire = base();
    let hand = wire["deck"]["hand"].as_array_mut().expect("hand");
    hand[0]["flags"] = serde_json::json!(["ethereal"]);
    assert!(hand_flags(&wire)[0].to_string().contains("ethereal"));
}

/// GH #1762: Ghost Seed's Ethereal on a Basic Strike is the card's own and
/// outlives the Hex.
#[test]
fn a_hexed_ghost_seed_strike_keeps_its_ethereal() {
    let mut wire = base();
    let relics = wire["player"]["relics"].as_array_mut().expect("relics");
    relics.push(serde_json::json!({"id": "GHOST_SEED"}));
    let hand = wire["deck"]["hand"].as_array_mut().expect("hand");
    for c in hand.iter_mut() {
        c["flags"] = serde_json::json!(["ethereal"]);
        c["affliction"] = serde_json::json!("Hexed");
    }
    for f in hand_flags(&wire) {
        assert!(
            f.to_string().contains("ethereal"),
            "lost Ghost Seed's Ethereal: {f}"
        );
    }
}

/// While the Hex lasts the board shows a Hexed card Ethereal (C#
/// `HexPower.TryModifyKeywordsInCombat`), so the engine's own board must too:
/// a Dampened Ghost Seed Strike loses its own Ethereal but stays Ethereal
/// under the Hex (Spire Bench seed 170000044).
#[test]
fn a_hexed_card_shows_the_hexs_ethereal_while_the_hex_lasts() {
    let mut wire = base();
    wire["player"]["powers"] = serde_json::json!([{ "name": "HEX", "amount": 2 }]);
    let hand = wire["deck"]["hand"].as_array_mut().expect("hand");
    for c in hand.iter_mut() {
        c["flags"] = serde_json::json!(["ethereal"]);
        c["affliction"] = serde_json::json!("Hexed");
    }
    for f in hand_flags(&wire) {
        assert!(
            f.to_string().contains("ethereal"),
            "Hexed card under a Hex shown non-Ethereal: {f}"
        );
    }
}
