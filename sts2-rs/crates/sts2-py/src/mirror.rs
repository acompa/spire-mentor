//! `sts2_rs.mirror_*`: the combat mirror's comparison of its prediction
//! against reality (`sts2_advisor::mirror`, GH #1426 step 1).
//!
//! `combat_mirror.py` keeps its functions and calls these; this is only the
//! Python door. A desync crosses as a `(path, mirror, world)` tuple.

use std::collections::BTreeSet;

use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::mirror::{self, Desync};

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("{name} is not JSON-shaped: {e}"))
    })
}

/// A desync as Python sees it: `(path, mirror, world)`.
type PyDesync<'py> = (String, Bound<'py, PyAny>, Bound<'py, PyAny>);

fn desyncs_to_py(py: Python<'_>, desyncs: Vec<Desync>) -> PyResult<Vec<PyDesync<'_>>> {
    desyncs
        .into_iter()
        .map(|d| {
            Ok((
                d.path,
                crate::value_to_py(py, &d.mirror)?,
                crate::value_to_py(py, &d.world)?,
            ))
        })
        .collect()
}

/// `combat_mirror.observables`: the deterministic half of a wire dict.
#[pyfunction]
pub fn mirror_observables<'py>(
    py: Python<'py>,
    wire: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let obs = mirror::observables(&json(wire, "wire")?);
    crate::value_to_py(py, &Value::Object(obs.into_iter().collect()))
}

/// `combat_mirror.diff_observables`, as `(path, mirror, world)` tuples.
#[pyfunction]
#[pyo3(signature = (mirror_wire, world_wire, skip_prefix=None, skip_paths=Vec::new()))]
pub fn mirror_diff_observables<'py>(
    py: Python<'py>,
    mirror_wire: &Bound<'py, PyAny>,
    world_wire: &Bound<'py, PyAny>,
    skip_prefix: Option<&str>,
    skip_paths: Vec<String>,
) -> PyResult<Vec<PyDesync<'py>>> {
    let skip: BTreeSet<String> = skip_paths.into_iter().collect();
    let diff = mirror::diff_observables(
        &json(mirror_wire, "mirror_wire")?,
        &json(world_wire, "world_wire")?,
        skip_prefix,
        &skip,
    );
    desyncs_to_py(py, diff)
}

/// `combat_mirror._cursor_drift`, as `(path, mirror, world)` tuples.
#[pyfunction]
pub fn mirror_cursor_drift<'py>(
    py: Python<'py>,
    mirror_wire: &Bound<'py, PyAny>,
    world_wire: &Bound<'py, PyAny>,
) -> PyResult<Vec<PyDesync<'py>>> {
    let drift = mirror::cursor_drift(
        &json(mirror_wire, "mirror_wire")?,
        &json(world_wire, "world_wire")?,
    );
    desyncs_to_py(py, drift)
}

/// `combat_mirror.unreported_paths`.
#[pyfunction]
pub fn mirror_unreported_paths(raw_world_wire: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    Ok(
        mirror::unreported_paths(&json(raw_world_wire, "raw_world_wire")?)
            .into_iter()
            .collect(),
    )
}

/// `combat_mirror.enemy_slot_key`.
#[pyfunction]
pub fn mirror_enemy_slot_key(
    enemy: &Bound<'_, PyAny>,
    index: usize,
    encounter: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let encounter = json(encounter, "encounter")?;
    Ok(mirror::enemy_slot_key(
        &json(enemy, "enemy")?,
        index,
        encounter.as_array().map_or(&[], Vec::as_slice),
    ))
}

/// `combat_mirror.wire_enemy_index`.
#[pyfunction]
pub fn mirror_wire_enemy_index(
    wire: &Bound<'_, PyAny>,
) -> PyResult<std::collections::BTreeMap<String, usize>> {
    Ok(mirror::wire_enemy_index(&json(wire, "wire")?))
}

/// `combat_mirror._unalignable_kinds`.
#[pyfunction]
pub fn mirror_unalignable_kinds(
    world_wire: &Bound<'_, PyAny>,
    mirror_wire: &Bound<'_, PyAny>,
) -> PyResult<Vec<String>> {
    Ok(mirror::unalignable_kinds(
        &json(world_wire, "world_wire")?,
        &json(mirror_wire, "mirror_wire")?,
    )
    .into_iter()
    .collect())
}

/// `combat_mirror._enemy_alignment`.
#[pyfunction]
pub fn mirror_enemy_alignment(
    world_wire: &Bound<'_, PyAny>,
    mirror_wire: &Bound<'_, PyAny>,
) -> PyResult<Vec<Option<usize>>> {
    Ok(mirror::enemy_alignment(
        &json(world_wire, "world_wire")?,
        &json(mirror_wire, "mirror_wire")?,
    ))
}

/// `combat_mirror._undisclosed_relic_counters`.
#[pyfunction]
pub fn mirror_undisclosed_relic_counters(wire: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    Ok(mirror::undisclosed_relic_counters(&json(wire, "wire")?))
}

/// `combat_mirror._undisclosed_power_counters`.
#[pyfunction]
pub fn mirror_undisclosed_power_counters(wire: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    Ok(mirror::undisclosed_power_counters(&json(wire, "wire")?))
}

/// `combat_mirror._relocate_parked`; `None` when no option survives.
#[pyfunction]
pub fn mirror_relocate_parked<'py>(
    py: Python<'py>,
    parked: &Bound<'py, PyAny>,
    seeded: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    mirror::relocate_parked(&json(parked, "parked")?, &json(seeded, "seeded")?)
        .map(|v| crate::value_to_py(py, &v))
        .transpose()
}

/// `combat_mirror._choice_effect_in_flight`, the desyncs as
/// `(path, mirror, world)` tuples.
#[pyfunction]
pub fn mirror_choice_effect_in_flight(
    desyncs: Vec<PyDesync<'_>>,
    picked: &Bound<'_, PyAny>,
) -> PyResult<bool> {
    let desyncs = desyncs
        .into_iter()
        .map(|(path, m, w)| {
            Ok(Desync {
                path,
                mirror: json(&m, "mirror")?,
                world: json(&w, "world")?,
            })
        })
        .collect::<PyResult<Vec<_>>>()?;
    let picked = json(picked, "picked")?;
    Ok(mirror::choice_effect_in_flight(
        &desyncs,
        picked.as_array().map_or(&[], Vec::as_slice),
    ))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(mirror_observables, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_diff_observables, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_cursor_drift, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_unreported_paths, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_enemy_slot_key, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_wire_enemy_index, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_unalignable_kinds, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_enemy_alignment, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_undisclosed_relic_counters, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_undisclosed_power_counters, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_relocate_parked, m)?)?;
    m.add_function(wrap_pyfunction!(mirror_choice_effect_in_flight, m)?)?;
    Ok(())
}
