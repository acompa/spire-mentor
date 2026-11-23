//! `RunState::deck_select_screens` (GH #1619, #1621): the deck selects as the
//! game's select screens. Smith and Cook can be cancelled back to the
//! campfire, and a pick-N deck select is one select over the deck as it stood
//! when it opened, applied on its last pick.
use sts2_core::cards::{Card, CardId};
use sts2_core::potions::CharacterId;
use sts2_core::run_manager::{ActionResult, Phase, RunAction, RunManager};
use sts2_core::runstate::RestSiteOption;
use sts2_core::RelicId;

fn deck(ids: &[CardId]) -> Vec<Card> {
    ids.iter()
        .map(|&card_id| Card {
            card_id,
            upgraded: false,
            ..Card::pod_defaults()
        })
        .collect()
}

fn run(screens: bool, ids: &[CardId]) -> RunManager {
    sts2_content::relics::dispatch::ensure_installed();
    let mut rm = RunManager::new(0, CharacterId::Ironclad)
        .with_content_callbacks(sts2_content::run_callbacks::ironclad_content_callbacks());
    rm.run_state.set_agent_decisions();
    rm.run_state.deck_select_screens = screens;
    rm.run_state.master_deck = deck(ids);
    rm.run_state.max_hp = 80;
    rm.run_state.current_hp = 40;
    rm
}

fn ids(rm: &RunManager) -> Vec<(CardId, bool)> {
    rm.run_state
        .master_deck
        .iter()
        .map(|c| (c.card_id, c.upgraded))
        .collect()
}

fn option_idx(rm: &RunManager, f: impl Fn(&RestSiteOption) -> bool) -> usize {
    rm.run_state
        .rest_site_options()
        .iter()
        .position(f)
        .expect("option offered")
}

const THREE: [CardId; 3] = [CardId::Bash, CardId::PommelStrike, CardId::Anger];

#[test]
fn smith_select_cancels_back_to_the_campfire() {
    let mut rm = run(true, &THREE);
    rm.enter_rest_site();
    let before = rm.get_available_actions();
    let smith = option_idx(&rm, |o| matches!(o, RestSiteOption::Smith { .. }));
    assert_eq!(
        rm.take_action(RunAction::RestOption { idx: smith }),
        ActionResult::Advanced
    );
    let actions = rm.get_available_actions();
    assert_eq!(
        actions,
        vec![
            RunAction::SmithCard { idx: 0 },
            RunAction::SmithCard { idx: 1 },
            RunAction::SmithCard { idx: 2 },
            RunAction::ConfirmDeckSelection,
        ]
    );
    assert!(rm.deck_select_cancelable());
    assert_eq!(
        rm.take_action(RunAction::ConfirmDeckSelection),
        ActionResult::Advanced
    );
    assert_eq!(rm.phase(), Phase::RestSite, "back at the campfire");
    assert_eq!(
        rm.get_available_actions(),
        before,
        "every option still open"
    );
    assert!(ids(&rm).iter().all(|&(_, up)| !up), "nothing upgraded");
    // Rest is still there.
    rm.take_action(RunAction::RestOption { idx: 0 });
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert_eq!(rm.run_state.current_hp, 40 + 24);
}

#[test]
fn smith_select_still_upgrades_the_picked_card() {
    let mut rm = run(true, &THREE);
    rm.enter_rest_site();
    let smith = option_idx(&rm, |o| matches!(o, RestSiteOption::Smith { .. }));
    rm.take_action(RunAction::RestOption { idx: smith });
    rm.take_action(RunAction::SmithCard { idx: 1 });
    assert_eq!(
        ids(&rm),
        vec![
            (CardId::Bash, false),
            (CardId::PommelStrike, true),
            (CardId::Anger, false)
        ]
    );
    assert_eq!(rm.phase(), Phase::MapChoice);
}

/// Parity mode keeps the oracle's flow: no cancel, one card auto-upgraded.
#[test]
fn parity_mode_smith_is_unchanged() {
    let mut rm = run(false, &THREE);
    rm.enter_rest_site();
    let smith = option_idx(&rm, |o| matches!(o, RestSiteOption::Smith { .. }));
    rm.take_action(RunAction::RestOption { idx: smith });
    assert!(!rm
        .get_available_actions()
        .contains(&RunAction::ConfirmDeckSelection));
    assert!(!rm.deck_select_cancelable());

    let mut one = run(false, &[CardId::Bash]);
    one.enter_rest_site();
    one.take_action(RunAction::RestOption { idx: smith });
    assert!(one.run_state.master_deck[0].upgraded);
    assert_eq!(one.phase(), Phase::MapChoice);
}

fn cook_site(screens: bool) -> (RunManager, usize) {
    let mut rm = run(screens, &THREE);
    rm.run_state.relics.push(RelicId::MeatCleaver);
    rm.enter_rest_site();
    let cook = option_idx(&rm, |o| matches!(o, RestSiteOption::Cook { .. }));
    (rm, cook)
}

#[test]
fn cook_select_cancels_back_to_the_campfire() {
    let (mut rm, cook) = cook_site(true);
    let before = rm.get_available_actions();
    let max_hp = rm.run_state.max_hp;
    rm.take_action(RunAction::RestOption { idx: cook });
    assert_eq!(
        rm.get_available_actions(),
        vec![
            RunAction::RemoveDeckCard { idx: 0 },
            RunAction::RemoveDeckCard { idx: 1 },
            RunAction::RemoveDeckCard { idx: 2 },
            RunAction::ConfirmDeckSelection,
        ]
    );
    rm.take_action(RunAction::ConfirmDeckSelection);
    assert_eq!(rm.phase(), Phase::RestSite);
    assert_eq!(rm.get_available_actions(), before);
    assert_eq!(rm.run_state.master_deck.len(), 3);
    assert_eq!(rm.run_state.max_hp, max_hp);
    assert_eq!(rm.run_state.pending_deck_removals, 0);
    assert_eq!(rm.run_state.pending_post_removal_max_hp_gain, 0);
}

#[test]
fn cook_select_is_one_pick_two_then_the_option_is_used() {
    let (mut rm, cook) = cook_site(true);
    let max_hp = rm.run_state.max_hp;
    rm.take_action(RunAction::RestOption { idx: cook });
    rm.take_action(RunAction::RemoveDeckCard { idx: 0 });
    // The first pick only marks the card: no cancel after it, deck unchanged.
    assert_eq!(rm.run_state.master_deck.len(), 3);
    assert_eq!(
        rm.get_available_actions(),
        vec![
            RunAction::RemoveDeckCard { idx: 0 },
            RunAction::RemoveDeckCard { idx: 1 },
        ]
    );
    // idx 1 of what is left = Anger.
    rm.take_action(RunAction::RemoveDeckCard { idx: 1 });
    assert_eq!(ids(&rm), vec![(CardId::PommelStrike, false)]);
    // `RunManager::new` plays v0.107.1: Cook's gain is 9 there (5 in v0.111.0).
    assert_eq!(rm.run_state.max_hp, max_hp + 9);
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert!(!rm.run_state.cook_select_open);
}

/// Parity mode: Cook queues its removals and is used at once.
#[test]
fn parity_mode_cook_is_unchanged() {
    let (mut rm, cook) = cook_site(false);
    rm.take_action(RunAction::RestOption { idx: cook });
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert!(!rm
        .get_available_actions()
        .contains(&RunAction::ConfirmDeckSelection));
    rm.take_action(RunAction::RemoveDeckCard { idx: 0 });
    assert_eq!(rm.run_state.master_deck.len(), 2, "one card at a time");
}

#[test]
fn pick_n_upgrade_is_one_select_applied_on_its_last_pick() {
    let mut rm = run(
        true,
        &[
            CardId::Bash,
            CardId::PommelStrike,
            CardId::Anger,
            CardId::Bash,
        ],
    );
    rm.debug_queue_deck_upgrades(2);
    assert_eq!(rm.get_available_actions().len(), 4);
    rm.take_action(RunAction::UpgradeDeckCard { idx: 2 });
    assert!(
        ids(&rm).iter().all(|&(_, up)| !up),
        "the deck waits for the last pick"
    );
    assert_eq!(rm.deck_select_picked(), &[2]);
    assert_eq!(
        rm.get_available_actions(),
        (0..3)
            .map(|idx| RunAction::UpgradeDeckCard { idx })
            .collect::<Vec<_>>()
    );
    // idx 2 of what is left = the second Bash.
    rm.take_action(RunAction::UpgradeDeckCard { idx: 2 });
    assert_eq!(
        ids(&rm),
        vec![
            (CardId::Bash, false),
            (CardId::PommelStrike, false),
            (CardId::Anger, true),
            (CardId::Bash, true)
        ]
    );
    assert_eq!(rm.run_state.pending_deck_upgrades, 0);
    assert!(rm.deck_select_picked().is_empty());
    assert!(
        !rm.deck_select_cancelable(),
        "a relic's upgrade is not cancelable"
    );
}

/// The transform-2 select offers the deck as it was: the second pick cannot
/// take the first pick's replacement (audit row 19), and the transforms run
/// in deck order, as in parity mode (same picks, same rolls, same deck).
#[test]
fn pick_n_transform_offers_the_pre_transform_deck() {
    let picked_first = 2;
    let mut rm = run(
        true,
        &[
            CardId::Bash,
            CardId::PommelStrike,
            CardId::Anger,
            CardId::Bash,
        ],
    );
    rm.debug_queue_deck_transforms(2, false);
    let offered = rm.get_available_actions().len();
    assert_eq!(offered, 4);
    rm.take_action(RunAction::TransformDeckCard { idx: picked_first });
    assert_eq!(rm.run_state.master_deck.len(), 4);
    assert_eq!(
        rm.get_available_actions().len(),
        3,
        "no replacement on offer"
    );
    rm.take_action(RunAction::TransformDeckCard { idx: 0 });
    assert!(rm.run_state.pending_deck_transforms.is_empty());
    let screens = ids(&rm);

    // Parity mode, same cards in deck order: Bash (0), then Anger (2 -> 1).
    let mut seq = run(
        false,
        &[
            CardId::Bash,
            CardId::PommelStrike,
            CardId::Anger,
            CardId::Bash,
        ],
    );
    seq.debug_queue_deck_transforms(2, false);
    seq.take_action(RunAction::TransformDeckCard { idx: 0 });
    seq.take_action(RunAction::TransformDeckCard { idx: 1 });
    assert_eq!(screens, ids(&seq));
    assert_eq!(
        screens[..2],
        [(CardId::PommelStrike, false), (CardId::Bash, false)]
    );
}

#[test]
fn pick_n_removal_is_one_select() {
    let mut rm = run(true, &THREE);
    rm.debug_queue_deck_removals(2);
    rm.take_action(RunAction::RemoveDeckCard { idx: 0 });
    assert_eq!(rm.run_state.master_deck.len(), 3);
    assert!(
        !rm.get_available_actions()
            .contains(&RunAction::ConfirmDeckSelection),
        "a relic's removal is not cancelable"
    );
    rm.take_action(RunAction::RemoveDeckCard { idx: 0 });
    assert_eq!(ids(&rm), vec![(CardId::Anger, false)]);
}

/// A single pick applies at once, as before.
#[test]
fn a_single_pick_is_unchanged() {
    let mut rm = run(true, &THREE);
    rm.debug_queue_deck_upgrades(1);
    rm.take_action(RunAction::UpgradeDeckCard { idx: 1 });
    assert!(rm.run_state.master_deck[1].upgraded);
    assert!(rm.deck_select_picked().is_empty());
}
