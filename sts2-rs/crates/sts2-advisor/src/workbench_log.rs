//! The live advisor's workbench-schema run log (GH #1426 step 4; was
//! `realgame_workbench_log.py` + the `game_log.GameLogger` it wrote through).
//!
//! Each advised live run becomes one `{step, ts, type: meta|state|action|
//! mcts|coach, data}` JSONL in the *headless state dialect*, so the workbench
//! replay view loads it unchanged ([`sts2_workbench::runlog::ReplayStore`]).
//! `combat_action` boards go through [`crate::headless::to_headless_combat_play`]
//! plus the render-only fields the Board needs; screen boards get thin
//! per-surface mappers to the headless decision names. One `state` line per
//! *distinct* board: the mod re-requests advice when a screen repaints.
//!
//! This is the one module of the crate that touches the filesystem: the
//! advisor binary and the Python advisor both write the file from here. Time
//! comes from the host's clock ([`Hooks::clock`], local ISO-8601 without an
//! offset, `datetime.now().isoformat()`), since std has no local time.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{json, Map, Value};

use crate::headless::{character_display_name, to_headless_combat_play};
use crate::py::{self, get, truthy};

/// The host's side of the logger: its local clock and its log lines.
#[derive(Clone)]
pub struct Hooks {
    /// Local time as `datetime.now().isoformat()` prints it.
    pub clock: Arc<dyn Fn() -> String + Send + Sync>,
    pub info: Arc<dyn Fn(&str) + Send + Sync>,
    pub warn: Arc<dyn Fn(&str) + Send + Sync>,
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hooks")
    }
}

/// `run_log_name`: a run log's name as the control plane's `GET /log` serves
/// it — the bare filename with anything outside `[A-Za-z0-9._-]` replaced
/// (the bundled page's run store keys by it, and coach records carry it).
#[must_use]
pub fn run_log_name(path: Option<&str>) -> Option<String> {
    let path = path.filter(|p| !p.is_empty())?;
    let base = path.rsplit(std::path::is_separator).next().unwrap_or(path);
    Some(
        base.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect(),
    )
}

// -- the JSONL file (`game_log.GameLogger`) ---------------------------------

/// One run's JSONL file: `GameLogger` as the advisor uses it.
#[derive(Debug)]
struct GameLog {
    path: PathBuf,
    file: Option<File>,
    step: u64,
}

impl GameLog {
    /// Create `{stamp}_{character}_{seed}.jsonl` in `dir` (exclusive create,
    /// `_2`, `_3`, … on a clash: two co-op seats share seed and character)
    /// and write the self-describing `meta` header. `game_version` is the
    /// build the run is played in; `None` (a build the advisor does not
    /// know) leaves the key out.
    fn create(
        dir: &Path,
        character: &str,
        seed: &str,
        ascension: i64,
        game_version: Option<&str>,
        now: &str,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let stem = format!(
            "{}_{character}_{}",
            sts2_workbench::runlog::file_stamp(now),
            seed.replace('/', "_")
        );
        let mut n = 1u32;
        let (path, file) = loop {
            let suffix = if n == 1 {
                String::new()
            } else {
                format!("_{n}")
            };
            let path = dir.join(format!("{stem}{suffix}.jsonl"));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(f) => break (path, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => return Err(e),
            }
        };
        let mut log = Self {
            path,
            file: Some(file),
            step: 0,
        };
        let mut meta = json!({"character": character, "seed": seed, "ascension": ascension});
        if let Some(v) = game_version {
            meta["game_version"] = json!(v);
        }
        log.write("meta", meta, now)?;
        Ok(log)
    }

    fn path_str(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    fn write(&mut self, kind: &str, data: Value, now: &str) -> std::io::Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        let entry = json!({"step": self.step, "ts": now, "type": kind, "data": data});
        let mut line = serde_json::to_string(&entry).map_err(std::io::Error::other)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        file.flush()
    }

    fn log_state(&mut self, state: Value, now: &str) -> std::io::Result<()> {
        if self.file.is_none() {
            return Ok(());
        }
        self.step += 1;
        self.write("state", state, now)
    }

    fn close(&mut self) {
        self.file = None;
    }

    /// Re-open a closed log for appending; `true` when it is open again.
    fn reopen(&mut self) -> std::io::Result<bool> {
        if self.file.is_some() {
            return Ok(false);
        }
        self.file = Some(OpenOptions::new().append(true).open(&self.path)?);
        Ok(true)
    }
}

// -- the per-surface state builders -----------------------------------------

/// Envelope fields that change on every message even when the board did not.
const VOLATILE_KEYS: [&str; 5] = [
    "state_id",
    "request_id",
    "mod_version",
    "type",
    "advice_for",
];

/// `d.get(key, default)`: the default only when the key is ABSENT.
fn get_or(d: &Value, key: &str, default: Value) -> Value {
    d.as_object()
        .and_then(|m| m.get(key))
        .cloned()
        .unwrap_or(default)
}

/// `d.get(key) or []` as a list.
fn list_or_empty(d: &Value, key: &str) -> Vec<Value> {
    let v = get(d, key);
    if truthy(v) {
        py::as_list(v).to_vec()
    } else {
        Vec::new()
    }
}

fn or_value(v: &Value, default: Value) -> Value {
    if truthy(v) {
        v.clone()
    } else {
        default
    }
}

fn display_name(character: &Value) -> Value {
    character_display_name(character).map_or_else(|| or_value(character, json!("?")), |n| json!(n))
}

fn context(msg: &Value) -> Value {
    json!({
        "act": get(msg, "act"),
        "floor": get(msg, "floor"),
        "total_floor": get(msg, "floor"),
        "room_type": or_value(get(msg, "room_type"), json!("")),
    })
}

fn player(msg: &Value) -> Value {
    let rs = get(msg, "run_state");
    let deck = list_or_empty(rs, "master_deck");
    let potions: Vec<Value> = list_or_empty(rs, "potions")
        .into_iter()
        .filter(truthy)
        .collect();
    json!({
        "name": display_name(get(rs, "character")),
        "hp": get(rs, "hp"),
        "max_hp": get(rs, "max_hp"),
        "block": 0,
        "gold": get(rs, "gold"),
        "potion_slots": get(rs, "potion_slot_count"),
        "relics": or_value(get(rs, "relics"), json!([])),
        "potions": potions,
        "deck_size": deck.len(),
        "deck": deck,
    })
}

/// A wire `options[]` entry as the row `describe_action` reads: it looks for
/// `text`/`name`/`title`/`option_id`, the wire carries `label`.
fn options_row(o: &Map<String, Value>) -> Value {
    let mut row = o.clone();
    let label = py::mget(o, "label");
    if truthy(label)
        && !["text", "name", "title", "option_id"]
            .iter()
            .any(|k| o.contains_key(*k))
    {
        row.insert("title".into(), label.clone());
    }
    Value::Object(row)
}

fn option_rows(msg: &Value) -> Vec<Value> {
    list_or_empty(msg, "options")
        .iter()
        .filter_map(Value::as_object)
        .map(options_row)
        .collect()
}

fn base_state(msg: &Value, decision: &str) -> Map<String, Value> {
    let v = json!({
        "type": "decision",
        "decision": decision,
        // NOT "context": that key is the card_select PURPOSE STRING in
        // native workbench logs.
        "live_context": context(msg),
        // Top-level floor/act so the timeline's carry-forward labelling works
        // when the log starts mid-run.
        "floor": get(msg, "floor"),
        "act": get(msg, "act"),
        // WHICH act (act 1 ships two models, GH #544): every painted region
        // is keyed by this name.
        "act_name": or_value(get(msg, "act_name"), Value::Null),
        "player": player(msg),
    });
    let Value::Object(m) = v else { unreachable!() };
    m
}

/// `(k is None, k)`: `None` sorts last.
fn none_last(a: &Value, b: &Value) -> std::cmp::Ordering {
    a.is_null().cmp(&b.is_null()).then_with(|| py::cmp(a, b))
}

/// The wire map graph as the frontend's `FullMap` shape: nodes regrouped
/// into rows, with per-node `visited` / `current` flags.
fn wire_map_to_full_map(m: &Value) -> Option<Value> {
    let nodes: Vec<&Map<String, Value>> = py::list(m, "nodes")
        .iter()
        .filter_map(Value::as_object)
        .collect();
    if nodes.is_empty() {
        return None;
    }
    let coord = |d: &Map<String, Value>| (py::mget(d, "col").clone(), py::mget(d, "row").clone());
    let same = |a: &(Value, Value), b: &(Value, Value)| py::eq(&a.0, &b.0) && py::eq(&a.1, &b.1);
    let visited: Vec<(Value, Value)> = py::list(m, "visited")
        .iter()
        .filter_map(Value::as_object)
        .map(coord)
        .collect();
    // `if cur:` — an empty dict is no current node.
    let cur = get(m, "current").as_object().filter(|c| !c.is_empty());
    let cur_key = cur.map(coord);
    let mut by_row: Vec<(Value, Vec<Value>)> = Vec::new();
    for n in nodes {
        let key = coord(n);
        let mut node = json!({
            "col": key.0,
            "row": key.1,
            "type": py::mget(n, "type"),
            "children": or_value(py::mget(n, "children"), json!([])),
        });
        if visited.iter().any(|v| same(v, &key)) {
            node["visited"] = json!(true);
        }
        if cur_key.as_ref().is_some_and(|c| same(c, &key)) {
            node["current"] = json!(true);
        }
        match by_row.iter_mut().find(|(r, _)| py::eq(r, &key.1)) {
            Some((_, row)) => row.push(node),
            None => by_row.push((key.1.clone(), vec![node])),
        }
    }
    by_row.sort_by(|a, b| none_last(&a.0, &b.0));
    let rows: Vec<Value> = by_row
        .into_iter()
        .map(|(_, mut row)| {
            row.sort_by(|a, b| none_last(get(a, "col"), get(b, "col")));
            Value::Array(row)
        })
        .collect();
    let mut out = json!({"rows": rows});
    if let Some(boss) = get(m, "boss").as_object() {
        out["boss"] =
            json!({"col": py::mget(boss, "col"), "row": py::mget(boss, "row"), "type": "Boss"});
    }
    if let Some(cur) = cur {
        out["current_coord"] = json!({"col": py::mget(cur, "col"), "row": py::mget(cur, "row")});
    }
    Some(out)
}

fn map_select(msg: &Value) -> Map<String, Value> {
    let mut state = base_state(msg, "map_select");
    let choices: Vec<Value> = list_or_empty(msg, "nodes")
        .iter()
        .filter_map(Value::as_object)
        .map(|n| json!({"col": py::mget(n, "col"), "row": py::mget(n, "row"), "type": py::mget(n, "type")}))
        .collect();
    state.insert("choices".into(), Value::Array(choices));
    let map = get(msg, "map");
    if map.is_object() {
        if let Some(full) = wire_map_to_full_map(map) {
            state.insert("full_map".into(), full);
        }
    }
    state
}

fn card_reward(msg: &Value) -> Map<String, Value> {
    let mut state = base_state(msg, "card_reward");
    state.insert("cards".into(), or_value(get(msg, "cards"), json!([])));
    state.insert(
        "can_skip".into(),
        json!(truthy(&get_or(msg, "can_skip", json!(true)))),
    );
    state
}

fn copy_present(state: &mut Map<String, Value>, msg: &Value, keys: &[&str]) {
    for key in keys {
        let v = get(msg, key);
        if !v.is_null() {
            state.insert((*key).into(), v.clone());
        }
    }
}

fn card_select(msg: &Value) -> Map<String, Value> {
    let mut state = base_state(msg, "card_select");
    state.insert("cards".into(), or_value(get(msg, "cards"), json!([])));
    copy_present(
        &mut state,
        msg,
        &["min_select", "max_select", "cancelable", "prompt_key"],
    );
    state
}

fn event(msg: &Value) -> Map<String, Value> {
    let mut state = base_state(msg, "event_choice");
    let options = option_rows(msg);
    let event_id = options
        .iter()
        .map(|o| get(o, "event_id"))
        .find(|v| truthy(v))
        .cloned();
    state.insert("options".into(), Value::Array(options));
    if let Some(id) = event_id {
        state.insert("event_name".into(), id);
    }
    state
}

fn options_only(msg: &Value, decision: &str) -> Map<String, Value> {
    let mut state = base_state(msg, decision);
    state.insert("options".into(), Value::Array(option_rows(msg)));
    state
}

fn shop(msg: &Value) -> Map<String, Value> {
    let mut state = options_only(msg, "shop");
    copy_present(&mut state, msg, &["player_gold", "purge_cost"]);
    // Shelves for the workbench Shop component. Prefer the full `stock`
    // (every slot, sold-out and unaffordable included); older mod builds only
    // sent the purchasable options — fall back to those.
    let item_of = |r: &Value| -> Map<String, Value> {
        let item = get(r, "item");
        if truthy(item) {
            py::as_obj(item).clone()
        } else {
            Map::new()
        }
    };
    let mut rows: Vec<(Value, Value, bool, Map<String, Value>, Value)> = Vec::new();
    for r in list_or_empty(msg, "stock") {
        if r.is_object() && truthy(get(&r, "kind")) {
            rows.push((
                get(&r, "kind").clone(),
                get(&r, "price").clone(),
                truthy(&get_or(&r, "stocked", json!(true))),
                item_of(&r),
                r.clone(),
            ));
        }
    }
    if rows.is_empty() {
        for o in list_or_empty(msg, "options") {
            let action = get(&o, "action");
            if o.is_object()
                && ["buy_card", "buy_relic", "buy_potion", "remove_card"]
                    .iter()
                    .any(|a| action.as_str() == Some(a))
            {
                rows.push((
                    action.clone(),
                    get(&o, "price").clone(),
                    truthy(&get_or(&o, "enabled", json!(true))),
                    item_of(&o),
                    o.clone(),
                ));
            }
        }
    }
    let (mut cards, mut relics, mut potions) = (Vec::new(), Vec::new(), Vec::new());
    for (kind, price, stocked, item, raw) in rows {
        match kind.as_str() {
            Some("buy_card") => {
                // Shop.tsx shows `cost` as the GOLD price tag and `card_cost`
                // as the energy badge — the wire card's `cost` is energy.
                let mut card = item;
                let energy = card.get("cost").cloned().unwrap_or(Value::Null);
                card.insert("card_cost".into(), energy);
                card.insert("cost".into(), price);
                card.insert("index".into(), json!(cards.len()));
                card.insert("is_stocked".into(), json!(stocked));
                cards.push(Value::Object(card));
            }
            Some(k @ ("buy_relic" | "buy_potion")) => {
                let shelf = if k == "buy_relic" {
                    &mut relics
                } else {
                    &mut potions
                };
                let name = or_value(py::mget(&item, "name"), get(&raw, "label").clone());
                let mut row = item;
                row.insert("index".into(), json!(shelf.len()));
                row.insert("name".into(), name);
                row.insert("cost".into(), price);
                row.insert("is_stocked".into(), json!(stocked));
                shelf.push(Value::Object(row));
            }
            Some("remove_card") => {
                state.insert("card_removal_cost".into(), price);
            }
            _ => {}
        }
    }
    state.insert("cards".into(), Value::Array(cards));
    state.insert("relics".into(), Value::Array(relics));
    state.insert("potions".into(), Value::Array(potions));
    if !state.contains_key("card_removal_cost") {
        if let Some(purge) = state.get("purge_cost").filter(|v| !v.is_null()).cloned() {
            state.insert("card_removal_cost".into(), purge);
        }
    }
    state
}

fn card_bundle(msg: &Value) -> Map<String, Value> {
    let mut state = base_state(msg, "card_bundle");
    state.insert("bundles".into(), or_value(get(msg, "bundles"), json!([])));
    state
}

fn terminal(msg: &Value) -> Value {
    let mut state = base_state(msg, "game_over");
    state.insert("victory".into(), json!(truthy(get(msg, "victory"))));
    Value::Object(state)
}

/// The screen surfaces with a builder (`combat_action` is special-cased).
fn build_screen(msg: &Value, advice_for: &str) -> Option<Map<String, Value>> {
    Some(match advice_for {
        "map_select" => map_select(msg),
        "card_reward" => card_reward(msg),
        "card_select" => card_select(msg),
        "event" => event(msg),
        "shop" => shop(msg),
        "card_bundle" => card_bundle(msg),
        "rest_site" | "treasure" | "boss_relic" | "crystal_sphere" | "reward_screen" => {
            options_only(msg, advice_for)
        }
        _ => return None,
    })
}

/// Wire Card fields the search translator drops but the Board needs.
const RENDER_CARD_KEYS: [&str; 4] = ["description", "stats", "rarity", "target_type"];

/// Copy display-only fields from the wire cards onto the translated pile
/// (positional: the translator keeps order 1:1). Only fills gaps.
fn merge_render_fields(translated: Option<&mut Value>, originals: &Value) {
    let Some(Value::Array(translated)) = translated else {
        return;
    };
    let originals = if truthy(originals) {
        py::as_list(originals)
    } else {
        &[]
    };
    for (t, o) in translated.iter_mut().zip(originals) {
        let (Some(t), Some(o)) = (t.as_object_mut(), o.as_object()) else {
            continue;
        };
        for key in RENDER_CARD_KEYS {
            if !t.contains_key(key) {
                if let Some(v) = o.get(key).filter(|v| !v.is_null()) {
                    t.insert(key.into(), v.clone());
                }
            }
        }
    }
}

/// Make a translated `combat_play` state workbench-renderable: the player's
/// display name, deck and gold, each enemy's canonical kind (art resolution:
/// the three Decimillipede segments share one display name), and per-card
/// text fields — all grafted from the wire message, render side only.
fn decorate_combat_for_render(state: &mut Map<String, Value>, msg: &Value) {
    let rs = get(msg, "run_state");
    if let Some(Value::Object(player)) = state.get_mut("player") {
        player
            .entry("name")
            .or_insert_with(|| display_name(get(rs, "character")));
        let deck = list_or_empty(rs, "master_deck");
        let size = deck.len();
        player.entry("deck").or_insert(Value::Array(deck));
        player.entry("deck_size").or_insert(json!(size));
        player
            .entry("gold")
            .or_insert_with(|| get(rs, "gold").clone());
    }
    let wire_enemies = list_or_empty(msg, "enemies");
    if let Some(Value::Array(enemies)) = state.get_mut("enemies") {
        for (enemy, wire) in enemies.iter_mut().zip(&wire_enemies) {
            let Some(enemy) = enemy.as_object_mut() else {
                continue;
            };
            if let Some(kind) = get(wire, "id").as_str().filter(|s| !s.is_empty()) {
                enemy.insert("kind".into(), json!(kind));
            }
        }
    }
    merge_render_fields(state.get_mut("hand"), get(msg, "hand"));
    for pile in ["draw_pile", "discard_pile", "exhaust_pile", "play_pile"] {
        merge_render_fields(state.get_mut(pile), get(get(msg, pile), "cards"));
    }
    merge_render_fields(state.get_mut("master_deck"), get(rs, "master_deck"));
}

/// The advice's ranked options as `root_outcomes` rows (only rows with
/// visit counts — screen advice has none).
#[must_use]
pub fn root_outcomes(advice: &Value) -> Vec<Value> {
    list_or_empty(advice, "options")
        .iter()
        .filter(|o| o.is_object() && truthy(get(o, "visits")))
        .map(|o| {
            json!({
                "stable_idx": get_or(o, "stable_idx", get_or(o, "card_index", json!(-1))),
                "action_label": get(o, "label"),
                "target_index": get(o, "target_index"),
                "q": get_or(o, "q", json!(0.0)),
                "visits": get_or(o, "visits", json!(0)),
                "wins": get_or(o, "wins", json!(0)),
                "losses": get_or(o, "losses", json!(0)),
                "ties": get_or(o, "ties", json!(0)),
                "is_best": truthy(get(o, "is_best")),
                "win_prob": get(o, "p_win"),
                "mean_final_hp": get(o, "mean_final_hp"),
                "hp_sd": get(o, "hp_sd"),
                "hp_n": get_or(o, "hp_n", json!(0)),
            })
        })
        .collect()
}

/// `str(raw or "").strip().upper()`.
fn norm_id(raw: &Value) -> String {
    if truthy(raw) {
        py::str_of(raw).trim().to_uppercase()
    } else {
        String::new()
    }
}

/// `int(v) if isinstance(v, int) else None` (a bool is an int).
fn py_int(v: &Value) -> Value {
    match v {
        Value::Bool(b) => json!(i64::from(*b)),
        Value::Number(n) if n.is_i64() || n.is_u64() => v.clone(),
        _ => Value::Null,
    }
}

/// `human_action_args`: the native action dict a `human_action` stands for,
/// or `{}`. Combat moves come out in the engine's frame against `state` (the
/// logged headless decision state): a card found in its hand by identity (id
/// + upgraded), a potion's `slot` its position among the HELD potions.
/// Anything unresolvable is `{}`, never a guessed index. Non-combat choices
/// are `{"index": i | None}` (`None` = skip).
#[must_use]
pub fn human_action_args(msg: &Value, state: &Value) -> Value {
    if get(msg, "decision_type").as_str() != Some("combat_action") {
        let choice = get(msg, "choice");
        return if choice.is_object() {
            json!({"index": py_int(get(choice, "index"))})
        } else {
            json!({})
        };
    }
    let action = get(msg, "action");
    let target = py_int(get(msg, "target_index"));
    if action.as_str() == Some("end_turn") {
        return json!({"action": "end_turn"});
    }
    let card = get(msg, "card");
    if card.is_object() && truthy(get(card, "id")) {
        let want = (norm_id(get(card, "id")), truthy(get(card, "upgraded")));
        for (i, c) in list_or_empty(state, "hand").iter().enumerate() {
            if c.is_object() && (norm_id(get(c, "id")), truthy(get(c, "upgraded"))) == want {
                return json!({"action": "play", "card_index": i, "target_index": target});
            }
        }
        return json!({});
    }
    let drink = get(msg, "potion");
    if drink.is_object() && truthy(get(drink, "id")) {
        let player = get(state, "player");
        let belt = {
            let from_player = get(player, "potions");
            if truthy(from_player) {
                py::as_list(from_player).to_vec()
            } else {
                list_or_empty(state, "potions")
            }
        };
        let want = norm_id(get(drink, "id"));
        let held = belt
            .iter()
            .filter(|p| p.is_object() && !truthy(get(p, "empty")) && truthy(get(p, "id")));
        for (slot, p) in held.enumerate() {
            if norm_id(get(p, "id")) == want {
                return json!({"action": "potion", "slot": slot, "target_index": target});
            }
        }
    }
    json!({})
}

/// `int(v)` as Python reads it, `None` where Python raises.
fn py_int_strict(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        #[allow(clippy::cast_possible_truncation)]
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

// -- the logger ---------------------------------------------------------------

#[derive(Debug, Default)]
struct Inner {
    log: Option<GameLog>,
    run_key: Option<(String, String)>,
    last_sig: Option<String>,
    /// The most recent logged decision state, which a human action's machine
    /// form is resolved against.
    last_state: Option<Value>,
    /// `state` lines written to the open file: the next one's replay step.
    state_count: usize,
    /// The log closed last: still named by `log_name`, and the death
    /// autopsy (built after the run end closed it) is appended to it.
    closed_log: Option<GameLog>,
    /// Combat-log rows of decisions that wrote no `state` line: they ride on
    /// the next one, so a fight's rows stay complete.
    pending_rows: Vec<Value>,
    /// The connection dropped since the last `state` line (GH #989).
    reconnected: bool,
}

impl Inner {
    fn close(&mut self) {
        if let Some(mut log) = self.log.take() {
            log.close();
            self.closed_log = Some(log);
        }
        self.run_key = None;
        self.last_sig = None;
        self.last_state = None;
        self.state_count = 0;
    }
}

/// One workbench-schema JSONL per live run, opened lazily. A run is
/// `(character, seed)` from the envelope; a change (or a terminal message)
/// closes the current file and the next decision opens a fresh one. Never
/// fails into the advisor loop: errors go to [`Hooks::warn`]. Thread-safe —
/// the coach's grader thread writes `coach` records while the advisor loop
/// writes everything else.
#[derive(Debug)]
pub struct WorkbenchRunLogger {
    dir: Option<PathBuf>,
    hooks: Hooks,
    inner: Mutex<Inner>,
}

impl WorkbenchRunLogger {
    /// `log_dir` `None` (or empty) disables the whole logger.
    #[must_use]
    pub fn new(log_dir: Option<&str>, hooks: Hooks) -> Self {
        Self {
            dir: log_dir.filter(|d| !d.is_empty()).map(PathBuf::from),
            hooks,
            inner: Mutex::new(Inner::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn now(&self) -> String {
        (self.hooks.clock)()
    }

    fn warn(&self, what: &str, err: &dyn std::fmt::Display) {
        (self.hooks.warn)(&format!("workbench {what} failed: {err}"));
    }

    /// The open run log's path.
    #[must_use]
    pub fn path(&self) -> Option<String> {
        self.lock().log.as_ref().map(GameLog::path_str)
    }

    /// The open run log's name as `GET /log` serves it, else the name of the
    /// file closed last.
    #[must_use]
    pub fn log_name(&self) -> Option<String> {
        let t = self.lock();
        let path = t
            .log
            .as_ref()
            .or(t.closed_log.as_ref())
            .map(GameLog::path_str);
        run_log_name(path.as_deref())
    }

    pub fn close(&self) {
        self.lock().close();
    }

    /// The connection to the mod dropped: the next logged board says so.
    pub fn note_reconnect(&self) {
        self.lock().reconnected = true;
    }

    fn run_key_of(msg: &Value) -> Option<(String, String)> {
        let character = get(get(msg, "run_state"), "character");
        let seed = get(msg, "seed");
        (truthy(character) && truthy(seed)).then(|| (py::str_of(character), py::str_of(seed)))
    }

    /// The open log for `msg`'s run, opening a fresh one on a new run; `false`
    /// when there is none to write into.
    fn ensure_open(&self, t: &mut Inner, msg: &Value) -> bool {
        let Some(dir) = &self.dir else {
            return false;
        };
        let Some(key) = Self::run_key_of(msg) else {
            return t.log.is_some(); // keep logging into the current run, if any
        };
        if t.log.is_some() && t.run_key.as_ref() == Some(&key) {
            return true;
        }
        t.close();
        let character =
            character_display_name(&json!(key.0)).map_or_else(|| key.0.clone(), str::to_string);
        let rs = get(msg, "run_state");
        let ascension_raw = get(rs, "ascension");
        let ascension = if truthy(ascension_raw) {
            py_int_strict(ascension_raw)
        } else {
            Some(0)
        };
        let opened = ascension
            .ok_or_else(|| {
                std::io::Error::other(format!(
                    "invalid literal for int(): {}",
                    py::repr(ascension_raw)
                ))
            })
            .and_then(|asc| {
                let now = self.now();
                // The replay reads the build from the header alone, and a
                // header without one is v0.107.1: a v0.113 run replayed
                // there lost the Fool's art (overlay never consulted) and
                // its cards' text. A board with no `game_version` is v0.107.1.
                let version = match crate::headless::game_version(msg) {
                    Ok(v) => {
                        Some(v.unwrap_or(sts2_core::game_version::GameVersion::V0_107_1.as_str()))
                    }
                    Err(_) => None,
                };
                let mut log = GameLog::create(dir, &character, &key.1, asc, version, &now)?;
                log.write(
                    "meta",
                    json!({"source": "realgame_advisor", "live": true}),
                    &now,
                )?;
                Ok(log)
            });
        match opened {
            Ok(log) => {
                (self.hooks.info)(&format!("workbench run log: {}", log.path_str()));
                t.log = Some(log);
                t.run_key = Some(key);
                t.last_sig = None;
                t.state_count = 0;
                true
            }
            Err(e) => {
                (self.hooks.warn)(&format!("workbench log open failed: {e}"));
                t.log = None;
                false
            }
        }
    }

    /// The board's identity: the envelope minus its volatile fields, in a
    /// canonical form (`serde_json` maps are sorted).
    fn signature(msg: &Value) -> String {
        let core: Map<String, Value> = py::as_obj(msg)
            .iter()
            .filter(|(k, _)| !VOLATILE_KEYS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        serde_json::to_string(&core).unwrap_or_default()
    }

    /// Append the decision's `state` record. Returns the new step's replay
    /// index (what the workbench gives this `state` line), or `None` when
    /// nothing was written (repeated board, unknown surface, logging off).
    pub fn log_decision(&self, msg: &Value, advice_for: &str) -> Option<usize> {
        let mut guard = self.lock();
        let t = &mut *guard;
        if !self.ensure_open(t, msg) {
            return None;
        }
        let rows_of = |m: &Value| list_or_empty(m, "combat_log");
        let sig = Self::signature(msg);
        if t.last_sig.as_ref() == Some(&sig) {
            t.pending_rows.extend(rows_of(msg));
            return None;
        }
        let mut state = if advice_for == "combat_action" {
            let mut board = py::as_obj(msg).clone();
            board.insert("type".into(), json!("combat_action"));
            let mut state = match to_headless_combat_play(&Value::Object(board)) {
                Ok(Value::Object(s)) => s,
                Ok(_) => return None,
                Err(e) => {
                    self.warn("log_decision", &e);
                    return None;
                }
            };
            state.insert("floor".into(), get(msg, "floor").clone());
            state.insert("act".into(), get(msg, "act").clone());
            state.insert(
                "act_name".into(),
                or_value(get(msg, "act_name"), Value::Null),
            );
            decorate_combat_for_render(&mut state, msg);
            state
        } else if let Some(state) = build_screen(msg, advice_for) {
            state
        } else {
            t.pending_rows.extend(rows_of(msg));
            return None;
        };
        // The game's own combat-log rows for this decision, stored RAW (the
        // workbench server localizes them when it serves the step), plus the
        // character id the rows that name no creature need.
        let mut rows = std::mem::take(&mut t.pending_rows);
        rows.extend(rows_of(msg));
        if !rows.is_empty() {
            state.insert("combat_log_raw".into(), Value::Array(rows));
        }
        if t.reconnected {
            state.insert("after_reconnect".into(), json!(true));
            t.reconnected = false;
        }
        let character = get(get(msg, "run_state"), "character");
        if truthy(character) {
            state.insert("character_id".into(), character.clone());
        }
        let state = Value::Object(state);
        let now = self.now();
        let log = t.log.as_mut()?;
        if let Err(e) = log.log_state(state.clone(), &now) {
            self.warn("log_decision", &e);
            return None;
        }
        t.last_sig = Some(sig);
        t.last_state = Some(state);
        t.state_count += 1;
        Some(t.state_count - 1)
    }

    /// Attach the advice to the current step as its `mcts` record.
    pub fn log_advice(&self, msg: &Value, advice: &Value, elapsed_s: f64, tree: &Value) {
        let mut t = self.lock();
        let Some(log) = t.log.as_mut() else {
            return;
        };
        let mut data = json!({
            "request_id": get(msg, "request_id"),
            "state_id": get(msg, "state_id"),
            "elapsed_s": py::round(elapsed_s, 2),
            "advice": advice,
        });
        let outcomes = root_outcomes(advice);
        if !outcomes.is_empty() {
            data["root_outcomes"] = Value::Array(outcomes);
        }
        if truthy(get(advice, "pv")) {
            data["pv"] = get(advice, "pv").clone();
        }
        if truthy(tree) {
            data["tree"] = tree.clone();
        }
        if let Err(e) = log.write("mcts", data, &self.now()) {
            self.warn("log_advice", &e);
        }
    }

    /// Append the human's move as the current step's `action` record, with
    /// its machine form ([`human_action_args`]).
    pub fn log_human_action(&self, msg: &Value) {
        let mut guard = self.lock();
        let t = &mut *guard;
        let Some(log) = t.log.as_mut() else {
            return;
        };
        let action = get(msg, "action");
        let detail = get(msg, "detail");
        let label = if truthy(detail) {
            detail.clone()
        } else {
            or_value(action, json!("?"))
        };
        let data = json!({
            "cmd": "action",
            "action": or_value(action, json!("human")),
            "label": label,
            "args": human_action_args(msg, t.last_state.as_ref().unwrap_or(&Value::Null)),
            "decision_type": get(msg, "decision_type"),
            "advice_request_id": get(msg, "advice_request_id"),
            "position_id": get(msg, "position_id"),
            "peeked": truthy(get(msg, "peeked")),
        });
        if let Err(e) = log.write("action", data, &self.now()) {
            self.warn("log_human_action", &e);
        }
    }

    /// Append one coach record. The death autopsy (and a withdrawal racing
    /// it) is built after the run end closed the file: those go into the
    /// file closed last, re-opened for the one line.
    pub fn log_coach(&self, record: &Value) {
        let mut guard = self.lock();
        let t = &mut *guard;
        let now = self.now();
        let res = if let Some(log) = t.log.as_mut() {
            log.write("coach", record.clone(), &now)
        } else {
            let late = matches!(get(record, "kind").as_str(), Some("autopsy" | "withdraw"));
            match t.closed_log.as_mut() {
                Some(closed) if late => closed.reopen().and_then(|open| {
                    if !open {
                        return Ok(());
                    }
                    let res = closed.write("coach", record.clone(), &now);
                    closed.close();
                    res
                }),
                _ => Ok(()),
            }
        };
        if let Err(e) = res {
            self.warn("log_coach", &e);
        }
    }

    /// Terminal `game_over` / `run_complete`: final state + summary, then
    /// the file closes.
    pub fn log_run_end(&self, msg: &Value) {
        let mut guard = self.lock();
        let t = &mut *guard;
        if let Some(log) = t.log.as_mut() {
            let now = self.now();
            let rs = get(msg, "run_state");
            let res = log.log_state(terminal(msg), &now).and_then(|()| {
                log.write(
                    "meta",
                    json!({
                        "summary": true,
                        "final_won": truthy(get(msg, "victory")),
                        "final_turn": null,
                        "final_player_hp": get(rs, "hp"),
                        "final_player_max_hp": get(rs, "max_hp"),
                        "final_floor": get(msg, "floor"),
                    }),
                    &self.now(),
                )
            });
            if let Err(e) = res {
                self.warn("log_run_end", &e);
            }
        }
        t.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hooks() -> Hooks {
        Hooks {
            clock: Arc::new(|| "2026-11-17T10:20:30.123456".to_string()),
            info: Arc::new(|_| {}),
            warn: Arc::new(|w| panic!("{w}")),
        }
    }

    fn lines(path: &str) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn a_repeated_board_carries_its_rows_to_the_next_state() {
        let dir = std::env::temp_dir().join(format!("wblog-{}", std::process::id()));
        let log = WorkbenchRunLogger::new(dir.to_str(), hooks());
        let msg = json!({
            "seed": "S1", "floor": 3, "run_state": {"character": "IRONCLAD"},
            "options": [{"label": "Rest", "index": 0}], "combat_log": [{"r": 1}],
        });
        assert_eq!(log.log_decision(&msg, "rest_site"), Some(0));
        let again = json!({"seed": "S1", "floor": 3, "run_state": {"character": "IRONCLAD"},
            "options": [{"label": "Rest", "index": 0}], "combat_log": [{"r": 2}],
            "request_id": "other"});
        // Only `combat_log` differs from the first board... which is part of
        // the signature, so this one is new.
        assert_eq!(log.log_decision(&again, "rest_site"), Some(1));
        assert_eq!(log.log_decision(&again, "rest_site"), None);
        let next = json!({"seed": "S1", "floor": 4, "run_state": {"character": "IRONCLAD"},
            "options": [], "combat_log": [{"r": 3}]});
        assert_eq!(log.log_decision(&next, "treasure"), Some(2));
        let path = log.path().unwrap();
        assert!(
            path.ends_with("20261117_102030_Ironclad_S1.jsonl"),
            "{path}"
        );
        let recs = lines(&path);
        assert_eq!(recs[0]["type"], "meta");
        assert_eq!(
            recs[1]["data"],
            json!({"source": "realgame_advisor", "live": true})
        );
        assert_eq!(recs[2]["data"]["options"][0]["title"], "Rest");
        assert_eq!(
            recs[4]["data"]["combat_log_raw"],
            json!([{"r": 2}, {"r": 3}])
        );
        assert_eq!(recs[4]["step"], 3);
        log.log_run_end(&json!({"victory": true}));
        assert_eq!(log.path(), None);
        assert_eq!(
            log.log_name().as_deref(),
            Some("20261117_102030_Ironclad_S1.jsonl")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_header_names_the_build_the_run_is_played_in() {
        // The replay picks its build (tables, the art overlay) from the
        // header alone; without it a v0.113 run replayed as v0.107.1 and
        // the Fool had no art.
        let dir = std::env::temp_dir().join(format!("wblog-ver-{}", std::process::id()));
        for (envelope, want) in [
            (json!("v0.113.1"), json!("v0.113.0")),
            (Value::Null, json!("v0.107.1")),
        ] {
            let log = WorkbenchRunLogger::new(dir.to_str(), hooks());
            let mut msg = json!({
                "seed": "S1", "floor": 3, "run_state": {"character": "FOOL"},
                "options": [{"label": "Rest", "index": 0}],
            });
            if !envelope.is_null() {
                msg["game_version"] = envelope;
            }
            assert_eq!(log.log_decision(&msg, "rest_site"), Some(0));
            let recs = lines(&log.path().unwrap());
            assert_eq!(recs[0]["type"], "meta");
            assert_eq!(recs[0]["data"]["game_version"], want);
            log.close();
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_human_card_play_resolves_by_identity() {
        let state = json!({"hand": [{"id": "STRIKE_R"}, {"id": "bash", "upgraded": true}]});
        let msg = json!({"decision_type": "combat_action", "card": {"id": "BASH", "upgraded": 1}, "target_index": true});
        assert_eq!(
            human_action_args(&msg, &state),
            json!({"action": "play", "card_index": 1, "target_index": 1})
        );
        let skip = json!({"decision_type": "card_reward", "choice": {"index": 2.0}});
        assert_eq!(human_action_args(&skip, &state), json!({"index": null}));
    }

    #[test]
    fn a_run_log_name_keeps_only_safe_characters() {
        assert_eq!(
            run_log_name(Some("/x/a b+é.jsonl")).as_deref(),
            Some("a_b__.jsonl")
        );
        assert_eq!(run_log_name(Some("")), None);
    }
}
