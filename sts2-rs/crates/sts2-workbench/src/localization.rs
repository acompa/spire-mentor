//! Resolve localization keys to display strings.
//!
//! Port of `sts2-pycli/python/workbench/localization.py` plus the loc-map
//! assembly `workbench.server.create_app` does around it. The engine emits
//! raw loc keys (`"STRIKE_IRONCLAD.title"`) and relies on display-time
//! enrichment; this module builds the flat `key -> string` map that
//! enrichment resolves against, and the helpers that read it.
//!
//! The Python loaders take file paths and probe the filesystem; here the
//! same functions take the already-parsed inputs (see [`crate::data`]) and
//! [`Loc::embedded`] reproduces `create_app`'s merge order exactly — the
//! same namespaced keys (`<id>.card_type`, `card_keyword.<Name>`,
//! `affliction.<ID>.<field>`, `rest_site.OPTION_<ID>.name`,
//! `<KEY>.description_plain`, …), the same `setdefault` vs overwrite
//! semantics, the same fallbacks.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use sts2_core::GameVersion;

use crate::data::{self, Data, LocMap, WikiCard, WikiMonster, WikiOwned};

pub use crate::data::{engine_wiki_version, ENGINE_WIKI_VERSION};

// ── The loc map ─────────────────────────────────────────────────────────

/// The flat merged localization map — the Python server's
/// `loc: dict[str, str]`.
///
/// Wraps a sorted map; the only place the Python dict's insertion order
/// could show is [`named_description_vars`]'s first-match scan, and the
/// shipped data has no two keys that collide there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Loc {
    map: LocMap,
}

impl Loc {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The map `create_app` builds over the default build's embedded
    /// bundle, built once. Same as `Loc::embedded_for(GameVersion::default())`.
    pub fn embedded() -> &'static Loc {
        Self::embedded_for(GameVersion::default())
    }

    /// The map `create_app` builds over build `version`'s embedded bundle
    /// ([`data::data_for`]), built once per build. Python twin:
    /// `workbench.localization.build_workbench_loc(..., version=...)`.
    ///
    /// With the display language set to Chinese ([`set_display_language`])
    /// and a zh bundle present, the map is the Chinese one ([`zh_overlay`]).
    pub fn embedded_for(version: GameVersion) -> &'static Loc {
        static ZH_CELLS: data::PerVersion<Loc> = data::PerVersion::new();
        if display_zh() {
            return ZH_CELLS.get_or_init(version, || {
                zh_overlay(data::data_for(version), data::zh_tables_for(version))
            });
        }
        Self::english_for(version)
    }

    /// The English map of build `version` whatever the display language: what
    /// art lookups keyed by a display NAME (relic / potion / event art) must
    /// read, since the Chinese map would hand them a Chinese name.
    pub fn english_for(version: GameVersion) -> &'static Loc {
        static CELLS: data::PerVersion<Loc> = data::PerVersion::new();
        CELLS.get_or_init(version, || build_loc(data::data_for(version)))
    }

    /// `loc.get(key)`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    /// `key in loc`.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// `loc[key] = value`, returning the previous value.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) -> Option<String> {
        self.map.insert(key.into(), value.into())
    }

    /// `loc.setdefault(key, value)` — insert only when `key` is absent.
    pub fn setdefault(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.map.entry(key.into()).or_insert_with(|| value.into());
    }

    /// `loc.update(other)` — every entry of `other` overwrites.
    pub fn update(&mut self, other: &Loc) {
        for (k, v) in &other.map {
            self.map.insert(k.clone(), v.clone());
        }
    }

    /// `(key, value)` pairs in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// True when there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The underlying map.
    #[must_use]
    pub fn as_map(&self) -> &LocMap {
        &self.map
    }

    /// Unwrap into the underlying map.
    #[must_use]
    pub fn into_map(self) -> LocMap {
        self.map
    }
}

impl From<LocMap> for Loc {
    fn from(map: LocMap) -> Self {
        Self { map }
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for Loc {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self {
            map: iter
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }
}

impl<K: Into<String>, V: Into<String>> Extend<(K, V)> for Loc {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.map.insert(k.into(), v.into());
        }
    }
}

impl<'a> IntoIterator for &'a Loc {
    type Item = (&'a String, &'a String);
    type IntoIter = std::collections::btree_map::Iter<'a, String, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

// ── Python string helpers ───────────────────────────────────────────────

/// Python `str.title()`: every cased character that follows a cased one is
/// lower-cased, every other cased character is upper-cased — so
/// `"neow's talisman"` -> `"Neow'S Talisman"`, exactly as CPython does it.
#[must_use]
pub fn py_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut previous_is_cased = false;
    for c in s.chars() {
        if previous_is_cased {
            out.extend(c.to_lowercase());
        } else {
            out.extend(c.to_uppercase());
        }
        previous_is_cased = c.is_uppercase() || c.is_lowercase();
    }
    out
}

/// Python `str.capitalize()`: first character upper-cased, the rest
/// lower-cased.
#[must_use]
pub fn py_capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut out: String = first.to_uppercase().collect();
            out.extend(chars.flat_map(char::to_lowercase));
            out
        }
    }
}

/// Python `re.match(pattern, s)` semantics for an anchored `^...$` pattern:
/// `$` also matches just before a single trailing newline.
fn strip_trailing_newline(s: &str) -> &str {
    s.strip_suffix('\n').unwrap_or(s)
}

// ── Loaders ─────────────────────────────────────────────────────────────

/// Build a `loc_key -> string` map for card titles and descriptions.
///
/// `wiki_cards` (the engine-pinned wiki manifest) goes in first and WINS;
/// `repo_entries` (the in-repo `localization_eng/cards.json` dump) only
/// fills keys the manifest has no value for. Both describe the same build
/// now, but the manifest carries the derived fields the raw dump has no
/// notion of (`description_plain`, per-var base values, keywords), and one
/// winner keeps template and plain text from coming out of different files.
///
/// Derived keys per manifest card `KEY`: `.title`, `.character`,
/// `.star_cost` (a number, or `"X"`), `.description` (the template),
/// `.description_plain`, `.upgraded_description_plain`,
/// `.upgrade_keywords` (`"+Innate, -Ethereal"`), `.keywords`
/// (comma-joined), and `.var.<type>` / `.uvar.<type>` base values.
#[must_use]
pub fn load_card_localization(repo_entries: &LocMap, wiki_cards: &[WikiCard]) -> Loc {
    let mut loc = Loc::new();
    for c in wiki_cards {
        let key = c.loc_key.as_str();
        if key.is_empty() {
            continue;
        }
        if let Some(title) = truthy(c.title.as_deref()) {
            loc.setdefault(format!("{key}.title"), title);
        }
        // The card's owning character drives cross-character art lookup.
        if let Some(character) = truthy(c.character.as_deref()) {
            loc.setdefault(format!("{key}.character"), character);
        }
        // Regent star cost — only the 21 cards with a `CanonicalStarCost`.
        if let Some(star) = c.star_cost {
            loc.setdefault(format!("{key}.star_cost"), star.to_string());
        }
        if c.x_star_cost == Some(true) {
            loc.setdefault(format!("{key}.star_cost"), "X");
        }
        if let Some(t) = truthy(c.description_template.as_deref()) {
            loc.setdefault(format!("{key}.description"), t);
        }
        // Pre-resolved plain descriptions (numbers already substituted).
        if let Some(p) = truthy(c.description_plain.as_deref()) {
            loc.setdefault(format!("{key}.description_plain"), p);
        }
        if let Some(p) = truthy(c.upgraded_description_plain.as_deref()) {
            loc.setdefault(format!("{key}.upgraded_description_plain"), p);
        }
        // Keyword-only upgrades (`add_keyword` / `remove_keyword`).
        let mut deltas: Vec<String> = Vec::new();
        for up in &c.upgrades {
            let Some(keyword) = truthy(up.keyword.as_deref()) else {
                continue;
            };
            match up.action.as_deref() {
                Some("add_keyword") => deltas.push(format!("+{keyword}")),
                Some("remove_keyword") => deltas.push(format!("-{keyword}")),
                _ => {}
            }
        }
        if !deltas.is_empty() {
            loc.setdefault(format!("{key}.upgrade_keywords"), deltas.join(", "));
        }
        // The card's own keywords (C# `CardModel.CanonicalKeywords`).
        if !c.keywords.is_empty() {
            loc.setdefault(format!("{key}.keywords"), c.keywords.join(","));
        }
        // Per-variable base values (`<KEY>.var.<type>` / `.uvar.<type>`).
        for var in &c.vars {
            let vtype = var.var_type.as_deref().unwrap_or("").to_lowercase();
            if vtype.is_empty() {
                continue;
            }
            if let Some(v) = var.base_value {
                loc.setdefault(format!("{key}.var.{vtype}"), v.to_string());
            }
            if let Some(v) = var.upgraded_value {
                loc.setdefault(format!("{key}.uvar.{vtype}"), v.to_string());
            }
        }
    }
    // Gap-filler only: the sole source for the cards the manifest dropped,
    // for every `<KEY>.selectionScreenPrompt`, and for the whole map when
    // no manifest is present.
    for (key, value) in repo_entries {
        loc.setdefault(key.clone(), value.clone());
    }
    loc
}

/// Python truthiness for an optional string: `None` and `""` are falsy.
fn truthy(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

/// `LargeCapsule` -> `LARGE_CAPSULE` (matches the wiki extractor).
///
/// Two passes, like the Python regexes: an underscore before every capital
/// that follows a lowercase letter or digit, then one before every capital
/// that follows another capital and starts a lowercase run (`ABCd` ->
/// `AB_CD`); then upper-cased.
#[must_use]
pub fn class_name_to_loc_key(class_name: &str) -> String {
    let pass1: Vec<char> = {
        let mut out = Vec::with_capacity(class_name.len() + 4);
        let mut prev: Option<char> = None;
        for c in class_name.chars() {
            if c.is_ascii_uppercase()
                && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
            {
                out.push('_');
            }
            out.push(c);
            prev = Some(c);
        }
        out
    };
    let mut out = String::with_capacity(pass1.len() + 4);
    for (i, &c) in pass1.iter().enumerate() {
        if c.is_ascii_uppercase()
            && i > 0
            && pass1[i - 1].is_ascii_uppercase()
            && pass1.get(i + 1).is_some_and(char::is_ascii_lowercase)
        {
            out.push('_');
        }
        out.push(c);
    }
    out.to_uppercase()
}

/// Build a `<LOC_KEY>.description_plain` map for relics and potions from
/// the wiki's per-item files, whose `description` already has every
/// template variable substituted with the item's base value. The
/// relic/potion analogue of a card's `description_plain`.
#[must_use]
pub fn load_owned_localization(items: &[WikiOwned]) -> Loc {
    let mut loc = Loc::new();
    for item in items {
        if item.class_name.is_empty() || item.description.is_empty() {
            continue;
        }
        loc.setdefault(
            format!(
                "{}.description_plain",
                class_name_to_loc_key(&item.class_name)
            ),
            item.description.clone(),
        );
    }
    loc
}

/// Fill any `{SomethingDescription}` placeholder from the item it names.
///
/// Tea Master's `"Pay {BoneTeaCost} Gold. {BoneTeaDescription}"` prints the
/// Bone Tea RELIC's description: strip `Description` and the rest is the
/// item's loc key (`BoneTea` -> `BONE_TEA`), matched case-insensitively
/// against the `<KEY>.description_plain` entries in `loc`.
///
/// `needed` / `supplied` are lowercased var names. Returns
/// `{var_name: text}` for the ones that resolved; missing items are left
/// out so the caller still falls back.
#[must_use]
pub fn named_description_vars(
    needed: &BTreeSet<String>,
    supplied: &BTreeSet<String>,
    loc: &Loc,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for name in needed.difference(supplied) {
        let Some(stem) = name.strip_suffix("description") else {
            continue;
        };
        if stem.is_empty() {
            continue;
        }
        for (key, value) in loc.iter() {
            let Some(item) = key.strip_suffix(".description_plain") else {
                continue;
            };
            if item.replace('_', "").to_lowercase() == stem {
                out.insert(name.clone(), value.to_string());
                break;
            }
        }
    }
    out
}

/// A relic/potion's pre-resolved plain description, or None.
#[must_use]
pub fn plain_owned_description<'a>(item_id: Option<&str>, loc: &'a Loc) -> Option<&'a str> {
    let item_id = truthy(item_id)?;
    loc.get(&format!("{item_id}.description_plain"))
}

/// The real category / rarity / printed costs of one card, from the wiki
/// manifest (`load_card_meta`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CardMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rarity: Option<String>,
    /// Display-ready: `"X"` for variable cost (the manifest's `-1`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    /// Only for the cards whose upgrade changes the cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgraded_cost: Option<String>,
}

impl CardMeta {
    fn is_empty(&self) -> bool {
        self.card_type.is_none()
            && self.rarity.is_none()
            && self.cost.is_none()
            && self.upgraded_cost.is_none()
    }
}

fn display_cost(cost: i64) -> String {
    if cost < 0 {
        "X".to_string()
    } else {
        cost.to_string()
    }
}

/// `loc_key -> CardMeta` from the wiki manifest. The engine ships every
/// card with a generic `type: "card"` and no rarity, so the real category
/// and rarity are looked up by the card's id, which equals `loc_key`.
#[must_use]
pub fn load_card_meta(wiki_cards: &[WikiCard]) -> BTreeMap<String, CardMeta> {
    let mut meta = BTreeMap::new();
    for c in wiki_cards {
        if c.loc_key.is_empty() {
            continue;
        }
        let entry = CardMeta {
            card_type: truthy(c.card_type.as_deref()).map(str::to_string),
            rarity: truthy(c.rarity.as_deref()).map(str::to_string),
            cost: c.energy_cost.map(display_cost),
            upgraded_cost: c.upgraded_cost.map(display_cost),
        };
        if !entry.is_empty() {
            meta.insert(c.loc_key.clone(), entry);
        }
    }
    meta
}

// ── Card keywords (C# `CardKeywordOrder` / `CardKeywordExtensions`) ────

/// `CardKeywordOrder.beforeDescription`, in DECLARATION order. The game
/// walks this array and `list.Insert(0, ...)` on each hit, so what a card
/// actually shows is this order REVERSED.
pub const KEYWORDS_BEFORE_DESCRIPTION: [&str; 5] =
    ["Ethereal", "Sly", "Retain", "Innate", "Unplayable"];
/// `CardKeywordOrder.afterDescription` — appended below the description.
pub const KEYWORDS_AFTER_DESCRIPTION: [&str; 2] = ["Exhaust", "Eternal"];

/// Build a `card_keyword.<Name> -> display title` map from the game's
/// `card_keywords.json`, falling back to the keyword's own name.
#[must_use]
pub fn load_card_keyword_localization(entries: &LocMap) -> Loc {
    let mut titles = Loc::new();
    for keyword in KEYWORDS_BEFORE_DESCRIPTION
        .iter()
        .chain(KEYWORDS_AFTER_DESCRIPTION.iter())
    {
        let title = entries
            .get(&format!("{}.title", keyword.to_uppercase()))
            .map(String::as_str)
            .filter(|t| !t.is_empty())
            .unwrap_or(keyword);
        titles.insert(format!("card_keyword.{keyword}"), title);
    }
    titles
}

/// Afflictions that PRINT on the card face (`AfflictionModel.
/// HasExtraCardText` is set by exactly these four in v0.107.1).
pub const AFFLICTIONS_WITH_CARD_TEXT: [&str; 4] = ["BOUND", "GALVANIZED", "RINGING", "TAINTED"];

/// Build an `affliction.<ID>.<field> -> string` map (`title` and
/// `extraCardText`) for the afflictions the game prints on a card face.
#[must_use]
pub fn load_affliction_localization(entries: &LocMap) -> Loc {
    let mut out = Loc::new();
    for aff in AFFLICTIONS_WITH_CARD_TEXT {
        for field in ["title", "extraCardText"] {
            if let Some(value) = entries
                .get(&format!("{aff}.{field}"))
                .filter(|v| !v.is_empty())
            {
                out.insert(format!("affliction.{aff}.{field}"), value.clone());
            }
        }
    }
    out
}

/// The `RestSiteOption` subclasses' `OptionId`s: v0.111.0's nine, then
/// v0.113.0's `EVOLVE` (Byrdpip) and `RESTORE` (Wuppy). An id a build's
/// dump lacks falls back to its title-cased id.
pub const REST_SITE_OPTION_IDS: [&str; 11] = [
    "HEAL", "SMITH", "DIG", "LIFT", "COOK", "CLONE", "HATCH", "KINDLE", "MEND", "EVOLVE", "RESTORE",
];

/// Build a `rest_site.OPTION_<ID>.name -> display title` map from the
/// game's `rest_site_ui.json`. Every known option id gets an entry —
/// falling back to the title-cased id.
#[must_use]
pub fn load_rest_site_localization(entries: &LocMap) -> Loc {
    let mut out = Loc::new();
    for opt in REST_SITE_OPTION_IDS {
        let title = entries
            .get(&format!("OPTION_{opt}.name"))
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| py_title(opt));
        out.insert(format!("rest_site.OPTION_{opt}.name"), title);
    }
    out
}

/// The extra-card-text line an afflicted card prints, markup included
/// (`"[purple]Bound[/purple]"`). None for an unafflicted card, and for an
/// affliction the game shows no text for. `{Amount}` is left in the
/// template for the caller to resolve alongside the description.
#[must_use]
pub fn affliction_card_line(affliction: Option<&str>, loc: &Loc) -> Option<String> {
    let affliction = truthy(affliction)?;
    let key = format!(
        "affliction.{}.extraCardText",
        affliction.trim().to_uppercase()
    );
    loc.get(&key)
        .filter(|t| !t.is_empty())
        .map(|text| format!("[purple]{text}[/purple]"))
}

/// The keywords a card actually has right now (C# `CardModel.Keywords`):
/// canonical keywords plus/minus whatever its `OnUpgrade` adds or removes.
#[must_use]
pub fn card_keywords(card_id: Option<&str>, upgraded: bool, loc: &Loc) -> BTreeSet<String> {
    let Some(card_id) = truthy(card_id) else {
        return BTreeSet::new();
    };
    let mut keywords: BTreeSet<String> = loc
        .get(&format!("{card_id}.keywords"))
        .unwrap_or("")
        .split(',')
        .filter(|k| !k.is_empty())
        .map(str::to_string)
        .collect();
    if upgraded {
        for delta in loc
            .get(&format!("{card_id}.upgrade_keywords"))
            .unwrap_or("")
            .split(',')
        {
            let delta = delta.trim();
            if let Some(k) = delta.strip_prefix('+') {
                keywords.insert(k.to_string());
            } else if let Some(k) = delta.strip_prefix('-') {
                keywords.remove(k);
            }
        }
    }
    keywords
}

/// `(lines above the description, lines below it)` for a card, each line
/// one keyword rendered the way the game renders it: `"[gold]Exhaust[/gold]."`.
#[must_use]
pub fn keyword_card_lines(
    card_id: Option<&str>,
    upgraded: bool,
    loc: &Loc,
) -> (Vec<String>, Vec<String>) {
    let have = card_keywords(card_id, upgraded, loc);
    if have.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let line = |keyword: &str| {
        let title = loc
            .get(&format!("card_keyword.{keyword}"))
            .unwrap_or(keyword);
        format!("[gold]{title}[/gold].")
    };
    let before = KEYWORDS_BEFORE_DESCRIPTION
        .iter()
        .rev()
        .filter(|k| have.contains(**k))
        .map(|k| line(k))
        .collect();
    let after = KEYWORDS_AFTER_DESCRIPTION
        .iter()
        .filter(|k| have.contains(**k))
        .map(|k| line(k))
        .collect();
    (before, after)
}

// ── Resolvers ───────────────────────────────────────────────────────────

/// `loc[value]` if `value` is a known loc key, else `value` (`""` for None).
#[must_use]
pub fn resolve(value: Option<&str>, loc: &Loc) -> String {
    match truthy(value) {
        None => String::new(),
        Some(v) => loc.get(v).unwrap_or(v).to_string(),
    }
}

/// A card's pre-resolved plain description, or None. Falls back to the
/// unupgraded text when the upgraded variant is missing.
#[must_use]
pub fn plain_description<'a>(
    card_id: Option<&str>,
    upgraded: bool,
    loc: &'a Loc,
) -> Option<&'a str> {
    let card_id = truthy(card_id)?;
    let base = || loc.get(&format!("{card_id}.description_plain"));
    if upgraded {
        return match loc.get(&format!("{card_id}.upgraded_description_plain")) {
            Some(up) if !up.is_empty() => Some(up),
            _ => base(),
        };
    }
    base()
}

/// A card variable's base value for display, or None if unknown.
///
/// The manifest stores power variables with the `Power` suffix stripped
/// (`VigorPower` -> `vigor`); try the bare name first, then the stripped
/// form. Prefers the upgraded value when `upgraded`.
#[must_use]
pub fn card_var_value<'a>(
    card_id: Option<&str>,
    var_name: &str,
    upgraded: bool,
    loc: &'a Loc,
) -> Option<&'a str> {
    let card_id = truthy(card_id)?;
    if var_name.is_empty() {
        return None;
    }
    let name = var_name.to_lowercase();
    let mut candidates = vec![name.clone()];
    if let Some(stem) = name.strip_suffix("power") {
        candidates.push(stem.to_string());
    }
    for cand in &candidates {
        if upgraded {
            if let Some(val) = loc.get(&format!("{card_id}.uvar.{cand}")) {
                return Some(val);
            }
        }
        if let Some(val) = loc.get(&format!("{card_id}.var.{cand}")) {
            return Some(val);
        }
    }
    None
}

/// `^[A-Z0-9][A-Z0-9_]*\.(title|name|description|flavor)$` — a loc key the
/// game/backend leaks when our bundled loc lacks an entry.
fn is_leaked_loc_key(value: &str) -> bool {
    let value = strip_trailing_newline(value);
    let Some((stem, field)) = value.split_once('.') else {
        return false;
    };
    let mut chars = stem.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_uppercase() || first.is_ascii_digit()) {
        return false;
    }
    if !chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return false;
    }
    matches!(field, "title" | "name" | "description" | "flavor")
}

/// Turn a leaked loc key like `WINGED_BOOTS.title` into `Winged Boots`.
fn prettify_loc_key(value: &str) -> String {
    let stem = value.split_once('.').map_or(value, |(s, _)| s);
    py_title(&stem.replace('_', " "))
}

/// True for a leaked dotted loc key (`X.pages.Y.description`), not prose:
/// no spaces, two or more `[A-Za-z0-9_]+` segments joined by dots.
#[must_use]
pub fn looks_like_unresolved_key(value: Option<&str>) -> bool {
    let Some(value) = truthy(value) else {
        return false;
    };
    let value = strip_trailing_newline(value);
    let mut segments = 0;
    for seg in value.split('.') {
        if seg.is_empty() || !seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
        segments += 1;
    }
    segments >= 2
}

/// Resolve a display name, prettifying any still-unresolved loc key.
#[must_use]
pub fn resolve_name(name: Option<&str>, loc: &Loc) -> String {
    let resolved = resolve(name, loc);
    if is_leaked_loc_key(&resolved) {
        prettify_loc_key(&resolved)
    } else {
        resolved
    }
}

/// Resolve an option's display label, prettifying an unresolved key.
///
/// Same as [`resolve_name`] except for a rest-site option: the generic
/// prettifier keeps only the segment before the first dot, which would turn
/// `rest_site.OPTION_DIG.name` into the useless "Rest Site" for every option
/// alike. The option id is the informative part, so prettify that instead.
fn resolve_option_label(raw: &str, loc: &Loc) -> String {
    let resolved = resolve(Some(raw), loc);
    // `is_leaked_loc_key` only recognises an UPPERCASE stem, so a rest-site
    // key slips past it and would be printed verbatim; catch it here.
    if let Some(id) = resolved
        .strip_prefix("rest_site.OPTION_")
        .and_then(|rest| rest.strip_suffix(".name"))
        .filter(|id| !id.is_empty())
    {
        return py_title(&id.replace('_', " ").to_lowercase());
    }
    resolve_name(Some(&resolved), loc)
}

/// Resolve the `name` of every dict in a list, in place.
fn resolve_names_in_list(list: Option<&mut Value>, loc: &Loc) {
    let Some(Value::Array(items)) = list else {
        return;
    };
    for item in items {
        let Value::Object(obj) = item else {
            continue;
        };
        let Some(name) = obj.get("name") else {
            continue;
        };
        let resolved = match name {
            Value::String(s) => resolve_name(Some(s), loc),
            Value::Null => resolve_name(None, loc),
            // Python would raise on a non-string name; leave it be.
            _ => continue,
        };
        obj.insert("name".to_string(), Value::String(resolved));
    }
}

/// [`resolve_state_names`], mutating `state` in place.
pub fn resolve_state_names_in_place(state: &mut Value, loc: &Loc) {
    let Value::Object(out) = state else {
        return;
    };
    // `relics`/`potions` at the top level are the shop / treasure offerings
    // (the persistent player trays are handled below under `player`);
    // `rewards` is the Blitz chest's relics (GH #1347).
    for key in [
        "hand",
        "cards",
        "draw_pile",
        "discard_pile",
        "exhaust_pile",
        "enemies",
        "relics",
        "potions",
        "rewards",
    ] {
        resolve_names_in_list(out.get_mut(key), loc);
    }
    if let Some(Value::Object(player)) = out.get_mut("player") {
        for tray in ["deck", "relics", "potions"] {
            resolve_names_in_list(player.get_mut(tray), loc);
        }
    }
    if let Some(Value::Array(options)) = out.get_mut("options") {
        for opt in options {
            let Value::Object(obj) = opt else {
                continue;
            };
            // `title` is the generic screens' label; `name` is what a REST
            // SITE carries (`rest_site.OPTION_SMITH.name`, built in
            // `game.rs`), and `describe_action` prefers `name` over `title`.
            // Resolving only `title` here logged the raw key: "Choose:
            // rest_site.OPTION_SMITH.name".
            for field in ["title", "name"] {
                let Some(Value::String(raw)) = obj.get(field) else {
                    continue;
                };
                if raw.is_empty() {
                    continue;
                }
                let resolved = resolve_option_label(raw, loc);
                obj.insert(field.to_string(), Value::String(resolved));
            }
        }
    }
}

/// A copy of `state` with card/enemy/option display names resolved, so a
/// history/event log built off the raw state reads `Play Strike` rather
/// than `Play STRIKE_IRONCLAD.title`. `None` / an empty state yields `{}`.
#[must_use]
pub fn resolve_state_names(state: Option<&Value>, loc: &Loc) -> Value {
    match state {
        None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
        Some(v) => {
            let mut out = v.clone();
            resolve_state_names_in_place(&mut out, loc);
            out
        }
    }
}

// ── Monster HP bands ────────────────────────────────────────────────────

/// A monster's initial-HP band: the base band and the one that applies
/// from `hp_ascension` upward.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct HpRange {
    pub min: i64,
    pub max: i64,
    pub min_asc: i64,
    pub max_asc: i64,
    pub hp_ascension: i64,
}

/// `MonsterKind -> HpRange` from the wiki manifest. `Creature.
/// SetUniqueMonsterHpValue` rolls each monster's max HP inside
/// `[MinInitialHp, MaxInitialHp]` and the combat room sizes the creature by
/// WHERE in that band it landed; no wire carries the band.
///
/// The manifest omits a bound that equals its partner: a fixed-HP monster
/// carries `min_hp_base` alone, and a monster with no ascension bump
/// carries no base pair at all. Filling both from what IS there keeps every
/// monster in the map.
#[must_use]
pub fn load_monster_hp_ranges(rows: &[WikiMonster]) -> BTreeMap<String, HpRange> {
    let mut out = BTreeMap::new();
    for row in rows {
        let (asc_lo, asc_hi) = (row.min_hp, row.max_hp);
        let base_lo = row.min_hp_base.or(asc_lo);
        let base_hi = row.max_hp_base.or(base_lo);
        let (Some(base_lo), Some(base_hi)) = (base_lo, base_hi) else {
            continue;
        };
        out.insert(
            row.class_name.clone(),
            HpRange {
                min: base_lo,
                max: base_hi,
                min_asc: asc_lo.unwrap_or(base_lo),
                max_asc: asc_hi.unwrap_or(base_hi),
                hp_ascension: row.hp_ascension.unwrap_or(0),
            },
        );
    }
    out
}

/// The HP bands over the default build's embedded bundle, built once.
pub fn embedded_monster_hp_ranges() -> &'static BTreeMap<String, HpRange> {
    embedded_monster_hp_ranges_for(GameVersion::default())
}

/// The HP bands over build `version`'s embedded bundle, built once per
/// build.
pub fn embedded_monster_hp_ranges_for(version: GameVersion) -> &'static BTreeMap<String, HpRange> {
    static CELLS: data::PerVersion<BTreeMap<String, HpRange>> = data::PerVersion::new();
    CELLS.get_or_init(version, || {
        load_monster_hp_ranges(&data::data_for(version).wiki_monsters)
    })
}

// ── `create_app`'s bundle ───────────────────────────────────────────────

/// The loc map `workbench.server.create_app` builds, in its exact merge
/// order:
///
/// 1. [`load_card_localization`] (wiki manifest wins, in-repo dump fills);
/// 2. `update` with [`load_card_keyword_localization`],
///    [`load_affliction_localization`], [`load_rest_site_localization`];
/// 3. `setdefault` `<id>.card_type` / `.rarity` / `.cost` /
///    `.upgraded_cost` from [`load_card_meta`];
/// 4. `update` with `relics.json` then `potions.json` (raw dumps);
/// 5. `setdefault` every `characters.json` entry;
/// 6. `setdefault` every [`load_owned_localization`] entry;
/// 7. `setdefault` every `events.json` entry (an event id can collide with
///    the relic it grants, and the relic name must win).
#[must_use]
pub fn build_loc(d: &Data) -> Loc {
    let mut card_loc = load_card_localization(&d.loc_cards, &d.wiki_cards);
    card_loc.update(&load_card_keyword_localization(&d.loc_card_keywords));
    card_loc.update(&load_affliction_localization(&d.loc_afflictions));
    card_loc.update(&load_rest_site_localization(&d.loc_rest_site_ui));
    for (key, meta) in load_card_meta(&d.wiki_cards) {
        if let Some(t) = meta.card_type.filter(|t| !t.is_empty()) {
            card_loc.setdefault(format!("{key}.card_type"), t);
        }
        if let Some(r) = meta.rarity.filter(|r| !r.is_empty()) {
            card_loc.setdefault(format!("{key}.rarity"), r);
        }
        if let Some(c) = meta.cost {
            card_loc.setdefault(format!("{key}.cost"), c);
        }
        if let Some(c) = meta.upgraded_cost {
            card_loc.setdefault(format!("{key}.upgraded_cost"), c);
        }
    }
    card_loc.extend(d.loc_relics.iter().map(|(k, v)| (k.clone(), v.clone())));
    card_loc.extend(d.loc_potions.iter().map(|(k, v)| (k.clone(), v.clone())));
    for (key, value) in &d.loc_characters {
        card_loc.setdefault(key.clone(), value.clone());
    }
    for (key, value) in load_owned_localization(&d.wiki_owned).iter() {
        card_loc.setdefault(key, value);
    }
    for (key, value) in &d.loc_events {
        card_loc.setdefault(key.clone(), value.clone());
    }
    // Encounter titles only (`VANTOM_BOSS.title`): the seed checker names
    // bosses with them; their `.loss` lines are not display names.
    for (key, value) in &d.loc_encounters {
        if key
            .rsplit_once('.')
            .is_some_and(|(_, field)| field == "title")
        {
            card_loc.setdefault(key.clone(), value.clone());
        }
    }
    card_loc
}

// ── Display language ────────────────────────────────────────────────────

static DISPLAY_ZH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set the language game text is DISPLAYED in: `"zh"` (any `zh*` tag)
/// switches [`Loc::embedded_for`] to the Chinese map when the bundle carries
/// one; anything else is English. Returns whether Chinese is now on.
///
/// Call it once, before the first request: the maps are built lazily and
/// cached, and every cache derived from them keeps whatever it saw first
/// (the browser engine sets it from its init config, `Workbench::new`).
/// Only display text changes: everything that maps a NAME back to an id
/// (`combat_import`, `display_maps::power_title_to_id`) reads the raw
/// English tables in [`crate::data`], never this map.
pub fn set_display_language(lang: &str) -> bool {
    let zh = lang.trim().to_ascii_lowercase().starts_with("zh") && data::zh_tables().is_present();
    DISPLAY_ZH.store(zh, std::sync::atomic::Ordering::Relaxed);
    zh
}

/// The game's Chinese name for an item given by its ENGLISH title (a relic,
/// potion, card or monster an engine message names in English, e.g. an
/// event's `{Potion}` var): the title mapped back to its id, then the
/// Chinese table read by id. `None` when it is not a known title.
#[must_use]
pub fn zh_name_for_english(english: &str) -> Option<&'static str> {
    static MAP: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    let map = MAP.get_or_init(|| {
        let zh = data::zh_tables();
        let mut out = BTreeMap::new();
        for v in GameVersion::ALL {
            let d = data::data_for(v);
            for (eng, zh_table) in [
                (&d.loc_relics, &zh.relics),
                (&d.loc_potions, &zh.potions),
                (&d.loc_cards, &zh.cards),
                (
                    &d.loc_monsters,
                    zh.extra.get("monsters").unwrap_or(&zh.cards),
                ),
            ] {
                for (key, title) in eng {
                    let is_name = key
                        .rsplit_once('.')
                        .is_some_and(|(_, f)| f == "title" || f == "name");
                    if let (true, Some(z)) = (is_name, zh_table.get(key)) {
                        if !z.is_empty() {
                            out.entry(title.clone()).or_insert_with(|| z.clone());
                        }
                    }
                }
            }
        }
        // Card rarities and types (an event's `{Rarity}` / `{Type}` var reads
        // `Common` / `Skill`): the game's `CARD_RARITY.*` / `CARD_TYPE.*`.
        if let Some(rows) = zh.extra.get("gameplay_ui") {
            for (key, z) in rows {
                let word = key
                    .strip_prefix("CARD_RARITY.")
                    .or_else(|| key.strip_prefix("CARD_TYPE."));
                if let (Some(w), false) = (word, z.is_empty()) {
                    out.entry(py_title(&w.to_lowercase()))
                        .or_insert_with(|| z.clone());
                }
            }
        }
        // Enchantments: no English table is bundled; their English title is
        // the id title-cased (`SHARP` -> `Sharp`, `SOULS_POWER` -> `Souls Power`).
        if let Some(rows) = zh.extra.get("enchantments") {
            for (key, z) in rows {
                if let Some(stem) = key.strip_suffix(".title") {
                    let eng = py_title(&stem.replace('_', " ").to_lowercase());
                    if !z.is_empty() {
                        out.entry(eng).or_insert_with(|| z.clone());
                    }
                }
            }
        }
        out
    });
    map.get(english).map(String::as_str)
}

/// True when game text displays in Chinese ([`set_display_language`]).
#[must_use]
pub fn display_zh() -> bool {
    DISPLAY_ZH.load(std::sync::atomic::Ordering::Relaxed)
}

/// Overwrite `dst`'s entries with `zh`'s where both have the key: a key the
/// Chinese tables lack stays English, and the overlay adds nothing a build
/// does not have (the zh set comes from the newest build).
fn overlay_existing(dst: &mut LocMap, zh: &LocMap) {
    for (k, v) in zh {
        if let Some(slot) = dst.get_mut(k) {
            if !v.is_empty() {
                v.clone_into(slot);
            }
        }
    }
}

/// `true` when `text` still holds an unfilled `{Var}` placeholder.
fn has_placeholder(text: &str) -> bool {
    text.contains('{')
}

/// Render `template` with `stats` the way a card face out of combat does
/// (markup kept, as the wiki's `description_plain` carries it).
fn render_plain(template: &str, stats: &serde_json::Map<String, Value>, upgraded: bool) -> String {
    crate::render::resolve_card_text(
        Some(template),
        Some(&Value::Object(stats.clone())),
        crate::render::CardTextFlags {
            in_combat: false,
            upgraded,
            extra_flags: None,
            keep_markup: true,
        },
    )
}

/// Spell the `[energy]N[/energy]` / `[star]N[/star]` glyph runs out the way
/// the game's Chinese text writes them ("获得2点能量"), for a relic / potion
/// text the page draws without the card frame's glyphs. A template that
/// already writes the count ("1点{Energy:energyIcons()}") keeps one count.
fn spell_icons_zh(text: &str) -> String {
    let mut out = text.to_string();
    for (tag, noun) in [("energy", "能量"), ("star", "辉星")] {
        let open = format!("[{tag}]");
        let close = format!("[/{tag}]");
        let mut done = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(i) = rest.find(&open) {
            let after = &rest[i + open.len()..];
            let Some(j) = after.find(&close) else {
                break;
            };
            let before = &rest[..i];
            done.push_str(before);
            if before.ends_with('点') {
                done.push_str(noun);
            } else {
                done.push_str(&after[..j]);
                done.push('点');
                done.push_str(noun);
            }
            rest = &after[j + close.len()..];
        }
        done.push_str(rest);
        out = done;
    }
    out
}

/// The vars an icon directive counts by (`{Energy:energyIcons()}` ->
/// `energy`), which `required_stat_vars` leaves out.
fn icon_vars(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    for piece in template.split('{').skip(1) {
        let Some((inner, _)) = piece.split_once('}') else {
            continue;
        };
        if let Some((name, fmt)) = inner.split_once(':') {
            if matches!(fmt, "energyIcons()" | "starIcons()") && !name.is_empty() {
                out.push(name.to_lowercase());
            }
        }
    }
    out
}

/// `{var_lowercase: value}` read off an English card text: `template`'s
/// simple placeholders (`{HitCount}`, `{HitCount:diff()}`) matched by
/// position against the resolved `plain` (markup stripped on both). Empty
/// when the template has anything else in braces or the shapes disagree.
fn infer_vars(template: &str, plain: &str) -> BTreeMap<String, String> {
    fn strip(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(i) = rest.find('[') {
            out.push_str(&rest[..i]);
            if let Some(j) = rest[i..].find(']') {
                rest = &rest[i + j + 1..];
            } else {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
        out.push_str(rest);
        out
    }
    // One template slot: the literal before it, then the var it prints,
    // with a `choose(N):word|{Var…}unit` directive's N, word and unit.
    struct Slot<'a> {
        lit: &'a str,
        var: Option<String>,
        choose: Option<(String, String, String)>,
        /// `{HitCount:plural:time|times}`: a word, no value; either matches.
        plural: Option<(String, String)>,
    }
    let (t, p) = (strip(template), strip(plain));
    let mut slots: Vec<Slot<'_>> = Vec::new();
    let mut rest = t.as_str();
    while let Some(i) = rest.find('{') {
        // The directive up to its matching brace (choose nests one level).
        let mut depth = 0;
        let mut end = None;
        for (k, c) in rest[i..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            return BTreeMap::new();
        };
        let inner = &rest[i + 1..end];
        let (name, fmt) = match inner.split_once(':') {
            Some((n, f)) => (n, Some(f)),
            None => (inner, None),
        };
        if name.is_empty() || !name.chars().all(char::is_alphanumeric) {
            return BTreeMap::new();
        }
        if let Some((one, many)) = fmt
            .and_then(|f| f.strip_prefix("plural:"))
            .filter(|f| !f.contains('{'))
            .and_then(|f| f.split_once('|'))
        {
            slots.push(Slot {
                lit: &rest[..i],
                var: None,
                choose: None,
                plural: Some((one.to_string(), many.to_string())),
            });
            rest = &rest[end + 1..];
            continue;
        }
        let choose = match fmt {
            None | Some("diff()") => None,
            Some(f) => {
                // `choose(2):twice|{HitCount:diff()} times`
                let Some((n, branches)) =
                    f.strip_prefix("choose(").and_then(|f| f.split_once("):"))
                else {
                    return BTreeMap::new();
                };
                let Some((word, other)) = branches.split_once('|') else {
                    return BTreeMap::new();
                };
                let unit = other.rsplit_once('}').map_or("", |(_, u)| u);
                if word.contains('{') || !other.starts_with('{') {
                    return BTreeMap::new();
                }
                Some((n.to_string(), word.to_string(), unit.to_string()))
            }
        };
        slots.push(Slot {
            lit: &rest[..i],
            var: Some(name.to_lowercase()),
            choose,
            plural: None,
        });
        rest = &rest[end + 1..];
    }
    slots.push(Slot {
        lit: rest,
        var: None,
        choose: None,
        plural: None,
    });
    let mut out = BTreeMap::new();
    let mut at = p.as_str();
    for slot in slots {
        let Some(tail) = at.strip_prefix(slot.lit) else {
            return BTreeMap::new();
        };
        at = tail;
        if let Some((one, many)) = &slot.plural {
            // The longer form first: "times" before "time".
            let (a, b) = if many.len() >= one.len() {
                (many, one)
            } else {
                (one, many)
            };
            let Some(tail) = at
                .strip_prefix(a.as_str())
                .or_else(|| at.strip_prefix(b.as_str()))
            else {
                return BTreeMap::new();
            };
            at = tail;
            continue;
        }
        let Some(var) = slot.var else {
            continue;
        };
        if let Some((n, word, _)) = &slot.choose {
            if let Some(tail) = at.strip_prefix(word.as_str()) {
                at = tail;
                out.insert(var, n.clone());
                continue;
            }
        }
        let mut n: String = at
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        // The wiki prints a count it cannot know as a literal `X`.
        if n.is_empty() && at.starts_with('X') {
            n = "X".to_string();
        }
        if n.is_empty() {
            return BTreeMap::new();
        }
        at = &at[n.len()..];
        if let Some((_, _, unit)) = &slot.choose {
            let Some(tail) = at.strip_prefix(unit.as_str()) else {
                return BTreeMap::new();
            };
            at = tail;
        }
        out.insert(var, n);
    }
    out
}

/// The display map of `d` in Simplified Chinese: [`build_loc`] over `d`
/// with the Chinese tables laid over the English ones key by key, then the
/// manifest-sourced card titles / templates replaced and every
/// pre-resolved `.description_plain` (English, from the wiki) re-rendered
/// from the Chinese template with the item's base values. A text whose
/// values are not all known keeps its English plain form rather than show a
/// raw `{Var}`. Without a zh bundle this is exactly [`build_loc`].
#[must_use]
pub fn zh_overlay(d: &Data, zh: &data::ZhTables) -> Loc {
    if !zh.is_present() {
        return build_loc(d);
    }
    // The English map, to read values the wiki only gives resolved.
    let eng = build_loc(d);
    let mut dz = d.clone();
    overlay_existing(&mut dz.loc_cards, &zh.cards);
    overlay_existing(&mut dz.loc_card_keywords, &zh.card_keywords);
    overlay_existing(&mut dz.loc_afflictions, &zh.afflictions);
    overlay_existing(&mut dz.loc_rest_site_ui, &zh.rest_site_ui);
    overlay_existing(&mut dz.loc_relics, &zh.relics);
    overlay_existing(&mut dz.loc_potions, &zh.potions);
    overlay_existing(&mut dz.loc_characters, &zh.characters);
    overlay_existing(&mut dz.loc_encounters, &zh.encounters);
    overlay_existing(&mut dz.loc_events, &zh.events);
    let mut loc = build_loc(&dz);
    // An ancient's room name (`NEOW.title`, the event's `event_name` key):
    // the English bundle has no row (the page prettifies the key to "Neow"),
    // the game's Chinese `ancients.json` has.
    if let Some(ancients) = zh.extra.get("ancients") {
        for (key, value) in ancients {
            if key.strip_suffix(".title").is_some() && !value.is_empty() {
                loc.insert(key.clone(), value.clone());
            }
        }
    }

    // Cards: the wiki manifest's English title / template win in
    // `load_card_localization`; the game's own Chinese wins here.
    for (key, value) in &zh.cards {
        let field = key.rsplit_once('.').map(|(_, f)| f);
        if matches!(field, Some("title" | "description"))
            && loc.contains_key(key)
            && !value.is_empty()
        {
            loc.insert(key.clone(), value.clone());
        }
    }
    for (key, template) in &zh.cards {
        let Some(card) = key.strip_suffix(".description") else {
            continue;
        };
        if template.is_empty() || !loc.contains_key(&format!("{card}.description_plain")) {
            continue;
        }
        for (field, upgraded) in [
            ("description_plain", false),
            ("upgraded_description_plain", true),
        ] {
            let plain_key = format!("{card}.{field}");
            if !loc.contains_key(&plain_key) {
                continue;
            }
            let flags = crate::render::CardTextFlags {
                in_combat: false,
                upgraded,
                extra_flags: None,
                keep_markup: true,
            };
            let mut stats = serde_json::Map::new();
            let mut complete = true;
            for var in crate::render::required_stat_vars(Some(template), flags) {
                match card_var_value(Some(card), &var, upgraded, &loc) {
                    Some(v) => {
                        stats.insert(var.clone(), crate::enrich::int_or_str(v));
                    }
                    None => complete = false,
                }
            }
            // An icon run's count, when known; without one the renderer spells
            // just the resource ("能量"), as the English plain text does.
            for var in icon_vars(template) {
                if let Some(v) = card_var_value(Some(card), &var, upgraded, &loc) {
                    stats.insert(var, crate::enrich::int_or_str(v));
                }
            }
            if !complete {
                // The wiki lists no base value for some vars (`HitCount`), but
                // its English plain text has them resolved: read them back off
                // the English template and plain text, by position.
                let (Some(t_en), Some(p_en)) =
                    (eng.get(&format!("{card}.description")), eng.get(&plain_key))
                else {
                    continue;
                };
                let found = infer_vars(t_en, p_en);
                let mut filled = true;
                for var in crate::render::required_stat_vars(Some(template), flags) {
                    if stats.contains_key(&var) {
                        continue;
                    }
                    match found.get(&var) {
                        Some(v) => {
                            stats.insert(var.clone(), crate::enrich::int_or_str(v));
                        }
                        None => filled = false,
                    }
                }
                if !filled {
                    continue;
                }
            }
            let text = render_plain(template, &stats, upgraded);
            if !text.is_empty() && !has_placeholder(&text) {
                loc.insert(plain_key, text);
            }
        }
    }

    // Relics / potions: the plain text is the wiki's English; render the
    // Chinese template with the item's base values.
    for table in [&zh.relics, &zh.potions] {
        for (key, template) in table {
            let Some(item) = key.strip_suffix(".description") else {
                continue;
            };
            let plain_key = format!("{item}.description_plain");
            if template.is_empty() || !loc.contains_key(&plain_key) {
                continue;
            }
            let mut stats = serde_json::Map::new();
            if let Some(vars) = zh.owned_vars.get(item) {
                for (name, value) in vars {
                    stats.insert(name.clone(), value.clone());
                    // `{VigorPower}` names the var the wiki types as `Vigor`.
                    stats.insert(format!("{name}power"), value.clone());
                }
            }
            // Values the wiki types under another name: read them back off
            // the English template and its resolved plain text.
            if let (Some(t_en), Some(p_en)) =
                (eng.get(&format!("{item}.description")), eng.get(&plain_key))
            {
                for (k, v) in infer_vars(t_en, p_en) {
                    stats
                        .entry(k)
                        .or_insert_with(|| crate::enrich::int_or_str(&v));
                }
            }
            // Every value the template prints must be known, or the renderer
            // would print the var's name; keep the English text then.
            let flags = crate::render::CardTextFlags {
                in_combat: false,
                upgraded: false,
                extra_flags: None,
                keep_markup: true,
            };
            let complete = crate::render::required_stat_vars(Some(template), flags)
                .into_iter()
                .all(|v| stats.contains_key(&v));
            if !complete {
                // The wiki left its English unresolved too (`{SelfStrength}`):
                // drop the plain text, so the item renders its template from
                // the run's own values like any item without one.
                if eng.get(&plain_key).is_some_and(|p| p.contains('{')) {
                    loc.insert(plain_key, String::new());
                }
                continue;
            }
            let text = spell_icons_zh(&render_plain(template, &stats, false));
            if !text.is_empty() && !has_placeholder(&text) {
                loc.insert(plain_key, text);
            }
        }
    }
    loc
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn loc(pairs: &[(&str, &str)]) -> Loc {
        pairs.iter().copied().collect()
    }

    fn wiki(v: Value) -> Vec<WikiCard> {
        serde_json::from_value(v).unwrap()
    }

    fn state_loc() -> Loc {
        loc(&[
            ("STRIKE_IRONCLAD.title", "Strike"),
            ("LOST_WISP.title", "Lost Wisp"),
            ("FIRE_POTION.title", "Fire Potion"),
            ("NEOWS_TALISMAN.title", "Neow's Talisman"),
            ("rest_site.OPTION_SMITH.name", "Smith"),
        ])
    }

    // ── zh overlay ──────────────────────────────────────────────────

    #[test]
    fn zh_overlay_shows_chinese_names_and_rendered_text() {
        let zh = data::zh_tables();
        if !zh.is_present() || !cfg!(workbench_game_data) {
            return; // a checkout without the game's tables (see build.rs)
        }
        let d = data::data_for(GameVersion::V0_111_0);
        let eng = build_loc(d);
        let loc = zh_overlay(d, zh);
        assert_eq!(loc.get("BASH.title"), Some("痛击"));
        let plain = loc.get("BASH.description_plain").unwrap();
        assert!(plain.contains("造成") && plain.contains('8'), "{plain}");
        assert!(!plain.contains('{'), "{plain}");
        let up = loc.get("BASH.upgraded_description_plain").unwrap();
        assert!(up.contains("10"), "{up}");
        let relic = loc.get("AKABEKO.description_plain").unwrap();
        assert!(relic.contains('8') && !relic.contains('{'), "{relic}");
        // Every key the English map has survives; ids never change.
        for (k, _) in eng.iter() {
            assert!(loc.contains_key(k), "{k} lost");
        }
        // No pre-rendered text is left with a raw placeholder.
        for (k, v) in loc.iter() {
            if k.ends_with("description_plain") {
                assert!(!v.contains('{'), "{k}: {v}");
            }
        }
    }

    #[test]
    fn spell_icons_zh_writes_one_count() {
        assert_eq!(spell_icons_zh("获得[energy]2[/energy]。"), "获得2点能量。");
        assert_eq!(
            spell_icons_zh("额外获得1点[energy]1[/energy]。"),
            "额外获得1点能量。"
        );
        assert_eq!(spell_icons_zh("获得[star]3[/star]。"), "获得3点辉星。");
        assert_eq!(spell_icons_zh("[gold]格挡[/gold]"), "[gold]格挡[/gold]");
    }

    #[test]
    fn infer_vars_reads_values_back_off_the_english_text() {
        let v = infer_vars(
            "Deal {Damage:diff()} damage to a random enemy {HitCount:diff()} times.",
            "Deal 3 damage to a random enemy 3 times.",
        );
        assert_eq!(v.get("hitcount").map(String::as_str), Some("3"));
        assert_eq!(v.get("damage").map(String::as_str), Some("3"));
        let tmpl = "Deal {Damage:diff()} damage to ALL enemies {HitCount:choose(2):twice|{HitCount:diff()} times}.";
        let twice = infer_vars(tmpl, "Deal 4 damage to ALL enemies twice.");
        assert_eq!(twice.get("hitcount").map(String::as_str), Some("2"));
        let three = infer_vars(tmpl, "Deal 4 damage to ALL enemies 3 times.");
        assert_eq!(three.get("hitcount").map(String::as_str), Some("3"));
        let x = infer_vars(
            "Deal {Damage:diff()} damage {HitCount:diff()} times.",
            "Deal 6 damage X times.",
        );
        assert_eq!(x.get("hitcount").map(String::as_str), Some("X"));
        let plural = infer_vars(
            "Deal {Damage:diff()} damage {HitCount:diff()} {HitCount:plural:time|times}.",
            "Deal 6 damage X times.",
        );
        assert_eq!(plural.get("hitcount").map(String::as_str), Some("X"));
        assert!(infer_vars("Deal {Damage} damage.", "Gain 3 Block.").is_empty());
    }

    #[test]
    fn a_text_reworked_between_builds_keeps_its_english_in_the_older_build() {
        let zh = data::zh_tables();
        if !zh.is_present() || data::zh_source_version() != Some(GameVersion::V0_113_0) {
            return;
        }
        let (old, new) = (
            data::data_for(GameVersion::V0_111_0),
            data::data_for(GameVersion::V0_113_0),
        );
        // A card whose English differs between the builds (if any).
        let Some(key) = old
            .loc_cards
            .iter()
            .find(|(k, v)| {
                k.ends_with(".description") && new.loc_cards.get(*k).is_some_and(|n| n != *v)
            })
            .map(|(k, _)| k.clone())
        else {
            return;
        };
        let own = data::zh_tables_for(GameVersion::V0_111_0);
        let shared = data::zh_tables_for(GameVersion::V0_113_0);
        assert!(shared.cards.contains_key(&key));
        // Dropped from the older build unless it has its own Chinese set.
        if !data::zh_has_own(GameVersion::V0_111_0) {
            assert_eq!(own.cards.get(&key), None, "{key}");
        }
    }

    #[test]
    fn zh_overlay_without_tables_is_build_loc() {
        let d = data::data_for(GameVersion::V0_111_0);
        assert_eq!(zh_overlay(d, &data::ZhTables::default()), build_loc(d));
    }

    // ── test_localization.py ────────────────────────────────────────

    #[test]
    fn load_card_meta_includes_cost() {
        let cards = wiki(json!([
            {"loc_key": "BASH", "type": "Attack", "rarity": "Basic", "energy_cost": 2},
            {"loc_key": "WHIRLWIND", "type": "Attack", "rarity": "Uncommon", "energy_cost": -1},
        ]));
        let meta = load_card_meta(&cards);
        assert_eq!(meta["BASH"].cost.as_deref(), Some("2"));
        assert_eq!(meta["BASH"].card_type.as_deref(), Some("Attack"));
        assert_eq!(meta["WHIRLWIND"].cost.as_deref(), Some("X")); // -1 -> variable cost
        assert_eq!(meta["WHIRLWIND"].upgraded_cost, None);
    }

    #[test]
    fn resolve_state_names_covers_cards_relics_potions_options() {
        let state = json!({
            "hand": [{"index": 0, "name": "STRIKE_IRONCLAD.title"}],
            "player": {
                "deck": [{"name": "STRIKE_IRONCLAD.title"}],
                "relics": [{"name": "LOST_WISP.title"}],
                "potions": [{"name": "FIRE_POTION.title"}],
            },
            "options": [{"index": 0, "title": "NEOWS_TALISMAN.title"}],
        });
        let out = resolve_state_names(Some(&state), &state_loc());
        assert_eq!(out["hand"][0]["name"], "Strike");
        assert_eq!(out["player"]["deck"][0]["name"], "Strike");
        assert_eq!(out["player"]["relics"][0]["name"], "Lost Wisp");
        assert_eq!(out["player"]["potions"][0]["name"], "Fire Potion");
        assert_eq!(out["options"][0]["title"], "Neow's Talisman");
        // Original is not mutated (deep copy).
        assert_eq!(state["player"]["relics"][0]["name"], "LOST_WISP.title");
        // Empty / absent state -> {}.
        assert_eq!(resolve_state_names(None, &state_loc()), json!({}));
        assert_eq!(
            resolve_state_names(Some(&json!({})), &state_loc()),
            json!({})
        );
    }

    #[test]
    fn resolve_state_names_resolves_a_rest_site_option_name() {
        // A rest-site option carries its label in `name` (`game.rs` writes
        // `rest_site.OPTION_<ID>.name`), not `title`, and `describe_action`
        // prefers `name` -- so resolving only `title` here leaked the raw key
        // into the run log: "Choose: rest_site.OPTION_SMITH.name".
        let out = resolve_state_names(
            Some(&json!({"options": [
                {"index": 0, "option_id": "SMITH", "name": "rest_site.OPTION_SMITH.name"},
                {"index": 1, "option_id": "DIG", "name": "rest_site.OPTION_DIG.name"},
            ]})),
            &state_loc(),
        );
        assert_eq!(out["options"][0]["name"], "Smith");
        // A key the bundle lacks is prettified, never printed raw.
        assert_eq!(out["options"][1]["name"], "Dig");
    }

    #[test]
    fn resolve_state_names_prettifies_a_leaked_relic_key() {
        // The Rust backend stores a relic as its loc key; a key the bundle
        // lacks is prettified so the run-log line reads "+Relic Winged Boots".
        let nxt = resolve_state_names(
            Some(&json!({"player": {"relics": [{"name": "WINGED_BOOTS.title"}]}})),
            &state_loc(),
        );
        assert_eq!(nxt["player"]["relics"][0]["name"], "Winged Boots");
    }

    #[test]
    fn load_card_localization_loads_plain_descriptions() {
        let cards = wiki(json!([{
            "loc_key": "THUNDERCLAP",
            "title": "Thunderclap",
            "description_template": "Deal {Damage} damage and apply {VulnerablePower} Vulnerable to ALL enemies.",
            "description_plain": "Deal 4 damage and apply 1 Vulnerable to ALL enemies.",
            "upgraded_description_plain": "Deal 7 damage and apply 1 Vulnerable to ALL enemies.",
        }]));
        let loc = load_card_localization(&LocMap::new(), &cards);
        assert!(loc
            .get("THUNDERCLAP.description_plain")
            .unwrap()
            .starts_with("Deal 4 damage"));
        assert!(loc
            .get("THUNDERCLAP.upgraded_description_plain")
            .unwrap()
            .starts_with("Deal 7 damage"));
    }

    #[test]
    fn plain_description_picks_base_or_upgraded() {
        let l = loc(&[
            ("THUNDERCLAP.description_plain", "Deal 4 damage."),
            ("THUNDERCLAP.upgraded_description_plain", "Deal 7 damage."),
        ]);
        assert_eq!(
            plain_description(Some("THUNDERCLAP"), false, &l),
            Some("Deal 4 damage.")
        );
        assert_eq!(
            plain_description(Some("THUNDERCLAP"), true, &l),
            Some("Deal 7 damage.")
        );
    }

    #[test]
    fn plain_description_falls_back_to_base_when_no_upgraded() {
        let l = loc(&[(
            "HAVOC.description_plain",
            "Play the top card of your Draw Pile and Exhaust it.",
        )]);
        assert!(plain_description(Some("HAVOC"), true, &l)
            .unwrap()
            .starts_with("Play the top card"));
    }

    #[test]
    fn plain_description_missing_returns_none() {
        assert_eq!(plain_description(Some("UNKNOWN"), false, &Loc::new()), None);
        assert_eq!(plain_description(None, false, &Loc::new()), None);
        assert_eq!(plain_description(Some(""), false, &Loc::new()), None);
    }

    #[test]
    fn card_var_value_resolves_power_suffix() {
        let l = loc(&[
            ("BASH.var.vulnerable", "2"),
            ("BASH.uvar.vulnerable", "3"),
            ("CLAW.var.increase", "2"),
        ]);
        // Template references {VulnerablePower}; manifest stores it as "vulnerable".
        assert_eq!(
            card_var_value(Some("BASH"), "VulnerablePower", false, &l),
            Some("2")
        );
        assert_eq!(
            card_var_value(Some("BASH"), "VulnerablePower", true, &l),
            Some("3")
        );
        // Plain (non-power) var matches directly.
        assert_eq!(
            card_var_value(Some("CLAW"), "Increase", false, &l),
            Some("2")
        );
        // Unknown var / card -> None.
        assert_eq!(card_var_value(Some("CLAW"), "Nope", false, &l), None);
        assert_eq!(card_var_value(None, "Increase", false, &l), None);
        assert_eq!(card_var_value(Some("CLAW"), "", false, &l), None);
    }

    #[test]
    fn engine_manifest_wins_over_stale_repo_localization() {
        let repo: LocMap = [
            ("PACTS_END.title", "Pact's End"),
            (
                "PACTS_END.description",
                "Can only be played if you have 3 cards.",
            ),
            ("PACTS_END.selectionScreenPrompt", "Choose a card"),
            ("GONE_IN_V107.description", "A card the manifest dropped."),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let cards = wiki(json!([{
            "loc_key": "PACTS_END",
            "title": "Pact's End",
            "description_template": "If you have 3 cards, deal 17 damage.",
        }]));
        let loc = load_card_localization(&repo, &cards);
        assert_eq!(
            loc.get("PACTS_END.description"),
            Some("If you have 3 cards, deal 17 damage.")
        );
        assert_eq!(
            loc.get("PACTS_END.selectionScreenPrompt"),
            Some("Choose a card")
        );
        assert_eq!(
            loc.get("GONE_IN_V107.description"),
            Some("A card the manifest dropped.")
        );
    }

    #[cfg(workbench_game_data)] // asserts on real game text (see build.rs)
    #[test]
    fn shipped_localization_resolves_pacts_end_to_v107_text() {
        let d = data::data();
        let loc = load_card_localization(&d.loc_cards, &d.wiki_cards);
        let desc = loc.get("PACTS_END.description").unwrap();
        assert!(desc.starts_with("If you have"), "{desc}");
        assert!(!desc.contains("Can only be played"));
        // A second card from the same drift class, changed for a different reason.
        assert!(loc
            .get("ASTRAL_PULSE.description")
            .unwrap()
            .contains("twice"));
    }

    #[test]
    fn keyword_card_lines_follow_the_games_own_order() {
        let cards = wiki(json!([
            {"loc_key": "ASCENDERS_BANE", "keywords": ["Eternal", "Unplayable", "Ethereal"]},
            // Gains a keyword only on upgrade (C# `OnUpgrade` AddKeyword).
            {"loc_key": "AFTERIMAGE", "upgrades": [{"action": "add_keyword", "keyword": "Innate"}]},
            // Loses one on upgrade.
            {
                "loc_key": "APPARITION",
                "keywords": ["Ethereal", "Exhaust"],
                "upgrades": [{"action": "remove_keyword", "keyword": "Ethereal"}],
            },
            {"loc_key": "STRIKE_IRONCLAD"},
        ]));
        let mut loc = load_card_localization(&LocMap::new(), &cards);
        // No card_keywords.json on disk -> titles fall back to the names.
        loc.update(&load_card_keyword_localization(&LocMap::new()));

        let s = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert_eq!(
            keyword_card_lines(Some("ASCENDERS_BANE"), false, &loc),
            (
                s(&["[gold]Unplayable[/gold].", "[gold]Ethereal[/gold]."]),
                s(&["[gold]Eternal[/gold]."])
            )
        );
        // A card with no keywords gets no lines at all.
        assert_eq!(
            keyword_card_lines(Some("STRIKE_IRONCLAD"), false, &loc),
            (vec![], vec![])
        );
        // Upgrade deltas apply only to the upgraded face.
        assert_eq!(
            keyword_card_lines(Some("AFTERIMAGE"), false, &loc),
            (vec![], vec![])
        );
        assert_eq!(
            keyword_card_lines(Some("AFTERIMAGE"), true, &loc),
            (s(&["[gold]Innate[/gold]."]), vec![])
        );
        let set = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<BTreeSet<_>>();
        assert_eq!(
            card_keywords(Some("APPARITION"), false, &loc),
            set(&["Ethereal", "Exhaust"])
        );
        assert_eq!(
            card_keywords(Some("APPARITION"), true, &loc),
            set(&["Exhaust"])
        );
        assert!(card_keywords(None, true, &loc).is_empty());
    }

    #[cfg(workbench_game_data)] // asserts on real game text (see build.rs)
    #[test]
    fn shipped_manifest_keywords_are_the_cards_own_not_its_hover_tips() {
        let d = data::data();
        let mut loc = load_card_localization(&d.loc_cards, &d.wiki_cards);
        loc.update(&load_card_keyword_localization(&d.loc_card_keywords));

        assert!(card_keywords(Some("BURNING_PACT"), false, &loc).is_empty());
        assert!(card_keywords(Some("CORRUPTION"), false, &loc).is_empty());
        assert!(card_keywords(Some("FEEL_NO_PAIN"), false, &loc).is_empty());
        assert!(card_keywords(Some("AFTERIMAGE"), false, &loc).is_empty());
        assert_eq!(
            card_keywords(Some("AFTERIMAGE"), true, &loc),
            BTreeSet::from(["Innate".to_string()])
        );
        // …and the card that started this: Tremble really does Exhaust.
        assert_eq!(
            keyword_card_lines(Some("TREMBLE"), false, &loc),
            (vec![], vec!["[gold]Exhaust[/gold].".to_string()])
        );
    }

    #[cfg(workbench_game_data)] // asserts on real game text (see build.rs)
    #[test]
    fn vendored_localization_describes_the_pinned_engine_build() {
        let d = data::data();
        assert_eq!(d.loc_build, ENGINE_WIKI_VERSION);
        // v0.107.1 replaced Pact's End's `IsPlayable` gate with an `OnPlay` check.
        assert!(!d.loc_cards["PACTS_END.description"].contains("Can only be played"));
        // Content the game removed after v0.103.2 but the engine still
        // implements must survive the re-vendor, or it renders nameless.
        assert_eq!(d.loc_cards["GRAPPLE.title"], "Grapple");
        assert_eq!(d.loc_cards["FOLLOW_THROUGH.title"], "Follow Through");
    }

    #[cfg(workbench_game_data_v0_111_0)] // asserts on real game text (see build.rs)
    #[test]
    fn every_bundle_describes_its_own_build() {
        for v in GameVersion::ALL {
            let d = data::data_for(v);
            assert_eq!(d.loc_build, engine_wiki_version(v), "{v}");
            // Content the engine still implements keeps its name in every
            // build's bundle (the vendor merge retains older keys).
            assert_eq!(d.loc_cards["GRAPPLE.title"], "Grapple", "{v}");
        }
    }

    #[cfg(workbench_game_data_v0_111_0)] // asserts on real game text (see build.rs)
    #[test]
    fn embedded_v0_111_loc_names_its_new_content() {
        let loc = Loc::embedded_for(GameVersion::V0_111_0);
        assert_eq!(loc.get("CACOPHONY.title"), Some("Cacophony"));
        assert!(loc.get("CACOPHONY.card_type").is_some());
        assert!(loc.get("CACOPHONY.description_plain").is_some());
        assert_eq!(loc.get("DOWSING_ROD.title"), Some("Dowsing Rod"));
        assert!(loc.get("DOWSING_ROD.description_plain").is_some());
        assert_eq!(loc.get("AMBERGRIS.title"), Some("Ambergris"));
        // Shared namespaced tables still land.
        assert_eq!(loc.get("card_keyword.Exhaust"), Some("Exhaust"));
        assert_eq!(loc.get("rest_site.OPTION_LIFT.name"), Some("Train"));
        // The default build's map does not know the new card.
        assert_eq!(Loc::embedded().get("CACOPHONY.title"), None);
        assert!(std::ptr::eq(
            Loc::embedded(),
            Loc::embedded_for(GameVersion::V0_107_1)
        ));
        assert!(!embedded_monster_hp_ranges_for(GameVersion::V0_111_0).is_empty());
    }

    #[cfg(workbench_game_data)] // asserts on real game text (see build.rs)
    #[test]
    fn rest_site_option_names_come_from_the_games_own_rows() {
        let loc = load_rest_site_localization(&data::data().loc_rest_site_ui);
        assert_eq!(loc.get("rest_site.OPTION_HEAL.name"), Some("Rest"));
        assert_eq!(loc.get("rest_site.OPTION_SMITH.name"), Some("Smith"));
        assert_eq!(loc.get("rest_site.OPTION_KINDLE.name"), Some("Kindle"));
        // The game calls Girya's option "Train", not "Lift".
        assert_eq!(loc.get("rest_site.OPTION_LIFT.name"), Some("Train"));
        for opt in REST_SITE_OPTION_IDS {
            let key = format!("rest_site.OPTION_{opt}.name");
            let resolved = resolve(Some(&key), &loc);
            assert_ne!(resolved, "");
            assert!(!looks_like_unresolved_key(Some(&resolved)));
        }
        // An option the dump lacks still renders a word (title-cased id).
        let fallback = load_rest_site_localization(&LocMap::new());
        assert_eq!(fallback.get("rest_site.OPTION_HEAL.name"), Some("Heal"));
    }

    // ── test_event_option_loc_keys.py (the localization halves) ────

    // Asserts on real game text and reads the v0.113.0 events.json, which
    // the public export omits (see build.rs).
    #[cfg(all(workbench_game_data, workbench_game_data_v0_113_0))]
    #[test]
    fn every_wired_static_event_option_loc_key_names_a_real_row() {
        // The Python gate reads the keys out of the Rust SOURCE (so it also
        // sees hand-built `EventOptionInfo` literals); here the static specs
        // are enumerated through the engine itself. A key with no row
        // silently degrades to the developer shorthand this replaced.
        let events = &data::data().loc_events;
        let mut missing = Vec::new();
        // The workbench ships the default build's events.json; the v0.113.0
        // events' keys are rows of `localization_eng_v0.113.0/events.json`.
        for id in sts2_core::EventId::all()
            .iter()
            .filter(|e| !sts2_core::EventId::V0113_ONLY.contains(e))
        {
            for opt in sts2_content::events::dispatch::spec_for(*id).options {
                if opt.loc_key.is_empty() {
                    continue;
                }
                let has_row = events.contains_key(&format!("{}.title", opt.loc_key))
                    || events.contains_key(&format!("{}.description", opt.loc_key));
                if !has_row {
                    missing.push(opt.loc_key.to_string());
                }
            }
        }
        // An option a pre-v0.113.0 event gains in v0.113.0 (Colorful
        // Philosophers' Fool) has its row in the v0.113.0 file only.
        let v0113: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/data/game/v0.113.0/localization_eng/events.json"
            ))
            .expect("v0.113.0 events.json"),
        )
        .expect("events.json parses");
        missing.retain(|k| {
            !v0113.contains_key(&format!("{k}.title"))
                && !v0113.contains_key(&format!("{k}.description"))
        });
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "event option loc_key(s) with no row in events.json: {missing:?}"
        );
    }

    #[cfg(workbench_game_data)] // asserts on real game text (see build.rs)
    #[test]
    fn named_description_vars_splices_the_named_items_plain_text() {
        // Tea Master's "{BoneTeaDescription}" prints the Bone Tea RELIC's
        // description, found by its loc key in the owned-item table.
        let owned = load_owned_localization(&data::data().wiki_owned);
        let needed: BTreeSet<String> = ["boneteacost", "boneteadescription"]
            .into_iter()
            .map(String::from)
            .collect();
        let supplied: BTreeSet<String> = ["boneteacost".to_string()].into_iter().collect();
        let out = named_description_vars(&needed, &supplied, &owned);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out["boneteadescription"],
            owned.get("BONE_TEA.description_plain").unwrap()
        );
        // Names that are not `<Something>Description`, or already supplied,
        // or name nothing we hold, are left out.
        let needed: BTreeSet<String> = ["gold", "description", "nosuchitemdescription"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(named_description_vars(&needed, &BTreeSet::new(), &owned).is_empty());
    }

    // ── Everything else public ─────────────────────────────────────

    #[test]
    fn class_name_to_loc_key_matches_the_wiki_extractor() {
        assert_eq!(class_name_to_loc_key("LargeCapsule"), "LARGE_CAPSULE");
        assert_eq!(class_name_to_loc_key("BurningBlood"), "BURNING_BLOOD");
        assert_eq!(class_name_to_loc_key("Akabeko"), "AKABEKO");
        assert_eq!(class_name_to_loc_key("ABCd"), "AB_CD");
        assert_eq!(class_name_to_loc_key("Bottle2Card"), "BOTTLE2_CARD");
        assert_eq!(class_name_to_loc_key("HTTPServer"), "HTTP_SERVER");
    }

    #[test]
    fn load_owned_localization_keys_by_class_name() {
        let items: Vec<WikiOwned> = serde_json::from_value(json!([
            {"class_name": "BurningBlood", "description": "Heal 6 HP."},
            {"class_name": "", "description": "skipped"},
            {"class_name": "NoDesc", "description": ""},
            {"class_name": "BurningBlood", "description": "second file loses"},
        ]))
        .unwrap();
        let loc = load_owned_localization(&items);
        assert_eq!(loc.len(), 1);
        assert_eq!(
            loc.get("BURNING_BLOOD.description_plain"),
            Some("Heal 6 HP.")
        );
        assert_eq!(
            plain_owned_description(Some("BURNING_BLOOD"), &loc),
            Some("Heal 6 HP.")
        );
        assert_eq!(plain_owned_description(None, &loc), None);
        assert_eq!(plain_owned_description(Some(""), &loc), None);
    }

    #[test]
    fn load_card_localization_derives_every_key_shape() {
        let cards = wiki(json!([
            {
                "loc_key": "BASH",
                "title": "Bash",
                "character": "Ironclad",
                "star_cost": 2,
                "x_star_cost": true,
                "keywords": ["Exhaust", "Innate"],
                "vars": [
                    {"type": "Damage", "base_value": 8, "upgraded_value": 10},
                    {"type": "Vulnerable", "base_value": 2},
                    {"type": "", "base_value": 99},
                    {"base_value": 98},
                ],
                "upgrades": [{"var": "Damage", "amount": 2}, {"keyword": "Innate", "action": "remove_keyword"}],
            },
            {"loc_key": "STARS", "x_star_cost": true},
            {"loc_key": "", "title": "no key"},
            {"loc_key": "EMPTY", "title": "", "character": "", "description_template": ""},
        ]));
        let loc = load_card_localization(&LocMap::new(), &cards);
        assert_eq!(loc.get("BASH.title"), Some("Bash"));
        assert_eq!(loc.get("BASH.character"), Some("Ironclad"));
        // A numeric star cost wins over the X flag (setdefault order).
        assert_eq!(loc.get("BASH.star_cost"), Some("2"));
        assert_eq!(loc.get("STARS.star_cost"), Some("X"));
        assert_eq!(loc.get("BASH.keywords"), Some("Exhaust,Innate"));
        assert_eq!(loc.get("BASH.upgrade_keywords"), Some("-Innate"));
        assert_eq!(loc.get("BASH.var.damage"), Some("8"));
        assert_eq!(loc.get("BASH.uvar.damage"), Some("10"));
        assert_eq!(loc.get("BASH.var.vulnerable"), Some("2"));
        assert_eq!(loc.get("BASH.uvar.vulnerable"), None);
        assert!(!loc
            .iter()
            .any(|(k, _)| k.ends_with(".var.") || k.contains("99") || k.contains("98")));
        assert!(!loc
            .iter()
            .any(|(k, _)| k.starts_with("EMPTY.") || k.starts_with('.')));
    }

    #[test]
    fn load_card_keyword_localization_covers_every_keyword() {
        let loc = load_card_keyword_localization(&data::data().loc_card_keywords);
        for k in KEYWORDS_BEFORE_DESCRIPTION
            .iter()
            .chain(KEYWORDS_AFTER_DESCRIPTION.iter())
        {
            assert_eq!(loc.get(&format!("card_keyword.{k}")), Some(*k), "{k}");
        }
        let entries: LocMap = [("EXHAUST.title", "Verbrauchen"), ("SLY.title", "")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let loc = load_card_keyword_localization(&entries);
        assert_eq!(loc.get("card_keyword.Exhaust"), Some("Verbrauchen"));
        assert_eq!(loc.get("card_keyword.Sly"), Some("Sly"));
    }

    #[cfg(workbench_game_data)] // asserts on real game text (see build.rs)
    #[test]
    fn load_affliction_localization_namespaces_the_four_printing_afflictions() {
        let loc = load_affliction_localization(&data::data().loc_afflictions);
        assert_eq!(loc.get("affliction.BOUND.title"), Some("Bound"));
        assert!(loc.get("affliction.BOUND.extraCardText").is_some());
        assert!(loc.get("affliction.GALVANIZED.extraCardText").is_some());
        // Entangled has a row the game never prints; it must not land.
        assert!(!loc.iter().any(|(k, _)| k.contains("ENTANGLED")));
        assert!(loc.iter().all(|(k, _)| k.starts_with("affliction.")));
    }

    #[test]
    fn affliction_card_line_wraps_the_extra_text() {
        let mut loc = load_affliction_localization(&data::data().loc_afflictions);
        loc.insert("affliction.BOUND.extraCardText", "Bound");
        assert_eq!(
            affliction_card_line(Some("Bound"), &loc),
            Some("[purple]Bound[/purple]".into())
        );
        assert_eq!(
            affliction_card_line(Some(" bound "), &loc),
            Some("[purple]Bound[/purple]".into())
        );
        assert_eq!(affliction_card_line(Some("Entangled"), &loc), None);
        assert_eq!(affliction_card_line(None, &loc), None);
        assert_eq!(affliction_card_line(Some(""), &loc), None);
    }

    #[test]
    fn resolve_and_resolve_name() {
        let l = state_loc();
        assert_eq!(resolve(Some("STRIKE_IRONCLAD.title"), &l), "Strike");
        assert_eq!(resolve(Some("plain prose"), &l), "plain prose");
        assert_eq!(resolve(None, &l), "");
        assert_eq!(resolve(Some(""), &l), "");
        assert_eq!(resolve_name(Some("WINGED_BOOTS.title"), &l), "Winged Boots");
        assert_eq!(
            resolve_name(Some("NEOWS_TALISMAN.description"), &l),
            "Neows Talisman"
        );
        assert_eq!(resolve_name(Some("X1_2.name"), &l), "X1 2");
        // Not a leaked key: lowercase stem, unknown field, no dot.
        assert_eq!(
            resolve_name(Some("winged_boots.title"), &l),
            "winged_boots.title"
        );
        assert_eq!(
            resolve_name(Some("WINGED_BOOTS.pages"), &l),
            "WINGED_BOOTS.pages"
        );
        assert_eq!(resolve_name(Some("WINGED_BOOTS"), &l), "WINGED_BOOTS");
        assert_eq!(resolve_name(None, &l), "");
    }

    #[test]
    fn py_title_matches_cpython() {
        assert_eq!(py_title("winged boots"), "Winged Boots");
        assert_eq!(py_title("neow's talisman"), "Neow'S Talisman");
        assert_eq!(py_title("HEAL"), "Heal");
        assert_eq!(py_title("mk1a b"), "Mk1A B");
        assert_eq!(py_title(""), "");
        assert_eq!(py_capitalize("dECIMILLIPEDE"), "Decimillipede");
        assert_eq!(py_capitalize("v1"), "V1");
        assert_eq!(py_capitalize(""), "");
    }

    #[test]
    fn looks_like_unresolved_key_spots_dotted_paths_only() {
        assert!(looks_like_unresolved_key(Some(
            "NEOW.pages.INITIAL.description"
        )));
        assert!(looks_like_unresolved_key(Some("a.b")));
        assert!(looks_like_unresolved_key(Some("a.b\n")));
        assert!(!looks_like_unresolved_key(Some("Some prose. With a dot.")));
        assert!(!looks_like_unresolved_key(Some("NEOW")));
        assert!(!looks_like_unresolved_key(Some("NEOW..pages")));
        assert!(!looks_like_unresolved_key(Some(".pages")));
        assert!(!looks_like_unresolved_key(Some("a-b.c")));
        assert!(!looks_like_unresolved_key(Some("")));
        assert!(!looks_like_unresolved_key(None));
    }

    #[cfg(workbench_game_data)] // asserts on real game text (see build.rs)
    #[test]
    fn load_monster_hp_ranges_fills_the_omitted_bounds() {
        let rows: Vec<WikiMonster> = serde_json::from_value(json!([
            {"class_name": "Nibbit", "min_hp": 19, "max_hp": 24, "min_hp_base": 18, "max_hp_base": 23, "hp_ascension": 8},
            {"class_name": "Fixed", "min_hp": 535, "max_hp": 535, "min_hp_base": 512, "hp_ascension": 8},
            {"class_name": "NoBump", "min_hp": 40, "max_hp": 44},
            {"class_name": "Nothing"},
        ]))
        .unwrap();
        let out = load_monster_hp_ranges(&rows);
        assert_eq!(
            out["Nibbit"],
            HpRange {
                min: 18,
                max: 23,
                min_asc: 19,
                max_asc: 24,
                hp_ascension: 8
            }
        );
        assert_eq!(
            out["Fixed"],
            HpRange {
                min: 512,
                max: 512,
                min_asc: 535,
                max_asc: 535,
                hp_ascension: 8
            }
        );
        assert_eq!(
            out["NoBump"],
            HpRange {
                min: 40,
                max: 40,
                min_asc: 40,
                max_asc: 44,
                hp_ascension: 0
            }
        );
        assert!(!out.contains_key("Nothing"));
        // Serialises with the Python dict's key order.
        assert_eq!(
            serde_json::to_string(&out["NoBump"]).unwrap(),
            r#"{"min":40,"max":40,"min_asc":40,"max_asc":44,"hp_ascension":0}"#
        );
        assert!(embedded_monster_hp_ranges().contains_key("Nibbit"));
    }

    #[cfg(workbench_game_data)] // asserts on real game text (see build.rs)
    #[test]
    fn embedded_loc_reproduces_create_app_merge_order() {
        let loc = Loc::embedded();
        let d = data::data();
        // Card meta folded in as flat keys.
        assert_eq!(loc.get("BASH.card_type"), Some("Attack"));
        assert_eq!(loc.get("BASH.rarity"), Some("Basic"));
        assert_eq!(loc.get("BASH.cost"), Some("2"));
        // The manifest carries X-cost as a separate `x_cost` flag the loader
        // does not read, so Whirlwind's printed cost is its `energy_cost` 0.
        assert_eq!(loc.get("WHIRLWIND.cost"), Some("0"));
        assert!(loc.get("STAMPEDE.upgraded_cost").is_some());
        // Namespaced tables.
        assert_eq!(loc.get("card_keyword.Exhaust"), Some("Exhaust"));
        assert_eq!(loc.get("affliction.BOUND.title"), Some("Bound"));
        assert_eq!(loc.get("rest_site.OPTION_LIFT.name"), Some("Train"));
        // Relic / potion dumps overwrite; characters fill; events fill.
        assert_eq!(
            loc.get("BURNING_BLOOD.title").map(String::from),
            d.loc_relics.get("BURNING_BLOOD.title").cloned()
        );
        assert!(loc.get("BURNING_BLOOD.description_plain").is_some());
        assert_eq!(
            loc.get("IRONCLAD.title").map(String::from),
            d.loc_characters.get("IRONCLAD.title").cloned()
        );
        // An event id that collides with the relic it grants keeps the
        // relic's name (events are merged with setdefault).
        assert_eq!(
            loc.get("LOST_WISP.title").map(String::from),
            d.loc_relics.get("LOST_WISP.title").cloned()
        );
        assert_ne!(
            loc.get("LOST_WISP.title"),
            d.loc_events.get("LOST_WISP.title").map(String::as_str)
        );
        assert!(loc.get("LOST_WISP.pages.INITIAL.description").is_some());
        // The in-repo dump still fills what the manifest dropped.
        assert_eq!(loc.get("GRAPPLE.title"), Some("Grapple"));
        // Every raw-dump key landed.
        for key in d
            .loc_cards
            .keys()
            .chain(d.loc_relics.keys())
            .chain(d.loc_potions.keys())
        {
            assert!(loc.contains_key(key), "{key}");
        }
        for key in d.loc_characters.keys().chain(d.loc_events.keys()) {
            assert!(loc.contains_key(key), "{key}");
        }
    }
}
