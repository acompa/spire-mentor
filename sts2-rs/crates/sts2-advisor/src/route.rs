//! The map route under the advisor's map advice (GH #1426; was in
//! `realgame_advisor.py`): the act's graph, the walk from the chosen node,
//! and the HP the player arrives with at each hop.
//!
//! A PROJECTION, not a plan: deck, gold, relics and potions stay frozen,
//! and the HP deltas are the run AI's flat per-act expectations
//! (`sts2_runai::eval` projection tables, the numbers its own planner
//! uses), not this deck against that encounter. [`route_note`] says so on
//! the HUD.

use serde_json::{json, Value};

use crate::i18n::trf;
use crate::py;

/// A defensive bound on a projected route; the act's last row ends it first.
pub const MAX_PROJECTED_HOPS: usize = 32;

/// A map coordinate `(col, row)`.
pub type Coord = (i64, i64);

/// `int(v)` for a coordinate field, or `None` where Python raises.
fn coord_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite())
                .map(|f| f.trunc() as i64)
        }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `(int(n["col"]), int(n["row"]))`, or `None` where Python raises.
#[must_use]
pub fn coord_of(n: &Value) -> Option<Coord> {
    Some((coord_int(n.get("col")?)?, coord_int(n.get("row")?)?))
}

/// The act's map by coordinate, in the order the mod lists it, the act's
/// start and boss included (`route_graph`).
///
/// The mod's `map.nodes` hold rows 1..N only: the act's start (row 0, the
/// Ancient) shows up just as the offered node at an act's entrance, and the
/// boss just as `map.boss`. The game links the start to every row-1 node
/// and the last row to the boss (`StandardActMap.cs:221-232`); the last
/// row's `children` already name the boss.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    nodes: Vec<(Coord, Value)>,
}

impl Graph {
    /// The node at `c`.
    #[must_use]
    pub fn get(&self, c: Coord) -> Option<&Value> {
        self.nodes.iter().find(|(k, _)| *k == c).map(|(_, v)| v)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    fn insert(&mut self, c: Coord, v: Value) {
        match self.nodes.iter_mut().find(|(k, _)| *k == c) {
            Some(slot) => slot.1 = v,
            None => self.nodes.push((c, v)),
        }
    }

    /// The children of the node at `c` that are on the graph.
    #[must_use]
    pub fn children(&self, c: Coord) -> Vec<Coord> {
        self.get(c)
            .map(|n| {
                py::list(n, "children")
                    .iter()
                    .filter_map(coord_of)
                    .filter(|k| self.get(*k).is_some())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// `route_graph(state)`.
#[must_use]
pub fn route_graph(state: &Value) -> Graph {
    let m = py::get(state, "map");
    let mut g = Graph::default();
    for n in py::list(m, "nodes") {
        if let Some(c) = coord_of(n) {
            g.insert(c, n.clone());
        }
    }
    if g.is_empty() {
        return g;
    }
    let boss = py::get(m, "boss");
    if boss.is_object() {
        if let Some(c) = coord_of(boss) {
            if g.get(c).is_none() {
                g.insert(
                    c,
                    json!({"col": c.0, "row": c.1, "type": "Boss", "children": []}),
                );
            }
        }
    }
    let first_row = g.nodes.iter().map(|((_, r), _)| *r).min().unwrap_or(0);
    let first: Vec<Value> = g
        .nodes
        .iter()
        .filter(|((_, r), _)| *r == first_row)
        .map(|((c, r), _)| json!({"col": c, "row": r}))
        .collect();
    for n in py::list(state, "nodes") {
        let Some(c) = coord_of(n) else { continue };
        if g.get(c).is_none() && c.1 < first_row {
            let mut start = n.clone();
            if let Value::Object(o) = &mut start {
                o.insert("children".into(), Value::Array(first.clone()));
            }
            g.insert(c, start);
        }
    }
    g
}

/// The run AI's expected-HP-cost tables, as the engine embeds them.
#[must_use]
pub fn hp_table() -> &'static sts2_runai::eval::ProjectionTables {
    &sts2_runai::eval::Weights::embedded().projection
}

/// HP after `room` resolves, from the run AI's expected-cost tables
/// (`_project_hp`). A drawing, not a death sentence: never below 1.
#[must_use]
pub fn project_hp(
    hp: f64,
    max_hp: f64,
    room: &Value,
    act: i64,
    t: &sts2_runai::eval::ProjectionTables,
) -> f64 {
    let slot = usize::try_from((act - 1).clamp(0, 2)).unwrap_or(0);
    // The bridge sends the C# `MapPointType` enum name (`RestSite`); the
    // training-side obs uses the snake form (`rest_site`). Fold both.
    let kind = py::str_of(room).to_lowercase().replace('_', "");
    let mut hp = match kind.as_str() {
        "monster" => hp - t.monster_hp_loss[slot],
        "elite" => hp - t.elite_hp_loss[slot],
        // Event rooms are a chance node; the table's flat prior is the
        // expectation over the pool, matching `projection.rs`.
        "ancient" => hp - t.event_hp_loss,
        "unknown" => hp - t.unknown_hp_loss,
        // The engine's own floor(max_hp * 0.3).
        "restsite" => hp + (max_hp * t.rest_heal_frac).floor(),
        _ => hp,
    };
    if max_hp > 0.0 {
        hp = hp.min(max_hp);
    }
    hp.max(1.0)
}

/// Python's `round(x)`: to the nearest integer, ties to even.
#[must_use]
pub fn round_half_even(x: f64) -> i64 {
    py::round(x, 0) as i64
}

/// The one-line route caption under the map advice.
#[must_use]
pub fn route_note(path: &[Value]) -> String {
    let mut note = trf(" · route {n} floors", &[("n", &path.len())]);
    if let Some(arrival) = path
        .last()
        .and_then(|s| s.get("hp"))
        .filter(|v| !v.is_null())
    {
        // Naming the frozen axes is the point: a player who knows only HP
        // moves can discount the late hops themselves.
        note.push_str(&trf(
            " → {hp} HP (deck/gold/relics frozen)",
            &[("hp", &py::str_of(arrival))],
        ));
    }
    note
}

/// Walk the route from `chosen`: follow single-child chains, and at each
/// fork ask `pick(hop, at, children, hp, walked)` which child comes next
/// (`None` ends the walk); `walked` is every node of the route so far, `at`
/// last. HP is walked forward through [`project_hp`] between hops,
/// so each fork is answered at the HP the player would arrive with.
pub fn walk(
    graph: &Graph,
    chosen: Coord,
    act: i64,
    mut hp: f64,
    max_hp: f64,
    mut pick: impl FnMut(usize, Coord, &[Coord], f64, &[Coord]) -> Option<Coord>,
) -> Vec<Value> {
    let table = hp_table();
    let mut path = Vec::new();
    let mut seen: Vec<Coord> = Vec::new();
    let mut cur = chosen;
    for hop in 0..MAX_PROJECTED_HOPS {
        let Some(node) = graph.get(cur) else { break };
        if seen.contains(&cur) {
            break;
        }
        seen.push(cur);
        let room = node.get("type").cloned().unwrap_or(json!("?"));
        path.push(json!({"col": cur.0, "row": cur.1, "type": room, "hop": hop, "hp": round_half_even(hp)}));
        hp = project_hp(hp, max_hp, &room, act, table);
        let children = graph.children(cur);
        cur = match children.as_slice() {
            [] => break, // the act's last row
            [only] => *only,
            _ => match pick(hop, cur, &children, hp, &seen) {
                Some(next) => next,
                None => break,
            },
        };
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_graph_adds_the_start_and_the_boss() {
        let state = json!({
            "map": {
                "nodes": [
                    {"col": 1, "row": 1, "type": "Monster", "children": [{"col": 1, "row": 2}]},
                    {"col": 3, "row": 1, "type": "Elite", "children": [{"col": 1, "row": 2}]},
                    {"col": 1, "row": 2, "type": "RestSite", "children": [{"col": 3, "row": 3}]},
                ],
                "boss": {"col": 3, "row": 3},
            },
            "nodes": [{"col": 3, "row": 0, "type": "Ancient"}],
        });
        let g = route_graph(&state);
        assert_eq!(g.children((3, 0)), vec![(1, 1), (3, 1)]);
        assert_eq!(g.get((3, 3)).unwrap()["type"], "Boss");
        let path = walk(&g, (3, 0), 1, 50.0, 80.0, |_, _, kids, _, _| Some(kids[1]));
        let rooms: Vec<&str> = path.iter().map(|s| s["type"].as_str().unwrap()).collect();
        assert_eq!(rooms, ["Ancient", "Elite", "RestSite", "Boss"]);
    }

    #[test]
    fn hp_never_drops_below_one_or_rises_past_max() {
        let t = hp_table();
        assert!((project_hp(2.0, 80.0, &json!("Elite"), 3, t) - 1.0).abs() < 1e-9);
        assert!(project_hp(79.0, 80.0, &json!("rest_site"), 1, t) <= 80.0);
        assert_eq!(round_half_even(2.5), 2);
        assert_eq!(
            route_note(&[json!({"hp": 40})]),
            " · route 1 floors → 40 HP (deck/gold/relics frozen)"
        );
    }
}
