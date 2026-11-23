//! The run-policy champions the screen advice scores with, loaded lazily per
//! character (GH #1426 step 4; was `realgame_ppo.PpoPolicyCache` over
//! `load_ppo_policy`'s Rust backend).
//!
//! The net for a character is the ONNX champion `champions.toml` declares
//! (`sts2_runai::ppo::net::champion_onnx_filename`, generated from it) under
//! `<data root>/data/ppo/`, at the encoder schema the manifest records for
//! it (`features_for`). Only the Rust backend: the torch and onnxruntime dev
//! backends stay in the Python advisor.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sts2_core::potions::CharacterId;
use sts2_runai::ppo::features::Features;
use sts2_runai::ppo::net::PpoNet;
use sts2_runai::ppo::obs::{ActionDesc, PpoObs};

use crate::screen_advice::{Lookup, ScreenNet};

/// One loaded champion, scored as `PpoNetPolicy.distribution` scores it.
#[derive(Clone)]
pub struct PolicyNet {
    pub net: Arc<PpoNet>,
    pub features: Features,
}

impl std::fmt::Debug for PolicyNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyNet")
            .field("t_max", &self.net.t_max())
            .field("a_max", &self.net.a_max())
            .field("features", &self.features)
            .finish()
    }
}

impl ScreenNet for PolicyNet {
    fn score(&self, obs: &PpoObs, actions: &[ActionDesc]) -> Result<(Vec<f64>, f64), String> {
        let feat = sts2_runai::ppo::encoder::featurize_with(obs, actions, self.features);
        let (obs_len, n_actions) = (feat.obs_cat.len(), feat.n_actions);
        if obs_len + n_actions > self.net.t_max() || n_actions > self.net.a_max() {
            return Err(format!(
                "decision exceeds export budget: obs={obs_len} act={n_actions} \
                 (t_max={} a_max={})",
                self.net.t_max(),
                self.net.a_max()
            ));
        }
        let (probs, value) = self
            .net
            .infer(&feat)
            .map_err(|e| format!("ppo policy: inference failed: {e}"))?;
        Ok((probs.into_iter().map(f64::from).collect(), f64::from(value)))
    }

    fn trained_on_packs(&self) -> bool {
        self.features.card_bundles
    }
}

/// A character's display or id spelling as the engine's id.
#[must_use]
pub fn character_id(name: &str) -> Option<CharacterId> {
    Some(match name.trim().to_lowercase().as_str() {
        "ironclad" => CharacterId::Ironclad,
        "silent" => CharacterId::Silent,
        "defect" => CharacterId::Defect,
        "regent" => CharacterId::Regent,
        "necrobinder" => CharacterId::Necrobinder,
        "fool" => CharacterId::Fool,
        _ => return None,
    })
}

type Loaded = Result<PolicyNet, String>;

/// Why a Fool run's screens get no ranking, player-facing.
pub const NO_FOOL_NET: &str =
    "none has been trained for The Fool yet (fights still get the combat search)";

/// Per-character lazy champion cache for one advisor process. Thread-safe:
/// the serve loop warms it ahead of the first screen from another thread.
#[derive(Debug)]
pub struct PolicyCache {
    data_root: PathBuf,
    cache: Mutex<BTreeMap<String, Loaded>>,
}

impl PolicyCache {
    #[must_use]
    pub fn new(data_root: PathBuf) -> Self {
        Self {
            data_root,
            cache: Mutex::new(BTreeMap::new()),
        }
    }

    fn load(&self, character: &str) -> Loaded {
        let Some(id) = character_id(character) else {
            return Err(format!(
                "no PPO champion for character {character:?} (no committed champion)"
            ));
        };
        let path = self
            .data_root
            .join("data/ppo")
            .join(sts2_runai::ppo::net::champion_onnx_filename(id));
        if !path.is_file() {
            if id == CharacterId::Fool {
                // No champion is trained for the Fool (champions.toml has no
                // entry): the HUD shows this as the screen's note.
                return Err(NO_FOOL_NET.to_string());
            }
            return Err(format!(
                "no ONNX champion for {character:?} at {}",
                path.display()
            ));
        }
        let net = PpoNet::load(&path).map_err(|e| {
            format!(
                "failed to load ONNX champion {:?}: failed to load PPO model: {e}",
                path.display().to_string()
            )
        })?;
        Ok(PolicyNet {
            net: Arc::new(net),
            features: sts2_runai::ppo::net::features_for(id),
        })
    }

    /// The character's net, or why there is none. The first miss for a
    /// character returns the error line to log (`Some`) once.
    pub fn get(&self, character: &str) -> (Loaded, Option<String>) {
        let key = {
            let k = character.trim().to_lowercase();
            if k.is_empty() {
                "ironclad".to_string()
            } else {
                k
            }
        };
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut first_miss = None;
        let loaded = cache.entry(key).or_insert_with(|| {
            let loaded = self.load(character);
            if let Err(e) = &loaded {
                first_miss = Some(format!(
                    "PPO policy unavailable for character={character:?}: {e} -- every \
                     non-combat decision for this character will use the random-legal policy."
                ));
            }
            loaded
        });
        (loaded.clone(), first_miss)
    }

    /// `policy_for` for [`crate::screen_advice::ScreenEnv`]; misses are
    /// pushed to `log` as error lines.
    pub fn lookup(&self, character: &str, log: &mut Vec<String>) -> Lookup {
        let (loaded, miss) = self.get(character);
        if let Some(line) = miss {
            log.push(line);
        }
        match loaded {
            Ok(net) => Lookup::Net(Box::new(net)),
            Err(why) => Lookup::Missing(why),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_champion_is_reported_once() {
        let cache = PolicyCache::new(PathBuf::from("/nonexistent"));
        let (first, miss) = cache.get("Ironclad");
        assert!(first.is_err());
        assert!(miss.is_some());
        let (_, again) = cache.get("ironclad");
        assert!(again.is_none(), "one line per character");
        assert!(cache
            .get("Watcher")
            .0
            .unwrap_err()
            .contains("no committed champion"));
    }

    #[test]
    fn the_fool_has_no_net_and_says_so() {
        let cache = PolicyCache::new(PathBuf::from("/nonexistent"));
        assert_eq!(cache.get("Fool").0.unwrap_err(), NO_FOOL_NET);
    }
}
