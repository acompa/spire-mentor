//! GH #1776: a v0.113 Infested Prisms board with a Tainted Outbreak.
//!
//! `VitalSparkPower` afflicts only Skills with Tainted (`Tainted.cs`
//! `CanAfflictCardType => cardType == CardType.Skill`), and v0.111.0 made
//! Outbreak a Skill (`Outbreak.cs:23`: `base(3, CardType.Skill,
//! CardRarity.Rare, TargetType.AllEnemies)`; v0.107.1 had a 1-cost Power).
//! The wire loader built every card from the v0.107.1 spec, so it read the
//! Silent's Outbreak as a Power, refused its Tainted, and failed the whole
//! board: the advisor had no plan for the entire fight, from the host's view
//! (`$.allies[0].discard_pile[6]`) and the partner's own (`$.deck.*`).

use serde_json::json;
use sts2_core::card_afflictions::CardAfflictionKind;
use sts2_core::cards::{CardId, CardType, TargetType};
use sts2_core::CombatState;
use sts2_wire::state_from_json;

/// The card as the live mod sends it: the C# id, its build's cost, Tainted.
fn tainted_outbreak(stable_idx: u32) -> serde_json::Value {
    json!({
        "affliction": "Tainted", "affliction_stacks": 3,
        "base_block": 0, "base_cost": 3, "base_damage": 0, "cost_override": 3,
        "id": "OUTBREAK", "kind": "card", "stable_idx": stable_idx, "upgraded": false,
    })
}

fn card(id: &str, stable_idx: u32) -> serde_json::Value {
    json!({"base_block": 5, "base_cost": 1, "base_damage": 0, "cost_override": 1,
           "id": id, "kind": "card", "stable_idx": stable_idx, "upgraded": false})
}

/// The recorded Infested Prism elite board (VITAL_SPARK on the Prism), moved
/// to `version`.
fn prism_board(version: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/gh1129_hellraiser_seeker_pre.json"),
    )
    .expect("fixture");
    let mut v: serde_json::Value = serde_json::from_str(&text).expect("fixture parses");
    v["game_version"] = json!(version);
    v
}

fn outbreaks(st: &CombatState) -> Vec<&sts2_core::cards::Card> {
    st.cards
        .iter()
        .filter(|c| c.card_id == CardId::OutbreakCard)
        .collect()
}

fn assert_tainted_skill(st: &CombatState, want: usize) {
    let found = outbreaks(st);
    assert_eq!(found.len(), want, "Outbreaks on the board");
    for c in found {
        assert_eq!(c.card_type, CardType::Skill);
        assert_eq!(c.target_type, TargetType::AllEnemies);
        assert_eq!(c.base_cost(), 3);
        assert_eq!(
            c.affliction.map(|a| (a.kind, a.stacks)),
            Some((CardAfflictionKind::Tainted, 3))
        );
    }
}

/// The partner's own advisor: its Outbreak sits in `$.deck.*`.
#[test]
fn a_tainted_outbreak_in_our_piles_loads_as_a_skill() {
    let mut v = prism_board("v0.113.0");
    v["player"]["class"] = json!("silent");
    v["deck"]["discard_pile"]
        .as_array_mut()
        .expect("discard")
        .push(tainted_outbreak(40));
    v["deck"]["hand"]
        .as_array_mut()
        .expect("hand")
        .push(tainted_outbreak(41));
    let st = state_from_json(&v).expect("a v0.113 Tainted Outbreak loads");
    assert_tainted_skill(&st, 2);
}

/// The Ironclad host's advisor: the Silent teammate's Outbreak sits in
/// `$.allies[0].discard_pile[6]`, exactly where the live run failed.
#[test]
fn a_teammates_tainted_outbreak_loads_as_a_skill() {
    let mut v = prism_board("v0.113.0");
    let mut discard: Vec<serde_json::Value> = (50..56).map(|i| card("DEFEND_SILENT", i)).collect();
    discard.push(tainted_outbreak(56));
    v["coop_full"] = json!(true);
    v["allies"] = json!([{
        "class": "silent", "hp": 60, "max_hp": 70, "block": 0,
        "energy": 3, "max_energy": 3, "powers": [], "ended_turn": false,
        "hand": [card("DEFEND_SILENT", 60)],
        "draw_pile": [tainted_outbreak(61)],
        "discard_pile": discard, "exhaust_pile": [], "play_pile": [],
        "relics": [],
    }]);
    let st = state_from_json(&v).expect("the host's view loads");
    assert_tainted_skill(&st, 2);
}

/// The type gate is C#'s application rule (`CardCmd.Afflict`), not an
/// invariant of a card that already carries one: a board whose card the
/// engine types differently than the game did keeps the game's affliction
/// instead of failing. Here the v0.107.1 Outbreak, which is a Power.
#[test]
fn a_reported_affliction_is_kept_past_the_type_gate() {
    let mut v = prism_board("v0.107.1");
    v["deck"]["hand"]
        .as_array_mut()
        .expect("hand")
        .push(tainted_outbreak(41));
    let st = state_from_json(&v).expect("the board loads");
    let found = outbreaks(&st);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].card_type, CardType::Power, "v0.107.1 Outbreak");
    assert_eq!(
        found[0].affliction.map(|a| a.kind),
        Some(CardAfflictionKind::Tainted)
    );
}

/// Every build's spec, not only v0.113's: the v0.111.0 board reads the
/// Skill too, and the v0.107.1 one keeps its Power.
#[test]
fn outbreak_loads_with_its_builds_type() {
    for (version, ty) in [
        ("v0.107.1", CardType::Power),
        ("v0.111.0", CardType::Skill),
        ("v0.113.0", CardType::Skill),
    ] {
        let mut v = prism_board(version);
        let mut plain = tainted_outbreak(41);
        plain.as_object_mut().expect("card").remove("affliction");
        plain
            .as_object_mut()
            .expect("card")
            .remove("affliction_stacks");
        v["deck"]["hand"].as_array_mut().expect("hand").push(plain);
        let st = state_from_json(&v).expect("loads");
        assert_eq!(outbreaks(&st)[0].card_type, ty, "{version}");
    }
}
