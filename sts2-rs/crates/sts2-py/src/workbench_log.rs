//! `sts2_rs.WorkbenchRunLoggerCore`: the live run's workbench-schema JSONL
//! (`sts2_advisor::workbench_log`, GH #1426 step 4).
//! `realgame_workbench_log.WorkbenchRunLogger` keeps its Python name over it.
//!
//! Every method releases the GIL before it takes the logger's lock: the
//! host's clock and log lines are Python callables, so a thread holding the
//! lock must be able to take the GIL while another thread waits for the lock.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::workbench_log::{self, Hooks, WorkbenchRunLogger};

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

/// A Python `f(str)` as a log sink; a raise inside it is dropped (the
/// logger never fails into the advisor loop).
fn sink(f: Py<PyAny>) -> Arc<dyn Fn(&str) + Send + Sync> {
    Arc::new(move |m: &str| {
        Python::with_gil(|py| {
            let _ = f.call1(py, (m,));
        });
    })
}

/// One workbench-schema JSONL per live run (thread-safe).
#[pyclass(name = "WorkbenchRunLoggerCore", module = "sts2_rs", frozen)]
#[derive(Debug)]
pub struct PyWorkbenchRunLogger {
    log: WorkbenchRunLogger,
}

#[pymethods]
impl PyWorkbenchRunLogger {
    /// `clock()` returns local ISO-8601 time; `info(msg)` / `warn(msg)` are
    /// the host's log lines.
    #[new]
    fn new(log_dir: Option<&str>, clock: Py<PyAny>, info: Py<PyAny>, warn: Py<PyAny>) -> Self {
        let clock = Arc::new(move || {
            Python::with_gil(|py| {
                clock
                    .call0(py)
                    .and_then(|v| v.extract::<String>(py))
                    .unwrap_or_default()
            })
        });
        Self {
            log: WorkbenchRunLogger::new(
                log_dir,
                Hooks {
                    clock,
                    info: sink(info),
                    warn: sink(warn),
                },
            ),
        }
    }

    #[getter]
    fn path(&self, py: Python<'_>) -> Option<String> {
        py.allow_threads(|| self.log.path())
    }

    #[getter]
    fn log_name(&self, py: Python<'_>) -> Option<String> {
        py.allow_threads(|| self.log.log_name())
    }

    fn close(&self, py: Python<'_>) {
        py.allow_threads(|| self.log.close());
    }

    fn note_reconnect(&self, py: Python<'_>) {
        py.allow_threads(|| self.log.note_reconnect());
    }

    fn log_decision(
        &self,
        py: Python<'_>,
        msg: &Bound<'_, PyAny>,
        advice_for: Option<&str>,
    ) -> PyResult<Option<usize>> {
        let msg = json(msg, "msg")?;
        // `None` names no surface: its rows wait for the next state, as the
        // Python builder table's miss did.
        Ok(py.allow_threads(|| self.log.log_decision(&msg, advice_for.unwrap_or(""))))
    }

    #[pyo3(signature = (msg, advice, elapsed_s, tree = None))]
    fn log_advice(
        &self,
        py: Python<'_>,
        msg: &Bound<'_, PyAny>,
        advice: &Bound<'_, PyAny>,
        elapsed_s: f64,
        tree: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let msg = json(msg, "msg")?;
        let advice = json(advice, "advice")?;
        let tree = tree
            .map(|t| json(t, "tree"))
            .transpose()?
            .unwrap_or(Value::Null);
        py.allow_threads(|| self.log.log_advice(&msg, &advice, elapsed_s, &tree));
        Ok(())
    }

    fn log_human_action(&self, py: Python<'_>, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        let msg = json(msg, "msg")?;
        py.allow_threads(|| self.log.log_human_action(&msg));
        Ok(())
    }

    fn log_coach(&self, py: Python<'_>, record: &Bound<'_, PyAny>) -> PyResult<()> {
        let record = json(record, "record")?;
        py.allow_threads(|| self.log.log_coach(&record));
        Ok(())
    }

    fn log_run_end(&self, py: Python<'_>, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        let msg = json(msg, "msg")?;
        py.allow_threads(|| self.log.log_run_end(&msg));
        Ok(())
    }
}

/// `realgame_workbench_log.run_log_name`.
#[pyfunction]
#[pyo3(signature = (path))]
fn workbench_run_log_name(path: Option<&str>) -> Option<String> {
    workbench_log::run_log_name(path)
}

/// `realgame_workbench_log.human_action_args`.
#[pyfunction]
#[pyo3(signature = (msg, state = None))]
fn workbench_human_action_args<'py>(
    py: Python<'py>,
    msg: &Bound<'py, PyAny>,
    state: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let msg = json(msg, "msg")?;
    let state = state
        .map(|s| json(s, "state"))
        .transpose()?
        .unwrap_or(Value::Null);
    crate::value_to_py(py, &workbench_log::human_action_args(&msg, &state))
}

/// `realgame_workbench_log._root_outcomes`.
#[pyfunction]
fn workbench_root_outcomes<'py>(
    py: Python<'py>,
    advice: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let advice = json(advice, "advice")?;
    crate::value_to_py(py, &Value::Array(workbench_log::root_outcomes(&advice)))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyWorkbenchRunLogger>()?;
    m.add_function(wrap_pyfunction!(workbench_run_log_name, m)?)?;
    m.add_function(wrap_pyfunction!(workbench_human_action_args, m)?)?;
    m.add_function(wrap_pyfunction!(workbench_root_outcomes, m)?)?;
    Ok(())
}
