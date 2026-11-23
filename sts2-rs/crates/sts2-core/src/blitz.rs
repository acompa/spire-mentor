//! Blitz mode (GH #1347): the run-side rules of C# `GameMode.Blitz`
//! (v0.113.0), a `BlitzRunModel` run (`Models.Blitz/BlitzRunModel.cs`).
//!
//! A Blitz act is a fixed line of 8 nodes ([`act_map`], `Map/BlitzActMap.cs`)
//! built with no RNG, every act opens on its Ancient, and the ascension
//! ladder is 0..=5 naming a set of standard levels
//! ([`crate::ascension::blitz_levels`]). The mode replaces the room rewards,
//! the merchant and the chest, and bans some cards and relics.

use crate::map::{ActMap, MapPointType};
use crate::RelicId;

/// C# `BlitzActMap.pointTypes` (`BlitzActMap.cs:10-18`): rows 1..=6 of
/// every act, between the Ancient (row 0) and the boss (row 7).
pub const POINT_TYPES: [MapPointType; 6] = [
    MapPointType::Monster,
    MapPointType::Monster,
    MapPointType::Shop,
    MapPointType::Elite,
    MapPointType::Treasure,
    MapPointType::RestSite,
];

/// `BlitzActMap._middle`: every point's column.
pub const COLUMN: i32 = 3;

/// C# `new BlitzActMap(state)` (`BlitzActMap.cs:30-51`, `ConnectChildren`
/// `:94-111`): the Ancient at `(3, 0)`, [`POINT_TYPES`] at `(3, 1..=6)`,
/// the boss at `(3, 7)`, each the only child of the one before. No RNG.
///
/// There is no second boss: Blitz never holds `DoubleBoss`
/// (`AscensionHelper.GetLevelForBlitz`), so `Act.HasSecondBoss` is false.
#[must_use]
pub fn act_map() -> ActMap {
    let rooms = POINT_TYPES.len() as i32;
    let mut map = ActMap::new(rooms);
    let start = map.get_or_create(COLUMN, 0);
    map.point_mut(start).point_type = MapPointType::Ancient;
    map.start = Some(start);
    let mut prev = start;
    for (i, &point_type) in POINT_TYPES.iter().enumerate() {
        let idx = map.get_or_create(COLUMN, i as i32 + 1);
        map.point_mut(idx).point_type = point_type;
        map.add_child(prev, idx);
        prev = idx;
    }
    let boss = map.get_or_create(COLUMN, rooms + 1);
    map.point_mut(boss).point_type = MapPointType::Boss;
    map.add_child(prev, boss);
    map.boss = Some(boss);
    map
}

/// C# `RelicModel.IsLateInAct3`'s Blitz threshold (`RelicModel.cs:478-487`):
/// `(pointTypes.Length + 2) * 2 + pointTypes.IndexOf(Elite) + 2 = 21`, the
/// act-3 Elite node in C#'s `TotalFloor` (which counts each act's Ancient).
/// A standard single-player run uses 41.
pub const LATE_IN_ACT3_TOTAL_FLOOR: i32 = (POINT_TYPES.len() as i32 + 2) * 2 + 3 + 2;

/// C# `RelicLists.notAllowedInBlitz` (`Helpers/RelicLists.cs:15-61`): the 44
/// relics `RelicModel.IsAllowed` refuses in a Blitz run (`RelicModel.cs:444`).
pub const RELICS_NOT_ALLOWED: [RelicId; 44] = [
    RelicId::AmethystAubergine,
    RelicId::BlackStar,
    RelicId::BookOfFiveRings,
    RelicId::BowlerHat,
    RelicId::CursedPearl,
    RelicId::DowsingRod,
    RelicId::DragonFruit,
    RelicId::DreamCatcher,
    RelicId::Driftwood,
    RelicId::Ectoplasm,
    RelicId::EternalFeather,
    RelicId::FishingRod,
    RelicId::FurCoat,
    RelicId::Girya,
    RelicId::GoldenCompass,
    RelicId::GoldenPearl,
    RelicId::JuzuBracelet,
    RelicId::LastingCandy,
    RelicId::LordsParasol,
    RelicId::LuckyFysh,
    RelicId::MawBank,
    RelicId::MealTicket,
    RelicId::MeatCleaver,
    RelicId::MembershipCard,
    RelicId::MiniatureTent,
    RelicId::OldCoin,
    RelicId::PaelsClaw,
    RelicId::PaelsGrowth,
    RelicId::PaelsWing,
    RelicId::Planisphere,
    RelicId::PrayerWheel,
    RelicId::PumpkinCandle,
    RelicId::RegalPillow,
    RelicId::SealOfGold,
    RelicId::Shovel,
    RelicId::SignetRing,
    RelicId::SilkenTress,
    RelicId::StoneHumidifier,
    RelicId::TheCourier,
    RelicId::TinyMailbox,
    RelicId::ToyBox,
    RelicId::VenerableTeaSet,
    RelicId::WhiteBeastStatue,
    RelicId::WingedBoots,
];

/// Whether `id` is on [`RELICS_NOT_ALLOWED`].
#[must_use]
pub fn relic_not_allowed(id: RelicId) -> bool {
    RELICS_NOT_ALLOWED.contains(&id)
}

/// C# `CardModel.IsAllowedInBlitz == false` (`Models/CardModel.cs:357`,
/// overridden by `Debt.cs:15`, `HandOfGreed.cs:27`, `MyShinies.cs:17`,
/// `Royalties.cs:15`): `IsAllowedInRun` drops these four in a Blitz run
/// (`CardModel.cs:1209`), so `CardFactory.FilterForRun` keeps them out of
/// every generated pool.
#[must_use]
pub const fn card_not_allowed(id: crate::cards::CardId) -> bool {
    use crate::cards::CardId;
    matches!(
        id,
        CardId::Debt | CardId::HandOfGreed | CardId::MyShinies | CardId::RoyaltiesCard
    )
}

/// C# `BlitzMerchantEntryType` (`Entities.Merchant/BlitzMerchantEntryType.cs`),
/// in enum order. The first 14 are the regular wares; from
/// [`MerchantEntryType::Glam`] on they are the "cursed" ones
/// (`BlitzMerchantEntry.IsCursed`, `BlitzMerchantEntry.cs:53`), which give a
/// curse when taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MerchantEntryType {
    /// Pick a deck card to upgrade.
    CardUpgrade,
    /// A 3-card reward, all upgraded.
    UpgradedCardReward,
    /// Upgrade two random deck cards.
    RandomUpgrades,
    /// Pick a deck card to transform.
    CardTransform,
    /// Pick a deck card to remove.
    CardRemoval,
    /// Two 3-card rewards.
    CardRewards,
    /// A 3-card colorless reward.
    ColorlessCardReward,
    /// One more potion slot and two potions.
    Potions,
    /// Pick a deck card for a rolled enchantment.
    CardEnchant,
    /// A 3-card reward, every card enchanted.
    EnchantedCardReward,
    /// A 3-card reward from another character's pool.
    PrismaticCardReward,
    /// A 3-card Rare reward.
    RareCardReward,
    /// One upgraded Rare card, added to the deck.
    UpgradedRareCard,
    /// A relic.
    Relic,
    /// Cursed: pick a deck card for Glam.
    Glam,
    /// Cursed: two more picks at this merchant.
    TwoMoreChoices,
    /// Cursed: a Common, an Uncommon and a Rare 3-card reward.
    CardRewardOfEachRarity,
    /// Cursed: a 3-card Rare reward, all upgraded.
    UpgradedRareCardReward,
    /// Cursed: two relics.
    TwoRelics,
    /// Cursed: +6 max HP and a full heal.
    FullHealAndMaxHp,
}

impl MerchantEntryType {
    /// Every type, in C# enum order (`Enum.GetValues<BlitzMerchantEntryType>()`).
    pub const ALL: [MerchantEntryType; 20] = [
        Self::CardUpgrade,
        Self::UpgradedCardReward,
        Self::RandomUpgrades,
        Self::CardTransform,
        Self::CardRemoval,
        Self::CardRewards,
        Self::ColorlessCardReward,
        Self::Potions,
        Self::CardEnchant,
        Self::EnchantedCardReward,
        Self::PrismaticCardReward,
        Self::RareCardReward,
        Self::UpgradedRareCard,
        Self::Relic,
        Self::Glam,
        Self::TwoMoreChoices,
        Self::CardRewardOfEachRarity,
        Self::UpgradedRareCardReward,
        Self::TwoRelics,
        Self::FullHealAndMaxHp,
    ];

    /// The C# enum member name (`"CardRewardOfEachRarity"`), the spelling
    /// the oracle and the mod label a ware with (`blitz:<name>`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CardUpgrade => "CardUpgrade",
            Self::UpgradedCardReward => "UpgradedCardReward",
            Self::RandomUpgrades => "RandomUpgrades",
            Self::CardTransform => "CardTransform",
            Self::CardRemoval => "CardRemoval",
            Self::CardRewards => "CardRewards",
            Self::ColorlessCardReward => "ColorlessCardReward",
            Self::Potions => "Potions",
            Self::CardEnchant => "CardEnchant",
            Self::EnchantedCardReward => "EnchantedCardReward",
            Self::PrismaticCardReward => "PrismaticCardReward",
            Self::RareCardReward => "RareCardReward",
            Self::UpgradedRareCard => "UpgradedRareCard",
            Self::Relic => "Relic",
            Self::Glam => "Glam",
            Self::TwoMoreChoices => "TwoMoreChoices",
            Self::CardRewardOfEachRarity => "CardRewardOfEachRarity",
            Self::UpgradedRareCardReward => "UpgradedRareCardReward",
            Self::TwoRelics => "TwoRelics",
            Self::FullHealAndMaxHp => "FullHealAndMaxHp",
        }
    }

    /// C# `BlitzMerchantEntry.IsCursed`: `MerchantEntryType >= Glam`.
    #[must_use]
    pub fn is_cursed(self) -> bool {
        self >= Self::Glam
    }

    /// C# `BlitzMerchantEntry.RareBlitzEntries` (`BlitzMerchantEntry.cs:33-49`):
    /// the three wares rolled at half weight.
    #[must_use]
    pub const fn is_rare(self) -> bool {
        matches!(
            self,
            Self::PrismaticCardReward | Self::RareCardReward | Self::UpgradedRareCard
        )
    }
}

/// C# `MerchantInventory.GetBlitzEntries`'s type roll
/// (`MerchantInventory.cs:209-235`): five regular wares drawn without
/// replacement, each `Shops.WeightedNextItem` over the remaining regular
/// types in enum order (weight 0.5 for a [`MerchantEntryType::is_rare`] one,
/// else 1), then one cursed ware, `Shops.NextItem` over the six cursed types.
///
/// `generate` is C# `BlitzMerchantEntry.Generate(player, type)`, called right
/// after each pick with the same stream: a ware's constructor may draw from
/// it (CardEnchant, PrismaticCardReward) before the next ware is rolled.
///
/// `WeightedNextItem` (`Random/Rng.cs:320-346`) draws one `NextFloat`, scales
/// it by the weight sum and walks the list subtracting weights until the
/// remainder is `<= 0`; if it never is, it returns `default(T)`, the enum's
/// zero value `CardUpgrade`, which `list.Remove` then fails to find when it
/// was already taken (so nothing leaves the list).
pub fn roll_merchant_entries<E>(
    rng: &mut crate::dotnet_rng::Rng,
    mut generate: impl FnMut(&mut crate::dotnet_rng::Rng, MerchantEntryType) -> E,
) -> Vec<E> {
    let (mut regular, cursed): (Vec<MerchantEntryType>, Vec<MerchantEntryType>) =
        MerchantEntryType::ALL.iter().partition(|t| !t.is_cursed());
    let weight = |t: MerchantEntryType| if t.is_rare() { 0.5_f32 } else { 1.0_f32 };
    let mut out = Vec::with_capacity(6);
    while out.len() < 5 && !regular.is_empty() {
        // `items.Sum(weightFetcher)` over floats accumulates in a double and
        // narrows the total; halves and ones are exact either way.
        let total = regular.iter().map(|&t| f64::from(weight(t))).sum::<f64>() as f32;
        let mut left = rng.next_float_f32() * total;
        let mut picked = MerchantEntryType::CardUpgrade;
        for &t in &regular {
            left -= weight(t);
            if left <= 0.0 {
                picked = t;
                break;
            }
        }
        if let Some(pos) = regular.iter().position(|&t| t == picked) {
            regular.remove(pos);
        }
        out.push(generate(rng, picked));
    }
    let idx = rng.next_int_exclusive(0, cursed.len() as i32) as usize;
    out.push(generate(rng, cursed[idx]));
    out
}

/// C# `BlitzMerchantEntry.BlitzEntryStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryStatus {
    /// Can be taken.
    Available,
    /// Was taken.
    Used,
    /// The picks ran out before it was taken.
    Disabled,
}

impl EntryStatus {
    /// The C# enum member name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Available => "Available",
            Self::Used => "Used",
            Self::Disabled => "Disabled",
        }
    }
}

/// One ware of the Blitz merchant (C# `BlitzMerchantEntry`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MerchantEntry {
    /// The ware.
    pub kind: MerchantEntryType,
    /// Whether it can still be taken.
    pub status: EntryStatus,
    /// `CardEnchant` only: the enchantment and amount its constructor rolled
    /// (`BlitzMerchantCardEnchantEntry.cs:54-60`); `None` when no option
    /// fits the deck (C# leaves a null model there).
    pub enchantment: Option<(crate::cards::EnchantmentId, i32)>,
    /// `PrismaticCardReward` only: the other character its constructor
    /// rolled (`BlitzMerchantPrismaticCardRewardEntry.cs:41-49`).
    pub character: Option<crate::CharacterId>,
}

impl MerchantEntry {
    /// A fresh, available ware of `kind` with no rolled extras.
    #[must_use]
    pub const fn new(kind: MerchantEntryType) -> Self {
        Self {
            kind,
            status: EntryStatus::Available,
            enchantment: None,
            character: None,
        }
    }
}

/// C# `MerchantInventory.CreateForBlitzMerchant` (`MerchantInventory.cs:
/// 132-137`): no cards, relics, potions or removal, six Blitz wares and
/// `BlitzChoicesRemaining = 3` (`Setup`, `:152-155`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerchantInventory {
    /// The six wares, in roll order (the cursed one last).
    pub entries: Vec<MerchantEntry>,
    /// C# `BlitzChoicesRemaining`.
    pub choices_remaining: i32,
}

/// `MerchantInventory._defaultBlitzChoices`.
pub const MERCHANT_CHOICES: i32 = 3;

impl MerchantInventory {
    /// A merchant offering `entries`, with the default three picks.
    #[must_use]
    pub const fn new(entries: Vec<MerchantEntry>) -> Self {
        Self {
            entries,
            choices_remaining: MERCHANT_CHOICES,
        }
    }

    /// Whether ware `idx` can be taken now (C# `IsStocked`; the host also
    /// refuses with no picks left).
    #[must_use]
    pub fn can_take(&self, idx: usize) -> bool {
        self.choices_remaining > 0
            && self
                .entries
                .get(idx)
                .is_some_and(|e| e.status == EntryStatus::Available)
    }

    /// C# `MerchantInventory.UpdateEntries` (`:265-285`) after ware `idx`
    /// was taken (`SetUsed`): one pick fewer, and at zero every ware still
    /// available is disabled.
    pub fn complete_purchase(&mut self, idx: usize) {
        if let Some(e) = self.entries.get_mut(idx) {
            e.status = EntryStatus::Used;
        }
        self.choices_remaining -= 1;
        if self.choices_remaining <= 0 {
            for e in &mut self.entries {
                if e.status == EntryStatus::Available {
                    e.status = EntryStatus::Disabled;
                }
            }
        }
    }
}

/// What taking a ware did, as far as the run loop needs to know (C#
/// `BlitzMerchantEntry.OnTrySelect`'s result and what it awaited).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectOutcome {
    /// `OnTrySelect` returned true: the ware is used. Any rewards it offered
    /// are queued on the run state.
    Taken,
    /// `OnTrySelect` returned false with no screen (RandomUpgrades with
    /// nothing upgradable): the ware stays available and no pick is spent.
    Refused,
    /// A cancelable deck pick was queued on the run state; the ware is used
    /// once the pick resolves, or stays available if it is cancelled.
    DeckPick(DeckPick),
}

/// The cancelable deck select a ware opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckPick {
    /// `FromDeckForUpgrade` (CardUpgrade).
    Upgrade,
    /// `FromDeckForTransformation` (CardTransform).
    Transform,
    /// `FromDeckForRemoval` (CardRemoval).
    Removal,
    /// `FromDeckForEnchantment` (CardEnchant, Glam).
    Enchant,
}

/// A solo Blitz chest (C# `TreasureRoomRelicSynchronizer.BeginRelicPicking`,
/// `TreasureRoomRelicSynchronizer.cs:96-152`): the usual relic plus one
/// more, taken over two rounds (`_roundsLeft = 2`). A skip ends the picking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chest {
    /// The relics still in the chest, each with the index it was first
    /// listed at (the oracle's `rewards[].index`, which does not renumber).
    pub relics: Vec<(usize, RelicId)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapCoord;

    /// `BlitzActMap.cs:30-51,94-111`: one column, Ancient, the six rooms,
    /// the boss; each node's only child is the next.
    #[test]
    fn act_map_is_the_fixed_line() {
        let map = act_map();
        assert_eq!(map.map_length, 7);
        assert_eq!(map.second_boss, None);
        let types: Vec<MapPointType> = (0..=7)
            .map(|row| map.get_point(MapCoord::new(3, row)).unwrap().point_type)
            .collect();
        assert_eq!(
            types,
            [
                MapPointType::Ancient,
                MapPointType::Monster,
                MapPointType::Monster,
                MapPointType::Shop,
                MapPointType::Elite,
                MapPointType::Treasure,
                MapPointType::RestSite,
                MapPointType::Boss,
            ]
        );
        assert_eq!(map.points.len(), 8, "no node off the line");
        for row in 0..7 {
            let kids = map.children_of(MapCoord::new(3, row));
            assert_eq!(kids.len(), 1, "row {row}");
            assert_eq!(kids[0].coord, MapCoord::new(3, row + 1));
        }
        assert!(map.children_of(MapCoord::new(3, 7)).is_empty());
        assert_eq!(map.point(map.start.unwrap()).coord, MapCoord::new(3, 0));
        assert_eq!(map.point(map.boss.unwrap()).coord, MapCoord::new(3, 7));
    }

    /// `RelicModel.cs:478-487`: `(6 + 2) * 2 + 3 + 2`.
    #[test]
    fn late_in_act3_is_the_act3_elite() {
        assert_eq!(LATE_IN_ACT3_TOTAL_FLOOR, 21);
    }

    /// `RelicLists.cs:15-61` has 44 distinct entries.
    #[test]
    fn the_relic_ban_list_has_44_distinct_relics() {
        let set: std::collections::BTreeSet<RelicId> = RELICS_NOT_ALLOWED.into_iter().collect();
        assert_eq!(set.len(), 44);
        assert!(relic_not_allowed(RelicId::WingedBoots));
        assert!(!relic_not_allowed(RelicId::PotionBelt));
    }

    #[test]
    fn four_cards_are_not_allowed() {
        use crate::cards::CardId;
        for id in [
            CardId::Debt,
            CardId::HandOfGreed,
            CardId::MyShinies,
            CardId::RoyaltiesCard,
        ] {
            assert!(card_not_allowed(id));
        }
        assert!(!card_not_allowed(CardId::Greed));
    }

    /// `BlitzMerchantEntryType.cs`: 14 regular wares then 6 cursed ones;
    /// `BlitzMerchantEntry.RareBlitzEntries` is the half-weight trio.
    #[test]
    fn merchant_entry_types_split_14_regular_6_cursed() {
        let cursed: Vec<_> = MerchantEntryType::ALL
            .into_iter()
            .filter(|t| t.is_cursed())
            .collect();
        assert_eq!(
            cursed,
            [
                MerchantEntryType::Glam,
                MerchantEntryType::TwoMoreChoices,
                MerchantEntryType::CardRewardOfEachRarity,
                MerchantEntryType::UpgradedRareCardReward,
                MerchantEntryType::TwoRelics,
                MerchantEntryType::FullHealAndMaxHp,
            ]
        );
        let rare: Vec<_> = MerchantEntryType::ALL
            .into_iter()
            .filter(|t| t.is_rare())
            .collect();
        assert_eq!(
            rare,
            [
                MerchantEntryType::PrismaticCardReward,
                MerchantEntryType::RareCardReward,
                MerchantEntryType::UpgradedRareCard,
            ]
        );
        assert_eq!(MerchantEntryType::TwoRelics.name(), "TwoRelics");
    }

    /// `MerchantInventory.GetBlitzEntries` (`:209-235`): five distinct
    /// regular wares, then one cursed; six `Shops` draws when no
    /// constructor draws; a constructor's draw lands between two picks.
    #[test]
    fn merchant_roll_is_five_regular_then_one_cursed() {
        for seed in 0..200i32 {
            let mut rng = crate::dotnet_rng::Rng::new(seed);
            let before = rng.counter();
            let kinds = roll_merchant_entries(&mut rng, |_, k| k);
            assert_eq!(rng.counter() - before, 6, "seed {seed}");
            assert_eq!(kinds.len(), 6);
            assert!(kinds[..5].iter().all(|k| !k.is_cursed()));
            assert!(kinds[5].is_cursed());
            let distinct: std::collections::BTreeSet<_> = kinds.iter().collect();
            assert_eq!(distinct.len(), 6, "seed {seed}: {kinds:?}");
        }
        // The weighted walk, by hand: 14 regular wares weigh 12.5. A roll of
        // exactly 0 lands on the first ware; the same stream replayed gives
        // the same wares.
        let mut a = crate::dotnet_rng::Rng::new(7);
        let mut b = crate::dotnet_rng::Rng::new(7);
        let first = roll_merchant_entries(&mut a, |_, k| k);
        let again = roll_merchant_entries(&mut b, |rng, k| {
            if k == first[0] {
                let _ = rng.next_int_exclusive(0, 5);
            }
            k
        });
        assert_eq!(first[0], again[0]);
        assert_eq!(b.counter(), a.counter() + 1, "the constructor's draw");
    }

    /// `MerchantInventory.UpdateEntries` (`:265-285`).
    #[test]
    fn merchant_picks_run_out_and_disable_the_rest() {
        let mut inv = MerchantInventory::new(
            MerchantEntryType::ALL[..6]
                .iter()
                .map(|&k| MerchantEntry::new(k))
                .collect(),
        );
        assert_eq!(inv.choices_remaining, 3);
        inv.complete_purchase(0);
        inv.complete_purchase(2);
        assert!(inv.can_take(1) && !inv.can_take(0) && !inv.can_take(9));
        inv.complete_purchase(1);
        assert_eq!(inv.choices_remaining, 0);
        let statuses: Vec<_> = inv.entries.iter().map(|e| e.status).collect();
        assert_eq!(
            statuses,
            [
                EntryStatus::Used,
                EntryStatus::Used,
                EntryStatus::Used,
                EntryStatus::Disabled,
                EntryStatus::Disabled,
                EntryStatus::Disabled,
            ]
        );
        assert!(!inv.can_take(3));
    }
}
