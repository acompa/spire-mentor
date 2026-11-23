//! The sim plays Crystal Sphere with the digger in agent mode
//! (`RunState::crystal_sphere_digger`, set by `set_agent_decisions`) and
//! with the oracle's fixed sweep in parity mode.

use sts2_content::events::crystal_sphere::{
    CrystalSphereItemKind, CrystalSphereMinigame, CrystalSphereTool, HEADLESS_SWEEP_CENTERS,
};
use sts2_content::events::crystal_sphere_digger::pick;
use sts2_content::events::dispatch::resolve_by_index;
use sts2_core::dotnet_rng::Rng;
use sts2_core::events::EventId;
use sts2_core::potions::CharacterId;
use sts2_core::run_manager::RunManager;
use sts2_core::runstate::RunState;

/// Option indices on the Crystal Sphere's page.
const PAY: usize = 0;
const DEBT: usize = 1;

/// A set-up Ironclad run (grab bags populated) standing in a Crystal Sphere
/// room with 300 gold.
fn in_crystal_sphere(seed: i32, agent: bool) -> RunState {
    let mut rm = RunManager::new(seed, CharacterId::Ironclad);
    if agent {
        rm.run_state.set_agent_decisions();
    }
    sts2_content::run_callbacks::initialize_run_setup(&mut rm);
    let mut rs = rm.run_state;
    rs.gold = 300;
    rs.enter_event_room(EventId::CrystalSphere);
    rs
}

/// The event Rng past `CalculateVars`, and the Uncover Future cost.
fn event_rng(rs: &RunState) -> (Rng, i32) {
    let mut rng = rs.rng.named("CRYSTAL_SPHERE");
    let cost = 50 + rng.next_int_exclusive(1, 50);
    (rng, cost)
}

/// Gold the revealed items pay out (small 10, big 30).
fn gold_of(revealed: &[CrystalSphereItemKind]) -> i32 {
    revealed
        .iter()
        .map(|k| match k {
            CrystalSphereItemKind::Gold { big: true } => 30,
            CrystalSphereItemKind::Gold { big: false } => 10,
            _ => 0,
        })
        .sum()
}

#[test]
fn agent_mode_turns_the_digger_on() {
    let mut rs = RunState::new();
    assert!(!rs.crystal_sphere_digger);
    assert!(rs.has_parity_decisions());
    rs.set_agent_decisions();
    assert!(rs.crystal_sphere_digger);
    assert!(rs.has_agent_decisions());
    rs.crystal_sphere_digger = false;
    assert!(!rs.has_agent_decisions());
    assert!(!rs.has_parity_decisions());
}

#[test]
fn debt_option_in_agent_mode_always_grants_a_relic() {
    for seed in 1..=50 {
        let mut rs = in_crystal_sphere(seed, true);
        let relics_before = rs.relics.len();
        let pending_before = rs.relic_rewards.len();
        resolve_by_index(&mut rs, DEBT).expect("debt resolves");
        let granted = rs.relics.len() - relics_before;
        let pending = rs.relic_rewards.len() - pending_before;
        assert_eq!(
            granted + pending,
            1,
            "seed {seed}: the digger's six divinations reveal the relic"
        );
    }
}

#[test]
fn agent_mode_event_is_deterministic() {
    for seed in 1..=20 {
        let play = || {
            let mut rs = in_crystal_sphere(seed, true);
            resolve_by_index(&mut rs, DEBT).expect("debt resolves");
            let deck: Vec<_> = rs.master_deck.iter().map(|c| c.card_id).collect();
            (
                rs.gold,
                deck,
                rs.relics.clone(),
                rs.potions.clone(),
                format!("{:?}", rs.pending_rewards),
            )
        };
        assert_eq!(play(), play(), "seed {seed}");
    }
}

#[test]
fn parity_mode_still_plays_the_sweep() {
    let mut digger_would_differ = false;
    for seed in 1..=20 {
        let mut rs = in_crystal_sphere(seed, false);
        assert!(rs.has_parity_decisions() && !rs.crystal_sphere_digger);
        let gold_before = rs.gold;
        let (rng, cost) = event_rng(&rs);

        // The pre-change logic: Big tool, the sweep centres, three divinations.
        let mut sweep = CrystalSphereMinigame::new(&mut rng.clone(), 3);
        sweep.set_tool(CrystalSphereTool::Big);
        for (x, y) in HEADLESS_SWEEP_CENTERS {
            if sweep.divinations() == 0 {
                break;
            }
            sweep.cell_clicked(x, y);
        }
        let mut dug = CrystalSphereMinigame::new(&mut rng.clone(), 3);
        dug.set_tool(CrystalSphereTool::Big);
        while let Some((x, y)) = pick(&dug.view(3)) {
            dug.cell_clicked(x, y);
        }
        digger_would_differ |= gold_of(dug.revealed()) != gold_of(sweep.revealed());

        resolve_by_index(&mut rs, PAY).expect("pay resolves");
        assert_eq!(
            rs.gold,
            gold_before - cost + gold_of(sweep.revealed()),
            "seed {seed}: parity mode pays out the sweep's gold"
        );
    }
    assert!(
        digger_would_differ,
        "no seed tells the sweep from the digger by gold"
    );
}
