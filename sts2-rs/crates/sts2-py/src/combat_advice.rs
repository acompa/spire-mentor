//! `sts2_rs.hint_*` / `advice_*`: a fight's advice from a ranking
//! (`sts2_advisor::combat_advice`, GH #1426). `sts2_hint.recommender`,
//! `realgame_advisor` and `realgame_instinct` keep their Python names; a
//! `HintTag` crosses as its `_asdict()`.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::combat_advice::{self as ca, NextPick, Tag, Unmapped};

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

fn tags_in(arg: &Bound<'_, PyAny>) -> PyResult<Vec<Tag>> {
    Ok(list(arg, "tags")?
        .iter()
        .enumerate()
        .map(|(i, v)| Tag::from_value(v, i))
        .collect())
}

fn tags_out<'py>(py: Python<'py>, tags: &[Tag]) -> PyResult<Vec<Bound<'py, PyAny>>> {
    tags.iter()
        .map(|t| crate::value_to_py(py, &t.to_value()))
        .collect()
}

fn tag_in(arg: &Bound<'_, PyAny>) -> PyResult<Tag> {
    Ok(Tag::from_value(&json(arg, "tag")?, 0))
}

/// `(kind, message)` of an unmappable action: `"unmappable"` (a play past
/// the hand) or `"unknown"` (outside the wire contract).
fn unmapped(e: Unmapped) -> (&'static str, String) {
    match e {
        Unmapped::Unmappable(m) => ("unmappable", m),
        Unmapped::Unknown(m) => ("unknown", m),
    }
}

/// `_stable_idx_for_action`: `(stable_idx, None)` or `(None, (kind, msg))`.
#[pyfunction]
fn hint_stable_idx_for_action(
    action: &Bound<'_, PyAny>,
    hand: &Bound<'_, PyAny>,
) -> PyResult<(Option<i64>, Option<(&'static str, String)>)> {
    Ok(
        match ca::stable_idx_for_action(&json(action, "action")?, &list(hand, "hand")?) {
            Ok(i) => (Some(i), None),
            Err(e) => (None, Some(unmapped(e))),
        },
    )
}

/// `tags_from_result`: `(tags, dropped, error)`; `error` is the
/// `(kind, msg)` of a row outside the wire contract.
#[pyfunction]
fn hint_tags_from_result<'py>(
    py: Python<'py>,
    result: &Bound<'py, PyAny>,
    hand: &Bound<'py, PyAny>,
) -> PyResult<(
    Vec<Bound<'py, PyAny>>,
    Vec<String>,
    Option<(&'static str, String)>,
)> {
    match ca::tags_from_result(&json(result, "result")?, &list(hand, "hand")?) {
        Ok((tags, dropped)) => Ok((tags_out(py, &tags)?, dropped, None)),
        Err(e) => Ok((Vec::new(), Vec::new(), Some(unmapped(e)))),
    }
}

#[pyfunction]
fn hint_crown_engine_heir<'py>(
    py: Python<'py>,
    tags: &Bound<'py, PyAny>,
) -> PyResult<Vec<Bound<'py, PyAny>>> {
    tags_out(py, &ca::crown_engine_heir(tags_in(tags)?))
}

#[pyfunction]
fn hint_choice_picks(
    tags: &Bound<'_, PyAny>,
    n_options: i64,
    min_select: i64,
    max_select: i64,
) -> PyResult<Vec<i64>> {
    Ok(ca::choice_picks_from_tags(
        &tags_in(tags)?,
        n_options,
        min_select,
        max_select,
    ))
}

/// `next_choice_pick_from_tags`: an option index, `"finalize"`, or `None`.
#[pyfunction]
fn hint_next_choice_pick<'py>(
    py: Python<'py>,
    tags: &Bound<'py, PyAny>,
    n_options: i64,
    taken: Vec<i64>,
    allow_finalize: bool,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    Ok(
        match ca::next_choice_pick_from_tags(&tags_in(tags)?, n_options, &taken, allow_finalize) {
            Some(NextPick::Option(i)) => Some(i.into_pyobject(py)?.into_any()),
            Some(NextPick::Finalize) => Some(pyo3::types::PyString::new(py, "finalize").into_any()),
            None => None,
        },
    )
}

#[pyfunction]
fn advice_tag_label(tag: &Bound<'_, PyAny>, state: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(ca::tag_label(&tag_in(tag)?, &json(state, "state")?))
}

#[pyfunction]
fn advice_pv_labels(pv: &Bound<'_, PyAny>, state: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    Ok(ca::pv_labels(&list(pv, "pv")?, &json(state, "state")?))
}

#[pyfunction]
fn advice_best_to_action<'py>(
    py: Python<'py>,
    tag: &Bound<'py, PyAny>,
    state: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    ca::best_to_action(&tag_in(tag)?, &json(state, "state")?)
        .map(|v| crate::value_to_py(py, &v))
        .transpose()
}

#[pyfunction]
fn advice_option_move<'py>(
    py: Python<'py>,
    tag: &Bound<'py, PyAny>,
    state: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    crate::value_to_py(py, &ca::option_move(&tag_in(tag)?, &json(state, "state")?))
}

#[pyfunction]
fn advice_veto_text(
    reason: &str,
    state: &Bound<'_, PyAny>,
    names: &crate::display_names::PyDisplayNames,
) -> PyResult<String> {
    Ok(ca::veto_text(reason, &json(state, "state")?, &names.names))
}

#[pyfunction]
fn advice_wire_unplayable(state: &Bound<'_, PyAny>) -> PyResult<Vec<(i64, String)>> {
    Ok(ca::wire_unplayable(&json(state, "state")?))
}

#[pyfunction]
fn advice_veto_unplayable<'py>(
    py: Python<'py>,
    tags: &Bound<'py, PyAny>,
    state: &Bound<'py, PyAny>,
) -> PyResult<(Vec<Bound<'py, PyAny>>, Vec<String>)> {
    let (kept, reasons) = ca::veto_unplayable(tags_in(tags)?, &json(state, "state")?);
    Ok((tags_out(py, &kept)?, reasons))
}

#[pyfunction]
#[pyo3(signature = (tags, state, sims, names, pv = None, pick = None))]
fn advice_build<'py>(
    py: Python<'py>,
    tags: &Bound<'py, PyAny>,
    state: &Bound<'py, PyAny>,
    sims: i64,
    names: &crate::display_names::PyDisplayNames,
    pv: Option<Vec<String>>,
    pick: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let pick = pick.map_or(Ok(Value::Null), |p| json(p, "pick"))?;
    let advice = ca::build_advice(
        tags_in(tags)?,
        &json(state, "state")?,
        sims,
        pv.as_deref(),
        &pick,
        &names.names,
    );
    crate::value_to_py(py, &advice)
}

#[pyfunction]
fn calibrated_p_win(raw: f64) -> f64 {
    ca::calibrated_p_win(raw)
}

/// `realgame_adapter.to_headless_combat_play`: a bridge `combat_action`
/// board in the headless `combat_play` dialect. `ValueError` for any other
/// board or an unsupported build.
#[pyfunction]
fn to_headless_combat_play<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let out = sts2_advisor::headless::to_headless_combat_play(&json(state, "state")?)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    crate::value_to_py(py, &out)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(hint_stable_idx_for_action, m)?)?;
    m.add_function(wrap_pyfunction!(hint_tags_from_result, m)?)?;
    m.add_function(wrap_pyfunction!(hint_crown_engine_heir, m)?)?;
    m.add_function(wrap_pyfunction!(hint_choice_picks, m)?)?;
    m.add_function(wrap_pyfunction!(hint_next_choice_pick, m)?)?;
    m.add_function(wrap_pyfunction!(advice_tag_label, m)?)?;
    m.add_function(wrap_pyfunction!(advice_pv_labels, m)?)?;
    m.add_function(wrap_pyfunction!(advice_best_to_action, m)?)?;
    m.add_function(wrap_pyfunction!(advice_option_move, m)?)?;
    m.add_function(wrap_pyfunction!(advice_veto_text, m)?)?;
    m.add_function(wrap_pyfunction!(advice_wire_unplayable, m)?)?;
    m.add_function(wrap_pyfunction!(advice_veto_unplayable, m)?)?;
    m.add_function(wrap_pyfunction!(advice_build, m)?)?;
    m.add_function(wrap_pyfunction!(calibrated_p_win, m)?)?;
    m.add_function(wrap_pyfunction!(to_headless_combat_play, m)?)?;
    Ok(())
}
