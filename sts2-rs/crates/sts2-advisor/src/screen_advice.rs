//! A screen's advice from the run-policy net (GH #1426; was the shaping half
//! of `realgame_advisor.compute_screen_advice` and its helpers).
//!
//! The net scores a decision's candidates; this turns those scores into the
//! wire `advice` the HUD draws: the options with their bars, the pick (the
//! run agent's group-mass decode), the note that says who decided, and the
//! answers for the screens no net ranks (the unranked default, the declined
//! potion swap, the Blitz merchant).

use serde_json::{json, Map, Value};

use crate::display_names::Names;
use crate::i18n::{is_zh, map_label, surface, tr, tr_dyn, trf};
use crate::py::{self, get, list, truthy};

/// A decision as the advice reads it: the candidates the net scored, the
/// bridge reply and label per candidate, and who decided when it was not
/// the net (`realgame_ppo.PpoDecision`).
#[derive(Debug, Clone, Copy)]
pub struct Decision<'a> {
    pub actions: &'a [Value],
    pub responses: &'a [Value],
    pub labels: &'a [String],
    pub source_note: &'a str,
}

/// The note of a pick the net never made (GH #399).
pub const FORCED_NOTE: &str = "forced (single option) — not a net ranking";

/// The prefix of the one structured line every unranked answer logs
/// (GH #980).
pub const UNRANKED_LOG_PREFIX: &str = "unranked fallback";

/// The run-policy head a screen with no head of its own is scored with,
/// and the note that says so.
#[must_use]
pub fn surface_standin(advice_for: &str) -> Option<(&'static str, &'static str)> {
    // A chest relic and a boss relic are the same question ("which of these
    // relics do I want"), only the price differs.
    (advice_for == "treasure").then_some(("boss_relic", tr("scored with the boss-relic head")))
}

/// Why a screen no net was trained on is handed back to the player.
#[must_use]
pub fn hand_back_reason(advice_for: &str) -> Option<&'static str> {
    match advice_for {
        "treasure" => Some(tr("the chest relic pick has no trained policy")),
        "card_bundle" => Some(tr("card bundles have no trained policy")),
        _ => None,
    }
}

/// Action kinds whose candidates are one intent fanned out over targets
/// (`ppo_decode.POOLED_KINDS`, `policy.rs::GroupKey`).
const POOLED_KINDS: [&str; 4] = [
    "shop_remove_card",
    "transform_deck_card",
    "enchant_deck_card",
    "smith_card",
];

/// `ppo_decode.group_decode_enabled`: the group decode is on unless
/// `STS2_RUNAI_PPO_GROUP_DECODE` is `"0"`.
#[must_use]
pub fn group_decode_enabled() -> bool {
    std::env::var("STS2_RUNAI_PPO_GROUP_DECODE").map_or(true, |v| v != "0")
}

/// The index of the largest value, ties toward the smallest index.
fn argmax_first(xs: &[f64]) -> usize {
    let mut best = 0;
    for (i, &x) in xs.iter().enumerate().skip(1) {
        if x > xs[best] {
            best = i;
        }
    }
    best
}

/// `ppo_decode.select_index`: the run agent's decode. With the group decode
/// on, each intent's mass is pooled (one shop removal per deck card is one
/// preference), the heaviest group wins, and its most probable member is
/// the pick; otherwise the plain first-max argmax.
#[must_use]
pub fn select_index(actions: &[Value], probs: &[f64], grouped: bool) -> usize {
    if !grouped || actions.len() != probs.len() {
        return argmax_first(probs);
    }
    let keys: Vec<String> = actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let kind = py::str_of(get(a, "action"));
            if POOLED_KINDS.contains(&kind.as_str()) {
                kind
            } else {
                format!("#{i}")
            }
        })
        .collect();
    let mut order: Vec<&str> = Vec::new();
    let mut mass: Vec<f64> = Vec::new();
    for (key, &p) in keys.iter().zip(probs) {
        if let Some(g) = order.iter().position(|k| *k == key.as_str()) {
            mass[g] += p;
        } else {
            order.push(key);
            mass.push(p);
        }
    }
    let winner = order[argmax_first(&mass)];
    let members: Vec<usize> = (0..keys.len()).filter(|&i| keys[i] == winner).collect();
    let member_probs: Vec<f64> = members.iter().map(|&i| probs[i]).collect();
    members[argmax_first(&member_probs)]
}

/// `realgame_ppo.decision_note`: who decided. A forced pick says it is
/// forced (and names the engine rule that made it, when one did); a ranking
/// gives the net's top probability.
#[must_use]
pub fn decision_note(decision: &Decision<'_>, probs: &[f64]) -> String {
    if decision.actions.len() == 1 {
        return if decision.source_note.is_empty() {
            tr(FORCED_NOTE).to_string()
        } else {
            format!("{} · {}", tr(FORCED_NOTE), decision.source_note)
        };
    }
    let top = probs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    trf(
        "run-policy net · p={p}%",
        &[("p", &format!("{:.0}", top * 100.0))],
    )
}

/// A JSON int as Python's `isinstance(v, int)` sees it (`True` is one).
fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

/// `int(v)`, or `None` where Python raises.
fn int_or_raise(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        #[allow(clippy::cast_possible_truncation)]
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite())
                .map(|f| f.trunc() as i64)
        }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `realgame_ppo._strip_bbcode`: `_RE_BBCODE_IMG` (`\[img\].*?\[/img\]`,
/// dot-all) removed, then `_RE_BBCODE_TAG` (`\[/?[a-zA-Z_][^\[\]]*\]`),
/// matched the way `re.sub` scans.
#[must_use]
pub fn strip_bbcode(text: &str) -> String {
    let mut without_img = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[img]") {
        let Some(len) = rest[start + 5..].find("[/img]") else {
            break; // no closer after this opener, so none after a later one
        };
        without_img.push_str(&rest[..start]);
        rest = &rest[start + 5 + len + 6..];
    }
    without_img.push_str(rest);
    let b = without_img.as_bytes();
    let tag_end = |at: usize| -> Option<usize> {
        let mut i = at + 1;
        if b.get(i) == Some(&b'/') {
            i += 1;
        }
        if !b
            .get(i)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        {
            return None;
        }
        i += 1;
        while let Some(&c) = b.get(i) {
            match c {
                b']' => return Some(i + 1),
                b'[' => return None,
                _ => i += 1,
            }
        }
        None
    };
    let mut out = String::with_capacity(without_img.len());
    let mut i = 0;
    let mut kept = 0;
    while i < b.len() {
        if b[i] == b'[' {
            if let Some(end) = tag_end(i) {
                out.push_str(&without_img[kept..i]);
                i = end;
                kept = end;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&without_img[kept..]);
    out
}

/// `screen_labels`: the run policy's option labels as the player reads
/// them. Three builder spellings are ids (a shop ware's, a shop removal's
/// `remove:<CARD_ID>`, a Neow relic's); the screen's own display names
/// replace them.
#[must_use]
pub fn screen_labels(
    state: &Value,
    advice_for: &str,
    decision: &Decision<'_>,
    names: &Names,
) -> Vec<String> {
    let mut labels: Vec<String> = decision.labels.to_vec();
    let mut by_index: Vec<(i64, &Value)> = Vec::new();
    for (i, o) in list(state, "options").iter().enumerate() {
        if !o.is_object() {
            continue;
        }
        let index = o.get("index").map_or(Some(i as i64), as_int);
        if let Some(k) = index {
            by_index.retain(|(j, _)| *j != k);
            by_index.push((k, o));
        }
    }
    let run_state = get(state, "run_state");
    let deck = list(run_state, "master_deck");
    let empty = Value::Object(Map::new());
    for (i, action) in decision.actions.iter().enumerate() {
        if i >= labels.len() {
            break;
        }
        let response = decision.responses.get(i).unwrap_or(&empty);
        let option = as_int(get(response, "index"))
            .and_then(|k| by_index.iter().find(|(j, _)| *j == k).map(|(_, o)| *o));
        match advice_for {
            "shop" => {
                if get(action, "action").as_str() == Some("shop_remove_card") {
                    if let Some(d) = as_int(get(action, "deck_idx"))
                        .and_then(|d| usize::try_from(d).ok())
                        .filter(|&d| d < deck.len())
                    {
                        labels[i] = trf("Remove {card}", &[("card", &names.card_name(&deck[d]))]);
                    }
                } else if let Some(o) = option {
                    labels[i] = names.shop_option_label(o);
                }
            }
            "event" => {
                if let Some(Value::String(shown)) = option.map(|o| get(o, "label")) {
                    let shown = strip_bbcode(shown);
                    if !shown.is_empty() {
                        labels[i] = shown;
                    }
                }
            }
            "potion_swap" => {
                if let Some(o) = option {
                    labels[i] = names.potion_swap_label(o, potion_belt(state));
                }
            }
            "map_select" => labels[i] = map_label(&labels[i]),
            _ => {}
        }
        // A word of our own on a skip row (`skip`, `confirm`); the takeover
        // never cross-checks a skip's label.
        if is_zh() && get(response, "action").as_str() == Some("skip") {
            labels[i] = tr_dyn(&labels[i]);
        }
    }
    labels
}

/// The belt a potion swap discards from: `run_state.potions`, else the
/// screen's own.
fn potion_belt(state: &Value) -> &[Value] {
    let run = list(get(state, "run_state"), "potions");
    if run.is_empty() {
        list(state, "potions")
    } else {
        run
    }
}

/// `_option_index`: the option a reply names. A card reward answers with
/// a bare `index`, a card select with an `indices` list.
fn option_index(response: &Value, fallback: usize) -> i64 {
    if let Some(i) = as_int(get(response, "index")) {
        return i;
    }
    if let Some(i) = list(response, "indices").first().and_then(as_int) {
        return i;
    }
    fallback as i64
}

/// What the net's scores made of a screen.
#[derive(Debug, Clone)]
pub struct Ranked {
    pub advice: Value,
    /// The pick, as an index into the decision's candidates.
    pub best: usize,
}

/// How a screen was asked: the bits of `compute_screen_advice`'s context
/// the shaping reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScreenContext<'a> {
    /// The option the shop already chose to remove, when this removal
    /// screen carries it (`shop_removal_carry_index`).
    pub carried: Option<usize>,
    /// A hand-back surface whose net WAS trained on it (GH #978).
    pub trained_surface: bool,
    /// Why another head scored this screen, when one did.
    pub standin_note: &'a str,
    /// A card offered INSIDE a fight (`is_fight_offer`).
    pub fight_offer: bool,
    /// Group-mass decode on (`group_decode_enabled`).
    pub grouped: bool,
}

/// The advice from the net's per-candidate `probs` (`[1.0]` for a forced
/// pick) and state value. The pick is the carried shop target when there
/// is one, else the run agent's decode; map options carry their node's
/// coordinate.
#[must_use]
pub fn rank(
    state: &Value,
    advice_for: &str,
    decision: &Decision<'_>,
    probs: &[f64],
    value: Option<f64>,
    ctx: &ScreenContext<'_>,
    names: &Names,
) -> Ranked {
    let best_i = ctx
        .carried
        .unwrap_or_else(|| select_index(decision.actions, probs, ctx.grouped));
    let offered: &[Value] = if advice_for == "map_select" {
        list(state, "nodes")
    } else {
        &[]
    };
    let labels = screen_labels(state, advice_for, decision, names);
    let empty = Value::Object(Map::new());
    let options: Vec<Value> = probs
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let response = decision.responses.get(i).unwrap_or(&empty);
            let mut option = json!({
                "label": labels.get(i).map_or("?", String::as_str),
                "visit_share": py::round(p, 4),
                "is_best": i == best_i,
                "index": option_index(response, i),
                "action": response.get("action").cloned().unwrap_or_else(|| json!("choose")),
            });
            if let Some(node) = offered.get(i) {
                // `int(node["col"])`, then `int(node["row"])`: a row that
                // does not convert leaves the column already set.
                if let Some(col) = node.get("col").and_then(int_or_raise) {
                    option["col"] = json!(col);
                    if let Some(row) = node.get("row").and_then(int_or_raise) {
                        option["row"] = json!(row);
                    }
                }
            }
            option
        })
        .collect();
    let mut note = decision_note(decision, probs);
    if let Some(v) = value {
        note += &format!(" · V={v:+.2}");
    }
    let best_response = decision.responses.get(best_i).unwrap_or(&empty);
    let mut best = json!({
        "action": best_response.get("action").cloned().unwrap_or_else(|| json!("choose")),
        "index": option_index(best_response, best_i),
        "label": options.get(best_i).map_or(Value::Null, |o| o["label"].clone()),
    });
    if advice_for == "potion_swap" {
        if let Some(raw) = decision.labels.get(best_i) {
            // The mod's own spelling (`discard <POTION_ID>`): the takeover
            // cross-checks `best.label` against it.
            best["label"] = json!(raw);
        }
    }
    if ctx.carried.is_some() {
        note += tr(" · target carried from the shop pick, not re-ranked here");
    }
    if !ctx.standin_note.is_empty() {
        note += &format!(" · {}", ctx.standin_note);
    }
    if ctx.fight_offer {
        // Trained on "add this to my DECK"; this copy is gone after the
        // fight, so the ranking must not read as a deck verdict.
        note += tr(" · scored as a deck pick; this copy only lasts the fight");
    }
    let mut advice = json!({"note": note, "options": options, "best": best});
    // A forced pick never asked the borrowed head anything, so there is no
    // untrained decision to hand back.
    let forced = decision.actions.len() == 1;
    if let Some(why) = hand_back_reason(advice_for) {
        if !ctx.trained_surface && !forced {
            advice["hand_back"] = json!(why);
        }
    }
    Ranked {
        advice,
        best: best_i,
    }
}

/// `_unranked`: an answer for a surface no head can rank, honest but still
/// usable. The first option is named as the pick and the note says nothing
/// ranked it; every answer carries `hand_back` (GH #874). The warning line
/// (`UNRANKED_LOG_PREFIX`, GH #980) when `trigger` names the way in.
#[must_use]
pub fn unranked(state: &Value, why: &str, trigger: Option<&str>) -> (Value, Option<String>) {
    let warning = trigger.map(|t| {
        let or_dash = |k: &str| {
            let v = get(state, k);
            if truthy(v) {
                py::str_of(v)
            } else {
                "-".to_string()
            }
        };
        format!(
            "{UNRANKED_LOG_PREFIX} trigger={t} prompt_key={} source_model={} :: {why}",
            or_dash("prompt_key"),
            or_dash("source_model")
        )
    });
    let options = ["options", "cards", "nodes"]
        .iter()
        .map(|k| get(state, k))
        .find(|v| truthy(v))
        .map_or(&[][..], py::as_list);
    if options.is_empty() {
        return (
            json!({"note": trf("{why} (nothing to pick)", &[("why", &why)]), "hand_back": why}),
            warning,
        );
    }
    let labels: Vec<String> = options
        .iter()
        .enumerate()
        .filter(|(_, o)| o.is_object())
        .map(|(i, o)| {
            ["label", "name", "id"]
                .iter()
                .map(|k| get(o, k))
                .find(|v| truthy(v))
                .map_or_else(|| trf("option {i}", &[("i", &i)]), py::str_of)
        })
        .collect();
    if labels.is_empty() {
        return (
            json!({"note": trf("{why} (nothing to pick)", &[("why", &why)])}),
            warning,
        );
    }
    let advice = json!({
        "note": trf(
            "{why} — defaulting to the first option, NOT a recommendation",
            &[("why", &why)],
        ),
        "options": labels.iter().enumerate().map(|(i, label)| json!({
            "label": label, "is_best": i == 0, "index": i, "action": "choose",
        })).collect::<Vec<_>>(),
        "best": {"action": "choose", "index": 0, "label": labels[0]},
        "hand_back": why,
    });
    (advice, warning)
}

/// `_decline_potion_swap`: the full-belt swap with the run policy's swap
/// head off: decline it, as the engines do (the reward stays on the screen).
#[must_use]
pub fn decline_potion_swap(state: &Value, names: &Names) -> Value {
    let group = crate::instinct_screens::potion_swap_group(list(state, "options"));
    let Some(decline) = group.first().copied() else {
        return json!({"note": tr("no potion swap on this screen")});
    };
    let label = get(decline, "label");
    let label = if truthy(label) {
        py::str_of(label)
    } else {
        "decline".to_string()
    };
    let belt = potion_belt(state);
    json!({
        "note": tr("potion swap: the run policy's swap head is off, so the reward stays"),
        "options": group.iter().map(|o| json!({
            "label": names.potion_swap_label(o, belt),
            "is_best": std::ptr::eq(*o, decline),
            "index": py::int_of(get(o, "index")),
            "action": "choose",
        })).collect::<Vec<_>>(),
        "best": {"action": "choose", "index": py::int_of(get(decline, "index")), "label": label},
    })
}

/// The Blitz merchant's pick action (`realgame_ppo.BLITZ_CHOICE_ACTION`).
pub const BLITZ_CHOICE_ACTION: &str = "blitz_choice";

/// `realgame_ppo.is_blitz_shop`: the Blitz merchant (free wares, a fixed
/// number of picks).
#[must_use]
pub fn is_blitz_shop(state: &Value) -> bool {
    !get(state, "blitz_choices_remaining").is_null()
        || list(state, "options")
            .iter()
            .any(|o| get(o, "action").as_str() == Some(BLITZ_CHOICE_ACTION))
}

/// `realgame_ppo.blitz_shop_choice`: the first uncursed ware in shop order
/// while picks remain, else leave; and why. Not a ranking: no head was
/// trained on Blitz.
#[must_use]
pub fn blitz_shop_choice(state: &Value) -> (i64, &'static str) {
    let options: Vec<&Value> = list(state, "options")
        .iter()
        .filter(|o| o.is_object())
        .collect();
    let index_of = |i: usize, o: &Value| o.get("index").map_or(i as i64, py::int_of);
    let leave = options
        .iter()
        .enumerate()
        .find(|(_, o)| get(o, "action").as_str() == Some("leave_shop"))
        .map_or(0, |(i, o)| index_of(i, o));
    let remaining = get(state, "blitz_choices_remaining");
    if !remaining.is_null() && py::int_of(remaining) <= 0 {
        return (leave, tr("no Blitz picks left"));
    }
    for (i, o) in options.iter().enumerate() {
        let enabled = o.get("enabled").is_none_or(truthy);
        if get(o, "action").as_str() != Some(BLITZ_CHOICE_ACTION) || !enabled {
            continue;
        }
        if truthy(get(get(o, "item"), "cursed")) {
            continue;
        }
        return (index_of(i, o), tr("first uncursed ware"));
    }
    (leave, tr("only cursed wares left"))
}

/// `_blitz_shop_answer`: the Blitz merchant, an admitted rule, never a net
/// ranking; a hand-back surface (nothing was trained on it). Also the
/// warning line it logs.
#[must_use]
pub fn blitz_shop_answer(state: &Value) -> (Value, String) {
    let (pick, reason) = blitz_shop_choice(state);
    let why = tr("Blitz shop: no net was trained on Blitz wares");
    let mut labels: Vec<(i64, String)> = Vec::new();
    for (i, o) in list(state, "options")
        .iter()
        .filter(|o| o.is_object())
        .enumerate()
    {
        let raw = [get(get(o, "item"), "title"), get(o, "label")]
            .into_iter()
            .find(|v| truthy(v))
            .map_or_else(|| trf("option {i}", &[("i", &i)]), py::str_of);
        let idx = o.get("index").map_or(i as i64, py::int_of);
        let label = strip_bbcode(&raw);
        match labels.iter_mut().find(|(k, _)| *k == idx) {
            Some(slot) => slot.1 = label,
            None => labels.push((idx, label)),
        }
    }
    let warning = format!(
        "{UNRANKED_LOG_PREFIX} trigger=blitz_shop prompt_key=- source_model=- :: {why} ({reason})"
    );
    let best_label = labels
        .iter()
        .find(|(k, _)| *k == pick)
        .map_or_else(|| pick.to_string(), |(_, l)| l.clone());
    let advice = json!({
        "note": trf(
            "{why} — {reason}, NOT a recommendation",
            &[("why", &why), ("reason", &reason)],
        ),
        "options": labels.iter().map(|(idx, label)| json!({
            "label": label, "is_best": *idx == pick, "index": idx, "action": "choose",
        })).collect::<Vec<_>>(),
        "best": {"action": "choose", "index": pick, "label": best_label},
        "hand_back": why,
    });
    (advice, warning)
}

/// `_project_map_path`: walk the run-policy net forward from its chosen
/// node to sketch the route. The map surface only ranks the CHILDREN of
/// where you stand, so one query answers "which node next", never "which
/// way through the act"; this re-asks the net at every fork, moving the
/// observation's `map_coord` along the graph, with HP walked forward between
/// hops ([`crate::route::walk`]) so each fork is answered at the HP the
/// player would arrive with. A PROJECTION, not a plan
/// ([`crate::route::route_note`] says so).
///
/// `score(obs, actions)` is the net's distribution over the candidates, or
/// `None` when it cannot answer (the walk ends there).
pub fn project_map_path(
    state: &Value,
    best_index: usize,
    score: &mut dyn FnMut(
        &sts2_runai::ppo::obs::PpoObs,
        &[sts2_runai::ppo::obs::ActionDesc],
    ) -> Option<Vec<f64>>,
) -> Vec<Value> {
    use crate::route;
    use sts2_runai::ppo::obs::ActionDesc;

    let offered = list(state, "nodes");
    if list(get(state, "map"), "nodes").is_empty() || best_index >= offered.len() {
        return Vec::new();
    }
    let Some(base) = sts2_runai::ppo::wire::build_decision(state, "map_select").map(|d| d.obs)
    else {
        return Vec::new();
    };
    let Some(chosen) = route::coord_of(&offered[best_index]) else {
        return Vec::new();
    };
    let graph = route::route_graph(state);
    // `_base_metrics`' `max_hp` (`or 0`), not the observation's (`or 1`).
    #[allow(clippy::cast_precision_loss)]
    let max_hp = py::int_of(get(get(state, "run_state"), "max_hp")) as f64;
    #[allow(clippy::cast_precision_loss)]
    let hp = base.hp as f64;
    let act = if base.act_index == 0 {
        1
    } else {
        base.act_index
    };
    route::walk(
        &graph,
        chosen,
        act,
        hp,
        max_hp,
        |hop, at, children, hp, _| {
            let mut obs = base.clone();
            obs.map_coord = Some(at);
            obs.total_floor = base.total_floor + hop as i64 + 1;
            obs.act_floor = at.1 + 1;
            obs.hp = route::round_half_even(hp);
            let actions: Vec<ActionDesc> = children
                .iter()
                .map(|&(col, _)| ActionDesc::Move { col })
                .collect();
            let probs = score(&obs, &actions)?;
            if probs.is_empty() {
                return None;
            }
            children.get(argmax_first(&probs)).copied()
        },
    )
}

/// The run-policy net, as the screen advice asks it.
pub trait ScreenNet {
    /// The full masked softmax over `actions`, and V(s).
    ///
    /// # Errors
    ///
    /// The decision exceeds the net's export budget, or inference failed.
    fn score(
        &self,
        obs: &sts2_runai::ppo::obs::PpoObs,
        actions: &[sts2_runai::ppo::obs::ActionDesc],
    ) -> Result<(Vec<f64>, f64), String>;

    /// Whether this net was trained on Scroll Boxes packs (`card_bundles`).
    fn trained_on_packs(&self) -> bool;
}

/// The run policy for a character.
pub enum Lookup {
    Net(Box<dyn ScreenNet>),
    /// No net could be loaded; why.
    Missing(String),
    /// A net this port does not score (a dev backend): the Python lane
    /// answers from here on.
    OtherBackend,
}

impl std::fmt::Debug for Lookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Net(_) => f.write_str("Lookup::Net"),
            Self::Missing(why) => write!(f, "Lookup::Missing({why:?})"),
            Self::OtherBackend => f.write_str("Lookup::OtherBackend"),
        }
    }
}

/// What [`compute_screen_advice`] answered.
#[derive(Debug, Clone)]
pub enum Outcome {
    Advice(Value),
    /// The policy is another backend's: hand the session-applied state back.
    OtherBackend(Value),
}

/// A log line: level, the Python logger it was written to, message.
pub type LogLine = (crate::advisor_mirror::Level, &'static str, String);

/// Everything around a screen request: the run's scratch, the fight mirror,
/// the search knobs for an in-fight choice, the policy lookup.
pub struct ScreenEnv<'a> {
    pub session: Option<&'a mut Map<String, Value>>,
    pub mirror: Option<&'a mut crate::advisor_mirror::AdvisorMirror>,
    pub cfg: Option<&'a crate::search::RecommendConfig>,
    pub sims: Option<u32>,
    pub names: &'a Names,
    pub wire_for: &'a mut crate::advisor_mirror::WireFor<'a>,
    pub policy_for: &'a mut dyn FnMut(&str) -> Lookup,
    /// What the advisor logs on the way.
    pub log: Vec<LogLine>,
}

impl std::fmt::Debug for ScreenEnv<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenEnv")
            .field("session", &self.session)
            .field("cfg", &self.cfg)
            .field("sims", &self.sims)
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

impl ScreenEnv<'_> {
    fn warn(&mut self, logger: &'static str, message: String) {
        self.log
            .push((crate::advisor_mirror::Level::Warning, logger, message));
    }

    /// `_searched_card_select_advice`: an in-fight choice answered by the
    /// combat search on the fight mirror's park, or `None`.
    fn searched(&mut self, state: &Value) -> Option<Value> {
        let mirror = self.mirror.as_deref_mut()?;
        mirror.answer_card_select(state, self.cfg, self.sims, None, None, self.wire_for)
    }

    /// An in-fight `card_select` no head covers: the combat search, else
    /// -- once cards are already taken -- confirm them, else the
    /// `hand_select` direction. The heuristic rungs pick ONE card; asked
    /// again after it (`selected_count`, a hand or pile select of several
    /// cards), they would pick another each time until the offer ran out.
    fn in_fight_select(&mut self, state: &Value) -> Option<Value> {
        if let Some(searched) = self.searched(state) {
            return Some(searched);
        }
        if crate::advisor_mirror::selected_count(state) > 0 {
            return Some(crate::advisor_mirror::confirm_selection_answer(state));
        }
        crate::hand_select::rank(state)
    }

    fn unranked(&mut self, state: &Value, why: &str, trigger: &str) -> Value {
        let (advice, warning) = unranked(state, why, Some(trigger));
        if let Some(w) = warning {
            self.warn("realgame_advisor", w);
        }
        advice
    }
}

/// `is_fight_offer`: a choose-a-card screen opened INSIDE a fight
/// (Discovery, a Skill Potion): a card generated into this fight.
#[must_use]
pub fn is_fight_offer(state: &Value) -> bool {
    get(state, "surface").as_str() == Some("choose_a_card") && !get(state, "combat").is_null()
}

/// `fight_offer_as_select`: an in-fight choose-a-card as the in-combat
/// `card_select` it is.
#[must_use]
pub fn fight_offer_as_select(state: &Value) -> Value {
    let mut out = state.clone();
    out["advice_for"] = json!("card_select");
    out["min_select"] = json!(1);
    out["max_select"] = json!(1);
    out["cancelable"] = json!(truthy(get(state, "can_skip")));
    out["source"] = json!("FromChooseACardScreen");
    out
}

/// `compute_screen_advice`: a screen's advice from the run-policy net.
///
/// The decision is the Rust-built one the net was trained against
/// (`ppo::wire`), scored for the full masked softmax so the HUD can show
/// every option's probability. A card generated into a fight goes to the
/// combat search on the fight mirror first; the Blitz merchant and the
/// Crystal Sphere are answered by rule; an in-fight `card_select` no head
/// covers falls to the search, then `hand_select`, then the unranked
/// default.
#[must_use]
pub fn compute_screen_advice(state: &Value, advice_for: &str, env: &mut ScreenEnv<'_>) -> Outcome {
    use sts2_runai::ppo::wire;
    let mut state = state.clone();
    if let Some(session) = env.session.as_deref_mut() {
        if let Some(map) = crate::session::apply_session_context(&state, session) {
            state["map"] = map;
        }
    }
    if advice_for == "card_reward" && is_fight_offer(&state) {
        // A card generated into the fight is a fight decision; the deck-pick
        // head below is only the fallback.
        if let Some(searched) = env.searched(&fight_offer_as_select(&state)) {
            return Outcome::Advice(searched);
        }
    }
    if advice_for == "shop" && is_blitz_shop(&state) {
        let (advice, warning) = blitz_shop_answer(&state);
        env.warn("realgame_advisor", warning);
        return Outcome::Advice(advice);
    }
    if advice_for == "crystal_sphere" {
        let (advice, warning) = crate::crystal_sphere::advice(&state);
        if let Some(w) = warning {
            env.warn("crystal_sphere_policy", w);
        }
        return Outcome::Advice(advice);
    }
    let character =
        crate::headless::character_display_name(get(get(&state, "run_state"), "character"))
            .unwrap_or("Ironclad");
    let net = match (env.policy_for)(character) {
        Lookup::Net(net) => net,
        Lookup::Missing(error) => {
            // An in-fight selection needs no run net: the combat search (or
            // the hand-select direction) answers it as it would with one.
            if advice_for == "card_select" {
                if let Some(answer) = env.in_fight_select(&state) {
                    return Outcome::Advice(answer);
                }
            }
            return Outcome::Advice(json!({
                "note": trf("no run-policy net: {error}", &[("error", &error)]),
                "hand_back": tr("no run-policy net is loaded"),
            }));
        }
        Lookup::OtherBackend => return Outcome::OtherBackend(state),
    };

    let mut standin_note = String::new();
    let mut trained_surface = false;
    let decision = match advice_for {
        // Keyed off the screen's own prompt_key first; prev_msg_type is only
        // the fallback.
        "card_select" => wire::build_card_select(&state, None),
        "card_bundle" => {
            if net.trained_on_packs() {
                trained_surface = true;
            } else {
                standin_note =
                    tr("packs scored by their first card; this net was not trained on packs")
                        .into();
            }
            wire::build_decision(&state, "card_bundle")
        }
        _ => {
            let (head, note) = surface_standin(advice_for).unwrap_or((advice_for, ""));
            standin_note = note.to_string();
            if advice_for == "potion_swap"
                && !sts2_runai::policy::resolve_potion_swap_flag_from_env()
            {
                return Outcome::Advice(decline_potion_swap(&state, env.names));
            }
            const HEADS: [&str; 7] = [
                "potion_swap",
                "map_select",
                "card_reward",
                "shop",
                "rest_site",
                "event",
                "boss_relic",
            ];
            if !HEADS.contains(&head) {
                let why = trf(
                    "no trained head for {surface}",
                    &[("surface", &surface(advice_for))],
                );
                return Outcome::Advice(env.unranked(
                    &state,
                    &why,
                    &format!("no_head:{advice_for}"),
                ));
            }
            let mut build_state = state.clone();
            if head != advice_for {
                build_state["type"] = json!(head);
            }
            wire::build_decision(&build_state, head)
        }
    };
    let Some(decision) = decision.filter(|d| !d.actions.is_empty()) else {
        // No head was trained on an in-combat selection: the combat search
        // on the mirror's park, then the punish/reward direction, then the
        // admitted arbitrary pick. Each says in its note which answered.
        if advice_for == "card_select" {
            if let Some(answer) = env.in_fight_select(&state) {
                return Outcome::Advice(answer);
            }
        }
        let why = trf(
            "{surface}: the net does not cover this surface",
            &[("surface", &surface(advice_for))],
        );
        let trigger = if advice_for == "card_select" {
            "card_select".to_string()
        } else {
            format!("no_decision:{advice_for}")
        };
        return Outcome::Advice(env.unranked(&state, &why, &trigger));
    };

    let (probs, value) = if decision.actions.len() == 1 {
        (vec![1.0], None)
    } else {
        match net.score(&decision.obs, &decision.actions) {
            Ok((probs, value)) => (probs, Some(value)),
            Err(exc) => {
                // Advice must always render (a 103-option shop removal can
                // exceed the export budget, GH #315).
                env.warn(
                    "realgame_advisor",
                    format!("run-policy inference failed for {advice_for}: {exc}"),
                );
                let why = trf(
                    "{surface}: run-policy inference failed ({exc})",
                    &[("surface", &surface(advice_for)), ("exc", &exc)],
                );
                return Outcome::Advice(env.unranked(&state, &why, "inference_failed"));
            }
        }
    };
    let actions: Vec<Value> = decision
        .actions
        .iter()
        .map(|a| serde_json::to_value(a).unwrap_or(Value::Null))
        .collect();
    // The shop already chose WHICH card to remove; this screen only
    // executes it.
    let mut carried = None;
    if advice_for == "card_select" {
        if let Some(session) = env.session.as_deref_mut() {
            let (index, warning) =
                crate::session::shop_removal_carry_index(&state, session, decision.responses.len());
            carried = index;
            if let Some(w) = warning {
                env.warn("realgame_ppo", w);
            }
        }
    }
    let view = Decision {
        actions: &actions,
        responses: &decision.responses,
        labels: &decision.labels,
        source_note: &decision.source_note,
    };
    let ctx = ScreenContext {
        carried,
        trained_surface,
        standin_note: &standin_note,
        fight_offer: is_fight_offer(&state),
        grouped: group_decode_enabled(),
    };
    let Ranked { mut advice, best } =
        rank(&state, advice_for, &view, &probs, value, &ctx, env.names);
    if advice_for == "shop" {
        if let Some(session) = env.session.as_deref_mut() {
            crate::session::stash_shop_removal_target(
                session,
                get(&state, "run_state"),
                &actions[best],
            );
        }
    }
    if advice_for == "map_select" {
        let path = project_map_path(&state, best, &mut |obs, actions| {
            net.score(obs, actions).ok().map(|(p, _)| p)
        });
        if !path.is_empty() {
            let note = crate::route::route_note(&path);
            if let Some(Value::String(n)) = advice.get_mut("note") {
                n.push_str(&note);
            }
            advice["path"] = Value::Array(path);
        }
    }
    Outcome::Advice(advice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbcode_strips_the_way_the_python_regexes_do() {
        assert_eq!(strip_bbcode("[red]6[/red] gold"), "6 gold");
        assert_eq!(strip_bbcode("a[img]x[b]y[/img]b"), "ab");
        assert_eq!(strip_bbcode("a[img]open"), "aopen");
        assert_eq!(strip_bbcode("[[b]x] [1] [/]"), "[x] [1] [/]");
        assert_eq!(strip_bbcode("é[gold]ü[/gold]"), "éü");
    }

    #[test]
    fn a_split_removal_preference_beats_one_stronger_buy() {
        let actions = vec![
            json!({"action": "shop_buy_card", "idx": 0}),
            json!({"action": "shop_remove_card", "deck_idx": 0}),
            json!({"action": "shop_remove_card", "deck_idx": 1}),
            json!({"action": "shop_remove_card", "deck_idx": 2}),
        ];
        let probs = [0.4, 0.2, 0.25, 0.15];
        assert_eq!(select_index(&actions, &probs, true), 2);
        assert_eq!(select_index(&actions, &probs, false), 0);
    }

    #[test]
    fn a_forced_pick_says_so_and_a_ranking_gives_its_top_probability() {
        let one = [json!({"action": "choose"})];
        let d = Decision {
            actions: &one,
            responses: &[],
            labels: &[],
            source_note: "",
        };
        assert_eq!(decision_note(&d, &[1.0]), FORCED_NOTE);
        let two = [json!({}), json!({})];
        let d = Decision { actions: &two, ..d };
        assert_eq!(decision_note(&d, &[0.305, 0.695]), "run-policy net · p=70%");
    }

    /// GH #1770: the mod asks again after each card of a hand select that
    /// takes several. Without a search to continue, the heuristic's one pick
    /// is done: confirm, not a second, third, ... "worst card".
    #[test]
    fn a_heuristic_select_confirms_once_a_card_is_taken() {
        let names = Names::new(std::iter::empty());
        let mut wire_for = |_: &Value, _: Option<&str>, _: i64, _: i64, _: u64| {
            Err::<Value, _>(("Unreachable".to_string(), "no wire".to_string()))
        };
        let mut policy_for = |_: &str| Lookup::Missing("no net".into());
        let mut env = ScreenEnv {
            session: None,
            mirror: None,
            cfg: None,
            sims: None,
            names: &names,
            wire_for: &mut wire_for,
            policy_for: &mut policy_for,
            log: Vec::new(),
        };
        let state = |selected: i64| {
            json!({
                "cards": [{"name": "Strike", "rarity": "Basic"}, {"name": "Poor Sleep", "is_curse": true}],
                "min_select": 0, "max_select": 999_999_999,
                "prompt_key": "GAMBLERS_BREW.selectionScreenPrompt",
                "selected_count": selected, "combat": {},
            })
        };
        let best = |out: Outcome| match out {
            Outcome::Advice(a) => a["best"].clone(),
            Outcome::OtherBackend(_) => panic!("no other backend"),
        };
        assert_eq!(
            best(compute_screen_advice(&state(0), "card_select", &mut env)),
            json!({"action": "choose", "index": 1, "label": "Poor Sleep"})
        );
        assert_eq!(
            best(compute_screen_advice(&state(1), "card_select", &mut env)),
            json!({"action": "skip", "label": "confirm"})
        );
    }
}
