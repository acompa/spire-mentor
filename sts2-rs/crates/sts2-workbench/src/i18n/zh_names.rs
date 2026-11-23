//! Chinese for the engine's own words (see `crate::i18n`). Keys are the
//! English text passed to `tr` / `trf` / `tr_dyn`.

pub(super) const TABLE: &[(&str, &str)] = &[
    // Intent tips the game's `intents.json` has no row for (`Hidden` has no
    // tip in the game) or whose row needs a count the wire does not carry.
    ("Hidden", "隐藏"),
    (
        "This enemy's intention is hidden.",
        "这个敌人的意图被隐藏了。",
    ),
    (
        "This enemy intends to give you Status cards.",
        "这个敌人将会给你状态牌。",
    ),
    (
        "This creature is trying to take you down with it before being destroyed.",
        "这个生物试图在被摧毁前拉你一起陪葬。",
    ),
    // An intent modifier that is neither a power nor a relic.
    ("Other", "其他"),
];
