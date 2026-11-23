//! The words the engine writes itself, in the page's display language.
//!
//! Game content (card / relic / monster names, card text) comes from the
//! game's own tables ([`crate::localization`], [`game_name`]). This module
//! covers the rest: the action labels, history lines, diffs and notes the
//! workbench composes ("Play {card}", "Map → ({x},{y})", "+Relic {name}").
//!
//! Strings are keyed by their English text, gettext-style, like the page
//! (`web/src/i18n`) and the mod (`L10n.cs`): [`tr`] returns the English when
//! the display language is English ([`crate::localization::display_zh`]) or
//! the key has no Chinese entry, so English output is byte-identical to
//! before. [`trf`] fills `{name}` placeholders.
//!
//! The Chinese tables are `i18n/zh_*.rs`, each a `TABLE: &[(&str, &str)]`
//! listed in [`TABLES`]. A test fails when a literal `tr("…")` / `trf("…"`
//! key in `src/` has no entry or its `{placeholders}` differ.
//!
//! Never feed a translated string back into logic (matching, parsing, ids):
//! translate only where text leaves for display.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

use crate::localization::display_zh;

mod zh_labels;
mod zh_names;
mod zh_text;

/// Every Chinese table (a key in two tables: the later one wins; the
/// coverage test rejects two different translations of one key).
const TABLES: &[&[(&str, &str)]] = &[zh_labels::TABLE, zh_names::TABLE, zh_text::TABLE];

fn zh_map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| TABLES.iter().flat_map(|t| t.iter().copied()).collect())
}

/// `en` in the display language.
#[must_use]
pub fn tr(en: &'static str) -> &'static str {
    if !display_zh() {
        return en;
    }
    zh_map().get(en).copied().unwrap_or(en)
}

/// [`tr`] for a key only known at run time (a closed set of English
/// constants that the tables list): returns an owned string.
#[must_use]
pub fn tr_dyn(en: &str) -> String {
    if !display_zh() {
        return en.to_string();
    }
    zh_map()
        .get(en)
        .map_or_else(|| en.to_string(), |s| (*s).to_string())
}

/// [`tr`] then fill each `{name}` from `vars` (a missing var stays as
/// written).
#[must_use]
pub fn trf(en: &'static str, vars: &[(&str, &dyn Display)]) -> String {
    fill(tr(en), vars)
}

/// Fill `{name}` placeholders in `template` from `vars`.
#[must_use]
pub fn fill(template: &str, vars: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let name = &after[..close];
        if let Some((_, v)) = vars.iter().find(|(k, _)| *k == name) {
            out.push_str(&v.to_string());
        } else {
            out.push('{');
            out.push_str(name);
            out.push('}');
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// A game name from the game's own Chinese table `table` (`"monsters"`,
/// `"powers"`, `"orbs"`, `"intents"`, `"acts"`, `"game_modes"`, ...) under
/// `key` (`"<ID>.name"`, `"<ID>.title"`), else `english`. English mode
/// always returns `english`.
#[must_use]
pub fn game_name(table: &str, key: &str, english: &str) -> String {
    if !display_zh() {
        return english.to_string();
    }
    crate::data::zh_tables()
        .get(table, key)
        .map_or_else(|| english.to_string(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_known_vars_only() {
        let n = 3;
        assert_eq!(fill("Floor {n} of {m}", &[("n", &n)]), "Floor 3 of {m}");
        assert_eq!(fill("no vars", &[]), "no vars");
        assert_eq!(fill("open { brace", &[]), "open { brace");
    }

    #[test]
    fn english_is_the_key() {
        // The display language defaults to English in tests.
        assert_eq!(tr("End turn"), "End turn");
        assert_eq!(game_name("monsters", "X.name", "Cultist"), "Cultist");
    }

    /// Literal `tr("…")` / `trf("…"` / `tr_dyn("…")` keys in `src/`.
    fn literal_keys(code: &str) -> Vec<String> {
        let mut out = Vec::new();
        for call in ["tr(\"", "trf(\"", "tr_dyn(\""] {
            let mut rest = code;
            while let Some(i) = rest.find(call) {
                let prev = rest[..i].chars().next_back();
                rest = &rest[i + call.len()..];
                if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let mut key = String::new();
                let mut chars = rest.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => key.push('\n'),
                            Some('t') => key.push('\t'),
                            Some(o) => key.push(o),
                            None => {}
                        },
                        c => key.push(c),
                    }
                }
                out.push(key);
            }
        }
        out
    }

    fn holes(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s
            .split('{')
            .skip(1)
            .filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string()))
            .filter(|n| n.chars().all(|c| c.is_alphanumeric() || c == '_'))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    fn rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "i18n") {
                    rs_files(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs")
                && p.file_name().is_some_and(|n| n != "i18n.rs")
            {
                out.push(p);
            }
        }
    }

    #[test]
    fn every_literal_key_has_a_chinese_entry_with_the_same_placeholders() {
        let map: HashMap<&str, &str> = TABLES.iter().flat_map(|t| t.iter().copied()).collect();
        let mut files = Vec::new();
        rs_files(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let (mut missing, mut bad) = (Vec::new(), Vec::new());
        for f in files {
            let code = std::fs::read_to_string(&f).unwrap();
            for key in literal_keys(&code) {
                match map.get(key.as_str()) {
                    None => missing.push(format!("{}: {key:?}", f.display())),
                    Some(zh) if holes(zh) != holes(&key) => bad.push(format!("{key:?} -> {zh:?}")),
                    Some(_) => {}
                }
            }
        }
        assert!(
            missing.is_empty(),
            "keys with no Chinese entry:\n{}",
            missing.join("\n")
        );
        assert!(bad.is_empty(), "placeholders differ:\n{}", bad.join("\n"));
    }

    #[test]
    fn no_two_tables_translate_one_key_differently() {
        let mut seen: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
        let mut clashes = Vec::new();
        for table in TABLES {
            for (k, v) in *table {
                if let Some(prev) = seen.insert(k, v) {
                    if prev != *v {
                        clashes.push(format!("{k:?}: {prev:?} vs {v:?}"));
                    }
                }
            }
        }
        assert!(clashes.is_empty(), "{}", clashes.join("\n"));
    }
}
