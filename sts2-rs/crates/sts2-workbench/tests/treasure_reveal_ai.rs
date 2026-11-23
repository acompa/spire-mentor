//! A chest the AI walks into shows what it held.
//!
//! The engine opens a treasure room on entry and jumps to the map, so the
//! only trace of the chest is `room_reveal` on the next state. It used to be
//! computed only for hand-played actions (`Game::act`): every AI move goes
//! through `take_native`, which left `pre_action` stale, so a run the AI
//! played (Let AI pick, AI plays, autobattle) passed its chests in silence.
#![cfg(workbench_game_data_v0_111_0)]

use serde_json::{json, Value};
use sts2_workbench::api::{Config, Workbench};

fn call(wb: &mut Workbench, method: &str, route: &str, body: Value) -> Value {
    match wb.call(method, route, &body) {
        Ok(v) => v,
        Err(e) => panic!("{method} {route} -> {} {}", e.status, e.detail),
    }
}

#[test]
fn an_ai_played_run_reveals_its_act_one_chest() {
    let mut wb = Workbench::new(Config {
        assets_base: "https://assets.test".to_string(),
        ..Config::default()
    });
    let mut step = call(
        &mut wb,
        "POST",
        "/api/live/start-faithful",
        json!({"character": "Ironclad", "seed": "7", "ascension": 0}),
    )["step"]
        .clone();
    // Act 1's chest stands mid-act; floor 12 is past it on every path.
    for _ in 0..3000 {
        let state = &step["state"];
        if let Some(reveal) = state.get("room_reveal") {
            assert_eq!(reveal["kind"], "treasure");
            assert!(reveal["relic"]["name"].is_string(), "no relic: {reveal}");
            return;
        }
        assert_ne!(
            step["decision"], "game_over",
            "the run ended before its chest"
        );
        assert!(
            state["floor"].as_i64().unwrap_or(0) <= 12,
            "the AI passed floor 12 without a chest reveal"
        );
        let r = call(&mut wb, "POST", "/api/live/run-mode-step", json!({}));
        assert!(r["error"].is_null(), "run-mode step failed: {}", r["error"]);
        step = r["step"].clone();
    }
    panic!("no chest reveal in 3000 AI steps");
}
