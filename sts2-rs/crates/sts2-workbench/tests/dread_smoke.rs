//! GH #1346: Dread mode over `Workbench::call`.
//!
//! A live run can be started in Dread (an ascension-10 run on v0.113.0 with
//! a per-act dominion). The mode travels with the run through every rebuild
//! path (seek, save -> restore, persist -> import, run log -> "play from
//! here"), the state names it and the dominion in force, and a standard
//! run's payloads never carry either key.
#![cfg(workbench_game_data_v0_113_0)]

use serde_json::{json, Value};
use sts2_core::{GameMode, GameVersion};
use sts2_workbench::api::{Config, Workbench};

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

fn start_body(extra: Value) -> Value {
    let mut body = json!({"character": "Ironclad", "seed": "DREAD", "act1": "overgrowth",
                          "game_version": "v0.113.0"});
    for (k, v) in extra.as_object().expect("object") {
        body[k] = v.clone();
    }
    body
}

/// The live run's engine-side mode, ascension and hashed master seed.
fn live_run(wb: &Workbench) -> (GameMode, u8, u64) {
    let rm = wb
        .session()
        .and_then(|s| s.game())
        .and_then(|g| g.native_run())
        .expect("a live run");
    (
        rm.run_state.game_mode,
        rm.ascension_level,
        rm.run_state.rng.seed,
    )
}

/// Take the first enabled option `steps` times (Neow, then the map...).
fn advance(wb: &mut Workbench, mut step: Value, steps: usize) -> Value {
    for _ in 0..steps {
        let state = &step["state"];
        let (action, args) = if step["decision"] == "card_select" {
            ("select_cards".to_string(), json!({"indices": "0"}))
        } else {
            let opts = state["options"].as_array().cloned().unwrap_or_default();
            match opts.iter().find(|o| o["enabled"].as_bool().unwrap_or(true)) {
                Some(o) => (o["action"].as_str().unwrap().to_string(), o["args"].clone()),
                None => ("proceed".to_string(), json!({})),
            }
        };
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
        step = resp["step"].clone();
    }
    step
}

#[test]
fn a_dread_start_is_an_ascension_10_run_that_names_its_dominion() {
    let mut wb = new_wb();
    // The requested ascension is overridden: the game's Dread screen embarks
    // at 10 whatever the lobby showed.
    let step = call(
        &mut wb,
        "POST",
        "/api/live/start",
        start_body(json!({"game_mode": "dread", "ascension": 3})),
    );
    let state = &step["state"];
    assert_eq!(state["game_mode"], json!("dread"));
    let dominion = state["dominion"].as_str().expect("a dominion entry");
    assert!(dominion.ends_with("_DOMINION"), "{dominion}");
    assert_eq!(state["ascension"], json!(10));
    let (mode, ascension, seed) = live_run(&wb);
    assert_eq!(mode, GameMode::Dread);
    assert_eq!(ascension, 10);
    assert_eq!(seed, GameVersion::V0_113_0.run_seed_hash("DREAD"));
    let sess = wb.session().expect("session");
    assert_eq!(sess.game_mode, GameMode::Dread);
    assert_eq!(sess.ascension, 10);
}

#[test]
fn a_standard_run_names_neither_key() {
    let mut wb = new_wb();
    for extra in [
        json!({}),
        json!({"game_mode": "standard"}),
        json!({"game_mode": null}),
    ] {
        let mut body = start_body(json!({"ascension": 10}));
        for (k, v) in extra.as_object().expect("object") {
            body[k] = v.clone();
        }
        let step = call(&mut wb, "POST", "/api/live/start", body);
        let state = step["state"].as_object().expect("state");
        assert!(!state.contains_key("game_mode"), "{extra}");
        assert!(!state.contains_key("dominion"), "{extra}");
        assert_eq!(live_run(&wb).0, GameMode::Standard);
        let save = wb.build_save("std", "t").expect("save");
        assert!(save.get("game_mode").is_none());
        assert!(wb.export_persist().get("game_mode").is_none());
    }
    call(&mut wb, "POST", "/api/live/quit", Value::Null);
    let log = wb.take_run_log().expect("log");
    assert!(!log.text.contains("game_mode"));
    assert!(!log.text.contains("dominion"));
}

#[test]
fn a_dread_start_the_engine_cannot_build_is_a_400() {
    let mut wb = new_wb();
    let cases = [
        (
            "/api/live/start",
            json!({"seed": "X", "game_mode": "dread", "game_version": "v0.111.0"}),
            "v0.113.0",
        ),
        (
            "/api/live/start",
            json!({"seed": "X", "game_mode": "dread", "game_version": "v0.107.1"}),
            "v0.113.0",
        ),
        (
            "/api/live/start",
            start_body(json!({"game_mode": "nightmare"})),
            "unknown game_mode",
        ),
        (
            "/api/live/start",
            start_body(json!({"game_mode": 1})),
            "game_mode must be a string",
        ),
        (
            "/api/live/start",
            start_body(json!({"game_mode": "dread", "backend": "csharp"})),
            "rust engine",
        ),
        (
            "/api/live/start-faithful",
            json!({"seed": "run_1", "game_mode": "dread", "game_version": "v0.113.0"}),
            "Dread",
        ),
        (
            "/api/live/import-state",
            json!({"payload": "{}", "game_mode": "dread", "game_version": "v0.113.0"}),
            "Dread",
        ),
    ];
    for (route, body, needle) in cases {
        let e = wb.call("POST", route, &body).unwrap_err();
        assert_eq!(e.status, 400, "{route} {body}: {}", e.detail);
        assert!(e.detail.contains(needle), "{route} {body}: {}", e.detail);
    }
    // Nothing was started.
    assert!(wb.session().is_none());
}

#[test]
fn save_persist_and_run_log_keep_a_dread_run() {
    let mut wb = new_wb();
    let step = call(
        &mut wb,
        "POST",
        "/api/live/start",
        start_body(json!({"game_mode": "dread"})),
    );
    let step = advance(&mut wb, step, 3);
    assert_eq!(step["state"]["game_mode"], json!("dread"));
    let want = live_run(&wb);
    assert_eq!(want.0, GameMode::Dread);

    // Seek to the root and back rebuilds a Dread run.
    let tl = call(&mut wb, "GET", "/api/live/timeline", Value::Null);
    let leaf = tl["active_leaf"].clone();
    let root = call(&mut wb, "POST", "/api/live/seek", json!({"node_id": 0}));
    assert_eq!(root["step"]["state"]["game_mode"], json!("dread"));
    assert_eq!(live_run(&wb), want);
    let back = call(&mut wb, "POST", "/api/live/seek", json!({"node_id": leaf}));
    assert_eq!(back["step"]["state"], step["state"]);

    // Save -> restore.
    let save = wb
        .build_save("dread", "2026-11-14T00:00:00Z")
        .expect("save");
    assert_eq!(save["game_mode"], json!("dread"));
    assert_eq!(save["ascension"], json!(10));
    let mut wb2 = new_wb();
    let restored = wb2
        .restore_save(&save, sts2_workbench::runlog::DEFAULT_NOW_ISO)
        .expect("restore");
    assert_eq!(live_run(&wb2), want);
    assert_eq!(restored["state"], step["state"]);

    // Persist -> import.
    let persist = wb.export_persist();
    assert_eq!(persist["game_mode"], json!("dread"));
    let mut wb3 = new_wb();
    let imported = wb3.import_persist(&persist, |_, _| {}).expect("reload");
    assert_eq!(live_run(&wb3), want);
    assert_eq!(imported["state"], step["state"]);

    // Run log meta -> the replay viewer's states -> "play from here".
    call(&mut wb, "POST", "/api/live/quit", Value::Null);
    let log = wb.take_run_log().expect("log");
    let meta: Value = serde_json::from_str(log.text.lines().next().unwrap()).unwrap();
    assert_eq!(meta["data"]["game_mode"], json!("dread"));
    let mut wb4 = new_wb();
    wb4.load_run(&log.filename, &log.text).expect("load");
    let recorded = call(&mut wb4, "GET", "/api/step/1", Value::Null);
    assert_eq!(recorded["state"]["game_mode"], json!("dread"));
    call(&mut wb4, "POST", "/api/live/from_replay", json!({"idx": 2}));
    assert_eq!(live_run(&wb4), want);

    // A stored payload without the key is a standard run...
    let mut plain = save.clone();
    plain.as_object_mut().expect("save").remove("game_mode");
    let mut wb5 = new_wb();
    // (its line may diverge from the Dread one; the constructor is the point)
    let _ = wb5.restore_save(&plain, sts2_workbench::runlog::DEFAULT_NOW_ISO);
    if wb5.session().is_some() {
        assert_eq!(live_run(&wb5).0, GameMode::Standard);
    }
    // ...and an unknown mode is refused, not replayed as a standard run.
    let mut bad = save.clone();
    bad["game_mode"] = json!("nightmare");
    assert!(new_wb()
        .restore_save(&bad, sts2_workbench::runlog::DEFAULT_NOW_ISO)
        .is_err_and(|e| e.status == 409));
    let mut bad = persist.clone();
    bad["game_mode"] = json!("nightmare");
    assert!(new_wb()
        .import_persist(&bad, |_, _| {})
        .is_err_and(|e| e.status == 409));
    // A Dread payload naming a build without Dread is refused, not a panic.
    let mut bad = save.clone();
    bad["game_version"] = json!("v0.111.0");
    assert!(new_wb()
        .restore_save(&bad, sts2_workbench::runlog::DEFAULT_NOW_ISO)
        .is_err_and(|e| e.status == 409));
}
