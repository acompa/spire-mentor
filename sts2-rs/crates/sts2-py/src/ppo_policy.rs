//! A `tract`-backed twin of `ppo.onnx_policy.OnnxPolicy`, for Python.
//!
//! The advisor's run policy answers every non-combat decision by
//! featurizing an observation and running the champion graph. Both halves
//! already exist in Rust — [`sts2_runai::ppo::encoder`] is pinned
//! bit-for-bit against `ppo/encoder.py` by `ppo_encoder_parity`, and
//! [`sts2_runai::ppo::net`] against torch by `ppo_net_parity` — but the
//! live lane still ran the Python pair, which is the *only* reason the
//! shipped advisor bundle carries `numpy` and `onnxruntime` (~54 MB of a
//! 165 MB stage, and most of its cold start).
//!
//! [`PyPpoNetPolicy::distribution`] has the exact signature
//! `OnnxPolicy.distribution` has — `(obs, actions, metrics) -> (probs,
//! value)` — so `realgame_ppo` swaps the backend without knowing it did.
//! Scoring the *observation dict* rather than the bridge message is what
//! makes that possible: it covers every surface the Python builders emit,
//! including `card_select`, which the wire-side port
//! ([`crate::ppo_wire`]) does not.
//!
//! # One champion resolver, not two
//!
//! The constructor takes a path. It deliberately does NOT resolve a
//! character to a champion, even though
//! [`sts2_runai::ppo::net::onnx_model_path_for`] could: `champions.toml`
//! is the single source of truth, Python reads it directly, and a second
//! resolver here is exactly the shape of the bug that shipped a container
//! serving a retired net (#339, and again nearly in #404/#402). The Rust
//! resolver also looks for a *sibling checkout* that does not exist inside
//! the frozen exe.

use std::path::Path;
use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyAny;

use sts2_runai::ppo::encoder;
use sts2_runai::ppo::features::Features;
use sts2_runai::ppo::net::PpoNet;
use sts2_runai::ppo::obs::{ActionDesc, PpoObs};

create_exception!(
    sts2_rs,
    PpoBudgetExceeded,
    PyRuntimeError,
    "Raised when a decision exceeds the exported graph's fixed token or\n\
     action budget. The Python twin of `ppo.schema.BudgetExceededError`;\n\
     `ppo.rust_policy` re-raises it as that type so existing callers --\n\
     which degrade to a random legal pick rather than crash a live run --\n\
     keep working unchanged."
);

/// Deserialize `value` into `T`, naming `what` in the error.
fn from_json<T: serde::de::DeserializeOwned>(value: serde_json::Value, what: &str) -> PyResult<T> {
    serde_json::from_value(value)
        .map_err(|e| PyValueError::new_err(format!("ppo policy: bad {what}: {e}")))
}

/// `{**metrics, **obs}` as one JSON object — the merge the encoder reads.
///
/// `realgame_ppo` splits the observation across two dicts because
/// `NetPolicy.distribution(obs, actions, metrics)` takes them separately;
/// [`PpoObs`] is one struct holding both.
///
/// `obs` wins the overlap, which matters for exactly one key: both dicts
/// carry `max_hp`, with different defaults (`_base_obs` uses `or 1`,
/// `_base_metrics` uses `, 0`), and it is `_base_obs`'s that
/// `ppo/encoder.py`'s `_Ctx.max_hp` actually reads.
fn merge_obs(obs: serde_json::Value, metrics: serde_json::Value) -> PyResult<serde_json::Value> {
    let (serde_json::Value::Object(obs), serde_json::Value::Object(metrics)) = (obs, metrics)
    else {
        return Err(PyValueError::new_err(
            "ppo policy: obs and metrics must both be dicts",
        ));
    };
    let mut merged = metrics;
    merged.extend(obs);
    Ok(serde_json::Value::Object(merged))
}

fn depythonize(value: &Bound<'_, PyAny>, what: &str) -> PyResult<serde_json::Value> {
    pythonize::depythonize(value)
        .map_err(|e| PyValueError::new_err(format!("ppo policy: {what} is not JSON-shaped: {e}")))
}

/// One loaded champion graph, scored through `tract`.
///
/// Construction is the expensive part (`tract` parses and optimizes the
/// ~8 MB graph), so hold one per character for the session — the same
/// contract `OnnxPolicy` documents, and the reason `realgame_ppo`'s
/// `PpoPolicyCache` exists. Map-route projection alone re-enters
/// [`Self::distribution`] up to 8 times per decision.
#[pyclass(name = "PpoNetPolicy", module = "sts2_rs")]
pub struct PyPpoNetPolicy {
    pub(crate) net: Arc<PpoNet>,
    /// Encoder schema THIS net was trained at — see [`Self::new`].
    pub(crate) features: Features,
}

/// Hand-written because `PpoNet` wraps a `tract` plan, which is not
/// `Debug` — and dumping it would be noise anyway. The budgets are what
/// identifies a loaded graph.
impl std::fmt::Debug for PyPpoNetPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PpoNetPolicy")
            .field("t_max", &self.net.t_max())
            .field("a_max", &self.net.a_max())
            .field("n_cont", &self.net.cont_dim())
            // Worth printing even though the graph itself is not: a policy
            // serving at the wrong schema is otherwise indistinguishable from
            // a correct one, since the tensor shapes are identical.
            .field("features", &self.features)
            .finish()
    }
}

#[pymethods]
impl PyPpoNetPolicy {
    /// Load an exported champion `.onnx` from `path`, at the encoder schema
    /// `features` names.
    ///
    /// `features` is a `STS2_PPO_FEATURES` spec (comma-separated, `""` for the
    /// default schema) — the caller passes the value `champions.toml` records
    /// for THIS net, because the constructor takes a path and so cannot look
    /// it up (see the module doc on why this type does not resolve champions).
    /// Omitting it falls back to the process-wide `STS2_PPO_FEATURES`, which
    /// is correct only when the process serves exactly one schema.
    ///
    /// Passing the wrong spec is silent: a gated feature rewrites an existing
    /// slot instead of adding a column, so shapes match either way and the net
    /// is simply evaluated on an observation it never trained on.
    ///
    /// Raises `ValueError` when the file is missing, `tract` cannot load it,
    /// or `features` names something unknown. Callers on the advice path treat
    /// a load failure as "no policy" and fall back, exactly as they do for a
    /// failed `onnxruntime` session.
    #[new]
    #[pyo3(signature = (path, features=None))]
    fn new(path: &str, features: Option<&str>) -> PyResult<Self> {
        let features = match features {
            // `try_parse`, not `parse`: the latter panics on an unknown name
            // (deliberately, so a typo cannot screen as the default arm), and a
            // panic crossing the FFI boundary is far worse than an exception.
            Some(spec) => Features::try_parse(spec).map_err(PyValueError::new_err)?,
            None => Features::active(),
        };
        let net = PpoNet::load(Path::new(path)).map_err(|e| {
            PyValueError::new_err(format!("failed to load PPO model {path:?}: {e}"))
        })?;
        Ok(Self {
            net: Arc::new(net),
            features,
        })
    }

    /// Token budget of the loaded graph (`OnnxPolicy.t_max`'s twin).
    #[getter]
    fn t_max(&self) -> usize {
        self.net.t_max()
    }

    /// Action-slot budget of the loaded graph (`OnnxPolicy.a_max`'s twin).
    #[getter]
    fn a_max(&self) -> usize {
        self.net.a_max()
    }

    /// Continuous-feature width of the loaded graph (`OnnxPolicy.n_cont`).
    #[getter]
    fn n_cont(&self) -> usize {
        self.net.cont_dim()
    }

    /// Masked softmax over the candidates, plus `V(s)`.
    ///
    /// Same contract as `ppo.onnx_policy.OnnxPolicy.distribution` and
    /// `ppo.net_policy.NetPolicy.distribution`: `probs` is one float per
    /// entry of `actions`, in order, summing to 1.
    ///
    /// Raises [`PpoBudgetExceeded`] when the decision does not fit the
    /// exported graph's fixed shape — the fixed-shape path's one failure
    /// mode that a caller must degrade on rather than crash (GH #315's
    /// 103-option shop removal is the known instance).
    fn distribution(
        &self,
        obs: &Bound<'_, PyAny>,
        actions: &Bound<'_, PyAny>,
        metrics: &Bound<'_, PyAny>,
    ) -> PyResult<(Vec<f64>, f64)> {
        let merged = merge_obs(depythonize(obs, "obs")?, depythonize(metrics, "metrics")?)?;
        let obs: PpoObs = from_json(merged, "obs")?;
        let actions: Vec<ActionDesc> = from_json(depythonize(actions, "actions")?, "actions")?;

        let feat = encoder::featurize_with(&obs, &actions, self.features);
        // Checked here rather than read off `infer`'s error string: the
        // budget overflow is the one failure the caller must tell apart
        // (it degrades to a random legal pick and counts the decision),
        // and matching on message text is how that distinction rots.
        let (obs_len, n_actions) = (feat.obs_cat.len(), feat.n_actions);
        if obs_len + n_actions > self.net.t_max() || n_actions > self.net.a_max() {
            return Err(PpoBudgetExceeded::new_err(format!(
                "decision exceeds export budget: obs={obs_len} act={n_actions} \
                 (t_max={} a_max={})",
                self.net.t_max(),
                self.net.a_max()
            )));
        }
        let (probs, value) = self
            .net
            .infer(&feat)
            .map_err(|e| PyRuntimeError::new_err(format!("ppo policy: inference failed: {e}")))?;
        Ok((probs.into_iter().map(f64::from).collect(), f64::from(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The overlap rule is not cosmetic: `_base_obs` and `_base_metrics`
    /// BOTH carry `max_hp` with different fallbacks (`or 1` vs `, 0`), and
    /// it is `_base_obs`'s that `ppo/encoder.py`'s `_Ctx.max_hp` reads. Merge
    /// the other way and every HP-ratio feature divides by zero-or-one on
    /// any state where the wire omitted `max_hp`.
    #[test]
    fn obs_wins_the_overlap_with_metrics() {
        let merged = merge_obs(json!({"max_hp": 80}), json!({"max_hp": 0, "floor": 12})).unwrap();
        assert_eq!(merged["max_hp"], json!(80));
        // Non-overlapping metrics still come through.
        assert_eq!(merged["floor"], json!(12));
    }

    #[test]
    fn merge_rejects_non_objects() {
        assert!(merge_obs(json!([1, 2]), json!({})).is_err());
        assert!(merge_obs(json!({}), json!("nope")).is_err());
    }

    /// The SpireMentor dialect omits structural keys the training producer
    /// always writes. This is the deserialize half of the contract that
    /// `ppo::obs`'s module note describes; without it every live map
    /// decision raises instead of scoring.
    #[test]
    fn bridge_shaped_obs_deserializes() {
        let merged = merge_obs(
            json!({
                "map": {
                    "rows": [[{"col": 0, "row": 0, "type": "Monster", "children": []}]],
                    // No `type` on the boss, no `current_coord`, no
                    // `visited`/`current` on the node -- the live shape.
                    "boss": {"col": 1, "row": 2},
                },
                "shop": {
                    // No `idx` / `on_sale` / `is_colorless` / `removal_used`.
                    "cards": [{"id": "STRIKE", "price": 50, "available": true}],
                    "relics": [],
                    "potions": [],
                    "removal_cost": 75,
                },
            }),
            // The scalars stay REQUIRED on purpose -- `_base_obs` /
            // `_base_metrics` write every one of them, so a missing scalar
            // is a broken producer, not a dialect difference.
            json!({
                "phase": "map_choice",
                "seed": 12345,
                "ascension": 7,
                "deck": [["STRIKE", false]],
                "relics": ["BURNING_BLOOD"],
                "potions": [],
                "total_floor": 3,
                "act_index": 0,
                "act_floor": 3,
                "hp": 68,
                "max_hp": 80,
                "gold": 99,
                "deck_size": 1,
                "num_upgraded": 0,
                "num_relics": 1,
                "num_potions": 0,
            }),
        )
        .unwrap();
        let obs: PpoObs = serde_json::from_value(merged).expect("bridge-shaped obs must parse");
        let map = obs.map.expect("map");
        assert!(map.current_coord.is_none());
        assert_eq!(map.boss.expect("boss").col, 1);
        assert_eq!(obs.shop.expect("shop").cards[0].price, 50);
    }
}

/// The state token's scalars, which `encoder.py::featurize_row` reads from
/// `metrics` (as `int(metrics.get(k) or 0)`), not from `obs`.
const METRIC_SCALARS: [&str; 7] = [
    "hp",
    "gold",
    "act_index",
    "num_upgraded",
    "num_relics",
    "total_floor",
    "act_floor",
];

/// [`PpoObs`]'s required keys, with the value `encoder.py` reads when one
/// is missing or `None` (it reads every key with `.get(k) or <default>`).
/// The env's observations carry them all; hand-built test rows and the e2e
/// `honest_obs` view need not.
fn obs_defaults() -> [(&'static str, serde_json::Value); 16] {
    use serde_json::json;
    [
        ("phase", json!("")),
        ("seed", json!(0)),
        ("ascension", json!(0)),
        ("deck", json!([])),
        ("relics", json!([])),
        ("potions", json!([])),
        ("total_floor", json!(0)),
        ("act_index", json!(0)),
        ("act_floor", json!(0)),
        ("hp", json!(0)),
        ("max_hp", json!(0)),
        ("gold", json!(0)),
        ("deck_size", json!(0)),
        ("num_upgraded", json!(0)),
        ("num_relics", json!(0)),
        ("num_potions", json!(0)),
    ]
}

/// `metrics[k]` as `featurize_row` reads it: a missing, null, zero or
/// false value is `0`.
fn metric_scalar(metrics: &serde_json::Map<String, serde_json::Value>, key: &str) -> i64 {
    match metrics.get(key) {
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        Some(serde_json::Value::Bool(b)) => i64::from(*b),
        _ => 0,
    }
}

/// One `[n, w]` array as `(bytearray of little-endian values, n)`.
fn rows_bytes<T: Copy, const W: usize>(
    py: Python<'_>,
    rows: &[[T; W]],
    to_le: impl Fn(T) -> [u8; 8],
    width: usize,
) -> Py<pyo3::types::PyByteArray> {
    let mut out = Vec::with_capacity(rows.len() * W * width);
    for row in rows {
        for &v in row {
            out.extend_from_slice(&to_le(v)[..width]);
        }
    }
    pyo3::types::PyByteArray::new(py, &out).unbind()
}

fn i64s_bytes(py: Python<'_>, v: &[i64]) -> Py<pyo3::types::PyByteArray> {
    let out: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
    pyo3::types::PyByteArray::new(py, &out).unbind()
}

/// The candidate actions, with one that names no kind (no string
/// `"action"` key) read as [`ActionDesc::Unknown`]: `encoder.py` reads the
/// kind with `.get("action")` and degrades it to the generic fixed token.
fn tolerant_actions(actions: serde_json::Value) -> PyResult<Vec<ActionDesc>> {
    let serde_json::Value::Array(items) = actions else {
        return Err(PyValueError::new_err("ppo policy: actions must be a list"));
    };
    items
        .into_iter()
        .map(|a| match a {
            serde_json::Value::Object(ref m)
                if !m.get("action").is_some_and(serde_json::Value::is_string) =>
            {
                Ok(ActionDesc::Unknown)
            }
            a => from_json(a, "actions"),
        })
        .collect()
}

/// `ppo/encoder.py::featurize_row` in Rust: the run-policy encoder the
/// trainer, the arena and the advisor all share (GH #1426 step 2).
///
/// `features` is the comma-separated `STS2_PPO_FEATURES` schema. Returns a
/// dict of little-endian `bytearray`s (`obs_cat`, `act_cat`: int64 `[n, 14]`;
/// `obs_cont`, `act_cont`: float32 `[n, N_CONT]`; `obs_sec`, `act_sec`,
/// `aux_room`: int64 `[n]`) plus `n_obs` and `n_actions`; `ppo.encoder`
/// wraps them as numpy arrays without a copy.
///
/// The state token's scalars come from `metrics`, exactly as Python reads
/// them; everything else from `{**metrics, **obs}`.
#[pyfunction]
#[pyo3(signature = (obs, actions, metrics, features = ""))]
pub fn ppo_featurize<'py>(
    py: Python<'py>,
    obs: &Bound<'py, PyAny>,
    actions: &Bound<'py, PyAny>,
    metrics: &Bound<'py, PyAny>,
    features: &str,
) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
    let features = Features::try_parse(features).map_err(PyValueError::new_err)?;
    let metrics = depythonize(metrics, "metrics")?;
    let mut merged = merge_obs(depythonize(obs, "obs")?, metrics.clone())?;
    if let (serde_json::Value::Object(m), serde_json::Value::Object(metrics)) =
        (&mut merged, &metrics)
    {
        for (key, default) in obs_defaults() {
            if m.get(key).is_none_or(serde_json::Value::is_null) {
                m.insert(key.into(), default);
            }
        }
        for key in METRIC_SCALARS {
            m.insert(key.into(), metric_scalar(metrics, key).into());
        }
    }
    let obs: PpoObs = from_json(merged, "obs")?;
    let actions = tolerant_actions(depythonize(actions, "actions")?)?;
    let f = encoder::featurize_with(&obs, &actions, features);

    let out = pyo3::types::PyDict::new(py);
    let i64_le = |v: i64| v.to_le_bytes();
    let f32_le = |v: f32| {
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&v.to_le_bytes());
        b
    };
    out.set_item("obs_cat", rows_bytes(py, &f.obs_cat, i64_le, 8))?;
    out.set_item("obs_cont", rows_bytes(py, &f.obs_cont, f32_le, 4))?;
    out.set_item("obs_sec", i64s_bytes(py, &f.obs_sec))?;
    out.set_item("act_cat", rows_bytes(py, &f.act_cat, i64_le, 8))?;
    out.set_item("act_cont", rows_bytes(py, &f.act_cont, f32_le, 4))?;
    out.set_item("act_sec", i64s_bytes(py, &f.act_sec))?;
    out.set_item("aux_room", i64s_bytes(py, &f.aux_room))?;
    out.set_item("n_obs", f.obs_cat.len())?;
    out.set_item("n_actions", f.n_actions)?;
    Ok(out)
}

/// `ppo/encoder.py::fine_surface` in Rust: the state token's surface key.
#[pyfunction]
pub fn ppo_fine_surface(actions: &Bound<'_, PyAny>) -> PyResult<&'static str> {
    let actions: Vec<ActionDesc> = from_json(depythonize(actions, "actions")?, "actions")?;
    Ok(encoder::fine_surface(&actions))
}
