//! The live game's branch tree for the advisor's control plane (GH #1426
//! step 4; was `realgame_livetree.LiveRunTree`).
//!
//! A thin tree over the events the advisor already sees: one node per
//! distinct decision the mod requested advice for, stamped with the mod's
//! snapshot ids (`snapshot_saved`) so the workbench's `BranchTree.tsx` can
//! offer the seekable ones as rewind targets. A `snapshot_restored` moves the
//! active pointer back to the node holding that snapshot; the next decision
//! then forks a child there, which is exactly the fork the tidy-tree draws.
//! It only OBSERVES a human-driven game and delegates seeks to the mod's
//! in-process snapshot restore.
//!
//! [`LiveRunTree::timeline`] is the `{nodes, active_path, active_leaf}` wire
//! shape the workbench's live views consume (`LiveNode` / `LiveTimeline`),
//! with two additive fields per node: `snapshot_id` and `seekable`.

use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::py::{self, get, truthy};

#[derive(Debug, Default)]
struct Tree {
    /// `(character, seed)` of the decisions' envelopes.
    run_key: Value,
    /// `run_start_time` of the mod's snapshot events.
    start_time: Value,
    /// Node id -> node, in id order.
    nodes: Vec<Map<String, Value>>,
    active: Option<usize>,
    pending_label: Option<usize>,
}

/// A JSON int as Python's `isinstance(v, int)` sees it.
fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

impl Tree {
    /// Two independent run identities: decision envelopes carry (character,
    /// seed), the mod's snapshot events `run_start_time`. A change in EITHER
    /// known identity means a new run; a first sighting just records it.
    fn reset(&mut self, run_key: Value, start_time: Value) {
        self.run_key = run_key;
        self.start_time = start_time;
        self.nodes.clear();
        self.active = None;
        self.pending_label = None;
        let root = self.new_node(None, "root", "run start", Value::Null, Value::Null);
        self.active = Some(root);
    }

    fn new_node(
        &mut self,
        parent: Option<usize>,
        decision: &str,
        label: &str,
        floor: Value,
        act: Value,
    ) -> usize {
        let id = self.nodes.len();
        let idx = parent.map_or(0, |p| py::int_of(&self.nodes[p]["idx"]) + 1);
        let node = json!({
            "id": id,
            "parent_id": parent,
            "idx": idx,
            "decision": decision,
            "label": label,
            "details": [],
            "error": false,
            "child_ids": [],
            "floor": floor,
            "act": act,
            "snapshot_id": null,
            "seekable": false,
            "turn": null,
            "tape_id": null,
            "log_idx": null,
        });
        let Value::Object(node) = node else {
            unreachable!("a json object")
        };
        self.nodes.push(node);
        if let Some(p) = parent {
            if let Some(Value::Array(kids)) = self.nodes[p].get_mut("child_ids") {
                kids.push(json!(id));
            }
        }
        id
    }

    fn parent(&self, id: usize) -> Option<usize> {
        self.nodes[id]
            .get("parent_id")
            .and_then(Value::as_u64)
            .and_then(|p| usize::try_from(p).ok())
    }

    fn decision(&self, id: usize) -> &str {
        self.nodes[id]
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    fn maybe_switch_run(&mut self, run_key: Value) {
        if run_key.is_null() {
            return;
        }
        if self.run_key.is_null() {
            self.run_key = run_key;
        } else if !py::eq(&run_key, &self.run_key) {
            self.reset(run_key, Value::Null);
        }
    }

    fn maybe_switch_start_time(&mut self, start_time: &Value) {
        if start_time.is_null() {
            return;
        }
        if self.start_time.is_null() {
            self.start_time = start_time.clone();
        } else if !py::eq(start_time, &self.start_time) {
            self.reset(Value::Null, start_time.clone());
        }
    }

    fn path(&self) -> Vec<usize> {
        let mut path = Vec::new();
        let mut cur = self.active;
        while let Some(c) = cur {
            path.push(c);
            cur = self.parent(c);
        }
        path.reverse();
        path
    }
}

/// The branch tree of one live run. Every entry point may be called from the
/// advisor's socket loop or the control plane's threads.
#[derive(Debug)]
pub struct LiveRunTree {
    tree: Mutex<Tree>,
}

impl Default for LiveRunTree {
    fn default() -> Self {
        Self::new()
    }
}

fn run_key_of(msg: &Value) -> Value {
    let character = get(get(msg, "run_state"), "character");
    let seed = get(msg, "seed");
    if truthy(character) && truthy(seed) {
        json!([character, seed])
    } else {
        Value::Null
    }
}

impl LiveRunTree {
    #[must_use]
    pub fn new() -> Self {
        let mut tree = Tree::default();
        tree.reset(Value::Null, Value::Null);
        Self {
            tree: Mutex::new(tree),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Tree> {
        self.tree
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A new advised decision: append a node under the active pointer.
    /// `log_idx` is the replay step of the decision's `state` line in the run
    /// log: the node -> step join the coach's "open in workbench" links need.
    pub fn on_decision(&self, msg: &Value, advice_for: &str, log_idx: Option<i64>) {
        let mut t = self.lock();
        t.maybe_switch_run(run_key_of(msg));
        let mut label = advice_for.replace('_', " ");
        let floor = get(msg, "floor").clone();
        if !floor.is_null() {
            label = format!("f{} · {label}", py::str_of(&floor));
        }
        let active = t.active;
        let id = t.new_node(active, advice_for, &label, floor, get(msg, "act").clone());
        let node = &mut t.nodes[id];
        let turn = get(msg, "turn");
        if advice_for == "combat_action" && int(turn).is_some_and(|n| n >= 1) {
            // An undo_turn target: the mod can replay this combat's tape to
            // the start of this node's turn.
            node.insert("turn".into(), turn.clone());
            node.insert("seekable".into(), json!(true));
        }
        node.insert("request_id".into(), get(msg, "request_id").clone());
        // The game's own combat-log rows since the previous decision, RAW:
        // the workbench renders them when it serves the tree.
        let log = match get(msg, "combat_log") {
            Value::Array(rows) => Value::Array(rows.clone()),
            _ => json!([]),
        };
        node.insert("combat_log_raw".into(), log);
        node.insert(
            "player_id".into(),
            get(get(msg, "run_state"), "character").clone(),
        );
        node.insert("log_idx".into(), json!(log_idx));
        t.active = Some(id);
        t.pending_label = Some(id);
    }

    /// Refine a decision node's label with what the human actually did: the
    /// node the action ANSWERED (its `advice_request_id`), else the newest
    /// node still awaiting its label. Returns that node's id.
    pub fn on_human_action(&self, msg: &Value) -> Option<usize> {
        let detail = [get(msg, "detail"), get(msg, "action")]
            .into_iter()
            .find(|v| truthy(v))?
            .clone();
        let mut t = self.lock();
        let rid = get(msg, "advice_request_id");
        let mut node = None;
        if truthy(rid) {
            node = (0..t.nodes.len())
                .rev()
                .find(|&i| py::eq(t.nodes[i].get("request_id").unwrap_or(&Value::Null), rid));
        }
        if node.is_none() {
            node = t.pending_label.or(t.active);
        }
        t.pending_label = None;
        let id = node.filter(|&i| i < t.nodes.len())?;
        if t.parent(id).is_none() || matches!(t.decision(id), "room_entry" | "game_over") {
            return None;
        }
        let floor = t.nodes[id].get("floor").cloned().unwrap_or(Value::Null);
        let label = if floor.is_null() {
            py::str_of(&detail)
        } else {
            format!("f{} · {}", py::str_of(&floor), py::str_of(&detail))
        };
        t.nodes[id].insert("label".into(), json!(label));
        Some(id)
    }

    /// Stamp the mod's snapshot onto the tree: a room-entry snapshot becomes
    /// its own node; a post-combat one stamps the active node (and, with the
    /// fight's tape, every combat node of the fight).
    pub fn on_snapshot_saved(&self, msg: &Value) {
        let snapshot_id = get(msg, "snapshot_id");
        if !truthy(snapshot_id) {
            return;
        }
        let mut t = self.lock();
        t.maybe_switch_start_time(get(msg, "run_start_time"));
        if truthy(get(msg, "post_combat")) {
            let Some(active) = t.active else { return };
            if t.nodes[active]
                .get("snapshot_id")
                .is_none_or(Value::is_null)
            {
                t.nodes[active].insert("snapshot_id".into(), snapshot_id.clone());
                t.nodes[active].insert("seekable".into(), json!(true));
            }
            if truthy(get(msg, "has_tape")) {
                let mut cur = Some(active);
                while let Some(c) = cur {
                    if t.decision(c) == "room_entry" {
                        break;
                    }
                    if t.nodes[c].get("turn").is_some_and(|v| !v.is_null()) {
                        t.nodes[c].insert("tape_id".into(), snapshot_id.clone());
                    }
                    cur = t.parent(c);
                }
            }
            return;
        }
        let floor = get(msg, "floor").clone();
        let label = if floor.is_null() {
            "room entry".to_string()
        } else {
            format!("floor {}", py::str_of(&floor))
        };
        let active = t.active;
        let id = t.new_node(active, "room_entry", &label, floor, get(msg, "act").clone());
        t.nodes[id].insert("snapshot_id".into(), snapshot_id.clone());
        t.nodes[id].insert("seekable".into(), json!(true));
        t.active = Some(id);
    }

    /// The game rewound: move the active pointer to the snapshot's node.
    pub fn on_snapshot_restored(&self, msg: &Value) {
        let snapshot_id = get(msg, "snapshot_id");
        if !truthy(snapshot_id) {
            return;
        }
        let mut t = self.lock();
        if let Some(i) = (0..t.nodes.len()).find(|&i| {
            py::eq(
                t.nodes[i].get("snapshot_id").unwrap_or(&Value::Null),
                snapshot_id,
            )
        }) {
            t.active = Some(i);
        }
    }

    /// The game replayed a combat tape to the start of a turn: move the
    /// active pointer to the last node BEFORE that turn (with `tape_id`, of
    /// the persisted fight under that id; without it, of the current fight).
    pub fn on_combat_undone(&self, msg: &Value) {
        let Some(turn) = int(get(msg, "turn")).filter(|&n| n >= 1) else {
            return;
        };
        let mut t = self.lock();
        let tape_id = get(msg, "tape_id");
        let chain: Vec<usize> = if truthy(tape_id) {
            (0..t.nodes.len())
                .filter(|&i| py::eq(t.nodes[i].get("tape_id").unwrap_or(&Value::Null), tape_id))
                .collect()
        } else {
            let mut chain = Vec::new();
            let mut cur = t.active;
            while let Some(c) = cur {
                if t.decision(c) == "room_entry" {
                    break;
                }
                if t.nodes[c].get("turn").is_some_and(|v| !v.is_null()) {
                    chain.push(c);
                }
                cur = t.parent(c);
            }
            chain.reverse();
            chain
        };
        let Some(&first) = chain.first() else { return };
        let before = chain.iter().rev().find(|&&i| {
            t.nodes[i]
                .get("turn")
                .and_then(int)
                .is_some_and(|n| n < turn)
        });
        if let Some(&b) = before {
            t.active = Some(b);
            return;
        }
        // Nothing earlier in the fight: land on its room entry.
        let mut cur = t.parent(first);
        while let Some(c) = cur {
            if t.decision(c) == "room_entry" {
                t.active = Some(c);
                return;
            }
            cur = t.parent(c);
        }
    }

    /// The run ended: a `game_over` leaf under the active pointer.
    pub fn on_run_end(&self, msg: &Value) {
        let mut t = self.lock();
        let victory = truthy(get(msg, "victory"));
        let active = t.active;
        let id = t.new_node(
            active,
            "game_over",
            if victory { "victory" } else { "defeat" },
            get(msg, "floor").clone(),
            get(msg, "act").clone(),
        );
        t.nodes[id].insert("error".into(), json!(!victory));
        t.active = Some(id);
    }

    #[must_use]
    pub fn snapshot_id_of(&self, node_id: usize) -> Value {
        let t = self.lock();
        t.nodes
            .get(node_id)
            .and_then(|n| n.get("snapshot_id").cloned())
            .unwrap_or(Value::Null)
    }

    /// The bridge verb that lands the game where `node_id` says: a combat
    /// node with a turn replays its tape (`undo_turn`; the snapshot on a
    /// fight's last node is the POST-combat save), a room-entry or
    /// post-combat node restores its snapshot; `None` when nothing is exact.
    #[must_use]
    pub fn seek_plan(&self, node_id: usize) -> Option<Value> {
        let t = self.lock();
        let node = t.nodes.get(node_id)?;
        let turn = node.get("turn").unwrap_or(&Value::Null);
        if int(turn).is_some_and(|n| n >= 1) {
            let mut plan = json!({"action": "undo_turn", "turn": turn});
            let tape = node.get("tape_id").unwrap_or(&Value::Null);
            if truthy(tape) {
                plan["tape_id"] = tape.clone();
            }
            return Some(plan);
        }
        let snapshot = node.get("snapshot_id").unwrap_or(&Value::Null);
        truthy(snapshot).then(|| json!({"action": "restore_snapshot", "snapshot_id": snapshot}))
    }

    /// The run-log replay step of `node_id`.
    #[must_use]
    pub fn log_idx(&self, node_id: &Value) -> Value {
        let Some(id) = node_index(node_id) else {
            return Value::Null;
        };
        let t = self.lock();
        t.nodes
            .get(id)
            .and_then(|n| n.get("log_idx").cloned())
            .unwrap_or(Value::Null)
    }

    /// Node ids from the root to the active node.
    #[must_use]
    pub fn active_path(&self) -> Vec<usize> {
        self.lock().path()
    }

    /// The `LiveTimeline` wire shape (plus `snapshot_id` / `seekable`).
    #[must_use]
    pub fn timeline(&self) -> Value {
        let t = self.lock();
        json!({
            "nodes": t.nodes.iter().map(|n| Value::Object(n.clone())).collect::<Vec<_>>(),
            "active_path": t.path(),
            "active_leaf": t.active,
        })
    }
}

/// A node id as the control plane hands it over (any JSON value): the
/// index it names, or `None` when it names no node (negative, non-integer).
#[must_use]
pub fn node_index(node_id: &Value) -> Option<usize> {
    int(node_id).and_then(|i| usize::try_from(i).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restore_forks_the_next_decision_at_the_snapshot() {
        let tree = LiveRunTree::new();
        tree.on_snapshot_saved(&json!({"snapshot_id": "s1", "floor": 1}));
        let msg = json!({"floor": 1, "request_id": "a1", "turn": 1,
                         "run_state": {"character": "IRONCLAD"}, "seed": "X"});
        tree.on_decision(&msg, "combat_action", Some(3));
        assert_eq!(
            tree.on_human_action(&json!({"detail": "Bash", "advice_request_id": "a1"})),
            Some(2)
        );
        tree.on_snapshot_restored(&json!({"snapshot_id": "s1"}));
        tree.on_decision(&json!({"floor": 1}), "card_reward", None);
        let tl = tree.timeline();
        assert_eq!(tl["active_path"], json!([0, 1, 3]));
        assert_eq!(tl["nodes"][1]["child_ids"], json!([2, 3]));
        assert_eq!(tl["nodes"][2]["label"], "f1 · Bash");
        assert_eq!(
            tree.seek_plan(2),
            Some(json!({"action": "undo_turn", "turn": 1}))
        );
        assert_eq!(tree.log_idx(&json!(2)), json!(3));
    }
}
