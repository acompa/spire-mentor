//! `sts2_rs.graft_run`: the live run behind a mod message rebuilt as a
//! `RunManager` (`sts2_advisor::graft`, GH #1426). `realgame_instinct.graft_run`
//! delegates here and re-raises [`GraftUnavailableError`] as
//! `InstinctUnavailable`.

use pyo3::create_exception;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use sts2_advisor::graft::{self, GraftError};

use crate::run_manager::PyRunManager;

create_exception!(
    sts2_rs,
    GraftUnavailableError,
    PyRuntimeError,
    "The run behind this message cannot be rebuilt; the message is player-facing."
);

/// `(rm, notes)`: the run behind `state`, built on game build `version`,
/// parked in `map_choice` at the player's node, and what could not be
/// restored.
#[pyfunction]
fn graft_run(state: &Bound<'_, PyAny>, version: &str) -> PyResult<(PyRunManager, Vec<String>)> {
    let state: serde_json::Value = pythonize::depythonize(state)
        .map_err(|e| PyValueError::new_err(format!("state is not JSON-shaped: {e}")))?;
    match graft::graft_run(&state, version) {
        Ok((rm, notes)) => Ok((PyRunManager::from_inner(rm), notes)),
        Err(GraftError::Unavailable(m)) => Err(GraftUnavailableError::new_err(m)),
        Err(GraftError::Value(m)) => Err(PyValueError::new_err(m)),
    }
}

/// `realgame_instinct_screens.card_spec`.
#[pyfunction]
fn graft_card_spec<'py>(py: Python<'py>, card: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let card: serde_json::Value = pythonize::depythonize(card)
        .map_err(|e| PyValueError::new_err(format!("card is not JSON-shaped: {e}")))?;
    crate::value_to_py(py, &graft::card_spec(&card))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(graft_run, m)?)?;
    m.add_function(wrap_pyfunction!(graft_card_spec, m)?)?;
    m.add(
        "GraftUnavailableError",
        m.py().get_type::<GraftUnavailableError>(),
    )?;
    m.add("GRAFT_COOP_MAP_APPROX", graft::COOP_MAP_APPROX)?;
    Ok(())
}
