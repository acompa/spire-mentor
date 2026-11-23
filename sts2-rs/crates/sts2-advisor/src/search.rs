//! The advisor's combat search knobs and the one search call (GH #1426; was
//! `sts2_hint.recommender.RecommendConfig` and `CombatMirror._recommend`).
//!
//! [`RecommendConfig`] is the Python dataclass with its defaults; a host
//! hands it over as the dict `dataclasses.asdict` makes.
//! [`RecommendConfig::params`] is what `sts2_rs.mcts_recommend` receives from
//! the advisor's call sites: they pass every knob but `dominance_pruning`
//! (off, matching the run agent) and `leaf_playouts` (the engine's one).

use serde_json::Value;
use sts2_core::CombatState;
use sts2_wire::emit::mcts::{parse_hidden_info, parse_rollout_policy, RecommendParams};

use crate::py;

/// `RecommendConfig`: the knobs of one search.
#[derive(Debug, Clone, PartialEq)]
pub struct RecommendConfig {
    pub sims: u32,
    pub depth: u32,
    /// `None`: what the mode deploys ([`Self::resolved_c_puct`]).
    pub c_puct: Option<f64>,
    pub seed: u64,
    pub threads: u32,
    pub fair: bool,
    /// The display name (`"Ironclad"`), or `None` to infer it from the cards.
    pub character: Option<String>,
    pub ascension: i64,
    pub combat_id: i64,
    pub rollout_policy: Option<String>,
    pub hidden_info: String,
    pub draw_widening_c: Option<f64>,
    pub endturn_widening_c: Option<f64>,
    pub endturn_widening_alpha: Option<f64>,
    pub common_random_numbers: bool,
    pub dominance_pruning: bool,
    pub win_hp_carry: Option<f64>,
    pub win_potion_carry: Option<f64>,
}

impl Default for RecommendConfig {
    fn default() -> Self {
        Self {
            sims: 300,
            depth: 200,
            c_puct: None,
            seed: 0,
            threads: 1,
            fair: false,
            character: None,
            ascension: 0,
            combat_id: 0,
            rollout_policy: None,
            hidden_info: "beliefdraw".into(),
            draw_widening_c: None,
            endturn_widening_c: None,
            endturn_widening_alpha: None,
            common_random_numbers: false,
            dominance_pruning: false,
            win_hp_carry: None,
            win_potion_carry: None,
        }
    }
}

fn opt_f64(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}

fn opt_str(v: Option<&Value>) -> Option<String> {
    v.filter(|v| !v.is_null()).map(py::str_of)
}

fn u32_of(v: Option<&Value>, default: u32) -> u32 {
    v.filter(|v| !v.is_null()).map_or(default, |v| {
        u32::try_from(py::int_of(v).max(0)).unwrap_or(u32::MAX)
    })
}

impl RecommendConfig {
    /// `RecommendConfig.from_dict`: the known keys over the defaults; any
    /// other key is ignored.
    #[must_use]
    pub fn from_value(d: &Value) -> Self {
        let mut c = Self::default();
        let g = |k: &str| d.get(k);
        c.sims = u32_of(g("sims"), c.sims);
        c.depth = u32_of(g("depth"), c.depth);
        c.threads = u32_of(g("threads"), c.threads);
        if let Some(v) = g("c_puct") {
            c.c_puct = v.as_f64();
        }
        if let Some(v) = g("seed").filter(|v| !v.is_null()) {
            c.seed = v
                .as_u64()
                .unwrap_or_else(|| py::int_of(v).max(0).unsigned_abs());
        }
        if let Some(v) = g("fair") {
            c.fair = py::truthy(v);
        }
        if let Some(v) = g("character") {
            c.character = opt_str(Some(v));
        }
        if let Some(v) = g("ascension").filter(|v| !v.is_null()) {
            c.ascension = py::int_of(v);
        }
        if let Some(v) = g("combat_id").filter(|v| !v.is_null()) {
            c.combat_id = py::int_of(v);
        }
        if let Some(v) = g("rollout_policy") {
            c.rollout_policy = opt_str(Some(v));
        }
        if let Some(Value::String(s)) = g("hidden_info") {
            c.hidden_info.clone_from(s);
        }
        for (key, slot) in [
            ("draw_widening_c", &mut c.draw_widening_c),
            ("endturn_widening_c", &mut c.endturn_widening_c),
            ("endturn_widening_alpha", &mut c.endturn_widening_alpha),
            ("win_hp_carry", &mut c.win_hp_carry),
            ("win_potion_carry", &mut c.win_potion_carry),
        ] {
            if let Some(v) = g(key) {
                *slot = opt_f64(Some(v));
            }
        }
        if let Some(v) = g("common_random_numbers") {
            c.common_random_numbers = py::truthy(v);
        }
        if let Some(v) = g("dominance_pruning") {
            c.dominance_pruning = py::truthy(v);
        }
        c
    }

    /// `resolved_c_puct`: an explicit constant, else the BeliefDraw budget
    /// rule over the per-tree budget, else the engine default 1.4.
    #[must_use]
    pub fn resolved_c_puct(&self) -> f64 {
        if let Some(c) = self.c_puct {
            return c;
        }
        if self.hidden_info == "beliefdraw" {
            let per_tree = self.sims / self.threads.max(1);
            return f64::from(sts2_mcts::belief_draw_c_puct(per_tree));
        }
        1.4
    }

    /// `resolved_draw_widening_c`: an explicit constant, else 2.0 under
    /// BeliefDraw, else the engine default (`None`).
    #[must_use]
    pub fn resolved_draw_widening_c(&self) -> Option<f64> {
        self.draw_widening_c
            .or_else(|| (self.hidden_info == "beliefdraw").then_some(2.0))
    }

    /// The `mcts_recommend` parameters the advisor's call sites pass.
    ///
    /// # Errors
    ///
    /// An unknown `hidden_info` or `rollout_policy`.
    #[allow(clippy::cast_possible_truncation)] // the engine's knobs are f32
    pub fn params(&self) -> Result<RecommendParams, String> {
        let hidden_info = parse_hidden_info(&self.hidden_info, "mcts_recommend")?;
        let rollout_policy = self
            .rollout_policy
            .as_deref()
            .map(|p| parse_rollout_policy(p, "mcts_recommend"))
            .transpose()?;
        let honest = hidden_info == sts2_mcts::HiddenInfoMode::BeliefDraw;
        let draw_widening_c = self
            .resolved_draw_widening_c()
            .map(|c| c as f32)
            .or(honest.then_some(sts2_mcts::BELIEF_DRAW_DRAW_WIDENING_C));
        Ok(RecommendParams {
            sims: self.sims,
            depth: self.depth,
            c_puct: self.resolved_c_puct() as f32,
            seed: self.seed,
            threads: self.threads,
            fair: self.fair,
            rollout_policy,
            hidden_info,
            draw_widening_c,
            endturn_widening_c: self.endturn_widening_c.map(|c| c as f32),
            endturn_widening_alpha: self.endturn_widening_alpha.map(|c| c as f32),
            common_random_numbers: self.common_random_numbers,
            leaf_playouts: None,
            net_expand_order: false,
            fpu_reduction: None,
            dominance_pruning: false,
            win_hp_carry: self.win_hp_carry.map(|c| c as f32),
            win_potion_carry: self.win_potion_carry.map(|c| c as f32),
        })
    }
}

/// One `mcts_recommend` over `state` (it searches its own clone): the raw
/// result dict.
///
/// # Errors
///
/// An unknown `hidden_info` or `rollout_policy`.
pub fn recommend(
    state: &CombatState,
    intent_damage_known: bool,
    cfg: &RecommendConfig,
) -> Result<Value, String> {
    let params = cfg.params()?;
    Ok(sts2_wire::emit::mcts::recommend_value(
        state,
        &params,
        intent_damage_known,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_constants_follow_the_mode_and_the_per_tree_budget() {
        let c = RecommendConfig::from_value(&json!({"sims": 4000, "threads": 1, "junk": 1}));
        assert!((c.resolved_c_puct() - 2.5).abs() < 1e-6);
        assert_eq!(c.resolved_draw_widening_c(), Some(2.0));
        let split = RecommendConfig::from_value(&json!({"sims": 4000, "threads": 16}));
        assert!((split.resolved_c_puct() - 0.8).abs() < 1e-6);
        let leaky = RecommendConfig::from_value(&json!({"hidden_info": "clairvoyant"}));
        assert!((leaky.resolved_c_puct() - 1.4).abs() < 1e-9);
        assert_eq!(leaky.resolved_draw_widening_c(), None);
        let pinned = RecommendConfig::from_value(&json!({"c_puct": 3.0, "draw_widening_c": 1.0}));
        assert!((pinned.resolved_c_puct() - 3.0).abs() < 1e-9);
        assert!(RecommendConfig::from_value(&json!({"hidden_info": "nope"}))
            .params()
            .is_err());
    }
}
