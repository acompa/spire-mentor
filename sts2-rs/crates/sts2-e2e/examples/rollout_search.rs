//! Stage-1 search probe: does a policy rollout improve the student's move?
//!
//! ```text
//! cargo run --release -p sts2-e2e --example rollout_search -- \
//!     WEIGHTS SEED_START N THREADS [scope=final] [k=3] [minp=0.05] \
//!     [skip=0.9] [batch=8] [maxn=32] [z=2.0]
//! ```
//!
//! Per seed the student plays greedily to the first combat decision in
//! `scope`, and the run forks there into two arms on the same true state:
//!
//! * **greedy**: the student's argmax to the end of the run;
//! * **search**: at every in-scope combat decision the top `k` classes with
//!   probability at least `minp` are each tried `batch` to `maxn` times. One
//!   try clones the run, re-deals what the player cannot see
//!   ([`E2eEnv::redeal_hidden`], one seed per try shared by all classes),
//!   applies the class and lets the student play greedily to the end of the
//!   RUN. The score is the run outcome, so no value estimate is involved. A
//!   challenger replaces the student's move only if its paired win-rate gain
//!   has `z` standard errors behind it. Decisions the student is sure of
//!   (top probability at least `skip`) are not searched.
//!
//! Scopes: `final` = the second act-3 boss fight (the fight's end is the
//! run's end), `act3boss` = both act-3 boss fights, `boss` = every boss
//! fight (rollouts get long).
//!
//! One JSON row per seed goes to stdout as it finishes; the summary (paired
//! wins, exact `McNemar` p) goes to stderr.

use std::io::Write;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sts2_e2e::env::{Decision, E2eEnv, Step};
use sts2_e2e::net::{E2eNet, PolicyRow};
use sts2_e2e::tokens::{encode_for, TokCache};

/// `Decision::act` during an act-3 fight (observed: boss fights read 1, 2, 3).
const ACT3: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Final,
    Act3Boss,
    Boss,
}

#[derive(Clone, Copy, Debug)]
struct Cfg {
    scope: Scope,
    k: usize,
    min_prob: f32,
    skip_prob: f32,
    batch: usize,
    max_n: usize,
    z: f64,
}

/// Counts the act-3 boss fights seen so far and says whether a decision is
/// in scope.
#[derive(Clone, Copy, Debug, Default)]
struct ScopeTracker {
    act3_boss_fights: usize,
    last_boss: Option<&'static str>,
}

impl ScopeTracker {
    fn hit(&mut self, scope: Scope, d: &Decision) -> bool {
        let Some(c) = d.combat.as_ref() else {
            return false;
        };
        if c.room != "boss" {
            return false;
        }
        // The two act-3 bosses stand on one map node, so the env tracks them
        // as one fight; the encounter id is what tells them apart.
        if d.act == ACT3 && c.encounter_id.is_some() && self.last_boss != c.encounter_id {
            self.last_boss = c.encounter_id;
            self.act3_boss_fights += 1;
        }
        match scope {
            Scope::Boss => true,
            Scope::Act3Boss => d.act == ACT3,
            Scope::Final => d.act == ACT3 && self.act3_boss_fights >= 2,
        }
    }
}

#[derive(Debug, Default)]
struct Stats {
    in_scope: u64,
    skipped: u64,
    searched: u64,
    no_signal: u64,
    rollouts: u64,
    forwards: u64,
    overrides: Vec<serde_json::Value>,
}

fn logits(net: &E2eNet, d: &Decision, env: &E2eEnv, cache: &mut TokCache) -> Vec<f32> {
    let row: PolicyRow = encode_for(net, d, env.rm(), cache)
        .unwrap_or_else(|e| panic!("tokenize: {e}"))
        .into();
    net.logits(&row)
}

fn softmax(lg: &[f32]) -> Vec<f32> {
    let m = lg.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let e: Vec<f32> = lg.iter().map(|x| (x - m).exp()).collect();
    let s: f32 = e.iter().sum();
    e.iter().map(|x| x / s).collect()
}

/// The student plays greedily from `step` to the end of the run.
fn rollout(
    net: &E2eNet,
    mut env: E2eEnv,
    mut cache: TokCache,
    mut step: Step,
    forwards: &mut u64,
) -> bool {
    loop {
        match step {
            Step::Terminal(t) => return t.won,
            Step::Decision(d) => {
                let c = sts2_e2e::net::argmax(&logits(net, &d, &env, &mut cache));
                *forwards += 1;
                step = env.step(c);
            }
        }
    }
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Mean and z of the paired differences `a[i] - b[i]`.
fn paired(a: &[f64], b: &[f64]) -> (f64, f64) {
    let count = a.len() as f64;
    let diffs: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let mean = diffs.iter().sum::<f64>() / count;
    let var = diffs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (count - 1.0).max(1.0);
    let se = (var / count).sqrt();
    let score = if se > 0.0 {
        mean / se
    } else if mean > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };
    (mean, score)
}

/// The run position a search starts from.
struct Root<'a> {
    env: &'a E2eEnv,
    cache: &'a TokCache,
    d: &'a Decision,
    seed: i64,
}

/// The class the search arm plays at `root.d`, whose logits are `lg`.
fn search(net: &E2eNet, cfg: &Cfg, root: &Root<'_>, lg: &[f32], st: &mut Stats) -> usize {
    let Root {
        env,
        cache,
        d,
        seed,
    } = *root;
    let probs = softmax(lg);
    let top = sts2_e2e::net::argmax(lg);
    st.in_scope += 1;
    let mut order: Vec<usize> = (0..probs.len()).filter(|&c| c != top).collect();
    order.sort_by(|&a, &b| probs[b].total_cmp(&probs[a]));
    let mut cands = vec![top];
    cands.extend(
        order
            .into_iter()
            .take(cfg.k.saturating_sub(1))
            .filter(|&c| probs[c] >= cfg.min_prob),
    );
    if probs[top] >= cfg.skip_prob || cands.len() < 2 {
        st.skipped += 1;
        return top;
    }
    st.searched += 1;
    let base = splitmix(seed.cast_unsigned() ^ ((d.row as u64) << 32));
    let mut wins: Vec<Vec<f64>> = vec![Vec::new(); cands.len()];
    let mut n = 0;
    while n < cfg.max_n {
        for i in n..n + cfg.batch {
            let deal = splitmix(base ^ i as u64);
            for (j, &c) in cands.iter().enumerate() {
                let mut e = env.clone();
                e.redeal_hidden(deal);
                let s = e.step(c);
                let won = rollout(net, e, cache.clone(), s, &mut st.forwards);
                st.rollouts += 1;
                wins[j].push(f64::from(u8::from(won)));
            }
        }
        n += cfg.batch;
        let flat = wins[1..].iter().all(|w| *w == wins[0]);
        if flat && n == cfg.batch {
            st.no_signal += 1;
            break;
        }
        let behind = wins[1..].iter().all(|w| paired(w, &wins[0]).0 <= 0.0);
        if behind && n >= 2 * cfg.batch {
            break;
        }
    }
    let (mut best, mut best_m, mut best_z) = (0, 0.0, 0.0);
    for j in 1..cands.len() {
        let (m, z) = paired(&wins[j], &wins[0]);
        if m > best_m {
            (best, best_m, best_z) = (j, m, z);
        }
    }
    if best == 0 || best_z < cfg.z {
        return top;
    }
    st.overrides.push(serde_json::json!({
        "row": d.row, "turn": d.turn(), "n": n,
        "p_top": probs[top], "p_pick": probs[cands[best]],
        "wr_top": wins[0].iter().sum::<f64>() / n as f64,
        "wr_pick": wins[best].iter().sum::<f64>() / n as f64,
        "z": if best_z.is_finite() { best_z } else { 99.0 },
    }));
    cands[best]
}

fn run_seed(net: &E2eNet, cfg: &Cfg, seed: i64) -> serde_json::Value {
    let t0 = Instant::now();
    let mut env = E2eEnv::new(seed);
    let mut cache = TokCache::new();
    let mut scope = ScopeTracker::default();
    let mut step = env.reset();
    let mut unused = 0;
    // The shared greedy prefix, up to the first in-scope decision.
    let fork = loop {
        match step {
            Step::Terminal(t) => {
                return serde_json::json!({
                    "seed": seed, "reached": false, "greedy": t.won, "search": t.won,
                    "floor": t.floor,
                });
            }
            Step::Decision(d) => {
                if scope.hit(cfg.scope, &d) {
                    break d;
                }
                let c = sts2_e2e::net::argmax(&logits(net, &d, &env, &mut cache));
                step = env.step(c);
            }
        }
    };
    let greedy = rollout(
        net,
        env.clone(),
        cache.clone(),
        Step::Decision(fork.clone()),
        &mut unused,
    );
    let prefix_s = t0.elapsed().as_secs_f64();

    let mut st = Stats::default();
    let mut step = Step::Decision(fork);
    // The fork decision was already counted by `scope.hit`; re-testing it is
    // idempotent (same fight), so the loop below treats it like any other.
    let (search_won, floor) = loop {
        match step {
            Step::Terminal(t) => break (t.won, t.floor),
            Step::Decision(d) => {
                let lg = logits(net, &d, &env, &mut cache);
                let c = if scope.hit(cfg.scope, &d) {
                    {
                        let root = Root {
                            env: &env,
                            cache: &cache,
                            d: &d,
                            seed,
                        };
                        search(net, cfg, &root, &lg, &mut st)
                    }
                } else {
                    sts2_e2e::net::argmax(&lg)
                };
                step = env.step(c);
            }
        }
    };
    serde_json::json!({
        "seed": seed, "reached": true, "greedy": greedy, "search": search_won,
        "floor": floor, "in_scope": st.in_scope, "skipped": st.skipped,
        "searched": st.searched, "no_signal": st.no_signal,
        "rollouts": st.rollouts, "forwards": st.forwards,
        "overrides": st.overrides, "prefix_s": prefix_s,
        "wall_s": t0.elapsed().as_secs_f64(),
    })
}

/// Exact two-sided `McNemar` (binomial) p for `b` vs `c` discordant pairs.
fn mcnemar(b: u64, c: u64) -> f64 {
    let n = b + c;
    if n == 0 {
        return 1.0;
    }
    let k = b.min(c);
    let mut term = 0.5_f64.powi(i32::try_from(n).expect("small n"));
    let mut tail = term;
    for i in 0..k {
        term *= (n - i) as f64 / (i + 1) as f64;
        tail += term;
    }
    (2.0 * tail).min(1.0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() >= 5,
        "usage: rollout_search WEIGHTS SEED_START N THREADS [key=value ...]"
    );
    let bytes = std::fs::read(&args[1]).expect("read weights");
    let net = Arc::new(E2eNet::from_bytes(&bytes).expect("weights"));
    let start: i64 = args[2].parse().expect("seed start");
    let n: i64 = args[3].parse().expect("n");
    let threads: usize = args[4].parse().expect("threads");
    let mut cfg = Cfg {
        scope: Scope::Final,
        k: 3,
        min_prob: 0.05,
        skip_prob: 0.9,
        batch: 8,
        max_n: 32,
        z: 2.0,
    };
    for kv in &args[5..] {
        let (key, v) = kv.split_once('=').expect("key=value");
        match key {
            "scope" => {
                cfg.scope = match v {
                    "final" => Scope::Final,
                    "act3boss" => Scope::Act3Boss,
                    "boss" => Scope::Boss,
                    _ => panic!("scope: final, act3boss or boss"),
                }
            }
            "k" => cfg.k = v.parse().expect("k"),
            "minp" => cfg.min_prob = v.parse().expect("minp"),
            "skip" => cfg.skip_prob = v.parse().expect("skip"),
            "batch" => cfg.batch = v.parse().expect("batch"),
            "maxn" => cfg.max_n = v.parse().expect("maxn"),
            "z" => cfg.z = v.parse().expect("z"),
            _ => panic!("unknown option {key}"),
        }
    }
    eprintln!("{cfg:?} seeds {start}..{} threads {threads}", start + n);

    let next = Arc::new(AtomicI64::new(start));
    let rows = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let wall = Instant::now();
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let (net, next, rows) = (Arc::clone(&net), Arc::clone(&next), Arc::clone(&rows));
            std::thread::spawn(move || loop {
                let seed = next.fetch_add(1, Ordering::Relaxed);
                if seed >= start + n {
                    break;
                }
                let r = run_seed(&net, &cfg, seed);
                let mut all = rows.lock().expect("lock");
                let mut out = std::io::stdout().lock();
                writeln!(out, "{r}").expect("stdout");
                out.flush().expect("flush");
                all.push(r);
            })
        })
        .collect();
    for h in handles {
        h.join().expect("worker");
    }
    let rows = rows.lock().expect("lock");
    let count =
        |f: &dyn Fn(&serde_json::Value) -> bool| rows.iter().filter(|r| f(r)).count() as u64;
    let flag = |r: &serde_json::Value, k: &str| r[k].as_bool().unwrap_or(false);
    let sum = |k: &str| rows.iter().filter_map(|r| r[k].as_u64()).sum::<u64>();
    let only_search = count(&|r| flag(r, "search") && !flag(r, "greedy"));
    let only_greedy = count(&|r| flag(r, "greedy") && !flag(r, "search"));
    let summary = serde_json::json!({
        "runs": rows.len(),
        "reached": count(&|r| flag(r, "reached")),
        "greedy_wins": count(&|r| flag(r, "greedy")),
        "search_wins": count(&|r| flag(r, "search")),
        "only_search": only_search,
        "only_greedy": only_greedy,
        "mcnemar_p": mcnemar(only_search, only_greedy),
        "in_scope": sum("in_scope"),
        "skipped": sum("skipped"),
        "searched": sum("searched"),
        "no_signal": sum("no_signal"),
        "rollouts": sum("rollouts"),
        "forwards": sum("forwards"),
        "overrides": rows.iter().filter_map(|r| r["overrides"].as_array()).map(Vec::len).sum::<usize>(),
        "wall_s": wall.elapsed().as_secs_f64(),
    });
    eprintln!("{summary}");
}
