//! Rank the in-combat card selections no trained head covers (GH #1426
//! step 3; was `realgame_hand_select.py`).
//!
//! `card_select` is answered by the run-policy net only for the
//! deck-mutation prompts (`TO_REMOVE` / `TO_TRANSFORM` / `TO_ENCHANT` /
//! `TO_UPGRADE` / `TO_OBTAIN`). An in-combat selection -- Burning Pact asking
//! which card to Exhaust, Gambler's Brew which to replace -- matches none of
//! them, and the advisor's fallback took the leftmost card: over the
//! 2026-08-06 ascension climb, 8 of 8 in-hand picks were index 0, including a
//! Burning Pact that exhausted a `Strike+` beside an unupgraded `Strike`
//! (`docs/experiments/2026-08-06-ascension-climb-issues.md` issue 5).
//!
//! This is deliberately NOT a net: it answers the one question the leftmost
//! default gets wrong most often -- *does the chosen card get hurt or
//! helped?* -- and picks the worst or the best card accordingly.
//!
//! Direction is data, not taste: every per-card prompt ships its English
//! text in `localization_eng` as `"<ID>.selectionScreenPrompt"`, and the
//! tables below are transcribed from those strings. A prompt that is not in
//! them stays UNKNOWN and keeps the leftmost default, with the note saying
//! so: a guess in the wrong direction is worse than an admitted arbitrary
//! pick.

use serde_json::{json, Value};

use crate::i18n::trf;
use crate::py;

/// Which way a selection points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The chosen card SUFFERS -- exhausted, discarded, removed, transformed
    /// away. Pick the worst card.
    Dispose,
    /// The chosen card BENEFITS -- copied, retained, drawn, made free. Pick
    /// the best card.
    Boost,
}

impl Direction {
    /// The Python module's constant (`"dispose"` / `"boost"`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dispose => "dispose",
            Self::Boost => "boost",
        }
    }
}

use Direction::{Boost, Dispose};

/// Keyed on `CardSelectorPrefs`' shared prompts (`CardSelectorPrefs.cs`).
/// The deck-mutation four are here for completeness even though the net's
/// card-select builder claims them first -- if it ever declines one (it
/// bails on any `max_select > 1`), the direction is still known.
pub const PROMPT_DIRECTION: [(&str, Direction); 6] = [
    ("TO_EXHAUST", Dispose),
    ("TO_DISCARD", Dispose),
    ("TO_REMOVE", Dispose),
    ("TO_TRANSFORM", Dispose),
    ("TO_UPGRADE", Boost),
    ("TO_ENCHANT", Boost),
];

/// Keyed on the source model id -- the `AbstractModel source` argument that
/// `CardSelectContextPatch` captures for the three `FromHand*` overloads,
/// and equally the stem of a `"<ID>.selectionScreenPrompt"` prompt key.
/// Transcribed from the English prompt text; the quoted string is that text.
///
/// Deliberately absent: SCULPTING_STRIKE ("add Ethereal") hands the card a
/// DRAWBACK in exchange for something else; DECISIONS_DECISIONS ("Decide a
/// Skill") auto-plays the pick, which is a play decision, not a value
/// ranking. Neither is a clean dispose-or-boost, so both stay UNKNOWN.
pub const SOURCE_DIRECTION: [(&str, Direction); 34] = [
    // -- the chosen card is destroyed / thrown away
    ("ASHWATER", Dispose),      // "Choose any number of cards to Exhaust."
    ("PURITY", Dispose),        // "Choose up to {N} cards to Exhaust."
    ("GAMBLERS_BREW", Dispose), // "Choose any number of cards to replace."
    ("GAMBLING_CHIP", Dispose), // "Choose any number of cards to replace."
    ("COMBAT_REWARD_CARD_REMOVAL", Dispose), // "...remove from your deck."
    ("GUARDS", Dispose),        // "Choose cards to Transform."
    ("CLAWS", Dispose),         // "Select cards to Transform into Maul."
    // -- the chosen card is duplicated / improved / retrieved
    ("DUAL_WIELD", Boost),        // "Choose a card to Copy."
    ("DOLLYS_MIRROR", Boost),     // "Select 1 card to Copy"
    ("NIGHTMARE", Boost),         // "Choose a Card." (copies it next turn)
    ("HAND_TRICK", Boost),        // "...add Sly to."
    ("SNAP", Boost),              // "...add Retain to."
    ("TRANSFIGURE", Boost),       // "...add Replay to."
    ("TOUCH_OF_INSANITY", Boost), // "Choose a card to make free."
    ("HEIRLOOM_HAMMER", Boost),   // "Choose a Colorless Card."
    ("SEA_GLASS", Boost),         // "...add to your Deck."
    // Put on top of the draw pile / back into hand: you want the card you
    // most want to see next, so the best one.
    ("COSMIC_INDIFFERENCE", Boost),
    ("GLIMMER", Boost),
    ("HEADBUTT", Boost),
    ("PHOTON_CUT", Boost),
    ("THINKING_AHEAD", Boost),
    ("GRAVEBLAST", Boost),
    ("HOLOGRAM", Boost),
    ("SEEKER_STRIKE", Boost),
    ("WISH", Boost),
    ("SECRET_TECHNIQUE", Boost),
    ("SECRET_WEAPON", Boost),
    ("DROPLET_OF_PRECOGNITION", Boost),
    ("LIQUID_MEMORIES", Boost),
    ("CHOICES_PARADOX", Boost),
    ("DREDGE", Boost),
    ("FOREGONE_CONCLUSION_POWER", Boost),
    ("STRATAGEM_POWER", Boost),
    ("WELL_LAID_PLANS_POWER", Boost),
];

/// What the picked cards of a selection undergo, as the verb of its
/// answer's set line ("Replace 2: Defend, Defend", GH #1770): the shared
/// prompts first, then the per-source prompts that take several cards.
/// Transcribed from the same `localization_eng` strings as the direction
/// tables; anything else is a plain "Pick".
pub const PROMPT_VERB: [(&str, &str); 6] = [
    ("TO_EXHAUST", "Exhaust"),
    ("TO_DISCARD", "Discard"),
    ("TO_REMOVE", "Remove"),
    ("TO_TRANSFORM", "Transform"),
    ("TO_UPGRADE", "Upgrade"),
    ("TO_ENCHANT", "Enchant"),
];

/// [`PROMPT_VERB`]'s per-source half, keyed like [`SOURCE_DIRECTION`].
pub const SOURCE_VERB: [(&str, &str); 14] = [
    ("ASHWATER", "Exhaust"),      // "Choose any number of cards to Exhaust."
    ("PURITY", "Exhaust"),        // "Choose up to {N} cards to Exhaust."
    ("GAMBLERS_BREW", "Replace"), // "Choose any number of cards to replace."
    ("GAMBLING_CHIP", "Replace"), // "Choose any number of cards to replace."
    ("GUARDS", "Transform"),      // "Choose cards to Transform."
    ("CLAWS", "Transform"),       // "Select cards to Transform into Maul."
    ("COMBAT_REWARD_CARD_REMOVAL", "Remove"),
    ("WELL_LAID_PLANS_POWER", "Retain"), // "Choose {N} cards to Retain."
    ("GLIMMER", "Put on top"),           // "Choose Cards to put on top of your Draw Pile"
    ("DREDGE", "Take"),                  // "Choose {N} cards to put into your Hand"
    ("NEOWS_FURY", "Take"),              // "Choose up to {N} cards to put into your Hand."
    ("FOREGONE_CONCLUSION_POWER", "Take"), // "...to add into your Hand."
    ("STRATAGEM_POWER", "Take"),         // "...to add into your Hand."
    ("SEA_GLASS", "Add"),                // "Select any number of cards to add to your Deck."
];

fn lookup<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table.iter().find(|(k, _)| *k == key).map(|(_, d)| *d)
}

/// The verb for what a `card_select`'s picked cards undergo ("Replace",
/// "Exhaust", ...), keyed as [`direction_of`] keys the direction: the
/// prompt, then its `<ID>` stem, then `source_model`. "Pick" when none
/// names it.
#[must_use]
pub fn verb_of(state: &Value) -> &'static str {
    let prompt = str_or_empty(state, "prompt_key");
    if let Some(v) = lookup(&PROMPT_VERB, &prompt.to_uppercase()) {
        return v;
    }
    let stem = prompt.split('.').next().unwrap_or("").to_uppercase();
    let source = str_or_empty(state, "source_model").to_uppercase();
    [stem, source]
        .iter()
        .filter(|k| !k.is_empty())
        .find_map(|k| lookup(&SOURCE_VERB, k))
        .unwrap_or("Pick")
}

/// `str(d.get(key) or "")`.
fn str_or_empty(d: &Value, key: &str) -> String {
    let v = py::get(d, key);
    if py::truthy(v) {
        py::str_of(v)
    } else {
        String::new()
    }
}

/// Python's `int(value)`, or `default` where it raises.
fn as_int(value: &Value, default: i64) -> i64 {
    match value {
        Value::Bool(b) => i64::from(*b),
        Value::Number(n) => n
            .as_i64()
            .or_else(|| {
                n.as_f64()
                    .filter(|f| f.is_finite())
                    .map(|f| f.trunc() as i64)
            })
            .unwrap_or(default),
        Value::String(s) => {
            let t = s.trim().replace('_', "");
            // `int("1_0")` is 10, but not a leading/trailing/doubled underscore.
            let ok_underscores =
                !s.trim().starts_with('_') && !s.trim().ends_with('_') && !s.contains("__");
            if ok_underscores {
                t.parse().unwrap_or(default)
            } else {
                default
            }
        }
        _ => default,
    }
}

/// Classify a `card_select` state: `(direction, key)` where `key` is
/// whatever named it, for the note; `None` direction is UNKNOWN.
/// `prompt_key` is authoritative: it survives `FromHandForDiscard`
/// delegating to `FromHand` (`CardSelectCmd.cs:763`, which overwrites the
/// captured `source` with the inner method name but leaves the prefs
/// untouched), so it is checked before `source_model`.
#[must_use]
pub fn direction_of(state: &Value) -> (Option<Direction>, String) {
    let prompt = str_or_empty(state, "prompt_key");
    let upper = prompt.to_uppercase();
    if let Some(d) = lookup(&PROMPT_DIRECTION, &upper) {
        return (Some(d), upper);
    }
    // "<ID>.selectionScreenPrompt" -> "<ID>" (CardModel.SelectionScreenPrompt).
    let stem = prompt.split('.').next().unwrap_or("").to_uppercase();
    let source = str_or_empty(state, "source_model");
    for key in [stem, source.to_uppercase()] {
        if !key.is_empty() {
            if let Some(d) = lookup(&SOURCE_DIRECTION, &key) {
                return (Some(d), key);
            }
        }
    }
    let key = if !prompt.is_empty() {
        prompt
    } else if !source.is_empty() {
        source
    } else {
        "?".into()
    };
    (None, key)
}

/// Rank a card from worthless to precious, worst first: class (curses,
/// statuses, basics, then by rarity), then upgrade level (the Strike+
/// bug: between two otherwise identical cards the unupgraded one goes),
/// then cost (cheapest first).
#[must_use]
pub fn card_value(card: &Value) -> (i64, i64, i64) {
    let rarity = str_or_empty(card, "rarity").to_lowercase();
    let ctype = str_or_empty(card, "type").to_lowercase();
    let klass = if py::truthy(py::get(card, "is_curse")) || ctype == "curse" || rarity == "curse" {
        0
    } else if py::truthy(py::get(card, "is_status")) || ctype == "status" || rarity == "status" {
        1
    } else {
        match rarity.as_str() {
            "basic" | "token" => 2,
            "uncommon" => 4,
            "rare" => 5,
            "ancient" => 6,
            _ => 3, // common, event, anything unknown
        }
    };
    (
        klass,
        as_int(py::get(card, "upgrade_level"), 0),
        as_int(py::get(card, "cost"), 0),
    )
}

/// Rank a `card_select` the nets do not cover, or `None` when its
/// direction is unknown (the caller keeps its leftmost fallback).
#[must_use]
pub fn rank(state: &Value) -> Option<Value> {
    let cards = py::list(state, "cards");
    if cards.is_empty() || !cards.iter().all(Value::is_object) {
        return None;
    }
    let (direction, key) = direction_of(state);
    let direction = direction?;
    let scored = cards.iter().map(card_value).enumerate();
    // Ties resolve to the LEFTMOST card either way, so the pick is
    // reproducible across a resumed run.
    let best_i = match direction {
        Dispose => scored.min_by_key(|(i, v)| (*v, *i)),
        Boost => scored.max_by_key(|(i, v)| (*v, std::cmp::Reverse(*i))),
    }
    .map_or(0, |(i, _)| i);
    let verb = if direction == Dispose {
        "worst"
    } else {
        "best"
    };
    let labels: Vec<String> = cards
        .iter()
        .enumerate()
        .map(|(i, c)| {
            ["name", "id"]
                .iter()
                .map(|k| py::get(c, k))
                .find(|v| py::truthy(v))
                .map_or_else(|| trf("option {i}", &[("i", &i)]), py::str_of)
        })
        .collect();
    let index = |i: usize| as_int(py::get(&cards[i], "index"), i as i64);
    let options: Vec<Value> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            json!({"label": label, "is_best": i == best_i, "index": index(i), "action": "choose"})
        })
        .collect();
    Some(json!({
        "note": trf(
            "no trained head for this selection ({key}); heuristic pick: the {verb} card by rarity, then upgrade, then cost",
            &[("key", &key), ("verb", &crate::i18n::tr_dyn(verb))],
        ),
        "options": options,
        "best": {"action": "choose", "index": index(best_i), "label": labels[best_i]},
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn state(prompt: &str, cards: &Value) -> Value {
        json!({"prompt_key": prompt, "cards": cards})
    }

    #[test]
    fn exhaust_takes_the_unupgraded_strike() {
        let cards = json!([
            {"name": "Strike+", "rarity": "Basic", "upgrade_level": 1, "index": 0},
            {"name": "Strike", "rarity": "Basic", "index": 1},
            {"name": "Dominate+", "rarity": "Rare", "upgrade_level": 1, "index": 2},
        ]);
        let got = rank(&state("TO_EXHAUST", &cards)).unwrap();
        assert_eq!(
            got["best"],
            json!({"action": "choose", "index": 1, "label": "Strike"})
        );
    }

    #[test]
    fn ties_go_to_the_leftmost_card_both_ways() {
        let cards = json!([{"name": "A"}, {"name": "B"}]);
        let pick = |p| rank(&state(p, &cards)).unwrap()["best"]["index"].clone();
        assert_eq!(pick("TO_DISCARD"), json!(0));
        assert_eq!(pick("DUAL_WIELD.selectionScreenPrompt"), json!(0));
    }

    #[test]
    fn an_unknown_prompt_is_not_ranked() {
        let cards = json!([{"name": "A"}]);
        assert_eq!(
            rank(&state("SCULPTING_STRIKE.selectionScreenPrompt", &cards)),
            None
        );
        assert_eq!(
            direction_of(&json!({"source_model": "x"})),
            (None, "x".into())
        );
        assert_eq!(
            direction_of(&json!({"source_model": "dual_wield"})).0,
            Some(Boost)
        );
    }

    #[test]
    fn a_selection_names_what_its_picks_undergo() {
        let verb = |prompt: &str, source: &str| {
            verb_of(&json!({"prompt_key": prompt, "source_model": source}))
        };
        assert_eq!(verb("GAMBLERS_BREW.selectionScreenPrompt", ""), "Replace");
        assert_eq!(verb("to_discard", "GAMBLERS_BREW"), "Discard");
        assert_eq!(verb("", "ashwater"), "Exhaust");
        assert_eq!(verb("SCULPTING_STRIKE.selectionScreenPrompt", ""), "Pick");
        assert_eq!(verb_of(&json!({})), "Pick");
    }

    #[test]
    fn values_read_like_python_int() {
        assert_eq!(
            card_value(&json!({"rarity": "Curse", "cost": "2"})),
            (0, 0, 2)
        );
        assert_eq!(
            card_value(&json!({"rarity": "Rare", "upgrade_level": 1.9, "cost": "x"})),
            (5, 1, 0)
        );
        assert_eq!(
            card_value(&json!({"is_status": 1, "cost": "1_0"})),
            (1, 0, 10)
        );
    }
}
