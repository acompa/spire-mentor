//! The env's part of `ppo.e2e.contracts` (schema `ironclad-e2e-v111` rev 2):
//! action kinds (`AK`), decision types (`DT`), classification
//! (`classify_ooc`, `classify_combat`), dedupe keys (`combat_class_keys`,
//! `ooc_class_keys`, `ooc_payloads`) and the open [`Selection`].
//!
//! Ids are wire values: never renumber. Keys are Python's canonical JSON
//! (`json.dumps(x, sort_keys=True)`, see [`crate::pyjson`]).

use serde_json::{json, Value};

use crate::pyjson::{self, as_int, get, get_null, list_or_empty, truthy};
use crate::snapshot;

/// A schema violation (`contracts.SchemaError`): an unknown action kind, a
/// kind outside its decision type, an option index outside its offer list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaError(pub String);

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SchemaError: {}", self.0)
    }
}

impl std::error::Error for SchemaError {}

fn err<T>(msg: impl Into<String>) -> Result<T, SchemaError> {
    Err(SchemaError(msg.into()))
}

/// Action kind ids (`contracts.AK`), the live ones.
#[allow(missing_docs)]
pub mod ak {
    pub const NONE: u8 = 0;
    pub const PLAY: u8 = 1;
    pub const PLAY_TARGET: u8 = 2;
    pub const END_TURN: u8 = 3;
    pub const POTION: u8 = 4;
    pub const POTION_TARGET: u8 = 5;
    pub const CHOOSE: u8 = 6;
    pub const CHOOSE_DONE: u8 = 7;
    pub const COMBAT_OTHER: u8 = 8;
    pub const MOVE: u8 = 16;
    pub const PICK_CARD_REWARD: u8 = 17;
    pub const SKIP_CARD_REWARD: u8 = 18;
    pub const SACRIFICE_CARD_REWARD: u8 = 19;
    pub const PICK_CARD_BUNDLE: u8 = 20;
    pub const PICK_RELIC_REWARD: u8 = 21;
    pub const SKIP_RELIC_REWARD: u8 = 22;
    pub const PICK_BOSS_RELIC: u8 = 23;
    pub const SHOP_BUY_CARD: u8 = 24;
    pub const SHOP_BUY_RELIC: u8 = 25;
    pub const SHOP_BUY_POTION: u8 = 26;
    pub const SHOP_REMOVE_CARD: u8 = 27;
    pub const LEAVE: u8 = 28;
    pub const REST_OPTION: u8 = 29;
    pub const SMITH_CARD: u8 = 31;
    pub const EVENT_CHOICE: u8 = 32;
    pub const COLLECT_TREASURE: u8 = 33;
    pub const POTION_SWAP_DISCARD: u8 = 34;
    pub const POTION_SWAP_DECLINE: u8 = 35;
    pub const REMOVE_DECK_CARD: u8 = 36;
    pub const UPGRADE_DECK_CARD: u8 = 37;
    pub const TRANSFORM_DECK_CARD: u8 = 38;
    pub const ENCHANT_DECK_CARD: u8 = 39;
    pub const GRANT_DECK_CARD: u8 = 40;
    pub const GRANT_CONFIRM: u8 = 41;
    /// The "ooc-potion" fill event's kinds (`contracts` section 4c, GH #1606).
    pub const USE_POTION_OOC: u8 = 54;
    pub const DISCARD_POTION: u8 = 55;
    /// The "#1069" fill event's kinds, the reward screen (`contracts`
    /// section 4d, GH #1620): the reserved ones, then the spares it names
    /// (`REWARD_KIND_NAMES`).
    pub const TAKE_REWARD_POTION: u8 = 48;
    pub const SKIP_REWARD_POTION: u8 = 49;
    pub const TAKE_REWARD_GOLD: u8 = 50;
    pub const SKIP_TREASURE: u8 = 51;
    pub const SKIP_REWARD_GOLD: u8 = 42;
    pub const TAKE_REWARD_RELIC: u8 = 43;
    pub const SKIP_REWARD_RELIC: u8 = 44;
    pub const TAKE_REWARD_CARD: u8 = 45;
    pub const SKIP_REWARD_CARD: u8 = 46;
    pub const PROCEED_REWARDS: u8 = 47;
    pub const TAKE_REWARD_SPECIAL_CARD: u8 = 58;
    pub const SKIP_REWARD_SPECIAL_CARD: u8 = 59;
    pub const TAKE_REWARD_CARD_REMOVAL: u8 = 60;
    pub const SKIP_REWARD_CARD_REMOVAL: u8 = 61;
}

/// `REWARD_SCREEN_KINDS`: a combat room's reward screen kinds (`contracts`
/// section 4d).
pub const REWARD_SCREEN_KINDS: [u8; 13] = [
    ak::TAKE_REWARD_POTION,
    ak::SKIP_REWARD_POTION,
    ak::TAKE_REWARD_GOLD,
    ak::SKIP_REWARD_GOLD,
    ak::TAKE_REWARD_RELIC,
    ak::SKIP_REWARD_RELIC,
    ak::TAKE_REWARD_CARD,
    ak::SKIP_REWARD_CARD,
    ak::PROCEED_REWARDS,
    ak::TAKE_REWARD_SPECIAL_CARD,
    ak::SKIP_REWARD_SPECIAL_CARD,
    ak::TAKE_REWARD_CARD_REMOVAL,
    ak::SKIP_REWARD_CARD_REMOVAL,
];

/// Decision type ids (`contracts.DT`), the live ones.
#[allow(missing_docs)]
pub mod dt {
    pub const NONE: u8 = 0;
    pub const COMBAT_TURN: u8 = 1;
    pub const COMBAT_CHOICE: u8 = 2;
    pub const NEOW: u8 = 8;
    pub const MAP: u8 = 9;
    pub const CARD_REWARD: u8 = 10;
    pub const CARD_BUNDLE: u8 = 11;
    pub const RELIC_REWARD: u8 = 12;
    pub const BOSS_RELIC: u8 = 13;
    pub const SHOP: u8 = 14;
    pub const REST: u8 = 15;
    pub const SMITH: u8 = 16;
    pub const EVENT: u8 = 17;
    pub const POTION_SWAP: u8 = 18;
    pub const CARD_REMOVE: u8 = 19;
    pub const CARD_UPGRADE: u8 = 20;
    pub const CARD_TRANSFORM: u8 = 21;
    pub const CARD_ENCHANT: u8 = 22;
    pub const CARD_GRANT: u8 = 23;
    pub const TREASURE: u8 = 24;
    /// The "#1069" fill event's types (`contracts` section 4d, GH #1620).
    pub const REWARD_SCREEN: u8 = 40;
    pub const TREASURE_CHOICE: u8 = 41;
}

/// Merged classes per decision, asserted (`MAX_OPTIONS`).
pub const MAX_OPTIONS: usize = 64;
/// Raw legal actions per decision, asserted (`MAX_MEMBERS`).
pub const MAX_MEMBERS: usize = 255;
/// A fight past this round is capped (`MAX_FIGHT_TURNS`).
pub const MAX_FIGHT_TURNS: i64 = 100;
/// Stored decisions per fight (`MAX_FIGHT_DECISIONS`).
pub const MAX_FIGHT_DECISIONS: usize = 1000;
/// Stored decisions per run (`MAX_RUN_DECISIONS`).
pub const MAX_RUN_DECISIONS: usize = 6000;

/// The name of a live action kind (`ACTION_KINDS[k].name`).
#[must_use]
#[allow(clippy::wildcard_imports)]
pub fn kind_name(k: u8) -> &'static str {
    use ak::*;
    match k {
        NONE => "NONE",
        PLAY => "PLAY",
        PLAY_TARGET => "PLAY_TARGET",
        END_TURN => "END_TURN",
        POTION => "POTION",
        POTION_TARGET => "POTION_TARGET",
        CHOOSE => "CHOOSE",
        CHOOSE_DONE => "CHOOSE_DONE",
        COMBAT_OTHER => "COMBAT_OTHER",
        MOVE => "MOVE",
        PICK_CARD_REWARD => "PICK_CARD_REWARD",
        SKIP_CARD_REWARD => "SKIP_CARD_REWARD",
        SACRIFICE_CARD_REWARD => "SACRIFICE_CARD_REWARD",
        PICK_CARD_BUNDLE => "PICK_CARD_BUNDLE",
        PICK_RELIC_REWARD => "PICK_RELIC_REWARD",
        SKIP_RELIC_REWARD => "SKIP_RELIC_REWARD",
        PICK_BOSS_RELIC => "PICK_BOSS_RELIC",
        SHOP_BUY_CARD => "SHOP_BUY_CARD",
        SHOP_BUY_RELIC => "SHOP_BUY_RELIC",
        SHOP_BUY_POTION => "SHOP_BUY_POTION",
        SHOP_REMOVE_CARD => "SHOP_REMOVE_CARD",
        LEAVE => "LEAVE",
        REST_OPTION => "REST_OPTION",
        SMITH_CARD => "SMITH_CARD",
        EVENT_CHOICE => "EVENT_CHOICE",
        COLLECT_TREASURE => "COLLECT_TREASURE",
        POTION_SWAP_DISCARD => "POTION_SWAP_DISCARD",
        POTION_SWAP_DECLINE => "POTION_SWAP_DECLINE",
        REMOVE_DECK_CARD => "REMOVE_DECK_CARD",
        UPGRADE_DECK_CARD => "UPGRADE_DECK_CARD",
        TRANSFORM_DECK_CARD => "TRANSFORM_DECK_CARD",
        ENCHANT_DECK_CARD => "ENCHANT_DECK_CARD",
        GRANT_DECK_CARD => "GRANT_DECK_CARD",
        GRANT_CONFIRM => "GRANT_CONFIRM",
        USE_POTION_OOC => "USE_POTION_OOC",
        DISCARD_POTION => "DISCARD_POTION",
        TAKE_REWARD_POTION => "TAKE_REWARD_POTION",
        SKIP_REWARD_POTION => "SKIP_REWARD_POTION",
        TAKE_REWARD_GOLD => "TAKE_REWARD_GOLD",
        SKIP_TREASURE => "SKIP_TREASURE",
        SKIP_REWARD_GOLD => "SKIP_REWARD_GOLD",
        TAKE_REWARD_RELIC => "TAKE_REWARD_RELIC",
        SKIP_REWARD_RELIC => "SKIP_REWARD_RELIC",
        TAKE_REWARD_CARD => "TAKE_REWARD_CARD",
        SKIP_REWARD_CARD => "SKIP_REWARD_CARD",
        PROCEED_REWARDS => "PROCEED_REWARDS",
        TAKE_REWARD_SPECIAL_CARD => "TAKE_REWARD_SPECIAL_CARD",
        SKIP_REWARD_SPECIAL_CARD => "SKIP_REWARD_SPECIAL_CARD",
        TAKE_REWARD_CARD_REMOVAL => "TAKE_REWARD_CARD_REMOVAL",
        SKIP_REWARD_CARD_REMOVAL => "SKIP_REWARD_CARD_REMOVAL",
        _ => "SPARE",
    }
}

/// The name of a live decision type (`DECISION_TYPES[d].name`).
#[must_use]
#[allow(clippy::wildcard_imports)]
pub fn dtype_name(d: u8) -> &'static str {
    use dt::*;
    match d {
        NONE => "NONE",
        COMBAT_TURN => "COMBAT_TURN",
        COMBAT_CHOICE => "COMBAT_CHOICE",
        NEOW => "NEOW",
        MAP => "MAP",
        CARD_REWARD => "CARD_REWARD",
        CARD_BUNDLE => "CARD_BUNDLE",
        RELIC_REWARD => "RELIC_REWARD",
        BOSS_RELIC => "BOSS_RELIC",
        SHOP => "SHOP",
        REST => "REST",
        SMITH => "SMITH",
        EVENT => "EVENT",
        POTION_SWAP => "POTION_SWAP",
        CARD_REMOVE => "CARD_REMOVE",
        CARD_UPGRADE => "CARD_UPGRADE",
        CARD_TRANSFORM => "CARD_TRANSFORM",
        CARD_ENCHANT => "CARD_ENCHANT",
        CARD_GRANT => "CARD_GRANT",
        TREASURE => "TREASURE",
        REWARD_SCREEN => "REWARD_SCREEN",
        TREASURE_CHOICE => "TREASURE_CHOICE",
        _ => "SPARE",
    }
}

/// The action kinds a decision type may offer (`DType.kinds`).
#[must_use]
#[allow(clippy::wildcard_imports)]
pub fn dtype_kinds(d: u8) -> &'static [u8] {
    use ak::*;
    match d {
        dt::COMBAT_TURN => &[PLAY, PLAY_TARGET, END_TURN, POTION, POTION_TARGET],
        dt::COMBAT_CHOICE => &[CHOOSE, CHOOSE_DONE],
        dt::NEOW | dt::EVENT => &[EVENT_CHOICE],
        dt::MAP => &[MOVE],
        dt::CARD_REWARD => &[PICK_CARD_REWARD, SKIP_CARD_REWARD, SACRIFICE_CARD_REWARD],
        dt::CARD_BUNDLE => &[PICK_CARD_BUNDLE],
        dt::RELIC_REWARD => &[PICK_RELIC_REWARD, SKIP_RELIC_REWARD],
        dt::BOSS_RELIC => &[PICK_BOSS_RELIC],
        dt::SHOP => &[
            SHOP_BUY_CARD,
            SHOP_BUY_RELIC,
            SHOP_BUY_POTION,
            SHOP_REMOVE_CARD,
            LEAVE,
        ],
        dt::REST => &[REST_OPTION, LEAVE],
        dt::SMITH => &[SMITH_CARD],
        dt::POTION_SWAP => &[POTION_SWAP_DISCARD, POTION_SWAP_DECLINE],
        dt::CARD_REMOVE => &[REMOVE_DECK_CARD],
        dt::CARD_UPGRADE => &[UPGRADE_DECK_CARD],
        dt::CARD_TRANSFORM => &[TRANSFORM_DECK_CARD],
        dt::CARD_ENCHANT => &[ENCHANT_DECK_CARD],
        dt::CARD_GRANT => &[GRANT_DECK_CARD, GRANT_CONFIRM],
        dt::TREASURE => &[COLLECT_TREASURE],
        // The hashed sets; `dtype_allows` adds the reward screen's spares.
        dt::REWARD_SCREEN => &[TAKE_REWARD_POTION, SKIP_REWARD_POTION, TAKE_REWARD_GOLD],
        dt::TREASURE_CHOICE => &[COLLECT_TREASURE, SKIP_TREASURE],
        _ => &[],
    }
}

/// Whether `d` is a combat decision type.
#[must_use]
pub const fn is_combat_dtype(d: u8) -> bool {
    matches!(d, dt::COMBAT_TURN | dt::COMBAT_CHOICE)
}

/// `_OOC_SIMPLE`: run-action name → kind, for the kinds that need no context.
#[allow(clippy::wildcard_imports)]
fn ooc_simple(a: &str) -> Option<u8> {
    use ak::*;
    Some(match a {
        "move" => MOVE,
        "sacrifice_card_reward" => SACRIFICE_CARD_REWARD,
        "pick_relic_reward" => PICK_RELIC_REWARD,
        "skip_relic" => SKIP_RELIC_REWARD,
        "pick_boss_relic" => PICK_BOSS_RELIC,
        "shop_buy_card" => SHOP_BUY_CARD,
        "shop_buy_relic" => SHOP_BUY_RELIC,
        "shop_buy_potion" => SHOP_BUY_POTION,
        "shop_remove_card" => SHOP_REMOVE_CARD,
        "leave_shop" => LEAVE,
        "rest_option" => REST_OPTION,
        "smith_card" => SMITH_CARD,
        "event_choice" => EVENT_CHOICE,
        "collect_treasure" => COLLECT_TREASURE,
        "remove_deck_card" => REMOVE_DECK_CARD,
        "upgrade_deck_card" => UPGRADE_DECK_CARD,
        "transform_deck_card" => TRANSFORM_DECK_CARD,
        "enchant_deck_card" => ENCHANT_DECK_CARD,
        // The "ooc-potion" fill event (`contracts` section 4c); never a leading kind.
        "use_potion_ooc" => USE_POTION_OOC,
        "discard_potion" => DISCARD_POTION,
        _ => return None,
    })
}

/// `_DTYPE_BY_KIND`: leading kind → decision type, checked in this order.
const DTYPE_BY_KIND: [(u8, u8); 27] = [
    (ak::REST_OPTION, dt::REST),
    (ak::SHOP_BUY_CARD, dt::SHOP),
    (ak::SHOP_BUY_RELIC, dt::SHOP),
    (ak::SHOP_BUY_POTION, dt::SHOP),
    (ak::SHOP_REMOVE_CARD, dt::SHOP),
    (ak::LEAVE, dt::SHOP),
    (ak::MOVE, dt::MAP),
    (ak::PICK_CARD_BUNDLE, dt::CARD_BUNDLE),
    (ak::PICK_CARD_REWARD, dt::CARD_REWARD),
    (ak::SKIP_CARD_REWARD, dt::CARD_REWARD),
    (ak::SACRIFICE_CARD_REWARD, dt::CARD_REWARD),
    (ak::PICK_RELIC_REWARD, dt::RELIC_REWARD),
    (ak::SKIP_RELIC_REWARD, dt::RELIC_REWARD),
    (ak::PICK_BOSS_RELIC, dt::BOSS_RELIC),
    (ak::SMITH_CARD, dt::SMITH),
    (ak::POTION_SWAP_DISCARD, dt::POTION_SWAP),
    (ak::POTION_SWAP_DECLINE, dt::POTION_SWAP),
    (ak::REMOVE_DECK_CARD, dt::CARD_REMOVE),
    (ak::UPGRADE_DECK_CARD, dt::CARD_UPGRADE),
    (ak::TRANSFORM_DECK_CARD, dt::CARD_TRANSFORM),
    (ak::ENCHANT_DECK_CARD, dt::CARD_ENCHANT),
    (ak::GRANT_DECK_CARD, dt::CARD_GRANT),
    (ak::GRANT_CONFIRM, dt::CARD_GRANT),
    // The reward screen (`contracts` section 4d): a combat room's always
    // offers proceed, the chest's always its skip.
    (ak::PROCEED_REWARDS, dt::REWARD_SCREEN),
    (ak::SKIP_TREASURE, dt::TREASURE_CHOICE),
    (ak::COLLECT_TREASURE, dt::TREASURE),
    (ak::EVENT_CHOICE, dt::EVENT),
];

fn action_name(a: &Value) -> Option<&str> {
    get(a, "action").and_then(Value::as_str)
}

/// `_reward_action_kind(action)`: a reward-screen action's kind, from the
/// reward it names (`contracts` section 4d, GH #1620).
fn reward_action_kind(action: &Value, a: &str) -> Result<u8, SchemaError> {
    use ak::{
        COLLECT_TREASURE, PROCEED_REWARDS, SKIP_REWARD_CARD, SKIP_REWARD_CARD_REMOVAL,
        SKIP_REWARD_GOLD, SKIP_REWARD_POTION, SKIP_REWARD_RELIC, SKIP_REWARD_SPECIAL_CARD,
        SKIP_TREASURE, TAKE_REWARD_CARD, TAKE_REWARD_CARD_REMOVAL, TAKE_REWARD_GOLD,
        TAKE_REWARD_POTION, TAKE_REWARD_RELIC, TAKE_REWARD_SPECIAL_CARD,
    };
    if a == "proceed_rewards" {
        return Ok(if get(action, "chest").is_some_and(truthy) {
            SKIP_TREASURE
        } else {
            PROCEED_REWARDS
        });
    }
    let take = a == "take_reward";
    let reward = get(action, "reward").and_then(Value::as_str);
    Ok(match (reward, take) {
        (Some("chest_relic"), true) => COLLECT_TREASURE,
        (Some("chest_relic"), false) => SKIP_TREASURE,
        (Some("gold"), true) => TAKE_REWARD_GOLD,
        (Some("gold"), false) => SKIP_REWARD_GOLD,
        (Some("potion"), true) => TAKE_REWARD_POTION,
        (Some("potion"), false) => SKIP_REWARD_POTION,
        (Some("relic"), true) => TAKE_REWARD_RELIC,
        (Some("relic"), false) => SKIP_REWARD_RELIC,
        (Some("card"), true) => TAKE_REWARD_CARD,
        (Some("card"), false) => SKIP_REWARD_CARD,
        (Some("special_card"), true) => TAKE_REWARD_SPECIAL_CARD,
        (Some("special_card"), false) => SKIP_REWARD_SPECIAL_CARD,
        (Some("card_removal"), true) => TAKE_REWARD_CARD_REMOVAL,
        (Some("card_removal"), false) => SKIP_REWARD_CARD_REMOVAL,
        _ => return err(format!("{a} names no known reward: {reward:?}")),
    })
}

/// `ooc_action_kind(action, bundles, grant_n_options)`.
///
/// # Errors
///
/// An unknown kind, a bundle skip, or a grant index outside its options.
pub fn ooc_action_kind(
    action: &Value,
    bundles: bool,
    grant_n_options: Option<usize>,
) -> Result<u8, SchemaError> {
    let a = action_name(action);
    match a {
        Some("pick_card_reward") => {
            if get_null(action, "idx").is_null() {
                if bundles {
                    return err("a card bundle offers no skip");
                }
                return Ok(ak::SKIP_CARD_REWARD);
            }
            Ok(if bundles {
                ak::PICK_CARD_BUNDLE
            } else {
                ak::PICK_CARD_REWARD
            })
        }
        Some("potion_swap") => Ok(if get_null(action, "discard_slot").is_null() {
            ak::POTION_SWAP_DECLINE
        } else {
            ak::POTION_SWAP_DISCARD
        }),
        Some("grant_deck_card") => {
            let Some(n) = grant_n_options else {
                return err("grant_deck_card needs grant_n_options");
            };
            let idx = get(action, "idx").and_then(as_int).unwrap_or(-1);
            if usize::try_from(idx).ok() == Some(n) {
                return Ok(ak::GRANT_CONFIRM);
            }
            if !(0..i64::try_from(n).unwrap_or(i64::MAX)).contains(&idx) {
                return err(format!("grant_deck_card idx {idx} outside 0..{n}"));
            }
            Ok(ak::GRANT_DECK_CARD)
        }
        Some(r @ ("take_reward" | "skip_reward" | "proceed_rewards")) => {
            reward_action_kind(action, r)
        }
        other => ooc_simple(other.unwrap_or("")).map_or_else(
            || err(format!("unknown out-of-combat action kind {other:?}")),
            Ok,
        ),
    }
}

/// `combat_action_kind(action)`: the kind of one `cs.legal_actions()` entry.
///
/// # Errors
///
/// An unknown combat kind.
pub fn combat_action_kind(action: &Value) -> Result<u8, SchemaError> {
    let tgt_none = get_null(action, "target_index").is_null();
    match action_name(action) {
        Some("play") => Ok(if tgt_none { ak::PLAY } else { ak::PLAY_TARGET }),
        Some("end_turn") => Ok(ak::END_TURN),
        Some("potion" | "drink_potion") => Ok(if tgt_none {
            ak::POTION
        } else {
            ak::POTION_TARGET
        }),
        Some("choose") => Ok(if get(action, "selection").is_some_and(truthy) {
            ak::CHOOSE
        } else {
            ak::CHOOSE_DONE
        }),
        Some("channel" | "evoke") => Ok(ak::COMBAT_OTHER),
        other => err(format!("unknown combat action kind {other:?}")),
    }
}

/// `OOC_POTION_HOST_DTYPES`: the decision types whose screens offer the
/// "ooc-potion" kinds (`contracts` section 4c, GH #1606).
const OOC_POTION_HOST_DTYPES: [u8; 12] = [
    dt::NEOW,
    dt::MAP,
    dt::CARD_REWARD,
    dt::CARD_BUNDLE,
    dt::RELIC_REWARD,
    dt::BOSS_RELIC,
    dt::SHOP,
    dt::REST,
    dt::EVENT,
    dt::TREASURE,
    dt::REWARD_SCREEN,
    dt::TREASURE_CHOICE,
];

/// `contracts.dtype_kinds(d)`: whether a decision of type `d` may offer `k`
/// (`DType.kinds`, plus the "ooc-potion" kinds on their host screens and the
/// reward screen's kinds, `contracts` section 4d).
fn dtype_allows(d: u8, k: u8) -> bool {
    dtype_kinds(d).contains(&k)
        || (d == dt::REWARD_SCREEN && REWARD_SCREEN_KINDS.contains(&k))
        || (matches!(k, ak::USE_POTION_OOC | ak::DISCARD_POTION)
            && OOC_POTION_HOST_DTYPES.contains(&d))
}

/// `check_kinds(dtype, kinds)`.
///
/// # Errors
///
/// A kind `dtype` cannot offer.
pub fn check_kinds(d: u8, kinds: &[u8]) -> Result<(), SchemaError> {
    let mut bad: Vec<&str> = kinds
        .iter()
        .filter(|&&k| !dtype_allows(d, k))
        .map(|&k| kind_name(k))
        .collect();
    bad.sort_unstable();
    bad.dedup();
    if bad.is_empty() {
        Ok(())
    } else {
        err(format!("{} cannot offer {bad:?}", dtype_name(d)))
    }
}

/// `classify_ooc(actions, event_name, bundles, grant_n_options)`:
/// `(dtype, kinds)` of an out-of-combat decision.
///
/// # Errors
///
/// No action, an unknown kind, or no decision type for the kinds.
pub fn classify_ooc(
    actions: &[Value],
    event_name: Option<&str>,
    bundles: bool,
    grant_n_options: Option<usize>,
) -> Result<(u8, Vec<u8>), SchemaError> {
    if actions.is_empty() {
        return err("a decision needs at least one action");
    }
    let kinds = actions
        .iter()
        .map(|a| ooc_action_kind(a, bundles, grant_n_options))
        .collect::<Result<Vec<_>, _>>()?;
    for (kind, d) in DTYPE_BY_KIND {
        if kinds.contains(&kind) {
            let d = if d == dt::EVENT && event_name == Some("Neow") {
                dt::NEOW
            } else {
                d
            };
            check_kinds(d, &kinds)?;
            return Ok((d, kinds));
        }
    }
    err("no decision type for the action kinds")
}

/// `classify_combat(legal, pending_choice)`: `(dtype, kinds)` of a combat
/// decision.
///
/// # Errors
///
/// No action, or an unknown or out-of-type kind.
pub fn classify_combat(
    legal: &[Value],
    has_pending_choice: bool,
) -> Result<(u8, Vec<u8>), SchemaError> {
    if legal.is_empty() {
        return err("a decision needs at least one action");
    }
    let kinds = legal
        .iter()
        .map(combat_action_kind)
        .collect::<Result<Vec<_>, _>>()?;
    let d = if has_pending_choice {
        dt::COMBAT_CHOICE
    } else {
        dt::COMBAT_TURN
    };
    check_kinds(d, &kinds)?;
    Ok((d, kinds))
}

/// `card_key(card)`: the visible identity of a card, `_canon(_card(card))`.
#[must_use]
pub fn card_key(card: &Value) -> String {
    pyjson::dumps(&snapshot::card(card))
}

fn choice_options(choice: &Value) -> &[Value] {
    for k in ["cards", "options"] {
        if let Some(Value::Array(a)) = get(choice, k) {
            if !a.is_empty() {
                return a;
            }
        }
    }
    &[]
}

/// `choice_option_key(choice, s, piles)`: the full card (`card_key`) when
/// option `s` sits in a pile the player sees, else the option's own fields.
///
/// # Errors
///
/// `s` outside the choice's options.
pub fn choice_option_key(choice: &Value, s: usize, piles: &Value) -> Result<String, SchemaError> {
    let Some(opt) = choice_options(choice).get(s) else {
        return err(format!("choice option {s} out of range"));
    };
    let pile_name = match get(opt, "pile").and_then(Value::as_str) {
        Some("hand") => Some("hand"),
        Some("draw") => Some("draw_pile"),
        Some("discard") => Some("discard_pile"),
        Some("exhaust") => Some("exhaust_pile"),
        _ => None,
    };
    let pile = pile_name
        .and_then(|p| get(piles, p))
        .and_then(Value::as_array);
    if let (Some(pile), Some(Value::Number(n))) = (pile, get(opt, "index")) {
        if let Some(c) = n
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| pile.get(i))
        {
            // The pile is part of the key: the tokenizer encodes it
            // (`choice_pile`), so a hand copy and a draw copy of one card are
            // two options (#1097, TEAR_ASUNDER offered from both).
            return Ok(pyjson::dumps(&json!({
                "pile": get_null(opt, "pile"),
                "card": card_key(c),
            })));
        }
    }
    Ok(pyjson::dumps(&json!({
        "id": get_null(opt, "id"),
        "upgraded": get_null(opt, "upgraded"),
        "type": get_null(opt, "type"),
        "pile": get_null(opt, "pile"),
    })))
}

fn selection_indices(a: &Value) -> Vec<usize> {
    list_or_empty(a, "selection")
        .iter()
        .filter_map(|x| as_int(x).and_then(|i| usize::try_from(i).ok()))
        .collect()
}

/// The sorted option keys a `choose` action selects.
///
/// # Errors
///
/// `_in_unordered_pile`: option `s` is a card of the draw, discard or
/// exhaust pile ([`choice_option_key`]'s full-card branch, not the hand).
fn in_unordered_pile(choice: &Value, s: usize, piles: &Value) -> bool {
    let Some(opt) = choice_options(choice).get(s) else {
        return false;
    };
    let pile_name = match get(opt, "pile").and_then(Value::as_str) {
        Some("draw") => "draw_pile",
        Some("discard") => "discard_pile",
        Some("exhaust") => "exhaust_pile",
        _ => return false,
    };
    let len = get(piles, pile_name)
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    get(opt, "index")
        .and_then(Value::as_u64)
        .and_then(|i| usize::try_from(i).ok())
        .is_some_and(|i| i < len)
}

/// A selected option out of range.
pub fn selected_keys(a: &Value, choice: &Value, piles: &Value) -> Result<Vec<String>, SchemaError> {
    let mut sel = selection_indices(a)
        .into_iter()
        .map(|x| choice_option_key(choice, x, piles))
        .collect::<Result<Vec<_>, _>>()?;
    sel.sort();
    Ok(sel)
}

/// `combat_class_keys(legal, hand, potions, choice, outcome, piles)`: one
/// merge key per legal combat action (`legal` in the belief view's index
/// space). `outcome(i)` is the choose-outcome key of action `i`.
///
/// # Errors
///
/// An unknown kind, a choose without a choice, an out-of-range index, or an
/// `outcome` failure.
pub fn combat_class_keys(
    legal: &[Value],
    hand: &[Value],
    potions: &[Value],
    choice: Option<&Value>,
    outcome: &mut dyn FnMut(usize) -> Result<String, SchemaError>,
    piles: &Value,
) -> Result<Vec<String>, SchemaError> {
    let mut keys = Vec::with_capacity(legal.len());
    for (i, a) in legal.iter().enumerate() {
        let kind = combat_action_kind(a)?;
        let key = match kind {
            ak::PLAY | ak::PLAY_TARGET => {
                let ci = get(a, "card_index")
                    .and_then(as_int)
                    .and_then(|c| usize::try_from(c).ok());
                let Some(c) = ci.and_then(|c| hand.get(c)) else {
                    return err(format!("play card_index outside the hand: {a}"));
                };
                pyjson::dumps(&json!(["play", card_key(c), get_null(a, "target_index")]))
            }
            ak::POTION | ak::POTION_TARGET => {
                let slot = get(a, "slot")
                    .and_then(as_int)
                    .and_then(|s| usize::try_from(s).ok());
                let pid = slot
                    .and_then(|s| potions.get(s))
                    .cloned()
                    .unwrap_or(Value::Null);
                pyjson::dumps(&json!(["potion", pid, get_null(a, "target_index")]))
            }
            ak::CHOOSE | ak::CHOOSE_DONE => {
                let Some(ch) = choice else {
                    return err("choose actions need choice and outcome");
                };
                let sel = selected_keys(a, ch, piles)?;
                // GH #1800: a pick made only of draw/discard/exhaust cards
                // (sent sorted: identical copies are one card to the player)
                // keys on content alone; its outcome adds only how the
                // engine's uids order the pile under a random draw.
                let picks = selection_indices(a);
                let unordered =
                    !picks.is_empty() && picks.iter().all(|&x| in_unordered_pile(ch, x, piles));
                let out = if unordered {
                    String::new()
                } else {
                    outcome(i)?
                };
                pyjson::dumps(&json!(["choose", out, sel]))
            }
            k => pyjson::dumps(&json!([kind_name(k), i])),
        };
        keys.push(key);
    }
    Ok(keys)
}

/// Kinds whose payload is pure content, so equal payloads merge
/// (`ooc_class_keys`' `mergeable`).
const MERGEABLE: [u8; 12] = [
    ak::PICK_CARD_REWARD,
    ak::SHOP_BUY_CARD,
    ak::SHOP_BUY_RELIC,
    ak::SHOP_BUY_POTION,
    ak::SHOP_REMOVE_CARD,
    ak::SMITH_CARD,
    ak::POTION_SWAP_DISCARD,
    ak::REMOVE_DECK_CARD,
    ak::UPGRADE_DECK_CARD,
    ak::TRANSFORM_DECK_CARD,
    ak::ENCHANT_DECK_CARD,
    ak::GRANT_DECK_CARD,
];

/// `ooc_class_keys(kinds, payloads)`: one key per out-of-combat option.
///
/// # Errors
///
/// `kinds` and `payloads` differ in length.
pub fn ooc_class_keys(kinds: &[u8], payloads: &[Value]) -> Result<Vec<String>, SchemaError> {
    if kinds.len() != payloads.len() {
        return err("kinds and payloads differ in length");
    }
    Ok(kinds
        .iter()
        .zip(payloads)
        .enumerate()
        .map(|(i, (&k, p))| {
            if MERGEABLE.contains(&k) {
                pyjson::dumps(&json!([k, p]))
            } else {
                pyjson::dumps(&json!([k, "pos", i]))
            }
        })
        .collect())
}

/// `_at(xs, i)`: `xs[int(i)]`, a [`SchemaError`] when out of range.
fn at<'a>(xs: Option<&'a Value>, i: &Value) -> Result<&'a Value, SchemaError> {
    let idx = as_int(i);
    let out = match (xs, idx) {
        (Some(Value::Array(a)), Some(n)) => {
            // Python indexes negative ints from the end.
            let n = if n < 0 {
                i64::try_from(a.len()).unwrap_or(0) + n
            } else {
                n
            };
            usize::try_from(n).ok().and_then(|n| a.get(n))
        }
        _ => None,
    };
    out.ok_or_else(|| SchemaError(format!("option index {i} outside its offer list")))
}

/// `xs[int(i)] if int(i) < len(xs) else None` over `obs.get(field) or []`.
fn aligned(obs: &Value, field: &str, i: &Value) -> Value {
    let xs = list_or_empty(obs, field);
    as_int(i)
        .and_then(|n| usize::try_from(n).ok())
        .and_then(|n| xs.get(n))
        .cloned()
        .unwrap_or(Value::Null)
}

/// `obs[field]`: a missing key is an error (a `KeyError` in Python).
fn required<'a>(obs: &'a Value, field: &str) -> Result<&'a Value, SchemaError> {
    get(obs, field).ok_or_else(|| SchemaError(format!("KeyError: obs[{field:?}]")))
}

fn deck_card(obs: &Value, deck_i: &Value) -> Result<Value, SchemaError> {
    let e = aligned(obs, "deck_enchantments", deck_i);
    Ok(json!([at(Some(required(obs, "deck")?), deck_i)?, e]))
}

/// `ooc_payloads(obs, actions, kinds)`: the visible payload of every
/// out-of-combat option, from the decision's observation
/// ([`crate::env::observe`]).
///
/// # Errors
///
/// An index outside its offer list, or a field the observation lacks.
pub fn ooc_payloads(
    obs: &Value,
    actions: &[Value],
    kinds: &[u8],
) -> Result<Vec<Value>, SchemaError> {
    let mut out = Vec::with_capacity(actions.len());
    for (a, &k) in actions.iter().zip(kinds) {
        let idx = get_null(a, "idx");
        let p = match k {
            ak::PICK_CARD_REWARD | ak::PICK_CARD_BUNDLE => json!([
                at(
                    get(obs, "card_reward_option_ids").filter(|v| truthy(v)),
                    &idx
                )?,
                aligned(obs, "card_reward_option_upgraded", &idx),
                aligned(obs, "card_reward_option_enchantments", &idx),
            ]),
            ak::SMITH_CARD => json!([
                at(Some(required(obs, "smith_options")?), &idx)?,
                aligned(obs, "smith_option_enchantments", &idx),
            ]),
            ak::REMOVE_DECK_CARD => json!([
                at(Some(required(obs, "card_removal_options")?), &idx)?,
                aligned(obs, "card_removal_option_enchantments", &idx),
            ]),
            ak::UPGRADE_DECK_CARD | ak::TRANSFORM_DECK_CARD | ak::ENCHANT_DECK_CARD => {
                let field = match k {
                    ak::UPGRADE_DECK_CARD => "upgrade_select_deck_indices",
                    ak::TRANSFORM_DECK_CARD => "transform_select_deck_indices",
                    _ => "enchant_select_deck_indices",
                };
                let deck_i = at(Some(required(obs, field)?), &idx)?.clone();
                deck_card(obs, &deck_i)?
            }
            ak::GRANT_DECK_CARD => json!([
                at(Some(required(obs, "card_grant_options")?), &idx)?,
                aligned(obs, "card_grant_option_enchantments", &idx),
            ]),
            ak::SHOP_BUY_CARD | ak::SHOP_BUY_RELIC | ak::SHOP_BUY_POTION => {
                let shelf = match k {
                    ak::SHOP_BUY_CARD => "cards",
                    ak::SHOP_BUY_RELIC => "relics",
                    _ => "potions",
                };
                let shop = required(obs, "shop")?;
                let want = as_int(&idx);
                let item = get(shop, shelf)
                    .and_then(Value::as_array)
                    .and_then(|items| {
                        // `{int(x["idx"]): x}`: the last item with that idx wins.
                        items
                            .iter()
                            .rev()
                            .find(|x| get(x, "idx").and_then(as_int) == want)
                    })
                    .ok_or_else(|| {
                        SchemaError(format!("option index {idx} outside its offer list"))
                    })?;
                let mut m = item.as_object().cloned().unwrap_or_default();
                m.remove("idx");
                Value::Object(m)
            }
            ak::SHOP_REMOVE_CARD => deck_card(obs, &get_null(a, "deck_idx"))?,
            ak::POTION_SWAP_DISCARD => at(
                Some(required(obs, "potions")?),
                &get_null(a, "discard_slot"),
            )?
            .clone(),
            _ => Value::Null,
        };
        out.push(p);
    }
    Ok(out)
}

/// `classes_from_keys(keys)`: dense class ids in first-occurrence order, and
/// each class's members in action order.
#[must_use]
pub fn classes_from_keys(keys: &[String]) -> (Vec<usize>, Vec<Vec<usize>>) {
    let mut ids: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    let mut cls = Vec::with_capacity(keys.len());
    for (i, k) in keys.iter().enumerate() {
        let c = *ids.entry(k.as_str()).or_insert_with(|| {
            members.push(Vec::new());
            members.len() - 1
        });
        members[c].push(i);
        cls.push(c);
    }
    (cls, members)
}

/// The open multi-pick selection (`contracts.Selection`): RUNCTX
/// `R_SEL_MIN/MAX/PICKED/CAN_SKIP`. Set on `COMBAT_CHOICE` and `CARD_GRANT`
/// only; every other decision carries the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// `min_select` (combat) / `pending_card_grant_min` (grant).
    pub min: i64,
    /// `max_select` (combat) / the grant's offered cards.
    pub max: i64,
    /// Picks already made.
    pub picked: i64,
    /// A CHOOSE_DONE / GRANT_CONFIRM option is offered.
    pub can_skip: bool,
}

impl Selection {
    /// As the fixture records it (`dataclasses.asdict`).
    #[must_use]
    pub fn to_value(&self) -> Value {
        json!({"min": self.min, "max": self.max, "picked": self.picked, "can_skip": self.can_skip})
    }
}

/// `combat_selection(choice, kinds)`: the open selection of a combat choice.
#[must_use]
pub fn combat_selection(choice: Option<&Value>, kinds: &[u8]) -> Selection {
    let Some(ch) = choice else {
        return Selection::default();
    };
    let n = i64::try_from(choice_options(ch).len()).unwrap_or(0);
    let num = |k: &str| {
        get(ch, k)
            .filter(|v| truthy(v))
            .and_then(as_int)
            .unwrap_or(0)
    };
    #[allow(clippy::naive_bytecount)]
    let chooses = i64::try_from(kinds.iter().filter(|&&k| k == ak::CHOOSE).count()).unwrap_or(0);
    Selection {
        min: num("min_select"),
        max: num("max_select"),
        picked: n - chooses,
        can_skip: kinds.contains(&ak::CHOOSE_DONE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reward screen ("#1069", GH #1620): each take / skip by the reward
    /// it names, a combat room's screen as REWARD_SCREEN, the chest's as
    /// TREASURE_CHOICE (its proceed leaves the relic, as its skip does).
    #[test]
    fn the_reward_screen_classifies_by_the_reward_each_action_names() {
        use serde_json::json as j;
        let acts = [
            j!({"action": "take_reward", "idx": 0, "reward": "gold", "amount": 12}),
            j!({"action": "take_reward", "idx": 1, "reward": "relic", "id": "Gorget"}),
            j!({"action": "take_reward", "idx": 2, "reward": "card"}),
            j!({"action": "skip_reward", "idx": 0, "reward": "gold", "amount": 12}),
            j!({"action": "skip_reward", "idx": 1, "reward": "relic", "id": "Gorget"}),
            j!({"action": "skip_reward", "idx": 2, "reward": "card"}),
            j!({"action": "proceed_rewards", "chest": false}),
            j!({"action": "discard_potion", "potion_slot": 0}),
        ];
        let (d, kinds) = classify_ooc(&acts, None, false, None).unwrap();
        assert_eq!(d, dt::REWARD_SCREEN);
        assert_eq!(
            kinds,
            vec![
                ak::TAKE_REWARD_GOLD,
                ak::TAKE_REWARD_RELIC,
                ak::TAKE_REWARD_CARD,
                ak::SKIP_REWARD_GOLD,
                ak::SKIP_REWARD_RELIC,
                ak::SKIP_REWARD_CARD,
                ak::PROCEED_REWARDS,
                ak::DISCARD_POTION,
            ]
        );
        let chest = [
            j!({"action": "take_reward", "idx": 0, "reward": "chest_relic", "id": "Gorget"}),
            j!({"action": "skip_reward", "idx": 0, "reward": "chest_relic", "id": "Gorget"}),
            j!({"action": "proceed_rewards", "chest": true}),
        ];
        assert_eq!(
            classify_ooc(&chest, None, false, None).unwrap(),
            (
                dt::TREASURE_CHOICE,
                vec![ak::COLLECT_TREASURE, ak::SKIP_TREASURE, ak::SKIP_TREASURE]
            )
        );
        assert_eq!(dtype_name(dt::REWARD_SCREEN), "REWARD_SCREEN");
        assert_eq!(kind_name(ak::PROCEED_REWARDS), "PROCEED_REWARDS");
        let unnamed = [j!({"action": "take_reward", "idx": 0})];
        assert!(classify_ooc(&unnamed, None, false, None).is_err());
    }

    /// The "ooc-potion" kinds ride on the screen's decision (GH #1606) and
    /// leave the decision type names alone.
    #[test]
    fn the_potion_popup_rides_on_the_screen_decision() {
        let acts = [
            serde_json::json!({"action": "move", "col": 1}),
            serde_json::json!({"action": "use_potion_ooc", "potion_slot": 0}),
            serde_json::json!({"action": "discard_potion", "potion_slot": 1}),
        ];
        let (d, kinds) = classify_ooc(&acts, None, false, None).unwrap();
        assert_eq!(
            (d, kinds),
            (
                dt::MAP,
                vec![ak::MOVE, ak::USE_POTION_OOC, ak::DISCARD_POTION]
            )
        );
        assert_eq!(kind_name(ak::USE_POTION_OOC), "USE_POTION_OOC");
        assert_eq!(kind_name(ak::DISCARD_POTION), "DISCARD_POTION");
        assert_eq!(dtype_name(43), "SPARE");
        assert!(check_kinds(dt::CARD_REMOVE, &[ak::REMOVE_DECK_CARD, ak::DISCARD_POTION]).is_err());
    }

    /// The tokenizer encodes a choose option's pile (`choice_pile`), so the
    /// same card in hand and in the draw pile must not merge (#1097: eval seed
    /// 160001757, a card_select offering a hand and a draw copy of
    /// TEAR_ASUNDER). Mirrors `test_a_choice_option_keys_on_its_pile`.
    #[test]
    fn a_choice_option_keys_on_its_pile() {
        let card = json!({"id": "TEAR_ASUNDER", "kind": "card", "upgraded": false, "base_damage": 5, "cost": 2});
        let piles = json!({"hand": [card.clone()], "draw_pile": [card]});
        let opt =
            |pile: &str| json!({"id": "TEAR_ASUNDER", "index": 0, "pile": pile, "upgraded": false});
        let choice = json!({"kind": "add_from_draw_to_hand", "cards": [opt("hand"), opt("draw"), opt("draw")]});
        let keys: Vec<String> = (0..3)
            .map(|s| choice_option_key(&choice, s, &piles).expect("key"))
            .collect();
        assert_ne!(keys[0], keys[1]);
        assert_eq!(keys[1], keys[2]);
    }
}
