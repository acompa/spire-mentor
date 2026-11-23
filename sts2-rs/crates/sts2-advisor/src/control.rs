//! The control plane's state, shared between the advisor loop and the HTTP
//! handler threads (GH #1426 step 4; was `realgame_control.ControlPlane`
//! minus its HTTP server, which the advisor binary hosts).
//!
//! The advisor owns the single connection to the mod, so anything else that
//! wants to talk to the live game goes through here. One command at a time:
//! the bridge's rewind verbs are fire-and-forget with broadcast replies, so a
//! command registers a waiter, sends the verb, and blocks (bounded) until the
//! advisor's receive loop routes the matching reply back.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::livetree::LiveRunTree;
use crate::py::{get, truthy};

/// The advisor's side of the bridge connection.
pub trait Link: Send + Sync {
    /// Send one message to the mod. `echo_state_id`: stamp the last
    /// request's `request_id` / `state_id` where the message has none (an
    /// advice reply does; a rewind command does not).
    ///
    /// # Errors
    ///
    /// The connection is gone.
    fn send(&self, msg: &Value, echo_state_id: bool) -> std::io::Result<()>;

    /// Whether a newer `advice_request` for the same surface is already
    /// queued (the mod drops any reply to the older one).
    fn newer_advice_request_pending(&self, advice_for: &Value) -> bool;
}

/// The rewind-lane broadcasts the control plane routes.
pub const BRIDGE_MESSAGE_TYPES: [&str; 5] = [
    "snapshots",
    "snapshot_saved",
    "snapshot_restored",
    "combat_undone",
    "error",
];

#[derive(Debug, Default)]
struct Waiter {
    payload: Mutex<Option<Value>>,
    ready: Condvar,
}

impl Waiter {
    fn fulfill(&self, payload: Value) {
        *self
            .payload
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(payload);
        self.ready.notify_all();
    }

    fn wait(&self, timeout: Duration) -> Option<Value> {
        let guard = self
            .payload
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut guard, _) = self
            .ready
            .wait_timeout_while(guard, timeout, |p| p.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.take()
    }
}

#[derive(Default)]
struct Inner {
    link: Option<Arc<dyn Link>>,
    status: Map<String, Value>,
    waiters: BTreeMap<String, Arc<Waiter>>,
    last_state: Option<Value>,
    /// Where the workbench run logs go (`GET /logs` lists it); `None` =
    /// the directory of the current log, if there is one.
    logs_dir: Option<PathBuf>,
}

/// State shared between the advisor loop and the HTTP handler threads.
pub struct ControlState {
    pub tree: Arc<LiveRunTree>,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for ControlState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlState").finish_non_exhaustive()
    }
}

impl ControlState {
    #[must_use]
    pub fn new(tree: Arc<LiveRunTree>) -> Self {
        let mut status = Map::new();
        status.insert("connected".into(), json!(false));
        Self {
            tree,
            inner: Mutex::new(Inner {
                status,
                ..Inner::default()
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // -- called from the advisor loop ----------------------------------

    pub fn set_link(&self, link: Option<Arc<dyn Link>>) {
        let mut t = self.lock();
        t.status.insert("connected".into(), json!(link.is_some()));
        t.link = link;
    }

    /// Track the latest run envelope for `/status` (any message type), and
    /// the latest decision position for `/state`.
    pub fn note_message(&self, msg: &Value) {
        let rs = get(msg, "run_state");
        let mtype = get(msg, "type").as_str();
        let mut t = self.lock();
        if mtype == Some("advice_request") {
            t.last_state = Some(msg.clone());
        } else if matches!(mtype, Some("game_over" | "run_complete")) {
            t.last_state = None;
        }
        let keep = |t: &Inner, key: &str| t.status.get(key).cloned().unwrap_or(Value::Null);
        let or_keep = |t: &Inner, v: &Value, key: &str| {
            if truthy(v) {
                v.clone()
            } else {
                keep(t, key)
            }
        };
        // `msg.get(k, status.get(k))`: present-but-null wins over the old value.
        let present_or_keep = |t: &Inner, key: &str| {
            msg.as_object()
                .and_then(|m| m.get(key))
                .cloned()
                .unwrap_or_else(|| keep(t, key))
        };
        let update = [
            ("connected", json!(true)),
            ("character", or_keep(&t, get(rs, "character"), "character")),
            ("seed", or_keep(&t, get(msg, "seed"), "seed")),
            ("floor", present_or_keep(&t, "floor")),
            ("act", present_or_keep(&t, "act")),
            ("state_id", present_or_keep(&t, "state_id")),
        ];
        for (k, v) in update {
            t.status.insert(k.into(), v);
        }
    }

    /// The latest `advice_request` (`/state`), or `None`.
    #[must_use]
    pub fn last_state(&self) -> Option<Value> {
        self.lock().last_state.clone()
    }

    pub fn set_log_path(&self, path: Option<&str>) {
        self.lock().status.insert("log_path".into(), json!(path));
    }

    /// The directory the workbench run logs are written to, so `GET /logs`
    /// lists earlier sessions' runs before this one opens a log.
    pub fn set_logs_dir(&self, dir: Option<&Path>) {
        self.lock().logs_dir = dir.map(Path::to_path_buf);
    }

    /// Route a rewind-lane broadcast from the mod ([`BRIDGE_MESSAGE_TYPES`]).
    pub fn on_bridge_message(&self, msg: &Value) {
        match get(msg, "type").as_str() {
            Some("snapshot_saved") => self.tree.on_snapshot_saved(msg),
            Some("snapshots") => self.fulfill("snapshots", msg),
            Some("snapshot_restored") => {
                self.tree.on_snapshot_restored(msg);
                self.fulfill("restore", msg);
            }
            Some("combat_undone") => {
                self.tree.on_combat_undone(msg);
                self.fulfill("restore", msg);
            }
            Some("error")
                if !get(msg, "snapshot_id").is_null()
                    || crate::py::str_of_or_empty(get(msg, "error")).starts_with("undo_") =>
            {
                self.fulfill("restore", msg);
            }
            _ => {}
        }
    }

    fn fulfill(&self, kind: &str, payload: &Value) {
        let waiter = self.lock().waiters.remove(kind);
        if let Some(w) = waiter {
            w.fulfill(payload.clone());
        }
    }

    // -- command round trips (HTTP handler threads) --------------------

    /// Send `action` and wait for the broadcast that answers it. `None` when
    /// there is no connection or the send failed (the log line in `warn`).
    pub fn round_trip(
        &self,
        kind: &str,
        action: &Value,
        timeout: Duration,
        warn: &dyn Fn(&str),
    ) -> Option<Value> {
        let (link, waiter) = {
            let mut t = self.lock();
            let link = t.link.clone()?;
            let waiter = Arc::new(Waiter::default());
            t.waiters.insert(kind.to_string(), waiter.clone());
            (link, waiter)
        };
        if let Err(e) = link.send(action, false) {
            self.lock().waiters.remove(kind);
            warn(&format!(
                "control command {} failed: {e}",
                crate::py::str_of(get(action, "action"))
            ));
            return None;
        }
        if let Some(payload) = waiter.wait(timeout) {
            return Some(payload);
        }
        self.lock().waiters.remove(kind);
        Some(json!({"type": "error", "error": "timeout", "detail": format!("{kind} timed out")}))
    }

    pub fn list_snapshots(&self, warn: &dyn Fn(&str)) -> Option<Value> {
        self.round_trip(
            "snapshots",
            &json!({"action": "list_snapshots"}),
            Duration::from_secs(5),
            warn,
        )
    }

    pub fn restore(&self, snapshot_id: &str, warn: &dyn Fn(&str)) -> Option<Value> {
        self.round_trip(
            "restore",
            &json!({"action": "restore_snapshot", "snapshot_id": snapshot_id}),
            Duration::from_secs(20),
            warn,
        )
    }

    /// A persisted-tape replay reloads a room AND fast-forwards a whole
    /// combat prefix: it gets more time than a plain restore.
    pub fn undo_turn(
        &self,
        turn: i64,
        tape_id: Option<&str>,
        warn: &dyn Fn(&str),
    ) -> Option<Value> {
        let mut action = json!({"action": "undo_turn", "turn": turn});
        if let Some(t) = tape_id.filter(|t| !t.is_empty()) {
            action["tape_id"] = json!(t);
        }
        self.round_trip("restore", &action, Duration::from_secs(60), warn)
    }

    #[must_use]
    pub fn status(&self) -> Value {
        Value::Object(self.lock().status.clone())
    }

    /// `GET /log`: the current run log as `(HTTP status, payload)`; the text
    /// is cut at the last complete line (the advisor appends while we read).
    #[must_use]
    pub fn live_log(&self, stat_only: bool) -> (u16, Value) {
        let path = self
            .lock()
            .status
            .get("log_path")
            .and_then(Value::as_str)
            .map(str::to_string);
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            return (
                404,
                json!({"error": "no_log", "detail": "the advisor has not opened a run log yet"}),
            );
        };
        read_log(Path::new(&path), stat_only)
    }

    fn logs_dir(&self) -> Option<PathBuf> {
        let t = self.lock();
        t.logs_dir.clone().or_else(|| {
            t.status
                .get("log_path")
                .and_then(Value::as_str)
                .filter(|p| !p.is_empty())
                .and_then(|p| Path::new(p).parent().map(Path::to_path_buf))
        })
    }

    /// `GET /logs`: every run log in the logs directory (`{name, size,
    /// mtime}`, newest first), so the bundled page can import the runs it
    /// did not watch live. Only names the page can store are listed.
    #[must_use]
    pub fn run_logs(&self) -> Value {
        let mut rows: Vec<(f64, Value)> = Vec::new();
        let entries = self
            .logs_dir()
            .and_then(|d| std::fs::read_dir(d).ok())
            .into_iter()
            .flatten()
            .flatten();
        for e in entries {
            let Some(name) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Ok(st) = e.metadata() else { continue };
            if !st.is_file() || !is_run_log_name(&name) {
                continue;
            }
            let mtime = mtime_of(&st);
            rows.push((
                mtime,
                json!({"name": name, "size": st.len(), "mtime": mtime}),
            ));
        }
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        json!({"logs": rows.into_iter().map(|(_, v)| v).collect::<Vec<_>>()})
    }

    /// `GET /log?name=<file>`: one log of [`Self::run_logs`], shaped like
    /// [`Self::live_log`]. A name that is not a bare run-log filename is a
    /// 400, so the route cannot read outside the logs directory.
    #[must_use]
    pub fn run_log(&self, name: &str, stat_only: bool) -> (u16, Value) {
        if !is_run_log_name(name) {
            return (
                400,
                json!({"error": "bad_name", "detail": "name must be a bare .jsonl filename"}),
            );
        }
        let Some(dir) = self.logs_dir() else {
            return (
                404,
                json!({"error": "no_log", "detail": "the advisor has no logs directory"}),
            );
        };
        read_log(&dir.join(name), stat_only)
    }
}

/// A filename the bundled page's run store takes (`engineRoutes.ts`
/// `RUN_FILENAME`): no path parts, no leading dot, `.jsonl`.
fn is_run_log_name(name: &str) -> bool {
    // Exactly `.jsonl`, lowercase: the page's pattern is case-sensitive.
    name.strip_suffix(".jsonl")
        .is_some_and(|stem| !stem.is_empty())
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn mtime_of(st: &std::fs::Metadata) -> f64 {
    st.modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64())
}

/// A log file as the `/log` routes answer it.
fn read_log(file: &Path, stat_only: bool) -> (u16, Value) {
    let read = file.metadata().and_then(|st| {
        let raw = if stat_only {
            Vec::new()
        } else {
            std::fs::read(file)?
        };
        Ok((st, raw))
    });
    let (st, raw) = match read {
        Ok(r) => r,
        Err(e) => return (404, json!({"error": "no_log", "detail": e.to_string()})),
    };
    let name = file
        .to_str()
        .and_then(|p| crate::workbench_log::run_log_name(Some(p)))
        .unwrap_or_default();
    let mut payload = json!({"name": name, "size": st.len(), "mtime": mtime_of(&st)});
    if !stat_only {
        let end = raw.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        payload["text"] = json!(String::from_utf8_lossy(&raw[..end]));
    }
    (200, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo(Arc<ControlState>);

    impl Link for Echo {
        fn send(&self, msg: &Value, _echo: bool) -> std::io::Result<()> {
            // The mod answers a restore with a broadcast, from its own thread.
            let control = self.0.clone();
            let id = get(msg, "snapshot_id").clone();
            std::thread::spawn(move || {
                control.on_bridge_message(&json!({"type": "snapshot_restored", "snapshot_id": id}));
            });
            Ok(())
        }

        fn newer_advice_request_pending(&self, _advice_for: &Value) -> bool {
            false
        }
    }

    #[test]
    fn a_restore_waits_for_its_broadcast() {
        let control = Arc::new(ControlState::new(Arc::new(LiveRunTree::new())));
        assert_eq!(control.restore("s1", &|_| {}), None, "no connection");
        control.set_link(Some(Arc::new(Echo(control.clone()))));
        let got = control.restore("s1", &|_| {}).unwrap();
        assert_eq!(got["type"], "snapshot_restored");
        assert_eq!(got["snapshot_id"], "s1");
    }

    #[test]
    fn status_keeps_what_a_message_does_not_say() {
        let control = ControlState::new(Arc::new(LiveRunTree::new()));
        control.note_message(&json!({"type": "advice_request", "seed": "S", "floor": 3,
            "run_state": {"character": "IRONCLAD"}}));
        control.note_message(&json!({"type": "human_action", "floor": null}));
        let st = control.status();
        assert_eq!(st["character"], "IRONCLAD");
        assert_eq!(st["seed"], "S");
        assert_eq!(st["floor"], Value::Null, "a present null replaces");
        assert!(control.last_state().is_some());
        control.note_message(&json!({"type": "game_over"}));
        assert!(control.last_state().is_none());
        assert_eq!(control.live_log(false).0, 404);
    }

    #[test]
    fn earlier_run_logs_are_listed_and_served_from_the_logs_dir_only() {
        let dir = std::env::temp_dir().join(format!("sts2-control-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a_run.jsonl"), "{\"a\":1}\n").unwrap();
        std::fs::write(dir.join("b_run.jsonl"), "{\"b\":1}\n{\"torn").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join(".hidden.jsonl"), "x\n").unwrap();
        std::fs::write(dir.join("sub").join("c.jsonl"), "x\n").unwrap();
        let control = ControlState::new(Arc::new(LiveRunTree::new()));
        assert_eq!(control.run_logs(), json!({"logs": []}), "no dir known yet");
        assert_eq!(control.run_log("a_run.jsonl", false).0, 404);
        control.set_logs_dir(Some(&dir));
        let mut names: Vec<String> = control.run_logs()["logs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap().to_string())
            .collect();
        names.sort();
        assert_eq!(names, ["a_run.jsonl", "b_run.jsonl"]);
        let (code, b) = control.run_log("b_run.jsonl", false);
        assert_eq!(code, 200);
        assert_eq!(b["text"], "{\"b\":1}\n", "cut at the last complete line");
        assert!(control.run_log("b_run.jsonl", true).1.get("text").is_none());
        for bad in [
            "../a_run.jsonl",
            "sub/c.jsonl",
            ".hidden.jsonl",
            "notes.txt",
            "",
        ] {
            assert_eq!(control.run_log(bad, false).0, 400, "{bad}");
        }
        assert_eq!(control.run_log("gone.jsonl", false).0, 404);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
