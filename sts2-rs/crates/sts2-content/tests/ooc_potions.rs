//! The potion popup (`ooc_potion_actions`, GH #1606): out-of-combat Use and Discard,
//! with the real content wiring (Entropic Brew's roll, Belt Buckle).
use sts2_content::run_callbacks::new_run_manager_for_character_from_string_seed;
use sts2_core::potions::PotionKind;
use sts2_core::powers::PowerKind;
use sts2_core::run_manager::{ActionResult, Phase, RunAction, RunManager};
use sts2_core::CharacterId;

fn agent_run(seed: &str) -> RunManager {
    sts2_content::potions::dispatch::ensure_installed();
    let mut rm = new_run_manager_for_character_from_string_seed(seed, CharacterId::Ironclad);
    rm.run_state.set_agent_decisions();
    rm.run_state.ooc_potion_actions = true;
    rm
}

/// Take the first offered action until `phase` (the popup's actions come
/// last, so this never drinks or discards).
fn drive_to(rm: &mut RunManager, phase: Phase) {
    for _ in 0..200 {
        if rm.phase() == phase {
            return;
        }
        let first = rm.get_available_actions()[0];
        let _ = rm.take_action(first);
    }
    panic!("never reached {phase:?}");
}

#[test]
fn entropic_brew_fills_the_belt_out_of_combat() {
    let mut rm = agent_run("OOCBREW1");
    drive_to(&mut rm, Phase::MapChoice);
    rm.run_state.potions = vec![PotionKind::EntropicBrew];
    rm.run_state.potion_slot_indices.clear();
    let cap = rm.run_state.potion_belt_cap();
    assert!(rm
        .get_available_actions()
        .contains(&RunAction::UsePotionOutOfCombat { potion_slot: 0 }));
    assert_eq!(
        rm.take_action(RunAction::UsePotionOutOfCombat { potion_slot: 0 }),
        ActionResult::Advanced
    );
    assert_eq!(
        rm.run_state.potions.len(),
        cap,
        "{:?}",
        rm.run_state.potions
    );
    // Same seed, same rolls.
    let mut again = agent_run("OOCBREW1");
    drive_to(&mut again, Phase::MapChoice);
    again.run_state.potions = vec![PotionKind::EntropicBrew];
    again.run_state.potion_slot_indices.clear();
    let _ = again.take_action(RunAction::UsePotionOutOfCombat { potion_slot: 0 });
    assert_eq!(again.run_state.potions, rm.run_state.potions);
}

#[test]
fn combat_discard_of_the_last_potion_arms_belt_buckle() {
    let mut rm = agent_run("OOCBUCKLE1");
    drive_to(&mut rm, Phase::Combat);
    let combat = rm.combat.as_mut().expect("combat");
    combat.player.potions.clear();
    combat.player.potion_slot_indices.clear();
    combat.player.potions.push(PotionKind::BlockPotion);
    combat.relics.push(sts2_core::Relic {
        id: std::num::NonZeroU32::new(9_999).expect("nz"),
        kind: sts2_core::RelicKind::BeltBuckle,
        counter: 0,
        is_wax: false,
        is_melted: false,
    });
    let dex = combat.player_power_amount(PowerKind::Dexterity);
    let actions = rm.get_available_actions();
    assert_eq!(
        actions.last(),
        Some(&RunAction::DiscardPotion { potion_slot: 0 }),
        "{actions:?}"
    );
    assert_eq!(
        rm.take_action(RunAction::DiscardPotion { potion_slot: 0 }),
        ActionResult::Advanced
    );
    let combat = rm.combat.as_ref().expect("combat");
    assert!(combat.player.potions.is_empty());
    assert_eq!(
        combat.player_power_amount(PowerKind::Dexterity),
        dex + sts2_core::BELT_BUCKLE_DEXTERITY
    );
}

#[test]
fn runs_without_the_flag_never_offer_the_popup() {
    let mut rm = new_run_manager_for_character_from_string_seed("OOCPARITY", CharacterId::Ironclad);
    // Agent mode alone does not open it: the flag is per host.
    rm.run_state.set_agent_decisions();
    rm.run_state.potions = vec![PotionKind::BloodPotion];
    for _ in 0..60 {
        let actions = rm.get_available_actions();
        assert!(
            !actions.iter().any(|a| matches!(
                a,
                RunAction::UsePotionOutOfCombat { .. } | RunAction::DiscardPotion { .. }
            )),
            "{actions:?}"
        );
        let Some(&first) = actions.first() else { break };
        let _ = rm.take_action(first);
    }
}
