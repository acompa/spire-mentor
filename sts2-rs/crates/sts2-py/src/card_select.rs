//! `sts2_rs.card_select_*`: an in-combat `card_select` off the fight's
//! mirror (`sts2_advisor::card_select`, GH #1426 step 4). `combat_mirror` /
//! `realgame_mcts` keep their Python names.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::card_select;

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

/// `combat_mirror.skip_arity_from_world`: `(can_skip, min_select)`.
#[pyfunction]
fn card_select_skip_arity(
    cancelable: &Bound<'_, PyAny>,
    source: &Bound<'_, PyAny>,
    min_select: i64,
    max_select: i64,
) -> PyResult<(bool, i64)> {
    Ok(card_select::skip_arity_from_world(
        &json(cancelable, "cancelable")?,
        &json(source, "source")?,
        min_select,
        max_select,
    ))
}

/// `combat_mirror.recognize_card_select`: `(kind, block, options)`, `block`
/// a `(source, id)` pair or `None`, `options` with the hand locators filled
/// in where the screen was recognized as a hand screen.
#[pyfunction]
#[pyo3(signature = (parked, options, board, wire, min_select, source_model, prompt_key, source=None))]
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn card_select_recognize<'py>(
    py: Python<'py>,
    parked: Option<&Bound<'py, PyAny>>,
    options: &Bound<'py, PyAny>,
    board: &Bound<'py, PyAny>,
    wire: &Bound<'py, PyAny>,
    min_select: i64,
    source_model: &Bound<'py, PyAny>,
    prompt_key: &Bound<'py, PyAny>,
    source: Option<&Bound<'py, PyAny>>,
) -> PyResult<(String, Option<(&'static str, String)>, Bound<'py, PyAny>)> {
    let source = source
        .map(|s| json(s, "source"))
        .transpose()?
        .unwrap_or(Value::Null);
    let parked = parked.map(|p| json(p, "parked")).transpose()?;
    let Value::Array(mut options) = json(options, "options")? else {
        return Err(PyValueError::new_err("options must be a list"));
    };
    let (kind, block) = card_select::recognize_card_select(
        parked.as_ref(),
        &mut options,
        &json(board, "board")?,
        &json(wire, "wire")?,
        min_select,
        &json(source_model, "source_model")?,
        &json(prompt_key, "prompt_key")?,
        &source,
    );
    Ok((
        kind,
        block.map(|b| (b.source, b.id)),
        crate::value_to_py(py, &Value::Array(options))?,
    ))
}

/// `realgame_mcts.pick_card_select_mirrored`'s answer: `(bridge action,
/// picks)` for the picks (`None`: the mirror dropped on the way to the
/// choice, so the minimum legal selection and no picks).
#[pyfunction]
fn card_select_answer<'py>(
    py: Python<'py>,
    cards: &Bound<'py, PyAny>,
    min_select: i64,
    picked: Option<Vec<i64>>,
) -> PyResult<(Bound<'py, PyAny>, Option<Vec<i64>>)> {
    let cards = json(cards, "cards")?;
    let (action, picks) = card_select::card_select_answer(
        cards.as_array().map_or(&[], Vec::as_slice),
        min_select,
        picked,
    );
    Ok((crate::value_to_py(py, &action)?, picks))
}

/// `_GENERATIVE_SOURCES`: `source_model` → the generative kind.
#[pyfunction]
fn card_select_generative_sources() -> Vec<(&'static str, &'static str)> {
    card_select::GENERATIVE_SOURCES.to_vec()
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(card_select_skip_arity, m)?)?;
    m.add_function(wrap_pyfunction!(card_select_recognize, m)?)?;
    m.add_function(wrap_pyfunction!(card_select_answer, m)?)?;
    m.add_function(wrap_pyfunction!(card_select_generative_sources, m)?)?;
    Ok(())
}
