//! `sts2_rs.CoachGraderCore` / `CoachAdviceCache` / `CoachEntry` /
//! `CoachGraderThread`: the live coach's grader
//! (`sts2_advisor::coach_grader`, GH #1426 step 4). `realgame_coach` keeps
//! its Python names over these.
//!
//! The positions and configs stay Python objects (a `CombatState` and a
//! `RecommendConfig` in the advisor, stand-ins in tests), and the searches
//! are the Python engine's (`realgame_coach.CoachEngine` or a stub). Every
//! call into the grader releases the GIL: its callbacks take it, from the
//! grader thread too.

use std::sync::{Arc, Mutex};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyCFunction, PyDict, PyTuple};
use serde_json::Value;
use sts2_advisor::coach_grader::{
    self as cg, AdviceCache, CoachGrader, Engine, Entry, EntryKind, Executor, GraderConfig,
    GraderThread, InlineExecutor, Label, SharedCache,
};

/// A Python position / config, shared by the grader's turns and jobs.
type PyObj = Arc<Py<PyAny>>;

fn json(arg: &Bound<'_, PyAny>, name: &str) -> PyResult<Value> {
    pythonize::depythonize(arg)
        .map_err(|e| PyValueError::new_err(format!("{name} is not JSON-shaped: {e}")))
}

fn opt_obj(arg: Option<&Bound<'_, PyAny>>) -> Option<PyObj> {
    arg.filter(|a| !a.is_none())
        .map(|a| Arc::new(a.clone().unbind()))
}

fn warn(py: Python<'_>, message: &str) {
    let _ = py
        .import("logging")
        .and_then(|l| l.call_method1("getLogger", ("realgame_coach",)))
        .and_then(|l| l.call_method1("warning", ("%s", message)));
}

fn exc_text(py: Python<'_>, e: &PyErr) -> String {
    e.value(py).to_string()
}

/// The Python engine (`CoachEngine` or a test stub), called with the GIL.
struct PyEngine {
    engine: Py<PyAny>,
}

fn none_or(py: Python<'_>, o: Option<&PyObj>) -> Py<PyAny> {
    o.map_or_else(|| py.None(), |o| o.clone_ref(py))
}

impl Engine<PyObj, PyObj> for PyEngine {
    fn position_info(&self, cs: Option<&PyObj>, msg: &Value) -> Value {
        Python::with_gil(|py| {
            let run = || -> PyResult<Value> {
                let msg = crate::value_to_py(py, msg)?;
                json(
                    &self
                        .engine
                        .bind(py)
                        .call_method1("position_info", (none_or(py, cs), msg))?,
                    "info",
                )
            };
            run().unwrap_or_else(|e| {
                warn(
                    py,
                    &format!("coach: position_info failed: {}", exc_text(py, &e)),
                );
                serde_json::json!({})
            })
        })
    }

    fn top_up(
        &self,
        cs: &PyObj,
        native: &Value,
        cfg: Option<&PyObj>,
        sims: u32,
    ) -> Result<Option<Value>, String> {
        Python::with_gil(|py| {
            let run = || -> PyResult<Option<Value>> {
                let native = crate::value_to_py(py, native)?;
                let out = self
                    .engine
                    .bind(py)
                    .call_method1("top_up", (cs.clone_ref(py), native, none_or(py, cfg), sims))?;
                if out.is_none() {
                    return Ok(None);
                }
                Ok(Some(json(&out, "top_up")?))
            };
            run().map_err(|e| exc_text(py, &e))
        })
    }

    fn catch_up(&self, cs: &PyObj, cfg: Option<&PyObj>, sims: u32) -> Result<Vec<Value>, String> {
        Python::with_gil(|py| {
            let run = || -> PyResult<Vec<Value>> {
                let out = self
                    .engine
                    .bind(py)
                    .call_method1("catch_up", (cs.clone_ref(py), none_or(py, cfg), sims))?;
                match json(&out, "catch_up")? {
                    Value::Array(a) => Ok(a),
                    _ => Ok(Vec::new()),
                }
            };
            run().map_err(|e| exc_text(py, &e))
        })
    }

    fn board(&self, cs: Option<&PyObj>) -> Option<Value> {
        Python::with_gil(|py| {
            let out = self
                .engine
                .bind(py)
                .call_method1("board", (none_or(py, cs),))
                .ok()?;
            if out.is_none() {
                return None;
            }
            json(&out, "board").ok()
        })
    }

    fn ai_turn(
        &self,
        cs: &PyObj,
        first: Option<&Value>,
        cfg: Option<&PyObj>,
        label: &Label,
    ) -> Result<(Vec<String>, Option<Value>), String> {
        Python::with_gil(|py| {
            let run = || -> PyResult<(Vec<String>, Option<Value>)> {
                let label = label.clone();
                let pylabel = PyCFunction::new_closure(
                    py,
                    None,
                    None,
                    move |args: &Bound<'_, PyTuple>, _kw: Option<&Bound<'_, PyDict>>| {
                        let state = json(&args.get_item(0)?, "state")?;
                        let action = json(&args.get_item(1)?, "action")?;
                        PyResult::Ok(label(&state, &action))
                    },
                )?;
                let first = first.map(|f| crate::value_to_py(py, f)).transpose()?;
                let out = self.engine.bind(py).call_method1(
                    "ai_turn",
                    (cs.clone_ref(py), first, none_or(py, cfg), pylabel),
                )?;
                let line: Vec<String> = out.get_item(0)?.extract()?;
                let end = out.get_item(1)?;
                let end = if end.is_none() {
                    None
                } else {
                    Some(json(&end, "end")?)
                };
                Ok((line, end))
            };
            run().map_err(|e| exc_text(py, &e))
        })
    }
}

/// One cached advice request (`realgame_coach.Entry`).
#[pyclass(name = "CoachEntry", module = "sts2_rs", frozen)]
#[derive(Debug)]
pub struct PyCoachEntry {
    entry: Arc<Entry<PyObj, PyObj>>,
}

#[pymethods]
impl PyCoachEntry {
    #[getter]
    fn request_id<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::value_to_py(py, &self.entry.request_id)
    }

    #[getter]
    fn state_id<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::value_to_py(py, &self.entry.state_id)
    }

    #[getter]
    fn kind(&self) -> &'static str {
        match self.entry.kind {
            EntryKind::Combat => "combat",
            EntryKind::Screen => "screen",
        }
    }

    #[getter]
    fn rows<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.entry
            .rows
            .as_ref()
            .map(|r| crate::value_to_py(py, &Value::Array(r.clone())))
            .transpose()
    }

    #[getter]
    fn options<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.entry
            .options
            .as_ref()
            .map(|r| crate::value_to_py(py, &Value::Array(r.clone())))
            .transpose()
    }

    #[getter]
    fn cs(&self, py: Python<'_>) -> Py<PyAny> {
        none_or(py, self.entry.cs.as_ref())
    }

    #[getter]
    fn cfg(&self, py: Python<'_>) -> Py<PyAny> {
        none_or(py, self.entry.cfg.as_ref())
    }

    #[getter]
    fn advice<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::value_to_py(py, &self.entry.advice)
    }

    #[getter]
    fn pv_labels(&self) -> Vec<String> {
        self.entry.pv_labels.clone()
    }

    #[getter]
    fn msg<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::value_to_py(py, &self.entry.msg)
    }
}

/// `realgame_coach.AdviceCache`: a bounded LRU of advice requests.
#[pyclass(name = "CoachAdviceCache", module = "sts2_rs", frozen)]
#[derive(Debug)]
pub struct PyAdviceCache {
    cache: SharedCache<PyObj, PyObj>,
}

fn list_of(arg: Option<&Bound<'_, PyAny>>, name: &str) -> PyResult<Option<Vec<Value>>> {
    match arg.filter(|a| !a.is_none()) {
        None => Ok(None),
        Some(a) => match json(a, name)? {
            Value::Array(v) => Ok(Some(v)),
            _ => Err(PyValueError::new_err(format!("{name} must be a list"))),
        },
    }
}

#[pymethods]
impl PyAdviceCache {
    #[new]
    #[pyo3(signature = (size = cg::ADVICE_CACHE_SIZE))]
    fn new(size: usize) -> Self {
        Self {
            cache: Arc::new(Mutex::new(AdviceCache::new(size))),
        }
    }

    /// Remember one request's result; `None` (nothing stored) without an id.
    #[pyo3(signature = (request_id, state_id, *, kind, rows = None, options = None, cs = None, cfg = None, advice = None, pv_labels = None, msg = None))]
    #[allow(clippy::too_many_arguments)]
    fn put(
        &self,
        py: Python<'_>,
        request_id: &Bound<'_, PyAny>,
        state_id: &Bound<'_, PyAny>,
        kind: &str,
        rows: Option<&Bound<'_, PyAny>>,
        options: Option<&Bound<'_, PyAny>>,
        cs: Option<&Bound<'_, PyAny>>,
        cfg: Option<&Bound<'_, PyAny>>,
        advice: Option<&Bound<'_, PyAny>>,
        pv_labels: Option<Vec<String>>,
        msg: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Option<PyCoachEntry>> {
        let kind = match kind {
            "combat" => EntryKind::Combat,
            "screen" => EntryKind::Screen,
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown entry kind {other:?}"
                )))
            }
        };
        let obj = |a: Option<&Bound<'_, PyAny>>, name: &str| -> PyResult<Value> {
            a.filter(|a| !a.is_none())
                .map_or(Ok(serde_json::json!({})), |a| json(a, name))
        };
        let entry = Entry {
            request_id: json(request_id, "request_id")?,
            state_id: json(state_id, "state_id")?,
            kind,
            rows: list_of(rows, "rows")?,
            options: list_of(options, "options")?,
            cs: opt_obj(cs),
            cfg: opt_obj(cfg),
            advice: obj(advice, "advice")?,
            pv_labels: pv_labels.unwrap_or_default(),
            msg: obj(msg, "msg")?,
        };
        let cache = self.cache.clone();
        Ok(py
            .allow_threads(move || cache.lock().ok().and_then(|mut c| c.put(entry)))
            .map(|entry| PyCoachEntry { entry }))
    }

    fn get(&self, py: Python<'_>, request_id: &Bound<'_, PyAny>) -> PyResult<Option<PyCoachEntry>> {
        let id = json(request_id, "request_id")?;
        let cache = self.cache.clone();
        Ok(py
            .allow_threads(move || cache.lock().ok().and_then(|mut c| c.get(&id)))
            .map(|entry| PyCoachEntry { entry }))
    }

    fn clear(&self, py: Python<'_>) {
        let cache = self.cache.clone();
        py.allow_threads(move || {
            if let Ok(mut c) = cache.lock() {
                c.clear();
            }
        });
    }

    fn __len__(&self, py: Python<'_>) -> usize {
        let cache = self.cache.clone();
        py.allow_threads(move || cache.lock().map_or(0, |c| c.len()))
    }
}

/// `realgame_coach.GraderThread`: the coach's background worker.
#[pyclass(name = "CoachGraderThread", module = "sts2_rs", frozen)]
#[derive(Debug)]
pub struct PyGraderThread {
    thread: Arc<GraderThread>,
}

#[pymethods]
impl PyGraderThread {
    #[new]
    fn new() -> Self {
        Self {
            thread: Arc::new(GraderThread::new()),
        }
    }

    /// Queue a Python job (run with the GIL on the worker).
    fn submit(&self, job: Py<PyAny>) {
        self.thread.submit(Box::new(move || {
            Python::with_gil(|py| {
                if let Err(e) = job.call0(py) {
                    warn(
                        py,
                        &format!("coach grader job failed: {}", exc_text(py, &e)),
                    );
                }
            });
        }));
    }

    fn cancel(&self) {
        self.thread.cancel();
    }

    fn stop(&self) {
        self.thread.stop();
    }

    /// Block until every queued job has run (tests); `False` on timeout.
    fn wait_idle(&self, py: Python<'_>, timeout: f64) -> bool {
        let thread = self.thread.clone();
        py.allow_threads(move || {
            thread.wait_idle(std::time::Duration::from_secs_f64(timeout.max(0.0)))
        })
    }
}

/// The executor a Python `executor` argument names.
struct SharedThread(Arc<GraderThread>);

impl Executor for SharedThread {
    fn submit(&self, job: cg::Job) {
        self.0.submit(job);
    }
    fn cancel(&self) {
        self.0.cancel();
    }
    fn stop(&self) {
        self.0.stop();
    }
}

/// `realgame_coach.CoachGrader`'s core.
#[pyclass(name = "CoachGraderCore", module = "sts2_rs", frozen)]
#[derive(Debug)]
pub struct PyCoachGrader {
    grader: CoachGrader<PyObj, PyObj>,
}

fn sink(f: Py<PyAny>, what: &'static str) -> cg::Sink {
    Box::new(move |record: &Value| {
        Python::with_gil(|py| {
            let run = || -> PyResult<()> {
                f.call1(py, (crate::value_to_py(py, record)?,))?;
                Ok(())
            };
            if let Err(e) = run() {
                warn(py, &format!("coach: {what} failed: {}", exc_text(py, &e)));
            }
        });
    })
}

#[pymethods]
impl PyCoachGrader {
    /// `executor`: a `CoachGraderThread`, or `None` for inline (jobs run
    /// before the call that queued them returns).
    #[new]
    #[pyo3(signature = (*, send, log, engine, names, cache, executor = None, catchup_sims = 2000, budget_s = 6.0, map_path = None, clock = None, locate = None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        send: Py<PyAny>,
        log: Py<PyAny>,
        engine: Py<PyAny>,
        names: &crate::display_names::PyDisplayNames,
        cache: &PyAdviceCache,
        executor: Option<&PyGraderThread>,
        catchup_sims: u32,
        budget_s: f64,
        map_path: Option<Py<PyAny>>,
        clock: Option<Py<PyAny>>,
        locate: Option<Py<PyAny>>,
    ) -> Self {
        let executor: Arc<dyn Executor> = match executor {
            Some(t) => Arc::new(SharedThread(t.thread.clone())),
            None => Arc::new(InlineExecutor::default()),
        };
        let clock: cg::Clock = if let Some(c) = clock {
            Box::new(move || {
                Python::with_gil(|py| {
                    c.call0(py)
                        .and_then(|v| v.extract::<f64>(py))
                        .unwrap_or(0.0)
                })
            })
        } else {
            let t0 = std::time::Instant::now();
            Box::new(move || t0.elapsed().as_secs_f64())
        };
        let map_path: Option<cg::MapPath> = map_path.map(|f| -> cg::MapPath {
            Box::new(move |msg: &Value, chosen: usize| {
                Python::with_gil(|py| {
                    let run = || -> PyResult<Option<Vec<Value>>> {
                        let out = f.call1(py, (crate::value_to_py(py, msg)?, chosen))?;
                        let out = out.bind(py);
                        if out.is_none() {
                            return Ok(None);
                        }
                        match json(out, "path")? {
                            Value::Array(a) => Ok(Some(a)),
                            _ => Ok(None),
                        }
                    };
                    run().unwrap_or(None)
                })
            })
        });
        let locate: Option<cg::Locate> = locate.map(|f| -> cg::Locate {
            Box::new(move |node: &Value| {
                Python::with_gil(|py| {
                    let run = || -> PyResult<(Option<String>, Option<i64>)> {
                        f.call1(py, (crate::value_to_py(py, node)?,))?.extract(py)
                    };
                    run().unwrap_or((None, None))
                })
            })
        });
        let cfg = GraderConfig {
            send: sink(send, "send"),
            log: sink(log, "log"),
            warn: Box::new(|m: &str| Python::with_gil(|py| warn(py, m))),
            engine: Box::new(PyEngine { engine }),
            executor,
            names: Arc::new(names.names.clone()),
            catchup_sims,
            budget_s,
            map_path,
            clock,
            locate,
            cache: cache.cache.clone(),
        };
        Self {
            grader: CoachGrader::new(cfg),
        }
    }

    #[getter]
    fn mod_coach(&self, py: Python<'_>) -> i64 {
        let g = self.grader.clone();
        py.allow_threads(move || g.mod_coach())
    }

    fn on_advice_request(&self, py: Python<'_>, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        let (g, msg) = (self.grader.clone(), json(msg, "msg")?);
        py.allow_threads(move || g.on_advice_request(&msg));
        Ok(())
    }

    fn on_rewind<'py>(
        &self,
        py: Python<'py>,
        msg: &Bound<'py, PyAny>,
        active_path: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let msg = json(msg, "msg")?;
        let path: Vec<Value> = active_path
            .try_iter()?
            .map(|n| json(&n?, "node"))
            .collect::<PyResult<_>>()?;
        let g = self.grader.clone();
        let out = py.allow_threads(move || g.on_rewind(&msg, &path));
        crate::value_to_py(py, &Value::Array(out))
    }

    #[pyo3(signature = (entry))]
    fn on_advice(&self, py: Python<'_>, entry: Option<&PyCoachEntry>) {
        let entry = entry.map(|e| e.entry.clone());
        let g = self.grader.clone();
        py.allow_threads(move || g.on_advice(entry.as_deref()));
    }

    fn on_combat_ended(&self, py: Python<'_>, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        let (g, msg) = (self.grader.clone(), json(msg, "msg")?);
        py.allow_threads(move || g.on_combat_ended(&msg));
        Ok(())
    }

    fn on_run_end(&self, py: Python<'_>) {
        let g = self.grader.clone();
        py.allow_threads(move || g.on_run_end());
    }

    fn on_game_over(&self, py: Python<'_>, msg: &Bound<'_, PyAny>) -> PyResult<()> {
        let (g, msg) = (self.grader.clone(), json(msg, "msg")?);
        py.allow_threads(move || g.on_game_over(&msg));
        Ok(())
    }

    #[pyo3(signature = (msg, pre_state = None, node_id = None))]
    fn on_human_action<'py>(
        &self,
        py: Python<'py>,
        msg: &Bound<'py, PyAny>,
        pre_state: Option<&Bound<'py, PyAny>>,
        node_id: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let msg = json(msg, "msg")?;
        let pre = opt_obj(pre_state);
        let node = node_id.map_or(Ok(Value::Null), |n| json(n, "node_id"))?;
        let g = self.grader.clone();
        let out = py.allow_threads(move || g.on_human_action(&msg, pre, &node));
        crate::value_to_py(py, &Value::Array(out))
    }

    fn label_human(&self, py: Python<'_>, msg: &Bound<'_, PyAny>) -> PyResult<String> {
        let (g, msg) = (self.grader.clone(), json(msg, "msg")?);
        Ok(py.allow_threads(move || g.label_human(&msg)))
    }

    fn label_engine(
        &self,
        py: Python<'_>,
        state: &Bound<'_, PyAny>,
        action: &Bound<'_, PyAny>,
    ) -> PyResult<String> {
        let (g, state, action) = (
            self.grader.clone(),
            json(state, "state")?,
            json(action, "action")?,
        );
        Ok(py.allow_threads(move || g.label_engine(&state, &action)))
    }

    /// `{id: node_id}` of the emitted records, in emission order.
    fn emitted<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let g = self.grader.clone();
        let items = py.allow_threads(move || g.emitted());
        pairs_dict(py, &items)
    }

    /// `{id: record}` of the emitted records, in emission order.
    fn emitted_records<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let g = self.grader.clone();
        let items = py.allow_threads(move || g.emitted_records());
        pairs_dict(py, &items)
    }

    /// How many ended turns are still being graded.
    fn grading(&self, py: Python<'_>) -> usize {
        let g = self.grader.clone();
        py.allow_threads(move || g.grading())
    }
}

fn pairs_dict<'py>(py: Python<'py>, items: &[(Value, Value)]) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    for (k, v) in items {
        d.set_item(crate::value_to_py(py, k)?, crate::value_to_py(py, v)?)?;
    }
    Ok(d)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCoachEntry>()?;
    m.add_class::<PyAdviceCache>()?;
    m.add_class::<PyGraderThread>()?;
    m.add_class::<PyCoachGrader>()?;
    Ok(())
}
