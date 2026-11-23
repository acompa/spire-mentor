//! Event relics the game obtains directly (GH #1605).
//!
//! C# gives these with `RelicCmd.Obtain` inside the option's body: no
//! "Take relic / Skip" step. In agent mode (`direct_event_relic_grant`) the
//! relic must land in `relics` while the option resolves, through
//! `RunState::grant_event_relic`; in parity mode (all flags off) it stays
//! queued on `relic_rewards` exactly as before.
//!
//! Also Trial's Double Down (`Trial.cs:121`,
//! `ThatWillKillPlayerIf(_ => true)`): every live chooser withholds it, so
//! agent mode does not offer it; parity mode still does.
use sts2_content::events::dispatch::{present, resolve_by_index};
use sts2_core::events::EventId;
use sts2_core::potions::CharacterId;
use sts2_core::run_manager::RunManager;
use sts2_core::runstate::RunState;
use sts2_core::RelicId;

fn run(agent: bool) -> RunManager {
    let mut rm = RunManager::new(7, CharacterId::Ironclad);
    rm.run_state.gold = 300;
    rm.run_state.current_hp = 30;
    if agent {
        rm.run_state.set_agent_decisions();
    }
    rm
}

fn option_ids(rs: &RunState) -> Vec<&'static str> {
    present(rs)
        .expect("in an event")
        .iter()
        .map(|o| o.option_id)
        .collect()
}

fn choose(rs: &mut RunState, option_id: &str) {
    let idx = option_ids(rs)
        .iter()
        .position(|o| *o == option_id)
        .unwrap_or_else(|| panic!("{option_id} not offered: {:?}", option_ids(rs)));
    resolve_by_index(rs, idx).expect("option resolves");
}

/// (event, option, the relic `RelicCmd.Obtain` gives).
const DIRECT: &[(EventId, &str, RelicId)] = &[
    // RoundTeaParty.cs:43
    (EventId::RoundTeaParty, "enjoy_tea", RelicId::RoyalPoison),
    // TeaMaster.cs:69,76,82
    (EventId::TeaMaster, "bone_tea", RelicId::BoneTea),
    (EventId::TeaMaster, "ember_tea", RelicId::EmberTea),
    (EventId::TeaMaster, "discourtesy", RelicId::TeaOfDiscourtesy),
    // GraveOfTheForgotten.cs:69
    (
        EventId::GraveOfTheForgotten,
        "accept",
        RelicId::ForgottenSoul,
    ),
    // LostWisp.cs:50
    (EventId::LostWisp, "claim", RelicId::LostWisp),
];

#[test]
fn agent_mode_obtains_event_relics_directly() {
    for &(event, option, relic) in DIRECT {
        let mut rm = run(true);
        let rs = &mut rm.run_state;
        rs.enter_event_room(event);
        choose(rs, option);
        assert!(
            rs.relics.contains(&relic),
            "{event:?}/{option}: {relic:?} obtained while the option resolves"
        );
        assert!(
            rs.relic_rewards.is_empty(),
            "{event:?}/{option}: nothing left to take or skip"
        );
    }
}

#[test]
fn parity_mode_still_queues_event_relics() {
    for &(event, option, relic) in DIRECT {
        let mut rm = run(false);
        let rs = &mut rm.run_state;
        rs.enter_event_room(event);
        let owned_before = rs.relics.clone();
        choose(rs, option);
        assert_eq!(rs.relics, owned_before, "{event:?}/{option}");
        assert_eq!(
            rs.relic_rewards
                .iter()
                .map(|r| r.relic_id)
                .collect::<Vec<_>>(),
            [relic],
            "{event:?}/{option}"
        );
    }
}

/// C# obtains Royal Poison BEFORE the full heal (`RoundTeaParty.cs:43-44`).
#[test]
fn enjoy_tea_still_heals_to_full_in_agent_mode() {
    let mut rm = run(true);
    let rs = &mut rm.run_state;
    rs.enter_event_room(EventId::RoundTeaParty);
    choose(rs, "enjoy_tea");
    assert_eq!(rs.current_hp, rs.max_hp);
}

fn trial_reject_page(agent: bool) -> Vec<&'static str> {
    let mut rm = run(agent);
    let rs = &mut rm.run_state;
    rs.enter_event_room(EventId::Trial);
    choose(rs, "reject");
    option_ids(rs)
}

#[test]
fn agent_mode_withholds_trial_double_down() {
    assert_eq!(trial_reject_page(true), ["reject_accept"]);
}

#[test]
fn parity_mode_keeps_trial_double_down() {
    assert_eq!(trial_reject_page(false), ["reject_accept", "double_down"]);
}
