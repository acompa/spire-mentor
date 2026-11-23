//! A click policy for the Crystal Sphere minigame: which cell to divine next.
//!
//! Ported from an implementation sent by Daniel Smith, based on the method in FriendlyChomper's video 'Can you always get the relic in Crystal Sphere?' (https://www.youtube.com/watch?v=rhS7Rj0OXRs).
//!
//! The digger reads only what the screen shows ([`SphereView`]): which cells
//! are still fogged, the item class of each cleared cell that shows an item
//! piece, the divinations left and the divinations the minigame started
//! with — never what lies under a fogged cell. It is a policy, not a game
//! rule: its item values and settings are its own, and the parity path
//! ([`super::crystal_sphere`]'s headless sweep) does not use it. It scores
//! every Big-tool click by the items the click would finish, a pro-rata
//! share of the items it would advance, and a prior over fresh boards; with
//! five or more divinations at the start it also keeps the relic guaranteed,
//! via an exact solver over the relic's placements. It plays only the Big
//! tool, while the solver still counts Small clicks among the ways to finish
//! the relic. The benchmark against the other policies is
//! `sts2-bench/examples/crystal_sphere_policies.rs`; its results are in
//! `docs/experiments/2026-11-14-crystal-sphere-digger.md`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use sts2_core::dotnet_rng::Rng;

use super::crystal_sphere::{CrystalSphereMinigame, GRID_SIZE};

/// What a cleared cell holding an item piece shows: the item's class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemClass {
    /// The 4x4 relic.
    Relic,
    /// A potion: 1x3 (common) or 2x2 (rare).
    Potion,
    /// A card reward: 2x2.
    Card,
    /// The curse: 2x2.
    Curse,
    /// Gold: 1x1 (small) or 2x1 (big).
    Gold,
}

/// The screen, as the digger sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SphereView {
    /// Bit `x * 11 + y` set: the cell is still fogged.
    pub hidden: u128,
    /// Cleared cells showing an item piece, with the item's class.
    pub shown: Vec<((i32, i32), ItemClass)>,
    /// Divinations left.
    pub remaining: u32,
    /// Divinations the minigame started with.
    pub total: u32,
}

/// Discount on the prior value of items a click only partly uncovers.
const GAMMA: f64 = 0.25;
/// Chase weight with five or more divinations at the start (zero below).
const CHASE: f64 = 1.0;
/// From this many divinations at the start the digger chases items and
/// keeps the relic guaranteed.
const CHASE_FROM: u32 = 5;
/// The prior: this many fresh boards, board `k` on `Rng::from_seed(PRIOR_SEED + k)`.
const PRIOR_BOARDS: u64 = 4000;
const PRIOR_SEED: u64 = 12345;
/// The solver's memo is cleared once it holds more than this many entries.
const MEMO_CAP: usize = 2_000_000;

const N: i32 = GRID_SIZE;
const CELLS: usize = (GRID_SIZE * GRID_SIZE) as usize;

/// A click: the cell and whether it uses the Big tool.
type Click = (i32, i32, bool);

fn bit(x: i32, y: i32) -> u128 {
    1u128 << (x * N + y)
}

fn inside(x: i32, y: i32) -> bool {
    (0..N).contains(&x) && (0..N).contains(&y)
}

/// Every cell of a mask, ascending bit order (x-major).
fn cells(mut mask: u128) -> impl Iterator<Item = (i32, i32)> {
    std::iter::from_fn(move || {
        if mask == 0 {
            return None;
        }
        let i = mask.trailing_zeros() as i32;
        mask &= mask - 1;
        Some((i / N, i % N))
    })
}

fn mask(of: impl IntoIterator<Item = (i32, i32)>) -> u128 {
    of.into_iter().fold(0, |m, (x, y)| m | bit(x, y))
}

/// The 3x3 block a Big click at `(x, y)` clears, clipped at the edge.
fn block(x: i32, y: i32) -> u128 {
    let mut m = 0;
    for dx in -1..=1 {
        for dy in -1..=1 {
            if inside(x + dx, y + dy) {
                m |= bit(x + dx, y + dy);
            }
        }
    }
    m
}

/// The cells a click clears (`GetAdjacentCells` for Big, the cell for Small).
fn area(click: Click) -> u128 {
    let (x, y, big) = click;
    if big {
        block(x, y)
    } else {
        bit(x, y)
    }
}

/// The fog the constructor leaves: all but the corners, grown twice along
/// rows and columns.
fn fresh_hidden() -> u128 {
    let last = N - 1;
    let mut clear = vec![(0, 0), (last, 0), (last, last), (0, last)];
    for _ in 0..2 {
        let grown: Vec<(i32, i32)> = clear
            .iter()
            .flat_map(|&(x, y)| [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)])
            .filter(|&(x, y)| inside(x, y))
            .collect();
        clear.extend(grown);
    }
    let all = (1u128 << CELLS) - 1;
    all & !mask(clear)
}

/// Each class's shapes `(width, height)` with how many items carry them, in
/// `populate_items` order (relic 4x4; potion 1x3 twice, 2x2 once; card 2x2
/// three times; curse 2x2; gold 1x1 five times, 2x1 twice).
fn shapes(class: ItemClass) -> &'static [((i32, i32), i64)] {
    match class {
        ItemClass::Relic => &[((4, 4), 1)],
        ItemClass::Potion => &[((1, 3), 2), ((2, 2), 1)],
        ItemClass::Card => &[((2, 2), 3)],
        ItemClass::Curse => &[((2, 2), 1)],
        ItemClass::Gold => &[((1, 1), 5), ((2, 1), 2)],
    }
}

/// The digger's gold-equivalent of an item, by class and shape.
fn value(class: ItemClass, shape: (i32, i32)) -> f64 {
    match (class, shape) {
        (ItemClass::Relic, (4, 4)) => 200.0,
        (ItemClass::Potion, (1, 3)) => 50.0,
        (ItemClass::Potion, (2, 2)) => 100.0,
        (ItemClass::Card, (2, 2)) => 92.0,
        (ItemClass::Curse, (2, 2)) => -100.0,
        (ItemClass::Gold, (1, 1)) => 10.0,
        (ItemClass::Gold, (2, 1)) => 30.0,
        _ => 0.0,
    }
}

/// Shown cells grouped by class into 4-connected components, in the order
/// their first cell is shown.
fn components(shown: &[((i32, i32), ItemClass)]) -> Vec<(ItemClass, Vec<(i32, i32)>)> {
    let class_at: HashMap<(i32, i32), ItemClass> = shown.iter().copied().collect();
    let mut seen: HashSet<(i32, i32)> = HashSet::new();
    let mut out = Vec::new();
    for &(start, class) in shown {
        if !seen.insert(start) {
            continue;
        }
        let mut stack = vec![start];
        let mut comp = Vec::new();
        while let Some((x, y)) = stack.pop() {
            comp.push((x, y));
            for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if class_at.get(&next) == Some(&class) && seen.insert(next) {
                    stack.push(next);
                }
            }
        }
        out.push((class, comp));
    }
    out
}

/// The top-left coordinates along one axis for an item `extent` long over a
/// component spanning `lo..=hi`: covering all of it, or (when `cover_all` is
/// false) overlapping it.
fn spans(lo: i32, hi: i32, extent: i32, cover_all: bool) -> std::ops::Range<i32> {
    if cover_all {
        (hi - extent + 1).max(0)..lo + 1
    } else {
        (lo - extent + 1).max(0)..hi + 1
    }
}

/// One way an item could explain a component: its shape, its cells still to
/// clear, and a weight.
type Placement = ((i32, i32), u128, i64);

/// Every placement of a `class` item explaining `comp` whose other cells are
/// all fogged; if no single item covers the whole component, those covering
/// part of it.
fn placements(class: ItemClass, comp: &[(i32, i32)], hidden: u128) -> Vec<Placement> {
    let (x_lo, x_hi) = (
        comp.iter().map(|c| c.0).min().unwrap_or(0),
        comp.iter().map(|c| c.0).max().unwrap_or(0),
    );
    let (y_lo, y_hi) = (
        comp.iter().map(|c| c.1).min().unwrap_or(0),
        comp.iter().map(|c| c.1).max().unwrap_or(0),
    );
    let comp_mask = mask(comp.iter().copied());
    let search = |cover_all: bool| {
        let mut found = Vec::new();
        for &((w, h), weight) in shapes(class) {
            for x in spans(x_lo, x_hi, w, cover_all) {
                for y in spans(y_lo, y_hi, h, cover_all) {
                    if x + w > N || y + h > N {
                        continue;
                    }
                    let mut item = 0u128;
                    for dx in 0..w {
                        for dy in 0..h {
                            item |= bit(x + dx, y + dy);
                        }
                    }
                    let fits = if cover_all {
                        comp_mask & !item == 0
                    } else {
                        comp_mask & item != 0
                    };
                    let rest = item & !comp_mask;
                    if fits && rest & !hidden == 0 {
                        found.push(((w, h), rest, weight));
                    }
                }
            }
        }
        found
    };
    let whole = search(true);
    if whole.is_empty() {
        search(false)
    } else {
        whole
    }
}

/// Per Big click (by cell index), the prior value of the items it finishes
/// on a fresh board (`complete`) and of those it only partly uncovers
/// (`partial`).
struct Prior {
    complete: [f64; CELLS],
    partial: [f64; CELLS],
}

fn prior() -> &'static Prior {
    static PRIOR: OnceLock<Prior> = OnceLock::new();
    PRIOR.get_or_init(build_prior)
}

fn build_prior() -> Prior {
    let mut prior = Prior {
        complete: [0.0; CELLS],
        partial: [0.0; CELLS],
    };
    let n = PRIOR_BOARDS as f64;
    for k in 0..PRIOR_BOARDS {
        let game = CrystalSphereMinigame::new(&mut Rng::from_seed(PRIOR_SEED + k), 0);
        // Which item covers each cell. An item left unplaced by a failed
        // populate attempt keeps position (0, 0), a cleared corner no item
        // can occupy.
        let mut item_at: [Option<usize>; CELLS] = [None; CELLS];
        for (i, item) in game.items().iter().enumerate() {
            if item.position == (0, 0) {
                continue;
            }
            let (w, h) = item.kind.size();
            for dx in 0..w {
                for dy in 0..h {
                    item_at[((item.position.0 + dx) * N + item.position.1 + dy) as usize] = Some(i);
                }
            }
        }
        let hidden = game.view(0).hidden;
        for (x, y) in cells(hidden) {
            let cleared = block(x, y) & hidden;
            let mut touched: Vec<usize> = Vec::new();
            for (cx, cy) in cells(cleared) {
                if let Some(i) = item_at[(cx * N + cy) as usize] {
                    if !touched.contains(&i) {
                        touched.push(i);
                    }
                }
            }
            for i in touched {
                let item = game.items()[i];
                let (w, h) = item.kind.size();
                let (px, py) = item.position;
                let left = (0..w)
                    .any(|dx| (0..h).any(|dy| hidden & !cleared & bit(px + dx, py + dy) != 0));
                let v = value(item.kind.class(), (w, h)) / n;
                let slot = if left {
                    &mut prior.partial
                } else {
                    &mut prior.complete
                };
                slot[(x * N + y) as usize] += v;
            }
        }
    }
    prior
}

/// A partly shown item: its class and its open placements `(shape, rest,
/// weight share)`.
type Open = (ItemClass, Vec<((i32, i32), u128, f64)>);

/// The digger's pick for this screen: a Big-tool cell, or `None` when no
/// cell is fogged or no divination is left.
pub fn pick(view: &SphereView) -> Option<(i32, i32)> {
    if view.hidden == 0 || view.remaining == 0 {
        return None;
    }
    let digger = Digger::of(view);
    let mut candidates = digger.relic_keeping();
    if candidates.is_empty() {
        // Only a Small click keeps the relic: the digger plays Big anyway.
        candidates = cells(view.hidden).collect();
    }
    let (items, claimed) = digger.open_items();
    let prior = prior();
    let mut best = None;
    let mut best_score = f64::NEG_INFINITY;
    for (x, y) in candidates {
        let s = digger.score((x, y, true), &items, claimed, prior);
        if s > best_score {
            best = Some((x, y));
            best_score = s;
        }
    }
    best.or_else(|| cells(view.hidden).next())
}

/// The digger's settings for one screen.
struct Digger<'v> {
    view: &'v SphereView,
    remaining: i64,
    chase: f64,
    solved: bool,
}

impl<'v> Digger<'v> {
    /// With five or more divinations at the start it chases items and keeps
    /// the relic guaranteed; with fewer it does neither.
    fn of(view: &'v SphereView) -> Self {
        let many = view.total >= CHASE_FROM;
        Self {
            view,
            remaining: i64::from(view.remaining),
            chase: if many { CHASE } else { 0.0 },
            solved: many,
        }
    }

    fn open_items(&self) -> (Vec<Open>, u128) {
        let mut items = Vec::new();
        let mut claimed = 0u128;
        for (class, comp) in components(&self.view.shown) {
            let places = placements(class, &comp, self.view.hidden);
            let total: i64 = places.iter().map(|p| p.2).sum();
            if total == 0 {
                continue;
            }
            let open: Vec<((i32, i32), u128, f64)> = places
                .into_iter()
                .filter(|p| p.1 != 0)
                .map(|(shape, rest, w)| (shape, rest, w as f64 / total as f64))
                .collect();
            for (_, rest, _) in &open {
                claimed |= rest;
            }
            items.push((class, open));
        }
        (items, claimed)
    }

    /// The value of the open items a click finishes, plus (when chasing) a
    /// pro-rata share of those it advances that the divinations left could
    /// still finish.
    fn finish_score(&self, items: &[Open], cleared: u128) -> f64 {
        let mut score = 0.0;
        let cleared_count = i64::from(cleared.count_ones());
        for (class, places) in items {
            for &(shape, rest, w) in places {
                let v = value(*class, shape);
                let rest_count = i64::from(rest.count_ones());
                if rest & !cleared == 0 {
                    score += w * v;
                } else if self.chase != 0.0
                    && v > 0.0
                    && self.remaining > 1
                    && rest_count <= 7 * (self.remaining - 1) + cleared_count
                {
                    score += self.chase * w * v * f64::from((rest & cleared).count_ones())
                        / rest_count as f64;
                }
            }
        }
        score
    }

    fn score(&self, click: Click, items: &[Open], claimed: u128, prior: &Prior) -> f64 {
        let reach = area(click);
        let cleared = reach & self.view.hidden;
        let mut score = self.finish_score(items, cleared);
        let share = f64::from((cleared & !claimed).count_ones()) / f64::from(reach.count_ones());
        let index = (click.0 * N + click.1) as usize;
        score += share * prior.complete[index];
        if self.remaining > 1 {
            score += GAMMA * share * prior.partial[index];
        }
        score - 1e-3 * f64::from(cleared.count_ones())
    }

    /// The Big clicks that keep the relic guaranteed, when it is and the
    /// digger keeps it; otherwise every fogged cell.
    fn relic_keeping(&self) -> Vec<(i32, i32)> {
        let all = || cells(self.view.hidden).collect();
        if !self.solved {
            return all();
        }
        let solver = solver();
        let (hidden, live) = solver.consistent(self.view);
        if hidden == solver.fresh && self.remaining >= 5 {
            return cells(hidden)
                .filter(|&(x, y)| fresh_keeps(self.remaining, (x, y, true)))
                .collect();
        }
        let live: Vec<u8> = live
            .into_iter()
            .filter(|&p| solver.pmask[usize::from(p)] & hidden != 0)
            .collect();
        if live.is_empty() || !solver.guaranteed(hidden, &live, self.remaining) {
            return all();
        }
        cells(hidden)
            .filter(|&(x, y)| solver.keeps(hidden, &live, self.remaining, (x, y, true)))
            .collect()
    }
}

/// The relic-keeping first clicks on a fresh board, solved once: with six
/// divinations every click keeps the relic guaranteed, with five only a Big
/// click at one of four cells.
fn fresh_keeps(digs: i64, click: Click) -> bool {
    if digs >= 6 {
        return true;
    }
    matches!(click, (4 | 6, 4 | 6, true))
}

/// The exact relic solver: the relic's placements on the fresh board (it is
/// placed first, so any 4x4 inside the fog) as cell masks.
struct Solver {
    fresh: u128,
    pmask: Vec<u128>,
}

/// Memo for [`Solver::guaranteed`], per thread.
type Memo = HashMap<(u128, Vec<u8>, i64), bool>;

thread_local! {
    // Unbounded by design: up to `MEMO_CAP` entries before it is cleared.
    #[allow(clippy::disallowed_methods)]
    static GUARANTEED: RefCell<Memo> = RefCell::new(HashMap::new());
}

fn solver() -> &'static Solver {
    static SOLVER: OnceLock<Solver> = OnceLock::new();
    SOLVER.get_or_init(|| {
        let fresh = fresh_hidden();
        let (w, h) = (4, 4);
        let mut pmask = Vec::new();
        for x in 0..=N - w {
            for y in 0..=N - h {
                let mut m = 0u128;
                for dx in 0..w {
                    for dy in 0..h {
                        m |= bit(x + dx, y + dy);
                    }
                }
                if m & !fresh == 0 {
                    pmask.push(m);
                }
            }
        }
        Solver { fresh, pmask }
    })
}

impl Solver {
    /// Every Big block centred on a cell of `mask`, joined.
    fn dilate(mask: u128) -> u128 {
        cells(mask).fold(0, |m, (x, y)| m | block(x, y))
    }

    /// A lower bound on the clicks `part` needs: cells pairwise at Chebyshev
    /// distance 3 or more cannot share a click.
    fn need(part: u128) -> i64 {
        let mut apart: Vec<(i32, i32)> = Vec::new();
        for c in cells(part) {
            if apart
                .iter()
                .all(|p| (c.0 - p.0).abs().max((c.1 - p.1).abs()) >= 3)
            {
                apart.push(c);
            }
        }
        apart.len() as i64
    }

    /// The fog and the relic placements the screen still allows.
    fn consistent(&self, view: &SphereView) -> (u128, Vec<u8>) {
        let hidden = view.hidden;
        let relic = mask(
            view.shown
                .iter()
                .filter(|(_, class)| *class == ItemClass::Relic)
                .map(|(cell, _)| *cell),
        );
        let cleared = !hidden;
        let live = (0..self.pmask.len())
            .filter(|&p| self.pmask[p] & relic == relic && self.pmask[p] & cleared & !relic == 0)
            .map(|p| p as u8)
            .collect();
        (hidden, live)
    }

    /// The clicks that touch a live placement's fogged cells: Big at each
    /// fogged cell near one, then Small at each such cell.
    fn relic_clicks(&self, hidden: u128, live: &[u8]) -> Vec<Click> {
        let union = live
            .iter()
            .fold(0, |u, &p| u | (self.pmask[usize::from(p)] & hidden));
        cells(Self::dilate(union) & hidden)
            .map(|(x, y)| (x, y, true))
            .chain(cells(union).map(|(x, y)| (x, y, false)))
            .collect()
    }

    /// The live placements grouped by what a click's cleared cells would
    /// show of them, in first-seen order.
    fn split(&self, cleared: u128, live: &[u8]) -> Vec<Vec<u8>> {
        let mut groups: Vec<(u128, Vec<u8>)> = Vec::new();
        for &p in live {
            let seen = self.pmask[usize::from(p)] & cleared;
            match groups.iter_mut().find(|(g, _)| *g == seen) {
                Some(group) => group.1.push(p),
                None => groups.push((seen, vec![p])),
            }
        }
        groups.into_iter().map(|(_, g)| g).collect()
    }

    /// Whether `digs` divinations finish the relic whichever of
    /// `placements` it is.
    fn guaranteed(&self, hidden: u128, placements: &[u8], digs: i64) -> bool {
        let live: Vec<u8> = placements
            .iter()
            .copied()
            .filter(|&p| self.pmask[usize::from(p)] & hidden != 0)
            .collect();
        if live.is_empty() {
            return true;
        }
        if live
            .iter()
            .any(|&p| Self::need(self.pmask[usize::from(p)] & hidden) > digs)
        {
            return false;
        }
        let union = live.iter().fold(0, |u, &p| u | self.pmask[usize::from(p)]);
        let key = (hidden & Self::dilate(union), live, digs);
        if let Some(hit) = GUARANTEED.with(|memo| memo.borrow().get(&key).copied()) {
            return hit;
        }
        let hit = self
            .relic_clicks(hidden, &key.1)
            .into_iter()
            .any(|click| self.keeps(hidden, &key.1, digs, click));
        GUARANTEED.with(|memo| {
            let mut memo = memo.borrow_mut();
            if memo.len() > MEMO_CAP {
                memo.clear();
            }
            memo.insert(key, hit);
        });
        hit
    }

    /// Whether `click` leaves the relic guaranteed with one divination fewer,
    /// whatever it shows.
    fn keeps(&self, hidden: u128, live: &[u8], digs: i64, click: Click) -> bool {
        let cleared = area(click) & hidden;
        if cleared == 0 || digs < 1 {
            return false;
        }
        self.split(cleared, live)
            .iter()
            .all(|group| self.guaranteed(hidden & !cleared, group, digs - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fog `fresh_hidden` computes is the fog the constructor leaves.
    #[test]
    fn fresh_hidden_matches_the_constructor() {
        let game = CrystalSphereMinigame::new(&mut Rng::from_seed(1), 6);
        assert_eq!(fresh_hidden(), game.view(6).hidden);
    }
}
