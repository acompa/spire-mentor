//! GH #1347: Blitz mode in the workbench.
//!
//! A live run can be started in Blitz (v0.113.0: a fixed 8-node line per
//! act, its own Neow, room rewards, merchant and chest, and an ascension
//! ladder of 0..=5). Every Blitz decision is a state the SPA can draw and an
//! action the engine takes; the mode travels with the run through every
//! rebuild path; and a standard run's payloads never name it.
#![cfg(workbench_game_data_v0_113_0)]

use serde_json::{json, Value};
use sts2_core::{GameMode, GameVersion};
use sts2_workbench::api::{Config, Workbench};
use sts2_workbench::assets::AssetResolver;
use sts2_workbench::enrich::EnrichCtx;
use sts2_workbench::game::Game;
use sts2_workbench::localization::Loc;
use sts2_workbench::step::live_step;

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
    let mut body = json!({"character": "Ironclad", "seed": "BLITZ", "act1": "overgrowth",
                          "game_version": "v0.113.0", "game_mode": "blitz"});
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

/// The first `n` card indices of a select, as the wire spells them.
fn first_indices(n: i64) -> String {
    (0..n.max(1))
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// One action over the API: the first enabled option, or the cards a select
/// asks for.
fn advance(wb: &mut Workbench, mut step: Value, steps: usize) -> Value {
    for _ in 0..steps {
        let state = &step["state"];
        let (action, args) = if step["decision"] == "card_select" {
            let n = state["min_select"].as_i64().unwrap_or(1);
            (
                "select_cards".to_string(),
                json!({"indices": first_indices(n)}),
            )
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
fn a_blitz_start_names_its_mode_and_plays_its_level() {
    let mut wb = new_wb();
    let step = call(
        &mut wb,
        "POST",
        "/api/live/start",
        start_body(json!({"ascension": 3})),
    );
    let state = step["state"].as_object().expect("state");
    assert_eq!(state["game_mode"], json!("blitz"));
    // Blitz has no dominion: the key is Dread's alone.
    assert!(!state.contains_key("dominion"));
    assert!(!state.contains_key("room_reveal"));
    // The flame shows the Blitz level, not a standard ascension.
    assert_eq!(state["ascension"], json!(3));
    let (mode, ascension, seed) = live_run(&wb);
    assert_eq!(mode, GameMode::Blitz);
    assert_eq!(ascension, 3);
    // The game reads a seed's I as 1.
    assert_eq!(seed, GameVersion::V0_113_0.run_seed_hash("BL1TZ"));
    let sess = wb.session().expect("session");
    assert_eq!(sess.game_mode, GameMode::Blitz);
    assert_eq!(sess.ascension, 3);

    // Neow's one Blitz option, then exactly three of ten cards.
    assert_eq!(step["decision"], json!("event_choice"));
    assert_eq!(state["event_options"].as_array().map(Vec::len), Some(1));
    assert_eq!(state["options"].as_array().map(Vec::len), Some(1));
    let pick = advance(&mut wb, step, 1);
    assert_eq!(pick["decision"], json!("card_select"));
    let s = &pick["state"];
    assert_eq!(s["cards"].as_array().map(Vec::len), Some(10));
    assert_eq!(s["min_select"], json!(3));
    assert_eq!(s["max_select"], json!(3));
    assert_eq!(s["context"], json!("grant"));
    let deck = s["player"]["deck"].as_array().map_or(0, Vec::len);
    // The map is the fixed line: one node to go to.
    let map = advance(&mut wb, pick, 1);
    assert_eq!(map["decision"], json!("map_select"));
    let s = &map["state"];
    assert_eq!(s["player"]["deck"].as_array().map(Vec::len), Some(deck + 3));
    assert_eq!(
        s["choices"],
        json!([{"col": 3, "row": 1, "type": "Monster",
                "icon": s["choices"][0]["icon"].clone()}])
    );
    let full = call(&mut wb, "GET", "/api/live/map", Value::Null);
    let rows = full["graphs"][0]["full_map"]["rows"]
        .as_array()
        .expect("rows");
    assert_eq!(rows.len(), 7, "the Ancient, then the six rooms");
    assert!(rows.iter().all(|r| r.as_array().map(Vec::len) == Some(1)));
}

#[test]
fn a_blitz_start_the_engine_cannot_build_is_a_400() {
    let mut wb = new_wb();
    let cases = [
        (
            "/api/live/start",
            start_body(json!({"game_version": "v0.111.0"})),
            "v0.113.0",
        ),
        (
            "/api/live/start",
            json!({"seed": "X", "game_mode": "blitz", "game_version": "v0.107.1"}),
            "Blitz mode needs game build v0.113.0",
        ),
        (
            "/api/live/start",
            start_body(json!({"ascension": 6})),
            "ascension is the Blitz level 0..=5 (got 6)",
        ),
        (
            "/api/live/start",
            start_body(json!({"ascension": 10})),
            "ascension is the Blitz level 0..=5 (got 10)",
        ),
        (
            "/api/live/start",
            start_body(json!({"backend": "csharp"})),
            "rust engine",
        ),
        (
            "/api/live/start",
            start_body(json!({"game_mode": "nightmare"})),
            "\"standard\", \"dread\" or \"blitz\"",
        ),
        (
            "/api/live/start-faithful",
            json!({"seed": "run_1", "game_mode": "blitz", "game_version": "v0.113.0"}),
            "Blitz mode is not available for a run-mode run",
        ),
        (
            "/api/live/import-state",
            json!({"payload": "{}", "game_mode": "blitz", "game_version": "v0.113.0"}),
            "Blitz mode is not available for an imported run",
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

/// The advisors, the run policy and the autoplayers read the raw ascension
/// and know neither the Blitz merchant nor its chest: every AI route is a
/// 409 in a Blitz run, and still answers in a standard one.
#[test]
fn the_ai_routes_are_refused_in_a_blitz_run() {
    let mut wb = new_wb();
    call(&mut wb, "POST", "/api/live/start", start_body(json!({})));
    for route in [
        "/api/live/hint",
        "/api/live/ppo-policy",
        "/api/live/choice-ranker",
        "/api/live/auto-action",
        "/api/live/play-to-end",
        "/api/live/autobattle/advance",
        "/api/live/run-mode",
        "/api/live/decision-advisor",
    ] {
        let e = wb.call("POST", route, &json!({})).unwrap_err();
        assert_eq!(e.status, 409, "{route}: {}", e.detail);
        assert!(e.detail.contains("Blitz"), "{route}: {}", e.detail);
    }
    let mut frames = Vec::new();
    wb.call_stream(
        "POST",
        "/api/live/decision-advisor/stream",
        &json!({}),
        &mut |f| frames.push(f.clone()),
    );
    assert_eq!(frames.len(), 1);
    assert!(frames[0]["error"].as_str().unwrap().contains("Blitz"));
    // The run itself still answers.
    call(&mut wb, "GET", "/api/live/state", Value::Null);
    call(&mut wb, "GET", "/api/live/timeline", Value::Null);

    // A standard run on the same build is not refused for its mode.
    let mut body = start_body(json!({}));
    body.as_object_mut().unwrap().remove("game_mode");
    call(&mut wb, "POST", "/api/live/start", body);
    if let Err(e) = wb.call("POST", "/api/live/choice-ranker", &json!({})) {
        assert!(!e.detail.contains("Blitz"), "{}", e.detail);
    }
}

#[test]
fn save_persist_and_run_log_keep_a_blitz_run() {
    let mut wb = new_wb();
    let step = call(
        &mut wb,
        "POST",
        "/api/live/start",
        start_body(json!({"ascension": 5})),
    );
    // Neow, the three cards, the first node.
    let step = advance(&mut wb, step, 3);
    assert_eq!(step["decision"], json!("combat_play"));
    assert_eq!(step["state"]["game_mode"], json!("blitz"));
    let want = live_run(&wb);
    assert_eq!((want.0, want.1), (GameMode::Blitz, 5));

    // Seek to the root and back rebuilds a Blitz run.
    let tl = call(&mut wb, "GET", "/api/live/timeline", Value::Null);
    let leaf = tl["active_leaf"].clone();
    let root = call(&mut wb, "POST", "/api/live/seek", json!({"node_id": 0}));
    assert_eq!(root["step"]["state"]["game_mode"], json!("blitz"));
    assert_eq!(live_run(&wb), want);
    let back = call(&mut wb, "POST", "/api/live/seek", json!({"node_id": leaf}));
    assert_eq!(back["step"]["state"], step["state"]);

    // Save -> restore.
    let save = wb
        .build_save("blitz", "2026-11-15T00:00:00Z")
        .expect("save");
    assert_eq!(save["game_mode"], json!("blitz"));
    assert_eq!(save["ascension"], json!(5));
    let mut wb2 = new_wb();
    let restored = wb2
        .restore_save(&save, sts2_workbench::runlog::DEFAULT_NOW_ISO)
        .expect("restore");
    assert_eq!(live_run(&wb2), want);
    assert_eq!(restored["state"], step["state"]);

    // Persist -> import.
    let persist = wb.export_persist();
    assert_eq!(persist["game_mode"], json!("blitz"));
    let mut wb3 = new_wb();
    let imported = wb3.import_persist(&persist, |_, _| {}).expect("reload");
    assert_eq!(live_run(&wb3), want);
    assert_eq!(imported["state"], step["state"]);

    // Run log meta -> the replay viewer's states -> "play from here".
    call(&mut wb, "POST", "/api/live/quit", Value::Null);
    let log = wb.take_run_log().expect("log");
    let meta: Value = serde_json::from_str(log.text.lines().next().unwrap()).unwrap();
    assert_eq!(meta["data"]["game_mode"], json!("blitz"));
    let mut wb4 = new_wb();
    wb4.load_run(&log.filename, &log.text).expect("load");
    let recorded = call(&mut wb4, "GET", "/api/step/1", Value::Null);
    assert_eq!(recorded["state"]["game_mode"], json!("blitz"));
    assert_eq!(recorded["decision"], json!("card_select"));
    call(&mut wb4, "POST", "/api/live/from_replay", json!({"idx": 2}));
    assert_eq!(live_run(&wb4), want);

    // An unknown mode is refused, not replayed as a standard run...
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
    // ...and so are a Blitz payload naming a build without Blitz and one
    // naming a level off the Blitz ladder: errors, not the engine's panic.
    let mut bad = save.clone();
    bad["game_version"] = json!("v0.111.0");
    assert!(new_wb()
        .restore_save(&bad, sts2_workbench::runlog::DEFAULT_NOW_ISO)
        .is_err_and(|e| e.status == 409));
    let mut bad = save.clone();
    bad["ascension"] = json!(9);
    assert!(new_wb()
        .restore_save(&bad, sts2_workbench::runlog::DEFAULT_NOW_ISO)
        .is_err());
}

// ── a whole run, decision by decision ───────────────────────────────────────

/// What one walk met.
#[derive(Default)]
struct Seen {
    decisions: Vec<String>,
    cancelled_a_pick: bool,
    took_a_cursed_ware: bool,
    kinds: std::collections::BTreeSet<String>,
}

fn act(game: &mut Game, action: &str, args: Value) -> Value {
    let state = game
        .act(action, &args)
        .unwrap_or_else(|e| panic!("{action} {args}: {}", e.0));
    assert!(
        state.get("error").is_none_or(Value::is_null),
        "{action} {args}: {}",
        state["error"]
    );
    state
}

/// The enriched live step the SPA is sent for `state`.
fn view(state: &Value) -> Value {
    let ctx = EnrichCtx {
        assets: AssetResolver::embedded_for(GameVersion::V0_113_0),
        loc: Loc::embedded_for(GameVersion::V0_113_0),
        hp_ranges: None,
        assets_base: "https://assets.test",
    };
    live_step(state.clone(), &ctx)
}

/// The Blitz merchant's wares of `state`, as the engine lists them.
fn wares(state: &Value) -> Vec<Value> {
    state["options"]
        .as_array()
        .expect("options")
        .iter()
        .filter(|o| o["action"] == "blitz_choice")
        .cloned()
        .collect()
}

/// Resolve whatever a taken ware opened, until the merchant is back.
fn back_to_the_shop(game: &mut Game, mut state: Value) -> Value {
    for _ in 0..40 {
        match state["decision"].as_str().unwrap_or("") {
            "shop" => return state,
            "card_select" => {
                let n = state["min_select"].as_i64().unwrap_or(1);
                state = act(game, "select_cards", json!({"indices": first_indices(n)}));
            }
            "card_reward" => state = act(game, "select_card_reward", json!({"card_index": 0})),
            "event_choice" => state = act(game, "choose_option", json!({"option_index": 0})),
            other => panic!("a Blitz ware opened {other}: {state}"),
        }
    }
    panic!("the merchant never came back");
}

/// The Blitz merchant: its shape, a cancelled deck pick, every pick spent.
fn play_the_merchant(game: &mut Game, mut state: Value, seen: &mut Seen) -> Value {
    assert_eq!(state["blitz_choices_remaining"], json!(3));
    assert_eq!(state["cards"], json!([]));
    assert_eq!(state["relics"], json!([]));
    assert_eq!(state["potions"], json!([]));
    assert!(state["card_removal_cost"].is_null());
    let options = state["options"].as_array().expect("options");
    assert_eq!(options.len(), 7, "leave + six wares");
    assert_eq!(
        options[0],
        json!({"index": 0, "id": "leave_shop", "action": "leave_shop",
               "label": "leave_shop", "enabled": true})
    );
    let listed = wares(&state);
    for (i, w) in listed.iter().enumerate() {
        let kind = w["item"]["blitz_kind"].as_str().expect("kind");
        assert_eq!(w["index"], json!(i + 1));
        assert_eq!(w["label"], json!(format!("blitz:{kind}")));
        assert_eq!(w["status"], json!("Available"));
        assert_eq!(w["enabled"], json!(true));
        assert_eq!(w["price"], json!(0));
        assert_eq!(
            w["item"]["cursed"],
            json!(i == 5),
            "the cursed ware is last"
        );
        assert!(!w["item"]["title"].as_str().unwrap_or("").is_empty());
        assert!(!w["item"]["description"].as_str().unwrap_or("").is_empty());
        assert!(!w["item"]["title"].as_str().unwrap().contains('{'));
        assert!(!w["item"]["description"].as_str().unwrap().contains('{'));
        seen.kinds.insert(kind.to_string());
    }
    assert!(listed[5]["item"]["description"]
        .as_str()
        .unwrap()
        .contains("Curse"));

    // What the SPA is sent: the wares under their own key, one normalized
    // action per ware, then Leave.
    let step = view(&state);
    let shown = step["state"]["blitz_wares"].as_array().expect("wares");
    assert_eq!(shown.len(), 6);
    assert_eq!(shown[5]["cursed"], json!(true));
    assert_eq!(shown[0]["title"], listed[0]["item"]["title"]);
    let opts = step["state"]["options"].as_array().expect("options");
    assert_eq!(opts.len(), 7);
    assert_eq!(opts[0]["action"], json!("blitz_choice"));
    assert_eq!(opts[0]["args"], json!({"index": 1}));
    assert_eq!(opts[0]["label"], listed[0]["item"]["title"]);
    assert_eq!(opts[6]["action"], json!("leave_room"));

    // A ware with a deck pick can be backed out of: nothing is spent.
    let deck_pick = ["CardUpgrade", "CardTransform", "CardRemoval", "CardEnchant"];
    if let Some(w) = listed
        .iter()
        .find(|w| deck_pick.contains(&w["item"]["blitz_kind"].as_str().unwrap()))
    {
        let pick = act(game, "blitz_choice", json!({"index": w["index"]}));
        if pick["decision"] == "card_select" {
            assert_eq!(pick["can_skip"], json!(true), "{pick}");
            let skip = view(&pick)["state"]["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["action"] == "skip_select");
            assert!(skip, "the SPA is offered the cancel");
            let again = act(game, "skip_select", json!({}));
            assert_eq!(again["decision"], json!("shop"));
            assert_eq!(again["blitz_choices_remaining"], json!(3));
            assert_eq!(wares(&again), listed, "the ware is still available");
            seen.cancelled_a_pick = true;
            state = again;
        } else {
            // One or zero eligible cards: the game skips the screen.
            state = back_to_the_shop(game, pick);
        }
    }

    // Spend every pick, the cursed ware first.
    let mut order: Vec<usize> = vec![6, 1, 2, 3, 4, 5];
    let mut curses = 0;
    while state["blitz_choices_remaining"].as_i64().unwrap_or(0) > 0 {
        let now = wares(&state);
        let Some(pos) = order
            .iter()
            .position(|&i| now[i - 1]["enabled"] == json!(true))
        else {
            break;
        };
        let index = order.remove(pos);
        let before = state["blitz_choices_remaining"].as_i64().unwrap();
        let kind = now[index - 1]["item"]["blitz_kind"]
            .as_str()
            .unwrap()
            .to_string();
        let deck_before = state["player"]["deck"].as_array().map_or(0, Vec::len);
        let opened = act(game, "blitz_choice", json!({"index": index}));
        state = back_to_the_shop(game, opened);
        let after = state["blitz_choices_remaining"].as_i64().unwrap();
        let status = wares(&state)[index - 1]["status"].clone();
        if status == json!("Available") {
            // RandomUpgrades with nothing to upgrade is refused and free.
            assert_eq!(kind, "RandomUpgrades");
            assert_eq!(after, before);
            continue;
        }
        assert_eq!(status, json!("Used"), "{kind}");
        assert_eq!(wares(&state)[index - 1]["enabled"], json!(false));
        let delta = if kind == "TwoMoreChoices" { 1 } else { -1 };
        assert_eq!(after, before + delta, "{kind}");
        if index == 6 {
            seen.took_a_cursed_ware = true;
            curses += 1;
            let deck_after = state["player"]["deck"].as_array().map_or(0, Vec::len);
            assert!(deck_after > deck_before, "{kind} adds a Curse");
        }
    }
    assert_eq!(curses, 1);
    // No picks left: what was not taken is disabled, and only Leave is on.
    assert_eq!(state["blitz_choices_remaining"], json!(0));
    for w in wares(&state) {
        assert_eq!(w["enabled"], json!(false));
        assert!(w["status"] == "Used" || w["status"] == "Disabled", "{w}");
    }
    assert!(wares(&state).iter().any(|w| w["status"] == "Disabled"));
    let step = view(&state);
    let enabled: Vec<&Value> = step["state"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["enabled"] == json!(true))
        .collect();
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0]["action"], json!("leave_room"));
    assert!(game.act("blitz_choice", &json!({"index": 1})).is_err());
    act(game, "leave_room", json!({}))
}

/// The chest: two relics, taken one per round under their first indices.
fn open_the_chest(game: &mut Game, state: &Value) -> Value {
    assert_eq!(state["chest_picks_left"], json!(2));
    let rewards = state["rewards"].as_array().expect("rewards").clone();
    assert_eq!(rewards.len(), 2);
    for (i, r) in rewards.iter().enumerate() {
        assert_eq!(r["index"], json!(i));
        assert_eq!(r["type"], json!("relic"));
        // The engine names a relic by its loc key; the view resolves it.
        assert_eq!(
            r["name"],
            json!(format!("{}.title", r["id"].as_str().unwrap()))
        );
    }
    assert!(state.get("room_reveal").is_none());
    let step = view(state);
    let shown = &step["state"]["rewards"];
    assert_ne!(shown[0]["name"], rewards[0]["name"], "the name is resolved");
    assert!(!shown[0]["name"].as_str().unwrap().contains('.'));
    assert!(shown[0]["image"].is_string() || shown[0]["image"].is_null());
    let opts = step["state"]["options"].as_array().unwrap();
    assert_eq!(opts.len(), 3);
    assert_eq!(opts[1]["action"], json!("take_reward"));
    assert_eq!(opts[1]["args"], json!({"reward_index": 1}));
    assert_eq!(opts[2]["action"], json!("skip_reward"));

    let relics = |s: &Value| s["player"]["relics"].as_array().map_or(0, Vec::len);
    let gold = state["player"]["gold"].clone();
    let had = relics(state);
    // Round one: the second relic; the first keeps its index.
    let round2 = act(game, "take_reward", json!({"reward_index": 1}));
    assert_eq!(round2["decision"], json!("rewards"));
    assert_eq!(round2["chest_picks_left"], json!(1));
    assert_eq!(round2["rewards"], json!([rewards[0].clone()]));
    assert_eq!(relics(&round2), had + 1);
    let after = act(game, "take_reward", json!({"reward_index": 0}));
    assert_ne!(after["decision"], json!("rewards"));
    assert!(relics(&after) >= had + 2);
    assert_eq!(after["player"]["gold"], gold, "a Blitz chest holds no gold");
    assert!(after.get("room_reveal").is_none());
    after
}

/// Play a Blitz run to its end with fights stamped won, checking every
/// decision the workbench shows on the way.
fn walk_a_run(character: &str, seed: &str, level: u8) -> Seen {
    const LINE: [&str; 7] = [
        "Monster", "Monster", "Shop", "Elite", "Treasure", "RestSite", "Boss",
    ];
    let mut seen = Seen::default();
    let mut game = Game::new();
    game.game_version = GameVersion::V0_113_0;
    game.game_mode = GameMode::Blitz;
    let mut state = game.start(character, seed, level, "roll").expect("start");
    let gold = state["player"]["gold"].clone();
    let mut rooms: std::collections::BTreeMap<i64, Vec<String>> = std::collections::BTreeMap::new();
    let mut ancients: Vec<i64> = Vec::new();
    for _ in 0..600 {
        let decision = state["decision"].as_str().unwrap_or("").to_string();
        seen.decisions.push(decision.clone());
        if decision == "game_over" {
            assert_eq!(state["victory"], json!(true));
            assert_eq!(rooms.len(), 3);
            for (act, line) in &rooms {
                assert_eq!(line, &LINE, "act {act}");
            }
            assert_eq!(ancients, [1, 2, 3], "every act opens on its Ancient");
            return seen;
        }
        assert_eq!(state["game_mode"], json!("blitz"));
        assert_eq!(state["ascension"], json!(level));
        assert!(state.get("dominion").is_none());
        let act_no = state["act"].as_i64().expect("act");
        if decision == "event_choice" {
            let options = state["options"].as_array().map_or(0, Vec::len);
            if state["event_name"] == json!("NEOW.title") {
                assert_eq!((act_no, &state["floor"]), (1, &json!(1)));
                assert!(rooms.is_empty());
                assert_eq!(options, 1, "Neow's one Blitz option");
                assert_eq!(state["options"][0]["title"], json!("Blitz"));
                ancients.push(1);
            } else {
                // The next act's Ancient, straight after the boss with no
                // map decision between: three relics, shown (as in a
                // standard run) from the boss room it follows.
                assert_eq!(state["event_name"], json!(""));
                assert_eq!(rooms[&act_no], LINE);
                assert_eq!(options, 3);
                ancients.push(act_no + 1);
            }
        }
        state = match decision.as_str() {
            "event_choice" => act(&mut game, "choose_option", json!({"option_index": 0})),
            "card_select" => {
                let n = state["min_select"].as_i64().unwrap_or(1);
                act(
                    &mut game,
                    "select_cards",
                    json!({"indices": first_indices(n)}),
                )
            }
            "map_select" => {
                // The fixed line: exactly one node to go to, in column 3.
                let choices = state["choices"].as_array().expect("choices");
                assert_eq!(choices.len(), 1, "{state}");
                let here = rooms.entry(act_no).or_default();
                let row = i64::try_from(here.len()).expect("a short line") + 1;
                assert_eq!(choices[0]["col"], json!(3));
                assert_eq!(choices[0]["row"], json!(row));
                assert_eq!(state["floor"], json!(8 * (act_no - 1) + row));
                assert_eq!(
                    ancients.last(),
                    Some(&act_no),
                    "the act's Ancient came first"
                );
                // The boss is the map's own `boss`, not a row node: its
                // choice carries no type (as in a standard run).
                here.push(choices[0]["type"].as_str().unwrap_or("Boss").to_string());
                let map = game.get_map(None);
                let line = map["rows"].as_array().expect("rows");
                // The Ancient, then the six rooms; the boss is `boss`.
                assert_eq!(line.len(), 7);
                assert!(line.iter().all(|r| r.as_array().map(Vec::len) == Some(1)));
                assert_eq!(map["boss"]["row"], json!(7));
                act(
                    &mut game,
                    "select_map_node",
                    json!({"col": choices[0]["col"], "row": choices[0]["row"]}),
                )
            }
            "combat_play" => {
                let rm = game.native_run_mut().expect("run");
                rm.resolve_combat_fast(0.0, true);
                game.get_state().expect("state")
            }
            "card_reward" => {
                assert_eq!(state["cards"].as_array().map(Vec::len), Some(3));
                act(&mut game, "select_card_reward", json!({"card_index": 0}))
            }
            "shop" => play_the_merchant(&mut game, state, &mut seen),
            "rewards" => open_the_chest(&mut game, &state),
            "rest_site" => act(&mut game, "choose_option", json!({"option_index": 0})),
            other => panic!("unexpected decision {other}: {state}"),
        };
        // Room rewards carry no gold in Blitz.
        assert_eq!(state["player"]["gold"], gold, "after {decision}");
    }
    panic!("the run never ended: {:?}", seen.decisions);
}

#[test]
fn a_blitz_run_is_playable_decision_by_decision() {
    let mut cancelled = false;
    let mut kinds = std::collections::BTreeSet::new();
    for (character, seed, level) in [
        ("Ironclad", "BLITZWB1", 0),
        ("Silent", "BLITZWB2", 5),
        ("Defect", "BLITZWB3", 2),
        ("Regent", "BLITZWB4", 0),
        ("Necrobinder", "BLITZWB5", 4),
        ("Fool", "BLITZWB6", 1),
        ("Ironclad", "BLITZWB7", 3),
        ("Silent", "BLITZWB8", 0),
    ] {
        let seen = walk_a_run(character, seed, level);
        for d in [
            "card_select",
            "map_select",
            "card_reward",
            "shop",
            "rewards",
        ] {
            assert!(
                seen.decisions.iter().any(|x| x == d),
                "{character} {seed}: no {d} in {:?}",
                seen.decisions
            );
        }
        assert!(seen.took_a_cursed_ware, "{character} {seed}");
        cancelled |= seen.cancelled_a_pick;
        kinds.extend(seen.kinds);
    }
    assert!(cancelled, "no walk met a ware with a deck pick");
    // Twenty-four merchants across the walks: all twenty wares turn up.
    assert_eq!(kinds.len(), 20, "wares met: {kinds:?}");
}

/// A skip closes the chest with nothing more awarded, as the game's one
/// SKIP button does (`NTreasureRoom.cs:256`); `proceed` does the same.
#[test]
fn skipping_the_chest_ends_it() {
    for verb in ["skip_reward", "proceed"] {
        let mut game = Game::new();
        game.game_version = GameVersion::V0_113_0;
        game.game_mode = GameMode::Blitz;
        let mut state = game
            .start("Ironclad", "BLITZWB1", 0, "roll")
            .expect("start");
        for _ in 0..200 {
            state = match state["decision"].as_str().unwrap_or("") {
                "rewards" => break,
                "event_choice" | "rest_site" => {
                    act(&mut game, "choose_option", json!({"option_index": 0}))
                }
                "card_select" => {
                    let n = state["min_select"].as_i64().unwrap_or(1);
                    act(
                        &mut game,
                        "select_cards",
                        json!({"indices": first_indices(n)}),
                    )
                }
                "map_select" => {
                    let c = state["choices"][0].clone();
                    act(
                        &mut game,
                        "select_map_node",
                        json!({"col": c["col"], "row": c["row"]}),
                    )
                }
                "combat_play" => {
                    game.native_run_mut()
                        .expect("run")
                        .resolve_combat_fast(0.0, true);
                    game.get_state().expect("state")
                }
                "card_reward" => act(&mut game, "skip_card_reward", json!({})),
                "shop" => act(&mut game, "leave_room", json!({})),
                other => panic!("unexpected decision {other}"),
            };
        }
        assert_eq!(state["decision"], json!("rewards"));
        let had = state["player"]["relics"].as_array().map_or(0, Vec::len);
        let after = act(&mut game, verb, json!({}));
        assert_eq!(after["decision"], json!("map_select"), "{verb}");
        assert_eq!(
            after["player"]["relics"].as_array().map_or(0, Vec::len),
            had
        );
        // Outside a chest the chest verbs are errors, not a panic.
        assert!(game
            .act("take_reward", &json!({"reward_index": 0}))
            .is_err());
    }
}

// ── the same rooms through the session and the API ─────────────────────────

/// Stamp the live fight won and return the engine's next state. The win is
/// not on the session's line, so this run is never rebuilt afterwards.
fn win_the_fight(wb: &mut Workbench) -> Value {
    let game = wb
        .session_mut()
        .and_then(|s| s.game_mut())
        .expect("a live game");
    game.native_run_mut()
        .expect("run")
        .resolve_combat_fast(0.0, true);
    game.get_state().expect("state")
}

fn api_act(wb: &mut Workbench, action: &str, args: Value) -> Value {
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
    resp
}

/// Every Blitz verb passes the session's allow-list, is labelled in the
/// history, and comes back as an enriched step: the merchant with three
/// wares taken, then both rounds of the chest.
#[test]
fn the_merchant_and_the_chest_play_through_the_api() {
    let mut wb = new_wb();
    let mut step = call(
        &mut wb,
        "POST",
        "/api/live/start",
        start_body(json!({"seed": "BLITZWB1", "act1": "roll"})),
    );
    let mut labels = Vec::new();
    let (mut shopped, mut chest_done) = (false, false);
    for _ in 0..200 {
        let state = step["state"].clone();
        let decision = state["decision"].as_str().unwrap_or("").to_string();
        let resp = match decision.as_str() {
            "combat_play" => {
                let raw = win_the_fight(&mut wb);
                // The next action is chosen off the engine's own state.
                step = json!({"state": raw});
                continue;
            }
            "event_choice" | "rest_site" => {
                api_act(&mut wb, "choose_option", json!({"option_index": 0}))
            }
            "card_select" => {
                let n = state["min_select"].as_i64().unwrap_or(1);
                api_act(
                    &mut wb,
                    "select_cards",
                    json!({"indices": first_indices(n)}),
                )
            }
            "map_select" => {
                let c = state["choices"][0].clone();
                api_act(
                    &mut wb,
                    "select_map_node",
                    json!({"col": c["col"], "row": c["row"]}),
                )
            }
            "card_reward" => api_act(&mut wb, "select_card_reward", json!({"card_index": 0})),
            "shop" => {
                shopped = true;
                // After a fight the state is the engine's raw one; the wares
                // are read from the API's own view of the merchant.
                let view = call(&mut wb, "GET", "/api/live/state", Value::Null);
                let wares = view["state"]["blitz_wares"].as_array().cloned();
                let open = wares
                    .unwrap_or_default()
                    .into_iter()
                    .find(|w| w["enabled"] == json!(true));
                match open {
                    Some(w) if view["state"]["blitz_choices_remaining"].as_i64() > Some(0) => {
                        let r = api_act(&mut wb, "blitz_choice", json!({"index": w["index"]}));
                        labels.push(r["label"].as_str().unwrap_or("").to_string());
                        assert_eq!(
                            r["label"],
                            json!(format!("Take {}", w["title"].as_str().unwrap()))
                        );
                        r
                    }
                    _ => api_act(&mut wb, "leave_room", json!({})),
                }
            }
            "rewards" => {
                let view = call(&mut wb, "GET", "/api/live/state", Value::Null);
                let rewards = view["state"]["rewards"].as_array().expect("rewards");
                let relic = rewards.last().expect("a relic").clone();
                assert!(relic["image"].is_string() || relic["image"].is_null());
                let r = api_act(
                    &mut wb,
                    "take_reward",
                    json!({"reward_index": relic["index"]}),
                );
                assert_eq!(
                    r["label"],
                    json!(format!("Take {}", relic["name"].as_str().unwrap()))
                );
                chest_done = rewards.len() == 1;
                r
            }
            other => panic!("unexpected decision {other}: {state}"),
        };
        step = resp["step"].clone();
        if chest_done {
            break;
        }
    }
    assert!(shopped && chest_done);
    assert!(labels.len() >= 3, "{labels:?}");
    // The history names what was taken.
    let tl = call(&mut wb, "GET", "/api/live/timeline", Value::Null);
    let text = tl.to_string();
    for label in &labels {
        assert!(text.contains(label.as_str()), "{label} not in the timeline");
    }
}

#[test]
fn the_blitz_verbs_are_allowed() {
    for verb in ["blitz_choice", "take_reward", "skip_reward"] {
        assert!(sts2_workbench::session::ALLOWED_ACTIONS.contains(&verb));
    }
}
