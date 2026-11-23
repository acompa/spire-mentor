//! GH #1702: a respawned Axebot telegraphs BOOT_UP's Strength
//! (`BootUpStrGain * (2 - Stock)`), written into its move at respawn. C#'s
//! `BuffIntent` shows no number, so a rebuild built the AI with the table's
//! factory 0.

use sts2_core::{Intent, PowerKind};
use sts2_wire::emit::combat::combat_state_to_value;

#[test]
fn a_rebuilt_respawned_axebot_telegraphs_its_boot_up_strength() {
    let mut state = sts2_wire::scenario::build_scenario_state_for_version(
        sts2_core::GameVersion::V0_111_0,
        7,
        "Ironclad",
        &vec!["STRIKE_IRONCLAD".to_owned(); 10],
        80,
        80,
        &[],
        &[],
        "AXEBOTS_NORMAL",
        10,
        None,
        None,
    )
    .expect("scenario");
    let bot = state.enemy_uids[0];
    // The respawned bot opens on BOOT_UP and telegraphs Defend + Buff.
    if let Some(c) = state.creatures.iter_mut().find(|c| c.id == bot.0) {
        let ai = c.ai.as_mut().expect("ai");
        assert!(ai.force_current_state_by_name("BOOT_UP"));
        c.intent = ai.current_move().intents.first().copied();
    }
    let stock = state
        .powers
        .iter()
        .find(|p| p.kind == PowerKind::Stock)
        .map(|p| p.id);
    let stock = stock.expect("a Stock power");
    if let Some(p) = state.powers.get_mut_by_id(stock) {
        p.amount = 1; // a first respawn
    }
    let wire = combat_state_to_value(&state).expect("serializes");
    let back = sts2_wire::state_from_json(&wire).expect("round-trips");
    let owner = back
        .creatures
        .iter()
        .find(|c| {
            c.powers.iter().any(|u| {
                back.powers
                    .get_by_id(u.0)
                    .is_some_and(|p| p.kind == PowerKind::Stock && p.amount == 1)
            })
        })
        .expect("the stocked bot");
    let intents = owner
        .ai
        .as_ref()
        .expect("ai")
        .current_move()
        .intents
        .clone();
    // A10 DeadlyEnemies: 4 * (2 - 1).
    assert!(
        intents.contains(&Intent::Buff {
            kind: PowerKind::Strength,
            amount: 4
        }),
        "{intents:?} (bot {bot:?})"
    );
}
