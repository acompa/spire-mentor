//! `sts2_rs.hand_select_*`: the in-combat card-selection heuristic
//! (`sts2_advisor::hand_select`, GH #1426 step 3). `realgame_hand_select`
//! keeps its Python names over these.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;
use sts2_advisor::hand_select::{self, Direction};

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

const fn direction_name(d: Option<Direction>) -> &'static str {
    match d {
        Some(d) => d.name(),
        None => "unknown",
    }
}

/// `realgame_hand_select.direction_of`: `(direction, key)`.
#[pyfunction]
fn hand_select_direction(state: &Bound<'_, PyAny>) -> PyResult<(&'static str, String)> {
    let (d, key) = hand_select::direction_of(&json(state, "state")?);
    Ok((direction_name(d), key))
}

/// `realgame_hand_select.card_value`.
#[pyfunction]
fn hand_select_card_value(card: &Bound<'_, PyAny>) -> PyResult<(i64, i64, i64)> {
    Ok(hand_select::card_value(&json(card, "card")?))
}

/// `realgame_hand_select.rank`.
#[pyfunction]
fn hand_select_rank<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    hand_select::rank(&json(state, "state")?)
        .map(|v| crate::value_to_py(py, &v))
        .transpose()
}

/// The direction tables, `(prompts, sources)`, as `{key: direction}`.
#[pyfunction]
fn hand_select_tables(py: Python<'_>) -> PyResult<(Bound<'_, PyDict>, Bound<'_, PyDict>)> {
    let table = |rows: &[(&str, Direction)]| -> PyResult<Bound<'_, PyDict>> {
        let d = PyDict::new(py);
        for (k, v) in rows {
            d.set_item(k, v.name())?;
        }
        Ok(d)
    };
    Ok((
        table(&hand_select::PROMPT_DIRECTION)?,
        table(&hand_select::SOURCE_DIRECTION)?,
    ))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(hand_select_direction, m)?)?;
    m.add_function(wrap_pyfunction!(hand_select_card_value, m)?)?;
    m.add_function(wrap_pyfunction!(hand_select_rank, m)?)?;
    m.add_function(wrap_pyfunction!(hand_select_tables, m)?)?;
    Ok(())
}
