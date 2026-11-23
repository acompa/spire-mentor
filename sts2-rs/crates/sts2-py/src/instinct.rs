//! `sts2_rs.instinct_*`: Instinct's advice shaping (`sts2_advisor::instinct`,
//! GH #1426 step 3). `realgame_instinct_screens` keeps its Python names.

use std::collections::BTreeMap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::instinct::{self, Pick};

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

fn pick(decision: &Bound<'_, PyAny>, probs: Vec<f64>, best: usize, ms: f64) -> PyResult<Pick> {
    Ok(Pick {
        decision: json(decision, "decision")?,
        probs,
        best,
        ms,
    })
}

/// `realgame_instinct.note_for`.
#[pyfunction]
fn instinct_note_for(decision: &Bound<'_, PyAny>, ms: f64, p_best: f64) -> PyResult<String> {
    Ok(instinct::note_for(
        &pick(decision, Vec::new(), 0, ms)?,
        p_best,
    ))
}

/// `realgame_instinct_screens.screen_advice`: `(advice, None)`, or
/// `(None, why)` when no class names anything on this screen.
#[pyfunction]
#[pyo3(signature = (decision, probs, best, ms, rows, note_extra = ""))]
#[allow(clippy::needless_pass_by_value)] // PyO3 extracts owned values
fn instinct_screen_advice<'py>(
    py: Python<'py>,
    decision: &Bound<'py, PyAny>,
    probs: Vec<f64>,
    best: usize,
    ms: f64,
    rows: BTreeMap<usize, Option<(Bound<'py, PyAny>, String)>>,
    note_extra: &str,
) -> PyResult<(Option<Bound<'py, PyAny>>, Option<String>)> {
    let n = probs.len();
    let mut table: Vec<Option<(Value, String)>> = vec![None; n];
    for (ci, row) in rows {
        if let (Some(slot), Some((resp, label))) = (table.get_mut(ci), row) {
            *slot = Some((json(&resp, "response")?, label));
        }
    }
    match instinct::screen_advice(&pick(decision, probs, best, ms)?, &table, note_extra) {
        Ok(v) => Ok((Some(crate::value_to_py(py, &v)?), None)),
        Err(why) => Ok((None, Some(why))),
    }
}

/// `realgame_instinct_screens.lone_option`.
#[pyfunction]
fn instinct_lone_option<'py>(
    py: Python<'py>,
    option: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    crate::value_to_py(py, &instinct::lone_option(&json(option, "option")?))
}

/// `InstinctAdvisor.evaluate`: `(pick, None)` with `pick = {decision,
/// probs, best, ms, estimate}`, or `(None, why)` when Instinct cannot
/// answer here. `driver` keeps its combat observation; the peek and the
/// tokenizer run on a copy.
#[pyfunction]
fn instinct_evaluate<'py>(
    py: Python<'py>,
    student: &crate::e2e::PyE2eStudent,
    driver: &mut crate::e2e::PyE2eDriver,
    rm: &mut crate::run_manager::PyRunManager,
    expect: Vec<String>,
) -> PyResult<(Option<Bound<'py, PyAny>>, Option<String>)> {
    let crate::e2e::PyE2eDriver { driver, cache } = driver;
    match instinct::evaluate(&student.net, driver, cache, &mut rm.inner, &expect) {
        Ok(ev) => {
            let v = serde_json::json!({
                "decision": ev.pick.decision,
                "probs": ev.pick.probs,
                "best": ev.pick.best,
                "ms": ev.pick.ms,
                "estimate": ev.estimate,
            });
            Ok((Some(crate::value_to_py(py, &v)?), None))
        }
        Err(why) => Ok((None, Some(why))),
    }
}

/// The student behind a Rust screen handler: `driver_for(seed)` returns
/// this run's `E2eDriver` (the Python advisor keeps the per-run state), and
/// the first estimate evaluated is kept for the advice. Without a student
/// only the screens answered by rule (Claws, the Blitz merchant) answer.
struct PyEvaluator<'py, 'a> {
    student: Option<&'a crate::e2e::PyE2eStudent>,
    driver_for: Option<&'a Bound<'py, PyAny>>,
    estimate: Option<Value>,
    /// The last pick evaluated, for the advisor's `last_pick`.
    last: Option<Pick>,
}

impl sts2_advisor::instinct_screens::Evaluator for PyEvaluator<'_, '_> {
    fn evaluate(
        &mut self,
        rm: &mut sts2_core::run_manager::RunManager,
        expect: &[&str],
    ) -> Result<Pick, String> {
        let (Some(student), Some(driver_for)) = (self.student, self.driver_for) else {
            return Err("no Instinct net loaded".to_string());
        };
        let handle = driver_for.call1((rm.seed,)).map_err(|e| e.to_string())?;
        let mut driver: PyRefMut<'_, crate::e2e::PyE2eDriver> =
            handle.extract().map_err(|e: PyErr| e.to_string())?;
        let crate::e2e::PyE2eDriver { driver, cache } = &mut *driver;
        let expect: Vec<String> = expect.iter().map(|s| (*s).to_string()).collect();
        let ev = instinct::evaluate(&student.net, driver, cache, rm, &expect)?;
        if self.estimate.is_none() {
            self.estimate.clone_from(&ev.estimate);
        }
        self.last = Some(ev.pick.clone());
        Ok(ev.pick)
    }
}

/// One Instinct screen handler in Rust (`realgame_instinct_screens`).
///
/// Returns `{"advice", "why", "estimate", "pick"}` (plus `"shop_removal"`
/// for the shop): `advice` is `None` and `why` says why when Instinct cannot
/// answer; `pick` is the last position the student evaluated (`{decision,
/// probs, best, ms}`) or `None`. A malformed screen raises `ValueError`.
#[pyfunction]
#[pyo3(signature = (screen, student, driver_for, state, version, names, last_room = None, last_screen = None, shop_removal = None, event_track = None))]
#[allow(clippy::too_many_arguments)]
fn instinct_screen<'py>(
    py: Python<'py>,
    screen: &str,
    student: Option<&crate::e2e::PyE2eStudent>,
    driver_for: Option<&Bound<'py, PyAny>>,
    state: &Bound<'py, PyAny>,
    version: &str,
    names: &crate::display_names::PyDisplayNames,
    last_room: Option<&str>,
    last_screen: Option<&str>,
    shop_removal: Option<(String, bool, String)>,
    event_track: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    use sts2_advisor::graft::GraftError;
    use sts2_advisor::instinct_screens as screens;
    let state = json(state, "state")?;
    let mut ev = PyEvaluator {
        student,
        driver_for,
        estimate: None,
        last: None,
    };
    let mut removal: Option<Option<screens::CardKey>> = None;
    let mut track_out: Option<Option<Value>> = None;
    let track_in = event_track.map(|t| json(t, "event_track")).transpose()?;
    let out = match screen {
        "card_reward" => screens::card_reward(&mut ev, &state, version, last_room),
        "card_bundle" => screens::card_bundle(&mut ev, &state, version),
        "rest_site" => screens::rest_site(&mut ev, &state, version),
        "map_select" => screens::map(&mut ev, &state, version),
        "boss_relic" => screens::boss_relic(&mut ev, &state, version),
        "potion_swap" => screens::potion_swap(&mut ev, &state, version, &names.names),
        "card_select" => {
            screens::deck_select(&mut ev, &state, version, last_screen, shop_removal.as_ref())
        }
        "event" => {
            let (out, track) = screens::event(&mut ev, &state, version, track_in.as_ref());
            track_out = Some(track);
            out
        }
        "shop" => screens::shop(&mut ev, &state, version, &names.names).map(|(advice, r)| {
            removal = Some(r);
            advice
        }),
        other => {
            return Err(PyValueError::new_err(format!(
                "no Rust Instinct screen {other:?}"
            )))
        }
    };
    let pick = ev.last.map(|p| {
        serde_json::json!({"decision": p.decision, "probs": p.probs, "best": p.best, "ms": p.ms})
    });
    let (advice, why) = match out {
        Ok(advice) => (Some(advice), None),
        Err(GraftError::Unavailable(why)) => (None, Some(why)),
        Err(GraftError::Value(m)) => return Err(PyValueError::new_err(m)),
    };
    let mut result = serde_json::json!({
        "advice": advice,
        "why": why,
        "estimate": ev.estimate,
        "pick": pick,
    });
    if let Some(r) = removal {
        result["shop_removal"] = serde_json::json!(r);
    }
    if let Some(t) = track_out {
        result["event_track"] = t.unwrap_or(Value::Null);
    }
    crate::value_to_py(py, &result)
}

/// `realgame_instinct_screens.note_event_choice`: the event trail with the
/// page's taken option (a `human_action`) recorded.
#[pyfunction]
fn instinct_note_event_choice<'py>(
    py: Python<'py>,
    track: &Bound<'py, PyAny>,
    msg: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let t = sts2_advisor::instinct_screens::note_event_choice(
        &json(track, "track")?,
        &json(msg, "msg")?,
    );
    crate::value_to_py(py, &t)
}

/// `InstinctAdvisor._advise_combat` (`sts2_advisor::instinct_combat`):
/// the student's answer for a `combat_action` board on the grafted run `rm`
/// (entered into the fight here), with this run's `driver`. Returns
/// `{"advice", "why", "estimate", "plan_failed", "log"}` (`log`: the info
/// lines on the way): `advice` is `None` and
/// `why` the player-facing reason when Instinct cannot answer (it stood in
/// the fight all the same). `cfg` is `dataclasses.asdict(RecommendConfig)`
/// or `None`.
///
/// Raises:
///     ValueError: the board has no engine fight (`UnsupportedStateError`
///         and the wire's errors as `combat_wire` / `state_from_dict` raise
///         them).
#[pyfunction]
#[pyo3(signature = (student, driver, rm, state, cfg = None, mirror = None))]
fn instinct_advise_combat<'py>(
    py: Python<'py>,
    student: &crate::e2e::PyE2eStudent,
    driver: &mut crate::e2e::PyE2eDriver,
    rm: &mut crate::run_manager::PyRunManager,
    state: &Bound<'py, PyAny>,
    cfg: Option<&Bound<'py, PyAny>>,
    mirror: Option<&Bound<'py, crate::advisor_mirror::PyAdvisorMirror>>,
) -> PyResult<Bound<'py, PyAny>> {
    use sts2_advisor::instinct_combat::{advise_combat, InstinctCombatError};
    use sts2_advisor::search::RecommendConfig;

    let state = json(state, "state")?;
    let cfg = match cfg.filter(|c| !c.is_none()) {
        Some(c) => RecommendConfig::from_value(&json(c, "cfg")?),
        None => RecommendConfig::default(),
    };
    let mut held = mirror.map(Bound::borrow_mut);
    let core = held.as_deref_mut().map(|c| &mut c.core);
    let crate::e2e::PyE2eDriver { driver, cache } = driver;
    let mut log = Vec::new();
    let out = advise_combat(
        &student.net,
        driver,
        cache,
        &mut rm.inner,
        &state,
        &cfg,
        core,
        &mut log,
    );
    let out = match out {
        Ok(o) => serde_json::json!({
            "advice": o.advice,
            "why": null,
            "estimate": o.estimate,
            "plan_failed": o.plan_failed,
            "log": log,
        }),
        Err(InstinctCombatError::Unavailable(why)) => serde_json::json!({
            "advice": null, "why": why, "estimate": null, "plan_failed": null, "log": log,
        }),
        Err(InstinctCombatError::Board(e)) => return Err(crate::advisor_mirror::board_err(e)),
        Err(InstinctCombatError::State(e)) => return Err(crate::combat_mirror::err(py, e)),
    };
    drop(held);
    crate::value_to_py(py, &out)
}

/// `InstinctAdvisor._advise_combat_select`
/// (`sts2_advisor::instinct_combat::advise_choice`): an in-combat
/// `card_select` Instinct picks on the fight mirror's fork. Returns
/// `{"advice", "why", "log"}`: `advice` is `None` and `why` the
/// player-facing reason when Instinct cannot answer. Without a `student`
/// only a screen the mirror answers itself (a remembered pick, a confirm)
/// is answered. `driver_for(seed)`
/// returns this run's `E2eDriver`; `wire_for` is the mirror's board import
/// (`realgame_advisor_mirror._wire_for`); `cfg` is
/// `dataclasses.asdict(RecommendConfig)`; `version` the build the run is
/// rebuilt on.
#[pyfunction]
#[pyo3(signature = (student, driver_for, mirror, state, cfg, sims, version, wire_for))]
#[allow(clippy::too_many_arguments)]
fn instinct_answer_card_select<'py>(
    py: Python<'py>,
    student: Option<&crate::e2e::PyE2eStudent>,
    driver_for: &Bound<'py, PyAny>,
    mirror: &Bound<'py, crate::advisor_mirror::PyAdvisorMirror>,
    state: &Bound<'py, PyAny>,
    cfg: &Bound<'py, PyAny>,
    sims: Option<u32>,
    version: &str,
    wire_for: Py<PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    use sts2_advisor::instinct_combat::advise_choice;
    use sts2_advisor::search::RecommendConfig;

    let state = json(state, "state")?;
    let cfg = RecommendConfig::from_value(&json(cfg, "cfg")?);
    let mut driver_of = |seed: i64| {
        let call = || -> PyResult<_> {
            let handle = driver_for.call1((seed,))?;
            let d: PyRef<'_, crate::e2e::PyE2eDriver> = handle.extract()?;
            Ok((d.driver.clone(), d.cache.clone()))
        };
        call().map_err(|e| crate::advisor_mirror::py_exc(py, &e))
    };
    let mut wire_of = |combat: &Value, character: Option<&str>, asc: i64, id: i64, seed: u64| {
        crate::advisor_mirror::call_wire_for(&wire_for, combat, character, asc, id, seed)
    };
    let mut held = mirror.borrow_mut();
    let mut log = Vec::new();
    let out = advise_choice(
        student.map(|s| &*s.net),
        &mut driver_of,
        &mut held.core,
        &state,
        &cfg,
        sims,
        version,
        &mut wire_of,
        &mut log,
    );
    drop(held);
    let out = match out {
        Ok(advice) => serde_json::json!({"advice": advice, "why": null, "log": log}),
        Err(why) => serde_json::json!({"advice": null, "why": why, "log": log}),
    };
    crate::value_to_py(py, &out)
}

/// `realgame_instinct.enter_live_combat`: install the live fight `cs` on
/// the grafted run `rm` as a `room` (`monster` / `elite` / `boss`) fight,
/// naming the board's encounter (`encounter_id`) when the engine has it.
/// Returns the note when it did not.
#[pyfunction]
fn instinct_enter_live_combat(
    rm: &mut crate::run_manager::PyRunManager,
    cs: &crate::PyCombatState,
    state: &Bound<'_, PyAny>,
    room: &str,
) -> PyResult<Option<String>> {
    use sts2_advisor::instinct_combat::{enter_live_combat, room_kind};
    let room = match room {
        "monster" | "elite" | "boss" => room_kind(&serde_json::json!({"room_type": room})),
        other => {
            return Err(PyValueError::new_err(format!(
                "debug_enter_combat: room_type must be monster/elite/boss, got {other:?}"
            )))
        }
    };
    Ok(enter_live_combat(
        &mut rm.inner,
        cs.inner_ref().clone(),
        &json(state, "state")?,
        room,
    ))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(instinct_enter_live_combat, m)?)?;
    m.add_function(wrap_pyfunction!(instinct_answer_card_select, m)?)?;
    m.add_function(wrap_pyfunction!(instinct_advise_combat, m)?)?;
    m.add_function(wrap_pyfunction!(instinct_note_for, m)?)?;
    m.add_function(wrap_pyfunction!(instinct_screen_advice, m)?)?;
    m.add_function(wrap_pyfunction!(instinct_lone_option, m)?)?;
    m.add_function(wrap_pyfunction!(instinct_evaluate, m)?)?;
    m.add_function(wrap_pyfunction!(instinct_screen, m)?)?;
    m.add_function(wrap_pyfunction!(instinct_note_event_choice, m)?)?;
    Ok(())
}
