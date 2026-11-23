//! `sts2_rs.screen_*`: a screen's advice from the run-policy net's scores
//! (`sts2_advisor::screen_advice`, GH #1426). `realgame_advisor` keeps its
//! Python names (`screen_labels`, `_unranked`, `_decline_potion_swap`,
//! `_blitz_shop_answer`) and delegates.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::screen_advice::{self as sa, Decision, ScreenContext};

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

fn list(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Vec<Value>> {
    match json(arg, name)? {
        Value::Array(a) => Ok(a),
        Value::Null => Ok(Vec::new()),
        _ => Err(PyValueError::new_err(format!("{name} must be a list"))),
    }
}

/// `screen_labels`: the decision's option labels as the player reads them.
#[pyfunction]
fn screen_labels(
    state: &Bound<'_, PyAny>,
    advice_for: &str,
    actions: &Bound<'_, PyAny>,
    responses: &Bound<'_, PyAny>,
    labels: Vec<String>,
    names: &crate::display_names::PyDisplayNames,
) -> PyResult<Vec<String>> {
    let (actions, responses) = (list(actions, "actions")?, list(responses, "responses")?);
    let decision = Decision {
        actions: &actions,
        responses: &responses,
        labels: &labels,
        source_note: "",
    };
    Ok(sa::screen_labels(
        &json(state, "state")?,
        advice_for,
        &decision,
        &names.names,
    ))
}

/// The shaping half of `compute_screen_advice`: `(advice, best)` from the
/// net's per-candidate `probs` and state `value` (`best` indexes the
/// decision's candidates). `carried`: the option the shop already chose.
#[pyfunction]
#[pyo3(signature = (
    state, advice_for, actions, responses, labels, source_note, probs, value, names,
    *, carried = None, trained_surface = false, standin_note = "", fight_offer = false,
))]
#[allow(clippy::too_many_arguments)]
fn screen_rank<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
    advice_for: &str,
    actions: &Bound<'py, PyAny>,
    responses: &Bound<'py, PyAny>,
    labels: Vec<String>,
    source_note: Option<String>,
    probs: Vec<f64>,
    value: Option<f64>,
    names: &crate::display_names::PyDisplayNames,
    carried: Option<usize>,
    trained_surface: bool,
    standin_note: &str,
    fight_offer: bool,
) -> PyResult<(Bound<'py, PyAny>, usize)> {
    let (actions, responses) = (list(actions, "actions")?, list(responses, "responses")?);
    let source_note = source_note.unwrap_or_default();
    let decision = Decision {
        actions: &actions,
        responses: &responses,
        labels: &labels,
        source_note: &source_note,
    };
    let ctx = ScreenContext {
        carried,
        trained_surface,
        standin_note,
        fight_offer,
        grouped: sa::group_decode_enabled(),
    };
    let ranked = sa::rank(
        &json(state, "state")?,
        advice_for,
        &decision,
        &probs,
        value,
        &ctx,
        &names.names,
    );
    Ok((crate::value_to_py(py, &ranked.advice)?, ranked.best))
}

/// `_unranked`: `(advice, warning)`; `warning` is the structured line to
/// log when `trigger` names the way in.
#[pyfunction]
#[pyo3(signature = (state, why, trigger = None))]
fn screen_unranked<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
    why: &str,
    trigger: Option<&str>,
) -> PyResult<(Bound<'py, PyAny>, Option<String>)> {
    let (advice, warning) = sa::unranked(&json(state, "state")?, why, trigger);
    Ok((crate::value_to_py(py, &advice)?, warning))
}

/// `_decline_potion_swap`.
#[pyfunction]
fn screen_decline_potion_swap<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
    names: &crate::display_names::PyDisplayNames,
) -> PyResult<Bound<'py, PyAny>> {
    crate::value_to_py(
        py,
        &sa::decline_potion_swap(&json(state, "state")?, &names.names),
    )
}

/// `_blitz_shop_answer`: `(advice, warning)`.
#[pyfunction]
fn screen_blitz_shop<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyAny>, String)> {
    let (advice, warning) = sa::blitz_shop_answer(&json(state, "state")?);
    Ok((crate::value_to_py(py, &advice)?, warning))
}

/// `realgame_ppo.is_blitz_shop`.
#[pyfunction]
fn screen_is_blitz_shop(state: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(sa::is_blitz_shop(&json(state, "state")?))
}

/// `realgame_ppo.blitz_shop_choice`: `(index, why)`.
#[pyfunction]
fn screen_blitz_shop_choice(state: &Bound<'_, PyAny>) -> PyResult<(i64, &'static str)> {
    Ok(sa::blitz_shop_choice(&json(state, "state")?))
}

/// `realgame_ppo._strip_bbcode`.
#[pyfunction]
fn screen_strip_bbcode(text: &str) -> String {
    sa::strip_bbcode(text)
}

/// `ppo_decode.select_index` with the decode named (`grouped`).
#[pyfunction]
fn ppo_select_index(actions: &Bound<'_, PyAny>, probs: Vec<f64>, grouped: bool) -> PyResult<usize> {
    if probs.is_empty() {
        return Err(PyValueError::new_err("argmax_first: empty list"));
    }
    Ok(sa::select_index(
        &list(actions, "actions")?,
        &probs,
        grouped,
    ))
}

/// `_project_map_path`: the run-policy net's route from the chosen map node
/// (`best_index` into the screen's `nodes`), re-asked at every fork.
#[pyfunction]
fn screen_map_path<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
    best_index: usize,
    policy: &crate::ppo_policy::PyPpoNetPolicy,
) -> PyResult<Vec<Bound<'py, PyAny>>> {
    use sts2_runai::ppo::encoder;
    let state = json(state, "state")?;
    let mut score = |obs: &sts2_runai::ppo::obs::PpoObs,
                     actions: &[sts2_runai::ppo::obs::ActionDesc]|
     -> Option<Vec<f64>> {
        let feat = encoder::featurize_with(obs, actions, policy.features);
        // Over the export budget, the Python backend raised and the walk
        // stopped there.
        if feat.obs_cat.len() + feat.n_actions > policy.net.t_max()
            || feat.n_actions > policy.net.a_max()
        {
            return None;
        }
        let (probs, _value) = policy.net.infer(&feat).ok()?;
        Some(probs.into_iter().map(f64::from).collect())
    };
    sa::project_map_path(&state, best_index, &mut score)
        .iter()
        .map(|step| crate::value_to_py(py, step))
        .collect()
}

/// The session keys `sts2_advisor::session` reads and writes; the rest of
/// the dict (the lanes' own keys) is left alone.
const SESSION_KEYS: [&str; 4] = [
    "last_map",
    "map_pos",
    "shop_remove_target",
    "card_reward_skipped",
];

/// A session dict's keys of [`SESSION_KEYS`], as JSON.
pub(crate) fn session_in(
    d: &Bound<'_, pyo3::types::PyDict>,
) -> PyResult<serde_json::Map<String, Value>> {
    let mut m = serde_json::Map::new();
    for k in SESSION_KEYS {
        if let Some(v) = d.get_item(k)? {
            m.insert(k.to_string(), json(&v, k)?);
        }
    }
    Ok(m)
}

/// Write [`SESSION_KEYS`] back into the caller's dict, in place. The
/// pairs (`last_map`, `map_pos`, `shop_remove_target`) go back as the
/// tuples the Python lanes stored.
pub(crate) fn session_out(
    d: &Bound<'_, pyo3::types::PyDict>,
    m: serde_json::Map<String, Value>,
) -> PyResult<()> {
    let py = d.py();
    for k in SESSION_KEYS {
        match m.get(k) {
            Some(Value::Array(pair)) if k != "card_reward_skipped" => {
                let items = pair
                    .iter()
                    .map(|v| crate::value_to_py(py, v))
                    .collect::<PyResult<Vec<_>>>()?;
                d.set_item(k, pyo3::types::PyTuple::new(py, items)?)?;
            }
            Some(v) => d.set_item(k, crate::value_to_py(py, v)?)?,
            None => {
                if d.contains(k)? {
                    d.del_item(k)?;
                }
            }
        }
    }
    Ok(())
}

/// `realgame_ppo.apply_session_context`'s work: update `session` from the
/// message and return the act graph to patch in, or `None`.
#[pyfunction]
fn session_apply<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
    session: &Bound<'py, pyo3::types::PyDict>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let mut m = session_in(session)?;
    let patched = sts2_advisor::session::apply_session_context(&json(state, "state")?, &mut m);
    session_out(session, m)?;
    patched.map(|v| crate::value_to_py(py, &v)).transpose()
}

/// `realgame_ppo.stash_shop_removal_target`.
#[pyfunction]
fn session_stash_shop_removal(
    session: &Bound<'_, pyo3::types::PyDict>,
    run_state: &Bound<'_, PyAny>,
    picked: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let mut m = session_in(session)?;
    sts2_advisor::session::stash_shop_removal_target(
        &mut m,
        &json(run_state, "run_state")?,
        &json(picked, "picked")?,
    );
    session_out(session, m)
}

/// `realgame_ppo.shop_removal_carry_index`: `(index, warning)`; consumes
/// the stash.
#[pyfunction]
fn session_shop_removal_carry(
    state: &Bound<'_, PyAny>,
    session: &Bound<'_, pyo3::types::PyDict>,
    n_responses: usize,
) -> PyResult<(Option<usize>, Option<String>)> {
    let mut m = session_in(session)?;
    let out = sts2_advisor::session::shop_removal_carry_index(
        &json(state, "state")?,
        &mut m,
        n_responses,
    );
    session_out(session, m)?;
    Ok(out)
}

/// `crystal_sphere_policy.choose`: `(action, warning)`, `action` `None`
/// when no enabled option is usable.
#[pyfunction]
fn crystal_sphere_choose<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
) -> PyResult<(Option<Bound<'py, PyAny>>, Option<String>)> {
    let c = sts2_advisor::crystal_sphere::choose(&json(state, "state")?);
    let action = c.action.map(|a| crate::value_to_py(py, &a)).transpose()?;
    Ok((action, c.warning))
}

/// `crystal_sphere_policy.pick_cell`.
#[pyfunction]
fn crystal_sphere_pick_cell(hidden: Vec<(i64, i64)>) -> Option<usize> {
    sts2_advisor::crystal_sphere::pick_cell(&hidden)
}

/// `_crystal_sphere_advice`: `(advice, warning)`.
#[pyfunction]
fn crystal_sphere_advice<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyAny>, Option<String>)> {
    let (advice, warning) = sts2_advisor::crystal_sphere::advice(&json(state, "state")?);
    Ok((crate::value_to_py(py, &advice)?, warning))
}

/// The Rust run-policy net, scored as `PpoNetPolicy.distribution` scores
/// it (the same budget check and error wording).
struct RustNet {
    net: std::sync::Arc<sts2_runai::ppo::net::PpoNet>,
    features: sts2_runai::ppo::features::Features,
}

impl sa::ScreenNet for RustNet {
    fn score(
        &self,
        obs: &sts2_runai::ppo::obs::PpoObs,
        actions: &[sts2_runai::ppo::obs::ActionDesc],
    ) -> Result<(Vec<f64>, f64), String> {
        let feat = sts2_runai::ppo::encoder::featurize_with(obs, actions, self.features);
        let (obs_len, n_actions) = (feat.obs_cat.len(), feat.n_actions);
        if obs_len + n_actions > self.net.t_max() || n_actions > self.net.a_max() {
            return Err(format!(
                "decision exceeds export budget: obs={obs_len} act={n_actions} \
                 (t_max={} a_max={})",
                self.net.t_max(),
                self.net.a_max()
            ));
        }
        let (probs, value) = self
            .net
            .infer(&feat)
            .map_err(|e| format!("ppo policy: inference failed: {e}"))?;
        Ok((probs.into_iter().map(f64::from).collect(), f64::from(value)))
    }

    fn trained_on_packs(&self) -> bool {
        self.features.card_bundles
    }
}

/// `compute_screen_advice` (`sts2_advisor::screen_advice`). Returns
/// `{"advice", "state", "log"}`: `advice` is `None` when the character's
/// policy is another backend's, and `state` is then the session-applied
/// state for the Python lane to answer. `policy_for(character)` returns a
/// `PpoNetPolicy` (the Rust net), a string (no net: why), or `None` (another
/// backend). `log` is `(level, logger, message)`; the mirror keeps its own.
#[pyfunction]
#[pyo3(signature = (state, advice_for, policy_for, names, wire_for, *, session = None, mirror = None, cfg = None, sims = None))]
#[allow(clippy::too_many_arguments)]
fn screen_compute<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
    advice_for: &str,
    policy_for: &Bound<'py, PyAny>,
    names: &crate::display_names::PyDisplayNames,
    wire_for: Py<PyAny>,
    session: Option<&Bound<'py, pyo3::types::PyDict>>,
    mirror: Option<&Bound<'py, crate::advisor_mirror::PyAdvisorMirror>>,
    cfg: Option<&Bound<'py, PyAny>>,
    sims: Option<u32>,
) -> PyResult<Bound<'py, PyAny>> {
    use sts2_advisor::screen_advice::{compute_screen_advice, Lookup, Outcome, ScreenEnv};

    let state = json(state, "state")?;
    let cfg = cfg
        .filter(|c| !c.is_none())
        .map(|c| json(c, "cfg").map(|v| sts2_advisor::search::RecommendConfig::from_value(&v)))
        .transpose()?;
    let mut session_map = session.map(session_in).transpose()?;
    let mut held = mirror.map(Bound::borrow_mut);
    let mut lookup_error: Option<PyErr> = None;
    let mut policy_lookup = |character: &str| -> Lookup {
        let found = match policy_for.call1((character,)) {
            Ok(found) => found,
            Err(e) => {
                lookup_error = Some(e);
                return Lookup::OtherBackend;
            }
        };
        if found.is_none() {
            return Lookup::OtherBackend;
        }
        if let Ok(why) = found.extract::<String>() {
            return Lookup::Missing(why);
        }
        match found.extract::<PyRef<'_, crate::ppo_policy::PyPpoNetPolicy>>() {
            Ok(p) => Lookup::Net(Box::new(RustNet {
                net: p.net.clone(),
                features: p.features,
            })),
            Err(e) => {
                lookup_error = Some(e);
                Lookup::OtherBackend
            }
        }
    };
    let mut wire_of = |combat: &Value, character: Option<&str>, asc: i64, id: i64, seed: u64| {
        crate::advisor_mirror::call_wire_for(&wire_for, combat, character, asc, id, seed)
    };
    let mut env = ScreenEnv {
        session: session_map.as_mut(),
        mirror: held.as_deref_mut().map(|c| &mut c.core),
        cfg: cfg.as_ref(),
        sims,
        names: &names.names,
        wire_for: &mut wire_of,
        policy_for: &mut policy_lookup,
        log: Vec::new(),
    };
    let outcome = compute_screen_advice(&state, advice_for, &mut env);
    let log: Vec<(&'static str, &'static str, String)> = std::mem::take(&mut env.log)
        .into_iter()
        .map(|(level, logger, msg)| (crate::advisor_mirror::level_name(level), logger, msg))
        .collect();
    drop(env);
    drop(held);
    if let (Some(d), Some(m)) = (session, session_map) {
        session_out(d, m)?;
    }
    if let Some(e) = lookup_error {
        return Err(e);
    }
    let out = match outcome {
        Outcome::Advice(advice) => serde_json::json!({"advice": advice, "state": null, "log": log}),
        Outcome::OtherBackend(state) => {
            serde_json::json!({"advice": null, "state": state, "log": log})
        }
    };
    crate::value_to_py(py, &out)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(screen_compute, m)?)?;
    m.add_function(wrap_pyfunction!(crystal_sphere_choose, m)?)?;
    m.add_function(wrap_pyfunction!(crystal_sphere_pick_cell, m)?)?;
    m.add_function(wrap_pyfunction!(crystal_sphere_advice, m)?)?;
    m.add_function(wrap_pyfunction!(session_apply, m)?)?;
    m.add_function(wrap_pyfunction!(session_stash_shop_removal, m)?)?;
    m.add_function(wrap_pyfunction!(session_shop_removal_carry, m)?)?;
    m.add_function(wrap_pyfunction!(screen_map_path, m)?)?;
    m.add_function(wrap_pyfunction!(screen_labels, m)?)?;
    m.add_function(wrap_pyfunction!(screen_rank, m)?)?;
    m.add_function(wrap_pyfunction!(screen_unranked, m)?)?;
    m.add_function(wrap_pyfunction!(screen_decline_potion_swap, m)?)?;
    m.add_function(wrap_pyfunction!(screen_blitz_shop, m)?)?;
    m.add_function(wrap_pyfunction!(screen_is_blitz_shop, m)?)?;
    m.add_function(wrap_pyfunction!(screen_blitz_shop_choice, m)?)?;
    m.add_function(wrap_pyfunction!(screen_strip_bbcode, m)?)?;
    m.add_function(wrap_pyfunction!(ppo_select_index, m)?)?;
    Ok(())
}
