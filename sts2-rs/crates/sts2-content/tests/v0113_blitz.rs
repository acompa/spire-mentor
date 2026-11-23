//! v0.113.0 Blitz mode (GH #1347): a `GameMode.Blitz` run, built the way the
//! PyO3 `RunManager(game_mode="blitz")` builds it and walked through the
//! run loop. Each test pins one piece of C# `BlitzRunModel` and friends
//! (in the v0.113.0 decompilation: `Models.Blitz/`,
//! `Map/BlitzActMap.cs`, `Entities.Merchant/BlitzMerchant*Entry.cs`).
//!
//! Fights are not played: `RunManager::resolve_combat_fast` stamps them won
//! and runs the real reward bridge, which is what these tests are about.

use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character,
    new_run_manager_for_character_with_ascension,
};
use sts2_core::blitz::{EntryStatus, MerchantEntryType};
use sts2_core::cards::CardId;
use sts2_core::map::MapPointType;
use sts2_core::run_manager::{ActionResult, Phase, RunAction, RunManager};
use sts2_core::runstate::RoomType;
use sts2_core::{AscensionLevel, CharacterId, GameMode, GameVersion};

const V113: GameVersion = GameVersion::V0_113_0;

const CHARACTERS: [CharacterId; 6] = [
    CharacterId::Ironclad,
    CharacterId::Silent,
    CharacterId::Defect,
    CharacterId::Regent,
    CharacterId::Necrobinder,
    CharacterId::Fool,
];

/// A v0.113.0 run of `mode` at `level`, in the order `sts2-py`'s constructor
/// chains the builders, standing at Neow.
fn run(mode: GameMode, character: CharacterId, seed: &str, level: u8) -> RunManager {
    let master = V113.run_seed_hash(&sts2_core::canonicalize_seed(seed));
    let rm = if level > 0 {
        new_run_manager_for_character_with_ascension(master as i32, character, level)
    } else {
        new_run_manager_for_character(master as i32, character)
    };
    let mut rm = rm
        .with_game_version(V113)
        .with_game_mode(mode)
        .with_run_seed(master)
        .with_neow_start();
    rm.run_state.set_agent_decisions();
    let mut rm = rm.with_rolled_act1();
    initialize_run_setup(&mut rm);
    rm
}

fn blitz(character: CharacterId, seed: &str, level: u8) -> RunManager {
    run(GameMode::Blitz, character, seed, level)
}

fn take(rm: &mut RunManager, action: RunAction) {
    assert_eq!(
        rm.take_action(action),
        ActionResult::Advanced,
        "{action:?} in {:?}",
        rm.phase()
    );
}

/// Resolve whatever is open with the first legal action until the run stands
/// on the map (or is over), winning fights outright. Wares are taken while
/// any is on offer, chest relics too.
fn to_map(rm: &mut RunManager) {
    for _ in 0..400 {
        match rm.phase() {
            Phase::MapChoice | Phase::RunOver
                if rm.get_available_actions().len() <= 1
                    && !matches!(
                        rm.get_available_actions().first(),
                        Some(
                            RunAction::UpgradeDeckCard { .. }
                                | RunAction::RemoveDeckCard { .. }
                                | RunAction::TransformDeckCard { .. }
                                | RunAction::EnchantDeckCard { .. }
                                | RunAction::GrantDeckCard { .. }
                                | RunAction::PotionSwap { .. }
                        )
                    ) =>
            {
                return
            }
            Phase::Combat => {
                assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);
            }
            _ => {
                let actions = rm.get_available_actions();
                let first = *actions.first().expect("an open phase offers an action");
                take(rm, first);
            }
        }
    }
    panic!("the run never settled: {:?}", rm.phase());
}

/// Step onto the next node of the line and return its room type.
fn step(rm: &mut RunManager) -> RoomType {
    let actions = rm.get_available_actions();
    assert_eq!(actions, [RunAction::Move { col: 3 }], "one way forward");
    take(rm, actions[0]);
    rm.run_state.current_room.expect("a room").room_type
}

/// `AscensionHelper.GetLevelForBlitz` (`AscensionHelper.cs:66-78`) and
/// `AscensionManager.ApplyEffectsTo` (`AscensionManager.cs:80-95`): Blitz
/// level n holds the first n of WearyTraveler, TightBelt, ToughEnemies,
/// DeadlyEnemies, AscendersBane and no other level.
#[test]
fn blitz_ascension_is_a_level_set() {
    for level in 0u8..=5 {
        let mut rm = blitz(CharacterId::Ironclad, "BLITZASC", level);
        let rs = &rm.run_state;
        assert_eq!((rs.game_mode, rs.ascension_level), (GameMode::Blitz, level));
        // WearyTraveler from 1: Neow leaves the run at 80% HP.
        let want_hp = if level >= 1 {
            rs.max_hp * 8 / 10
        } else {
            rs.max_hp
        };
        assert_eq!(rs.current_hp, want_hp, "level {level}");
        // TightBelt from 2.
        assert_eq!(rs.max_potion_slots, if level >= 2 { 2 } else { 3 });
        // AscendersBane only at 5, at the deck tail.
        let banes = rs
            .master_deck
            .iter()
            .filter(|c| c.card_id == CardId::AscendersBane)
            .count();
        assert_eq!(banes, usize::from(level == 5), "level {level}");
        if level == 5 {
            assert_eq!(
                rs.master_deck.last().map(|c| c.card_id),
                Some(CardId::AscendersBane)
            );
        }
        // Never Poverty, Inflation, Scarcity, SwarmingElites or DoubleBoss.
        for never in [
            AscensionLevel::SwarmingElites,
            AscensionLevel::Poverty,
            AscensionLevel::Inflation,
            AscensionLevel::Scarcity,
            AscensionLevel::DoubleBoss,
        ] {
            assert!(!rs.has_ascension(never), "level {level} {never:?}");
        }
        assert!(!rs.has_double_boss);
        assert_eq!(rs.card_rarity_odds.ascension_level, 0);
        // ToughEnemies from 3, DeadlyEnemies from 4: what a fight thresholds.
        to_map(&mut rm);
        assert_eq!(step(&mut rm), RoomType::Monster);
        let combat = rm.combat.as_ref().expect("a fight");
        assert_eq!(combat.ascension, [0, 0, 0, 8, 9, 9][usize::from(level)]);
        assert_eq!(combat.game_mode, GameMode::Blitz);
    }
}

/// The same numbers in a standard run keep the standard ladder.
#[test]
fn standard_ascension_setup_did_not_move() {
    for (level, slots, bane, double) in [
        (0u8, 3, 0, false),
        (3, 3, 0, false),
        (4, 2, 0, false),
        (5, 2, 1, false),
        (10, 2, 1, true),
    ] {
        let rm = run(GameMode::Standard, CharacterId::Ironclad, "BLITZASC", level);
        let rs = &rm.run_state;
        assert_eq!(rs.max_potion_slots, slots, "A{level}");
        let banes = rs
            .master_deck
            .iter()
            .filter(|c| c.card_id == CardId::AscendersBane)
            .count();
        assert_eq!(banes, bane, "A{level}");
        assert_eq!(rs.has_double_boss, double, "A{level}");
        assert_eq!(rs.card_rarity_odds.ascension_level, level);
        assert_eq!(rs.has_ascension(AscensionLevel::Poverty), level >= 3);
        assert_eq!(rs.combat_ascension(), level);
    }
}

/// `RunManager.with_game_mode` refuses what the game cannot start: a Blitz
/// level above 5 (`AscensionManager.maxAscensionAllowedBlitz`).
#[test]
#[should_panic(expected = "ascension is 0..=5")]
fn blitz_refuses_ascension_six() {
    let _ = blitz(CharacterId::Ironclad, "BLITZASC", 6);
}

/// `ActModel.GenerateRooms(.., isBlitz)` (`ActModel.cs:336-342`): one weak
/// encounter instead of the act's `NumberOfWeakEncounters`, the same total.
#[test]
fn blitz_generates_one_weak_encounter_per_act() {
    use sts2_core::run_room_gen::encounter_is_weak;
    for seed in ["BLITZ1", "BLITZ2", "BLITZ3"] {
        let b = blitz(CharacterId::Silent, seed, 0);
        let s = run(GameMode::Standard, CharacterId::Silent, seed, 0);
        for (act, (bg, sg)) in b
            .run_state
            .generated_rooms
            .iter()
            .zip(&s.run_state.generated_rooms)
            .enumerate()
        {
            let weak = |ids: &[&'static str]| ids.iter().filter(|id| encounter_is_weak(id)).count();
            assert_eq!(weak(&bg.normal_encounters), 1, "{seed} act {act}");
            assert!(encounter_is_weak(bg.normal_encounters[0]));
            assert!(!encounter_is_weak(bg.normal_encounters[1]));
            assert!(
                weak(&sg.normal_encounters) >= 2,
                "standard keeps its weak run"
            );
            assert_eq!(bg.normal_encounters.len(), sg.normal_encounters.len());
            assert_eq!(bg.elite_encounters.len(), 15);
        }
    }
}

/// `Neow.cs:218-223,295-307` and `BlitzRunModel.GiveNeowRewards`
/// (`BlitzRunModel.cs:26-44`): one option, no event `Rng`; ten cards off
/// `PlayerRng.Rewards` (a rarity float and a pick each), offered sorted by
/// rarity, exactly three taken.
#[test]
fn blitz_neow_is_ten_cards_pick_three() {
    for character in CHARACTERS {
        let mut rm = blitz(character, "BLITZNEOW", 0);
        assert_eq!(rm.phase(), Phase::Event);
        assert!(rm.run_state.neow_offers.is_none(), "no relic offers rolled");
        assert_eq!(
            rm.get_available_actions(),
            [RunAction::EventChoice { idx: 0 }],
            "{character:?}"
        );
        let deck = rm.run_state.master_deck.len();
        let rewards = rm.run_state.rng.rewards.counter();
        take(&mut rm, RunAction::EventChoice { idx: 0 });
        assert_eq!(rm.run_state.rng.rewards.counter() - rewards, 20);
        let grant = rm.run_state.pending_card_grants.first().expect("the grid");
        assert_eq!(
            (grant.options.len(), grant.pick, grant.min_pick),
            (10, 3, 3)
        );
        let version = rm.run_state.game_version;
        let ranks: Vec<u8> = grant
            .options
            .iter()
            .map(|c| {
                sts2_content::cards::spec_index::card_rarity_rank_csharp_in(c.card_id, version)
            })
            .collect();
        assert!(
            ranks.windows(2).all(|w| w[0] <= w[1]),
            "{character:?} {ranks:?}"
        );
        assert!(grant.options.iter().all(|c| !c.upgraded));
        let ids: std::collections::BTreeSet<CardId> =
            grant.options.iter().map(|c| c.card_id).collect();
        assert_eq!(ids.len(), 10, "distinct cards");
        // A mandatory pick of three: no confirm action until it is done.
        assert_eq!(rm.get_available_actions().len(), 10);
        for _ in 0..3 {
            take(&mut rm, RunAction::GrantDeckCard { idx: 0 });
        }
        assert_eq!(rm.run_state.master_deck.len(), deck + 3);
        assert!(rm.run_state.pending_card_grants.is_empty());
        assert_eq!(rm.phase(), Phase::MapChoice);
        assert_eq!(rm.get_available_actions(), [RunAction::Move { col: 3 }]);
    }
}

/// `BlitzActMap` (`BlitzActMap.cs:10-51`) and `RunManager.EnterAct`
/// (`RunManager.cs:1474`): every act is Ancient, Monster, Monster, Shop,
/// Elite, Treasure, RestSite, Boss with one way forward; acts 2 and 3 open on
/// their Ancient; the third boss wins the run (no second boss). 24 floors.
#[test]
fn a_blitz_run_walks_three_fixed_lines() {
    let line = [
        RoomType::Monster,
        RoomType::Monster,
        RoomType::Shop,
        RoomType::Elite,
        RoomType::Treasure,
        RoomType::RestSite,
        RoomType::Boss,
    ];
    for character in CHARACTERS {
        for seed in ["BLITZWALK1", "BLITZWALK2"] {
            let mut rm = blitz(character, seed, 5);
            for act in 1..=3u8 {
                to_map(&mut rm);
                assert_eq!(rm.run_state.act_index, act, "{character:?} {seed}");
                let map = rm.run_state.current_act_map.as_ref().expect("a map");
                assert_eq!(map.points.len(), 8);
                assert_eq!(
                    map.point(map.start.unwrap()).point_type,
                    MapPointType::Ancient
                );
                for (i, &want) in line.iter().enumerate() {
                    assert_eq!(
                        step(&mut rm),
                        want,
                        "{character:?} {seed} act {act} row {i}"
                    );
                    if i + 1 < line.len() {
                        to_map(&mut rm);
                        assert_eq!(rm.phase(), Phase::MapChoice);
                    }
                }
                assert_eq!(rm.run_state.csharp_total_floor(), i32::from(act) * 8);
                assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);
            }
            assert_eq!(rm.phase(), Phase::RunOver, "{character:?} {seed}");
            assert!(rm.run_state.player_won);
        }
    }
}

/// `BlitzRunModel.TryReplaceDefaultRoomRewards` (`BlitzRunModel.cs:90-118`)
/// and `TryModifyCardRewardOptionsLate` (`:46-88`), by `PlayerRng.Rewards`
/// draws: no gold and no potion-odds roll anywhere; a Monster room is three
/// card picks (plus a potion unless its encounter is weak); an Elite is
/// three picks and two relic rarity rolls; from act 2 one more draw for the
/// upgraded card(s).
#[test]
fn blitz_room_rewards_have_no_gold_and_fixed_rarities() {
    use sts2_content::cards::spec_index::card_rarity_rank_csharp_in as rank;
    let mut rm = blitz(CharacterId::Ironclad, "BLITZREWARD", 0);
    for act in 1..=2u8 {
        to_map(&mut rm);
        let version = rm.run_state.game_version;
        let pity = rm.run_state.card_rarity_odds.current_value;
        // Weak monster: card reward only.
        let (gold, potions) = (rm.run_state.gold, rm.run_state.potions.len());
        assert_eq!(step(&mut rm), RoomType::Monster);
        let before = rm.run_state.rng.rewards.counter();
        assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);
        assert_eq!(rm.phase(), Phase::CardReward);
        let late = u32::from(act >= 2);
        let late_cards = usize::from(act >= 2);
        assert_eq!(
            rm.run_state.rng.rewards.counter() - before,
            3 + late,
            "act {act}"
        );
        assert_eq!(
            (rm.run_state.gold, rm.run_state.potions.len()),
            (gold, potions)
        );
        let options = rm.card_reward_options().to_vec();
        assert_eq!(options.len(), 3);
        assert!(options.iter().all(|&id| rank(id, version) == 2), "Common");
        let upgraded = rm
            .card_reward_upgraded_flags()
            .iter()
            .filter(|&&u| u)
            .count();
        assert!(upgraded <= late_cards, "act {act}: {upgraded} upgraded");
        to_map(&mut rm);
        // Regular monster: the card reward, then a potion.
        assert_eq!(step(&mut rm), RoomType::Monster);
        let before = rm.run_state.rng.rewards.counter();
        assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);
        assert!(rm.run_state.rng.rewards.counter() - before > 3 + late);
        assert_eq!(rm.run_state.gold, gold);
        assert_eq!(rm.run_state.potions.len(), potions + 1);
        to_map(&mut rm);
        assert_eq!(step(&mut rm), RoomType::Shop);
        take(&mut rm, RunAction::LeaveShop);
        // Elite: three Uncommons, two relics.
        let relics = rm.run_state.relics.len();
        let gold = rm.run_state.gold;
        assert_eq!(step(&mut rm), RoomType::Elite);
        let before = rm.run_state.rng.rewards.counter();
        assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);
        assert_eq!(rm.run_state.rng.rewards.counter() - before, 3 + late + 2);
        assert!(rm
            .card_reward_options()
            .iter()
            .all(|&id| rank(id, version) == 3));
        assert_eq!(
            rm.run_state.relic_rewards.len() + rm.run_state.relics.len(),
            relics + 2
        );
        assert_eq!(rm.run_state.gold, gold, "no elite gold");
        assert_eq!(
            rm.run_state.card_rarity_odds.current_value, pity,
            "uniform odds leave the pity alone"
        );
        to_map(&mut rm);
        assert_eq!(step(&mut rm), RoomType::Treasure);
        to_map(&mut rm);
        assert_eq!(step(&mut rm), RoomType::RestSite);
        to_map(&mut rm);
        // Boss before the last act: three Rares (never upgraded late), a
        // potion, no gold.
        let gold = rm.run_state.gold;
        assert_eq!(step(&mut rm), RoomType::Boss);
        assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);
        assert_eq!(rm.phase(), Phase::CardReward);
        assert!(rm
            .card_reward_options()
            .iter()
            .all(|&id| rank(id, version) == 4));
        assert!(rm.card_reward_upgraded_flags().iter().all(|&u| !u));
        assert_eq!(rm.run_state.gold, gold);
    }
}

/// `TreasureRoomRelicSynchronizer.BeginRelicPicking` / `AwardRelics`
/// (`TreasureRoomRelicSynchronizer.cs:96-152,241-333`) and
/// `OneOffSynchronizer.DoTreasureRoomRewards` (`:79-97`) for a solo Blitz
/// chest: two rarity rolls, one shuffle draw per round, two rounds, no gold;
/// a skip ends it with nothing more.
#[test]
fn blitz_chest_is_two_relics_over_two_rounds() {
    let at_chest = |seed: &str| {
        let mut rm = blitz(CharacterId::Defect, seed, 0);
        loop {
            to_map(&mut rm);
            let actions = rm.get_available_actions();
            let before = (
                rm.run_state.rng.treasure_room.counter(),
                rm.run_state.rng.rewards.counter(),
                rm.run_state.gold,
            );
            take(&mut rm, actions[0]);
            if rm.run_state.current_room.unwrap().room_type == RoomType::Treasure {
                return (rm, before);
            }
        }
    };
    let (mut rm, (treasure, rewards, gold)) = at_chest("BLITZCHEST");
    assert_eq!(rm.phase(), Phase::Treasure);
    assert_eq!(rm.run_state.rng.treasure_room.counter() - treasure, 2);
    assert_eq!(rm.run_state.rng.rewards.counter(), rewards, "no gold roll");
    assert_eq!(rm.run_state.gold, gold);
    let chest: Vec<_> = rm.blitz_chest().expect("an open chest").relics.clone();
    assert_eq!(chest.len(), 2);
    assert_eq!(
        rm.get_available_actions(),
        [
            RunAction::TakeChestRelic { idx: 0 },
            RunAction::TakeChestRelic { idx: 1 },
            RunAction::SkipRelic,
        ]
    );
    // Round one: the second relic first. The other stays, under its index.
    let relics = rm.run_state.relics.len();
    take(&mut rm, RunAction::TakeChestRelic { idx: 1 });
    assert_eq!(rm.run_state.rng.treasure_room.counter() - treasure, 3);
    assert!(rm.run_state.relics.contains(&chest[1].1));
    settle_picks(&mut rm);
    assert_eq!(rm.phase(), Phase::Treasure);
    assert_eq!(
        rm.get_available_actions(),
        [RunAction::TakeChestRelic { idx: 0 }, RunAction::SkipRelic]
    );
    take(&mut rm, RunAction::TakeChestRelic { idx: 0 });
    assert_eq!(rm.run_state.rng.treasure_room.counter() - treasure, 4);
    assert!(rm.run_state.relics.len() >= relics + 2);
    assert!(rm.blitz_chest().is_none());

    // A skip closes the chest: no relic, no shuffle draw.
    let (mut rm, (treasure, _, _)) = at_chest("BLITZCHEST");
    let relics = rm.run_state.relics.clone();
    take(&mut rm, RunAction::SkipRelic);
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert_eq!(rm.run_state.relics, relics);
    assert_eq!(rm.run_state.rng.treasure_room.counter() - treasure, 2);
}

/// Resolve whatever pick or reward screen a just-obtained relic opened.
fn settle_picks(rm: &mut RunManager) {
    for _ in 0..50 {
        let actions = rm.get_available_actions();
        let settle = matches!(
            actions.first(),
            Some(
                RunAction::UpgradeDeckCard { .. }
                    | RunAction::RemoveDeckCard { .. }
                    | RunAction::TransformDeckCard { .. }
                    | RunAction::EnchantDeckCard { .. }
                    | RunAction::GrantDeckCard { .. }
                    | RunAction::PotionSwap { .. }
                    | RunAction::PickCardReward { .. }
                    | RunAction::PickRelicReward
            )
        );
        if !settle {
            break;
        }
        take(rm, actions[0]);
    }
}

/// The run standing in its first Blitz merchant.
fn at_merchant(character: CharacterId, seed: &str) -> RunManager {
    let mut rm = blitz(character, seed, 0);
    loop {
        to_map(&mut rm);
        if step(&mut rm) == RoomType::Shop {
            return rm;
        }
    }
}

/// `MerchantInventory.CreateForBlitzMerchant` / `GetBlitzEntries` / `Setup`
/// (`MerchantInventory.cs:132-165,209-235`): five distinct regular wares and
/// one cursed one, three picks, nothing else for sale and no gold spent.
#[test]
fn blitz_merchant_offers_five_wares_and_a_cursed_one() {
    for character in CHARACTERS {
        for seed in ["BLITZSHOP1", "BLITZSHOP2", "BLITZSHOP3"] {
            let rm = at_merchant(character, seed);
            assert_eq!(rm.phase(), Phase::Shop);
            assert!(rm.shop_inventory().is_none(), "no standard stock");
            let shop = rm.blitz_shop().expect("the Blitz merchant");
            assert_eq!(shop.choices_remaining, 3);
            let kinds: Vec<MerchantEntryType> = shop.entries.iter().map(|e| e.kind).collect();
            assert_eq!(kinds.len(), 6, "{character:?} {seed}");
            assert!(kinds[..5].iter().all(|k| !k.is_cursed()));
            assert!(kinds[5].is_cursed());
            let distinct: std::collections::BTreeSet<_> = kinds.iter().collect();
            assert_eq!(distinct.len(), 6);
            for e in &shop.entries {
                assert_eq!(e.status, EntryStatus::Available);
                assert_eq!(
                    e.character.is_some(),
                    e.kind == MerchantEntryType::PrismaticCardReward
                );
                if let Some(other) = e.character {
                    assert_ne!(other, character, "Prismatic names another character");
                }
                if e.kind != MerchantEntryType::CardEnchant {
                    assert!(e.enchantment.is_none());
                }
            }
            let mut actions = rm.get_available_actions();
            assert_eq!(actions.pop(), Some(RunAction::LeaveShop));
            assert_eq!(
                actions,
                (0..6)
                    .map(|idx| RunAction::BlitzChoice { idx })
                    .collect::<Vec<_>>()
            );
        }
    }
}

/// `MerchantInventory.UpdateEntries` (`:265-285`) through the run loop: each
/// taken ware spends a pick, the third disables the rest, and the merchant
/// can be left at any time.
#[test]
fn blitz_merchant_takes_three_wares_then_only_leaves() {
    let mut seen = std::collections::BTreeSet::new();
    for character in CHARACTERS {
        for n in 0..12 {
            let seed = format!("BLITZBUY{n}");
            // Rotate which wares are tried first so every slot gets taken.
            let mut rm = at_merchant(character, &seed);
            let gold = rm.run_state.gold;
            let mut order: Vec<usize> = (0..6).collect();
            order.rotate_left(n % 6);
            for idx in order {
                if !rm.blitz_shop().is_some_and(|s| s.can_take(idx)) {
                    continue;
                }
                let before = rm.blitz_shop().unwrap().clone();
                let kind = before.entries[idx].kind;
                take(&mut rm, RunAction::BlitzChoice { idx });
                // Resolve the ware's pick or reward screens; the room stays.
                for _ in 0..40 {
                    if rm.phase() == Phase::Shop
                        && matches!(
                            rm.get_available_actions().first(),
                            Some(RunAction::BlitzChoice { .. } | RunAction::LeaveShop)
                        )
                    {
                        break;
                    }
                    let actions = rm.get_available_actions();
                    take(&mut rm, actions[0]);
                }
                assert_eq!(rm.phase(), Phase::Shop, "{kind:?} returns to the merchant");
                let after = rm.blitz_shop().expect("still open");
                if after.entries[idx].status == EntryStatus::Used {
                    seen.insert(kind);
                    let bonus = if kind == MerchantEntryType::TwoMoreChoices {
                        2
                    } else {
                        0
                    };
                    assert_eq!(
                        after.choices_remaining,
                        before.choices_remaining - 1 + bonus,
                        "{kind:?}"
                    );
                    if kind.is_cursed() {
                        assert!(
                            rm.run_state
                                .master_deck
                                .iter()
                                .any(|c| c.card_type == sts2_core::cards::CardType::Curse),
                            "{kind:?} gives a curse"
                        );
                    }
                } else {
                    // Refused (nothing to act on): the ware and the pick stay.
                    assert_eq!(
                        after.entries[idx].status,
                        EntryStatus::Available,
                        "{kind:?}"
                    );
                    assert_eq!(after.choices_remaining, before.choices_remaining);
                }
            }
            let shop = rm.blitz_shop().unwrap();
            if shop.choices_remaining <= 0 {
                assert!(shop
                    .entries
                    .iter()
                    .all(|e| e.status != EntryStatus::Available));
                assert_eq!(rm.get_available_actions(), [RunAction::LeaveShop]);
            }
            assert_eq!(rm.run_state.gold, gold, "wares are free");
            take(&mut rm, RunAction::LeaveShop);
            assert!(rm.blitz_shop().is_none());
            assert_eq!(rm.phase(), Phase::MapChoice);
        }
    }
    // Every kind's draws are pinned one by one below; this walk only has to
    // reach most of them through the run loop.
    assert!(seen.len() >= 15, "ware kinds taken: {seen:?}");
}

/// A ware's deck pick is cancelable (`CardSelectorPrefs.Cancelable`): the
/// cancel leaves the ware available and the pick unspent.
#[test]
fn blitz_merchant_deck_pick_can_be_cancelled() {
    let pickers = [
        MerchantEntryType::CardUpgrade,
        MerchantEntryType::CardTransform,
        MerchantEntryType::CardRemoval,
    ];
    let mut cancelled = 0;
    for n in 0..40 {
        let mut rm = at_merchant(CharacterId::Ironclad, &format!("BLITZCANCEL{n}"));
        let shop = rm.blitz_shop().unwrap().clone();
        let Some(idx) = shop.entries.iter().position(|e| pickers.contains(&e.kind)) else {
            continue;
        };
        let deck: Vec<_> = rm.run_state.master_deck.clone();
        take(&mut rm, RunAction::BlitzChoice { idx });
        assert!(rm.blitz_pick_open());
        let actions = rm.get_available_actions();
        assert_eq!(actions.last(), Some(&RunAction::ConfirmDeckSelection));
        assert!(actions.len() > 1, "the pick itself is on offer");
        take(&mut rm, RunAction::ConfirmDeckSelection);
        assert!(!rm.blitz_pick_open());
        assert_eq!(rm.blitz_shop().unwrap(), &shop, "nothing spent");
        assert_eq!(rm.run_state.master_deck, deck);
        assert_eq!(rm.phase(), Phase::Shop);
        cancelled += 1;
    }
    assert!(cancelled > 5, "the seeds reach a picker ware ({cancelled})");
}

/// `CardModel.IsAllowedInBlitz` (`Debt.cs:15`, `HandOfGreed.cs:27`,
/// `MyShinies.cs:17`, `Royalties.cs:15`) through `CardFactory.FilterForRun`,
/// and `RelicLists.notAllowedInBlitz` through `RelicModel.IsAllowed`
/// (`RelicModel.cs:442-453`, `IsLateInAct3` = floor 21, `PotionBelt.cs:20-27`).
#[test]
fn blitz_bans_four_cards_and_44_relics() {
    use sts2_content::cards::spec_index::RunCardFilter;
    use sts2_core::runstate::relic_is_allowed_in;
    use sts2_core::RelicId;
    let b = blitz(CharacterId::Regent, "BLITZBAN", 0);
    let s = run(GameMode::Standard, CharacterId::Regent, "BLITZBAN", 0);
    for id in [
        CardId::Debt,
        CardId::HandOfGreed,
        CardId::MyShinies,
        CardId::RoyaltiesCard,
    ] {
        assert!(!RunCardFilter::of(&b.run_state).allows(id), "{id:?}");
        assert!(RunCardFilter::of(&s.run_state).allows(id), "{id:?}");
    }
    assert!(RunCardFilter::of(&b.run_state).allows(CardId::Greed));
    for &id in &sts2_core::blitz::RELICS_NOT_ALLOWED {
        assert!(!b.run_state.is_relic_allowed(id), "{id:?}");
    }
    assert!(s.run_state.is_relic_allowed(RelicId::WingedBoots));
    // Late in act 3: floor 21 in Blitz, 41 otherwise.
    for (mode, late) in [(GameMode::Blitz, 21), (GameMode::Standard, 41)] {
        assert!(relic_is_allowed_in(RelicId::FrozenEgg, late - 1, mode));
        assert!(!relic_is_allowed_in(RelicId::FrozenEgg, late, mode));
    }
    assert!(relic_is_allowed_in(
        RelicId::PotionBelt,
        20,
        GameMode::Blitz
    ));
    assert!(!relic_is_allowed_in(
        RelicId::PotionBelt,
        21,
        GameMode::Blitz
    ));
    assert!(relic_is_allowed_in(
        RelicId::PotionBelt,
        45,
        GameMode::Standard
    ));
}

/// `(shops, rewards, transformations)` stream read counts.
fn streams(rm: &RunManager) -> (u32, u32, u32) {
    let rng = &rm.run_state.rng;
    (
        rng.shops.counter(),
        rng.rewards.counter(),
        rng.transformations.counter(),
    )
}

/// The cards of every `CardReward` a ware queued, in offer order.
fn queued_card_rewards(rm: &RunManager) -> Vec<Vec<(CardId, bool)>> {
    rm.run_state
        .pending_rewards
        .iter()
        .filter_map(|r| match r {
            sts2_core::runstate::Reward::Card(c) => Some(
                c.options
                    .iter()
                    .copied()
                    .zip(c.upgraded.iter().copied())
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// Each ware's `OnTrySelect`, by the streams it reads and what it leaves on
/// the run (`Entities.Merchant/BlitzMerchant*Entry.cs`, v0.113.0). The run
/// stands in an act-1 merchant, where the act-scaled upgrade roll never
/// upgrades (`CardFactory.RollForUpgrade`: `0 + actIndex * scaling`), so an
/// upgraded card is one the ware upgraded.
#[test]
fn every_blitz_ware_reads_its_own_streams() {
    use sts2_content::blitz::merchant_select;
    use sts2_content::cards::spec_index::card_rarity_rank_csharp_in as rank;
    use sts2_core::blitz::{DeckPick, MerchantEntry, SelectOutcome};
    use MerchantEntryType as K;
    let fresh = || at_merchant(CharacterId::Ironclad, "BLITZWARES");
    let version = V113;
    // (ware, Shops reads, Rewards reads, outcome)
    let cases: [(K, u32, u32, SelectOutcome); 16] = [
        // No draw until the player has picked.
        (
            K::CardUpgrade,
            0,
            0,
            SelectOutcome::DeckPick(DeckPick::Upgrade),
        ),
        (
            K::CardTransform,
            0,
            0,
            SelectOutcome::DeckPick(DeckPick::Transform),
        ),
        (
            K::CardRemoval,
            0,
            0,
            SelectOutcome::DeckPick(DeckPick::Removal),
        ),
        (K::Glam, 0, 0, SelectOutcome::DeckPick(DeckPick::Enchant)),
        // 3 x (rarity, pick), `NoUpgrades`.
        (K::UpgradedCardReward, 6, 0, SelectOutcome::Taken),
        // One `StableShuffle` seed.
        (K::RandomUpgrades, 1, 0, SelectOutcome::Taken),
        // 2 rewards x 3 x (rarity, pick, upgrade).
        (K::CardRewards, 18, 0, SelectOutcome::Taken),
        (K::ColorlessCardReward, 9, 0, SelectOutcome::Taken),
        // 3 x (rarity, pick, upgrade) + one enchantment pick per card.
        (K::EnchantedCardReward, 12, 0, SelectOutcome::Taken),
        (K::RareCardReward, 9, 0, SelectOutcome::Taken),
        // Uniform + `NoUpgradeRoll`: one pick.
        (K::UpgradedRareCard, 1, 0, SelectOutcome::Taken),
        // `RollRarity(Shops)`.
        (K::Relic, 1, 0, SelectOutcome::Taken),
        // Cursed wares: the curse pick comes first.
        (K::TwoMoreChoices, 1, 0, SelectOutcome::Taken),
        // The three uniform rewards have no rng override: `PlayerRng.Rewards`.
        (K::CardRewardOfEachRarity, 1, 9, SelectOutcome::Taken),
        (K::UpgradedRareCardReward, 10, 0, SelectOutcome::Taken),
        (K::TwoRelics, 3, 0, SelectOutcome::Taken),
    ];
    for (kind, shops, rewards, outcome) in cases {
        let mut rm = fresh();
        let deck = rm.run_state.master_deck.clone();
        let relic_rewards = rm.run_state.relic_rewards.len();
        let before = streams(&rm);
        let got = merchant_select(&mut rm.run_state, &MerchantEntry::new(kind));
        let after = streams(&rm);
        assert_eq!(got, outcome, "{kind:?}");
        assert_eq!(
            (after.0 - before.0, after.1 - before.1, after.2 - before.2),
            (shops, rewards, 0),
            "{kind:?} (shops, rewards, transformations)"
        );
        let offered = queued_card_rewards(&rm);
        let ranks = |cards: &[(CardId, bool)]| -> Vec<u8> {
            cards.iter().map(|&(id, _)| rank(id, version)).collect()
        };
        let curses = |rm: &RunManager| {
            rm.run_state
                .master_deck
                .iter()
                .filter(|c| c.card_type == sts2_core::cards::CardType::Curse)
                .count()
        };
        let cursed_now = curses(&rm)
            - deck
                .iter()
                .filter(|c| c.card_type == sts2_core::cards::CardType::Curse)
                .count();
        assert_eq!(
            cursed_now,
            usize::from(kind.is_cursed() && kind != K::Glam),
            "{kind:?}: a cursed ware gives its curse first (Glam after its pick)"
        );
        match kind {
            K::UpgradedCardReward => {
                assert_eq!(offered.len(), 1);
                assert_eq!(offered[0].len(), 3);
                assert!(offered[0].iter().all(|&(_, up)| up), "all upgraded");
            }
            K::CardRewards => {
                assert_eq!(offered.iter().map(Vec::len).collect::<Vec<_>>(), [3, 3]);
                assert!(offered.iter().flatten().all(|&(_, up)| !up));
            }
            K::ColorlessCardReward => {
                assert_eq!(offered.len(), 1);
                for &(id, _) in &offered[0] {
                    assert!(
                        sts2_content::cards::spec_index::character_for_card_id(id).is_none(),
                        "{id:?} is colorless"
                    );
                }
            }
            K::EnchantedCardReward => {
                let reward = rm
                    .run_state
                    .pending_rewards
                    .iter()
                    .find_map(|r| match r {
                        sts2_core::runstate::Reward::Card(c) => Some(c.clone()),
                        _ => None,
                    })
                    .expect("a card reward");
                assert_eq!(reward.enchantments.len(), 3);
                assert!(reward.enchantments.iter().all(|e| !e.is_empty()));
            }
            K::RareCardReward => assert_eq!(ranks(&offered[0]), [4, 4, 4]),
            K::UpgradedRareCardReward => {
                assert_eq!(ranks(&offered[0]), [4, 4, 4]);
                assert!(offered[0].iter().all(|&(_, up)| up));
            }
            K::CardRewardOfEachRarity => {
                let by_reward: Vec<Vec<u8>> = offered.iter().map(|r| ranks(r)).collect();
                assert_eq!(by_reward, [[2, 2, 2], [3, 3, 3], [4, 4, 4]]);
            }
            K::UpgradedRareCard => {
                assert!(offered.is_empty(), "no screen");
                assert_eq!(rm.run_state.master_deck.len(), deck.len() + 1);
                let card = rm.run_state.master_deck.last().unwrap();
                assert_eq!(rank(card.card_id, version), 4);
                assert!(card.upgraded);
            }
            K::RandomUpgrades => {
                let upgraded =
                    |d: &[sts2_core::cards::Card]| d.iter().filter(|c| c.upgraded).count();
                assert_eq!(upgraded(&rm.run_state.master_deck), upgraded(&deck) + 2);
            }
            K::Relic => assert_eq!(rm.run_state.relic_rewards.len(), relic_rewards + 1),
            K::TwoRelics => assert_eq!(rm.run_state.relic_rewards.len(), relic_rewards + 2),
            K::CardUpgrade => assert_eq!(rm.run_state.pending_deck_upgrades, 1),
            K::CardRemoval => assert_eq!(rm.run_state.pending_deck_removals, 1),
            K::CardTransform => assert_eq!(rm.run_state.pending_deck_transforms.len(), 1),
            K::Glam => {
                let pick = rm.run_state.pending_deck_enchants.first().expect("a pick");
                assert_eq!(
                    (pick.enchantment, pick.amount),
                    (sts2_core::cards::EnchantmentId::Glam, 1)
                );
            }
            _ => {}
        }
    }

    // FullHealAndMaxHp: the curse, +6 max HP, then full HP.
    let mut rm = fresh();
    rm.run_state.current_hp = 20;
    let (max_hp, before) = (rm.run_state.max_hp, streams(&rm));
    assert_eq!(
        merchant_select(&mut rm.run_state, &MerchantEntry::new(K::FullHealAndMaxHp)),
        SelectOutcome::Taken
    );
    assert_eq!(streams(&rm).0 - before.0, 1);
    assert_eq!(
        (rm.run_state.max_hp, rm.run_state.current_hp),
        (max_hp + 6, max_hp + 6)
    );

    // Potions: one more slot, two potions, all on Shops.
    let mut rm = fresh();
    rm.run_state.potions.clear();
    let (slots, before) = (rm.run_state.max_potion_slots, streams(&rm));
    assert_eq!(
        merchant_select(&mut rm.run_state, &MerchantEntry::new(K::Potions)),
        SelectOutcome::Taken
    );
    let after = streams(&rm);
    assert!(after.0 > before.0);
    assert_eq!((after.1, after.2), (before.1, before.2));
    assert_eq!(rm.run_state.max_potion_slots, slots + 1);
    assert_eq!(rm.run_state.potions.len(), 2);

    // PrismaticCardReward: the constructor's character, then its pool.
    let mut rm = fresh();
    let mut entry = MerchantEntry::new(K::PrismaticCardReward);
    entry.character = Some(CharacterId::Silent);
    let before = streams(&rm);
    assert_eq!(
        merchant_select(&mut rm.run_state, &entry),
        SelectOutcome::Taken
    );
    assert_eq!(streams(&rm).0 - before.0, 9);
    for &(id, _) in &queued_card_rewards(&rm)[0] {
        assert_eq!(
            sts2_content::cards::spec_index::character_for_card_id(id),
            Some(CharacterId::Silent),
            "{id:?}"
        );
    }

    // CardEnchant: nothing rolled at purchase; the constructor's pick is what
    // the deck pick stamps. With no enchantment (no option fits the deck)
    // the ware cannot be taken.
    let mut rm = fresh();
    let mut entry = MerchantEntry::new(K::CardEnchant);
    assert_eq!(
        merchant_select(&mut rm.run_state, &entry),
        SelectOutcome::Refused
    );
    entry.enchantment = Some((sts2_core::cards::EnchantmentId::Sharp, 2));
    let before = streams(&rm);
    assert_eq!(
        merchant_select(&mut rm.run_state, &entry),
        SelectOutcome::DeckPick(DeckPick::Enchant)
    );
    assert_eq!(streams(&rm), before);
    let pick = rm.run_state.pending_deck_enchants.first().expect("a pick");
    assert_eq!(
        (pick.enchantment, pick.amount, pick.type_restriction),
        (
            sts2_core::cards::EnchantmentId::Sharp,
            2,
            Some(sts2_core::cards::CardType::Attack)
        )
    );

    // Glam's curse comes once its card is picked (`GiveCurse` before
    // `CardCmd.Enchant`): one Shops read, a curse at the deck tail. Debt is
    // not allowed in Blitz, so the pool holds nine curses.
    let mut rm = fresh();
    let before = streams(&rm);
    sts2_content::blitz::merchant_after_pick(&mut rm.run_state, &MerchantEntry::new(K::Glam));
    assert_eq!(streams(&rm).0 - before.0, 1);
    let curse = rm.run_state.master_deck.last().unwrap();
    assert_eq!(curse.card_type, sts2_core::cards::CardType::Curse);
    assert_ne!(curse.card_id, CardId::Debt);
}

/// `RandomUpgrades` with nothing upgradable (`BlitzMerchantRandomCardUpgradesEntry.
/// cs:21`): `StableShuffle` still seeds its child `Rng` off Shops, then the
/// purchase fails (`FailureForbidden`) and nothing is spent.
#[test]
fn blitz_random_upgrades_with_nothing_to_upgrade_draws_and_refuses() {
    use sts2_content::blitz::merchant_select;
    use sts2_core::blitz::{MerchantEntry, SelectOutcome};
    let mut rm = at_merchant(CharacterId::Ironclad, "BLITZWARES");
    let version = rm.run_state.game_version;
    for card in &mut rm.run_state.master_deck {
        if card.is_upgradable() {
            card.upgrade_for(version);
        }
    }
    let deck = rm.run_state.master_deck.clone();
    let before = streams(&rm);
    let entry = MerchantEntry::new(MerchantEntryType::RandomUpgrades);
    assert_eq!(
        merchant_select(&mut rm.run_state, &entry),
        SelectOutcome::Refused
    );
    assert_eq!(streams(&rm).0 - before.0, 1, "the shuffle's draw");
    assert_eq!(rm.run_state.master_deck, deck);
}

/// `MerchantInventory.GetBlitzEntries` (`:209-235`) with its constructors:
/// six type draws on Shops, plus one for a CardEnchant ware
/// (`BlitzMerchantCardEnchantEntry.cs:54-60`) and one for a Prismatic ware
/// (`BlitzMerchantPrismaticCardRewardEntry.cs:41-49`); no other stream moves.
#[test]
fn blitz_merchant_generation_draws_on_shops_only() {
    for n in 0..40 {
        let mut rm = blitz(CharacterId::Necrobinder, &format!("BLITZGEN{n}"), 0);
        loop {
            to_map(&mut rm);
            let before = streams(&rm);
            if step(&mut rm) != RoomType::Shop {
                continue;
            }
            let after = streams(&rm);
            let shop = rm.blitz_shop().expect("the merchant");
            let extra = shop
                .entries
                .iter()
                .filter(|e| {
                    matches!(
                        e.kind,
                        MerchantEntryType::CardEnchant | MerchantEntryType::PrismaticCardReward
                    )
                })
                .count() as u32;
            assert_eq!(after.0 - before.0, 6 + extra, "seed {n}: {shop:?}");
            assert_eq!((after.1, after.2), (before.1, before.2));
            for e in &shop.entries {
                if e.kind == MerchantEntryType::CardEnchant {
                    let (id, amount) = e.enchantment.expect("a starter deck can be enchanted");
                    assert!(sts2_content::blitz::SHOP_ENCHANTMENT_OPTIONS.contains(&(id, amount)));
                }
            }
            break;
        }
    }
}

/// `RandomUpgrades` picks through `StableShuffle`, whose `List.Sort()` uses
/// the whole `CardModel.CompareTo` key (`CardModel.cs:2473-2526`): model id,
/// upgrade level, then enchantment and more. So which Strike is upgraded
/// depends on whether it carries Sharp, never on where it sits in the deck.
/// (Sorting by id alone left same-id cards in deck order, and Ironclad:1000
/// at Blitz 5 upgraded the Sharp Strike where C# upgraded a plain one.)
#[test]
fn blitz_random_upgrades_sort_by_the_full_card_key() {
    use sts2_content::blitz::merchant_select;
    use sts2_core::blitz::{MerchantEntry, SelectOutcome};
    use sts2_core::cards::EnchantmentId;
    let entry = MerchantEntry::new(MerchantEntryType::RandomUpgrades);
    // The deck's upgradable cards: three plain Strikes and one Sharp Strike,
    // the Sharp one first or last.
    let sharp_upgraded = |n: usize, sharp_first: bool| -> bool {
        let mut rm = at_merchant(CharacterId::Ironclad, &format!("BLITZSORT{n}"));
        let version = rm.run_state.game_version;
        let strike = *rm
            .run_state
            .master_deck
            .iter()
            .find(|c| c.card_id == CardId::StrikeIronclad)
            .expect("a starter Strike");
        let mut sharp = strike;
        sharp.set_enchantment(EnchantmentId::Sharp, 2);
        let mut deck = vec![strike, strike, strike];
        deck.insert(if sharp_first { 0 } else { 3 }, sharp);
        rm.run_state.master_deck = deck;
        assert_eq!(
            merchant_select(&mut rm.run_state, &entry),
            SelectOutcome::Taken
        );
        let _ = version;
        let upgraded: Vec<bool> = rm
            .run_state
            .master_deck
            .iter()
            .filter(|c| c.upgraded)
            .map(|c| c.has_enchantment(EnchantmentId::Sharp))
            .collect();
        assert_eq!(upgraded.len(), 2, "two cards upgraded");
        upgraded.contains(&true)
    };
    let mut outcomes = std::collections::BTreeSet::new();
    for n in 0..24 {
        let first = sharp_upgraded(n, true);
        assert_eq!(
            first,
            sharp_upgraded(n, false),
            "seed {n}: deck order decided"
        );
        outcomes.insert(first);
    }
    assert_eq!(
        outcomes.len(),
        2,
        "the Sharp Strike is upgraded on some seeds only"
    );
}
