//! Instinct's advice shaping (GH #1426 step 3; was in
//! `realgame_instinct.py` / `realgame_instinct_screens.py`): the student's
//! per-class answer on one screen turned into the wire `advice`.

use serde_json::{json, Map, Value};

use crate::i18n::{is_zh, tr, tr_dyn, trf};
use crate::py;

/// The student's answer at one position (`InstinctPick`), before it is
/// shaped for the wire.
#[derive(Debug, Clone)]
pub struct Pick {
    /// The peeked decision (`E2ePeek.decision`).
    pub decision: Value,
    /// Softmax per merged class.
    pub probs: Vec<f64>,
    /// The greedy class.
    pub best: usize,
    pub ms: f64,
}

/// The advice note: `"Instinct (e2e net, no search) · card reward · 87% · 12 ms"`.
#[must_use]
pub fn note_for(pick: &Pick, p_best: f64) -> String {
    let dtype = py::get(&pick.decision, "decision_type");
    let dtype = if py::truthy(dtype) {
        py::str_of(dtype)
    } else {
        String::new()
    };
    let dtype = dtype.to_lowercase().replace('_', " ");
    let dtype = if is_zh() { tr_dyn(&dtype) } else { dtype };
    trf(
        "Instinct (e2e net, no search) · {dtype} · {p}% · {ms} ms",
        &[
            ("dtype", &dtype),
            ("p", &format!("{:.0}", p_best * 100.0)),
            ("ms", &format!("{:.0}", pick.ms)),
        ],
    )
}

/// The wire `advice` from per-class screen responses.
///
/// `rows[class] = (response, label)`, where `response` is the screen's
/// `{"action": "choose", "index": i}` or `{"action": "skip"}`; `None` when
/// the class names nothing on this screen (dropped). Classes that share a
/// response (several removal targets answered by one "remove" button) are
/// shown once, with their probabilities summed and the student's best
/// member's label. The best is the student's greedy class, or, when that
/// names nothing here, its next choice.
///
/// # Errors
///
/// No class names anything on this screen (the player-facing message).
pub fn screen_advice(
    pick: &Pick,
    rows: &[Option<(Value, String)>],
    note_extra: &str,
) -> Result<Value, String> {
    let mut order: Vec<usize> = (0..pick.probs.len()).collect();
    order.sort_by(|&a, &b| pick.probs[b].total_cmp(&pick.probs[a]));
    // (key, response, label, p), in first-seen order.
    let mut merged: Vec<((Value, Value), Value, String, f64)> = Vec::new();
    let mut best_key: Option<(Value, Value)> = None;
    for ci in order {
        let Some(Some((response, label))) = rows.get(ci) else {
            continue;
        };
        let key = (
            py::get(response, "action").clone(),
            py::get(response, "index").clone(),
        );
        let p = pick.probs[ci];
        let same = |k: &(Value, Value)| py::eq(&k.0, &key.0) && py::eq(&k.1, &key.1);
        match merged.iter_mut().find(|m| same(&m.0)) {
            Some(m) => m.3 += p,
            None => merged.push((key.clone(), response.clone(), label.clone(), p)),
        }
        if best_key.is_none() {
            best_key = Some(key);
        }
    }
    let Some(best_key) = best_key else {
        return Err(tr("none of Instinct's choices are on this screen").into());
    };
    let is_best = |k: &(Value, Value)| py::eq(&k.0, &best_key.0) && py::eq(&k.1, &best_key.1);
    let mut ranked: Vec<&((Value, Value), Value, String, f64)> = merged.iter().collect();
    ranked.sort_by(|a, b| b.3.total_cmp(&a.3));
    let options: Vec<Value> = ranked
        .iter()
        .map(|(key, resp, label, p)| {
            let p = py::round(*p, 4);
            json!({
                "label": label,
                "visit_share": p,
                "prob": p,
                "is_best": is_best(key),
                "index": resp.get("index").cloned().unwrap_or(json!(-1)),
                "action": resp.get("action").cloned().unwrap_or(json!("choose")),
            })
        })
        .collect();
    let best_row = merged
        .iter()
        .find(|m| is_best(&m.0))
        .expect("the best key was merged");
    let mut best: Map<String, Value> = py::as_obj(&best_row.1).clone();
    best.insert("label".into(), json!(best_row.2));
    let mut note = note_for(pick, best_row.3);
    if !matches!(rows.get(pick.best), Some(Some(_))) {
        note.push_str(tr(" · its first choice is not on this screen"));
    }
    if !note_extra.is_empty() {
        note.push_str(" · ");
        note.push_str(note_extra);
    }
    Ok(json!({"note": note, "options": options, "best": best}))
}

/// A screen with one button: no decision, the button at probability 1.
#[must_use]
pub fn lone_option(o: &Value) -> Value {
    let label = ["label", "id"]
        .iter()
        .map(|k| py::get(o, k))
        .find(|v| py::truthy(v))
        .map_or_else(|| tr("continue").to_string(), py::str_of);
    let index = o.get("index").map_or(0, py::int_of);
    json!({
        "note": tr("Instinct (e2e net, no search) · one option"),
        "options": [{
            "label": label,
            "visit_share": 1.0,
            "prob": 1.0,
            "is_best": true,
            "index": index,
            "action": "choose",
        }],
        "best": {"action": "choose", "index": index, "label": label},
    })
}

/// A combat action's run form (`sts2_e2e::env::combat_to_rm`) back in the
/// legal-action form the advisor's mapping reads (`_combat_form`).
#[must_use]
pub fn combat_form(run_action: &Value) -> Value {
    let get = |k| py::get(run_action, k).clone();
    match py::get(run_action, "action").as_str() {
        Some("play_card") => json!({
            "action": "play", "card_index": get("hand_idx"), "target_index": get("target_index"),
        }),
        Some("drink_potion") => json!({
            "action": "drink_potion", "slot": get("potion_slot"), "target_index": get("target_index"),
        }),
        Some("combat_resolve_choice") => {
            let option = get("option");
            let selection = if option.is_null() {
                json!([])
            } else {
                json!([option])
            };
            json!({"action": "choose", "selection": selection})
        }
        Some("combat_resolve_choice_selection") => {
            let sel = py::get(run_action, "selection");
            let sel = if py::truthy(sel) {
                sel.clone()
            } else {
                json!([])
            };
            json!({"action": "choose", "selection": sel})
        }
        _ => run_action.clone(),
    }
}

/// The pick for a position with one move (`forced_pick`): that move, at
/// probability 1. `kind` is the peek's `"combat"` / `"run"`.
#[must_use]
pub fn forced_pick(action: &Value, kind: &str, ms: f64) -> Pick {
    let combat = kind == "combat";
    let action = if combat {
        combat_form(action)
    } else {
        action.clone()
    };
    Pick {
        decision: json!({
            "decision_type": "FORCED",
            "is_combat": combat,
            "classes": [{"class": 0, "actions": [action]}],
        }),
        probs: vec![1.0],
        best: 0,
        ms,
    }
}

/// [`evaluate`]'s answer: the pick, and the win probe's read of the
/// position when the weights carry one (GH #1222).
#[derive(Debug, Clone)]
pub struct Evaluated {
    pub pick: Pick,
    pub estimate: Option<Value>,
}

/// Peek `rm` with a copy of this run's driver state and run the student
/// (`InstinctAdvisor.evaluate`).
///
/// `expect`: the decision types this screen can be; anything else means the
/// rebuilt run is not where the screen is.
///
/// # Errors
///
/// The player-facing reason Instinct cannot answer here.
pub fn evaluate(
    net: &sts2_e2e::net::E2eNet,
    driver: &mut sts2_e2e::env::Driver,
    cache: &sts2_e2e::tokens::TokCache,
    rm: &mut sts2_core::run_manager::RunManager,
    expect: &[String],
) -> Result<Evaluated, String> {
    use sts2_e2e::env::{ApplyKind, Next};
    let t0 = std::time::Instant::now();
    let ms = || t0.elapsed().as_secs_f64() * 1000.0;
    // A failed observation only costs the intent log a snapshot.
    let _ = driver.observe_combat_position(rm);
    let mut probe = driver.clone();
    let mut probe_cache = cache.clone();
    match probe.peek(rm) {
        Next::Terminal(t) => {
            let why = t
                .error
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| t.outcome.to_string());
            Err(trf(
                "Instinct cannot read this position ({why})",
                &[("why", &why)],
            ))
        }
        // One move (a lone legal action, or one merged class): the env takes
        // it without asking; Instinct answers it with certainty.
        Next::Forced(f) => {
            let kind = match f.kind {
                ApplyKind::Combat => "combat",
                ApplyKind::Run { .. } => "run",
            };
            Ok(Evaluated {
                pick: forced_pick(&f.action, kind, ms()),
                estimate: None,
            })
        }
        Next::Decision(d) => {
            let decision = d.to_value();
            let dtype = py::get(&decision, "decision_type");
            if !expect.is_empty() && !expect.iter().any(|e| dtype.as_str() == Some(e.as_str())) {
                return Err(trf(
                    "the rebuilt run stands at {dtype}, not this screen",
                    &[("dtype", &py::str_of(dtype))],
                ));
            }
            let ev = sts2_e2e::tokens::encode_for(net, &d, rm, &mut probe_cache)
                .and_then(|row| net.evaluate(&row.into()))
                .map_err(|e| {
                    trf(
                        "Instinct could not evaluate this position ({e})",
                        &[("e", &e)],
                    )
                })?;
            let estimate = ev
                .estimate
                .map(|e| e.to_json(Some(sts2_e2e::env::hp_now(rm))));
            Ok(Evaluated {
                pick: Pick {
                    decision,
                    probs: sts2_e2e::net::softmax(&ev.logits),
                    best: sts2_e2e::net::argmax(&ev.logits),
                    ms: ms(),
                },
                estimate,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(probs: &[f64], best: usize) -> Pick {
        Pick {
            decision: json!({"decision_type": "CARD_REWARD"}),
            probs: probs.to_vec(),
            best,
            ms: 12.4,
        }
    }

    #[test]
    fn shared_responses_merge_and_the_greedy_class_is_best() {
        let choose = |i: i64| Some((json!({"action": "choose", "index": i}), format!("card {i}")));
        let rows = vec![choose(0), choose(1), choose(1), None];
        let got = screen_advice(&pick(&[0.2, 0.3, 0.25, 0.25], 1), &rows, "").unwrap();
        assert_eq!(
            got["best"],
            json!({"action": "choose", "index": 1, "label": "card 1"})
        );
        assert_eq!(got["options"][0]["prob"], json!(0.55));
        assert_eq!(
            got["note"],
            "Instinct (e2e net, no search) · card reward · 55% · 12 ms"
        );
    }

    #[test]
    fn a_greedy_class_off_the_screen_is_noted() {
        let rows = vec![None, Some((json!({"action": "skip"}), "skip".to_string()))];
        let got = screen_advice(&pick(&[0.9, 0.1], 0), &rows, "x").unwrap();
        assert!(got["note"]
            .as_str()
            .unwrap()
            .ends_with("10% · 12 ms · its first choice is not on this screen · x"));
        assert_eq!(got["options"][0]["index"], json!(-1));
        assert!(screen_advice(&pick(&[1.0], 0), &[None], "").is_err());
    }
}
