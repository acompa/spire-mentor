//! GH #1496: the v0.113.0 Wuppy pet survives the wire as `$.player.wuppy`
//! (absent without one) and is rebuilt with its `WuppysLoyaltyPower`.

use sts2_core::{MonsterKind, PowerKind};
use sts2_wire::emit::combat_state_to_value;
use sts2_wire::state_from_json;

fn fixture() -> serde_json::Value {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/combat_action_ironclad.json"),
    )
    .expect("fixture");
    serde_json::from_str(&text).expect("fixture parses")
}

fn pet(st: &sts2_core::CombatState) -> Option<&sts2_core::Creature> {
    st.creatures
        .iter()
        .find(|c| c.species == Some(MonsterKind::Wuppy))
}

#[test]
fn a_board_without_wuppy_emits_none() {
    let st = state_from_json(&fixture()).expect("builds");
    assert!(pet(&st).is_none());
    let v = combat_state_to_value(&st).expect("emits");
    assert!(v["player"].get("wuppy").is_none());
}

#[test]
fn wuppy_round_trips_alive_and_dead() {
    for (alive, hp) in [(true, 17), (false, 0)] {
        let mut v = fixture();
        v["game_version"] = serde_json::json!("v0.113.0");
        v["player"]["wuppy"] =
            serde_json::json!({"alive": alive, "hp": hp, "max_hp": 30, "block": 4});
        let st = state_from_json(&v).expect("builds");
        let c = pet(&st).expect("Wuppy rebuilt");
        assert_eq!((c.current_hp, c.max_hp, c.block), (hp, 30, 4));
        assert!(!st.enemy_uids.contains(&sts2_core::CreatureUid(c.id)));
        assert!(c.powers.iter().any(|u| st
            .powers
            .get_by_id(u.0)
            .is_some_and(|p| p.kind == PowerKind::WuppysLoyalty)));
        let out = combat_state_to_value(&st).expect("emits");
        assert_eq!(out["player"]["wuppy"], v["player"]["wuppy"]);
        let back = state_from_json(&out).expect("re-parses");
        assert_eq!(
            combat_state_to_value(&back).expect("emits")["player"]["wuppy"],
            v["player"]["wuppy"]
        );
    }
}

#[test]
fn the_monster_name_resolves() {
    assert_eq!(
        sts2_wire::state_dict::monster_kind_from_python_name("Wuppy"),
        Some(MonsterKind::Wuppy)
    );
    assert_eq!(
        sts2_wire::state_dict::monster_kind_from_python_name("WUPPY"),
        Some(MonsterKind::Wuppy)
    );
}
