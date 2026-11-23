//! Blitz mode content (GH #1347): the card, potion, relic and curse rolls
//! of C# `BlitzRunModel` (`Models.Blitz/BlitzRunModel.cs`, v0.113.0) and the
//! Blitz merchant's twenty wares (`Entities.Merchant/BlitzMerchant*Entry.cs`).
//! The run loop's side lives in [`sts2_core::blitz`] and
//! `sts2_core::run_manager`; this module is wired in through
//! [`sts2_core::run_manager::ContentCallbacks`].

use sts2_core::blitz::{
    DeckPick, MerchantEntry, MerchantEntryType, MerchantInventory, SelectOutcome,
};
use sts2_core::cards::{Card, CardId, CardType, EnchantmentId};
use sts2_core::dotnet_rng::Rng;
use sts2_core::runstate::{RoomType, RunState};
use sts2_core::CharacterId;

use crate::cards::spec_index::{eligible_ids_for_rarity_in, RunCardFilter};
use crate::cards::CardRarity;

/// C# `BlitzRunModel.shopEnchantmentOptions` (`BlitzRunModel.cs:129-147`),
/// in list order: the enchantment and the amount it is applied with.
pub const SHOP_ENCHANTMENT_OPTIONS: [(EnchantmentId, i32); 6] = [
    (EnchantmentId::Sharp, 2),
    (EnchantmentId::Nimble, 3),
    (EnchantmentId::Adroit, 3),
    (EnchantmentId::Steady, 1),
    (EnchantmentId::Swift, 2),
    (EnchantmentId::Vigorous, 5),
];

/// The rarity table a card roll uses (C# `CardRarityOddsType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Odds {
    /// `Uniform`: no rarity roll; any non-Basic, non-Ancient card of the pool.
    Uniform,
    /// `Blitz` (`CardRarityOdds.cs:185-191`): Common 0.4, Uncommon 0.5,
    /// Rare 0.1.
    Blitz,
    /// `BlitzNeow` (`:192-198`): Common 0.55, Uncommon 0.4, Rare 0.05.
    BlitzNeow,
}

impl Odds {
    /// `(rare, uncommon)` of C# `GetBaseOdds`, as the `float`s it returns.
    const fn base(self) -> (f32, f32) {
        match self {
            Odds::Uniform => (0.33, 0.33),
            Odds::Blitz => (0.1, 0.5),
            Odds::BlitzNeow => (0.05, 0.4),
        }
    }
}

/// C# `CardRarityOdds.RollWithBaseOdds(type, rng)` (`CardRarityOdds.cs:
/// 136-151`): one `NextFloat`, Rare below the rare odds, Uncommon below
/// `uncommon + rare` (a `float` sum), else Common. No pity is read or moved.
fn roll_with_base_odds(rng: &mut Rng, odds: Odds) -> CardRarity {
    let (rare, uncommon) = odds.base();
    let roll = rng.next_float_f32();
    if roll < rare {
        CardRarity::Rare
    } else if roll < uncommon + rare {
        CardRarity::Uncommon
    } else {
        CardRarity::Common
    }
}

/// C# `CardRarityExtensions.GetNextHighestRarityWithWrapping` over the three
/// reward rarities (`CardRarityExtensions.cs:14-31`).
const fn next_rarity_wrapping(r: CardRarity) -> CardRarity {
    match r {
        CardRarity::Common => CardRarity::Uncommon,
        CardRarity::Uncommon => CardRarity::Rare,
        _ => CardRarity::Common,
    }
}

/// The card pool a roll draws from (C# `CardCreationOptions.CardPools`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pool {
    /// A character's `CardPool`.
    Character(CharacterId),
    /// `ColorlessCardPool`.
    Colorless,
}

/// The `Rng` a roll reads (C# `options.RngOverride ?? PlayerRng.Rewards`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// `PlayerRng.Rewards`.
    Rewards,
    /// `PlayerRng.Shops` (`WithRngOverride(_player.PlayerRng.Shops)`).
    Shops,
}

/// One `CardFactory.CreateForReward(player, count, options)` call, as far as
/// its draws go (`CardFactory.cs:88-107,209-232`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardRoll {
    /// `options.CardPools`.
    pub pool: Pool,
    /// `options.RarityOdds`.
    pub odds: Odds,
    /// `options.CardPoolFilter`, when it is `c => c.Rarity == r`.
    pub rarity: Option<CardRarity>,
    /// `!Flags.HasFlag(NoUpgradeRoll)`: one `NextFloat` per card after its
    /// pick (`RollForUpgrade`, `CardFactory.cs:292-309`, base chance 0).
    pub upgrade_roll: bool,
    /// The stream every draw of the roll reads.
    pub stream: Stream,
}

fn stream_of(rs: &mut RunState, stream: Stream) -> &mut Rng {
    match stream {
        Stream::Rewards => &mut rs.rng.rewards,
        Stream::Shops => &mut rs.rng.shops,
    }
}

/// The pool's cards of `rarity` in C# pool order, through
/// `CardFactory.FilterForRun` ([`RunCardFilter`]: no multiplayer-only card,
/// and in Blitz none of the four `IsAllowedInBlitz == false` cards).
fn pool_ids(rs: &RunState, pool: Pool, rarity: CardRarity) -> Vec<CardId> {
    let run = RunCardFilter::of(rs);
    match pool {
        Pool::Character(c) => eligible_ids_for_rarity_in(&[c], false, rarity, run),
        Pool::Colorless => eligible_ids_for_rarity_in(&[], true, rarity, run),
    }
}

/// C# `CardFactory.CreateForReward(player, count, options)` without its
/// modify hooks: `count` distinct cards, each rolled as
///
/// 1. (not for `Uniform`) the rarity: `RollWithBaseOdds` on the roll's
///    stream, then `GetNextAllowedRarity` over the rarities still present in
///    the filtered, blacklisted pool, wrapping Rare to Common;
/// 2. the card: `rng.NextItem` over the pool's cards of that rarity (for
///    `Uniform`, of every reward rarity the filter lets through), minus the
///    cards already rolled;
/// 3. (with [`CardRoll::upgrade_roll`]) the upgrade: one `NextFloat` against
///    `act index * UpgradedCardOddScaling` (0 for a Rare; never for a card
///    that cannot upgrade, though the float is drawn first).
///
/// The returned cards still need the reward's modify hooks
/// ([`RunState::offer_card_slate`]) where C# runs them.
pub fn create_for_reward(rs: &mut RunState, roll: CardRoll, count: usize) -> Vec<Card> {
    use sts2_core::card_rewards::{
        combat_card_upgrade_base_chance, roll_for_upgrade_with_dotnet, RewardRarity,
    };
    const REWARD_RARITIES: [CardRarity; 3] =
        [CardRarity::Common, CardRarity::Uncommon, CardRarity::Rare];
    let version = rs.game_version;
    let act_index = rs.act_index.saturating_sub(1);
    let scarcity = rs.card_rarity_odds.ascension_level;
    let allowed = |r: CardRarity| roll.rarity.is_none_or(|only| only == r);
    let mut chosen: Vec<CardId> = Vec::new();
    let mut out: Vec<Card> = Vec::with_capacity(count);
    for _ in 0..count {
        let candidates_of = |rs: &RunState, r: CardRarity, chosen: &[CardId]| -> Vec<CardId> {
            if !allowed(r) {
                return Vec::new();
            }
            let mut ids = pool_ids(rs, roll.pool, r);
            ids.retain(|id| !chosen.contains(id));
            ids
        };
        let (candidates, rarity) = if roll.odds == Odds::Uniform {
            // `items = options2.Where(Rarity != Basic && Rarity != Ancient)`:
            // pool order, which for one rarity is that rarity's list.
            // The Blitz uniform rolls all carry a rarity filter.
            let only = roll.rarity.unwrap_or(CardRarity::Common);
            debug_assert!(roll.rarity.is_some(), "uniform rolls carry a rarity filter");
            (candidates_of(rs, only, &chosen), only)
        } else {
            let mut rarity = roll_with_base_odds(stream_of(rs, roll.stream), roll.odds);
            let mut candidates = candidates_of(rs, rarity, &chosen);
            // `GetNextAllowedRarity`: walk the wrap until a rarity the pool
            // still has; a full cycle is `CardRarity.None` (C# throws).
            for _ in 0..REWARD_RARITIES.len() {
                if !candidates.is_empty() {
                    break;
                }
                rarity = next_rarity_wrapping(rarity);
                candidates = candidates_of(rs, rarity, &chosen);
            }
            (candidates, rarity)
        };
        if candidates.is_empty() {
            break;
        }
        let rng = stream_of(rs, roll.stream);
        let pick = candidates[rng.next_int_exclusive(0, candidates.len() as i32) as usize];
        chosen.push(pick);
        let upgrade_rarity = match rarity {
            CardRarity::Rare => RewardRarity::Rare,
            CardRarity::Uncommon => RewardRarity::Uncommon,
            _ => RewardRarity::Common,
        };
        let rolled_upgrade = roll.upgrade_roll
            && roll_for_upgrade_with_dotnet(
                rng,
                upgrade_rarity,
                act_index,
                scarcity,
                combat_card_upgrade_base_chance(),
            );
        if let Some(mut card) = crate::cards::v0111::card_pod_for(pick, false, version) {
            // `RollForUpgrade` upgrades only an upgradable card.
            if rolled_upgrade && card.is_upgradable() {
                card.upgrade_for(version);
            }
            out.push(card);
        }
    }
    out
}

/// Upgrade every upgradable card of `cards` (C# `CardCmd.Upgrade` per card,
/// a no-op on one that cannot upgrade).
fn upgrade_all(cards: &mut [Card], version: sts2_core::GameVersion) {
    for card in cards {
        if card.is_upgradable() {
            card.upgrade_for(version);
        }
    }
}

// ── Neow ─────────────────────────────────────────────────────────────

/// `BlitzRunModel.GiveNeowRewards`: ten cards on offer.
pub const NEOW_CARDS: usize = 10;
/// `new CardSelectorPrefs(prompt, 3)`: exactly three are taken.
pub const NEOW_PICKS: usize = 3;

/// C# `BlitzRunModel.GiveNeowRewards(player)` (`BlitzRunModel.cs:26-44`),
/// reached from Neow's single BLITZ option (`Neow.cs:218-223,295-307`):
/// `CardFactory.CreateForReward(player, 10, new CardCreationOptions(
/// [Character.CardPool], Other, BlitzNeow).WithFlags(NoUpgrades))` on
/// `PlayerRng.Rewards` (a rarity `NextFloat` and a pick `NextInt` per card,
/// no upgrade roll), ordered by rarity (`OrderBy`, stable), then a mandatory
/// pick of exactly three (`FromSimpleGridForRewards`), each added to the
/// deck. Queues the pick as a [`sts2_core::runstate::PendingCardGrant`].
pub fn give_neow_rewards(rs: &mut RunState) {
    let character = rs.character_id;
    let cards = create_for_reward(
        rs,
        CardRoll {
            pool: Pool::Character(character),
            odds: Odds::BlitzNeow,
            rarity: None,
            upgrade_roll: false,
            stream: Stream::Rewards,
        },
        NEOW_CARDS,
    );
    // `CreateForReward` runs `Hook.TryModifyCardRewardOptions` on the list
    // (no `NoModifyHooks`); it is not a `CardReward`.
    let mut cards = rs.offer_card_slate(cards, false);
    let version = rs.game_version;
    let rank = |c: &Card| match crate::cards::spec_index::rarity_for_card_id_in(c.card_id, version)
    {
        Some(CardRarity::Common) => 2u8,
        Some(CardRarity::Uncommon) => 3,
        Some(CardRarity::Rare) => 4,
        _ => 0,
    };
    cards.sort_by_key(rank);
    let pick = NEOW_PICKS.min(cards.len());
    if pick > 0 {
        rs.pending_card_grants
            .push(sts2_core::runstate::PendingCardGrant {
                options: cards,
                pick,
                min_pick: pick,
            });
    }
}

// ── Room rewards ─────────────────────────────────────────────────────

/// The card reward of a won Blitz room. C# `BlitzRunModel.
/// TryReplaceDefaultRoomRewards` (`BlitzRunModel.cs:90-118`) offers
/// `new CardReward(GetCardRewardOptions(player, rarity), 3, player)` with
/// Common for a Monster room, Uncommon for an Elite, Rare for a boss before
/// the last act; `GetCardRewardOptions` (`:124-127`) is
/// `ForNonCombatWithUniformOdds([Character.CardPool], c => c.Rarity ==
/// rarity).WithFlags(NoUpgradeRoll | IsFromCombat)`: three `Rewards`
/// `NextItem`s, no rarity roll, no upgrade roll, no pity.
///
/// Then the reward's modify hooks, and last `BlitzRunModel.
/// TryModifyCardRewardOptionsLate` (`:46-88`; run models come after the
/// relics in `RunState.IterateHookListeners`): outside a Boss room, in act 2
/// one card picked with `Rewards.NextItem` is upgraded, in act 3 the first
/// two of `cardRewards.UnstableShuffle(Rewards)` are (the shuffle reorders
/// the offered list itself). A card that cannot upgrade is left alone, the
/// draw still made.
pub fn roll_room_card_reward(rs: &mut RunState) -> Vec<Card> {
    let room = rs.current_room.map(|r| r.room_type);
    let rarity = match room {
        Some(RoomType::Elite) => CardRarity::Uncommon,
        Some(RoomType::Boss) => CardRarity::Rare,
        _ => CardRarity::Common,
    };
    let character = rs.character_id;
    let cards = create_for_reward(
        rs,
        CardRoll {
            pool: Pool::Character(character),
            odds: Odds::Uniform,
            rarity: Some(rarity),
            upgrade_roll: false,
            stream: Stream::Rewards,
        },
        3,
    );
    let mut cards = rs.offer_card_slate(cards, true);
    let act = rs.act_index.saturating_sub(1);
    if act == 0 || room == Some(RoomType::Boss) || cards.is_empty() {
        return cards;
    }
    let version = rs.game_version;
    if act == 1 {
        let idx = rs.rng.rewards.next_int_exclusive(0, cards.len() as i32) as usize;
        upgrade_all(&mut cards[idx..=idx], version);
    } else {
        rs.rng.rewards.shuffle_for(version, &mut cards);
        let n = cards.len().min(2);
        upgrade_all(&mut cards[..n], version);
    }
    cards
}

/// [`sts2_core::run_manager::ContentCallbacks::blitz_room_potion`]: the
/// `PotionReward(player)` of a Blitz room's replaced set, populated after the
/// card reward (`PotionReward.Populate` -> `CreateRandomPotionOutOfCombat`
/// on `PlayerRng.Rewards`) and claimed. A Monster room has one unless its
/// encounter `IsWeak`; a boss before the last act always (`BlitzRunModel.cs:
/// 96-99,115`); an Elite never.
pub fn room_potion(rs: &mut RunState, room_type: RoomType) -> bool {
    let grants = match room_type {
        RoomType::Monster => !last_encounter_is_weak(rs),
        RoomType::Boss => rs.act_index < 3,
        _ => false,
    };
    if !grants {
        return false;
    }
    let character = rs.character_id;
    let potion =
        crate::potions::all::create_random_potions_out_of_combat(&mut rs.rng.rewards, character, 1)
            .into_iter()
            .next();
    if let Some(kind) = potion {
        let _ = rs.claim_potion_reward(kind);
    }
    true
}

/// C# `combatRoom.Encounter.IsWeak` for the fight just won.
fn last_encounter_is_weak(rs: &RunState) -> bool {
    rs.current_encounter_idx
        .and_then(crate::encounters::by_index)
        .is_some_and(|enc| sts2_core::run_room_gen::encounter_is_weak(enc.id))
}

// ── Merchant ─────────────────────────────────────────────────────────

/// Whether `enchantment` can enchant deck card `card` (C#
/// `EnchantmentModel.CanEnchant`).
fn can_enchant(rs: &RunState, card: &Card, enchantment: EnchantmentId) -> bool {
    crate::cards::spec_index::spec_for_card_id(card.card_id).is_some_and(|spec| {
        crate::enchantments::can_enchant_card(&spec, card, enchantment, rs.game_version)
    })
}

/// C# `UnlockState.Characters` with everything unlocked: `ModelDb.
/// AllCharacters` order (`ModelDb.cs:146-154`).
const ALL_CHARACTERS: [CharacterId; 6] = [
    CharacterId::Ironclad,
    CharacterId::Silent,
    CharacterId::Regent,
    CharacterId::Necrobinder,
    CharacterId::Defect,
    CharacterId::Fool,
];

/// C# `BlitzMerchantEntry.Generate(player, type)`: the ware, with the
/// `Shops` draw two constructors make.
///
/// - `BlitzMerchantCardEnchantEntry` (`:54-60`): `Shops.NextItem` over the
///   [`SHOP_ENCHANTMENT_OPTIONS`] that can enchant at least one deck card;
///   an empty list draws nothing.
/// - `BlitzMerchantPrismaticCardRewardEntry` (`:41-49`): `Shops.NextItem`
///   over the unlocked characters other than the player's.
fn generate_entry(rs: &mut RunState, kind: MerchantEntryType) -> MerchantEntry {
    let mut entry = MerchantEntry::new(kind);
    match kind {
        MerchantEntryType::CardEnchant => {
            let options: Vec<(EnchantmentId, i32)> = SHOP_ENCHANTMENT_OPTIONS
                .into_iter()
                .filter(|&(e, _)| rs.master_deck.iter().any(|c| can_enchant(rs, c, e)))
                .collect();
            if !options.is_empty() {
                let idx = rs.rng.shops.next_int_exclusive(0, options.len() as i32) as usize;
                entry.enchantment = Some(options[idx]);
            }
        }
        MerchantEntryType::PrismaticCardReward => {
            let own = rs.character_id;
            let others: Vec<CharacterId> =
                ALL_CHARACTERS.into_iter().filter(|&c| c != own).collect();
            let idx = rs.rng.shops.next_int_exclusive(0, others.len() as i32) as usize;
            entry.character = Some(others[idx]);
        }
        _ => {}
    }
    entry
}

/// [`sts2_core::run_manager::ContentCallbacks::generate_blitz_merchant`]:
/// C# `MerchantInventory.CreateForBlitzMerchant` ->
/// `GetBlitzEntries` (`MerchantInventory.cs:132-137,209-235`), all on
/// `PlayerRng.Shops`.
pub fn generate_merchant(rs: &mut RunState) -> MerchantInventory {
    // The roll borrows the stream; a constructor needs the run state and the
    // same stream. Swap the stream out for the duration.
    let mut shops = std::mem::replace(&mut rs.rng.shops, Rng::new(0));
    let entries = sts2_core::blitz::roll_merchant_entries(&mut shops, |rng, kind| {
        std::mem::swap(&mut rs.rng.shops, rng);
        let entry = generate_entry(rs, kind);
        std::mem::swap(&mut rs.rng.shops, rng);
        entry
    });
    rs.rng.shops = shops;
    MerchantInventory::new(entries)
}

/// C# `BlitzMerchantEntry.GiveCurse` (`BlitzMerchantEntry.cs:193-201`):
/// `Shops.NextItem` over the curse pool's cards with
/// `CanBeGeneratedByModifiers && IsAllowedInRun`, ordered by id (Debt is not
/// allowed in Blitz, which leaves nine), then `CardPileCmd.AddCursesToDeck`.
fn give_curse(rs: &mut RunState) {
    let run = RunCardFilter::of(rs);
    let pool: Vec<CardId> = crate::relics::neow_relics::NEOWS_BONES_CURSE_POOL
        .iter()
        .copied()
        .filter(|&id| run.allows(id))
        .collect();
    if pool.is_empty() {
        return;
    }
    let idx = rs.rng.shops.next_int_exclusive(0, pool.len() as i32) as usize;
    if let Some(card) = crate::cards::v0111::card_pod_for(pool[idx], false, rs.game_version) {
        rs.add_card_to_deck_with_hooks(card, sts2_core::relic_effects::AddSource::Other);
    }
}

/// Queue a populated `CardReward` (`RewardsCmd.OfferCustom`).
fn offer_cards(rs: &mut RunState, cards: Vec<Card>) {
    if !cards.is_empty() {
        rs.pending_rewards.push(sts2_core::runstate::Reward::Card(
            sts2_core::runstate::CardReward::from_cards(&cards),
        ));
    }
}

/// A `new CardReward(options, 3, player)` over `roll`, populated: the roll,
/// then the reward's modify hooks (`IsCardReward`).
fn card_reward(rs: &mut RunState, roll: CardRoll) -> Vec<Card> {
    let cards = create_for_reward(rs, roll, 3);
    rs.offer_card_slate(cards, true)
}

/// A relic pulled as the Relic / TwoRelics wares pull theirs:
/// `RelicFactory.PullNextRelicFromBack(player, RollRarity(Shops), _ => true)`.
fn pull_relic(rs: &mut RunState) -> Option<sts2_core::RelicId> {
    let rarity = sts2_core::runstate::roll_relic_rarity(&mut rs.rng.shops);
    rs.pop_relic_from_grab_bag_back(rarity, |_| true)
        .map(|(id, _)| id)
}

/// The Blitz-odds, `Shops`-stream roll most card wares share.
const fn shop_roll(pool: Pool) -> CardRoll {
    CardRoll {
        pool,
        odds: Odds::Blitz,
        rarity: None,
        upgrade_roll: true,
        stream: Stream::Shops,
    }
}

/// [`sts2_core::run_manager::ContentCallbacks::blitz_merchant_select`]: a
/// ware's C# `OnTrySelect`, up to its first player decision. Every ware's
/// draws are on `PlayerRng.Shops` unless noted; a cursed ware gives its
/// curse first ([`give_curse`]).
#[allow(clippy::too_many_lines)]
pub fn merchant_select(rs: &mut RunState, entry: &MerchantEntry) -> SelectOutcome {
    use sts2_core::runstate::{PendingDeckTransform, RelicReward};
    let own = Pool::Character(rs.character_id);
    let version = rs.game_version;
    match entry.kind {
        // `FromDeckForUpgrade(prefs(1) { Cancelable, RequireManualConfirmation })`
        // (`BlitzMerchantCardUpgradeEntry.cs:21`): no upgradable card, no
        // screen and no purchase.
        MerchantEntryType::CardUpgrade => {
            if !rs.master_deck.iter().any(Card::is_upgradable) {
                return SelectOutcome::Refused;
            }
            rs.deck_pick_manual_confirm = true;
            rs.pending_deck_upgrades += 1;
            SelectOutcome::DeckPick(DeckPick::Upgrade)
        }
        // One 3-card reward, `Blitz` odds, `NoUpgrades`; `AfterGenerated`
        // upgrades all three (`BlitzMerchantUpgradedCardRewardEntry.cs:20`).
        MerchantEntryType::UpgradedCardReward => {
            let mut cards = card_reward(
                rs,
                CardRoll {
                    upgrade_roll: false,
                    ..shop_roll(own)
                },
            );
            upgrade_all(&mut cards, version);
            offer_cards(rs, cards);
            SelectOutcome::Taken
        }
        // `Deck.Where(IsUpgradable).ToList().StableShuffle(Shops).Take(2)`
        // (`BlitzMerchantRandomCardUpgradesEntry.cs:21`): the shuffle draws
        // even for an empty list, and with nothing to upgrade the purchase
        // fails (`FailureForbidden`) with the draw made.
        MerchantEntryType::RandomUpgrades => {
            let mut upgradable: Vec<usize> = rs
                .master_deck
                .iter()
                .enumerate()
                .filter(|(_, c)| c.is_upgradable())
                .map(|(i, _)| i)
                .collect();
            // `StableShuffle` sorts first: `List.Sort()` by `CardModel.
            // CompareTo` (`CardModel.cs:2473-2526`), which after the model
            // id and the upgrade level compares the enchantment, the
            // affliction, the cost, the replay count and the dynamic vars.
            // Two Strikes are therefore not interchangeable once one of them
            // carries Sharp, and the sort puts the plain one first.
            let deck = rs.master_deck.clone();
            rs.rng
                .shops
                .stable_shuffle_by_for(version, &mut upgradable, &|a: usize, b: usize| {
                    sts2_core::cards::card_model_compare_for(&deck[a], &deck[b], version)
                });
            if upgradable.is_empty() {
                return SelectOutcome::Refused;
            }
            for &i in upgradable.iter().take(2) {
                rs.master_deck[i].upgrade_for(version);
            }
            SelectOutcome::Taken
        }
        // `FromDeckForTransformation(prefs(1) { Cancelable })` then
        // `TransformToRandom(card, PlayerRng.Transformations)`
        // (`BlitzMerchantCardTransformEntry.cs:20`). The select needs no
        // screen when at most one card is transformable: none is no
        // purchase, one is transformed at once.
        MerchantEntryType::CardTransform => {
            let transformable = rs
                .master_deck
                .iter()
                .filter(|c| crate::cards::spec_index::card_is_transformable_in_deck(c))
                .count();
            match transformable {
                0 => SelectOutcome::Refused,
                1 => {
                    let _ = crate::run_callbacks::transform_from_deck(
                        rs,
                        &[PendingDeckTransform::default()],
                    );
                    SelectOutcome::Taken
                }
                _ => {
                    rs.pending_deck_transforms
                        .push(PendingDeckTransform::default());
                    SelectOutcome::DeckPick(DeckPick::Transform)
                }
            }
        }
        // `MerchantCardRemovalEntry.DoCardRemoval(player, isCancelable: true)`
        // (`MerchantCardRemovalEntry.cs:69-82`, manual confirmation).
        MerchantEntryType::CardRemoval => {
            if rs.removal_offer_order().is_empty() {
                return SelectOutcome::Refused;
            }
            rs.deck_pick_manual_confirm = true;
            rs.pending_deck_removals += 1;
            SelectOutcome::DeckPick(DeckPick::Removal)
        }
        // Two 3-card rewards, populated in order
        // (`BlitzMerchantCardRewardsEntry.cs:20`).
        MerchantEntryType::CardRewards => {
            for _ in 0..2 {
                let cards = card_reward(rs, shop_roll(own));
                offer_cards(rs, cards);
            }
            SelectOutcome::Taken
        }
        MerchantEntryType::ColorlessCardReward => {
            let cards = card_reward(rs, shop_roll(Pool::Colorless));
            offer_cards(rs, cards);
            SelectOutcome::Taken
        }
        // `GainMaxPotionCount(1)`, then two `PotionReward.SetRng(Shops)`
        // (`BlitzMerchantPotionsEntry.cs:18`), both populated before either
        // is taken.
        MerchantEntryType::Potions => {
            rs.gain_potion_slots(1);
            let character = rs.character_id;
            let potions: Vec<_> = (0..2)
                .filter_map(|_| {
                    crate::potions::all::create_random_potions_out_of_combat(
                        &mut rs.rng.shops,
                        character,
                        1,
                    )
                    .into_iter()
                    .next()
                })
                .collect();
            for kind in potions {
                let _ = rs.claim_potion_reward(kind);
            }
            SelectOutcome::Taken
        }
        // `FromDeckForEnchantment(player, model, amount, prefs(1) { Cancelable })`
        // (`BlitzMerchantCardEnchantEntry.cs:62`): no eligible card is no
        // purchase.
        MerchantEntryType::CardEnchant => {
            let Some((enchantment, amount)) = entry.enchantment else {
                return SelectOutcome::Refused;
            };
            enchant_from_deck(rs, enchantment, amount, false)
        }
        // A 3-card reward whose `AfterGenerated` gives every card without an
        // enchantment `Shops.NextItem` over the options that can enchant it
        // (`BlitzMerchantEnchantedCardRewardEntry.cs:22`).
        MerchantEntryType::EnchantedCardReward => {
            let mut cards = card_reward(rs, shop_roll(own));
            for card in &mut cards {
                if !card.enchantments.is_empty() {
                    continue;
                }
                let options: Vec<(EnchantmentId, i32)> = SHOP_ENCHANTMENT_OPTIONS
                    .into_iter()
                    .filter(|&(e, _)| can_enchant(rs, card, e))
                    .collect();
                if options.is_empty() {
                    continue;
                }
                let idx = rs.rng.shops.next_int_exclusive(0, options.len() as i32) as usize;
                let (enchantment, amount) = options[idx];
                let _ = crate::enchantments::apply_static_enchantment(card, enchantment, amount);
            }
            offer_cards(rs, cards);
            SelectOutcome::Taken
        }
        MerchantEntryType::PrismaticCardReward => {
            let character = entry.character.unwrap_or(CharacterId::Ironclad);
            let cards = card_reward(rs, shop_roll(Pool::Character(character)));
            offer_cards(rs, cards);
            SelectOutcome::Taken
        }
        // `Blitz` odds with `c => c.Rarity == Rare`: the rarity float is
        // drawn and then walked up to Rare (`BlitzMerchantRareCardRewardEntry.cs:21`).
        MerchantEntryType::RareCardReward => {
            let cards = card_reward(
                rs,
                CardRoll {
                    rarity: Some(CardRarity::Rare),
                    ..shop_roll(own)
                },
            );
            offer_cards(rs, cards);
            SelectOutcome::Taken
        }
        // `CreateForReward(player, 1, Uniform, Rare, NoUpgradeRoll, Shops)`,
        // upgraded and added to the deck with no screen
        // (`BlitzMerchantUpgradedRareCardEntry.cs:22`).
        MerchantEntryType::UpgradedRareCard => {
            let cards = create_for_reward(
                rs,
                CardRoll {
                    pool: own,
                    odds: Odds::Uniform,
                    rarity: Some(CardRarity::Rare),
                    upgrade_roll: false,
                    stream: Stream::Shops,
                },
                1,
            );
            let mut cards = rs.offer_card_slate(cards, false);
            upgrade_all(&mut cards, version);
            for card in cards {
                rs.add_card_to_deck_with_hooks(card, sts2_core::relic_effects::AddSource::Other);
            }
            SelectOutcome::Taken
        }
        MerchantEntryType::Relic => {
            if let Some(id) = pull_relic(rs) {
                rs.relic_rewards.push(RelicReward::new(id));
            }
            SelectOutcome::Taken
        }
        // `FromDeckForEnchantment(player, Glam, 1, prefs(1) { Cancelable })`;
        // the curse comes after the pick (`BlitzMerchantGlamEntry.cs:52`,
        // [`merchant_after_pick`]).
        MerchantEntryType::Glam => enchant_from_deck(rs, EnchantmentId::Glam, 1, true),
        // `BlitzChoicesRemaining += 2` is the run loop's.
        MerchantEntryType::TwoMoreChoices => {
            give_curse(rs);
            SelectOutcome::Taken
        }
        // Three rewards, Common / Uncommon / Rare, each
        // `ForNonCombatWithUniformOdds(.., rarity).WithFlags(NoRarityModification)`
        // with NO rng override: nine `PlayerRng.Rewards` picks
        // (`BlitzMerchantCardRewardOfEachRarityEntry.cs:21`).
        MerchantEntryType::CardRewardOfEachRarity => {
            give_curse(rs);
            for rarity in [CardRarity::Common, CardRarity::Uncommon, CardRarity::Rare] {
                let cards = card_reward(
                    rs,
                    CardRoll {
                        pool: own,
                        odds: Odds::Uniform,
                        rarity: Some(rarity),
                        upgrade_roll: false,
                        stream: Stream::Rewards,
                    },
                );
                offer_cards(rs, cards);
            }
            SelectOutcome::Taken
        }
        // A Rare-filtered `Blitz` reward (upgrade roll kept), all three
        // upgraded by `AfterGenerated`
        // (`BlitzMerchantUpgradedRareCardRewardEntry.cs:21`).
        MerchantEntryType::UpgradedRareCardReward => {
            give_curse(rs);
            let mut cards = card_reward(
                rs,
                CardRoll {
                    rarity: Some(CardRarity::Rare),
                    ..shop_roll(own)
                },
            );
            upgrade_all(&mut cards, version);
            offer_cards(rs, cards);
            SelectOutcome::Taken
        }
        MerchantEntryType::TwoRelics => {
            give_curse(rs);
            for _ in 0..2 {
                if let Some(id) = pull_relic(rs) {
                    rs.relic_rewards.push(RelicReward::new(id));
                }
            }
            SelectOutcome::Taken
        }
        // `GainMaxHp(6)`, then heal to full
        // (`BlitzMerchantFullHealAndMaxHpEntry.cs:17`).
        MerchantEntryType::FullHealAndMaxHp => {
            give_curse(rs);
            rs.gain_max_hp(6);
            let missing = rs.max_hp - rs.current_hp;
            let _ = rs.heal(missing);
            SelectOutcome::Taken
        }
    }
}

/// C# `CardSelectCmd.FromDeckForEnchantment(player, enchantment, amount,
/// prefs(1) { Cancelable })` (`CardSelectCmd.cs:491-535`) and what the two
/// enchant wares do with its card. No eligible deck card: no purchase. One:
/// it is returned with no screen (`cards.Count <= prefs.MinSelect`) and
/// enchanted at once, after the curse when `cursed` (Glam). More: the pick is
/// queued for the player.
fn enchant_from_deck(
    rs: &mut RunState,
    enchantment: EnchantmentId,
    amount: i32,
    cursed: bool,
) -> SelectOutcome {
    use sts2_core::runstate::PendingDeckEnchant;
    let eligible: Vec<usize> = (0..rs.master_deck.len())
        .filter(|&i| can_enchant(rs, &rs.master_deck[i], enchantment))
        .collect();
    match eligible.as_slice() {
        [] => SelectOutcome::Refused,
        &[only] => {
            if cursed {
                give_curse(rs);
            }
            let _ = crate::enchantments::apply_static_enchantment(
                &mut rs.master_deck[only],
                enchantment,
                amount,
            );
            SelectOutcome::Taken
        }
        _ => {
            let type_restriction =
                matches!(enchantment, EnchantmentId::Sharp | EnchantmentId::Vigorous)
                    .then_some(CardType::Attack);
            rs.pending_deck_enchants.push(PendingDeckEnchant {
                enchantment,
                amount,
                type_restriction,
                secondary_type: None,
                optional: false,
            });
            SelectOutcome::DeckPick(DeckPick::Enchant)
        }
    }
}

/// [`sts2_core::run_manager::ContentCallbacks::blitz_merchant_after_pick`]:
/// what a ware's `OnTrySelect` does once its deck pick returned a card.
/// Glam gives its curse (`GiveCurse` runs between the pick and the enchant;
/// the curse lands at the deck tail either way); a removal counts toward
/// `CardShopRemovalsUsed` (`MerchantCardRemovalEntry.cs:78`).
pub fn merchant_after_pick(rs: &mut RunState, entry: &MerchantEntry) {
    match entry.kind {
        MerchantEntryType::Glam => give_curse(rs),
        MerchantEntryType::CardRemoval => {
            rs.card_shop_removals_used = rs.card_shop_removals_used.saturating_add(1);
        }
        _ => {}
    }
}
