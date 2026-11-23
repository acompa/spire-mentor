//! `sts2_rs.TurnPlanWalk` and `sts2_rs.turn_*`: the advisor's turn plan
//! (`sts2_advisor::turn_plan`, GH #1426 step 3). `realgame_turn_plan`
//! re-exports them under their Python names.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::turn_plan::{self, PlanWalk};

use crate::PyCombatState;

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

fn opt_json(arg: Option<&Bound<'_, PyAny>>, name: &str) -> PyResult<Value> {
    arg.map_or(Ok(Value::Null), |a| json(a, name))
}

/// `realgame_turn_plan.PlanWalk`: maps one line of engine actions onto the
/// request's hand, step by step.
#[pyclass(name = "TurnPlanWalk", module = "sts2_rs")]
#[derive(Debug)]
pub struct PyPlanWalk {
    walk: PlanWalk,
}

#[pymethods]
impl PyPlanWalk {
    #[new]
    fn new(wire: &Bound<'_, PyAny>, state: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            walk: PlanWalk::new(&json(wire, "wire")?, &json(state, "state")?),
        })
    }

    #[getter]
    const fn done(&self) -> bool {
        self.walk.done
    }

    /// The steps so far (a copy).
    #[getter]
    fn steps<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        self.walk
            .steps
            .iter()
            .map(|s| crate::value_to_py(py, s))
            .collect()
    }

    /// Drop the last step (the line could not play it after all).
    fn pop_step(&mut self) {
        self.walk.steps.pop();
    }

    fn to_step<'py>(
        &self,
        py: Python<'py>,
        action: &Bound<'py, PyAny>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.walk
            .to_step(&json(action, "action")?)
            .map(|s| crate::value_to_py(py, &s))
            .transpose()
    }

    fn push(&mut self, step: &Bound<'_, PyAny>) -> PyResult<()> {
        self.walk.push(json(step, "step")?);
        Ok(())
    }

    #[pyo3(signature = (action, before, after, result = None))]
    fn advance(
        &mut self,
        action: &Bound<'_, PyAny>,
        before: &Bound<'_, PyAny>,
        after: &Bound<'_, PyAny>,
        result: Option<&str>,
    ) -> PyResult<()> {
        self.walk.advance(
            &json(action, "action")?,
            &json(before, "before")?,
            &json(after, "after")?,
            result,
        );
        Ok(())
    }
}

/// `realgame_turn_plan.step_label`.
#[pyfunction]
fn turn_step_label(state: &Bound<'_, PyAny>, step: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(turn_plan::step_label(
        &json(state, "state")?,
        &json(step, "step")?,
    ))
}

/// `realgame_turn_plan.same_move`.
#[pyfunction]
fn turn_same_move(step: &Bound<'_, PyAny>, best: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(turn_plan::same_move(
        &json(step, "step")?,
        &json(best, "best")?,
    ))
}

/// `realgame_turn_plan.first_step`.
#[pyfunction]
fn turn_first_step<'py>(
    py: Python<'py>,
    best: Option<&Bound<'py, PyAny>>,
    state: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    turn_plan::first_step(&opt_json(best, "best")?, &json(state, "state")?)
        .map(|s| crate::value_to_py(py, &s))
        .transpose()
}

/// `realgame_turn_plan.search_plan`. `research(fight)` gets each re-search
/// position as a `CombatState` carrying `cs`'s seed and metadata; an
/// exception it raises ends the plan where it got to (logged at debug by the
/// Python wrapper, which wraps `research`).
#[pyfunction]
#[pyo3(signature = (cs, pv, wire, state, best, research = None, budget_s = turn_plan::PLAN_BUDGET_S))]
#[allow(clippy::too_many_arguments)]
fn turn_search_plan<'py>(
    py: Python<'py>,
    cs: PyRef<'py, PyCombatState>,
    pv: &Bound<'py, PyAny>,
    wire: &Bound<'py, PyAny>,
    state: &Bound<'py, PyAny>,
    best: Option<&Bound<'py, PyAny>>,
    research: Option<Bound<'py, PyAny>>,
    budget_s: f64,
) -> PyResult<Vec<Bound<'py, PyAny>>> {
    let Value::Array(pv) = json(pv, "pv")? else {
        return Err(PyValueError::new_err("pv must be a list"));
    };
    let (wire, state, best) = (
        json(wire, "wire")?,
        json(state, "state")?,
        opt_json(best, "best")?,
    );
    let mut call = |fight: &sts2_core::CombatState| -> Option<Vec<Value>> {
        let research = research.as_ref()?;
        let obj = Py::new(py, cs.with_state(fight.clone())).ok()?;
        let line = research.call1((obj,)).ok()?;
        match pythonize::depythonize::<Value>(&line).ok()? {
            Value::Array(a) => Some(a),
            _ => None,
        }
    };
    let research: Option<&mut dyn FnMut(&sts2_core::CombatState) -> Option<Vec<Value>>> =
        if research_given(research.as_ref()) {
            Some(&mut call)
        } else {
            None
        };
    turn_plan::search_plan(&cs.inner, &pv, &wire, &state, &best, research, budget_s)
        .iter()
        .map(|s| crate::value_to_py(py, s))
        .collect()
}

fn research_given(r: Option<&Bound<'_, PyAny>>) -> bool {
    r.is_some_and(|r| !r.is_none())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyPlanWalk>()?;
    m.add_function(wrap_pyfunction!(turn_step_label, m)?)?;
    m.add_function(wrap_pyfunction!(turn_same_move, m)?)?;
    m.add_function(wrap_pyfunction!(turn_first_step, m)?)?;
    m.add_function(wrap_pyfunction!(turn_search_plan, m)?)?;
    m.add("TURN_PLAN_MAX_STEPS", turn_plan::MAX_PLAN_STEPS)?;
    m.add("TURN_PLAN_BUDGET_S", turn_plan::PLAN_BUDGET_S)?;
    Ok(())
}
