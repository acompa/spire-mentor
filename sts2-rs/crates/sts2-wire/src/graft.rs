//! The graft dialect: park a rebuilt [`RunManager`] AT a decision the live
//! game shows, from data in the SAME shape the matching `RunManager` getter
//! emits (`emit::run_manager`). Each setter here is the inverse of a getter,
//! so `setter(getter(run))` on a real run reproduces its action list — the
//! contract the tests below pin.
//!
//! Why inject rather than re-roll: a room's options depend on run history
//! the live snapshot does not carry (rarity pity, unknown-room odds, visited
//! events, grab bags, potion odds), so a re-rolled reward / shop / event is
//! a different one. The Instinct advisor (`sts2-pycli/python/
//! realgame_instinct.py`) grafts the run from the mod's envelope and then
//! injects the options the game shows through these.
//!
//! Every function is a reconstruction surface over the core `debug_*`
//! setters: no RNG read, no on-obtain hook. Never call one on a recorded or
//! parity-pinned run. Ids are accepted in every spelling the getters and the
//! mod use (`CARD.X` / `X` / `PascalCase`, `RELIC.X`, `EVENT.X`, C#
//! `Id.Entry` UPPER_SNAKE). Errors are plain messages; the `PyO3` host raises
//! them as `ValueError`.

use serde_json::Value;
use sts2_core::cards::{Card, CardId, CardType, EnchantmentId};
use sts2_core::events::EventId;
use sts2_core::potions::PotionKind;
use sts2_core::run_manager::RunManager;
use sts2_core::runstate::{RestSiteOption, RoomType};
use sts2_core::shop::{
    merchant_price_with_modifiers, ShopCardEntry, ShopInventory, ShopPotionEntry, ShopPotionRarity,
    ShopRelicEntry, ShopRelicRarity, SHOP_ENTRY_SOLD_OUT_PRICE,
};
use sts2_core::{GameVersion, RelicId};

/// A graft failure, raised as `ValueError` by the `PyO3` host.
pub type GraftResult<T> = Result<T, String>;

/// Lower-cased alphanumerics: `DOLL_ROOM`, `DollRoom` and `Doll Room` meet.
fn norm(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn strip_ns<'a>(raw: &'a str, ns: &str) -> &'a str {
    let raw = raw.trim();
    raw.strip_prefix(ns).unwrap_or(raw)
}

// ---------------------------------------------------------------------------
// Id resolvers
// ---------------------------------------------------------------------------

/// A card id in any getter / mod spelling (`CARD.STRIKE_IRONCLAD`,
/// `STRIKE_IRONCLAD`, `StrikeIronclad`), including the v0.111.0-only cards
/// the name tables omit.
#[must_use]
pub fn resolve_card_id(raw: &str, version: GameVersion) -> Option<CardId> {
    let s = strip_ns(raw, "CARD.");
    sts2_content::names::card_id_from_str(s).or_else(|| {
        let want = norm(s);
        CardId::all_for(version)
            .into_iter()
            .find(|c| norm(c.to_python_name()) == want)
    })
}

/// A relic id in any getter / mod spelling (`RELIC.SNECKO_EYE`,
/// `SNECKO_EYE`, `SneckoEye`), including the v0.111.0-only relics.
#[must_use]
pub fn resolve_relic_id(raw: &str, version: GameVersion) -> Option<RelicId> {
    let s = strip_ns(raw, "RELIC.");
    sts2_content::names::relic_id_from_str(s).or_else(|| {
        let want = norm(s);
        RelicId::all_for(version)
            .into_iter()
            .find(|r| norm(r.to_python_name()) == want)
    })
}

/// A potion id: `POTION.FIRE_POTION`, `FIRE_POTION` or `FirePotion`.
#[must_use]
pub fn resolve_potion(raw: &str) -> Option<PotionKind> {
    sts2_content::names::potion_kind_from_str(strip_ns(raw, "POTION."))
}

/// An enchantment id: `ENCHANTMENT.GOOPY`, `GOOPY`, `Goopy`, `Perfect Fit`.
#[must_use]
pub fn resolve_enchantment(raw: &str) -> Option<EnchantmentId> {
    EnchantmentId::from_wire_name(strip_ns(raw, "ENCHANTMENT."))
}

/// Loc-key segments the live game uses for an event whose `Id.Entry`
/// differs (the option `text_key`'s first segment), as
/// `sts2-pycli/python/realgame_ppo.py::_EVENT_KEY_ALIASES` maps them.
const EVENT_ALIASES: &[(&str, EventId)] = &[
    ("MR_STRUGGLES", EventId::DollRoom),
    ("PROCEED", EventId::TheArchitect),
];

/// An event id as the game spells it — C# `Id.Entry` (`TEA_MASTER`,
/// `NEOW`, `DOLL_ROOM`), the engine's `python_name` (`DollRoom`), with or
/// without an `EVENT.` prefix or a leading `THE_` — or one of the loc-key
/// aliases (`MR_STRUGGLES`, `PROCEED`). C#'s `Id.Entry` is the class name in
/// UPPER_SNAKE, and every v0.111.0 event class has an `EventId` of the same
/// name, so the alphanumeric comparison covers the whole roster.
#[must_use]
pub fn resolve_event_id(raw: &str) -> Option<EventId> {
    let s = strip_ns(raw, "EVENT.");
    let want = norm(s);
    if want.is_empty() {
        return None;
    }
    if let Some((_, id)) = EVENT_ALIASES.iter().find(|(k, _)| norm(k) == want) {
        return Some(*id);
    }
    let find = |w: &str| {
        EventId::all()
            .iter()
            .copied()
            .find(|id| norm(id.python_name()) == w)
    };
    find(&want)
        .or_else(|| want.strip_prefix("the").and_then(find))
        .or_else(|| find(&format!("the{want}")))
}

/// A room type in `current_room()["room_type"]` spelling (`monster`,
/// `rest_site`, …), case- and separator-insensitive (`RestSite`, `REST_SITE`).
#[must_use]
pub fn resolve_room_type(raw: &str) -> Option<RoomType> {
    Some(match norm(raw).as_str() {
        "unknown" => RoomType::Unknown,
        "monster" => RoomType::Monster,
        "elite" => RoomType::Elite,
        "boss" => RoomType::Boss,
        "shop" | "merchant" => RoomType::Shop,
        "restsite" | "rest" => RoomType::RestSite,
        "treasure" => RoomType::Treasure,
        "event" | "ancient" => RoomType::Event,
        _ => return None,
    })
}

fn resolve_card_type(raw: &str) -> Option<CardType> {
    Some(match norm(raw).as_str() {
        "attack" => CardType::Attack,
        "skill" => CardType::Skill,
        "power" => CardType::Power,
        "status" => CardType::Status,
        "curse" => CardType::Curse,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------

/// One card spec as the graft takes it: `{"id", "upgraded", "enchantment",
/// "enchantment_amount"}`, or a bare id string (`"BASH"`, `"BASH+1"`).
///
/// `upgraded` is a bool or an upgrade level (`> 0` = upgraded). `enchantment`
/// is a name, `null`, or the `(name, amount)` pair the `*_enchantments()`
/// getters emit; `enchantment_amount` defaults to that pair's amount, else 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardSpec {
    pub id: String,
    pub upgraded: bool,
    pub enchantment: Option<(String, i32)>,
}

fn int_of(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_bool().map(i64::from))
        .or_else(|| v.as_f64().map(|f| f as i64))
}

/// Parse a [`CardSpec`] out of a dict or a bare id string.
///
/// # Errors
/// When `v` is neither, or carries no `id`.
pub fn card_spec(v: &Value) -> GraftResult<CardSpec> {
    if let Some(s) = v.as_str() {
        let (id, upgraded) = match s.strip_suffix("+1") {
            Some(base) => (base, true),
            None => (s, false),
        };
        return Ok(CardSpec {
            id: id.to_owned(),
            upgraded,
            enchantment: None,
        });
    }
    let obj = v
        .as_object()
        .ok_or_else(|| format!("card must be a dict or an id string, got {v}"))?;
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("card {v} has no string \"id\""))?
        .to_owned();
    let upgraded = obj.get("upgraded").and_then(int_of).is_some_and(|n| n > 0);
    let amount_key = obj.get("enchantment_amount").and_then(int_of);
    let enchantment = match obj.get("enchantment") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some((name.clone(), amount_key.unwrap_or(1))),
        Some(Value::Array(pair)) => {
            let name = pair
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| format!("card {v}: enchantment pair has no name"))?;
            let amount = amount_key
                .or_else(|| pair.get(1).and_then(int_of))
                .unwrap_or(1);
            Some((name.to_owned(), amount))
        }
        Some(other) => return Err(format!("card {v}: bad enchantment {other}")),
    };
    let enchantment = enchantment.map(|(n, a)| (n, i32::try_from(a).unwrap_or(1)));
    Ok(CardSpec {
        id,
        upgraded,
        enchantment,
    })
}

/// Build `spec` for `version` the way every deck add does
/// ([`sts2_core::build_card_for_version`]), then stamp its enchantment the
/// way an interactive deck enchant does (the stacks, plus Tezcatara's
/// Ember's cost-0 `OnEnchant`; every other static effect is read off the
/// enchantment in combat).
///
/// # Errors
/// An unknown card or enchantment id, or a card with no registered spec.
pub fn build_card(spec: &CardSpec, version: GameVersion) -> GraftResult<Card> {
    let id = resolve_card_id(&spec.id, version)
        .ok_or_else(|| format!("unknown card id {:?}", spec.id))?;
    let mut card = sts2_core::build_card_for_version(id, spec.upgraded, version)
        .ok_or_else(|| format!("no card spec for {:?}", spec.id))?;
    if let Some((name, amount)) = &spec.enchantment {
        let e = resolve_enchantment(name).ok_or_else(|| format!("unknown enchantment {name:?}"))?;
        card.set_enchantment(e, *amount);
        if e == EnchantmentId::TezcatarasEmber {
            // C# `TezcatarasEmber.OnEnchant`: `UpgradeBy(-GetWithModifiers(None))`.
            card.cost.upgrade_by(-i32::from(card.base_cost()));
        }
    }
    Ok(card)
}

fn list<'a>(v: &'a Value, what: &str) -> GraftResult<&'a Vec<Value>> {
    v.as_array()
        .ok_or_else(|| format!("{what} must be a list, got {v}"))
}

/// Build every card of a card list, failing on the first bad one.
fn build_cards(v: &Value, version: GameVersion, what: &str) -> GraftResult<Vec<Card>> {
    list(v, what)?
        .iter()
        .map(|c| build_card(&card_spec(c)?, version))
        .collect()
}

/// `debug_set_master_deck`: replace the master deck with `cards` (card
/// specs, in deck order). Cards that cannot be built are left out and their
/// ids returned; nothing else changes and no hook fires.
///
/// # Errors
/// When `cards` is not a list of card specs.
pub fn set_master_deck(rm: &mut RunManager, cards: &Value) -> GraftResult<Vec<String>> {
    let version = rm.run_state.game_version;
    let mut deck = Vec::new();
    let mut missing = Vec::new();
    for c in list(cards, "cards")? {
        let spec = card_spec(c)?;
        match build_card(&spec, version) {
            Ok(mut card) => {
                stamp_mad_science(&mut card, c);
                deck.push(card);
            }
            Err(_) => missing.push(spec.id),
        }
    }
    rm.debug_set_master_deck(deck);
    Ok(missing)
}

/// Mad Science's type is per card (C# `MadScience.cs` `Type =>
/// TinkerTimeType`), set by Tinker Time with its rider. A spec naming them
/// (`"type"`, `"tinker_time_rider"`) types the built card; without them it
/// stays the default-constructed Attack. A deck screen filters by that type:
/// Momentum offers Attacks, Swift a Power, Royally Approved Attacks or Skills.
fn stamp_mad_science(card: &mut Card, spec: &Value) {
    use sts2_content::cards::colorless::{set_mad_science_variant, TinkerTimeRider};
    if card.card_id != CardId::MadScience {
        return;
    }
    let Some(mode) = spec
        .get("type")
        .and_then(Value::as_str)
        .and_then(resolve_card_type)
        .filter(|t| matches!(t, CardType::Attack | CardType::Skill | CardType::Power))
    else {
        return;
    };
    let rider = spec
        .get("tinker_time_rider")
        .and_then(Value::as_str)
        .and_then(TinkerTimeRider::from_wire_name)
        .unwrap_or(TinkerTimeRider::None);
    set_mad_science_variant(card, mode, rider);
}

/// `debug_set_room_type`.
///
/// # Errors
/// An unknown room type.
pub fn set_room_type(rm: &mut RunManager, room_type: &str) -> GraftResult<()> {
    let rt = resolve_room_type(room_type).ok_or_else(|| {
        format!(
            "unknown room type {room_type:?} (monster / elite / boss / event / shop / \
             rest_site / treasure / unknown)"
        )
    })?;
    rm.debug_set_room_type(rt);
    Ok(())
}

/// Name the fight the run stands in (GH #1477, #1197): the game's
/// `EncounterModel.Id.Entry`, which is the sim's encounter id. A table
/// encounter sets `RunState::current_encounter_idx`; an event fight stages its
/// [`EventCombatRequest`] on `RunState::event_combat` as the event does.
/// Either way [`crate::emit::run_manager::combat_encounter_id`] then reads
/// `id` back once the run is in combat — the e2e net's `X_ID_AUX` input.
///
/// # Errors
/// An id that is neither a sim encounter nor one of the sim's event fights
/// in the run's game build.
pub fn set_combat_encounter(rm: &mut RunManager, id: &str) -> GraftResult<()> {
    use sts2_core::events::{BattlewornDummySetting as S, EventCombatRequest, EventEncounter};
    let id = id.trim().trim_start_matches("ENCOUNTER.");
    if let Some(def) = sts2_content::encounters::find(id) {
        rm.run_state.current_encounter_idx = Some(sts2_content::encounters::encounter_index(def));
        rm.run_state.event_combat = None;
        return Ok(());
    }
    let version = rm.run_state.game_version;
    let events = [
        EventEncounter::BattlewornDummy(S::Setting1),
        EventEncounter::BattlewornDummy(S::Setting2),
        EventEncounter::BattlewornDummy(S::Setting3),
        EventEncounter::MysteriousKnight,
        EventEncounter::FakeMerchant,
        EventEncounter::DenseVegetation,
        EventEncounter::PunchOff,
    ];
    let encounter = events
        .into_iter()
        .find(|&e| crate::emit::names::event_encounter_id_for(e, version) == id)
        .ok_or_else(|| format!("unknown encounter {id:?} for game build {version:?}"))?;
    // Only the fight is rebuilt, not the event behind it: the request names
    // the event's encounter and its parent, with no victory rewards.
    rm.run_state.event_combat = Some(match encounter {
        EventEncounter::BattlewornDummy(_) => {
            EventCombatRequest::resuming(encounter, EventId::BattlewornDummy)
        }
        EventEncounter::MysteriousKnight => {
            EventCombatRequest::no_resume(encounter, EventId::TheLanternKey, Vec::new())
        }
        EventEncounter::FakeMerchant => {
            EventCombatRequest::no_resume(encounter, EventId::FakeMerchant, Vec::new())
        }
        EventEncounter::DenseVegetation => {
            EventCombatRequest::no_resume(encounter, EventId::DenseVegetation, Vec::new())
        }
        EventEncounter::PunchOff => {
            EventCombatRequest::no_resume(encounter, EventId::PunchOff, Vec::new())
        }
    });
    Ok(())
}

/// `debug_open_card_offer`: open `Phase::CardReward` with `cards` (card
/// specs). With `bundles` (a list of packs, each a list of card specs or ids
/// — `card_reward_bundles()`'s shape) the offer is a pack pick and `cards`
/// may be empty.
///
/// # Errors
/// A bad card spec, or an empty offer.
pub fn open_card_offer(
    rm: &mut RunManager,
    cards: &Value,
    can_skip: bool,
    choose_screen: bool,
    bundles: Option<&Value>,
) -> GraftResult<()> {
    let version = rm.run_state.game_version;
    if let Some(b) = bundles.filter(|b| !b.is_null()) {
        let packs = list(b, "bundles")?
            .iter()
            .map(|pack| {
                list(pack, "bundle")?
                    .iter()
                    .map(|c| {
                        let spec = card_spec(c)?;
                        resolve_card_id(&spec.id, version)
                            .ok_or_else(|| format!("unknown card id {:?}", spec.id))
                    })
                    .collect::<GraftResult<Vec<_>>>()
            })
            .collect::<GraftResult<Vec<_>>>()?;
        if packs.is_empty() || packs.iter().any(Vec::is_empty) {
            return Err("bundles must hold at least one non-empty pack".to_owned());
        }
        rm.debug_open_card_offer(&[], true, false, Some(packs));
        return Ok(());
    }
    let built = build_cards(cards, version, "cards")?;
    if built.is_empty() {
        return Err("a card offer needs at least one card".to_owned());
    }
    rm.debug_open_card_offer(&built, can_skip, choose_screen, None);
    Ok(())
}

/// `debug_enter_event`: bind `event_id` fresh (page 0) in an Event room.
/// Returns the bound event's `python_name`.
///
/// # Errors
/// An event id that resolves to no [`EventId`].
pub fn enter_event(rm: &mut RunManager, event_id: &str) -> GraftResult<String> {
    let id = resolve_event_id(event_id).ok_or_else(|| format!("unknown event id {event_id:?}"))?;
    rm.debug_enter_event(
        id,
        sts2_content::events::dispatch::begin_event_in_entered_room,
    );
    Ok(id.python_name().to_owned())
}

/// `debug_enter_rest_site`: stand at a rest site with `taken` option ids
/// (`"HEAL"`, `"SMITH"`, … — `rest_site_options()`'s ids) already used this
/// visit. Returns the offered option ids in `RestOption` index order.
///
/// # Errors
/// A taken id the site does not offer.
pub fn enter_rest_site(rm: &mut RunManager, taken: &[String]) -> GraftResult<Vec<String>> {
    // Match the ids against the full offer, so a taken option keeps the
    // state (`Smith { enabled }`, `Lift { lifts_done }`) the site holds.
    let full = rm.debug_enter_rest_site(Vec::new());
    let mut pool: Vec<RestSiteOption> = full;
    let mut taken_opts = Vec::new();
    for t in taken {
        let want = norm(t);
        let pos = pool
            .iter()
            .position(|o| norm(o.option_id()) == want)
            .ok_or_else(|| format!("rest site offers no {t:?} option to have taken"))?;
        taken_opts.push(pool.remove(pos));
    }
    let offered = rm.debug_enter_rest_site(taken_opts);
    Ok(offered.iter().map(|o| o.option_id().to_owned()).collect())
}

/// `debug_queue_deck_enchant`.
///
/// # Errors
/// An unknown enchantment or card type.
pub fn queue_deck_enchant(
    rm: &mut RunManager,
    enchantment: &str,
    amount: i32,
    optional: bool,
    count: u32,
    card_type: Option<&str>,
) -> GraftResult<()> {
    queue_deck_enchant_of_types(rm, enchantment, amount, optional, count, card_type, None)
}

/// [`queue_deck_enchant`] restricted to one card type or either of two:
/// Royally Approved (Royal Stamp) enchants an Attack or a Skill
/// (`RoyallyApproved.CanEnchantCardType`).
///
/// # Errors
/// An unknown enchantment or card type.
pub fn queue_deck_enchant_of_types(
    rm: &mut RunManager,
    enchantment: &str,
    amount: i32,
    optional: bool,
    count: u32,
    card_type: Option<&str>,
    second_type: Option<&str>,
) -> GraftResult<()> {
    let e = resolve_enchantment(enchantment)
        .ok_or_else(|| format!("unknown enchantment {enchantment:?}"))?;
    let resolve = |t: Option<&str>| {
        t.map(|t| resolve_card_type(t).ok_or_else(|| format!("unknown card type {t:?}")))
            .transpose()
    };
    let ty = resolve(card_type)?;
    let second = resolve(second_type)?;
    let from = rm.run_state.pending_deck_enchants.len();
    rm.debug_queue_deck_enchants(count, e, amount, optional, ty);
    for p in &mut rm.run_state.pending_deck_enchants[from..] {
        p.secondary_type = second;
    }
    Ok(())
}

/// `debug_queue_card_grant`.
///
/// # Errors
/// A bad card spec, an empty grant, or `pick` of zero.
pub fn queue_card_grant(
    rm: &mut RunManager,
    cards: &Value,
    pick: usize,
    min_pick: usize,
) -> GraftResult<()> {
    let built = build_cards(cards, rm.run_state.game_version, "cards")?;
    if built.is_empty() || pick == 0 {
        return Err("a card grant needs at least one card and pick >= 1".to_owned());
    }
    rm.debug_queue_card_grant(built, pick, min_pick);
    Ok(())
}

/// `debug_queue_potion_swap`.
///
/// # Errors
/// An unknown potion id.
pub fn queue_potion_swap(rm: &mut RunManager, potion: &str) -> GraftResult<()> {
    let kind = resolve_potion(potion).ok_or_else(|| format!("unknown potion id {potion:?}"))?;
    rm.debug_queue_potion_swap(kind);
    Ok(())
}

/// `debug_open_boss_relic`.
///
/// # Errors
/// An unknown relic id, or no relic at all.
pub fn open_boss_relic(rm: &mut RunManager, relics: &[String]) -> GraftResult<()> {
    let version = rm.run_state.game_version;
    let ids = relics
        .iter()
        .map(|r| resolve_relic_id(r, version).ok_or_else(|| format!("unknown relic id {r:?}")))
        .collect::<GraftResult<Vec<_>>>()?;
    if ids.is_empty() {
        return Err("a boss-relic offer needs at least one relic".to_owned());
    }
    rm.debug_open_boss_relic(ids);
    Ok(())
}

// ---------------------------------------------------------------------------
// Shop
// ---------------------------------------------------------------------------

/// The stored base price whose displayed (relic-discounted,
/// [`merchant_price_with_modifiers`]) price is `displayed`: the smallest base
/// the forward computation maps onto it, `None` when no base does. The
/// sold-out sentinel maps to itself.
#[must_use]
pub fn invert_merchant_price(relics: &[RelicId], displayed: i32) -> Option<i32> {
    if displayed <= 0 || displayed >= SHOP_ENTRY_SOLD_OUT_PRICE {
        return Some(displayed);
    }
    // The strongest discount is Courier × Membership = 0.4, so the base is at
    // most 2.5× the displayed price (plus the floor's slack).
    let hi = displayed.saturating_mul(3).saturating_add(3);
    (displayed..=hi).find(|&b| merchant_price_with_modifiers(relics, b) == displayed)
}

struct Priced {
    base: i32,
    matched: bool,
}

/// The stored price of one inventory row: its `base_price` when given, else
/// its displayed `price` inverted; a sold-out row (`available == false` with
/// a sentinel price) stays sold out.
fn row_price(row: &serde_json::Map<String, Value>, relics: &[RelicId]) -> GraftResult<Priced> {
    if let Some(base) = row.get("base_price").and_then(int_of) {
        return Ok(Priced {
            base: i32::try_from(base).unwrap_or(SHOP_ENTRY_SOLD_OUT_PRICE),
            matched: true,
        });
    }
    let displayed = row
        .get("price")
        .and_then(int_of)
        .ok_or_else(|| format!("shop row {row:?} has no price"))?;
    let displayed = i32::try_from(displayed).unwrap_or(SHOP_ENTRY_SOLD_OUT_PRICE);
    let sold_out = row.get("available").and_then(Value::as_bool) == Some(false)
        && displayed >= SHOP_ENTRY_SOLD_OUT_PRICE;
    if sold_out {
        return Ok(Priced {
            base: SHOP_ENTRY_SOLD_OUT_PRICE,
            matched: true,
        });
    }
    Ok(match invert_merchant_price(relics, displayed) {
        Some(base) => Priced {
            base,
            matched: true,
        },
        None => Priced {
            base: displayed,
            matched: false,
        },
    })
}

fn card_rarity(id: CardId, version: GameVersion) -> sts2_core::card_rewards::RewardRarity {
    use sts2_core::card_rewards::RewardRarity;
    match sts2_core::lookup_card_rarity_rank().map(|f| f(id, version)) {
        Some(3) => RewardRarity::Uncommon,
        Some(4) => RewardRarity::Rare,
        _ => RewardRarity::Common,
    }
}

/// `debug_open_shop`: open `Phase::Shop` with `inventory` in exactly the
/// shape `shop_inventory()` emits — `{"cards": [{"id", "upgraded"?,
/// "price", "on_sale", "is_colorless", "available"}], "relics": [{"id",
/// "price", "available"}], "potions": [...], "removal_cost",
/// "removal_used"}`. Prices are the DISPLAYED ones (after Courier /
/// Membership Card); each is inverted to the stored base price the forward
/// computation reproduces, unless the row carries an explicit
/// `"base_price"`. Returns the rows whose displayed price no base maps onto
/// (`"cards[2]"`, `"removal_cost"`, …) — those keep the displayed value.
///
/// `"fake_merchant": true` opens the Fake Merchant's shop instead (GH
/// #1607): its displayed prices are the rolled ones (no merchant-price
/// relic folds in an event room), it has no removal, and the run stands in
/// its bound event, so leaving ends the event.
///
/// Rarity buckets are not on the wire: cards take their spec rarity, relic
/// slot 2 is the Shop slot, potions read Common. Only The Courier's restock
/// reads them, and a restock re-rolls off cursors the snapshot moved anyway.
///
/// # Errors
/// A malformed inventory or an unknown id.
pub fn open_shop(rm: &mut RunManager, inventory: &Value) -> GraftResult<Vec<String>> {
    let version = rm.run_state.game_version;
    let inv_obj = inventory
        .as_object()
        .ok_or_else(|| format!("inventory must be a dict, got {inventory}"))?;
    let fake_merchant = inv_obj
        .get("fake_merchant")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // No relic discounts the Fake Merchant: invert against none.
    let relics_owned: Vec<RelicId> = if fake_merchant {
        Vec::new()
    } else {
        rm.run_state.active_relics().into_owned()
    };
    let rows = |key: &str| -> GraftResult<Vec<serde_json::Map<String, Value>>> {
        match inv_obj.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(v) => list(v, key)?
                .iter()
                .map(|r| {
                    r.as_object()
                        .cloned()
                        .ok_or_else(|| format!("{key} row must be a dict, got {r}"))
                })
                .collect(),
        }
    };
    let mut unmatched = Vec::new();
    let mut inv = ShopInventory::empty();

    for (i, row) in rows("cards")?.iter().enumerate() {
        let priced = row_price(row, &relics_owned)?;
        if !priced.matched {
            unmatched.push(format!("cards[{i}]"));
        }
        let card = match row.get("id").filter(|v| !v.is_null()) {
            Some(_) => Some(build_card(
                &card_spec(&Value::Object(row.clone()))?,
                version,
            )?),
            None => None,
        };
        let is_colorless = row
            .get("is_colorless")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let entry = ShopCardEntry {
            rarity: card
                .as_ref()
                .map_or(sts2_core::card_rewards::RewardRarity::Common, |c| {
                    card_rarity(c.card_id, version)
                }),
            card_type: if is_colorless {
                CardType::Status
            } else {
                card.as_ref().map_or(CardType::Attack, |c| c.card_type)
            },
            card,
            is_colorless,
            on_sale: row.get("on_sale").and_then(Value::as_bool).unwrap_or(false),
            price: priced.base,
        };
        if is_colorless {
            inv.colorless_cards.push(entry);
        } else {
            inv.cards.push(entry);
        }
    }

    for (i, row) in rows("relics")?.iter().enumerate() {
        let priced = row_price(row, &relics_owned)?;
        if !priced.matched {
            unmatched.push(format!("relics[{i}]"));
        }
        let relic_id = match row.get("id").and_then(Value::as_str) {
            Some(r) => Some(
                resolve_relic_id(r, version).ok_or_else(|| format!("unknown relic id {r:?}"))?,
            ),
            None => None,
        };
        inv.relics.push(ShopRelicEntry {
            relic_rarity: if i == 2 {
                ShopRelicRarity::Shop
            } else {
                ShopRelicRarity::Common
            },
            relic_id,
            price: priced.base,
        });
    }

    for (i, row) in rows("potions")?.iter().enumerate() {
        let priced = row_price(row, &relics_owned)?;
        if !priced.matched {
            unmatched.push(format!("potions[{i}]"));
        }
        let potion_kind = match row.get("id").and_then(Value::as_str) {
            Some(p) => Some(resolve_potion(p).ok_or_else(|| format!("unknown potion id {p:?}"))?),
            None => None,
        };
        inv.potions.push(ShopPotionEntry {
            potion_rarity: ShopPotionRarity::Common,
            potion_kind,
            price: priced.base,
        });
    }

    if let Some(cost) = inv_obj.get("removal_cost").and_then(int_of) {
        let displayed = i32::try_from(cost).unwrap_or(SHOP_ENTRY_SOLD_OUT_PRICE);
        if let Some(base) = inv_obj.get("removal_base_cost").and_then(int_of) {
            inv.removal_cost = i32::try_from(base).unwrap_or(displayed);
        } else if let Some(base) = invert_merchant_price(&relics_owned, displayed) {
            inv.removal_cost = base;
        } else {
            inv.removal_cost = displayed;
            unmatched.push("removal_cost".to_owned());
        }
    }
    inv.removal_used = inv_obj
        .get("removal_used")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if fake_merchant {
        inv.fake_merchant = true;
        inv.removal_used = true;
        inv.removal_cost = 0;
        rm.run_state.current_event = Some(sts2_core::events::EventId::FakeMerchant);
        rm.debug_set_room_type(sts2_core::runstate::RoomType::Event);
    }
    rm.debug_open_shop(inv);
    Ok(unmatched)
}

/// `force_add_relic`: add `relic_id` (`RELIC.` prefix tolerated) and fire
/// its on-obtain hooks, as picking it up would. `false` when the run
/// already holds it.
///
/// # Errors
/// An unknown relic id.
pub fn force_add_relic(rm: &mut RunManager, relic_id: &str) -> GraftResult<bool> {
    let stripped = relic_id.strip_prefix("RELIC.").unwrap_or(relic_id);
    let rid = resolve_relic_id(stripped, rm.run_state.game_version)
        .ok_or_else(|| format!("unknown relic id {relic_id:?}"))?;
    let rs = &mut rm.run_state;
    if rs.relics.contains(&rid) {
        return Ok(false);
    }
    rs.relics.push(rid);
    rs.last_obtained_relic = Some(rid);
    let _ = sts2_core::run_effects::dispatch_after_obtained(rs);
    rs.last_obtained_relic = None;
    Ok(true)
}

/// `debug_set_potion_state`: the belt and the potions waiting on a full
/// belt, replaced whole (Python potion names).
///
/// # Errors
/// An unknown potion; the run is then partly set.
pub fn set_potion_state(
    rm: &mut RunManager,
    potions: &[String],
    pending: &[String],
) -> GraftResult<()> {
    let parse = |name: &str| {
        crate::state_dict::potion_kind_from_python_name(name)
            .ok_or_else(|| format!("unknown potion {name:?}"))
    };
    let rs = &mut rm.run_state;
    rs.potions.clear();
    for name in potions {
        rs.potions.push(parse(name)?);
    }
    rs.pending_potion_swaps.clear();
    for name in pending {
        rs.pending_potion_swaps.push(parse(name)?);
    }
    Ok(())
}

/// `debug_set_rng_counters`: fast-forward each named run RNG stream to its
/// cursor (`counters`, in order). Returns the names this engine has no
/// stream for, sorted.
///
/// # Errors
/// No counters (a seed without cursors would rebuild every stream at count
/// 0), `run_seed` not this run's seed, or a cursor behind its stream.
pub fn set_rng_counters(
    rm: &mut RunManager,
    run_seed: u64,
    counters: &[(String, u32)],
) -> GraftResult<Vec<String>> {
    if counters.is_empty() {
        return Err("debug_set_rng_counters: empty counters — a seed without \
             cursors rebuilds every stream at count 0, which destroys \
             more lineage than it restores. Pass the whole block or none."
            .into());
    }
    // `RunRngSet.Seed`: ≤ u32::MAX in v0.107.1, a full u64 in v0.111.0
    // (GH #788). The caller passes the unsigned view (a negative 32-bit
    // seed is normalised by `state_import._rng_block`).
    let ours = rm.run_state.rng.seed;
    if run_seed != ours {
        return Err(format!(
            "debug_set_rng_counters: run_seed {run_seed} is not this run's \
             seed ({ours}) — these cursors belong to a different run"
        ));
    }
    let mut ignored: Vec<String> = Vec::new();
    for (name, target) in counters {
        let Some(stream) = rm.run_state.rng.stream_mut(name) else {
            ignored.push(name.clone());
            continue;
        };
        let current = stream.counter();
        if *target < current {
            return Err(format!(
                "debug_set_rng_counters: stream {name:?} is already at \
                 {current}, cannot rewind to {target}"
            ));
        }
        stream.fast_forward_counter(*target);
    }
    ignored.sort();
    Ok(ignored)
}

/// `debug_set_neow_offers`: stand on the screen's three Neow offers (two
/// positive, then the curse), as snake-case relic ids (`RELIC.` tolerated).
///
/// # Errors
/// A name that is not a Neow relic.
pub fn set_neow_offers(
    rm: &mut RunManager,
    positive_1: &str,
    positive_2: &str,
    cursed: &str,
) -> GraftResult<()> {
    let parse = |name: &str| {
        let id = name.strip_prefix("RELIC.").unwrap_or(name);
        sts2_core::NeowRelic::from_snake_case_id(id)
            .ok_or_else(|| format!("{name:?} is not a Neow relic"))
    };
    rm.run_state.neow_offers = Some(sts2_core::NeowOffers {
        positive_1: parse(positive_1)?,
        positive_2: parse(positive_2)?,
        cursed: parse(cursed)?,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_ids_resolve_from_every_spelling() {
        for (raw, want) in [
            ("TEA_MASTER", EventId::TeaMaster),
            ("NEOW", EventId::Neow),
            ("DOLL_ROOM", EventId::DollRoom),
            ("MR_STRUGGLES", EventId::DollRoom),
            ("PROCEED", EventId::TheArchitect),
            ("THE_ARCHITECT", EventId::TheArchitect),
            ("EVENT.LANTERN_KEY", EventId::TheLanternKey),
            ("DollRoom", EventId::DollRoom),
            ("Tablet of Truth", EventId::TabletOfTruth),
        ] {
            assert_eq!(resolve_event_id(raw), Some(want), "{raw}");
        }
        assert_eq!(resolve_event_id("NOT_AN_EVENT"), None);
        assert_eq!(resolve_event_id(""), None);
    }

    #[test]
    fn every_event_resolves_from_its_csharp_id_entry() {
        // C# `Id.Entry` is the class name in UPPER_SNAKE.
        for &id in EventId::all() {
            let mut entry = String::new();
            for (i, ch) in id.python_name().chars().enumerate() {
                if ch.is_ascii_uppercase() && i > 0 {
                    entry.push('_');
                }
                entry.push(ch.to_ascii_uppercase());
            }
            assert_eq!(resolve_event_id(&entry), Some(id), "{entry}");
        }
    }

    #[test]
    fn price_inversion_reproduces_the_displayed_price() {
        let both = [RelicId::TheCourier, RelicId::MembershipCard];
        for relics in [
            &[][..],
            &[RelicId::TheCourier][..],
            &[RelicId::MembershipCard][..],
            &both[..],
        ] {
            for displayed in 1..400 {
                let base = invert_merchant_price(relics, displayed).expect("reachable");
                assert_eq!(merchant_price_with_modifiers(relics, base), displayed);
            }
        }
        assert_eq!(
            invert_merchant_price(&both, SHOP_ENTRY_SOLD_OUT_PRICE),
            Some(SHOP_ENTRY_SOLD_OUT_PRICE)
        );
    }

    #[test]
    fn a_deck_mad_science_takes_the_specs_type() {
        sts2_content::cards::dispatch::ensure_installed();
        let build = |v: Value| {
            let mut card = build_card(&card_spec(&v).unwrap(), GameVersion::V0_111_0).unwrap();
            stamp_mad_science(&mut card, &v);
            card.card_type
        };
        let mad = |t: &str| serde_json::json!({"id": "MAD_SCIENCE", "type": t, "tinker_time_rider": "Curious"});
        assert_eq!(build(mad("Power")), CardType::Power);
        assert_eq!(build(mad("Skill")), CardType::Skill);
        assert_eq!(
            build(serde_json::json!({"id": "MAD_SCIENCE"})),
            CardType::Attack
        );
        // Only Mad Science's type is per card.
        assert_eq!(
            build(serde_json::json!({"id": "BASH", "type": "Power"})),
            CardType::Attack
        );
    }

    #[test]
    fn card_spec_accepts_the_getter_shapes() {
        let v: Value = serde_json::json!({
            "id": "CARD.BASH", "upgraded": 1, "enchantment": ["GOOPY", 2]
        });
        assert_eq!(
            card_spec(&v).unwrap(),
            CardSpec {
                id: "CARD.BASH".into(),
                upgraded: true,
                enchantment: Some(("GOOPY".into(), 2)),
            }
        );
        let s = card_spec(&serde_json::json!("BASH+1")).unwrap();
        assert!(s.upgraded && s.id == "BASH" && s.enchantment.is_none());
    }
}
