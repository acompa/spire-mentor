//! GH #1813: Search against The Insatiable in co-op.
//!
//! C# `TheInsatiable.LiquifyMove` puts one `SandpitPower` per player on the
//! boss, each bound to its player by `PowerModel.Target`, and
//! `FranticEscape.OnPlay` raises only the Sandpit whose `Target` is the
//! card's owner (v0.113.0 `FranticEscape.cs`). When a Sandpit runs out,
//! `SandpitPower.AfterRemoved` kills its target outright.
//!
//! The live advisor lost both halves of that. The mod did not send the
//! Sandpit's target (its `applier` is the boss), so the rebuilt board read
//! both Sandpits as ours; and the engine's Frantic Escape raised the boss's
//! first Sandpit, whoever's it was. Either way one of "our" countdowns could
//! not be pushed back, we died on a fixed turn whatever we played, and every
//! line of the search came back with no wins (the advisor's `p_win=0.0187`
//! is its calibration of a raw 0).

use serde_json::{json, Value};
use sts2_wire::emit::mcts::{recommend_value, RecommendParams};

const US: &str = "76561198135460848";
const THEM: &str = "7001";

fn install() {
    sts2_content::cards::dispatch::install_default_registry();
    sts2_core::register_card_factory(sts2_content::cards::spec_index::card_pod_from_id);
    sts2_core::relic_effects::install_default_registry();
}

fn card(id: &str, name: &str, kind: &str, cost: i64) -> Value {
    json!({"id": id, "name": name, "type": kind, "cost": cost, "effective_cost": cost})
}

fn strike() -> Value {
    card("STRIKE_IRONCLAD", "Strike", "Attack", 1)
}

/// The mod's board two turns from the end: The Insatiable at 20 HP with the
/// teammate's Sandpit (3) listed before ours (1). We hold Frantic Escape and
/// two Strikes; next turn draws five more. The teammate has ended their turn
/// and holds nothing that deals damage, so we must survive to win.
fn board(target_field: bool) -> Value {
    let sandpit = |amount: i64, target: &str| {
        let mut p = json!({"id": "SANDPIT_POWER", "name": "Sandpit", "amount": amount});
        if target_field {
            p["target"] = json!(target);
        }
        p
    };
    let defend = card("DEFEND_SILENT", "Defend", "Skill", 1);
    json!({
        "type": "combat_action", "round": 3, "game_version": "v0.113.0",
        "room_type": "Boss", "act": 2,
        "player": {"hp": 60, "max_hp": 80, "block": 0, "energy": 3, "max_energy": 3,
            "net_id": US, "powers": []},
        "hand": [card("FRANTIC_ESCAPE", "Frantic Escape", "Status", 1), strike(), strike()],
        "draw_pile": {"cards": [strike(), strike(), strike(), strike(), strike()]},
        "discard_pile": {"cards": []}, "exhaust_pile": {"cards": []},
        "enemies": [{"id": "THE_INSATIABLE", "name": "The Insatiable", "is_alive": true,
            "hp": 20, "max_hp": 321, "block": 0,
            "powers": [sandpit(3, THEM), sandpit(1, US)],
            "next_move_id": "THRASH_MOVE",
            "intents": [{"kind": "MultiAttackIntent", "intent_type": "Attack",
                "damage": 8, "hits": 2}]}],
        "potions": [],
        "allies": [{"character": "SILENT", "hp": 60, "max_hp": 70, "block": 0,
            "energy": 0, "max_energy": 3, "net_id": THEM, "ended_turn": true,
            "powers": [], "hand": [],
            "draw_pile": {"cards": [defend.clone(), defend.clone(), defend.clone(),
                defend.clone(), defend.clone(), defend]},
            "discard_pile": {"cards": []}, "exhaust_pile": {"cards": []},
            "relics": []}],
        "run_state": {"gold": 0, "character": "IRONCLAD", "ascension": 0,
            "relics": [], "master_deck": []},
    })
}

fn wire(target_field: bool) -> Value {
    let (wire, warnings) =
        sts2_workbench::combat_import::combat_wire_from_bridge(&board(target_field), "Ironclad", 0)
            .expect("the board imports");
    assert!(warnings.is_empty(), "{warnings:?}");
    wire
}

/// The mod names each Sandpit's player (`target`); the wire binds it to that
/// seat. A board from an older mod has no seat for either.
#[test]
fn each_sandpit_reaches_the_wire_bound_to_its_player() {
    let seats = |w: &Value| -> Vec<Value> {
        w["encounter"][0]["powers"]
            .as_array()
            .expect("powers")
            .iter()
            .map(|p| p.get("applier_seat").cloned().unwrap_or(Value::Null))
            .collect()
    };
    assert_eq!(seats(&wire(true)), vec![json!(1), json!(0)]);
    assert_eq!(seats(&wire(false)), vec![Value::Null, Value::Null]);
}

/// Our Frantic Escape buys back OUR Sandpit, so we live through the enemy
/// turn while the teammate's countdown is left alone.
#[test]
fn our_frantic_escape_raises_our_own_sandpit() {
    install();
    let mut st = sts2_wire::state_from_json(&wire(true)).expect("the engine builds it");
    let sandpits = |st: &sts2_core::CombatState| -> Vec<(i32, u8)> {
        st.powers
            .iter()
            .filter(|p| p.kind == sts2_core::PowerKind::Sandpit)
            .map(|p| (p.amount, sts2_core::coop::seat_of_power(st, p.id)))
            .collect()
    };
    assert_eq!(sandpits(&st), vec![(3, 1), (1, 0)]);
    let _ = sts2_core::apply(
        &mut st,
        sts2_core::Action::PlayCard {
            hand_index: 0,
            target: None,
        },
    );
    assert_eq!(sandpits(&st), vec![(3, 1), (2, 0)]);
    let _ = sts2_core::apply(&mut st, sts2_core::Action::EndTurn);
    assert!(st.player.current_hp > 0, "the pit did not take us");
    assert_eq!(sandpits(&st), vec![(2, 1), (1, 0)]);
}

/// The teammate's Frantic Escape buys back their Sandpit, not the boss's
/// first one, which here is ours.
#[test]
fn the_teammates_frantic_escape_raises_their_own_sandpit() {
    install();
    let mut b = board(true);
    let powers = b["enemies"][0]["powers"].as_array_mut().expect("powers");
    powers.reverse();
    b["allies"][0]["ended_turn"] = json!(false);
    b["allies"][0]["energy"] = json!(3);
    b["allies"][0]["hand"] = json!([card("FRANTIC_ESCAPE", "Frantic Escape", "Status", 1)]);
    let (wire, _) =
        sts2_workbench::combat_import::combat_wire_from_bridge(&b, "Ironclad", 0).expect("imports");
    let mut st = sts2_wire::state_from_json(&wire).expect("the engine builds it");
    let sandpits = |st: &sts2_core::CombatState| -> Vec<(i32, u8)> {
        st.powers
            .iter()
            .filter(|p| p.kind == sts2_core::PowerKind::Sandpit)
            .map(|p| (p.amount, sts2_core::coop::seat_of_power(st, p.id)))
            .collect()
    };
    assert_eq!(sandpits(&st), vec![(1, 0), (3, 1)]);
    let _ = sts2_core::coop::apply_as(
        &mut st,
        0,
        sts2_core::Action::PlayCard {
            hand_index: 0,
            target: None,
        },
    );
    assert_eq!(sandpits(&st), vec![(1, 0), (4, 1)]);
}

/// The search sees the way out: Frantic Escape now and the Strikes next turn
/// win, ending the turn without it loses. Before the fix every line scored
/// zero wins, the flat `p_win` the live advisor reported.
#[test]
fn search_finds_the_frantic_escape_line() {
    install();
    let st = sts2_wire::state_from_json(&wire(true)).expect("the engine builds it");
    let params =
        RecommendParams::from_config(&json!({"sims": 2000, "seed": 7}), "insatiable").unwrap();
    let out = recommend_value(&st, &params, true);
    let rows = out["actions"].as_array().expect("actions");
    let p_win = |kind: &str, card_index: Option<i64>| {
        rows.iter()
            .find(|r| {
                r["action"]["action"] == json!(kind)
                    && card_index.is_none_or(|i| r["action"]["card_index"] == json!(i))
            })
            .unwrap_or_else(|| panic!("no {kind} row in {rows:?}"))["p_win"]
            .as_f64()
            .expect("p_win")
    };
    // The pick (a Strike or Frantic Escape first: the turn's three cards
    // commute) wins; ending the turn without Frantic Escape does not.
    assert_eq!(rows[0]["is_best"], json!(true));
    let best = rows[0]["p_win"].as_f64().expect("p_win");
    assert!(best > 0.9, "the best line wins: p_win {best}; {rows:?}");
    let end_turn = p_win("end_turn", None);
    assert!(end_turn < 0.01, "ending the turn loses: p_win {end_turn}");
}
