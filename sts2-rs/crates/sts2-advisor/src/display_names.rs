//! Player-facing names for game ids, and the wire guard that keeps ids off
//! the HUD (GH #1426 step 3).
//!
//! Port of `sts2-pycli/python/display_names.py`, whose module docstring has
//! the history. The mod's payloads carry both spellings of most things (a
//! card's `id` `SHRUG_IT_OFF` and its `name` `Shrug It Off`), and a field that
//! is an id on one surface put `SHRUG_IT_OFF` on the HUD. This is the rest:
//!
//! * [`Names::display_name`]: one id in any spelling (`SHRUG_IT_OFF`,
//!   `CARD.SHRUG_IT_OFF`, `SHRUG_IT_OFF.title`) to the game's English name;
//! * [`Names::humanize`]: every id-shaped token inside a display string
//!   replaced by its name (`remove:STRIKE_IRONCLAD` -> `Remove Strike`);
//! * [`Names::sanitize_advice`] / [`Names::sanitize_coach`]: the final guard
//!   at the wire boundary, over every human-facing string the advisor sends.
//!
//! The Python patterns lean on look-around, which the `regex` crate does not
//! have; the scanners below match exactly what those patterns match (each
//! documents its pattern), so the text the HUD shows is byte-identical.

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use crate::i18n::{is_zh, tr, trf};
use crate::py::{self, truthy};

/// Localization tables and the key suffix holding each entry's display
/// name, in lookup order (a card and a relic sharing an id resolve as the
/// card).
pub const TABLES: [(&str, &str); 11] = [
    ("cards", ".title"),
    ("relics", ".title"),
    ("potions", ".title"),
    ("monsters", ".name"),
    ("events", ".title"),
    ("enchantments", ".title"),
    ("powers", ".title"),
    ("orbs", ".title"),
    ("afflictions", ".title"),
    ("encounters", ".title"),
    ("characters", ".title"),
];

/// `advice_for` surfaces whose `best.label` the mod cross-checks against its
/// own option labels before clicking (`SimAdvisor.ReconcileIndex`).
pub const CROSS_CHECKED_SURFACES: [&str; 8] = [
    "card_reward",
    "card_select",
    "card_bundle",
    "boss_relic",
    "treasure",
    "rest_site",
    "potion_swap",
    "event",
];

/// Coach record keys holding display text.
const COACH_TEXT_KEYS: [&str; 7] = ["label", "you", "ai", "why", "hand", "name", "note"];

/// Registry prefixes an id may carry (`CARD.`).
const ID_PREFIXES: [&str; 8] = [
    "CARD",
    "RELIC",
    "POTION",
    "POWER",
    "MONSTER",
    "EVENT",
    "ENCHANTMENT",
    "ORB",
];

/// Verbs a shop-removal candidate spells as `remove:STRIKE`.
const VERBS: [&str; 5] = ["remove", "buy", "upgrade", "transform", "discard"];

/// Python's `\w`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Python's `str.title()`.
fn title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for c in s.chars() {
        let cased = c.is_lowercase() || c.is_uppercase();
        if cased {
            if prev_cased {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
        } else {
            out.push(c);
        }
        prev_cased = cased;
    }
    out
}

/// `re.sub(r"[^A-Z0-9]", "", raw.upper())`.
fn squash(raw: &str) -> String {
    raw.to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        .collect()
}

/// `re.fullmatch(r"[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]*)+", stem)`: engine
/// CamelCase (`ShrugItOff`).
fn is_camel(stem: &str) -> bool {
    let b: Vec<char> = stem.chars().collect();
    if b.is_empty() || !b[0].is_ascii_uppercase() {
        return false;
    }
    let lower = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    let mut i = 1;
    let start = i;
    while i < b.len() && lower(b[i]) {
        i += 1;
    }
    if i == start {
        return false;
    }
    let mut groups = 0;
    while i < b.len() {
        if !b[i].is_ascii_uppercase() {
            return false;
        }
        i += 1;
        while i < b.len() && lower(b[i]) {
            i += 1;
        }
        groups += 1;
    }
    groups > 0
}

/// `str(v or "")` for a value used as text.
fn text_of(v: &Value) -> String {
    if truthy(v) {
        py::str_of(v)
    } else {
        String::new()
    }
}

/// `s.strip().casefold()`, close enough: the labels are game text.
fn fold(s: &str) -> String {
    s.trim().to_lowercase()
}

/// The display-name tables, built once from the localization dumps.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_field_names)] // `names`: the field predates the twin
pub struct Names {
    /// `ID` (upper, `_`-separated) -> display name.
    names: BTreeMap<String, String>,
    /// The same names keyed without separators (`STRIKEIRONCLAD`).
    squashed: BTreeMap<String, String>,
    /// The game's Chinese names (`localization_zhs`), used while the
    /// advisor answers in Chinese ([`crate::i18n::is_zh`]); an id it does
    /// not know falls back to the English.
    zh: Option<Box<Names>>,
}

impl Names {
    /// Build from the localization tables in lookup order: for each
    /// directory (newest build first, the bundled one last), each of
    /// [`TABLES`] with its `(key, value)` entries in file order. The first
    /// definition of a name wins.
    pub fn new<'a>(tables: impl IntoIterator<Item = (&'a str, &'a [(String, String)])>) -> Self {
        let mut names: BTreeMap<String, String> = BTreeMap::new();
        // Insertion order, which `_squashed` walks: the first stem to squash
        // to a key wins.
        let mut order: Vec<String> = Vec::new();
        let mut define = |stem: String, value: &String| {
            if let std::collections::btree_map::Entry::Vacant(slot) = names.entry(stem) {
                order.push(slot.key().clone());
                slot.insert(value.clone());
            }
        };
        for (table, entries) in tables {
            let Some(suffix) = TABLES.iter().find(|(t, _)| *t == table).map(|(_, s)| *s) else {
                continue;
            };
            for (key, value) in entries {
                let Some(stem) = key.strip_suffix(suffix) else {
                    continue;
                };
                if stem.contains('.') || value.is_empty() || value.contains('{') {
                    continue;
                }
                let stem = stem.to_uppercase();
                let bare = (table == "powers")
                    .then(|| stem.strip_suffix("_POWER").map(str::to_owned))
                    .flatten();
                define(stem, value);
                if let Some(bare) = bare {
                    define(bare, value);
                }
            }
        }
        let mut squashed: BTreeMap<String, String> = BTreeMap::new();
        for stem in &order {
            squashed
                .entry(squash(stem))
                .or_insert_with(|| names[stem].clone());
        }
        Self {
            names,
            squashed,
            zh: None,
        }
    }

    /// These names with `zh` as their Chinese twin.
    #[must_use]
    pub fn with_zh(mut self, zh: Names) -> Self {
        self.zh = (!zh.is_empty()).then(|| Box::new(zh));
        self
    }

    /// The Chinese twin is loaded.
    #[must_use]
    pub fn has_zh(&self) -> bool {
        self.zh.is_some()
    }

    /// The name a table knows for `stem` (upper or squashed spelling), in
    /// the advisor's language.
    fn known(&self, stem: &str) -> Option<String> {
        let own = |n: &Self| {
            n.names
                .get(&stem.to_uppercase())
                .or_else(|| n.squashed.get(&squash(stem)))
                .cloned()
        };
        if is_zh() {
            if let Some(name) = self.zh.as_deref().and_then(own) {
                return Some(name);
            }
        }
        own(self)
    }

    /// How many ids have a name.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// No table loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The English name for a game id in any spelling; the id prettified
    /// when no table knows it (`SOME_NEW_CARD` -> `Some New Card`).
    #[must_use]
    pub fn display_name(&self, raw: &Value) -> String {
        self.display_name_str(&text_of(raw))
    }

    /// [`Self::display_name`] of a string.
    #[must_use]
    pub fn display_name_str(&self, raw: &str) -> String {
        let text = raw.trim();
        if text.is_empty() {
            return String::new();
        }
        let upgraded = text.ends_with('+');
        let text = text.trim_end_matches('+');
        let mut parts: Vec<&str> = text.split('.').collect();
        if parts.len() > 1 {
            let last = parts[parts.len() - 1].to_lowercase();
            if last == "title" || last == "name" {
                parts.pop();
            }
        }
        let stem = parts[parts.len() - 1];
        let name = self.known(stem).unwrap_or_else(|| {
            let spaced = if is_camel(stem) {
                let mut out = String::new();
                for (i, c) in stem.chars().enumerate() {
                    if i > 0 && c.is_ascii_uppercase() {
                        out.push(' ');
                    }
                    out.push(c);
                }
                out
            } else {
                stem.to_owned()
            };
            title(spaced.replace('_', " ").trim())
        });
        if upgraded {
            name + "+"
        } else {
            name
        }
    }

    /// `_upper_token`: a bare all-caps word is replaced only when a table
    /// knows it (and it is 3+ letters), so prose capitals (`NOT`, `MCTS`)
    /// stay; anything else id-shaped is named.
    fn upper_token(&self, whole: &str, stem: &str) -> String {
        if !stem.contains('_') && whole == stem {
            return match self.names.get(stem) {
                Some(name) if stem.chars().count() >= 3 => {
                    self.known(stem).unwrap_or_else(|| name.clone())
                }
                _ => whole.to_owned(),
            };
        }
        self.display_name_str(whole)
    }

    /// `text` with every id-shaped token replaced by its display name.
    /// Idempotent on display text: a name has no `_` and is not all caps.
    #[must_use]
    pub fn humanize(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        let out = sub_verb_prefix(text);
        let out = self.sub_upper_ids(&out);
        sub_snake(&out)
    }

    /// `humanize` over a value: strings are rewritten, the rest pass through.
    fn humanize_value(&self, v: &mut Value) {
        if let Value::String(s) = v {
            *s = self.humanize(s);
        }
    }

    /// `_UPPER_ID.sub(_upper_token, text)`, the pattern being
    /// `(?<![\w./\\-])(?:(?:CARD|RELIC|…|ORB)\.)?(?P<stem>[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)*)(?:\.(?:title|name))?(?![\w/\\])`.
    fn sub_upper_ids(&self, text: &str) -> String {
        let c: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < c.len() {
            let left_ok = i == 0 || {
                let p = c[i - 1];
                !(is_word(p) || matches!(p, '.' | '/' | '\\' | '-'))
            };
            if left_ok {
                if let Some((end, stem_range)) = match_upper_id(&c, i) {
                    let whole: String = c[i..end].iter().collect();
                    let stem: String = c[stem_range.0..stem_range.1].iter().collect();
                    out.push_str(&self.upper_token(&whole, &stem));
                    i = end;
                    continue;
                }
            }
            out.push(c[i]);
            i += 1;
        }
        out
    }

    /// A wire Card object's name: its `name`, else its id's display name.
    #[must_use]
    pub fn card_name(&self, card: &Value) -> String {
        let Value::Object(card) = card else {
            return "?".into();
        };
        if let Some(Value::String(name)) = card.get("name") {
            if !name.trim().is_empty() {
                return name.trim().to_owned();
            }
        }
        let mut label = self.display_name(py::mget(card, "id"));
        if label.is_empty() {
            label = "?".into();
        }
        if truthy(py::mget(card, "upgrade_level")) || truthy(py::mget(card, "upgraded")) {
            label.push('+');
        }
        label
    }

    /// A `shop` option as the player reads it: the item's name.
    #[must_use]
    pub fn shop_option_label(&self, option: &Value) -> String {
        let action = py::get(option, "action");
        match action.as_str() {
            Some("leave_shop") => return tr("Leave shop").into(),
            Some("remove_card") => return tr("Remove a card").into(),
            _ => {}
        }
        if let Value::Object(item) = py::get(option, "item") {
            if let Some(Value::String(name)) = item.get("name") {
                if !name.trim().is_empty() {
                    return name.trim().to_owned();
                }
            }
            let id = py::mget(item, "id");
            if truthy(id) {
                return self.display_name(id);
            }
        }
        let label = py::get(option, "label");
        let raw = if truthy(label) {
            py::str_of(label)
        } else if truthy(action) {
            py::str_of(action)
        } else {
            "?".into()
        };
        self.humanize(&raw)
    }

    /// A full-belt potion swap option (PROTOCOL.md v5.2) in words.
    #[must_use]
    pub fn potion_swap_label(&self, option: &Value, belt: &[Value]) -> String {
        let reward = match py::get(option, "reward") {
            r @ Value::Object(_) => r,
            _ => &Value::Null,
        };
        let new = self.or_name(py::get(reward, "name"), py::get(reward, "id"));
        let new = if new.is_empty() {
            tr("the new potion").to_owned()
        } else {
            new
        };
        let slot = py::get(option, "discard_slot");
        if slot.is_null() {
            return trf("Keep your potions (leave {new})", &[("new", &new)]);
        }
        let held = belt.iter().find(|p| {
            p.is_object() && py::eq(py::get(p, "slot"), slot) && !truthy(py::get(p, "empty"))
        });
        let old = if let Some(held) = held {
            self.or_name(py::get(held, "name"), py::get(held, "id"))
        } else {
            let raw = text_of(py::get(option, "label"));
            match raw.strip_prefix("discard ") {
                Some(rest) => self.display_name_str(rest),
                None => trf("slot {n}", &[("n", &py::str_of(slot))]),
            }
        };
        trf("Discard {old} for {new}", &[("old", &old), ("new", &new)])
    }

    /// `name or display_name(id)`.
    fn or_name(&self, name: &Value, id: &Value) -> String {
        if truthy(name) {
            py::str_of(name)
        } else {
            self.display_name(id)
        }
    }

    /// The final guard on every human-facing string of an `advice` object:
    /// `note`, `hand_back`, `instinct_unavailable`, `pv[]`,
    /// `options[].label`, `plan[].label` and `best.label`, except a
    /// `best.label` the takeover cross-checks (on a
    /// [`CROSS_CHECKED_SURFACES`] screen a label that IS one of the mod's
    /// own option labels stays as sent, or F6 / F7 would refuse the click).
    pub fn sanitize_advice(&self, advice: &mut Value, advice_for: Option<&str>, state: &Value) {
        let Value::Object(map) = advice else {
            return;
        };
        for key in ["note", "hand_back", "instinct_unavailable"] {
            if let Some(v) = map.get_mut(key) {
                self.humanize_value(v);
            }
        }
        if let Some(Value::Array(pv)) = map.get_mut("pv") {
            for v in pv {
                self.humanize_value(v);
            }
        }
        for key in ["options", "plan"] {
            if let Some(Value::Array(rows)) = map.get_mut(key) {
                for row in rows {
                    if let Some(label) = row.get_mut("label") {
                        self.humanize_value(label);
                    }
                }
            }
        }
        if let Some(Value::Object(best)) = map.get_mut("best") {
            if let Some(Value::String(label)) = best.get_mut("label") {
                let keep = advice_for.is_some_and(|a| CROSS_CHECKED_SURFACES.contains(&a))
                    && mod_labels(state).contains(&fold(label));
                if !keep {
                    *label = self.humanize(label);
                }
            }
        }
    }

    /// The same guard over a `coach` record (PROTOCOL.md §5 `coach`),
    /// recursively.
    pub fn sanitize_coach(&self, record: &mut Value) {
        match record {
            Value::Object(map) => {
                for (key, value) in map.iter_mut() {
                    let text_key = COACH_TEXT_KEYS.contains(&key.as_str());
                    match value {
                        Value::String(s) if text_key => *s = self.humanize(s),
                        Value::Array(items) if text_key => {
                            for v in items {
                                if let Value::String(s) = v {
                                    *s = self.humanize(s);
                                } else {
                                    self.sanitize_coach(v);
                                }
                            }
                        }
                        other => self.sanitize_coach(other),
                    }
                }
            }
            Value::Array(items) => {
                for v in items {
                    self.sanitize_coach(v);
                }
            }
            _ => {}
        }
    }
}

/// `_mod_labels`: the option labels the mod itself sent, folded.
fn mod_labels(state: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    for key in ["options", "cards", "bundles"] {
        for o in py::list(state, key) {
            if !o.is_object() {
                continue;
            }
            for field in ["label", "name"] {
                if let Some(Value::String(v)) = o.get(field) {
                    if !v.trim().is_empty() {
                        out.insert(fold(v));
                    }
                }
            }
        }
    }
    out
}

/// An id at `c[i..]`, the left context already checked: `(end, stem range)`.
/// The optional registry prefix is tried first and the suffix-less form
/// after the suffixed one, as the regex backtracks.
fn match_upper_id(c: &[char], i: usize) -> Option<(usize, (usize, usize))> {
    let right_ok = |j: usize| j >= c.len() || !(is_word(c[j]) || matches!(c[j], '/' | '\\'));
    let mut starts = Vec::with_capacity(2);
    for prefix in ID_PREFIXES {
        let n = prefix.len();
        if c.len() > i + n && c[i..i + n].iter().copied().eq(prefix.chars()) && c[i + n] == '.' {
            starts.push(i + n + 1);
        }
    }
    starts.push(i);
    for s in starts {
        let Some(stem_end) = scan_stem(c, s) else {
            continue;
        };
        for suffix in ["title", "name"] {
            let n = suffix.len();
            if c.len() >= stem_end + 1 + n
                && c[stem_end] == '.'
                && c[stem_end + 1..stem_end + 1 + n]
                    .iter()
                    .copied()
                    .eq(suffix.chars())
                && right_ok(stem_end + 1 + n)
            {
                return Some((stem_end + 1 + n, (s, stem_end)));
            }
        }
        if right_ok(stem_end) {
            return Some((stem_end, (s, stem_end)));
        }
    }
    None
}

/// `[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)*` at `s`, maximal: its end. Every stem
/// character is `\w`, so a shorter stem never satisfies the right context
/// the maximal one failed.
fn scan_stem(c: &[char], s: usize) -> Option<usize> {
    let up = |ch: char| ch.is_ascii_uppercase() || ch.is_ascii_digit();
    if s >= c.len() || !c[s].is_ascii_uppercase() {
        return None;
    }
    let mut j = s + 1;
    while j < c.len() && up(c[j]) {
        j += 1;
    }
    while j + 1 < c.len() && c[j] == '_' && up(c[j + 1]) {
        j += 2;
        while j < c.len() && up(c[j]) {
            j += 1;
        }
    }
    Some(j)
}

/// A [`VERBS`] entry in Chinese (the game's own words).
fn zh_verb(verb: &str) -> &'static str {
    match verb {
        "remove" => "移除",
        "buy" => "购买",
        "upgrade" => "升级",
        "transform" => "变化",
        _ => "丢弃",
    }
}

/// `re.sub(r"\b(remove|buy|upgrade|transform|discard):(?=\S)", …)`: the verb
/// capitalized and a space.
fn sub_verb_prefix(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    'scan: while i < c.len() {
        if i == 0 || !is_word(c[i - 1]) {
            for verb in VERBS {
                let n = verb.len();
                if c.len() > i + n + 1
                    && c[i..i + n].iter().copied().eq(verb.chars())
                    && c[i + n] == ':'
                    && !c[i + n + 1].is_whitespace()
                {
                    if is_zh() {
                        out.push_str(zh_verb(verb));
                    } else {
                        let mut chars = verb.chars();
                        if let Some(f) = chars.next() {
                            out.extend(f.to_uppercase());
                            out.extend(chars);
                        }
                    }
                    out.push(' ');
                    i += n + 1;
                    continue 'scan;
                }
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// `re.sub(r"(?<![\w./\\-])[a-z]+(?:_[a-z0-9]+)+(?![\w./\\])", words)`: a
/// machine verb in snake_case (`leave_shop`) to its words. Maximal munch is
/// the only candidate, for the reason [`scan_stem`] gives.
fn sub_snake(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let low = |ch: char| ch.is_ascii_lowercase() || ch.is_ascii_digit();
    let mut i = 0;
    while i < c.len() {
        let left_ok = i == 0 || {
            let p = c[i - 1];
            !(is_word(p) || matches!(p, '.' | '/' | '\\' | '-'))
        };
        if left_ok && c[i].is_ascii_lowercase() {
            let mut j = i + 1;
            while j < c.len() && c[j].is_ascii_lowercase() {
                j += 1;
            }
            let mut groups = 0;
            while j + 1 < c.len() && c[j] == '_' && low(c[j + 1]) {
                j += 2;
                while j < c.len() && low(c[j]) {
                    j += 1;
                }
                groups += 1;
            }
            let right_ok = j >= c.len() || !(is_word(c[j]) || matches!(c[j], '.' | '/' | '\\'));
            if groups > 0 && right_ok {
                out.extend(c[i..j].iter().map(|&ch| if ch == '_' { ' ' } else { ch }));
                i = j;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// A JSON object's string entries in FILE order (`serde_json`'s map sorts
/// its keys, and [`Names::new`] lets the first definition of a name win).
struct Entries(Vec<(String, String)>);

impl<'de> serde::Deserialize<'de> for Entries {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Entries;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut m: A,
            ) -> Result<Entries, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = m.next_entry::<String, Value>()? {
                    // Values may be lists/dicts in some files: strings only.
                    if let Value::String(s) = v {
                        out.push((k, s));
                    }
                }
                Ok(Entries(out))
            }
        }
        d.deserialize_map(V)
    }
}

/// `canonical_maps._load_json`: one localization table's string entries, in
/// file order; `<name>.json.gz` when the `.json` is absent (the packaged mod
/// ships them gzipped). Empty when missing or unreadable.
fn load_table(path: &std::path::Path) -> Vec<(String, String)> {
    use std::io::Read as _;
    let gz = path.with_file_name(format!(
        "{}.gz",
        path.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default()
    ));
    let text = if !path.is_file() && gz.is_file() {
        std::fs::File::open(&gz).ok().and_then(|f| {
            let mut s = String::new();
            flate2::read::GzDecoder::new(f)
                .read_to_string(&mut s)
                .ok()?;
            Some(s)
        })
    } else {
        std::fs::read_to_string(path).ok()
    };
    text.and_then(|t| serde_json::from_str::<Entries>(&t).ok())
        .map(|e| e.0)
        .unwrap_or_default()
}

/// `display_names._loc_dirs`: the newest vendored build first
/// (`localization_eng_v*`, a dev checkout), the bundled one last.
#[must_use]
pub fn loc_dirs(data_root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut newer: Vec<std::path::PathBuf> = std::fs::read_dir(data_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("localization_eng_v")
        })
        .map(|e| e.path())
        .collect();
    newer.sort();
    newer.reverse();
    newer.push(data_root.join("localization_eng"));
    newer
}

/// `display_names._names`: the tables over every localization directory
/// under `data_root`, with the game's Chinese tables (`localization_zhs/`,
/// when shipped) as their twin.
#[must_use]
pub fn load(data_root: &std::path::Path) -> Names {
    load_dirs(&loc_dirs(data_root)).with_zh(load_dirs(&[data_root.join("localization_zhs")]))
}

/// The tables over `dirs`, in order.
fn load_dirs(dirs: &[std::path::PathBuf]) -> Names {
    let tables: Vec<(&str, Vec<(String, String)>)> = dirs
        .iter()
        .filter(|d| d.is_dir())
        .flat_map(|d| {
            TABLES
                .iter()
                .map(move |(t, _)| (*t, load_table(&d.join(format!("{t}.json")))))
        })
        .collect();
    Names::new(tables.iter().map(|(t, e)| (*t, e.as_slice())))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn names() -> Names {
        let cards = vec![
            ("SHRUG_IT_OFF.title".to_owned(), "Shrug It Off".to_owned()),
            ("STRIKE_IRONCLAD.title".to_owned(), "Strike".to_owned()),
            ("BASH.title".to_owned(), "Bash".to_owned()),
            ("BAD.title".to_owned(), "Deal {Damage}".to_owned()),
        ];
        let powers = vec![("VIGOR_POWER.title".to_owned(), "Vigor".to_owned())];
        Names::new([("cards", cards.as_slice()), ("powers", powers.as_slice())])
    }

    #[test]
    fn ids_in_every_spelling_resolve() {
        let n = names();
        assert_eq!(n.display_name_str("CARD.SHRUG_IT_OFF"), "Shrug It Off");
        assert_eq!(n.display_name_str("SHRUG_IT_OFF.title+"), "Shrug It Off+");
        assert_eq!(n.display_name_str("StrikeIronclad"), "Strike");
        assert_eq!(n.display_name_str("VIGOR"), "Vigor");
        assert_eq!(n.display_name_str("SOME_NEW_CARD"), "Some New Card");
        assert_eq!(n.display_name_str("SomeNewCard"), "Some New Card");
        assert_eq!(n.display_name_str("BAD"), "Bad");
    }

    #[test]
    fn humanize_names_ids_and_keeps_prose() {
        let n = names();
        assert_eq!(n.humanize("remove:STRIKE_IRONCLAD"), "Remove Strike");
        assert_eq!(
            n.humanize("BASH, NOT a pick; leave_shop via MCTS"),
            "Bash, NOT a pick; leave shop via MCTS"
        );
        assert_eq!(
            n.humanize("path/SHRUG_IT_OFF stays"),
            "path/SHRUG_IT_OFF stays"
        );
        assert_eq!(n.humanize("CARD.BASH.title!"), "Bash!");
        assert_eq!(n.humanize("x.title_case"), "x.title_case");
    }

    #[test]
    fn a_cross_checked_best_label_the_mod_sent_is_kept() {
        let n = names();
        let state = json!({"options": [{"label": "SHRUG_IT_OFF"}]});
        let mut advice = json!({"best": {"label": "SHRUG_IT_OFF"}, "note": "BASH"});
        n.sanitize_advice(&mut advice, Some("card_reward"), &state);
        assert_eq!(advice["best"]["label"], json!("SHRUG_IT_OFF"));
        assert_eq!(advice["note"], json!("Bash"));
        n.sanitize_advice(&mut advice, Some("shop"), &state);
        assert_eq!(advice["best"]["label"], json!("Shrug It Off"));
    }

    #[test]
    fn title_is_pythons() {
        assert_eq!(title("it's 1st up"), "It'S 1St Up");
    }
}
