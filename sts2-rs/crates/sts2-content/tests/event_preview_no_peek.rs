//! `preview_option_effect` must not leak hidden RNG (GH #1035).
//!
//! The preview feeds the PPO run policy's event features. It resolves the
//! option on a state clone, so an option whose payoff is rolled only on pick
//! would hand the policy the exact outcome — CrystalSphere's divinations did.
//!
//! Method: enter every event from many seeds with otherwise identical state,
//! walk its pages, and group options by everything the player sees (label,
//! description, vars). The preview may vary only between groups, never
//! within one: within a group the only difference is the hidden seed.
use std::collections::{BTreeMap, BTreeSet};

use sts2_content::events::dispatch::{present, preview_option_effect, resolve_by_index};
use sts2_core::events::EventId;
use sts2_core::potions::{CharacterId, PotionKind};
use sts2_core::run_manager::RunManager;
use sts2_core::runstate::RunState;
use sts2_core::RelicId;

const SEEDS: i32 = 40;
const MAX_DEPTH: usize = 3;

fn run_state(seed: i32, act: u8) -> RunManager {
    let mut rm = RunManager::new(seed, CharacterId::Ironclad);
    let rs = &mut rm.run_state;
    rs.act_index = act;
    rs.gold = 300;
    rs.current_hp = 60;
    rs.potions = vec![PotionKind::BlockPotion, PotionKind::FirePotion];
    rs.potion_slot_indices = vec![0, 1];
    rs.relics
        .extend([RelicId::Anchor, RelicId::Lantern, RelicId::HappyFlower]);
    rs.direct_event_relic_grant = true;
    rm
}

/// (visible option signature, preview) for every option reachable within
/// `MAX_DEPTH` pages.
fn walk(rs: &RunState, path: &str, depth: usize, out: &mut Vec<(String, String)>) {
    if depth > MAX_DEPTH || rs.current_event.is_none() {
        return;
    }
    let Some(opts) = present(rs) else { return };
    for (i, o) in opts.iter().enumerate() {
        let visible = format!(
            "{path}/{i}|{}|{}|{}|{}|{:?}|{:?}",
            o.option_id, o.label, o.description, o.enabled, o.vars, o.title_vars
        );
        out.push((visible, format!("{:?}", preview_option_effect(rs, i))));
        if o.enabled {
            let mut next = rs.clone();
            if resolve_by_index(&mut next, i).is_some() {
                // The label is part of the path: a page reached by taking a
                // DIFFERENT visible option (Endless Conveyor's grab row names
                // the dish just eaten) is a different visible history.
                walk(
                    &next,
                    &format!("{path}/{}[{}]", o.option_id, o.label),
                    depth + 1,
                    out,
                );
            }
        }
    }
}

/// Visible-option groups whose preview differs across hidden seeds.
fn leaks(events: &[EventId]) -> Vec<String> {
    // Some events panic outside their real preconditions; those seeds are
    // skipped, not counted as leaks.
    std::panic::set_hook(Box::new(|_| {}));
    let mut found = Vec::new();
    for &ev in events {
        for act in 0u8..3 {
            let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for seed in 1..=SEEDS {
                let mut rm = run_state(seed, act);
                let rows = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rm.run_state.enter_event_room(ev);
                    let mut out = Vec::new();
                    walk(&rm.run_state, "", 0, &mut out);
                    out
                }));
                for (visible, preview) in rows.unwrap_or_default() {
                    groups.entry(visible).or_default().insert(preview);
                }
            }
            found.extend(
                groups
                    .into_iter()
                    .filter(|(_, previews)| previews.len() > 1)
                    .map(|(visible, previews)| format!("{ev:?} act{act} {visible}: {previews:?}")),
            );
        }
    }
    let _ = std::panic::take_hook();
    found
}

#[test]
fn no_event_preview_depends_on_hidden_rng() {
    sts2_content::cards::dispatch::ensure_installed();
    let found = leaks(EventId::all());
    assert!(
        found.is_empty(),
        "{} leaking options:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// CrystalSphere previews its visible cost only: Uncover Future = the rolled
/// gold price, Payment Plan = the Debt curse — never the divination haul.
#[test]
fn crystal_sphere_previews_cost_not_haul() {
    sts2_content::cards::dispatch::ensure_installed();
    let mut hauls = 0;
    for seed in 1..=SEEDS {
        let mut rm = run_state(seed, 1);
        rm.run_state.enter_event_room(EventId::CrystalSphere);
        let rs = &rm.run_state;
        let opts = present(rs).expect("options");
        let idx = |id: &str| opts.iter().position(|o| o.option_id == id).expect(id);
        let cost = sts2_content::events::act2_hive::crystal_sphere_pay_cost(rs);

        let pay = preview_option_effect(rs, idx("pay")).expect("pay");
        assert_eq!(pay.gold_delta, -cost);
        assert_eq!(
            (pay.potions_gained, pay.cards_gained, pay.relics_gained),
            (0, 0, 0)
        );

        let debt = preview_option_effect(rs, idx("debt")).expect("debt");
        assert_eq!(debt.gold_delta, 0);
        assert_eq!(debt.cards_gained, 1, "the Debt curse");
        assert_eq!((debt.potions_gained, debt.relics_gained), (0, 0));

        // The real resolve still plays the minigame and pays out on top.
        let mut live = rs.clone();
        resolve_by_index(&mut live, idx("debt")).expect("resolves");
        if live.gold > rs.gold || !live.pending_rewards.is_empty() {
            hauls += 1;
        }
    }
    assert!(
        hauls > 0,
        "no Payment Plan paid out beyond its preview — test is vacuous"
    );
}

/// The War Historian's chest rolls two potions and two relics on pick. With a
/// full belt, whether the potions fit read the rolled relics (a Potion Belt
/// among them makes room), so the preview told the student the hidden relic
/// roll (Spire Bench seed 170000032: no potions lost natively, two in the
/// advisor's rebuild of the same screen).
#[test]
fn war_historians_chest_previews_the_same_whatever_relics_it_rolls() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::install_default_registry();
    let preview = |front: RelicId| {
        let mut rm = run_state(1, 2);
        let rs = &mut rm.run_state;
        rs.max_potion_slots = 2;
        rs.interactive_potion_swap = true;
        for rarity in [
            sts2_core::runstate::RelicRarity::Common,
            sts2_core::runstate::RelicRarity::Uncommon,
            sts2_core::runstate::RelicRarity::Rare,
        ] {
            rs.relic_grab_bag_by_rarity
                .insert(rarity, vec![front, RelicId::Vajra, RelicId::Pocketwatch]);
        }
        rs.master_deck.push(
            sts2_content::cards::spec_index::card_pod_from_id(
                sts2_core::cards::CardId::LanternKey,
                false,
            )
            .expect("LanternKey card POD"),
        );
        rs.enter_event_room(EventId::WarHistorianRepy);
        let opts = present(rs).expect("options");
        let chest = opts
            .iter()
            .position(|o| o.loc_key.contains("UNLOCK_CHEST"))
            .expect("the chest option");
        preview_option_effect(rs, chest).expect("a preview")
    };
    assert_eq!(
        format!("{:?}", preview(RelicId::PotionBelt)),
        format!("{:?}", preview(RelicId::Anchor))
    );
}
