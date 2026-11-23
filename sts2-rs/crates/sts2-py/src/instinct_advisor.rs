//! `sts2_rs.InstinctAdvisorCore`: the Instinct student and its per-run
//! state (`sts2_advisor::instinct_advisor`, GH #1426 step 4).
//! `realgame_instinct.InstinctAdvisor` keeps its Python name over it.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;
use sts2_advisor::instinct_advisor::{Context, InstinctAdvisor, InstinctError};
use sts2_advisor::search::RecommendConfig;

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

type Log = Vec<(&'static str, String)>;

/// The student, loaded once, plus its per-run state (thread-safe). Every
/// method that can log returns its lines as `(level, message)` pairs.
#[pyclass(name = "InstinctAdvisorCore", module = "sts2_rs", frozen)]
#[derive(Debug)]
pub struct PyInstinctAdvisor {
    core: InstinctAdvisor,
}

#[pymethods]
impl PyInstinctAdvisor {
    /// `weights`: the all-character net; `fool_weights`: the Fool's
    /// (v0.113.0 games), if any.
    #[new]
    #[pyo3(signature = (weights, fool_weights = None))]
    fn new(weights: std::path::PathBuf, fool_weights: Option<std::path::PathBuf>) -> Self {
        let core = InstinctAdvisor::new(weights);
        Self {
            core: match fool_weights {
                Some(f) => core.with_fool(f),
                None => core,
            },
        }
    }

    fn available(&self) -> (bool, Log) {
        let mut log = Vec::new();
        (self.core.available(&mut log), log)
    }

    fn capabilities<'py>(&self, py: Python<'py>) -> PyResult<(Bound<'py, PyAny>, Log)> {
        let mut log = Vec::new();
        let caps = self.core.capabilities(&mut log);
        Ok((crate::value_to_py(py, &caps)?, log))
    }

    /// The log line when there was per-run state to drop.
    fn reset(&self, reason: &str) -> Option<String> {
        self.core.reset(reason)
    }

    /// Follow the live fight's enemies through renamed uids
    /// (`[(uid then, uid now)]`, `CombatMirrorCore.take_enemy_remap`).
    fn remap_enemies(&self, pairs: Vec<(u32, u32)>) {
        self.core.remap_enemies(&pairs);
    }

    fn on_human_action(&self, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        self.core.on_human_action(&json(msg, "msg")?);
        Ok(())
    }

    #[staticmethod]
    fn eligibility(state: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
        Ok(InstinctAdvisor::eligibility(&json(state, "state")?))
    }

    /// `(outcome, payload, log)`: `("advice", advice)`, `("unavailable",
    /// why)` or `("failed", (python_type, message))`. `cfg` is
    /// `dataclasses.asdict(RecommendConfig)`; `session` the advisor's
    /// per-run dict, updated in place.
    #[pyo3(signature = (state, advice_for, names, cfg = None, mirror = None, session = None, sims = None))]
    #[allow(clippy::too_many_arguments)]
    fn advise<'py>(
        &self,
        py: Python<'py>,
        state: &Bound<'py, PyAny>,
        advice_for: &str,
        names: &crate::display_names::PyDisplayNames,
        cfg: Option<&Bound<'py, PyAny>>,
        mirror: Option<&Bound<'py, crate::advisor_mirror::PyAdvisorMirror>>,
        session: Option<&Bound<'py, PyDict>>,
        sims: Option<u32>,
    ) -> PyResult<(&'static str, Bound<'py, PyAny>, Log)> {
        let state = json(state, "state")?;
        let cfg = cfg
            .filter(|c| !c.is_none())
            .map(|c| json(c, "cfg").map(|v| RecommendConfig::from_value(&v)))
            .transpose()?;
        let mut session_map = session.map(crate::screen_advice::session_in).transpose()?;
        let mut held = mirror.map(Bound::borrow_mut);
        let mut log = Vec::new();
        let out = self.core.advise(
            &state,
            advice_for,
            Context {
                names: &names.names,
                cfg: cfg.as_ref(),
                mirror: held.as_deref_mut().map(|m| &mut m.core),
                session: session_map.as_mut(),
                sims,
            },
            &mut log,
        );
        drop(held);
        if let (Some(d), Some(m)) = (session, session_map) {
            crate::screen_advice::session_out(d, m)?;
        }
        outcome(py, out, log)
    }

    /// `InstinctAdvisor._dispatch` (no eligibility check, no net load), as
    /// [`Self::advise`].
    ///
    /// `(outcome, payload, log)`: `("advice", advice)`, `("unavailable",
    /// why)` or `("failed", (python_type, message))`. `cfg` is
    /// `dataclasses.asdict(RecommendConfig)`; `session` the advisor's
    /// per-run dict, updated in place.
    #[pyo3(signature = (state, advice_for, names, cfg = None, mirror = None, session = None, sims = None))]
    #[allow(clippy::too_many_arguments)]
    fn dispatch<'py>(
        &self,
        py: Python<'py>,
        state: &Bound<'py, PyAny>,
        advice_for: &str,
        names: &crate::display_names::PyDisplayNames,
        cfg: Option<&Bound<'py, PyAny>>,
        mirror: Option<&Bound<'py, crate::advisor_mirror::PyAdvisorMirror>>,
        session: Option<&Bound<'py, PyDict>>,
        sims: Option<u32>,
    ) -> PyResult<(&'static str, Bound<'py, PyAny>, Log)> {
        let state = json(state, "state")?;
        let cfg = cfg
            .filter(|c| !c.is_none())
            .map(|c| json(c, "cfg").map(|v| RecommendConfig::from_value(&v)))
            .transpose()?;
        let mut session_map = session.map(crate::screen_advice::session_in).transpose()?;
        let mut held = mirror.map(Bound::borrow_mut);
        let mut log = Vec::new();
        let out = self.core.dispatch_unloaded(
            &state,
            advice_for,
            Context {
                names: &names.names,
                cfg: cfg.as_ref(),
                mirror: held.as_deref_mut().map(|m| &mut m.core),
                session: session_map.as_mut(),
                sims,
            },
            &mut log,
        );
        drop(held);
        if let (Some(d), Some(m)) = (session, session_map) {
            crate::screen_advice::session_out(d, m)?;
        }
        outcome(py, out, log)
    }

    /// `InstinctAdvisor._advise_combat_select` (no net load), as
    /// [`Self::advise`].
    ///
    /// `(outcome, payload, log)`: `("advice", advice)`, `("unavailable",
    /// why)` or `("failed", (python_type, message))`. `cfg` is
    /// `dataclasses.asdict(RecommendConfig)`; `session` the advisor's
    /// per-run dict, updated in place.
    #[pyo3(signature = (state, names, cfg = None, mirror = None, session = None, sims = None))]
    #[allow(clippy::too_many_arguments)]
    fn combat_select<'py>(
        &self,
        py: Python<'py>,
        state: &Bound<'py, PyAny>,
        names: &crate::display_names::PyDisplayNames,
        cfg: Option<&Bound<'py, PyAny>>,
        mirror: Option<&Bound<'py, crate::advisor_mirror::PyAdvisorMirror>>,
        session: Option<&Bound<'py, PyDict>>,
        sims: Option<u32>,
    ) -> PyResult<(&'static str, Bound<'py, PyAny>, Log)> {
        let state = json(state, "state")?;
        let cfg = cfg
            .filter(|c| !c.is_none())
            .map(|c| json(c, "cfg").map(|v| RecommendConfig::from_value(&v)))
            .transpose()?;
        let mut session_map = session.map(crate::screen_advice::session_in).transpose()?;
        let mut held = mirror.map(Bound::borrow_mut);
        let mut log = Vec::new();
        let out = self.core.combat_select_unloaded(
            &state,
            Context {
                names: &names.names,
                cfg: cfg.as_ref(),
                mirror: held.as_deref_mut().map(|m| &mut m.core),
                session: session_map.as_mut(),
                sims,
            },
            &mut log,
        );
        drop(held);
        if let (Some(d), Some(m)) = (session, session_map) {
            crate::screen_advice::session_out(d, m)?;
        }
        outcome(py, out, log)
    }

    /// `InstinctAdvisor.rust_screen`: one screen handler, `(outcome,
    /// payload, log)` as [`Self::advise`].
    fn screen<'py>(
        &self,
        py: Python<'py>,
        screen: &str,
        state: &Bound<'py, PyAny>,
        names: &crate::display_names::PyDisplayNames,
    ) -> PyResult<(&'static str, Bound<'py, PyAny>, Log)> {
        let out = self
            .core
            .screen(screen, &json(state, "state")?, &names.names);
        outcome(py, out, Vec::new())
    }

    /// `InstinctAdvisor.evaluate`: peek `rm` with this run's driver and run
    /// the student. `None` and the reason when it cannot answer.
    fn evaluate<'py>(
        &self,
        py: Python<'py>,
        state: &Bound<'py, PyAny>,
        rm: &mut crate::run_manager::PyRunManager,
        expect: Vec<String>,
    ) -> PyResult<(Option<Bound<'py, PyAny>>, Option<String>)> {
        let state = json(state, "state")?;
        match self.core.evaluate(&state, &mut rm.inner, &expect) {
            Ok(ev) => {
                let out = serde_json::json!({
                    "decision": ev.pick.decision,
                    "probs": ev.pick.probs,
                    "best": ev.pick.best,
                    "ms": ev.pick.ms,
                    "estimate": ev.estimate,
                });
                Ok((Some(crate::value_to_py(py, &out)?), None))
            }
            Err(why) => Ok((None, Some(why))),
        }
    }

    /// `InstinctAdvisor.choice_picks`: the student's picks on the in-fight
    /// choice `rm` stands on (the native run's COMBAT_CHOICE decisions).
    /// `(picks, None)`, or `(None, why)` when it cannot answer.
    fn choice_picks(
        &self,
        state: &Bound<'_, PyAny>,
        rm: &mut crate::run_manager::PyRunManager,
        max_select: i64,
    ) -> PyResult<(Option<Vec<i64>>, Option<String>)> {
        let state = json(state, "state")?;
        Ok(
            match self.core.choice_picks(&state, &mut rm.inner, max_select) {
                Ok(picks) => (Some(picks), None),
                Err(why) => (None, Some(why)),
            },
        )
    }

    #[getter]
    fn last_room(&self) -> Option<String> {
        self.core.last_room()
    }

    #[setter]
    fn set_last_room(&self, v: Option<String>) {
        self.core.set_last_room(v);
    }

    #[getter]
    fn last_screen(&self) -> Option<String> {
        self.core.last_screen()
    }

    #[setter]
    fn set_last_screen(&self, v: Option<String>) {
        self.core.set_last_screen(v);
    }

    #[getter]
    fn shop_removal(&self) -> Option<(String, bool, String)> {
        self.core.shop_removal()
    }

    #[setter]
    fn set_shop_removal(&self, v: Option<(String, bool, String)>) {
        self.core.set_shop_removal(v);
    }

    #[getter]
    fn event_track<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.core
            .event_track()
            .map(|t| crate::value_to_py(py, &t))
            .transpose()
    }

    #[setter]
    fn set_event_track(&self, v: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let v = v
            .filter(|t| !t.is_none())
            .map(|t| json(t, "event_track"))
            .transpose()?;
        self.core.set_event_track(v);
        Ok(())
    }

    /// The last position a screen handler asked the student about:
    /// `{decision, probs, best, ms}` or `None`.
    #[getter]
    fn last_pick<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.core
            .last_pick()
            .map(|p| {
                let v = serde_json::json!({
                    "decision": p.decision, "probs": p.probs, "best": p.best, "ms": p.ms,
                });
                crate::value_to_py(py, &v)
            })
            .transpose()
    }
}

/// An answer as `(outcome, payload, log)` for the Python wrapper.
fn outcome(
    py: Python<'_>,
    out: Result<Value, InstinctError>,
    log: Log,
) -> PyResult<(&'static str, Bound<'_, PyAny>, Log)> {
    let (kind, payload) = match out {
        Ok(advice) => ("advice", advice),
        Err(InstinctError::Unavailable(why)) => ("unavailable", Value::String(why)),
        Err(InstinctError::Failed(ty, msg)) => ("failed", serde_json::json!([ty, msg])),
    };
    Ok((kind, crate::value_to_py(py, &payload)?, log))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyInstinctAdvisor>()?;
    m.add(
        "INSTINCT_CHARACTERS",
        sts2_advisor::instinct_advisor::INSTINCT_CHARACTERS.to_vec(),
    )?;
    m.add(
        "INSTINCT_FOOL_GAME_VERSION",
        sts2_advisor::instinct_advisor::FOOL_GAME_VERSION,
    )?;
    m.add(
        "INSTINCT_GAME_VERSIONS",
        sts2_advisor::instinct_advisor::INSTINCT_GAME_VERSIONS.to_vec(),
    )?;
    Ok(())
}
