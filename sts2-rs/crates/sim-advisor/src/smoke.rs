//! `--smoke`: the packaged binary's self-check (build gate, GH #380 — was
//! `realgame_advisor._smoke`). No game, no bridge: the bundled localization
//! tables, EVERY bundled champion through a real map decision (the
//! featurizer and the forward pass are first touched at the first real
//! decision, so a load alone proves little), Instinct through 20 decisions
//! of a campaign run (a missing net fails unless `SIMADVISOR_NO_INSTINCT=1`),
//! and a socket bind. Prints `SMOKE OK` and returns `true` only when all pass.

use std::path::Path;

use serde_json::{json, Value};
use sts2_advisor::display_names;
use sts2_advisor::policy::PolicyCache;
use sts2_advisor::screen_advice::{compute_screen_advice, Outcome, ScreenEnv};

/// A synthetic `map_select` decision: small, character-agnostic, enough to
/// drive one full forward pass.
fn smoke_state(character: &str) -> Value {
    json!({
        "type": "map_select",
        "floor": 5,
        "act": 1,
        "run_state": {
            "character": character,
            "hp": 60, "max_hp": 75, "gold": 120, "ascension": 0,
            "master_deck": [{"id": "STRIKE_IRONCLAD"}, {"id": "DEFEND_IRONCLAD"}],
            "relics": [{"id": "BURNING_BLOOD"}],
            "potions": [],
        },
        "nodes": [
            {"index": 0, "type": "Monster", "row": 6, "col": 1},
            {"index": 1, "type": "Shop", "row": 6, "col": 3},
        ],
        "map": {
            "row_count": 10, "col_count": 6,
            "current": {"col": 2, "row": 5},
            "boss": {"col": 2, "row": 15},
            "visited": [], "nodes": [],
        },
    })
}

const CHARACTERS: [&str; 5] = ["Ironclad", "Silent", "Defect", "Regent", "Necrobinder"];

fn check_policies(root: &Path, names: &display_names::Names) -> Result<Vec<String>, String> {
    let policies = PolicyCache::new(root.to_path_buf());
    let mut lines = Vec::new();
    for character in CHARACTERS {
        let net = policies
            .get(character)
            .0
            .map_err(|e| format!("{character}: {e}"))?;
        let mut misses = Vec::new();
        let mut policy_for = |c: &str| policies.lookup(c, &mut misses);
        let mut wire_for = sts2_advisor::board::wire_for;
        let mut env = ScreenEnv {
            session: None,
            mirror: None,
            cfg: None,
            sims: None,
            names,
            wire_for: &mut wire_for,
            policy_for: &mut policy_for,
            log: Vec::new(),
        };
        let advice = match compute_screen_advice(&smoke_state(character), "map_select", &mut env) {
            Outcome::Advice(a) => a,
            Outcome::OtherBackend(_) => return Err(format!("{character}: no Rust net")),
        };
        let probs: Vec<f64> = advice
            .get("options")
            .and_then(Value::as_array)
            .map(|o| {
                o.iter()
                    .filter_map(|r| r.get("visit_share").and_then(Value::as_f64))
                    .collect()
            })
            .unwrap_or_default();
        let total: f64 = probs.iter().sum();
        if probs.len() != 2 || (total - 1.0).abs() > 1e-3 {
            return Err(format!(
                "{character}: bad distribution {probs:?} ({advice})"
            ));
        }
        lines.push(format!(
            "run policy OK: {character} (T_MAX={}, A_MAX={}, p={probs:?})",
            net.net.t_max(),
            net.net.a_max()
        ));
    }
    Ok(lines)
}

fn check_instinct(weights: &Path) -> Result<Option<String>, String> {
    use sts2_e2e::env::{E2eEnv, Step};
    if !weights.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(weights).map_err(|e| e.to_string())?;
    let net = sts2_e2e::net::E2eNet::from_bytes(&bytes)?;
    let seed = 170_000_000;
    let mut env = E2eEnv::over_run(sts2_e2e::env::new_run(seed), seed);
    let mut cache = sts2_e2e::tokens::TokCache::new();
    let mut step = env.reset();
    for played in 0..20 {
        match step {
            Step::Terminal(t) => {
                return Err(format!("self-play ended after {played} decisions: {t:?}"))
            }
            Step::Decision(d) => {
                let row = sts2_e2e::tokens::encode_for(&net, &d, env.rm(), &mut cache)?;
                let logits = net.try_logits(&row.into())?;
                step = env.step(sts2_e2e::net::argmax(&logits));
            }
        }
    }
    Ok(Some(format!(
        "instinct OK ({}, 20 decisions)",
        weights
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default()
    )))
}

/// Run every check, printing one line each; `true` when all passed.
#[must_use]
pub fn smoke(root: &Path, weights: &Path) -> bool {
    sts2_advisor::serve::install_engine();
    let mut ok = true;
    let names = display_names::load(root);
    let n = names.len();
    if n == 0 {
        ok = false;
        println!(
            "localization FAIL: no names loaded under {}",
            root.display()
        );
    } else {
        println!("localization OK ({n} names)");
    }
    match check_policies(root, &names) {
        Ok(lines) => lines.iter().for_each(|l| println!("{l}")),
        Err(e) => {
            ok = false;
            println!("run policy FAIL: {e}");
        }
    }
    match check_instinct(weights) {
        Ok(Some(line)) => println!("{line}"),
        // A missing net is a broken package unless it was built without one
        // on purpose: a SKIP here once let a dist ship with no working
        // Instinct (stage and binary disagreed on the file name).
        Ok(None) if std::env::var("SIMADVISOR_NO_INSTINCT").as_deref() == Ok("1") => println!(
            "instinct SKIP (no net at {}; SIMADVISOR_NO_INSTINCT=1)",
            weights.display()
        ),
        Ok(None) => {
            ok = false;
            println!(
                "instinct FAIL: no net at {} (set SIMADVISOR_NO_INSTINCT=1 for a build without it)",
                weights.display()
            );
        }
        Err(e) => {
            ok = false;
            println!("instinct FAIL: {e}");
        }
    }
    match std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()) {
        Ok(addr) => println!("socket OK (ephemeral port {})", addr.port()),
        Err(e) => {
            ok = false;
            println!("socket FAIL: {e}");
        }
    }
    println!("{}", if ok { "SMOKE OK" } else { "SMOKE FAIL" });
    ok
}
