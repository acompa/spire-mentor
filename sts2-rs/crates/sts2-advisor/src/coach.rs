//! The live coach's pure helpers (GH #1426 step 3; were in
//! `realgame_coach.py`): matching a played move to its regret row and
//! topping it up, the engine action a `human_action` stands for, and the
//! `recap` / `autopsy` building blocks (a turn's starting position, what a
//! line did on the board, the costliest decisions).
//!
//! The grader itself (`CoachGrader`: the advice cache, the per-turn state
//! machine, the background executor) stays in Python until the main loop
//! moves (step 4).
//!
//! These follow the LIVE coach, not the browser coach's twins in
//! `sts2_workbench::advisors`: an AnyAlly play matches its teammate's row
//! (GH #1017), and every flag is read with Python truthiness.

use serde_json::{json, Map, Value};
use sts2_workbench::advisors::{action_stable_key, PLAYED_MIN_VISITS};
use sts2_workbench::enemy_labels::enemy_labels;

use crate::i18n::{is_zh, join_list, tr, trf};
use crate::py;

/// How many of a fight's costliest turns its recap shows.
pub const RECAP_COSTLIEST: usize = 2;
/// How many decisions the death autopsy shows.
pub const AUTOPSY_ITEMS: usize = 3;

const END_TURN: i64 = -1;
const POTION_BASE: i64 = -100;
/// Powers whose removal is an enemy waking up (C# `AsleepPower` /
/// `SlumberPower`: unblocked damage strips both and ends the free turns).
const SLEEP_POWERS: [&str; 2] = ["ASLEEP", "SLUMBER"];

/// Python's `isinstance(v, int) and not isinstance(v, bool)`, as the value.
fn int_or_none(v: &Value) -> Option<i64> {
    v.as_i64()
}

/// `float(v)` for a JSON number (`True` is 1.0); anything else is 0.0.
fn float_of(v: &Value) -> f64 {
    match v {
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// `str(raw or "").strip().upper()`.
fn norm(raw: &Value) -> String {
    if py::truthy(raw) {
        py::str_of(raw).trim().to_uppercase()
    } else {
        String::new()
    }
}

/// `str(d.get(a) or d.get(b) or fallback)`.
fn name_or(d: &Value, keys: &[&str], fallback: &str) -> String {
    keys.iter()
        .map(|k| py::get(d, k))
        .find(|v| py::truthy(v))
        .map_or_else(|| fallback.to_string(), py::str_of)
}

/// `regret_review._match_row`'s exact-then-`stable_idx`-only rule, as an
/// index. Exact includes the teammate of an AnyAlly play (`ally_index`):
/// with three or four players one card has a row per teammate (GH #1017).
#[must_use]
pub fn match_row_index(rows: &[Value], native: &Value) -> Option<usize> {
    let (idx, target) = action_stable_key(native);
    let idx = Value::from(idx?);
    let target = target.map_or(Value::Null, Value::from);
    let ally = py::get(native, "ally_index");
    let same_idx = |r: &Value| py::eq(py::get(r, "stable_idx"), &idx);
    rows.iter()
        .position(|r| {
            same_idx(r)
                && py::eq(py::get(r, "target_index"), &target)
                && py::eq(py::get(r, "ally_index"), ally)
        })
        .or_else(|| rows.iter().position(same_idx))
}

/// The played move's row exists, is not the recommended one, and has fewer
/// than `PLAYED_MIN_VISITS` visits.
#[must_use]
pub fn played_needs_top_up(rows: &[Value], native: &Value) -> bool {
    match_row_index(rows, native).is_some_and(|i| {
        let row = &rows[i];
        !py::truthy(py::get(row, "is_best"))
            && py::int_of(py::get(row, "visits")) < PLAYED_MIN_VISITS
    })
}

/// The played row's index and the stats a forced-root search (`fresh`)
/// overwrites it with: `stable_idx` / `action` / `target_index` are kept
/// and the row is never crowned. `None` when no row matches.
#[must_use]
pub fn top_up_fields(rows: &[Value], native: &Value, fresh: &Value) -> Option<(usize, Value)> {
    let i = match_row_index(rows, native)?;
    let mut m = Map::new();
    m.insert("win_prob".into(), json!(float_of(py::get(fresh, "p_win"))));
    m.insert("q".into(), json!(float_of(py::get(fresh, "q"))));
    for k in ["visits", "wins", "losses", "ties", "hp_n"] {
        m.insert(k.into(), json!(py::int_of(py::get(fresh, k))));
    }
    for (k, src) in [("mean_final_hp", "mean_final_hp"), ("hp_sd", "hp_sd")] {
        let v = py::get(fresh, src);
        m.insert(
            k.into(),
            if v.is_null() {
                Value::Null
            } else {
                json!(float_of(v))
            },
        );
    }
    m.insert("is_best".into(), json!(false));
    Some((i, Value::Object(m)))
}

/// A regret row's move as the engine's action dict.
#[must_use]
pub fn native_from_row(row: &Value) -> Option<Value> {
    let idx = py::get(row, "stable_idx").as_i64()?;
    let target = py::get(row, "target_index").clone();
    let ally = py::get(row, "ally_index");
    if idx >= 0 {
        return Some(if ally.is_null() {
            json!({"action": "play", "card_index": idx, "target_index": target})
        } else {
            json!({"action": "play", "card_index": idx, "ally_index": ally})
        });
    }
    if idx == END_TURN {
        return Some(json!({"action": "end_turn"}));
    }
    if POTION_BASE - 5 < idx && idx <= POTION_BASE {
        let slot = POTION_BASE - idx;
        return Some(if ally.is_null() {
            json!({"action": "potion", "slot": slot, "target_index": target})
        } else {
            // Thrown at a teammate (GH #1017).
            json!({"action": "potion", "slot": slot, "ally_index": ally})
        });
    }
    None
}

/// The engine action a combat `human_action` (`msg`) stands for at a
/// position (`info`: `CoachEngine.position_info`). A card is located in its
/// hand by IDENTITY and a potion in its dense belt, the same rules the
/// advisor mirror follows; `None` when neither resolves.
#[must_use]
pub fn native_action(msg: &Value, info: &Value) -> Option<Value> {
    if py::get(msg, "action").as_str() == Some("end_turn") {
        return Some(json!({"action": "end_turn"}));
    }
    let target = py::get(msg, "target_index")
        .as_i64()
        .map_or(Value::Null, Value::from);
    let ally = py::get(msg, "ally_index").as_i64();
    let card = py::get(msg, "card");
    if card.is_object() && py::truthy(py::get(card, "id")) {
        let want = (
            norm(py::get(card, "id")),
            py::truthy(py::get(card, "upgraded")),
        );
        for (i, held) in py::list(info, "hand").iter().enumerate() {
            let held = py::as_list(held);
            let id = held.first().unwrap_or(&Value::Null);
            let up = held.get(1).is_some_and(py::truthy);
            if (norm(id), up) == want {
                return Some(match ally {
                    // At the teammate the human picked (GH #1017).
                    Some(a) => json!({"action": "play", "card_index": i, "ally_index": a}),
                    None => json!({"action": "play", "card_index": i, "target_index": target}),
                });
            }
        }
        return None;
    }
    let drink = py::get(msg, "potion");
    if drink.is_object() && py::truthy(py::get(drink, "id")) {
        let want = norm(py::get(drink, "id"));
        for (slot, pid) in py::list(info, "potions").iter().enumerate() {
            if py::truthy(pid) && norm(pid) == want {
                return Some(match ally {
                    Some(a) => json!({"action": "potion", "slot": slot, "ally_index": a}),
                    None => json!({"action": "potion", "slot": slot, "target_index": target}),
                });
            }
        }
    }
    None
}

/// One enemy's telegraphed move as the recap prints it: `"attack 6"`,
/// `"attack 6×2"`, else the lower-cased `intent_type`; a multi-intent move
/// joins them with `" + "`. `"unknown"` when nothing reads.
#[must_use]
pub fn intent_text(intents: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for intent in py::as_list(intents) {
        if !intent.is_object() {
            continue;
        }
        let text = if let Some(damage) = int_or_none(py::get(intent, "damage")) {
            let hits = int_or_none(py::get(intent, "hits"))
                .filter(|&h| h != 0)
                .unwrap_or(1);
            if hits > 1 {
                trf(
                    "attack {damage}×{hits}",
                    &[("damage", &damage), ("hits", &hits)],
                )
            } else {
                trf("attack {damage}", &[("damage", &damage)])
            }
        } else {
            let kind = py::get(intent, "intent_type");
            let kind = if py::truthy(kind) {
                py::str_of(kind)
            } else {
                String::new()
            };
            let kind = kind.trim();
            if kind.is_empty() {
                continue;
            }
            intent_word(&kind.to_lowercase())
        };
        if !parts.contains(&text) {
            parts.push(text);
        }
    }
    if parts.is_empty() {
        tr("unknown").into()
    } else {
        parts.join(" + ")
    }
}

/// An intent type (lowercased) in the advisor's language: the game's own
/// intent names in Chinese.
fn intent_word(kind: &str) -> String {
    if !is_zh() {
        return kind.to_string();
    }
    match kind.replace('_', "").as_str() {
        "attack" => "攻势",
        "buff" => "强化",
        "carddebuff" => "恶意",
        "deathblow" => "濒死一击",
        "debuff" | "debuffstrong" | "status" | "statuscard" => "策略",
        "defend" => "守势",
        "escape" => "懦弱",
        "heal" => "回复",
        "sleep" => "沉睡",
        "stun" => "击晕",
        "summon" => "召唤",
        "unknown" => "未知",
        _ => return kind.to_string(),
    }
    .to_string()
}

/// The position a combat turn started from, for the recap and autopsy.
/// `msg` is the turn's first combat `advice_request` (the mod's dialect);
/// `None` without one -- a turn without its position is never shown.
#[must_use]
pub fn turn_context(msg: &Value) -> Option<Value> {
    if !py::truthy(msg) {
        return None;
    }
    let player = py::get(msg, "player");
    let hand: Vec<String> = py::list(msg, "hand")
        .iter()
        .filter(|c| c.is_object())
        .map(|c| name_or(c, &["name", "id"], "?"))
        .collect();
    let roster: Vec<Value> = py::list(msg, "enemies")
        .iter()
        .filter(|e| e.is_object())
        .cloned()
        .collect();
    // Numbered by `enemy_labels` over the wire list, as the option labels
    // are (a dead entry takes no number).
    let enemies: Vec<Value> = roster
        .iter()
        .zip(enemy_labels(&roster))
        .filter(|(e, _)| e.get("is_alive").is_none_or(py::truthy))
        .map(|(e, label)| {
            json!({
                "name": label,
                "hp": int_or_none(py::get(e, "hp")),
                "intent": intent_text(py::get(e, "intents")),
            })
        })
        .collect();
    let field = |k| int_or_none(py::get(player, k));
    Some(json!({
        "hand": hand,
        "energy": field("energy"),
        "max_energy": field("max_energy"),
        "hp": field("hp"),
        "max_hp": field("max_hp"),
        "block": field("block"),
        "enemies": enemies,
    }))
}

fn asleep(enemy: &Value) -> bool {
    py::list(enemy, "powers").iter().any(|p| {
        p.is_object()
            && py::get(p, "name")
                .as_str()
                .is_some_and(|n| SLEEP_POWERS.contains(&n))
    })
}

/// What a line visibly did to the enemies between two engine boards
/// (`state_to_dict` of one turn's first position and of the position its
/// line ended the turn from): `("wakes", i)` / `("kills", i)` per encounter
/// slot, or a lone `("wins", -1)` when the line won the fight.
#[must_use]
pub fn board_events(start: &Value, end: &Value) -> Vec<(&'static str, i64)> {
    if py::get(end, "phase").as_str() == Some("Victory") {
        return vec![("wins", -1)];
    }
    let after: Vec<&Value> = py::list(end, "encounter")
        .iter()
        .filter(|e| e.is_object())
        .collect();
    let mut out = Vec::new();
    for (i, e) in py::list(start, "encounter").iter().enumerate() {
        if !e.is_object() || py::truthy(py::get(e, "is_dead")) {
            continue;
        }
        // A dict keyed by uid: the LAST entry with a uid wins.
        let uid = py::get(e, "uid");
        let Some(f) = after.iter().rev().find(|f| py::eq(py::get(f, "uid"), uid)) else {
            continue;
        };
        if py::truthy(py::get(f, "is_dead")) {
            out.push(("kills", i as i64));
        } else if asleep(e) && !asleep(f) {
            out.push(("wakes", i as i64));
        }
    }
    out
}

/// The recap's reason line: what one line did on the board that the other
/// did not, e.g. "Your line wakes Lagavulin Matriarch on turn 1" (GH #888).
/// `None` when both lines did the same.
#[must_use]
pub fn why_text(
    you: &[(String, i64)],
    ai: &[(String, i64)],
    names: &[String],
    turn: &Value,
) -> Option<String> {
    let phrase = |(kind, i): &(String, i64)| {
        if kind == "wins" {
            return tr("wins the fight").to_string();
        }
        let name = usize::try_from(*i)
            .ok()
            .and_then(|i| names.get(i).cloned())
            .unwrap_or_else(|| trf("enemy {n}", &[("n", &(i + 1))]));
        match kind.as_str() {
            "kills" => trf("kills {name}", &[("name", &name)]),
            "wakes" => trf("wakes {name}", &[("name", &name)]),
            _ => format!("{kind} {name}"),
        }
    };
    let mut parts = Vec::new();
    for (subject, mine, theirs) in [(tr("your line"), you, ai), (tr("the AI's line"), ai, you)] {
        let only: Vec<String> = mine
            .iter()
            .filter(|e| !theirs.contains(e))
            .map(phrase)
            .collect();
        if !only.is_empty() {
            parts.push(trf(
                "{subject} {events}",
                &[
                    ("subject", &subject),
                    ("events", &join_list(&only, " and ")),
                ],
            ));
        }
    }
    if parts.is_empty() {
        return None;
    }
    let mut text = join_list(&parts, "; ");
    if !turn.is_null() {
        text.push_str(&trf(" on turn {n}", &[("n", &py::str_of(turn))]));
    }
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
}

/// A graded turn worth naming: a slip or blunder with its position.
fn costly(record: &Value) -> bool {
    matches!(
        py::get(record, "verdict").as_str(),
        Some("slip" | "blunder")
    ) && py::truthy(py::get(record, "context"))
}

/// `float(v or 0.0)`.
fn float_or_zero(v: &Value) -> f64 {
    if py::truthy(v) {
        float_of(v)
    } else {
        0.0
    }
}

/// `sorted(items, key=key)` for a float key: stable, ascending.
fn sorted_by_key<'a>(
    items: impl Iterator<Item = &'a Value>,
    key: impl Fn(&Value) -> f64,
) -> Vec<&'a Value> {
    let mut v: Vec<&Value> = items.collect();
    v.sort_by(|a, b| key(a).total_cmp(&key(b)));
    v
}

/// The run's costliest decisions, for the death autopsy. Turns rank by
/// expected HP lost (win-chance points when HP is unknown), choices by how
/// far the AI's top pick out-weighed the player's; the two lists are
/// interleaved, turns first, and one fills in when the other runs out.
#[must_use]
pub fn rank_autopsy(turns: &[Value], choices: &[Value], k: usize) -> Vec<Value> {
    let ranked_turns = sorted_by_key(turns.iter().filter(|t| costly(t)), |t| {
        let drop = py::get(t, "hp_drop");
        -(if drop.is_null() {
            float_or_zero(py::get(t, "severity"))
        } else {
            float_of(drop)
        })
    });
    let prob = |c: &Value, side: &str| float_or_zero(py::get(py::get(c, side), "prob"));
    let mut ranked_choices: Vec<&Value> = choices
        .iter()
        .filter(|c| py::get(c, "verdict").as_str() == Some("flag"))
        .collect();
    ranked_choices.sort_by(|a, b| {
        let key = |c: &Value| (-(prob(c, "top") - prob(c, "played")), -prob(c, "top"));
        let (ka, kb) = (key(a), key(b));
        ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    });
    interleave(&ranked_turns, &ranked_choices, k)
}

/// `t1, c1, t2, c2, ...` up to `k`; the longer list fills the rest.
fn interleave(turns: &[&Value], choices: &[&Value], k: usize) -> Vec<Value> {
    let mut out = Vec::new();
    let (mut ti, mut ci) = (0, 0);
    while out.len() < k && (ti < turns.len() || ci < choices.len()) {
        if ti < turns.len() && (out.len() % 2 == 0 || ci >= choices.len()) {
            out.push(json!({"kind": "turn", "record": turns[ti]}));
            ti += 1;
        } else {
            out.push(json!({"kind": "choice", "record": choices[ci]}));
            ci += 1;
        }
    }
    out
}

/// The `recap` record of a won fight (PROTOCOL.md, `coach`).
///
/// `fight` carries the grader's `_Fight` (`hp_start`, `node_id`,
/// `first_msg`, `turns`, `ungraded`, `unfinished`); `turn_records` are the
/// fight's graded turn records still on the active path; `position` is its
/// `{"log_name", "log_idx"}`. `None` when either HP end is unknown.
#[must_use]
pub fn build_recap(
    fight: &Value,
    end_msg: &Value,
    turn_records: &[Value],
    position: &Value,
) -> Option<Value> {
    let hp_start = int_or_none(py::get(fight, "hp_start"))?;
    let hp_end = int_or_none(py::get(end_msg, "hp"))?;
    let mut costliest = sorted_by_key(turn_records.iter().filter(|r| costly(r)), |r| {
        -float_or_zero(py::get(r, "severity"))
    });
    costliest.truncate(RECAP_COSTLIEST);
    let first = py::get(fight, "first_msg");
    let node = py::get(fight, "node_id");
    let floor = end_msg
        .get("floor")
        .unwrap_or_else(|| py::get(first, "floor"));
    let room = if py::truthy(py::get(end_msg, "room_type")) {
        py::get(end_msg, "room_type")
    } else {
        py::get(first, "room_type")
    };
    let mut out: Map<String, Value> = py::as_obj(position).clone();
    let fields = json!({
        "kind": "recap",
        "id": format!("recap:{}", py::str_of(node)),
        "node_id": node,
        "floor": floor,
        "room_type": room,
        "hp_start": hp_start,
        "hp_end": hp_end,
        "hp_lost": (hp_start - hp_end).max(0),
        // Reserved, always null for now: the opening search's best-row mean
        // final HP averages playouts finished by the quick default rollout
        // policy, not the AI's own play, so it overstated the AI's loss
        // (live: "AI typically -12" where the player lost 6). Silence beats
        // a wrong number; the mod renders null as "-N HP".
        "ai_hp_lost": null,
        "turns": py::get(fight, "turns"),
        "turns_graded": turn_records.len(),
        "turns_ungraded": py::get(fight, "ungraded"),
        "unfinished_turns": py::get(fight, "unfinished"),
        "costliest": costliest,
    });
    out.extend(py::as_obj(&fields).clone());
    Some(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn intents_read_like_the_recap_prints_them() {
        let intents = json!([
            {"intent_type": "Attack", "damage": 6, "hits": 2},
            {"intent_type": "Debuff"},
            {"intent_type": "Debuff"},
        ]);
        assert_eq!(intent_text(&intents), "attack 6×2 + debuff");
        assert_eq!(
            intent_text(&json!([{"damage": 11, "hits": 0}])),
            "attack 11"
        );
        assert_eq!(intent_text(&Value::Null), "unknown");
    }

    #[test]
    fn rows_map_back_to_engine_moves() {
        let row = |v| native_from_row(&v);
        assert_eq!(
            row(json!({"stable_idx": -1})),
            Some(json!({"action": "end_turn"}))
        );
        assert_eq!(
            row(json!({"stable_idx": -102, "ally_index": 1})),
            Some(json!({"action": "potion", "slot": 2, "ally_index": 1}))
        );
        assert_eq!(row(json!({"stable_idx": -105})), None);
        let rows = [
            json!({"stable_idx": 0, "ally_index": 0}),
            json!({"stable_idx": 0, "ally_index": 1}),
        ];
        let lift = json!({"action": "play", "card_index": 0, "ally_index": 1});
        assert_eq!(match_row_index(&rows, &lift), Some(1));
    }

    #[test]
    fn the_why_line_names_what_only_one_line_did() {
        let names = ["Lagavulin".to_string()];
        let wakes = [("wakes".to_string(), 0)];
        assert_eq!(
            why_text(&wakes, &[], &names, &json!(1)).as_deref(),
            Some("Your line wakes Lagavulin on turn 1")
        );
        assert_eq!(why_text(&wakes, &wakes, &names, &json!(1)), None);
    }
}
