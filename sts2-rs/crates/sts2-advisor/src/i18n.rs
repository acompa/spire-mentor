//! The advisor's own sentences in the player's language.
//!
//! The mod says which language its game runs in: an `advice_request` carries
//! `"language": "zh"` when the mod shows Chinese (PROTOCOL.md v5.4), and no
//! `language` otherwise. [`set_language`] records it per request
//! (`serve::Advisor::on_advice_request`), and it holds until the next
//! request: a `human_action`, `combat_ended` or `game_over` carries no
//! language and is answered (its `coach` records) in the last request's.
//!
//! The language is process-wide, not thread-local, on purpose: the coach
//! composes its records on its own grader thread, after the request that
//! set the language has returned. One advisor process serves one player
//! (an AI partner has its own process), so the process's language is the
//! player's. Nothing else sets it, so the Python bindings (`sts2_rs`), the
//! replay tooling and every test that does not ask stay in English.
//!
//! Strings are keyed by their English text, gettext-style, like the
//! workbench (`sts2_workbench::i18n`) and the mod (`L10n.cs`): [`tr`]
//! returns the English when the language is English or the key has no
//! Chinese entry, so English output is byte-identical to before. [`trf`]
//! fills `{name}` placeholders. Game names (cards, relics, monsters) come
//! from the game's own Chinese tables, by id ([`crate::display_names`]).
//!
//! The Chinese table is `i18n/zh.rs`. A test fails when a literal
//! `tr("…")` / `trf("…"` key in `sts2-advisor/src` or `sim-advisor/src` has
//! no entry or its `{placeholders}` differ.
//!
//! Never feed a translated string back into logic (matching, parsing, ids):
//! translate only where text leaves for display.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

mod zh;

static ZH: AtomicBool = AtomicBool::new(false);

/// Record the language of the request being answered: `Some("zh")` (or any
/// `zh*` code) is Chinese, anything else (absent included) English.
pub fn set_language(code: Option<&str>) {
    let zh = code.is_some_and(|c| c.trim().to_lowercase().starts_with("zh"));
    ZH.store(zh, Ordering::Relaxed);
}

/// The advisor answers in Chinese.
#[must_use]
pub fn is_zh() -> bool {
    ZH.load(Ordering::Relaxed)
}

fn zh_map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| zh::TABLE.iter().copied().collect())
}

/// `en` in the advisor's language.
#[must_use]
pub fn tr(en: &'static str) -> &'static str {
    if !is_zh() {
        return en;
    }
    zh_map().get(en).copied().unwrap_or(en)
}

/// [`tr`] for a key only known at run time (a closed set of English
/// constants that the table lists): returns an owned string.
#[must_use]
pub fn tr_dyn(en: &str) -> String {
    if !is_zh() {
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

/// The list separator between named things: `", "` / `"、"`.
#[must_use]
pub fn join_list(items: &[String], en_sep: &'static str) -> String {
    if is_zh() {
        let sep = match en_sep {
            " and " | ", " => "、",
            "; " => "；",
            other => other,
        };
        return items.join(sep);
    }
    items.join(en_sep)
}

/// An `advice_for` surface id as the player reads it: the id itself in
/// English (as every message spelled it before), its name in Chinese.
#[must_use]
pub fn surface(advice_for: &str) -> String {
    if !is_zh() {
        return advice_for.to_string();
    }
    match advice_for {
        "combat_action" => "战斗",
        "card_reward" => "卡牌奖励",
        "card_select" => "选牌",
        "card_bundle" => "卡包",
        "map_select" => "地图",
        "shop" => "商店",
        "event" => "事件",
        "rest_site" => "休息处",
        "boss_relic" => "Boss遗物",
        "treasure" => "宝箱",
        "potion_swap" => "药水替换",
        "crystal_sphere" => "水晶球",
        "reward_screen" => "奖励界面",
        other => other,
    }
    .to_string()
}

/// A map node label (`sts2_runai::ppo::wire::map_labels`: `Monster`,
/// `Rest Site (left)`, `Elite (#4 from left)`) in the advisor's language,
/// with the room names the game's own map legend uses.
#[must_use]
pub fn map_label(en: &str) -> String {
    if !is_zh() {
        return en.to_string();
    }
    let (name, side) = match en.strip_suffix(')').and_then(|s| s.split_once(" (")) {
        Some((name, side)) => (name, Some(side)),
        None => (en, None),
    };
    let room = match name {
        "Monster" => "敌人",
        "Elite" => "精英",
        "Boss" => "Boss",
        "Rest Site" => "休息处",
        "Shop" => "商店",
        "Treasure" => "宝箱",
        "Event" => "事件",
        "Unknown" => "未知",
        "Ancient" => "先古之民",
        other => other,
    };
    let Some(side) = side else {
        return room.to_string();
    };
    let side = match side {
        "left" => "左".to_string(),
        "middle" => "中".to_string(),
        "right" => "右".to_string(),
        other => other
            .strip_prefix('#')
            .and_then(|s| s.strip_suffix(" from left"))
            .map_or_else(|| other.to_string(), |n| format!("左数第{n}个")),
    };
    format!("{room}（{side}）")
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
    fn english_is_the_key_until_a_request_asks_for_chinese() {
        assert_eq!(tr("End turn"), "End turn");
        set_language(Some("zh"));
        assert!(is_zh());
        assert_eq!(tr("End turn"), "结束回合");
        set_language(None);
        assert_eq!(tr("End turn"), "End turn");
    }

    /// Literal `tr("…")` / `trf("…"` / `tr_dyn("…")` keys in a source (the
    /// literal may follow on the next line).
    fn literal_keys(code: &str) -> Vec<String> {
        let mut out = Vec::new();
        for call in ["tr(", "trf(", "tr_dyn("] {
            let mut rest = code;
            while let Some(i) = rest.find(call) {
                let prev = rest[..i].chars().next_back();
                rest = &rest[i + call.len()..];
                if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                // The key may start on the next line (`trf(\n    "…"`).
                let Some(lit) = rest.trim_start().strip_prefix('"') else {
                    continue;
                };
                rest = lit;
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
            .filter(|n| {
                n.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
            })
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
        let map: HashMap<&str, &str> = zh::TABLE.iter().copied().collect();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        rs_files(&root.join("src"), &mut files);
        let sim = root.join("../sim-advisor/src");
        if sim.is_dir() {
            rs_files(&sim, &mut files);
        }
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
    fn no_key_is_translated_twice() {
        let mut seen: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
        let mut clashes = Vec::new();
        for (k, v) in zh::TABLE {
            if let Some(prev) = seen.insert(k, v) {
                clashes.push(format!("{k:?}: {prev:?} vs {v:?}"));
            }
        }
        assert!(clashes.is_empty(), "{}", clashes.join("\n"));
    }
}
