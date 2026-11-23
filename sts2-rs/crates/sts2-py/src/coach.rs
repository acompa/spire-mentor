//! `sts2_rs.coach_*`: the live coach's pure helpers
//! (`sts2_advisor::coach`, GH #1426 step 3). `realgame_coach` keeps its
//! Python names over these.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::coach;

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

fn opt_json(arg: Option<&Bound<'_, PyAny>>, name: &str) -> PyResult<Value> {
    arg.map_or(Ok(Value::Null), |a| json(a, name))
}

fn list(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Vec<Value>> {
    match json(arg, name)? {
        Value::Array(a) => Ok(a),
        Value::Null => Ok(Vec::new()),
        _ => Err(PyValueError::new_err(format!("{name} must be a list"))),
    }
}

fn out(py: Python<'_>, v: Option<Value>) -> PyResult<Option<Bound<'_, PyAny>>> {
    v.map(|v| crate::value_to_py(py, &v)).transpose()
}

type Events = Vec<(String, i64)>;

#[pyfunction]
fn coach_match_row_index(
    rows: &Bound<'_, PyAny>,
    native: &Bound<'_, PyAny>,
) -> PyResult<Option<usize>> {
    Ok(coach::match_row_index(
        &list(rows, "rows")?,
        &json(native, "native")?,
    ))
}

#[pyfunction]
fn coach_played_needs_top_up(rows: &Bound<'_, PyAny>, native: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(coach::played_needs_top_up(
        &list(rows, "rows")?,
        &json(native, "native")?,
    ))
}

/// `(index, fields)` the played row is updated with, or `None`.
#[pyfunction]
fn coach_top_up_fields<'py>(
    py: Python<'py>,
    rows: &Bound<'py, PyAny>,
    native: &Bound<'py, PyAny>,
    fresh: &Bound<'py, PyAny>,
) -> PyResult<Option<(usize, Bound<'py, PyAny>)>> {
    coach::top_up_fields(
        &list(rows, "rows")?,
        &json(native, "native")?,
        &json(fresh, "fresh")?,
    )
    .map(|(i, f)| Ok((i, crate::value_to_py(py, &f)?)))
    .transpose()
}

#[pyfunction]
fn coach_native_from_row<'py>(
    py: Python<'py>,
    row: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    out(py, coach::native_from_row(&json(row, "row")?))
}

#[pyfunction]
fn coach_native_action<'py>(
    py: Python<'py>,
    msg: &Bound<'py, PyAny>,
    info: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    out(
        py,
        coach::native_action(&json(msg, "msg")?, &json(info, "info")?),
    )
}

#[pyfunction]
#[pyo3(signature = (intents = None))]
fn coach_intent_text(intents: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
    Ok(coach::intent_text(&opt_json(intents, "intents")?))
}

#[pyfunction]
#[pyo3(signature = (msg = None))]
fn coach_turn_context<'py>(
    py: Python<'py>,
    msg: Option<&Bound<'py, PyAny>>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    out(py, coach::turn_context(&opt_json(msg, "msg")?))
}

#[pyfunction]
fn coach_board_events(
    start: &Bound<'_, PyAny>,
    end: &Bound<'_, PyAny>,
) -> PyResult<Vec<(&'static str, i64)>> {
    Ok(coach::board_events(
        &json(start, "start")?,
        &json(end, "end")?,
    ))
}

#[pyfunction]
#[pyo3(signature = (you, ai, names, turn = None))]
fn coach_why_text(
    you: Events,
    ai: Events,
    names: Vec<String>,
    turn: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<String>> {
    Ok(coach::why_text(&you, &ai, &names, &opt_json(turn, "turn")?))
}

#[pyfunction]
#[pyo3(signature = (turns, choices, k = coach::AUTOPSY_ITEMS))]
fn coach_rank_autopsy<'py>(
    py: Python<'py>,
    turns: &Bound<'py, PyAny>,
    choices: &Bound<'py, PyAny>,
    k: usize,
) -> PyResult<Vec<Bound<'py, PyAny>>> {
    coach::rank_autopsy(&list(turns, "turns")?, &list(choices, "choices")?, k)
        .iter()
        .map(|v| crate::value_to_py(py, v))
        .collect()
}

#[pyfunction]
fn coach_build_recap<'py>(
    py: Python<'py>,
    fight: &Bound<'py, PyAny>,
    end_msg: &Bound<'py, PyAny>,
    turn_records: &Bound<'py, PyAny>,
    position: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    out(
        py,
        coach::build_recap(
            &json(fight, "fight")?,
            &json(end_msg, "end_msg")?,
            &list(turn_records, "turn_records")?,
            &json(position, "position")?,
        ),
    )
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(coach_match_row_index, m)?)?;
    m.add_function(wrap_pyfunction!(coach_played_needs_top_up, m)?)?;
    m.add_function(wrap_pyfunction!(coach_top_up_fields, m)?)?;
    m.add_function(wrap_pyfunction!(coach_native_from_row, m)?)?;
    m.add_function(wrap_pyfunction!(coach_native_action, m)?)?;
    m.add_function(wrap_pyfunction!(coach_intent_text, m)?)?;
    m.add_function(wrap_pyfunction!(coach_turn_context, m)?)?;
    m.add_function(wrap_pyfunction!(coach_board_events, m)?)?;
    m.add_function(wrap_pyfunction!(coach_why_text, m)?)?;
    m.add_function(wrap_pyfunction!(coach_rank_autopsy, m)?)?;
    m.add_function(wrap_pyfunction!(coach_build_recap, m)?)?;
    m.add("COACH_RECAP_COSTLIEST", coach::RECAP_COSTLIEST)?;
    m.add("COACH_AUTOPSY_ITEMS", coach::AUTOPSY_ITEMS)?;
    Ok(())
}
