//! `sts2_rs.DisplayNames`: player-facing names for game ids
//! (`sts2_advisor::display_names`, GH #1426 step 3).
//!
//! `display_names.py` reads the localization tables and builds one of these;
//! its functions call it.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;
use sts2_advisor::display_names::Names;

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

/// The display-name tables.
#[pyclass(name = "DisplayNames", module = "sts2_rs", frozen)]
#[derive(Debug)]
pub struct PyDisplayNames {
    pub(crate) names: Names,
}

#[pymethods]
impl PyDisplayNames {
    /// `tables`: `(table, [(key, value), ...])` in lookup order, every
    /// localization directory's tables, newest build first.
    #[new]
    fn new(tables: Vec<(String, Vec<(String, String)>)>) -> Self {
        Self {
            names: Names::new(tables.iter().map(|(t, e)| (t.as_str(), e.as_slice()))),
        }
    }

    fn display_name(&self, raw: &Bound<'_, PyAny>) -> PyResult<String> {
        Ok(self.names.display_name(&json(raw, "raw")?))
    }

    fn humanize(&self, text: &str) -> String {
        self.names.humanize(text)
    }

    fn card_name(&self, card: &Bound<'_, PyAny>) -> PyResult<String> {
        Ok(self.names.card_name(&json(card, "card")?))
    }

    fn shop_option_label(&self, option: &Bound<'_, PyAny>) -> PyResult<String> {
        Ok(self.names.shop_option_label(&json(option, "option")?))
    }

    #[pyo3(signature = (option, belt=None))]
    fn potion_swap_label(
        &self,
        option: &Bound<'_, PyAny>,
        belt: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<String> {
        let belt = match belt {
            Some(b) if !b.is_none() => match json(b, "belt")? {
                Value::Array(items) => items,
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        Ok(self
            .names
            .potion_swap_label(&json(option, "option")?, &belt))
    }

    /// The sanitized copy of `advice` (the native advisor's guard; the
    /// Python one rewrites its dict in place).
    #[pyo3(signature = (advice, advice_for=None, state=None))]
    fn sanitize_advice<'py>(
        &self,
        py: Python<'py>,
        advice: &Bound<'py, PyAny>,
        advice_for: Option<&str>,
        state: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut advice = json(advice, "advice")?;
        let state = state.map_or(Ok(Value::Null), |s| json(s, "state"))?;
        self.names.sanitize_advice(&mut advice, advice_for, &state);
        crate::value_to_py(py, &advice)
    }

    /// The sanitized copy of a `coach` record.
    fn sanitize_coach<'py>(
        &self,
        py: Python<'py>,
        record: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut record = json(record, "record")?;
        self.names.sanitize_coach(&mut record);
        crate::value_to_py(py, &record)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDisplayNames>()
}
