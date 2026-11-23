//! Crystal Sphere click policies, compared on boards from our own engine.
//!
//! Plays four policies on the same boards (same seed, same board for every
//! policy) at 3 and 6 divinations, and prints per policy: mean value with
//! its standard error, relic rate, curse rate, mean items revealed, and the
//! paired difference from the #872 order with its standard error.
//!
//! - `random hidden cell`: a uniformly random hidden cell, Big tool.
//! - `sim sweep`: the sim's old fixed order, `HEADLESS_SWEEP_CENTERS`.
//! - `live rule (#872)`: Big tool; centres `(3,2) (3,5) (3,8) (6,2) (6,5)
//!   (6,8)` in that order, then the cell that clears the most hidden cells,
//!   nearest the grid centre (`crystal_sphere_policy.py` `pick_cell`).
//! - `digger`: `events::crystal_sphere_digger::pick`.
//!
//! Usage:
//!
//! ```text
//! cargo run --release -p sts2-bench --example crystal_sphere_policies -- [boards] [first_seed]
//! ```
//!
//! Defaults: 20000 boards, first seed 9_000_000.

use sts2_content::events::crystal_sphere::{
    CrystalSphereItemKind, CrystalSphereMinigame, CrystalSphereTool, GRID_SIZE,
    HEADLESS_SWEEP_CENTERS,
};
use sts2_content::events::crystal_sphere_digger::pick;
use sts2_core::dotnet_rng::Rng;
use sts2_core::PotionRarity;

/// The #872 order's Big-tool centres.
const LIVE_CENTERS: [(i32, i32); 6] = [(3, 2), (3, 5), (3, 8), (6, 2), (6, 5), (6, 8)];

const POLICIES: [&str; 4] = [
    "random hidden cell",
    "sim sweep (old)",
    "live rule (#872)",
    "digger",
];
const LIVE: usize = 2;

/// Gold-equivalents of a revealed item.
fn value(kind: &CrystalSphereItemKind) -> f64 {
    match kind {
        CrystalSphereItemKind::Relic => 200.0,
        CrystalSphereItemKind::Potion(PotionRarity::Rare) => 100.0,
        CrystalSphereItemKind::Potion(_) => 50.0,
        CrystalSphereItemKind::CardReward(_) => 92.0,
        CrystalSphereItemKind::Curse => -100.0,
        CrystalSphereItemKind::Gold { big: false } => 10.0,
        CrystalSphereItemKind::Gold { big: true } => 30.0,
    }
}

fn hidden_cells(game: &CrystalSphereMinigame) -> Vec<(i32, i32)> {
    (0..GRID_SIZE)
        .flat_map(|x| (0..GRID_SIZE).map(move |y| (x, y)))
        .filter(|&(x, y)| game.is_hidden(x, y))
        .collect()
}

/// The #872 rule: first hidden centre, else the cell clearing the most
/// hidden cells, nearest the grid centre, then first in scan order.
fn live_pick(game: &CrystalSphereMinigame) -> Option<(i32, i32)> {
    let hidden = hidden_cells(game);
    if let Some(&c) = LIVE_CENTERS.iter().find(|c| hidden.contains(c)) {
        return Some(c);
    }
    let mid = GRID_SIZE / 2;
    hidden.iter().copied().min_by_key(|&(x, y)| {
        let cover = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dy| (x + dx, y + dy)))
            .filter(|c| hidden.contains(c))
            .count();
        (-(cover as i32), (x - mid).pow(2) + (y - mid).pow(2))
    })
}

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Plays one board; returns (value, relic, curse, items revealed).
fn play(seed: u64, digs: u32, policy: usize) -> (f64, bool, bool, usize) {
    let mut game = CrystalSphereMinigame::new(&mut Rng::from_seed(seed), digs);
    game.set_tool(CrystalSphereTool::Big);
    let mut xs = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for step in 0..digs {
        let click = match policy {
            0 => {
                let hidden = hidden_cells(&game);
                if hidden.is_empty() {
                    None
                } else {
                    Some(hidden[(xorshift(&mut xs) % hidden.len() as u64) as usize])
                }
            }
            1 => Some(HEADLESS_SWEEP_CENTERS[step as usize]),
            2 => live_pick(&game),
            _ => pick(&game.view(digs)),
        };
        let Some((x, y)) = click else { break };
        game.cell_clicked(x, y);
    }
    let revealed = game.revealed();
    (
        revealed.iter().map(value).sum(),
        revealed.contains(&CrystalSphereItemKind::Relic),
        revealed.contains(&CrystalSphereItemKind::Curse),
        revealed.len(),
    )
}

fn mean_se(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, (var / n).sqrt())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let boards: u64 = args.next().map_or(20_000, |s| s.parse().expect("boards"));
    let first: u64 = args
        .next()
        .map_or(9_000_000, |s| s.parse().expect("first seed"));
    for digs in [3u32, 6] {
        println!(
            "\n== {digs} divinations, {boards} boards from seed {first} (values: relic 200, rare potion 100, card 92, common potion 50, big gold 30, small gold 10, curse -100)"
        );
        println!(
            "{:<20} {:>7} {:>6} {:>8} {:>8} {:>6} {:>18}",
            "policy", "EV", "+-SE", "relic%", "curse%", "items", "vs #872 (+-SE)"
        );
        let mut vals = vec![Vec::new(); POLICIES.len()];
        let mut relic = [0usize; 4];
        let mut curse = [0usize; 4];
        let mut items = [0usize; 4];
        for seed in first..first + boards {
            for p in 0..POLICIES.len() {
                let (v, r, c, n) = play(seed, digs, p);
                vals[p].push(v);
                relic[p] += usize::from(r);
                curse[p] += usize::from(c);
                items[p] += n;
            }
        }
        let pct = |k: usize| 100.0 * k as f64 / boards as f64;
        for p in 0..POLICIES.len() {
            let (mean, se) = mean_se(&vals[p]);
            let diff: Vec<f64> = vals[p]
                .iter()
                .zip(&vals[LIVE])
                .map(|(a, b)| a - b)
                .collect();
            let (dm, dse) = mean_se(&diff);
            println!(
                "{:<20} {mean:>7.1} {se:>6.1} {:>7.1}% {:>7.1}% {:>6.2} {dm:>+11.1} ({dse:>4.1})",
                POLICIES[p],
                pct(relic[p]),
                pct(curse[p]),
                items[p] as f64 / boards as f64,
            );
        }
    }
}
