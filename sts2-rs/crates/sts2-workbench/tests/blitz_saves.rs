//! GH #1347: a Blitz run saved mid-run comes back as the same run.
//!
//! A workbench save is the active line (the run identity plus every action
//! from the root), and loading replays it, so it is only as good as the
//! engine's determinism over the Blitz-only state: the merchant's rolled
//! wares and remaining picks, and the chest's second round. Three positions
//! are saved — mid-fight, at the merchant with picks left, and in the chest
//! between its two picks — and each must come back identical, state and
//! engine, through every rebuild path (a named save, the reload snapshot, a
//! seek to the root and back), and then play on identically.
//! `tests/workbench/test_workbench_blitz_saves.py` is this file's twin.
#![cfg(workbench_game_data_v0_113_0)]

use std::collections::BTreeSet;

use serde_json::{json, Value};
use sts2_core::run_manager::RunManager;
use sts2_core::GameMode;
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

fn run(wb: &Workbench) -> &RunManager {
    wb.session()
        .and_then(|s| s.game())
        .and_then(|g| g.native_run())
        .expect("a live run")
}

/// The test's player. It needs no AI: it only has to get through two
/// hallway fights and an elite on some seed, and to be a pure function of
/// the step, so the same step always yields the same action.
#[derive(Default)]
struct Player {
    /// Wares already asked for at the merchant on a floor (one that is
    /// refused stays on offer and must not be asked for again).
    tried: BTreeSet<(i64, i64)>,
}

impl Player {
    fn choose(&mut self, step: &Value) -> (String, Value) {
        let state = &step["state"];
        let options = state["options"].as_array().cloned().unwrap_or_default();
        let enabled = |o: &&Value| o["enabled"].as_bool().unwrap_or(true);
        let verb = |o: &Value| (o["action"].as_str().unwrap().to_string(), o["args"].clone());
        match step["decision"].as_str().unwrap_or("") {
            "combat_play" => {
                let hand = state["hand"].as_array().cloned().unwrap_or_default();
                let playable = |c: &&Value| c["can_play"].as_bool() == Some(true);
                let aimed = |c: &&Value| c["target_type"] == json!("AnyEnemy");
                // The weakest enemy, by its place in the list.
                let target = state["enemies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .min_by_key(|e| e["hp"].as_i64().unwrap_or(i64::MAX))
                    .map(|e| e["index"].clone());
                let card = hand
                    .iter()
                    .filter(playable)
                    .find(aimed)
                    .or_else(|| hand.iter().find(playable));
                let Some(c) = card else {
                    return ("end_turn".to_string(), json!({}));
                };
                let mut args = json!({"card_index": c["index"]});
                if aimed(&c) {
                    args["target_index"] = target.unwrap_or_else(|| json!(0));
                }
                ("play_card".to_string(), args)
            }
            "card_select" => {
                let n = state["min_select"].as_i64().unwrap_or(1).max(1);
                let indices: Vec<String> = (0..n).map(|i| i.to_string()).collect();
                (
                    "select_cards".to_string(),
                    json!({"indices": indices.join(",")}),
                )
            }
            "shop" => {
                let floor = state["floor"].as_i64().unwrap_or(0);
                let left = state["blitz_choices_remaining"].as_i64().unwrap_or(0);
                let ware = options.iter().filter(enabled).find(|o| {
                    o["action"] == "blitz_choice"
                        && !self
                            .tried
                            .contains(&(floor, o["args"]["index"].as_i64().unwrap_or(-1)))
                });
                if let Some(o) = ware.filter(|_| left > 0) {
                    self.tried
                        .insert((floor, o["args"]["index"].as_i64().unwrap_or(-1)));
                    return verb(o);
                }
                ("leave_room".to_string(), json!({}))
            }
            _ => options
                .iter()
                .find(enabled)
                .map_or_else(|| ("proceed".to_string(), json!({})), verb),
        }
    }
}

fn act(wb: &mut Workbench, action: &str, args: &Value) -> Value {
    let resp = call(
        wb,
        "POST",
        "/api/live/action",
        json!({"action": action, "args": args}),
    );
    assert!(
        resp["error"].is_null(),
        "{action} {args} rejected: {}",
        resp["error"]
    );
    resp["step"].clone()
}

/// `other` is the run `wb` is, to the last RNG counter.
fn assert_same_run(wb: &Workbench, other: &Workbench, wher: &str) {
    let (a, b) = (run(wb), run(other));
    assert_eq!(a.phase(), b.phase(), "{wher}: phase");
    assert_eq!(a.run_state, b.run_state, "{wher}: run state");
    assert_eq!(a.combat, b.combat, "{wher}: combat state");
}

/// Save `wb` where it stands, rebuild it three ways, and play all of them
/// on with the same actions.
fn check_position(wb: &mut Workbench, player: &mut Player, step: &Value, wher: &str) -> Value {
    assert_eq!(run(wb).run_state.game_mode, GameMode::Blitz);
    // The position as a rebuild shows it: an action's own response may
    // carry what only that action produced.
    let at_save = call(wb, "GET", "/api/live/state", Value::Null);
    assert_eq!(at_save["decision"], step["decision"], "{wher}");

    // A named save.
    let save = wb.build_save(wher, "2026-11-15T00:00:00Z").expect("save");
    assert_eq!(save["game_mode"], json!("blitz"));
    let mut saved = new_wb();
    let restored = saved
        .restore_save(&save, sts2_workbench::runlog::DEFAULT_NOW_ISO)
        .unwrap_or_else(|e| panic!("{wher}: restore -> {} {}", e.status, e.detail));
    assert_eq!(restored["decision"], step["decision"], "{wher}: save");
    assert_eq!(restored["state"], at_save["state"], "{wher}: save");
    assert_same_run(wb, &saved, &format!("{wher}: save"));

    // The reload snapshot.
    let mut reloaded = new_wb();
    let imported = reloaded
        .import_persist(&wb.export_persist(), |_, _| {})
        .unwrap_or_else(|e| panic!("{wher}: reload -> {} {}", e.status, e.detail));
    assert_eq!(imported["state"], at_save["state"], "{wher}: reload");
    assert_same_run(wb, &reloaded, &format!("{wher}: reload"));

    // A checkpoint: the restored run seeks to its root and back.
    let leaf = call(&mut saved, "GET", "/api/live/timeline", Value::Null)["active_leaf"].clone();
    let root = call(&mut saved, "POST", "/api/live/seek", json!({"node_id": 0}));
    assert_eq!(root["step"]["state"]["game_mode"], json!("blitz"));
    let back = call(
        &mut saved,
        "POST",
        "/api/live/seek",
        json!({"node_id": leaf}),
    );
    assert_eq!(back["step"]["state"], at_save["state"], "{wher}: seek");
    assert_same_run(wb, &saved, &format!("{wher}: seek"));

    // All of them play on as one run.
    let mut step = step.clone();
    for i in 0..12 {
        if step["decision"] == "game_over" {
            break;
        }
        let (action, args) = player.choose(&step);
        step = act(wb, &action, &args);
        for (name, other) in [("save", &mut saved), ("reload", &mut reloaded)] {
            let theirs = act(other, &action, &args);
            let at = format!("{wher}: {name}, {i} actions on ({action} {args})");
            assert_eq!(theirs["decision"], step["decision"], "{at}");
            assert_eq!(theirs["state"], step["state"], "{at}");
            assert_same_run(wb, other, &at);
        }
    }
    step
}

/// What a walk saved at.
#[derive(Default, Debug)]
struct Saved {
    fight: bool,
    merchant: bool,
    chest: bool,
}

/// Walk one run to the chest's second round, checking each position on the
/// way. Stops early (returning what it reached) if the player dies.
fn walk(seed: &str, level: u8) -> Saved {
    let mut saved = Saved::default();
    let mut player = Player::default();
    let mut wb = new_wb();
    let mut step = call(
        &mut wb,
        "POST",
        "/api/live/start",
        json!({"character": "Ironclad", "seed": seed, "act1": "roll", "ascension": level,
               "game_version": "v0.113.0", "game_mode": "blitz"}),
    );
    let mut fight_actions = 0;
    for _ in 0..2000 {
        let state = &step["state"];
        match step["decision"].as_str().unwrap_or("") {
            "game_over" => return saved,
            // Mid-fight and mid-turn: two cards into the first fight.
            "combat_play" if !saved.fight => {
                if fight_actions == 2 {
                    saved.fight = true;
                    step = check_position(&mut wb, &mut player, &step, &format!("{seed} fight"));
                    continue;
                }
                fight_actions += 1;
            }
            // The merchant with a ware taken and picks left.
            "shop"
                if !saved.merchant
                    && state["blitz_choices_remaining"].as_i64().unwrap_or(0) > 0
                    && state["blitz_wares"]
                        .as_array()
                        .is_some_and(|w| w.iter().any(|w| w["status"] == "Used")) =>
            {
                saved.merchant = true;
                step = check_position(&mut wb, &mut player, &step, &format!("{seed} merchant"));
                continue;
            }
            // The chest between its two picks.
            "rewards" if !saved.chest && state["chest_picks_left"] == json!(1) => {
                saved.chest = true;
                check_position(&mut wb, &mut player, &step, &format!("{seed} chest"));
                return saved;
            }
            _ => {}
        }
        let (action, args) = player.choose(&step);
        step = act(&mut wb, &action, &args);
    }
    panic!("{seed}: the walk never reached the chest: {saved:?}");
}

#[test]
fn a_blitz_run_saved_mid_run_reloads_and_plays_on_identically() {
    let mut all = Saved::default();
    // The player is no strategist: take the first seeds it survives on.
    for (seed, level) in [
        ("BLITZSAVE1", 0),
        ("BLITZSAVE2", 3),
        ("BLITZSAVE3", 5),
        ("BLITZSAVE4", 0),
        ("BLITZSAVE5", 0),
        ("BLITZSAVE6", 0),
    ] {
        let got = walk(seed, level);
        eprintln!("{seed} A{level}: {got:?}");
        all.fight |= got.fight;
        all.merchant |= got.merchant;
        all.chest |= got.chest;
        if all.fight && all.merchant && all.chest && level > 0 {
            break;
        }
    }
    assert!(
        all.fight && all.merchant && all.chest,
        "not every position was reached: {all:?}"
    );
}
