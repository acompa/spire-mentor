//! Co-op: hold a move the teammate is about to make better (GH #1426 step 4;
//! was `coop_coordination.py`).
//!
//! The combat search plans for the local player alone, so it cannot see that
//! a teammate who has not ended their turn is about to change the value of
//! our move: their Vulnerable makes our attack hit harder, their Weak or
//! block makes our block partly wasted. It does not change WHAT the advisor
//! recommends, only WHEN: the reply carries `wait_for_ally` and autoplay
//! holds the move a short, capped time.
//!
//! The card tables come from the v0.107.1 + v0.111.0 decompilation: every
//! card whose `OnPlay` applies the power to an enemy. Left out on purpose:
//! DOUBT (a curse that makes its owner Weak) and MAD_SCIENCE (its effect is
//! chosen at runtime).

use serde_json::{json, Value};

use crate::i18n::{tr, trf};
use crate::py::{self, get, truthy};

/// Cards that make an enemy Vulnerable: our attack should land after them.
pub const ENEMY_VULNERABLE: [&str; 21] = [
    "ASSASSINATE",
    "BASH",
    "BEAM_CELL",
    "BREAK",
    "COMET",
    "DOMINATE",
    "EXPOSE",
    "FALLING_STAR",
    "FEAR",
    "GAMMA_BLAST",
    "HIGH_FIVE",
    "KNOW_THY_PLACE",
    "METEOR_SHOWER",
    "MOLTEN_FIST",
    "PUTREFY",
    "SHOCKWAVE",
    "SQUASH",
    "TAUNT",
    "THUNDERCLAP",
    "TREMBLE",
    "UPPERCUT",
];

/// Cards that make an enemy Weak: our block should come after them.
pub const ENEMY_WEAK: [&str; 19] = [
    "COMET",
    "DEATHBRINGER",
    "DEFY",
    "FALLING_STAR",
    "GAMMA_BLAST",
    "GO_FOR_THE_EYES",
    "HAZE",
    "KNOW_THY_PLACE",
    "LEG_SWEEP",
    "MALAISE",
    "METEOR_SHOWER",
    "NEUTRALIZE",
    "NULL",
    "PUTREFY",
    "SCARE",
    "SHOCKWAVE",
    "SUCKER_PUNCH",
    "SUPPRESS",
    "UPPERCUT",
];

/// Multiplayer cards that block for us, or cover us (INTERCEPT: the
/// teammate takes our hits): our block should come after.
pub const ALLY_BLOCK: [&str; 5] = [
    "CONSTELLATION",
    "DEMONIC_SHIELD",
    "LIFT",
    "RALLY",
    "INTERCEPT",
];

/// How long one reply may hold its move. The mod caps the round at 6 s.
pub const WAIT_MS: i64 = 2500;

fn card_id(card: &Value) -> String {
    let id = get(card, "id");
    if truthy(id) {
        py::str_of(id).to_uppercase()
    } else {
        String::new()
    }
}

/// `isinstance(v, int)`: a bool is an int.
fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

fn affordable(card: &Value, energy: i64) -> bool {
    if truthy(get(card, "costs_x")) {
        return energy > 0;
    }
    let cost = match card.as_object().and_then(|m| m.get("effective_cost")) {
        Some(c) => c,
        None => get(card, "cost"),
    };
    as_int(cost).is_some_and(|c| 0 <= c && c <= energy)
}

fn blocks(card: &Value) -> bool {
    get(card, "stats").as_object().is_some_and(|stats| {
        stats
            .iter()
            .any(|(k, v)| k.contains("block") && as_int(v).is_some_and(|n| n > 0))
    })
}

fn every_enemy_has(state: &Value, power_prefix: &str) -> bool {
    let alive: Vec<&Value> = py::list(state, "enemies")
        .iter()
        .filter(|e| !truthy(get(e, "is_dead")) && get(e, "hp").as_f64().is_some_and(|hp| hp > 0.0))
        .collect();
    !alive.is_empty()
        && alive.iter().all(|e| {
            py::list(e, "powers")
                .iter()
                .any(|p| py::str_of_or_empty(get(p, "id")).starts_with(power_prefix))
        })
}

/// The `wait_for_ally` hint for this combat reply, or `None`: when a living
/// teammate who has not ended their turn can afford a card that should go
/// before our recommended play (`state`: the `combat_action` board; only
/// the advice's `best` is read).
#[must_use]
pub fn ally_wait(state: &Value, advice: &Value) -> Option<Value> {
    let best = get(advice, "best");
    let hand = py::list(state, "hand");
    let idx = as_int(get(best, "card_index"))
        .and_then(|i| usize::try_from(i).ok())
        .filter(|&i| i < hand.len())?;
    let ours = &hand[idx];
    let mut want: Vec<(&[&str], &str)> = Vec::new();
    if get(ours, "type").as_str() == Some("Attack") && !every_enemy_has(state, "VULNERABLE") {
        want.push((&ENEMY_VULNERABLE, tr("Vulnerable before our attack")));
    }
    if blocks(ours) {
        if !every_enemy_has(state, "WEAK") {
            want.push((&ENEMY_WEAK, tr("Weak before our block")));
        }
        want.push((&ALLY_BLOCK, tr("their block for us first")));
    }
    let ours_id = card_id(ours);
    if want.is_empty()
        || ENEMY_VULNERABLE.contains(&ours_id.as_str())
        || ENEMY_WEAK.contains(&ours_id.as_str())
    {
        return None;
    }
    for ally in py::list(state, "allies") {
        if truthy(get(ally, "is_dead")) || truthy(get(ally, "ended_turn")) {
            continue;
        }
        let energy = py::int_of(get(ally, "energy"));
        for card in py::list(ally, "hand") {
            if !affordable(card, energy) {
                continue;
            }
            let id = card_id(card);
            for (ids, reason) in &want {
                if ids.contains(&id.as_str()) {
                    let name = get(card, "name");
                    let character = get(ally, "character");
                    return Some(json!({
                        "reason": reason,
                        "card": if truthy(name) { name.clone() } else { json!(id) },
                        "character": if truthy(character) { character.clone() } else { json!("") },
                        "max_ms": WAIT_MS,
                    }));
                }
            }
        }
    }
    None
}

/// `apply_ally_wait`: attach [`ally_wait`] to a combat reply and name it in
/// the note.
#[must_use]
pub fn apply_ally_wait(state: &Value, advice: Value) -> Value {
    let Some(wait) = ally_wait(state, &advice) else {
        return advice;
    };
    let mut advice = advice;
    let hint = trf(
        "co-op: wait for the teammate's {card} ({reason})",
        &[
            ("card", &py::str_of(get(&wait, "card"))),
            ("reason", &py::str_of(get(&wait, "reason"))),
        ],
    );
    let note = get(&advice, "note");
    let note = if truthy(note) {
        format!("{} · {hint}", py::str_of(note))
    } else {
        hint
    };
    advice["wait_for_ally"] = wait;
    advice["note"] = json!(note);
    advice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attack_waits_for_the_teammates_bash() {
        let state = json!({
            "hand": [{"id": "STRIKE_IRONCLAD", "type": "Attack"}],
            "enemies": [{"hp": 10, "powers": []}],
            "allies": [
                {"character": "SILENT", "energy": 0, "hand": [{"id": "BASH", "cost": 2}]},
                {"character": "IRONCLAD", "energy": 3, "hand": [{"id": "bash", "cost": 2, "name": "Bash"}]},
            ],
        });
        let advice = apply_ally_wait(&state, json!({"best": {"card_index": 0}, "note": "x"}));
        assert_eq!(
            advice["wait_for_ally"],
            json!({"reason": "Vulnerable before our attack", "card": "Bash", "character": "IRONCLAD", "max_ms": 2500})
        );
        assert_eq!(
            advice["note"],
            "x · co-op: wait for the teammate's Bash (Vulnerable before our attack)"
        );
        // Every enemy already Vulnerable: nothing to wait for.
        let mut vuln = state.clone();
        vuln["enemies"][0]["powers"] = json!([{"id": "VULNERABLE_POWER"}]);
        assert_eq!(ally_wait(&vuln, &json!({"best": {"card_index": 0}})), None);
        // An end turn holds nothing.
        assert_eq!(
            ally_wait(&state, &json!({"best": {"action": "end_turn"}})),
            None
        );
    }
}
