//! The Fake Merchant as a shop page (GH #1607): C# `FakeMerchant` has no event
//! options; `NFakeMerchant` shows a merchant inventory of six fake relics at
//! their rolled prices, any of which can be bought while gold lasts, a Foul
//! Potion thrown at it starts the fight with the unsold stock as rewards, and
//! Proceed leaves.
use sts2_content::events::dispatch::begin_event_in_entered_room;
use sts2_content::run_callbacks::new_run_manager_for_character_from_string_seed;
use sts2_core::events::{EventEncounter, EventId};
use sts2_core::potions::PotionKind;
use sts2_core::run_manager::{ActionResult, Phase, RunAction, RunManager};
use sts2_core::{CharacterId, RelicId, Reward};

fn run_at_map(seed: &str, agent: bool) -> RunManager {
    sts2_content::potions::dispatch::ensure_installed();
    let mut rm = new_run_manager_for_character_from_string_seed(seed, CharacterId::Ironclad);
    if agent {
        rm.run_state.set_agent_decisions();
    }
    for _ in 0..200 {
        if rm.phase() == Phase::MapChoice {
            return rm;
        }
        let first = rm.get_available_actions()[0];
        let _ = rm.take_action(first);
    }
    panic!("never reached the map");
}

fn enter_fake_merchant(rm: &mut RunManager, gold: i32) {
    rm.run_state.gold = gold;
    rm.debug_enter_event(EventId::FakeMerchant, begin_event_in_entered_room);
}

/// `(relic, price)` per shop slot, sold slots as `None`.
fn stock(rm: &RunManager) -> Vec<Option<(RelicId, i32)>> {
    rm.shop_inventory()
        .expect("shop open")
        .relics
        .iter()
        .map(|e| {
            sts2_core::shop::is_shop_entry_available(e.price)
                .then(|| (e.relic_id.expect("stocked"), e.price))
        })
        .collect()
}

fn buys(rm: &RunManager) -> Vec<usize> {
    rm.get_available_actions()
        .into_iter()
        .filter_map(|a| match a {
            RunAction::ShopBuyRelic { idx } => Some(idx),
            _ => None,
        })
        .collect()
}

#[test]
fn opens_as_a_shop_of_six_fake_relics() {
    let mut rm = run_at_map("FAKEMERCH1", true);
    enter_fake_merchant(&mut rm, 500);
    assert_eq!(rm.phase(), Phase::Shop);
    assert!(rm.in_fake_merchant_shop());
    assert_eq!(rm.run_state.current_event, Some(EventId::FakeMerchant));
    let s = stock(&rm);
    assert_eq!(s.len(), 6);
    for (relic, price) in s.iter().flatten() {
        assert!(relic.to_python_name().starts_with("FAKE_"), "{relic:?}");
        assert!((43..=57).contains(price), "{price}");
    }
    let actions = rm.get_available_actions();
    let mut want: Vec<RunAction> = (0..6).map(|idx| RunAction::ShopBuyRelic { idx }).collect();
    want.push(RunAction::LeaveShop);
    assert_eq!(actions, want, "relics only, then leave");
}

#[test]
fn buys_any_number_of_relics_at_their_rolled_prices() {
    let mut rm = run_at_map("FAKEMERCH2", true);
    enter_fake_merchant(&mut rm, 500);
    let s = stock(&rm);
    let (r3, p3) = s[3].expect("stocked");
    let (r0, p0) = s[0].expect("stocked");
    assert_eq!(
        rm.take_action(RunAction::ShopBuyRelic { idx: 3 }),
        ActionResult::Advanced
    );
    assert_eq!(rm.phase(), Phase::Shop, "the shop stays open");
    assert_eq!(rm.run_state.gold, 500 - p3);
    assert_eq!(
        rm.take_action(RunAction::ShopBuyRelic { idx: 0 }),
        ActionResult::Advanced
    );
    assert_eq!(rm.run_state.gold, 500 - p3 - p0);
    assert!(rm.run_state.relics.contains(&r3) && rm.run_state.relics.contains(&r0));
    assert_eq!(buys(&rm), vec![1, 2, 4, 5], "bought slots stay empty");
    let after = stock(&rm);
    assert!(after[0].is_none() && after[3].is_none());
}

#[test]
fn unaffordable_relics_are_not_offered_and_no_discount_applies() {
    let mut rm = run_at_map("FAKEMERCH3", true);
    // MembershipCard halves a MerchantRoom's prices only
    // (`MerchantEntry.Cost`); the Fake Merchant is an EventRoom.
    rm.run_state.relics.push(RelicId::MembershipCard);
    rm.run_state.relics.push(RelicId::TheCourier);
    enter_fake_merchant(&mut rm, 500);
    let s = stock(&rm);
    let cheapest = s.iter().flatten().map(|&(_, p)| p).min().expect("stock");
    rm.run_state.gold = cheapest;
    let offered = buys(&rm);
    assert!(!offered.is_empty());
    for (idx, slot) in s.iter().enumerate() {
        let (_, price) = slot.expect("stocked");
        assert_eq!(offered.contains(&idx), price <= cheapest, "slot {idx}");
    }
    let idx = offered[0];
    let _ = rm.take_action(RunAction::ShopBuyRelic { idx });
    assert_eq!(rm.run_state.gold, 0, "charged the rolled price");
    assert!(stock(&rm)[idx].is_none(), "TheCourier does not restock it");
    assert_eq!(rm.get_available_actions(), vec![RunAction::LeaveShop]);
}

#[test]
fn leaving_ends_the_event() {
    let mut rm = run_at_map("FAKEMERCH4", true);
    enter_fake_merchant(&mut rm, 500);
    let _ = rm.take_action(RunAction::ShopBuyRelic { idx: 1 });
    assert_eq!(rm.take_action(RunAction::LeaveShop), ActionResult::Advanced);
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert_eq!(rm.run_state.current_event, None);
    assert!(rm.shop_inventory().is_none());
}

#[test]
fn foul_potion_starts_the_fight_with_the_unsold_stock() {
    let mut rm = run_at_map("FAKEMERCH5", true);
    rm.run_state.ooc_potion_actions = true;
    rm.run_state.potions = vec![PotionKind::BloodPotion, PotionKind::FoulPotion];
    rm.run_state.potion_slot_indices.clear();
    enter_fake_merchant(&mut rm, 500);
    let s = stock(&rm);
    let _ = rm.take_action(RunAction::ShopBuyRelic { idx: 2 });
    let _ = rm.take_action(RunAction::ShopBuyRelic { idx: 4 });
    let gold = rm.run_state.gold;
    let throw = RunAction::UsePotionOutOfCombat { potion_slot: 1 };
    assert!(rm.get_available_actions().contains(&throw));
    assert_eq!(rm.take_action(throw), ActionResult::Advanced);
    assert_eq!(rm.phase(), Phase::Combat);
    assert_eq!(rm.run_state.gold, gold, "no 100 gold at the Fake Merchant");
    assert_eq!(rm.run_state.potions, vec![PotionKind::BloodPotion]);
    assert!(rm.shop_inventory().is_none());
    let req = rm.run_state.event_combat.as_ref().expect("fight staged");
    assert_eq!(req.encounter, EventEncounter::FakeMerchant);
    assert!(!req.resume);
    let rewards: Vec<RelicId> = req
        .victory_rewards
        .iter()
        .map(|r| match r {
            Reward::Relic(rr) => rr.relic_id,
            other => panic!("{other:?}"),
        })
        .collect();
    let mut want = vec![RelicId::FakeMerchantsRug];
    want.extend(
        s.iter()
            .enumerate()
            .filter(|(i, _)| *i != 2 && *i != 4)
            .map(|(_, slot)| slot.expect("stocked").0),
    );
    assert_eq!(rewards, want);
}

#[test]
fn parity_mode_shows_the_shop_but_no_throw() {
    let mut rm = run_at_map("FAKEMERCH6", false);
    rm.run_state.potions = vec![PotionKind::FoulPotion];
    rm.run_state.potion_slot_indices.clear();
    enter_fake_merchant(&mut rm, 500);
    assert_eq!(rm.phase(), Phase::Shop);
    let actions = rm.get_available_actions();
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, RunAction::UsePotionOutOfCombat { .. })),
        "{actions:?}"
    );
    assert_eq!(actions.last(), Some(&RunAction::LeaveShop));
}

/// The stock is the event RNG's shuffle (`FAKE_MERCHANT` stream off the run
/// seed): the same seed stocks the same relics.
#[test]
fn stock_is_seed_deterministic() {
    let mut a = run_at_map("FAKEMERCH7", true);
    let mut b = run_at_map("FAKEMERCH7", true);
    enter_fake_merchant(&mut a, 500);
    enter_fake_merchant(&mut b, 500);
    assert_eq!(stock(&a), stock(&b));
}
