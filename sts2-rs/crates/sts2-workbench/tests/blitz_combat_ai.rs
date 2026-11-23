//! GH #1347: the AI in a Blitz run answers in a fight, and only there.
//!
//! A fight is the same game in every mode: the combat search reads
//! `CombatState::ascension`, which a Blitz run fills with the standard level
//! that scales its enemies (`RunState::combat_ascension`: 0, 8 or 9). So the
//! hint, the search tree, the in-fight choice ranker, "play best" on an
//! in-fight choice and the per-turn grade answer in a Blitz fight. The run
//! policies, the run-level advisors and the autoplayers know neither the
//! Blitz merchant nor its map: their routes stay 409, and outside a fight
//! the fight routes are 409 too. `tests/workbench/test_workbench_blitz.py`
//! is this file's twin.
#![cfg(workbench_game_data_v0_113_0)]

use serde_json::{json, Value};
use sts2_workbench::api::{Config, Workbench};

/// The routes a Blitz fight answers.
const FIGHT_ROUTES: [&str; 5] = [
    "/api/live/hint",
    "/api/live/search_tree",
    "/api/live/choice-ranker",
    "/api/live/auto-action",
    "/api/live/last-turn-grade",
];

/// The run-level AI: refused in a Blitz run wherever it stands.
const RUN_ROUTES: [(&str, &str); 15] = [
    ("POST", "/api/live/e2e-advice"),
    ("POST", "/api/live/last-choice-grade"),
    ("POST", "/api/live/regret-review"),
    ("POST", "/api/live/card-advisor"),
    ("POST", "/api/live/decision-advisor"),
    ("POST", "/api/live/run-mode"),
    ("POST", "/api/live/run-mode-step"),
    ("POST", "/api/live/faithful-step"),
    ("POST", "/api/live/play-to-end"),
    ("POST", "/api/live/autobattle/advance"),
    ("POST", "/api/live/autobattle/step"),
    ("GET", "/api/live/autobattle/verdict"),
    ("POST", "/api/live/ppo-policy"),
    ("POST", "/api/live/wpa-review"),
    ("POST", "/api/live/wpa-verify"),
];

const STREAM_ROUTES: [&str; 3] = [
    "/api/live/card-advisor/stream",
    "/api/live/decision-advisor/stream",
    "/api/live/last-choice-check/stream",
];

fn new_wb() -> Workbench {
    Workbench::new(Config {
        assets_base: "https://assets.test".to_string(),
        ..Config::default()
    })
}

fn call(wb: &mut Workbench, method: &str, route: &str, body: Value) -> Value {
    match wb.call(method, route, &body) {
        Ok(v) => v,
        Err(e) => panic!("{method} {route} -> {} {}", e.status, e.detail),
    }
}

fn search() -> Value {
    json!({"config": {"sims": 48}})
}

fn start(wb: &mut Workbench, level: u8, mode: Option<&str>) -> Value {
    let mut body = json!({"character": "Ironclad", "seed": "BLITZAI", "act1": "overgrowth",
                          "game_version": "v0.113.0", "ascension": level});
    if let Some(mode) = mode {
        body["game_mode"] = json!(mode);
    }
    call(wb, "POST", "/api/live/start", body)
}

fn act(wb: &mut Workbench, action: &str, args: Value) -> Value {
    let resp = call(
        wb,
        "POST",
        "/api/live/action",
        json!({"action": action, "args": args}),
    );
    assert!(
        resp["error"].is_null(),
        "{action} rejected: {}",
        resp["error"]
    );
    resp["step"].clone()
}

/// The first enabled option, or the cards a select asks for.
fn first_option(wb: &mut Workbench, step: &Value) -> Value {
    let state = &step["state"];
    if step["decision"] == "card_select" {
        let n = state["min_select"].as_i64().unwrap_or(1).max(1);
        let indices: Vec<String> = (0..n).map(|i| i.to_string()).collect();
        return act(wb, "select_cards", json!({"indices": indices.join(",")}));
    }
    let opts = state["options"].as_array().cloned().unwrap_or_default();
    match opts.iter().find(|o| o["enabled"].as_bool().unwrap_or(true)) {
        Some(o) => act(wb, o["action"].as_str().unwrap(), o["args"].clone()),
        None => act(wb, "proceed", json!({})),
    }
}

/// Whether the engine is in a fight (a card play or an in-fight choice).
fn in_fight(wb: &Workbench) -> bool {
    wb.session()
        .and_then(|s| s.game())
        .and_then(|g| g.native_run())
        .is_some_and(|rm| {
            rm.combat.is_some() && rm.phase() == sts2_core::run_manager::Phase::Combat
        })
}

/// The 409 a route answers, by whether it names Blitz.
fn refusal(wb: &mut Workbench, method: &str, route: &str) -> (u16, bool, String) {
    match wb.call(method, route, &search()) {
        Ok(v) => panic!("{method} {route} answered: {v}"),
        Err(e) => (e.status, e.detail.contains("Blitz"), e.detail),
    }
}

fn assert_run_ai_is_refused(wb: &mut Workbench, wher: &str) {
    for (method, route) in RUN_ROUTES {
        let (status, blitz, detail) = refusal(wb, method, route);
        assert_eq!(status, 409, "{wher} {route}: {detail}");
        assert!(blitz, "{wher} {route}: {detail}");
    }
    for route in STREAM_ROUTES {
        let mut frames = Vec::new();
        wb.call_stream("POST", route, &json!({}), &mut |f| frames.push(f.clone()));
        assert_eq!(frames.len(), 1, "{wher} {route}");
        let error = frames[0]["error"].as_str().unwrap_or("");
        assert!(error.contains("Blitz"), "{wher} {route}: {}", frames[0]);
    }
}

/// The hint's move as an action (`web/src/hint.ts` `bestAction`).
fn hinted_move(wb: &mut Workbench) -> (String, Value) {
    let hint = call(wb, "POST", "/api/live/hint", search());
    let rows = hint["scores"].as_array().expect("scores");
    let best = rows
        .iter()
        .find(|r| r["is_best"] == json!(true))
        .unwrap_or_else(|| panic!("no starred row: {hint}"));
    let idx = best["stable_idx"].as_i64().expect("stable_idx");
    let mut args = json!({});
    if !best["target_index"].is_null() {
        args["target_index"] = best["target_index"].clone();
    }
    if idx == -1 {
        return ("end_turn".to_string(), json!({}));
    }
    if idx <= -100 {
        args["potion_index"] = json!(-100 - idx);
        return ("use_potion".to_string(), args);
    }
    args["card_index"] = json!(idx);
    ("play_card".to_string(), args)
}

/// ~65 s (a whole blitz fight under the fight AI): the slow profile runs it
/// (`cargo nextest run --profile slow`, `just gate-full`).
#[test]
fn slow_the_fight_ai_answers_in_a_blitz_fight_and_only_there() {
    let mut wb = new_wb();
    // Blitz 3 holds ToughEnemies: the fight is a standard ascension-8 one.
    let mut step = start(&mut wb, 3, Some("blitz"));

    // Neow: no fight, so no AI at all.
    assert_eq!(step["decision"], json!("event_choice"));
    for route in FIGHT_ROUTES {
        let (status, blitz, detail) = refusal(&mut wb, "POST", route);
        assert_eq!(status, 409, "Neow {route}: {detail}");
        assert!(blitz, "Neow {route}: {detail}");
    }
    assert_run_ai_is_refused(&mut wb, "Neow");

    // Neow, the three cards, the first node.
    for _ in 0..3 {
        step = first_option(&mut wb, &step);
    }
    assert_eq!(step["decision"], json!("combat_play"));
    assert!(in_fight(&wb));
    let fight_ascension = wb
        .session()
        .and_then(|s| s.game())
        .and_then(|g| g.combat_state())
        .map(|cs| cs.ascension);
    assert_eq!(fight_ascension, Some(8));

    // The hint and the tree answer; the routes with nothing to say here
    // refuse for their own reason, not for the mode.
    let hint = call(&mut wb, "POST", "/api/live/hint", search());
    let rows = hint["scores"].as_array().expect("scores");
    assert!(!rows.is_empty(), "{hint}");
    assert_eq!(
        rows.iter().filter(|r| r["is_best"] == json!(true)).count(),
        1
    );
    let tree = call(&mut wb, "POST", "/api/live/search_tree", search());
    assert!(tree["error"].is_null(), "{}", tree["error"]);
    assert!(!tree["nodes"].as_array().expect("nodes").is_empty());
    for route in [
        "/api/live/choice-ranker",
        "/api/live/auto-action",
        "/api/live/last-turn-grade",
    ] {
        let (status, blitz, detail) = refusal(&mut wb, "POST", route);
        assert_eq!(status, 409, "fight {route}: {detail}");
        assert!(!blitz, "fight {route}: {detail}");
    }
    assert_run_ai_is_refused(&mut wb, "fight");

    // The hint plays the fight out; an in-fight choice is "play best"'s.
    let mut graded = 0;
    for _ in 0..400 {
        if !in_fight(&wb) {
            break;
        }
        if step["decision"] == "card_select" {
            step = match wb.call("POST", "/api/live/auto-action", &json!({})) {
                Ok(rec) => {
                    assert_eq!(rec["policy"], json!("combat_mcts"), "{rec}");
                    act(
                        &mut wb,
                        rec["action"].as_str().unwrap(),
                        rec["args"].clone(),
                    )
                }
                // A multi-pick has no single answer.
                Err(e) => {
                    assert!(!e.detail.contains("Blitz"), "{}", e.detail);
                    first_option(&mut wb, &step)
                }
            };
            continue;
        }
        assert_eq!(step["decision"], json!("combat_play"), "{step}");
        let (action, args) = hinted_move(&mut wb);
        step = act(&mut wb, &action, args);
        if action == "end_turn" && in_fight(&wb) {
            match wb.call("POST", "/api/live/last-turn-grade", &json!({})) {
                Ok(_) => graded += 1,
                Err(e) => assert!(!e.detail.contains("Blitz"), "{}", e.detail),
            }
        }
    }
    assert!(!in_fight(&wb), "the fight never ended");
    assert_ne!(step["decision"], json!("game_over"), "the hint lost");
    // Informational: a turn is graded only when none of its moves is
    // uncertain, so a short fight may grade none.
    eprintln!("turns graded: {graded}");

    // Out of the fight the AI is gone again.
    for route in FIGHT_ROUTES {
        let (status, blitz, detail) = refusal(&mut wb, "POST", route);
        assert_eq!(status, 409, "after {route}: {detail}");
        assert!(blitz, "after {route}: {detail}");
    }
    assert_run_ai_is_refused(&mut wb, "after the fight");
}

/// A standard run on the same build keeps its AI: the run-level "play best"
/// answers at Neow, and a fight route refuses for its own reason there.
#[test]
fn a_standard_run_keeps_its_ai() {
    let mut wb = new_wb();
    let step = start(&mut wb, 3, None);
    assert_eq!(step["decision"], json!("event_choice"));
    let rec = call(&mut wb, "POST", "/api/live/auto-action", json!({}));
    assert!(rec["action"].is_string(), "{rec}");
    for route in ["/api/live/hint", "/api/live/search_tree"] {
        let (status, blitz, detail) = refusal(&mut wb, "POST", route);
        assert_eq!(status, 409, "{route}: {detail}");
        assert!(!blitz, "{route}: {detail}");
    }
    let (_, blitz, detail) = refusal(&mut wb, "POST", "/api/live/play-to-end");
    assert!(!blitz, "{detail}");
}
