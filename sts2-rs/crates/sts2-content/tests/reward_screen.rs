//! `RunState::reward_screen` (GH #1620): the game's reward screen. A won
//! combat room's rewards and the treasure chest's relic are taken or skipped
//! one by one, in any order, and proceeding leaves the rest.
use sts2_content::run_callbacks::{
    new_run_manager_for_character_from_string_seed,
    new_run_manager_for_character_from_string_seed_for,
};
use sts2_core::cards::{Card, CardId};
use sts2_core::potions::PotionKind;
use sts2_core::run_manager::{ActionResult, Phase, RunAction, RunManager};
use sts2_core::runstate::{CardReward, ScreenReward};
use sts2_core::{CharacterId, GameVersion, RelicId};

fn keep_alive(rm: &mut RunManager) {
    if !rm.is_over() {
        rm.debug_set_player(Some(999), Some(999), None);
    }
}

/// The walk's move: an event's last option (usually leave: some first options
/// loop), else the first action (the screen's first take).
fn pick(rm: &RunManager) -> Option<RunAction> {
    let actions = rm.get_available_actions();
    if rm.phase() == Phase::Event {
        if let Some(last) = actions
            .iter()
            .rev()
            .find(|a| matches!(a, RunAction::EventChoice { .. }))
        {
            return Some(*last);
        }
    }
    actions.first().copied()
}

fn agent_run(seed: &str, screen: bool) -> RunManager {
    agent_run_for(None, seed, screen)
}

fn agent_run_for(version: Option<GameVersion>, seed: &str, screen: bool) -> RunManager {
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();
    let mut rm = match version {
        None => new_run_manager_for_character_from_string_seed(seed, CharacterId::Ironclad),
        Some(v) => {
            new_run_manager_for_character_from_string_seed_for(v, seed, CharacterId::Ironclad)
        }
    };
    sts2_content::run_callbacks::initialize_run_setup(&mut rm);
    rm.run_state.set_agent_decisions();
    rm.run_state.interactive_potion_swap = true;
    rm.run_state.reward_screen = screen;
    rm
}

/// What the player holds, read at every map decision.
#[derive(Debug, PartialEq)]
struct Holdings {
    floor: i32,
    gold: i32,
    hp: i32,
    relics: Vec<RelicId>,
    potions: Vec<PotionKind>,
    deck: Vec<(CardId, bool)>,
}

fn holdings(rm: &RunManager) -> Holdings {
    Holdings {
        floor: rm.run_state.total_floor,
        gold: rm.run_state.gold,
        hp: rm.run_state.current_hp,
        relics: rm.run_state.relics.clone(),
        potions: rm.run_state.potions.clone(),
        deck: rm
            .run_state
            .master_deck
            .iter()
            .map(|c| (c.card_id, c.upgraded))
            .collect(),
    }
}

/// Play the first offered action `steps` times (the screen lists its takes
/// first, so this takes every reward in the set's order), keeping the run
/// alive; the holdings at each map decision, and how many screens opened.
fn first_action_walk(seed: &str, screen: bool, steps: usize) -> (Vec<Holdings>, usize) {
    let mut rm = agent_run(seed, screen);
    let mut out = Vec::new();
    let mut screens = 0;
    let mut last_floor = -1;
    for _ in 0..steps {
        if rm.is_over() {
            break;
        }
        keep_alive(&mut rm);
        if !screen {
            assert_ne!(
                rm.phase(),
                Phase::Rewards,
                "parity flow opened a reward screen"
            );
            assert!(rm.reward_screen().is_none());
        }
        // Once per floor: a chest opened by the move itself leaves an
        // auto-collect run on the map, while the screen run passes through
        // its reward screen.
        if rm.phase() == Phase::MapChoice && rm.run_state.total_floor != last_floor {
            last_floor = rm.run_state.total_floor;
            out.push(holdings(&rm));
        }
        if rm.phase() == Phase::Rewards && rm.reward_screen().is_some() {
            let listed = rm.reward_screen().map_or(0, <[_]>::len);
            let actions = rm.get_available_actions();
            if actions.first() == Some(&RunAction::TakeReward { idx: 0 }) {
                screens += 1;
            }
            assert_eq!(actions.len(), 2 * listed + 1, "{actions:?}");
        }
        let Some(first) = pick(&rm) else {
            break;
        };
        let _ = rm.take_action(first);
    }
    (out, screens)
}

#[test]
fn taking_every_reward_in_order_matches_auto_collect() {
    for seed in ["REWARDSCREEN1", "REWARDSCREEN2", "REWARDSCREEN3"] {
        let (auto, none) = first_action_walk(seed, false, 20_000);
        let (screen, screens) = first_action_walk(seed, true, 20_000);
        assert_eq!(none, 0, "{seed}: no screen in the parity flow");
        assert!(screens >= 5, "{seed}: only {screens} screens");
        assert!(
            auto.len() >= 10,
            "{seed}: only {} map decisions",
            auto.len()
        );
        assert_eq!(auto, screen, "{seed}");
    }
}

/// Play first actions up to the first reward screen of a combat room.
fn to_first_screen(seed: &str) -> RunManager {
    let mut rm = agent_run(seed, true);
    for _ in 0..2000 {
        keep_alive(&mut rm);
        if rm.phase() == Phase::Rewards {
            return rm;
        }
        let first = pick(&rm).expect("an action");
        let _ = rm.take_action(first);
    }
    panic!("no reward screen");
}

fn listed(rm: &RunManager) -> Vec<ScreenReward> {
    rm.reward_screen()
        .expect("screen open")
        .iter()
        .map(|e| e.reward.clone())
        .collect()
}

fn index_of(rm: &RunManager, f: impl Fn(&ScreenReward) -> bool) -> usize {
    rm.reward_screen()
        .expect("screen open")
        .iter()
        .find(|e| f(&e.reward))
        .expect("listed")
        .index
}

#[test]
fn a_combat_rooms_rewards_can_be_left_and_taken_in_any_order() {
    let mut rm = to_first_screen("REWARDSCREEN1");
    let rewards = listed(&rm);
    assert!(
        matches!(rewards.first(), Some(ScreenReward::Gold(_))),
        "{rewards:?}"
    );
    assert!(
        matches!(rewards.last(), Some(ScreenReward::Card(_))),
        "{rewards:?}"
    );
    let gold_before = rm.run_state.gold;
    let deck_before = rm.run_state.master_deck.len();
    // The card reward first: its pick, then back to the screen.
    let card = index_of(&rm, |r| matches!(r, ScreenReward::Card(_)));
    assert_eq!(
        rm.take_action(RunAction::TakeReward { idx: card }),
        ActionResult::Advanced
    );
    assert_eq!(rm.phase(), Phase::CardReward);
    assert_eq!(rm.card_reward_options().len(), 3);
    rm.take_action(RunAction::PickCardReward { idx: Some(1) });
    assert_eq!(rm.run_state.master_deck.len(), deck_before + 1);
    assert_eq!(rm.phase(), Phase::Rewards, "back on the screen");
    assert!(!rm
        .get_available_actions()
        .contains(&RunAction::TakeReward { idx: card }));
    // Skip the gold: it is never gained.
    let gold = index_of(&rm, |r| matches!(r, ScreenReward::Gold(_)));
    rm.take_action(RunAction::SkipReward { idx: gold });
    assert_eq!(rm.run_state.gold, gold_before);
    // Leave whatever is left.
    let potions = rm.run_state.potions.clone();
    if rm.phase() == Phase::Rewards {
        assert_eq!(
            rm.take_action(RunAction::ProceedRewards),
            ActionResult::Advanced
        );
    }
    assert_eq!(rm.run_state.potions, potions, "a left potion is not taken");
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert!(rm.reward_screen().is_none());
}

#[test]
fn a_taken_gold_reward_pays_and_an_index_is_stable() {
    let mut rm = to_first_screen("REWARDSCREEN2");
    let Some(ScreenReward::Gold(amount)) = listed(&rm).first().cloned() else {
        panic!("gold first");
    };
    let last = rm.reward_screen().unwrap().last().unwrap().index;
    let before = rm.run_state.gold;
    rm.take_action(RunAction::TakeReward { idx: 0 });
    assert_eq!(rm.run_state.gold, before + amount);
    assert_eq!(
        rm.take_action(RunAction::TakeReward { idx: 0 }),
        ActionResult::InvalidForPhase,
        "taken"
    );
    assert!(rm
        .get_available_actions()
        .contains(&RunAction::TakeReward { idx: last }));
}

#[test]
fn a_skipped_relic_is_not_obtained_and_a_taken_one_is() {
    let mut rm = agent_run("REWARDSCREEN1", true);
    rm.debug_open_reward_screen(vec![
        ScreenReward::Relic {
            relic: RelicId::Anchor,
            is_wax: false,
        },
        ScreenReward::Relic {
            relic: RelicId::Vajra,
            is_wax: true,
        },
    ]);
    rm.take_action(RunAction::SkipReward { idx: 0 });
    assert!(!rm.run_state.relics.contains(&RelicId::Anchor));
    assert_eq!(rm.phase(), Phase::Rewards);
    rm.take_action(RunAction::TakeReward { idx: 1 });
    assert!(rm.run_state.relics.contains(&RelicId::Vajra));
    assert!(rm.run_state.wax_relics.contains(&RelicId::Vajra));
    assert!(rm.reward_screen().is_none(), "the last reward closes it");
    assert_eq!(rm.phase(), Phase::MapChoice);
}

/// A card reward still on the screen hears the taken egg
/// (`CardReward.OnRelicObtained`); one already taken does not.
#[test]
fn a_taken_egg_upgrades_the_card_reward_still_listed() {
    let mut rm = agent_run("REWARDSCREEN1", true);
    rm.debug_open_reward_screen(vec![
        ScreenReward::Relic {
            relic: RelicId::MoltenEgg,
            is_wax: false,
        },
        ScreenReward::Card(CardReward {
            options: vec![CardId::PommelStrike, CardId::ShrugItOff, CardId::Anger],
            ..CardReward::empty()
        }),
    ]);
    rm.take_action(RunAction::TakeReward { idx: 0 });
    let Some(ScreenReward::Card(reward)) = listed(&rm).first().cloned() else {
        panic!("card listed");
    };
    assert_eq!(reward.upgraded, vec![true, false, true]);
}

#[test]
fn a_full_belt_parks_the_potion_swap() {
    let mut rm = agent_run("REWARDSCREEN1", true);
    let cap = rm.run_state.potion_belt_cap();
    rm.run_state.potions = vec![PotionKind::FirePotion; cap];
    rm.run_state.potion_slot_indices.clear();
    rm.debug_open_reward_screen(vec![
        ScreenReward::Potion(PotionKind::BlockPotion),
        ScreenReward::Gold(10),
    ]);
    rm.take_action(RunAction::TakeReward { idx: 0 });
    assert_eq!(
        rm.get_available_actions().first(),
        Some(&RunAction::PotionSwap { discard_slot: None })
    );
    rm.take_action(RunAction::PotionSwap {
        discard_slot: Some(0),
    });
    assert!(rm.run_state.potions.contains(&PotionKind::BlockPotion));
    assert_eq!(rm.phase(), Phase::Rewards, "the gold is still there");
}

#[test]
fn a_special_card_and_a_card_removal() {
    let mut rm = agent_run("REWARDSCREEN1", true);
    let deck = rm.run_state.master_deck.len();
    let card = Card {
        card_id: CardId::Bash,
        ..Card::pod_defaults()
    };
    rm.debug_open_reward_screen(vec![
        ScreenReward::SpecialCard(card),
        ScreenReward::CardRemoval(1),
    ]);
    rm.take_action(RunAction::TakeReward { idx: 0 });
    assert_eq!(rm.run_state.master_deck.len(), deck + 1);
    rm.take_action(RunAction::TakeReward { idx: 1 });
    assert!(matches!(
        rm.get_available_actions().first(),
        Some(RunAction::RemoveDeckCard { .. })
    ));
    rm.take_action(RunAction::RemoveDeckCard { idx: 0 });
    assert_eq!(rm.run_state.master_deck.len(), deck);
}

/// The chest's relic: the gold is paid on opening, the relic waits with a
/// skip (`NTreasureRoom`), and the same seed takes the same relic.
fn treasure_cols(rm: &RunManager) -> Vec<i32> {
    let Some(map) = rm.run_state.current_act_map.as_ref() else {
        return Vec::new();
    };
    let children = match rm.run_state.current_room.and_then(|r| r.map_coord) {
        Some(coord) => map.children_of(coord),
        None => match map.start {
            Some(s) => map.children_of(map.point(s).coord),
            None => Vec::new(),
        },
    };
    children
        .iter()
        .filter(|p| p.point_type == sts2_core::map::MapPointType::Treasure)
        .map(|p| p.coord.col)
        .collect()
}

fn to_chest(version: Option<GameVersion>, screen: bool) -> Option<RunManager> {
    let mut rm = agent_run_for(version, "REWARDSCREEN3", screen);
    for _ in 0..3000 {
        keep_alive(&mut rm);
        if rm.is_over() {
            return None;
        }
        // Walk to a treasure node when one is offered.
        let actions = rm.get_available_actions();
        if rm.phase() == Phase::MapChoice {
            if let Some(&mv) = actions.iter().find(|a| {
                let RunAction::Move { col } = a else {
                    return false;
                };
                treasure_cols(&rm).contains(col)
            }) {
                let gold = rm.run_state.gold;
                rm.take_action(mv);
                assert!(
                    rm.run_state.gold > gold,
                    "the chest's gold is paid on opening"
                );
                return Some(rm);
            }
        }
        let _ = rm.take_action(pick(&rm).expect("an action"));
    }
    None
}

#[test]
fn the_chest_relic_can_be_skipped() {
    let auto = to_chest(None, false).expect("a treasure room");
    let mut screen = to_chest(None, true).expect("a treasure room");
    assert_eq!(auto.run_state.gold, screen.run_state.gold);
    assert_eq!(screen.phase(), Phase::Rewards);
    let [ScreenReward::ChestRelic(relic)] = listed(&screen)[..] else {
        panic!("{:?}", listed(&screen));
    };
    assert_eq!(auto.run_state.relics.last(), Some(&relic));
    let relics = screen.run_state.relics.clone();
    screen.take_action(RunAction::SkipReward { idx: 0 });
    assert_eq!(screen.run_state.relics, relics, "skipped");
    assert_eq!(screen.phase(), Phase::MapChoice);
    assert_eq!(auto.phase(), Phase::MapChoice);
}

/// v0.113.0 draws the treasure-room stream once when the relic is awarded
/// (`TreasureRoomRelicSynchronizer.AwardRelics`, `list.StableShuffle(_rng)`
/// on a child Rng). A single player's skip returns before it (`OnPicked`:
/// `_singleplayerSkipped = true; return;`), so a skipped chest costs no draw
/// and the next chest rolls its rarity one draw earlier.
#[test]
fn a_skipped_chest_costs_no_treasure_draw_on_v0_113() {
    let v = Some(GameVersion::V0_113_0);
    let auto = to_chest(v, false).expect("a treasure room");
    let screen = to_chest(v, true).expect("a treasure room");
    let opened = screen.run_state.rng.treasure_room.counter();
    assert_eq!(auto.run_state.rng.treasure_room.counter(), opened + 1);
    let mut taken = screen.clone();
    taken.take_action(RunAction::TakeReward { idx: 0 });
    assert_eq!(taken.run_state.rng.treasure_room.counter(), opened + 1);
    let mut skipped = screen.clone();
    skipped.take_action(RunAction::SkipReward { idx: 0 });
    assert_eq!(skipped.run_state.rng.treasure_room.counter(), opened);
    let mut left = screen;
    left.take_action(RunAction::ProceedRewards);
    assert_eq!(left.run_state.rng.treasure_room.counter(), opened);
}

// --------------------------------------------------------------------------
// GH #1641: an event's or relic's own reward set (`RewardsCmd.OfferCustom`)
// opens the screen too.
// --------------------------------------------------------------------------

/// A run at its first map decision, with or without the screen.
fn at_map(screen: bool) -> RunManager {
    let mut rm = agent_run("REWARDSCREEN1", screen);
    for _ in 0..200 {
        if rm.phase() == Phase::MapChoice {
            return rm;
        }
        let first = pick(&rm).expect("an action");
        let _ = rm.take_action(first);
    }
    panic!("never reached the map");
}

/// Whispering Hollow's Gold option (`OfferCustom` with two `PotionReward`s).
fn whispering_gold(screen: bool) -> RunManager {
    let mut rm = at_map(screen);
    rm.run_state.gold = 200;
    rm.run_state.potions.clear();
    rm.debug_enter_event(
        sts2_core::events::EventId::WhisperingHollow,
        sts2_content::events::dispatch::begin_event_in_entered_room,
    );
    assert_eq!(rm.phase(), Phase::Event);
    assert_eq!(
        rm.take_action(RunAction::EventChoice { idx: 0 }),
        ActionResult::Advanced
    );
    rm
}

#[test]
fn an_events_reward_set_waits_on_the_screen() {
    let auto = whispering_gold(false);
    assert_eq!(auto.run_state.potions.len(), 2, "auto-collect takes both");
    assert_ne!(auto.phase(), Phase::Rewards);

    let mut rm = whispering_gold(true);
    assert_eq!(rm.phase(), Phase::Rewards);
    assert!(rm.run_state.potions.is_empty(), "nothing taken yet");
    let potions: Vec<PotionKind> = listed(&rm)
        .iter()
        .map(|r| match r {
            ScreenReward::Potion(k) => *k,
            other => panic!("not a potion: {other:?}"),
        })
        .collect();
    assert_eq!(potions, auto.run_state.potions, "the same rolls, in order");
    rm.take_action(RunAction::SkipReward { idx: 0 });
    assert_eq!(rm.phase(), Phase::Rewards);
    rm.take_action(RunAction::TakeReward { idx: 1 });
    assert_eq!(rm.run_state.potions, vec![potions[1]]);
    assert!(rm.reward_screen().is_none());
    assert_eq!(rm.phase(), Phase::MapChoice);
}

#[test]
fn proceeding_off_an_events_screen_leaves_the_rest() {
    let mut rm = whispering_gold(true);
    rm.take_action(RunAction::ProceedRewards);
    assert!(rm.run_state.potions.is_empty());
    assert_eq!(rm.phase(), Phase::MapChoice);
}

#[test]
fn a_relic_set_taken_off_a_screen_opens_on_top_of_it() {
    let mut rm = agent_run("REWARDSCREEN1", true);
    rm.run_state.potions.clear();
    rm.debug_open_reward_screen(vec![
        ScreenReward::Gold(10),
        ScreenReward::Relic {
            relic: RelicId::LostCoffer,
            is_wax: false,
        },
    ]);
    let gold = rm.run_state.gold;
    // Lost Coffer's `AfterObtained`: `OfferCustom([CardReward, PotionReward])`.
    rm.take_action(RunAction::TakeReward { idx: 1 });
    assert!(rm.run_state.relics.contains(&RelicId::LostCoffer));
    assert_eq!(rm.phase(), Phase::Rewards);
    let inner = listed(&rm);
    assert!(
        matches!(
            inner.as_slice(),
            [ScreenReward::Potion(_), ScreenReward::Card(_)]
        ),
        "{inner:?}"
    );
    rm.take_action(RunAction::SkipReward { idx: 0 });
    assert!(rm.run_state.potions.is_empty(), "skipped");
    let deck = rm.run_state.master_deck.len();
    rm.take_action(RunAction::TakeReward { idx: 1 });
    assert_eq!(rm.phase(), Phase::CardReward, "the card pick opens");
    rm.take_action(RunAction::PickCardReward { idx: Some(0) });
    assert_eq!(rm.run_state.master_deck.len(), deck + 1);
    // The coffer's screen is done: the one under it is back.
    assert_eq!(rm.phase(), Phase::Rewards);
    assert_eq!(listed(&rm), vec![ScreenReward::Gold(10)]);
    rm.take_action(RunAction::TakeReward { idx: 0 });
    assert_eq!(rm.run_state.gold, gold + 10);
    assert_eq!(rm.phase(), Phase::MapChoice);
}

#[test]
fn a_relics_set_is_granted_without_the_screen() {
    let mut rm = agent_run("REWARDSCREEN1", false);
    rm.run_state.potions.clear();
    // The parity flow: Lost Coffer's potion lands on the belt, its card
    // opens the pick.
    let relics = rm.run_state.relics.len();
    rm.run_state.offer_relic_rewards(RelicId::LostCoffer, 1);
    let _ = rm.run_state.drain_relic_rewards_directly();
    assert_eq!(rm.run_state.relics.len(), relics + 1);
    assert_ne!(rm.phase(), Phase::Rewards);
    assert!(rm.reward_screen().is_none());
}

/// Neow's Bones offers its two relics with `WithSkippingDisallowed`: takes
/// only, and its curse lands once both are in.
#[test]
fn neows_bones_set_must_be_taken_whole() {
    let mut rm = agent_run("REWARDSCREEN1", true);
    rm.debug_open_reward_screen(vec![ScreenReward::Relic {
        relic: RelicId::NeowsBones,
        is_wax: false,
    }]);
    let deck = rm.run_state.master_deck.len();
    rm.take_action(RunAction::TakeReward { idx: 0 });
    assert_eq!(rm.phase(), Phase::Rewards);
    let relics: Vec<RelicId> = listed(&rm)
        .iter()
        .map(|r| match r {
            ScreenReward::Relic { relic, .. } => *relic,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(relics.len(), 2);
    assert_eq!(
        rm.get_available_actions(),
        vec![
            RunAction::TakeReward { idx: 0 },
            RunAction::TakeReward { idx: 1 }
        ],
        "no skip, no proceed"
    );
    assert_eq!(
        rm.take_action(RunAction::ProceedRewards),
        ActionResult::InvalidForPhase
    );
    // Walk on: take every reward and resolve whatever the relics open.
    for _ in 0..50 {
        if rm.phase() == Phase::MapChoice {
            break;
        }
        keep_alive(&mut rm);
        let first = pick(&rm).expect("an action");
        let _ = rm.take_action(first);
    }
    assert_eq!(rm.phase(), Phase::MapChoice);
    for r in &relics {
        assert!(rm.run_state.relics.contains(r), "{r:?}");
    }
    let curses = rm
        .run_state
        .master_deck
        .iter()
        .skip(deck)
        .filter(|c| {
            [
                CardId::Clumsy,
                CardId::Debt,
                CardId::Decay,
                CardId::Doubt,
                CardId::Guilty,
                CardId::Injury,
                CardId::Normality,
                CardId::Regret,
                CardId::Shame,
                CardId::Writhe,
            ]
            .contains(&c.card_id)
        })
        .count();
    assert_eq!(curses, 1, "the curse lands after the set");
}
