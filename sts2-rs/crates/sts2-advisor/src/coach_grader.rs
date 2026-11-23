//! The live coach's grader (GH #1426 step 4; was `realgame_coach.CoachGrader`,
//! `AdviceCache`, `CoachEngine` and the executors).
//!
//! The advisor computes advice for every decision whether or not the mod
//! shows it. The grader keeps that advice per request ([`AdviceCache`]) and
//! grades each `human_action` against the advice for exactly the position it
//! was taken from. Combat is graded PER TURN: each action is diffed with
//! `regret_review::assess_decision` (after a forced-root top-up when the
//! played move's row is too thin to trust), and the turn is graded when it
//! ends (`grade_turn`). Silence beats a wrong number: a turn with any action
//! that stays uncertain, or that cannot be graded, emits nothing to the mod
//! (the log records it as `ungraded`). Run-level choices are graded by policy
//! AGREEMENT (`grade_choice`), never by a win-chance cost. A `recap` follows
//! each won fight and an `autopsy` the run's death, both built behind the
//! grading still in flight.
//!
//! Searches run on an [`Executor`]: [`GraderThread`] in the advisor (during
//! the enemy turn, never waited on), [`InlineExecutor`] in tests. A job is
//! never run while the grader's lock is held: an inline job runs when the
//! public call that queued it returns.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};

use serde_json::{json, Value};

use crate::coach;
use crate::combat_mirror::Mirrored;
use crate::display_names::Names;
use crate::i18n::{tr, tr_dyn, trf};
use crate::py::{self, get, list, truthy};
use crate::search::RecommendConfig;

/// How many advice requests the cache remembers.
pub const ADVICE_CACHE_SIZE: usize = 64;
/// Forced-root sims for a played-move top-up.
pub const TOP_UP_SIMS: u32 = 400;
/// Budget of each step of the background AI-line search.
pub const AI_LINE_SIMS: u32 = 1000;
/// Defensive cap on an AI line's length.
pub const AI_LINE_MAX_STEPS: usize = 12;
/// Root-parallel workers for a catch-up search.
pub const CATCH_UP_THREADS: u32 = 2;
/// How often a recap / autopsy re-queues behind a turn still being graded.
pub const SUMMARY_REQUEUE_MAX: u32 = 20;

/// Why a turn the fight ended (no end-turn report) was dropped.
const COMBAT_ENDED_MID_TURN: &str = "combat ended mid-turn";

/// The chip's name for each run-level decision.
fn label_kind(decision: &str) -> Option<&'static str> {
    Some(match decision {
        "map_select" => tr("Map"),
        "card_reward" => tr("Card reward"),
        "card_bundle" => tr("Card bundle"),
        "shop" => tr("Shop"),
        "event" => tr("Event"),
        "rest_site" => tr("Rest site"),
        "boss_relic" => tr("Boss relic"),
        "treasure" => tr("Treasure"),
        "card_select" => tr("Card select"),
        _ => return None,
    })
}

/// The `capabilities.coach` level a mod needs before a record kind is sent.
fn summary_level(kind: &str) -> i64 {
    if matches!(kind, "recap" | "autopsy") {
        2
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// The advice cache
// ---------------------------------------------------------------------------

/// What kind of advice an [`Entry`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// The search's per-action regret rows over a native position.
    Combat,
    /// A screen's ranked options.
    Screen,
}

/// One advice request's raw result, as the grader needs it. `P` is the
/// native position the search ran on, `C` its resolved config.
#[derive(Debug, Clone)]
pub struct Entry<P, C> {
    pub request_id: Value,
    pub state_id: Value,
    pub kind: EntryKind,
    pub rows: Option<Vec<Value>>,
    pub options: Option<Vec<Value>>,
    pub cs: Option<P>,
    pub cfg: Option<C>,
    pub advice: Value,
    pub pv_labels: Vec<String>,
    /// The wire `advice_request` the advice answered.
    pub msg: Value,
}

/// Bounded LRU of [`Entry`] keyed by `request_id`.
#[derive(Debug)]
pub struct AdviceCache<P, C> {
    size: usize,
    entries: VecDeque<Arc<Entry<P, C>>>,
}

impl<P, C> Default for AdviceCache<P, C> {
    fn default() -> Self {
        Self::new(ADVICE_CACHE_SIZE)
    }
}

impl<P, C> AdviceCache<P, C> {
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            size,
            entries: VecDeque::new(),
        }
    }

    /// Remember one request's result; `None` (nothing stored) without an id.
    pub fn put(&mut self, entry: Entry<P, C>) -> Option<Arc<Entry<P, C>>> {
        if entry.request_id.is_null() {
            return None;
        }
        self.entries
            .retain(|e| !py::eq(&e.request_id, &entry.request_id));
        let entry = Arc::new(entry);
        self.entries.push_back(entry.clone());
        while self.entries.len() > self.size {
            self.entries.pop_front();
        }
        Some(entry)
    }

    /// The entry for `request_id`, freshened.
    pub fn get(&mut self, request_id: &Value) -> Option<Arc<Entry<P, C>>> {
        if request_id.is_null() {
            return None;
        }
        let i = self
            .entries
            .iter()
            .position(|e| py::eq(&e.request_id, request_id))?;
        let entry = self.entries.remove(i)?;
        self.entries.push_back(entry.clone());
        Some(entry)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// The engine
// ---------------------------------------------------------------------------

/// Names one engine action from the engine state it is played in.
pub type Label = Arc<dyn Fn(&Value, &Value) -> String + Send + Sync>;

/// The searches the coach runs (stubbed in tests), on positions `P` with
/// configs `C`.
pub trait Engine<P, C>: Send + Sync {
    /// `{"hand": [[id, upgraded]], "potions": [id | null], "round": int | null}`:
    /// from the native state when there is one (its hand order is the frame
    /// the rows index), from the wire board otherwise.
    fn position_info(&self, cs: Option<&P>, msg: &Value) -> Value;

    /// A forced-root search pricing `native` from `cs`.
    ///
    /// # Errors
    ///
    /// The search could not run.
    fn top_up(
        &self,
        cs: &P,
        native: &Value,
        cfg: Option<&C>,
        sims: u32,
    ) -> Result<Option<Value>, String>;

    /// Search a position the advisor never searched: its regret rows.
    ///
    /// # Errors
    ///
    /// The search could not run.
    fn catch_up(&self, cs: &P, cfg: Option<&C>, sims: u32) -> Result<Vec<Value>, String>;

    /// The engine board of `cs`.
    fn board(&self, cs: Option<&P>) -> Option<Value>;

    /// The AI's line for the turn from `cs` (its labels), and the board the
    /// line ended the turn from (`None` when it stopped short).
    ///
    /// # Errors
    ///
    /// A search could not run.
    fn ai_turn(
        &self,
        cs: &P,
        first: Option<&Value>,
        cfg: Option<&C>,
        label: &Label,
    ) -> Result<(Vec<String>, Option<Value>), String>;
}

/// `cfg` re-budgeted with the mode-resolved constants re-derived for it
/// (`_cfg_for`); the coach's searches pass no `fair` / CRN.
fn cfg_for(cfg: Option<&RecommendConfig>, sims: u32, threads: u32) -> RecommendConfig {
    RecommendConfig {
        sims,
        threads,
        c_puct: None,
        fair: false,
        common_random_numbers: false,
        ..cfg.cloned().unwrap_or_default()
    }
}

fn state_dict(cs: &sts2_core::CombatState) -> Result<Value, String> {
    sts2_wire::emit::combat_state_to_value(cs).map_err(|e| e.to_string())
}

/// The engine the advisor runs: the combat search on native states.
#[derive(Debug, Default, Clone, Copy)]
pub struct CoachEngine;

impl CoachEngine {
    fn recommend(
        cs: &Mirrored,
        state: &sts2_core::CombatState,
        cfg: Option<&RecommendConfig>,
        sims: u32,
        threads: u32,
    ) -> Result<Value, String> {
        crate::search::recommend(state, cs.intent_damage_known, &cfg_for(cfg, sims, threads))
    }
}

impl Engine<Mirrored, RecommendConfig> for CoachEngine {
    fn position_info(&self, cs: Option<&Mirrored>, msg: &Value) -> Value {
        if let Some(cs) = cs {
            let d = state_dict(&cs.state).unwrap_or(Value::Null);
            let hand: Vec<Value> = list(get(&d, "deck"), "hand")
                .iter()
                .map(|c| {
                    json!([
                        py::str_of_or_empty(get(c, "id")),
                        truthy(get(c, "upgraded"))
                    ])
                })
                .collect();
            let potions: Vec<Value> = list(get(&d, "player"), "potions")
                .iter()
                .map(|p| {
                    if truthy(p) {
                        json!(py::str_of(p))
                    } else {
                        Value::Null
                    }
                })
                .collect();
            let rnd = get(&d, "round");
            return json!({"hand": hand, "potions": potions, "round": int_or_null(rnd)});
        }
        let hand: Vec<Value> = list(msg, "hand")
            .iter()
            .filter(|c| c.is_object())
            .map(|c| {
                json!([
                    py::str_of_or_empty(get(c, "id")),
                    truthy(get(c, "upgraded")) || truthy(get(c, "upgrade_level"))
                ])
            })
            .collect();
        let potions: Vec<Value> = list(msg, "potions")
            .iter()
            .filter(|p| p.is_object() && !truthy(get(p, "empty")) && truthy(get(p, "id")))
            .map(|p| get(p, "id").clone())
            .collect();
        json!({"hand": hand, "potions": potions, "round": int_or_null(get(msg, "round"))})
    }

    fn top_up(
        &self,
        cs: &Mirrored,
        native: &Value,
        cfg: Option<&RecommendConfig>,
        sims: u32,
    ) -> Result<Option<Value>, String> {
        let action = crate::mirror_state::parse_bridge_action(native)?;
        let params = cfg_for(cfg, sims, 1).params()?;
        Ok(sts2_wire::emit::mcts::score_action_forced(
            &cs.state, &params, &action,
        ))
    }

    fn catch_up(
        &self,
        cs: &Mirrored,
        cfg: Option<&RecommendConfig>,
        sims: u32,
    ) -> Result<Vec<Value>, String> {
        let result = Self::recommend(cs, &cs.state, cfg, sims, CATCH_UP_THREADS)?;
        Ok(sts2_workbench::advisors::rows_from_mcts(
            &result,
            &json!({}),
        ))
    }

    fn board(&self, cs: Option<&Mirrored>) -> Option<Value> {
        cs.and_then(|cs| state_dict(&cs.state).ok())
    }

    fn ai_turn(
        &self,
        cs: &Mirrored,
        first: Option<&Value>,
        cfg: Option<&RecommendConfig>,
        label: &Label,
    ) -> Result<(Vec<String>, Option<Value>), String> {
        // Stops after end turn, when combat ends, or after a move that put a
        // new card into the hand: past a draw the line would name cards the
        // search dealt itself (`advisors.rs::label_line`'s rule).
        let mut state = cs.state.clone();
        let mut out = Vec::new();
        let mut end = None;
        for step in 0..AI_LINE_MAX_STEPS {
            let action = if let Some(a) = first.filter(|_| step == 0) {
                a.clone()
            } else {
                let result = Self::recommend(cs, &state, cfg, AI_LINE_SIMS, 1)?;
                let Some(best) = list(&result, "actions")
                    .iter()
                    .find(|a| truthy(get(a, "is_best")))
                else {
                    break;
                };
                get(best, "action").clone()
            };
            let before = state_dict(&state)?;
            out.push(label(&before, &action));
            let hand_before: Vec<Value> = list(get(&before, "deck"), "hand")
                .iter()
                .map(|c| get(c, "stable_idx").clone())
                .collect();
            let parsed = crate::mirror_state::parse_bridge_action(&action)?;
            let outcome =
                crate::mirror_state::step_result_name(sts2_core::apply(&mut state, parsed));
            if get(&action, "action").as_str() == Some("end_turn") {
                end = Some(before);
                break;
            }
            let after = state_dict(&state)?;
            if matches!(outcome, "combat_won" | "combat_lost") {
                end = Some(after);
                break;
            }
            let drew = list(get(&after, "deck"), "hand")
                .iter()
                .any(|c| !hand_before.iter().any(|h| py::eq(h, get(c, "stable_idx"))));
            if drew {
                break;
            }
        }
        Ok((out, end))
    }
}

fn int_or_null(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_i64() || n.is_u64() => v.clone(),
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Executors
// ---------------------------------------------------------------------------

/// One unit of grading work.
pub type Job = Box<dyn FnOnce() + Send>;

/// Where the grader's jobs run.
pub trait Executor: Send + Sync {
    fn submit(&self, job: Job);
    /// Drop every job queued so far (an undo made them moot).
    fn cancel(&self);
    fn stop(&self);
    /// Run what an inline executor has queued (a no-op for a thread).
    fn drain(&self) {}
}

/// Runs a submitted job when the public call that queued it returns.
#[derive(Default)]
pub struct InlineExecutor {
    queue: Mutex<VecDeque<Job>>,
}

impl std::fmt::Debug for InlineExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InlineExecutor")
    }
}

impl Executor for InlineExecutor {
    fn submit(&self, job: Job) {
        self.queue.lock().expect("inline queue").push_back(job);
    }

    fn cancel(&self) {}

    fn stop(&self) {}

    fn drain(&self) {
        loop {
            let job = self.queue.lock().expect("inline queue").pop_front();
            match job {
                Some(job) => job(),
                None => return,
            }
        }
    }
}

/// The coach's one background worker: a thread over a job queue. A job
/// already running when [`Executor::cancel`] is called finishes, and its
/// result is discarded by the grader, which checks the turn it belongs to.
pub struct GraderThread {
    tx: Mutex<mpsc::Sender<(u64, Option<Job>)>>,
    generation: Arc<AtomicU64>,
    pending: Arc<(Mutex<usize>, Condvar)>,
}

impl std::fmt::Debug for GraderThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GraderThread")
    }
}

impl Default for GraderThread {
    fn default() -> Self {
        Self::new()
    }
}

impl GraderThread {
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<(u64, Option<Job>)>();
        let generation = Arc::new(AtomicU64::new(0));
        let pending = Arc::new((Mutex::new(0usize), Condvar::new()));
        let (gen, done) = (generation.clone(), pending.clone());
        std::thread::Builder::new()
            .name("coach-grader".into())
            .spawn(move || {
                while let Ok((g, job)) = rx.recv() {
                    let Some(job) = job else { return };
                    if g == gen.load(Ordering::SeqCst) {
                        // A panicking job must not take the worker down.
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                    }
                    let (count, cv) = &*done;
                    let mut n = count.lock().expect("grader count");
                    *n = n.saturating_sub(1);
                    cv.notify_all();
                }
            })
            .expect("spawn the coach grader");
        Self {
            tx: Mutex::new(tx),
            generation,
            pending,
        }
    }

    /// Block until every queued job has run (tests); `false` on timeout.
    #[must_use]
    pub fn wait_idle(&self, timeout: std::time::Duration) -> bool {
        let (count, cv) = &*self.pending;
        let guard = count.lock().expect("grader count");
        cv.wait_timeout_while(guard, timeout, |n| *n > 0)
            .is_ok_and(|(_, t)| !t.timed_out())
    }
}

impl Executor for GraderThread {
    fn submit(&self, job: Job) {
        *self.pending.0.lock().expect("grader count") += 1;
        let g = self.generation.load(Ordering::SeqCst);
        if self
            .tx
            .lock()
            .expect("grader tx")
            .send((g, Some(job)))
            .is_err()
        {
            *self.pending.0.lock().expect("grader count") -= 1;
        }
    }

    fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn stop(&self) {
        let _ = self.tx.lock().expect("grader tx").send((0, None));
    }
}

// ---------------------------------------------------------------------------
// The grader's state
// ---------------------------------------------------------------------------

/// One combat action of the open turn.
#[derive(Debug, Clone)]
struct Act<P, C> {
    label: String,
    peeked: bool,
    native: Option<Value>,
    rows: Option<Vec<Value>>,
    position: Option<P>,
    cfg: Option<C>,
    pending: bool,
    regret: Option<Value>,
    /// Fewer than two legal moves: nothing to compare.
    forced: bool,
    failed: Option<String>,
}

/// One combat, from its first combat advice to `combat_ended`.
#[derive(Debug, Clone)]
struct Fight {
    first_msg: Value,
    node_id: Value,
    hp_start: Option<i64>,
    turn_ids: Vec<String>,
    turns: i64,
    ungraded: i64,
    unfinished: i64,
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_field_names)] // `turn`: the round, as the record names it
struct Turn<P, C> {
    node_id: Value,
    turn: Value,
    floor: Value,
    acts: Vec<Act<P, C>>,
    ungraded_actions: i64,
    closed: bool,
    done: bool,
    deadline: Option<f64>,
    ai: Option<Vec<String>>,
    why: Option<String>,
    ai_pending: bool,
    fight: Option<u64>,
    start_msg: Option<Value>,
}

type TurnId = u64;
type FightId = u64;

/// An ordered map with Python dict semantics (re-assignment keeps the
/// position), keyed by JSON.
#[derive(Debug, Default, Clone)]
struct Ordered<V> {
    items: Vec<(Value, V)>,
}

impl<V> Ordered<V> {
    fn get(&self, k: &Value) -> Option<&V> {
        self.items
            .iter()
            .find(|(key, _)| py::eq(key, k))
            .map(|(_, v)| v)
    }

    fn insert(&mut self, k: Value, v: V) {
        match self.items.iter_mut().find(|(key, _)| py::eq(key, &k)) {
            Some(slot) => slot.1 = v,
            None => self.items.push((k, v)),
        }
    }

    fn set_default(&mut self, k: Value, v: V) {
        if self.get(&k).is_none() {
            self.items.push((k, v));
        }
    }

    fn remove(&mut self, k: &Value) -> Option<V> {
        let i = self.items.iter().position(|(key, _)| py::eq(key, k))?;
        Some(self.items.remove(i).1)
    }

    fn clear(&mut self) {
        self.items.clear();
    }
}

#[derive(Debug)]
struct State<P, C> {
    mod_coach: i64,
    turn: Option<TurnId>,
    turns: BTreeMap<TurnId, Turn<P, C>>,
    last_cfg: Option<C>,
    last_combat_msg: Value,
    floor: Value,
    names: BTreeMap<String, String>,
    anon: u64,
    grading: Vec<TurnId>,
    emitted: Ordered<Value>,
    emitted_records: Ordered<Value>,
    fight: Option<FightId>,
    fights: BTreeMap<FightId, Fight>,
    in_fight: bool,
    round_start: Ordered<Value>,
    run_ungraded: i64,
    next_id: u64,
}

impl<P, C> State<P, C> {
    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn anon_node(&mut self) -> Value {
        self.anon += 1;
        json!(format!("x{}", self.anon))
    }
}

/// Puts one record on the wire / in the run log.
pub type Sink = Box<dyn Fn(&Value) + Send + Sync>;
/// `node id -> (log_name, log_idx)`.
pub type Locate = Box<dyn Fn(&Value) -> (Option<String>, Option<i64>) + Send + Sync>;
/// `(map_select msg, chosen option) -> route`.
pub type MapPath = Box<dyn Fn(&Value, usize) -> Option<Vec<Value>> + Send + Sync>;
/// Seconds, monotonic.
pub type Clock = Box<dyn Fn() -> f64 + Send + Sync>;

/// How the grader is wired.
#[allow(missing_debug_implementations)] // callbacks and trait objects
pub struct GraderConfig<P, C> {
    pub send: Sink,
    pub log: Sink,
    /// Where a line goes when it is logged (`(level, message)`).
    pub warn: Box<dyn Fn(&str) + Send + Sync>,
    pub engine: Box<dyn Engine<P, C>>,
    pub executor: Arc<dyn Executor>,
    pub names: Arc<Names>,
    pub catchup_sims: u32,
    pub budget_s: f64,
    pub map_path: Option<MapPath>,
    pub clock: Clock,
    pub locate: Option<Locate>,
    pub cache: SharedCache<P, C>,
}

struct Inner<P, C> {
    state: Mutex<State<P, C>>,
    cfg: GraderConfig<P, C>,
}

/// An advice cache several holders share (the grader and the advisor loop).
pub type SharedCache<P, C> = Arc<Mutex<AdviceCache<P, C>>>;

/// Per-connection live coach. `send` is only called while the mod
/// advertised `capabilities.coach >= 1` (`>= 2` for a recap / autopsy);
/// `log` is always called, including for turns that stay ungraded. Both may
/// be called from the executor's thread.
pub struct CoachGrader<P, C> {
    inner: Arc<Inner<P, C>>,
}

impl<P, C> Clone for CoachGrader<P, C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<P, C> std::fmt::Debug for CoachGrader<P, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CoachGrader")
    }
}

fn int_of_strict(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

fn norm(raw: &Value) -> String {
    py::str_of_or_empty(raw).trim().to_uppercase()
}

/// A screen opened inside a fight.
fn in_combat_screen(msg: &Value) -> bool {
    !get(msg, "combat").is_null() || get(msg, "surface").as_str() == Some("choose_a_card")
}

fn player_hp(msg: &Value) -> Option<i64> {
    int_of_strict(get(get(msg, "player"), "hp"))
        .or_else(|| int_of_strict(get(get(msg, "run_state"), "hp")))
}

/// `_in_range`: an int (not a bool) indexing `items`.
fn in_range(len: usize, index: &Value) -> Option<usize> {
    let i = int_of_strict(index)?;
    usize::try_from(i).ok().filter(|&i| i < len)
}

fn enemy_label(enemies: &[Value], index: &Value) -> Option<String> {
    in_range(enemies.len(), index)
        .map(|i| sts2_workbench::enemy_labels::enemy_labels(enemies).swap_remove(i))
}

fn ally_label(allies: &[Value], index: &Value) -> Option<String> {
    let i = in_range(allies.len(), index)?;
    let names: Vec<String> = allies
        .iter()
        .enumerate()
        .map(|(j, a)| {
            let c = get(a, "character");
            if truthy(c) {
                py::str_of(c)
            } else {
                trf("teammate {n}", &[("n", &j)])
            }
        })
        .collect();
    Some(sts2_workbench::enemy_labels::labels_from_names(&names).swap_remove(i))
}

impl<P, C> CoachGrader<P, C>
where
    P: Clone + Send + Sync + 'static,
    C: Clone + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(cfg: GraderConfig<P, C>) -> Self {
        let state = State {
            mod_coach: 0,
            turn: None,
            turns: BTreeMap::new(),
            last_cfg: None,
            last_combat_msg: json!({}),
            floor: Value::Null,
            names: BTreeMap::new(),
            anon: 0,
            grading: Vec::new(),
            emitted: Ordered::default(),
            emitted_records: Ordered::default(),
            fight: None,
            fights: BTreeMap::new(),
            in_fight: false,
            round_start: Ordered::default(),
            run_ungraded: 0,
            next_id: 0,
        };
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(state),
                cfg,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State<P, C>> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn submit(&self, job: impl FnOnce(&Self) + Send + 'static) {
        let me = self.clone();
        self.inner.cfg.executor.submit(Box::new(move || job(&me)));
    }

    /// Run what an inline executor queued during a public call.
    fn settle(&self) {
        self.inner.cfg.executor.drain();
    }

    fn warn(&self, message: &str) {
        (self.inner.cfg.warn)(message);
    }

    /// The emitted records' `id -> node_id`, in emission order.
    #[must_use]
    pub fn emitted(&self) -> Vec<(Value, Value)> {
        self.lock().emitted.items.clone()
    }

    /// The emitted records, `id -> record`, in emission order.
    #[must_use]
    pub fn emitted_records(&self) -> Vec<(Value, Value)> {
        self.lock().emitted_records.items.clone()
    }

    /// How many ended turns are still being graded.
    #[must_use]
    pub fn grading(&self) -> usize {
        self.lock().grading.len()
    }

    /// The mod's advertised `capabilities.coach`.
    #[must_use]
    pub fn mod_coach(&self) -> i64 {
        self.lock().mod_coach
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, AdviceCache<P, C>> {
        self.inner
            .cfg
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Remember one request's result; `None` without a request id.
    pub fn cache_put(&self, entry: Entry<P, C>) -> Option<Arc<Entry<P, C>>> {
        self.cache().put(entry)
    }

    /// The cached advice for `request_id`.
    pub fn cache_get(&self, request_id: &Value) -> Option<Arc<Entry<P, C>>> {
        self.cache().get(request_id)
    }

    // -- advice side -------------------------------------------------------

    /// Every `advice_request`: track the mod's capability and the floor, and
    /// preempt grading that the next turn has overtaken (the enemy turn is
    /// over: whatever is still grading is dropped, never waited on).
    pub fn on_advice_request(&self, msg: &Value) {
        {
            let mut st = self.lock();
            let coach = get(get(msg, "capabilities"), "coach");
            st.mod_coach = int_of_strict(coach)
                .filter(|_| !coach.is_boolean())
                .unwrap_or(match coach {
                    Value::Bool(b) => i64::from(*b),
                    _ => 0,
                });
            if !get(msg, "floor").is_null() {
                st.floor = get(msg, "floor").clone();
            }
            let advice_for = get(msg, "advice_for");
            let advice_for = if truthy(advice_for) {
                advice_for.as_str().unwrap_or("")
            } else {
                "combat_action"
            };
            if advice_for == "combat_action" {
                let rnd = int_of_strict(get(msg, "round"));
                for id in st.grading.clone() {
                    let turn_round = st.turns.get(&id).and_then(|t| int_of_strict(&t.turn));
                    let overtaken = match (rnd, turn_round) {
                        (Some(r), Some(t)) => r > t,
                        _ => true,
                    };
                    if overtaken {
                        self.drop_turn(
                            &mut st,
                            id,
                            "preempted: the next turn's advice arrived first",
                        );
                    }
                }
            }
        }
        self.settle();
    }

    /// An undo / snapshot restore: abandon grading, withdraw every emitted
    /// record whose node is not on the root -> active path. Returns the
    /// withdrawal (also sent / logged).
    pub fn on_rewind(&self, msg: &Value, active_path: &[Value]) -> Vec<Value> {
        self.inner.cfg.executor.cancel();
        let record = {
            let mut st = self.lock();
            let why = format!("undone ({})", py::str_of(get(msg, "type")));
            self.abandon_open_turn(&mut st, &why);
            for id in st.grading.clone() {
                self.drop_turn(&mut st, id, &why);
            }
            let gone: Vec<Value> = st
                .emitted
                .items
                .iter()
                .filter(|(_, node)| !active_path.iter().any(|p| py::eq(p, node)))
                .map(|(rid, _)| rid.clone())
                .collect();
            for rid in &gone {
                st.emitted.remove(rid);
                st.emitted_records.remove(rid);
                if let Some(f) = st.fight.and_then(|f| st.fights.get_mut(&f)) {
                    let rid = py::str_of(rid);
                    f.turn_ids.retain(|t| *t != rid);
                }
            }
            if gone.is_empty() {
                None
            } else {
                let record = json!({"kind": "withdraw", "ids": gone});
                self.deliver(&mut st, &record);
                Some(record)
            }
        };
        self.settle();
        record.into_iter().collect()
    }

    /// Advice was served and cached; remember what labelling needs.
    pub fn on_advice(&self, entry: Option<&Entry<P, C>>) {
        let Some(entry) = entry else { return };
        {
            let mut st = self.lock();
            if entry.kind == EntryKind::Combat {
                if entry.cfg.is_some() {
                    st.last_cfg.clone_from(&entry.cfg);
                }
                st.last_combat_msg = entry.msg.clone();
                learn_names(&mut st.names, &entry.msg);
                if !st.in_fight {
                    let id = st.new_id();
                    st.fights.insert(
                        id,
                        Fight {
                            first_msg: entry.msg.clone(),
                            node_id: Value::Null,
                            hp_start: player_hp(&entry.msg),
                            turn_ids: Vec::new(),
                            turns: 0,
                            ungraded: 0,
                            unfinished: 0,
                        },
                    );
                    st.fight = Some(id);
                    st.in_fight = true;
                    st.round_start.clear();
                }
                let rnd = get(&entry.msg, "round");
                if !rnd.is_null() {
                    st.round_start.set_default(rnd.clone(), entry.msg.clone());
                }
            } else if !in_combat_screen(&entry.msg) {
                // A run-level screen: any turn still open ended with the
                // fight (a killing blow reports no end turn).
                self.abandon_open_turn(&mut st, COMBAT_ENDED_MID_TURN);
                st.in_fight = false;
            }
        }
        self.settle();
    }

    // -- fight / run summaries ---------------------------------------------

    /// The mod's `combat_ended`: queue the won fight's recap behind the
    /// fight's turns still being graded.
    pub fn on_combat_ended(&self, msg: &Value) {
        let fight = {
            let mut st = self.lock();
            self.abandon_open_turn(&mut st, COMBAT_ENDED_MID_TURN);
            st.in_fight = false;
            st.fight.take()
        };
        if let Some(fight) = fight {
            if truthy(get(msg, "victory")) {
                let msg = msg.clone();
                self.submit(move |g| g.recap_job(fight, &msg, 0));
            }
        }
        self.settle();
    }

    fn recap_job(&self, fight: FightId, msg: &Value, tries: u32) {
        let built = {
            let mut st = self.lock();
            if self.still_grading(&mut st, Some(fight)) && tries < SUMMARY_REQUEUE_MAX {
                None
            } else {
                let Some(f) = st.fights.get(&fight).cloned() else {
                    return;
                };
                let records: Vec<Value> = f
                    .turn_ids
                    .iter()
                    .filter_map(|rid| st.emitted_records.get(&json!(rid)).cloned())
                    .collect();
                let node = if f.node_id.is_null() {
                    let node = st.anon_node();
                    if let Some(f) = st.fights.get_mut(&fight) {
                        f.node_id = node.clone();
                    }
                    node
                } else {
                    f.node_id.clone()
                };
                Some((f, node, records))
            }
        };
        let Some((f, node, records)) = built else {
            // A slip's AI-line job was queued after this one: go behind it.
            let msg = msg.clone();
            self.submit(move |g| g.recap_job(fight, &msg, tries + 1));
            return;
        };
        let summary = json!({
            "first_msg": f.first_msg,
            "node_id": node,
            "hp_start": f.hp_start,
            "turns": f.turns,
            "ungraded": f.ungraded,
            "unfinished": f.unfinished,
        });
        let position = self.log_position(&node);
        // No recap when the player's HP is unknown.
        if let Some(recap) = coach::build_recap(&summary, msg, &records, &position) {
            let mut st = self.lock();
            self.deliver(&mut st, &recap);
        }
    }

    /// Any ended turn (of `fight`, or any) still being graded within its
    /// budget (overdue turns are dropped here).
    fn still_grading(&self, st: &mut State<P, C>, fight: Option<FightId>) -> bool {
        for id in st.grading.clone() {
            let Some(t) = st.turns.get(&id) else { continue };
            if t.done || fight.is_some_and(|f| t.fight != Some(f)) {
                continue;
            }
            // `any(...)`: stop at the first turn still within its budget.
            if !self.overdue(st, id) {
                return true;
            }
        }
        false
    }

    /// A run ended with nothing to summarize: cancel the grading still
    /// queued and forget the run.
    pub fn on_run_end(&self) {
        self.inner.cfg.executor.cancel();
        self.reset_run();
        self.settle();
    }

    fn reset_run(&self) {
        let mut st = self.lock();
        st.turn = None;
        for id in std::mem::take(&mut st.grading) {
            if let Some(t) = st.turns.get_mut(&id) {
                t.done = true;
            }
        }
        st.emitted.clear();
        st.emitted_records.clear();
        st.fight = None;
        st.in_fight = false;
        st.round_start.clear();
        st.run_ungraded = 0;
        st.turns.retain(|_, t| !t.done);
        drop(st);
        self.cache().clear();
    }

    /// Death: queue the run's autopsy (behind the grading in flight), then
    /// forget the run.
    pub fn on_game_over(&self, msg: &Value) {
        {
            let mut st = self.lock();
            self.abandon_open_turn(&mut st, COMBAT_ENDED_MID_TURN);
            st.in_fight = false;
            st.fight = None;
        }
        let msg = msg.clone();
        self.submit(move |g| g.autopsy_job(&msg, 0));
        self.settle();
    }

    fn autopsy_job(&self, msg: &Value, tries: u32) {
        let snapshot = {
            let mut st = self.lock();
            if self.still_grading(&mut st, None) && tries < SUMMARY_REQUEUE_MAX {
                None
            } else {
                let records: Vec<Value> = st
                    .emitted_records
                    .items
                    .iter()
                    .map(|(_, r)| r.clone())
                    .collect();
                Some((records, st.run_ungraded))
            }
        };
        let Some((records, ungraded)) = snapshot else {
            let msg = msg.clone();
            self.submit(move |g| g.autopsy_job(&msg, tries + 1));
            return;
        };
        let turns: Vec<Value> = records
            .iter()
            .filter(|r| get(r, "kind").as_str() == Some("turn"))
            .cloned()
            .collect();
        let choices: Vec<Value> = records
            .iter()
            .filter(|r| get(r, "kind").as_str() == Some("choice"))
            .cloned()
            .collect();
        let log_name = self.log_position(&Value::Null)["log_name"].clone();
        let seed = get(msg, "seed");
        let id_tail = if truthy(seed) {
            py::str_of(seed)
        } else if truthy(&log_name) {
            py::str_of(&log_name)
        } else {
            "run".to_string()
        };
        let record = json!({
            "kind": "autopsy",
            "id": format!("autopsy:{id_tail}"),
            "log_name": log_name,
            "floor": get(msg, "floor"),
            "act": get(msg, "act"),
            "items": coach::rank_autopsy(&turns, &choices, coach::AUTOPSY_ITEMS),
            "turns_ungraded": ungraded,
            "decisions_graded": turns.len() + choices.len(),
        });
        {
            let mut st = self.lock();
            self.deliver(&mut st, &record);
        }
        self.reset_run();
    }

    // -- human side --------------------------------------------------------

    /// Grade one `human_action`; returns the records emitted before it
    /// returned. `pre_state` is the mirror's position BEFORE the action was
    /// followed: a catch-up search starts there when the advice on screen
    /// was not for this exact position.
    pub fn on_human_action(
        &self,
        msg: &Value,
        pre_state: Option<P>,
        node_id: &Value,
    ) -> Vec<Value> {
        let out = if get(msg, "decision_type").as_str() == Some("combat_action") {
            self.on_combat_action(msg, pre_state, node_id)
        } else {
            self.on_choice(msg, node_id)
        };
        self.settle();
        out
    }

    /// The cached advice for EXACTLY the position the action was taken from.
    fn valid_entry(&self, msg: &Value, kind: EntryKind) -> Option<Arc<Entry<P, C>>> {
        let entry = self.cache().get(get(msg, "advice_request_id"))?;
        if entry.kind != kind {
            return None;
        }
        let position = get(msg, "position_id");
        if position.is_null() || !py::eq(&entry.state_id, position) {
            return None;
        }
        Some(entry)
    }

    fn on_combat_action(&self, msg: &Value, pre_state: Option<P>, node_id: &Value) -> Vec<Value> {
        let entry = self.valid_entry(msg, EntryKind::Combat);
        let is_end = get(msg, "action").as_str() == Some("end_turn");
        let label = if is_end {
            tr("end turn").to_string()
        } else {
            self.label_human(msg)
        };
        let engine = &self.inner.cfg.engine;
        let (position, rows, cfg, info) = if let Some(e) = &entry {
            let info = if is_end {
                json!({})
            } else {
                engine.position_info(e.cs.as_ref(), &e.msg)
            };
            let rows = e.rows.clone().unwrap_or_default();
            (e.cs.clone(), Some(rows), e.cfg.clone(), info)
        } else {
            let info = pre_state.as_ref().map_or_else(
                || json!({}),
                |cs| engine.position_info(Some(cs), &json!({})),
            );
            (pre_state, None, self.lock().last_cfg.clone(), info)
        };
        let mut act = Act {
            label,
            peeked: truthy(get(msg, "peeked")),
            native: coach::native_action(msg, &info),
            rows,
            position,
            cfg,
            pending: false,
            regret: None,
            forced: false,
            failed: None,
        };
        match (&act.native, &act.rows) {
            (None, _) => act.failed = Some("move not found at its position".into()),
            (Some(_), None) => {
                if act.position.is_none() {
                    act.failed = Some("no advice and no position to search".into());
                } else {
                    act.pending = true; // catch-up after the turn ends
                }
            }
            (Some(native), Some(rows)) => {
                if coach::played_needs_top_up(rows, native) {
                    act.pending = true; // top-up after the turn ends, never inline
                } else {
                    assess(&mut act);
                }
            }
        }

        let mut st = self.lock();
        let open = st
            .turn
            .filter(|id| st.turns.get(id).is_some_and(|t| !t.closed));
        let turn_id = match open {
            Some(id) => id,
            None => Self::new_turn(&mut st, entry.as_deref(), &info, node_id),
        };
        let deadline = (self.inner.cfg.clock)() + self.inner.cfg.budget_s;
        let Some(turn) = st.turns.get_mut(&turn_id) else {
            return Vec::new();
        };
        turn.acts.push(act);
        if !is_end {
            return Vec::new();
        }
        turn.closed = true;
        turn.deadline = Some(deadline);
        let pending: Vec<usize> = (0..turn.acts.len())
            .filter(|&i| turn.acts[i].pending)
            .collect();
        let fight = turn.fight;
        if let Some(f) = fight.and_then(|f| st.fights.get_mut(&f)) {
            f.turns += 1;
        }
        st.turn = None;
        st.grading.push(turn_id);
        for i in pending {
            self.submit_job(turn_id, move |g| g.grade_pending(turn_id, i));
        }
        self.maybe_finish(&mut st, turn_id)
    }

    /// Open the turn an action starts.
    fn new_turn(
        st: &mut State<P, C>,
        entry: Option<&Entry<P, C>>,
        info: &Value,
        node_id: &Value,
    ) -> TurnId {
        let entry_round = entry
            .as_ref()
            .map_or(&Value::Null, |e| get(&e.msg, "round"));
        let rnd = if truthy(entry_round) {
            entry_round.clone()
        } else {
            get(info, "round").clone()
        };
        let entry_floor = entry
            .as_ref()
            .map_or(&Value::Null, |e| get(&e.msg, "floor"));
        let floor = if truthy(entry_floor) {
            entry_floor.clone()
        } else {
            st.floor.clone()
        };
        let start_msg = st
            .round_start
            .get(&rnd)
            .cloned()
            .filter(truthy)
            .or_else(|| entry.as_ref().map(|e| e.msg.clone()));
        let id = st.new_id();
        let fight = st.fight;
        st.turns.insert(
            id,
            Turn {
                node_id: node_id.clone(),
                turn: rnd,
                floor,
                acts: Vec::new(),
                ungraded_actions: 0,
                closed: false,
                done: false,
                deadline: None,
                ai: None,
                why: None,
                ai_pending: false,
                fight,
                start_msg,
            },
        );
        st.turn = Some(id);
        if let Some(f) = fight.and_then(|f| st.fights.get_mut(&f)) {
            if f.node_id.is_null() {
                f.node_id = node_id.clone();
            }
        }
        id
    }

    /// Catch-up search and/or played-row top-up for one action (executor).
    fn grade_pending(&self, turn_id: TurnId, i: usize) -> Result<(), String> {
        let (rows, native, position, cfg) = {
            let st = self.lock();
            let Some(act) = st.turns.get(&turn_id).and_then(|t| t.acts.get(i)) else {
                return Ok(());
            };
            (
                act.rows.clone(),
                act.native.clone(),
                act.position.clone(),
                act.cfg.clone(),
            )
        };
        let engine = &self.inner.cfg.engine;
        let mut rows = if let Some(rows) = rows {
            rows
        } else {
            let Some(position) = position.as_ref() else {
                return Err("no position to search".into());
            };
            engine.catch_up(position, cfg.as_ref(), self.inner.cfg.catchup_sims)?
        };
        if let (Some(native), Some(position)) = (native.as_ref(), position.as_ref()) {
            if coach::played_needs_top_up(&rows, native) {
                if let Some(fresh) = engine.top_up(position, native, cfg.as_ref(), TOP_UP_SIMS)? {
                    if let Some((idx, fields)) = coach::top_up_fields(&rows, native, &fresh) {
                        if let (Some(row), Value::Object(f)) = (rows[idx].as_object_mut(), fields) {
                            row.extend(f);
                        }
                    }
                }
            }
        }
        let mut st = self.lock();
        if let Some(act) = st.turns.get_mut(&turn_id).and_then(|t| t.acts.get_mut(i)) {
            act.rows = Some(rows);
            assess(act);
            act.pending = false;
        }
        Ok(())
    }

    /// Queue one unit of grading work for `turn_id`: skipped once the turn
    /// is done or overdue; a failure drops the turn.
    fn submit_job(
        &self,
        turn_id: TurnId,
        work: impl FnOnce(&Self) -> Result<(), String> + Send + 'static,
    ) {
        self.submit(move |g| {
            {
                let mut st = g.lock();
                if st.turns.get(&turn_id).is_none_or(|t| t.done) || g.overdue(&mut st, turn_id) {
                    return;
                }
            }
            if let Err(exc) = work(g) {
                g.warn(&format!("coach: grading job failed: {exc}"));
                let mut st = g.lock();
                g.drop_turn(&mut st, turn_id, &format!("grading failed: {exc}"));
                return;
            }
            let mut st = g.lock();
            if !g.overdue(&mut st, turn_id) {
                g.maybe_finish(&mut st, turn_id);
            }
        });
    }

    /// Drop the turn once its catch-up budget is spent: a late chip is worse
    /// than none.
    fn overdue(&self, st: &mut State<P, C>, turn_id: TurnId) -> bool {
        let Some(turn) = st.turns.get(&turn_id) else {
            return false;
        };
        let Some(deadline) = turn.deadline.filter(|_| !turn.done) else {
            return false;
        };
        if (self.inner.cfg.clock)() <= deadline {
            return false;
        }
        let why = format!(
            "catch-up budget of {}s exceeded",
            py::fmt_g(self.inner.cfg.budget_s)
        );
        self.drop_turn(st, turn_id, &why);
        true
    }

    // -- turn assembly -----------------------------------------------------

    /// Give up on a turn: silent to the mod, logged as ungraded.
    fn drop_turn(&self, st: &mut State<P, C>, turn_id: TurnId, reason: &str) {
        st.grading.retain(|id| *id != turn_id);
        let Some(turn) = st.turns.get_mut(&turn_id) else {
            return;
        };
        if turn.done {
            return;
        }
        turn.done = true;
        let (closed, fight) = (turn.closed, turn.fight);
        if closed {
            st.run_ungraded += 1;
            if let Some(f) = fight.and_then(|f| st.fights.get_mut(&f)) {
                f.ungraded += 1;
            }
        }
        let mut record = self.turn_head(st, turn_id);
        record["verdict"] = json!("ungraded");
        record["reason"] = json!(reason);
        (self.inner.cfg.log)(&record);
    }

    /// Drop the turn still open. One the fight ended counts as
    /// `unfinished`, never `ungraded`: it was never closed.
    fn abandon_open_turn(&self, st: &mut State<P, C>, reason: &str) {
        let Some(id) = st.turn.take() else { return };
        let Some(turn) = st.turns.get(&id) else {
            return;
        };
        if turn.acts.is_empty() {
            return;
        }
        if reason == COMBAT_ENDED_MID_TURN {
            if let Some(f) = turn.fight.and_then(|f| st.fights.get_mut(&f)) {
                f.unfinished += 1;
            }
        }
        self.drop_turn(st, id, reason);
    }

    fn turn_head(&self, st: &mut State<P, C>, turn_id: TurnId) -> Value {
        let needs_node = st.turns.get(&turn_id).is_some_and(|t| t.node_id.is_null());
        if needs_node {
            let node = st.anon_node();
            if let Some(t) = st.turns.get_mut(&turn_id) {
                t.node_id = node;
            }
        }
        let Some(turn) = st.turns.get(&turn_id) else {
            return json!({});
        };
        let mut head = json!({
            "kind": "turn",
            "id": format!("turn:{}", py::str_of(&turn.node_id)),
            "node_id": turn.node_id,
            "turn": turn.turn,
            "floor": turn.floor,
        });
        let pos = self.log_position(&turn.node_id);
        head["log_name"] = pos["log_name"].clone();
        head["log_idx"] = pos["log_idx"].clone();
        head
    }

    /// `{"log_name", "log_idx"}` of a node (both `null` when unknown).
    fn log_position(&self, node_id: &Value) -> Value {
        let (name, idx) = self
            .inner
            .cfg
            .locate
            .as_ref()
            .map_or((None, None), |locate| locate(node_id));
        json!({"log_name": name, "log_idx": idx})
    }

    /// Emit the turn's record once every action is graded.
    fn maybe_finish(&self, st: &mut State<P, C>, turn_id: TurnId) -> Vec<Value> {
        let Some(turn) = st.turns.get(&turn_id) else {
            return Vec::new();
        };
        if turn.done || !turn.closed || turn.acts.iter().any(|a| a.pending) {
            return Vec::new();
        }
        if let Some(failed) = turn.acts.iter().find_map(|a| a.failed.clone()) {
            self.drop_turn(st, turn_id, &failed);
            return Vec::new();
        }
        if turn.acts.iter().any(|a| {
            a.regret
                .as_ref()
                .is_some_and(|r| get(r, "verdict").as_str() == Some("uncertain"))
        }) {
            self.drop_turn(st, turn_id, "a move stayed uncertain after top-up");
            return Vec::new();
        }
        let regrets: Vec<Option<Value>> = turn
            .acts
            .iter()
            .filter(|a| !a.forced)
            .map(|a| a.regret.clone())
            .collect();
        let Some(grade) = sts2_workbench::advisors::grade_turn(&regrets) else {
            self.drop_turn(st, turn_id, "a move could not be graded");
            return Vec::new();
        };
        let verdict = py::str_of(get(&grade, "verdict"));
        if matches!(verdict.as_str(), "slip" | "blunder") && turn.ai.is_none() {
            if !turn.ai_pending {
                if let Some(t) = st.turns.get_mut(&turn_id) {
                    t.ai_pending = true;
                }
                self.submit_job(turn_id, move |g| g.build_ai_line(turn_id));
            }
            return Vec::new();
        }
        let context_msg = turn.start_msg.clone().filter(truthy).or_else(|| {
            (turn.fight == st.fight)
                .then(|| st.round_start.get(&turn.turn).cloned())
                .flatten()
        });
        let you: Vec<String> = turn.acts.iter().map(|a| a.label.clone()).collect();
        let fields = json!({
            "verdict": verdict,
            "you": you,
            "ai": turn.ai.clone().unwrap_or_default(),
            "why": turn.why,
            "hp_drop": get(&grade, "hp_drop"),
            "win_before": get(&grade, "win_before"),
            "win_after": get(&grade, "win_after"),
            "peeked": turn.acts.iter().any(|a| a.peeked),
            "ungraded_actions": turn.ungraded_actions,
            "severity": get(&grade, "severity"),
            "context": context_msg.as_ref().and_then(coach::turn_context),
        });
        let mut record = self.turn_head(st, turn_id);
        if let (Some(r), Value::Object(f)) = (record.as_object_mut(), fields) {
            r.extend(f);
        }
        let fight = st.turns.get(&turn_id).and_then(|t| t.fight);
        if let Some(f) = fight.and_then(|f| st.fights.get_mut(&f)) {
            f.turn_ids.push(py::str_of(get(&record, "id")));
        }
        if let Some(t) = st.turns.get_mut(&turn_id) {
            t.done = true;
        }
        st.grading.retain(|id| *id != turn_id);
        self.deliver(st, &record);
        vec![record]
    }

    /// The AI's line from the turn's first position (executor).
    fn build_ai_line(&self, turn_id: TurnId) -> Result<(), String> {
        let (first, names, last_msg) = {
            let st = self.lock();
            let Some(turn) = st.turns.get(&turn_id) else {
                return Ok(());
            };
            (
                turn.acts.first().cloned(),
                st.names.clone(),
                st.last_combat_msg.clone(),
            )
        };
        let mut line = Vec::new();
        let mut why = None;
        if let Some(first) = &first {
            let best = first
                .rows
                .as_ref()
                .and_then(|rows| rows.iter().find(|r| truthy(get(r, "is_best"))).cloned());
            if let (Some(position), true) = (
                first.position.as_ref(),
                first.rows.as_ref().is_some_and(|r| !r.is_empty()),
            ) {
                let native = best.as_ref().and_then(coach::native_from_row);
                let display = self.inner.cfg.names.clone();
                let label: Label = Arc::new(move |state: &Value, action: &Value| {
                    label_engine(&display, &names, &last_msg, state, action)
                });
                let (l, ai_end) = self.inner.cfg.engine.ai_turn(
                    position,
                    native.as_ref(),
                    first.cfg.as_ref(),
                    &label,
                )?;
                line = l;
                why = self.why(turn_id, ai_end.as_ref());
            }
            if line.is_empty() && first.rows.as_ref().is_some_and(|r| !r.is_empty()) {
                if let Some(best) = best {
                    let action = get(&best, "action");
                    line = vec![if truthy(action) {
                        py::str_of(action)
                    } else {
                        "?".into()
                    }];
                }
            }
        }
        let mut st = self.lock();
        if let Some(t) = st.turns.get_mut(&turn_id) {
            t.ai = Some(line);
            t.why = why;
            t.ai_pending = false;
        }
        Ok(())
    }

    /// The reason line from the AI line's end board (executor).
    fn why(&self, turn_id: TurnId, ai_end: Option<&Value>) -> Option<String> {
        let (first_pos, last_pos, last_native, start_msg, turn_round) = {
            let st = self.lock();
            let turn = st.turns.get(&turn_id)?;
            let last = turn.acts.last()?;
            (
                turn.acts.first()?.position.clone(),
                last.position.clone(),
                last.native.clone(),
                turn.start_msg.clone(),
                turn.turn.clone(),
            )
        };
        let ai_end = ai_end?;
        if get(last_native.as_ref()?, "action").as_str() != Some("end_turn") {
            return None;
        }
        let engine = &self.inner.cfg.engine;
        let start = engine.board(first_pos.as_ref())?;
        let you_end = engine.board(last_pos.as_ref())?;
        let roster: Vec<Value> = start_msg
            .as_ref()
            .map_or(&[][..], |m| list(m, "enemies"))
            .iter()
            .filter(|e| e.is_object())
            .cloned()
            .collect();
        let mut names = sts2_workbench::enemy_labels::enemy_labels(&roster);
        let enc = list(&start, "encounter");
        if names.len() != enc.len() {
            names = enc
                .iter()
                .map(|e| {
                    let kind = get(e, "kind");
                    let raw = if truthy(kind) {
                        py::str_of(kind)
                    } else {
                        "?".into()
                    };
                    py::title(&raw.replace('_', " "))
                })
                .collect();
        }
        let owned = |events: Vec<(&'static str, i64)>| -> Vec<(String, i64)> {
            events
                .into_iter()
                .map(|(k, i)| (k.to_string(), i))
                .collect()
        };
        coach::why_text(
            &owned(coach::board_events(&start, &you_end)),
            &owned(coach::board_events(&start, ai_end)),
            &names,
            &turn_round,
        )
    }

    /// Name one combat `human_action` the way the AI line names moves.
    pub fn label_human(&self, msg: &Value) -> String {
        let (names, last_msg) = {
            let st = self.lock();
            (st.names.clone(), st.last_combat_msg.clone())
        };
        let display = &self.inner.cfg.names;
        let card = get(msg, "card");
        if card.is_object() && truthy(get(card, "id")) {
            let cid = norm(get(card, "id"));
            let mut name = names
                .get(&cid)
                .cloned()
                .unwrap_or_else(|| display.display_name_str(&cid));
            if truthy(get(card, "upgraded")) {
                name.push('+');
            }
            return name + &target_suffix(&last_msg, get(msg, "target_index"));
        }
        let potion = get(msg, "potion");
        if potion.is_object() && truthy(get(potion, "id")) {
            let pid = norm(get(potion, "id"));
            let name = names
                .get(&pid)
                .cloned()
                .unwrap_or_else(|| display.display_name_str(&pid));
            return trf("drink {name}", &[("name", &name)]);
        }
        let detail = get(msg, "detail");
        let action = get(msg, "action");
        if truthy(detail) {
            py::str_of(detail)
        } else if truthy(action) {
            py::str_of(action)
        } else {
            "?".into()
        }
    }

    /// Name one engine action from the engine state it is played in, with
    /// the names learned so far (`_label_engine`).
    pub fn label_engine(&self, state: &Value, action: &Value) -> String {
        let (names, last_msg) = {
            let st = self.lock();
            (st.names.clone(), st.last_combat_msg.clone())
        };
        label_engine(&self.inner.cfg.names, &names, &last_msg, state, action)
    }

    // -- run-level choices -------------------------------------------------

    fn on_choice(&self, msg: &Value, node_id: &Value) -> Vec<Value> {
        let decision = get(msg, "decision_type").as_str().unwrap_or("").to_string();
        let entry = self.valid_entry(msg, EntryKind::Screen);
        let mut st = self.lock();
        let in_combat = match &entry {
            Some(e) => in_combat_screen(&e.msg),
            None => st.turn.is_some(),
        };
        if matches!(decision.as_str(), "card_select" | "card_reward") && in_combat {
            // Searched in-combat selects carry no per-option stats: skipped,
            // never guessed.
            if let Some(t) = st.turn.and_then(|id| st.turns.get_mut(&id)) {
                t.ungraded_actions += 1;
            }
            return Vec::new();
        }
        self.abandon_open_turn(&mut st, COMBAT_ENDED_MID_TURN);
        let Some(entry) = entry else {
            return Vec::new();
        };
        let Some(kind_label) = label_kind(&decision) else {
            return Vec::new();
        };
        let options: Vec<&Value> = entry
            .options
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|o| o.is_object())
            .collect();
        let choice = get(msg, "choice");
        let index = get(choice, "index");
        let played = if index.is_null() {
            options
                .iter()
                .position(|o| get(o, "action").as_str() == Some("skip"))
        } else {
            options.iter().position(|o| {
                py::eq(get(o, "index"), index) && get(o, "action").as_str() != Some("skip")
            })
        };
        let pairs: Vec<(String, f64)> = options
            .iter()
            .map(|o| {
                let label = get(o, "label");
                (
                    if truthy(label) {
                        py::str_of(label)
                    } else {
                        "?".into()
                    },
                    get(o, "visit_share").as_f64().unwrap_or(0.0),
                )
            })
            .collect();
        let Some(played) = played.filter(|_| !pairs.is_empty()) else {
            return Vec::new();
        };
        let probs: Vec<f64> = pairs.iter().map(|p| p.1).collect();
        let g = sts2_workbench::session_choice::grade_choice(&probs, played);
        let mut ranked = pairs.clone();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let node = if node_id.is_null() {
            st.anon_node()
        } else {
            node_id.clone()
        };
        let mut record = json!({
            "kind": "choice",
            "id": format!("choice:{}", py::str_of(&node)),
            "node_id": node,
            "decision": decision,
            "label_kind": kind_label,
            "verdict": g.verdict,
            "strength": tr_dyn(g.strength),
            "played": {"label": pairs[played].0, "prob": pairs[played].1, "rank": g.rank},
            "top": {"label": pairs[g.top].0, "prob": pairs[g.top].1},
            "alternatives": ranked.iter().take(3).map(|(l, p)| json!({"label": l, "prob": p})).collect::<Vec<_>>(),
            "peeked": truthy(get(msg, "peeked")),
        });
        let pos = self.log_position(&node);
        record["log_name"] = pos["log_name"].clone();
        record["log_idx"] = pos["log_idx"].clone();
        if decision == "map_select" {
            drop(st);
            if let Some(path) = self.map_paths(&entry, played, g.top) {
                record["path"] = path;
            }
            st = self.lock();
        }
        self.deliver(&mut st, &record);
        vec![record]
    }

    fn map_paths(&self, entry: &Entry<P, C>, played: usize, top: usize) -> Option<Value> {
        let ai = get(&entry.advice, "path");
        if !truthy(ai) {
            return None;
        }
        let mut out = json!({"ai": ai});
        if played == top {
            out["you"] = ai.clone();
        } else if let Some(map_path) = &self.inner.cfg.map_path {
            if let Some(you) = map_path(&entry.msg, played).filter(|y| !y.is_empty()) {
                out["you"] = Value::Array(you);
            }
        }
        Some(out)
    }

    // -- delivery ----------------------------------------------------------

    /// Log the record; send it only to a mod that advertised the capability.
    fn deliver(&self, st: &mut State<P, C>, record: &Value) {
        let kind = py::str_of(get(record, "kind"));
        if matches!(kind.as_str(), "turn" | "choice" | "recap") {
            let id = get(record, "id").clone();
            st.emitted
                .insert(id.clone(), get(record, "node_id").clone());
            st.emitted_records.insert(id, record.clone());
        }
        (self.inner.cfg.log)(record);
        if st.mod_coach >= summary_level(&kind) {
            (self.inner.cfg.send)(record);
        }
    }
}

/// `_assess`: the action's regret against its rows.
fn assess<P, C>(act: &mut Act<P, C>) {
    let rows = act.rows.clone().unwrap_or_default();
    if rows.is_empty() {
        act.failed = Some("the search ranked no moves".into());
        return;
    }
    if rows.len() == 1 {
        act.forced = true; // one legal move: nothing to compare it with
        return;
    }
    let step = sts2_workbench::advisors::DecisionStep {
        idx: 0,
        node_id: None,
        state: json!({"enemies": []}),
        chosen: act.native.clone().unwrap_or(Value::Null),
        turn: Value::Null,
        floor: Value::Null,
        act: Value::Null,
        best_line: Vec::new(),
    };
    act.regret = sts2_workbench::advisors::assess_decision(
        &step,
        &rows,
        sts2_workbench::advisors::DEFAULT_BLUNDER_DROP_PCT,
    );
    if act.regret.is_none() {
        act.failed = Some("played move has no row".into());
    }
}

/// `_learn_names`: the card / potion names the wire carries.
fn learn_names(names: &mut BTreeMap<String, String>, msg: &Value) {
    let mut cards: Vec<&Value> = list(msg, "hand").iter().collect();
    for pile in ["draw_pile", "discard_pile", "exhaust_pile"] {
        cards.extend(list(get(msg, pile), "cards"));
    }
    cards.extend(list(get(msg, "run_state"), "master_deck"));
    for c in cards {
        if c.is_object() && truthy(get(c, "id")) && truthy(get(c, "name")) {
            names
                .entry(norm(get(c, "id")))
                .or_insert_with(|| py::str_of(get(c, "name")).trim_end_matches('+').to_string());
        }
    }
    for p in list(msg, "potions") {
        if p.is_object() && truthy(get(p, "id")) && truthy(get(p, "name")) {
            names
                .entry(norm(get(p, "id")))
                .or_insert_with(|| py::str_of(get(p, "name")));
        }
    }
}

/// Name one engine action from the engine state it is played in.
fn label_engine(
    display: &Names,
    names: &BTreeMap<String, String>,
    last_msg: &Value,
    state: &Value,
    action: &Value,
) -> String {
    let lookup = |id: &str| {
        names
            .get(id)
            .cloned()
            .unwrap_or_else(|| display.display_name_str(id))
    };
    match get(action, "action").as_str() {
        Some("end_turn") => tr("end turn").into(),
        Some("play") => {
            let hand = list(get(state, "deck"), "hand");
            let card = in_range(hand.len(), get(action, "card_index"))
                .map_or(Value::Null, |i| hand[i].clone());
            let cid = norm(get(&card, "id"));
            let mut name = lookup(&cid);
            if name.is_empty() {
                name = "?".into();
            }
            if truthy(get(&card, "upgraded")) {
                name.push('+');
            }
            let ally = get(action, "ally_index");
            if !ally.is_null() {
                return match ally_label(list(state, "allies"), ally) {
                    Some(a) => format!("{name} ({a})"),
                    None => name,
                };
            }
            name + &target_suffix(last_msg, get(action, "target_index"))
        }
        Some("potion" | "drink_potion") => {
            let belt = list(get(state, "player"), "potions");
            let pid = in_range(belt.len(), get(action, "slot"))
                .map_or_else(String::new, |i| norm(&belt[i]));
            let mut name = lookup(&pid);
            if name.is_empty() {
                name = tr("potion").into();
            }
            trf("drink {name}", &[("name", &name)])
        }
        _ => {
            let kind = get(action, "action");
            let raw = if truthy(kind) {
                py::str_of(kind)
            } else {
                "?".into()
            };
            crate::combat_advice::action_words(&raw)
        }
    }
}

/// ` (Cultist)` when the fight has more than one enemy.
fn target_suffix(last_msg: &Value, target: &Value) -> String {
    let enemies = list(last_msg, "enemies");
    if enemies.len() <= 1 {
        return String::new();
    }
    enemy_label(enemies, target).map_or_else(String::new, |l| format!(" ({l})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A position as the stub engine reads it: the hand, and the board.
    #[derive(Debug, Clone)]
    struct Pos {
        hand: Vec<&'static str>,
    }

    struct Stub;

    impl Engine<Pos, ()> for Stub {
        fn position_info(&self, cs: Option<&Pos>, _msg: &Value) -> Value {
            let hand: Vec<Value> = cs
                .map(|p| p.hand.iter().map(|c| json!([c, false])).collect())
                .unwrap_or_default();
            json!({"hand": hand, "potions": [], "round": 3})
        }
        fn top_up(
            &self,
            _: &Pos,
            _: &Value,
            _: Option<&()>,
            _: u32,
        ) -> Result<Option<Value>, String> {
            Ok(None)
        }
        fn catch_up(&self, _: &Pos, _: Option<&()>, _: u32) -> Result<Vec<Value>, String> {
            Ok(Vec::new())
        }
        fn board(&self, _: Option<&Pos>) -> Option<Value> {
            None
        }
        fn ai_turn(
            &self,
            _: &Pos,
            _: Option<&Value>,
            _: Option<&()>,
            _: &Label,
        ) -> Result<(Vec<String>, Option<Value>), String> {
            Ok((vec!["Bash".into(), "end turn".into()], None))
        }
    }

    type Records = Arc<Mutex<Vec<Value>>>;

    fn grader() -> (CoachGrader<Pos, ()>, Records, Records) {
        let (sent, logged): (Records, Records) = Default::default();
        let (s, l) = (sent.clone(), logged.clone());
        let g = CoachGrader::new(GraderConfig {
            send: Box::new(move |r| s.lock().unwrap().push(r.clone())),
            log: Box::new(move |r| l.lock().unwrap().push(r.clone())),
            warn: Box::new(|_| {}),
            engine: Box::new(Stub),
            executor: Arc::new(InlineExecutor::default()),
            names: Arc::new(Names::default()),
            catchup_sims: 64,
            budget_s: 6.0,
            map_path: None,
            clock: Box::new(|| 0.0),
            locate: None,
            cache: Arc::new(Mutex::new(AdviceCache::new(ADVICE_CACHE_SIZE))),
        });
        g.on_advice_request(&json!({"capabilities": {"coach": 1}, "floor": 5}));
        (g, sent, logged)
    }

    fn row(stable_idx: i64, win: f64, hp: f64, visits: i64, best: bool) -> Value {
        json!({
            "stable_idx": stable_idx, "action": "?", "win_prob": win, "q": win,
            "visits": visits, "is_best": best, "target_index": null,
            "mean_final_hp": hp, "hp_n": visits,
        })
    }

    fn combat_entry(
        g: &CoachGrader<Pos, ()>,
        rid: &str,
        sid: i64,
        rows: Vec<Value>,
        hand: Vec<&'static str>,
    ) {
        let entry = g.cache_put(Entry {
            request_id: json!(rid),
            state_id: json!(sid),
            kind: EntryKind::Combat,
            rows: Some(rows),
            options: None,
            cs: Some(Pos { hand }),
            cfg: None,
            advice: json!({}),
            pv_labels: Vec::new(),
            msg: json!({"round": 3, "floor": 5, "hand": []}),
        });
        g.on_advice(entry.as_deref());
    }

    fn play(card: &str, rid: &str, pos: i64) -> Value {
        json!({
            "type": "human_action", "decision_type": "combat_action", "action": "play",
            "card": {"id": card, "upgraded": false}, "target_index": null,
            "advice_request_id": rid, "position_id": pos,
        })
    }

    fn end(rid: &str, pos: i64) -> Value {
        json!({
            "type": "human_action", "decision_type": "combat_action", "action": "end_turn",
            "advice_request_id": rid, "position_id": pos,
        })
    }

    fn turns(records: &Records) -> Vec<Value> {
        records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == "turn")
            .cloned()
            .collect()
    }

    #[test]
    fn a_turn_on_the_line_is_graded_best_and_a_wasted_end_turn_a_blunder() {
        let (g, sent, _) = grader();
        combat_entry(
            &g,
            "a1",
            1,
            vec![
                row(0, 0.95, 50.0, 400, true),
                row(1, 0.94, 45.0, 400, false),
            ],
            vec!["BASH", "DEFEND_IRONCLAD"],
        );
        g.on_human_action(&play("BASH", "a1", 1), None, &json!(10));
        combat_entry(
            &g,
            "a2",
            2,
            vec![
                row(0, 0.95, 50.0, 400, true),
                row(-1, 0.95, 50.0, 400, false),
            ],
            vec!["DEFEND_IRONCLAD"],
        );
        g.on_human_action(&end("a2", 2), None, &json!(11));
        let graded = turns(&sent);
        assert_eq!(graded.len(), 1);
        assert_eq!(graded[0]["verdict"], "best");
        assert_eq!(graded[0]["id"], "turn:10");

        combat_entry(
            &g,
            "a3",
            3,
            vec![
                row(0, 0.95, 50.0, 400, true),
                row(-1, 0.94, 38.0, 200, false),
            ],
            vec!["BASH"],
        );
        g.on_human_action(&end("a3", 3), None, &json!(12));
        let graded = turns(&sent);
        assert_eq!(graded[1]["verdict"], "blunder");
        assert_eq!(graded[1]["hp_drop"], json!(12.0));
        assert_eq!(graded[1]["ai"], json!(["Bash", "end turn"]));
    }

    #[test]
    fn advice_for_another_position_is_never_used() {
        let (g, sent, logged) = grader();
        combat_entry(
            &g,
            "a1",
            1,
            vec![
                row(0, 0.95, 50.0, 400, true),
                row(-1, 0.9, 40.0, 400, false),
            ],
            vec!["BASH"],
        );
        // The action was taken from position 2: the cached advice is stale,
        // and with no position to search the turn goes ungraded, silently.
        g.on_human_action(&end("a1", 2), None, &json!(5));
        assert!(turns(&sent).is_empty());
        let log = turns(&logged);
        assert_eq!(log[0]["verdict"], "ungraded");
        assert_eq!(log[0]["reason"], "no advice and no position to search");
    }

    #[test]
    fn a_choice_is_graded_by_agreement_and_an_undo_withdraws_it() {
        let (g, sent, _) = grader();
        let entry = g.cache_put(Entry {
            request_id: json!("s1"),
            state_id: json!(7),
            kind: EntryKind::Screen,
            rows: None,
            options: Some(vec![
                json!({"label": "Bash", "visit_share": 0.7, "index": 0, "action": "choose"}),
                json!({"label": "Clash", "visit_share": 0.2, "index": 1, "action": "choose"}),
                json!({"label": "Skip", "visit_share": 0.1, "index": 2, "action": "skip"}),
            ]),
            cs: None,
            cfg: None,
            advice: json!({}),
            pv_labels: Vec::new(),
            msg: json!({"type": "card_reward"}),
        });
        g.on_advice(entry.as_deref());
        let msg = json!({
            "decision_type": "card_reward", "choice": {"index": 1},
            "advice_request_id": "s1", "position_id": 7,
        });
        let out = g.on_human_action(&msg, None, &json!(20));
        assert_eq!(out[0]["verdict"], "flag");
        assert_eq!(out[0]["played"]["rank"], 2);
        let withdrawn = g.on_rewind(&json!({"type": "combat_undone"}), &[json!(1)]);
        assert_eq!(
            withdrawn,
            vec![json!({"kind": "withdraw", "ids": ["choice:20"]})]
        );
        assert_eq!(sent.lock().unwrap().len(), 2);
    }
}
