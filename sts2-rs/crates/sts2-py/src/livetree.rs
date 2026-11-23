//! `sts2_rs.LiveRunTreeCore`: the live game's branch tree
//! (`sts2_advisor::livetree`, GH #1426 step 4). `realgame_livetree.LiveRunTree`
//! keeps its Python name over it.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::livetree::LiveRunTree;

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

/// The branch tree of one live run (thread-safe).
#[pyclass(name = "LiveRunTreeCore", module = "sts2_rs", frozen)]
#[derive(Debug)]
pub struct PyLiveRunTree {
    tree: LiveRunTree,
}

#[pymethods]
impl PyLiveRunTree {
    #[new]
    fn new() -> Self {
        Self {
            tree: LiveRunTree::new(),
        }
    }

    #[pyo3(signature = (msg, advice_for, log_idx = None))]
    fn on_decision(
        &self,
        msg: &Bound<'_, PyAny>,
        advice_for: &str,
        log_idx: Option<i64>,
    ) -> PyResult<()> {
        self.tree
            .on_decision(&json(msg, "msg")?, advice_for, log_idx);
        Ok(())
    }

    fn on_human_action(&self, msg: &Bound<'_, PyAny>) -> PyResult<Option<usize>> {
        Ok(self.tree.on_human_action(&json(msg, "msg")?))
    }

    fn on_snapshot_saved(&self, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        self.tree.on_snapshot_saved(&json(msg, "msg")?);
        Ok(())
    }

    fn on_snapshot_restored(&self, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        self.tree.on_snapshot_restored(&json(msg, "msg")?);
        Ok(())
    }

    fn on_combat_undone(&self, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        self.tree.on_combat_undone(&json(msg, "msg")?);
        Ok(())
    }

    fn on_run_end(&self, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        self.tree.on_run_end(&json(msg, "msg")?);
        Ok(())
    }

    fn snapshot_id_of<'py>(
        &self,
        py: Python<'py>,
        node_id: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        match index(node_id) {
            Some(id) => crate::value_to_py(py, &self.tree.snapshot_id_of(id)),
            None => Ok(py.None().into_bound(py)),
        }
    }

    fn seek_plan<'py>(
        &self,
        py: Python<'py>,
        node_id: &Bound<'py, PyAny>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        index(node_id)
            .and_then(|id| self.tree.seek_plan(id))
            .map(|p| crate::value_to_py(py, &p))
            .transpose()
    }

    fn log_idx<'py>(
        &self,
        py: Python<'py>,
        node_id: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let id = json(node_id, "node_id").unwrap_or(Value::Null);
        crate::value_to_py(py, &self.tree.log_idx(&id))
    }

    fn active_path(&self) -> Vec<usize> {
        self.tree.active_path()
    }

    fn timeline<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::value_to_py(py, &self.tree.timeline())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyLiveRunTree>()?;
    Ok(())
}

/// The node a Python id names (`None` for a negative or non-integer one,
/// which the Python dict lookup also missed).
fn index(node_id: &Bound<'_, PyAny>) -> Option<usize> {
    json(node_id, "node_id")
        .ok()
        .and_then(|v| sts2_advisor::livetree::node_index(&v))
}
