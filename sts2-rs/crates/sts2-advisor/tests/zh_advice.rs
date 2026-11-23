//! A game in Chinese gets the advisor's words in Chinese (PROTOCOL.md v5.4
//! `language`), and a game in English the same bytes as before.
//!
//! Gated on the game's Chinese tables beside the English ones
//! (`sts2-pycli/localization_zhs`); the language is process-wide, which
//! nextest's one process per test keeps to this test.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sts2_advisor::display_names::{self, Names};
use sts2_advisor::{coach, i18n, screen_advice};

fn data_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sts2-pycli")
}

/// The tables, or `None` (skip) when this checkout has no Chinese ones.
fn names() -> Option<Names> {
    let root = data_root();
    if !root.join("localization_zhs").join("cards.json").is_file() {
        eprintln!("skipped: no localization_zhs under {}", root.display());
        return None;
    }
    let names = display_names::load(&root);
    assert!(names.has_zh());
    Some(names)
}

/// A screen nothing ranks, its note composed by the advisor and its labels
/// spelled as ids, through the wire guard.
fn unranked_advice(names: &Names) -> Value {
    let state = json!({"options": [
        {"label": "remove:STRIKE_IRONCLAD"},
        {"label": "SHRUG_IT_OFF"},
    ]});
    let why = i18n::tr("no run-policy net is loaded");
    let (mut advice, _) = screen_advice::unranked(&state, why, None);
    advice["note"] = json!(format!(
        "{}{}",
        advice["note"].as_str().unwrap(),
        sts2_advisor::route::route_note(&[json!({"hp": 41})])
    ));
    names.sanitize_advice(&mut advice, Some("shop"), &state);
    advice
}

#[test]
fn a_chinese_game_gets_a_chinese_note_and_chinese_names() {
    let Some(names) = names() else { return };

    i18n::set_language(Some("zh"));
    let zh = unranked_advice(&names);
    assert_eq!(
        zh["note"],
        json!("没有加载对局策略网络 —— 默认选第一项，并非推荐 · 路线 1 层 → 41 生命（牌组/金币/遗物按当前不变）")
    );
    assert_eq!(zh["hand_back"], json!("没有加载对局策略网络"));
    assert_eq!(zh["options"][0]["label"], json!("移除 打击"));
    assert_eq!(zh["options"][1]["label"], json!("耸肩无视"));
    assert_eq!(zh["best"]["label"], json!("移除 打击"));
    assert_eq!(i18n::map_label("Rest Site (left)"), "休息处（左）");

    // A request without `language` is English again, byte for byte.
    i18n::set_language(None);
    let en = unranked_advice(&names);
    assert_eq!(
        en["note"],
        json!("no run-policy net is loaded — defaulting to the first option, NOT a recommendation · route 1 floors → 41 HP (deck/gold/relics frozen)")
    );
    assert_eq!(en["options"][0]["label"], json!("Remove Strike"));
    assert_eq!(en["options"][1]["label"], json!("Shrug It Off"));
}

/// A `turn` coach record as the grader assembles one: the moves, the
/// reason line and the turn's starting position.
fn turn_record(names: &Names) -> Value {
    let start = json!({
        "hand": [{"id": "BASH"}, {"id": "SHRUG_IT_OFF", "name": ""}],
        "player": {"energy": 3, "max_energy": 3, "hp": 70, "max_hp": 80, "block": 0},
        "enemies": [
            {"name": "Cultist", "hp": 48, "intents": [{"intent_type": "Buff"}]},
            {"name": "Cultist", "hp": 50, "intents": [{"damage": 6, "hits": 2}]},
        ],
    });
    let labels = vec!["A #1".to_string(), "A #2".to_string()];
    let why = coach::why_text(
        &[("kills".to_string(), 0)],
        &[("wakes".to_string(), 1), ("wins".to_string(), -1)],
        &labels,
        &json!(3),
    );
    let mut record = json!({
        "kind": "turn",
        "verdict": "slip",
        "you": ["BASH", i18n::tr("end turn")],
        "ai": ["SHRUG_IT_OFF", i18n::tr("end turn")],
        "why": why,
        "context": coach::turn_context(&start),
    });
    names.sanitize_coach(&mut record);
    record
}

#[test]
fn a_chinese_game_gets_a_chinese_coach_record() {
    let Some(names) = names() else { return };

    i18n::set_language(Some("zh"));
    let zh = turn_record(&names);
    assert_eq!(zh["you"], json!(["痛击", "结束回合"]));
    assert_eq!(zh["ai"], json!(["耸肩无视", "结束回合"]));
    assert_eq!(
        zh["why"],
        json!("你的打法击杀A #1；AI 的打法唤醒A #2、赢下战斗（第3回合）")
    );
    let ctx = &zh["context"];
    assert_eq!(ctx["hand"], json!(["痛击", "耸肩无视"]));
    assert_eq!(ctx["enemies"][0]["intent"], json!("强化"));
    assert_eq!(ctx["enemies"][1]["intent"], json!("攻击 6×2"));

    i18n::set_language(None);
    let en = turn_record(&names);
    assert_eq!(en["you"], json!(["Bash", "end turn"]));
    assert_eq!(
        en["why"],
        json!("Your line kills A #1; the AI's line wakes A #2 and wins the fight on turn 3")
    );
    assert_eq!(en["context"]["enemies"][0]["intent"], json!("buff"));
    assert_eq!(en["context"]["enemies"][1]["intent"], json!("attack 6×2"));
}
