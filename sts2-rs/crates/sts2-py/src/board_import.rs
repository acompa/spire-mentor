//! `sts2_rs.combat_wire_from_bridge`: a live SpireMentor fight board → the
//! engine wire dict `state_from_dict` takes (GH #1426 step 1).
//!
//! The translation is `sts2_workbench::combat_import`, the one the wasm
//! workbench's paste import already runs; this is only its Python door. It
//! replaces `realgame_adapter.to_headless_combat_play` followed by
//! `sts2_hint.adapter.translate` on the advisor's live path.

use pyo3::prelude::*;

use crate::state_dict::UnsupportedStateError;

/// Translate the mod's `combat_action` board into the engine wire dict.
///
/// Returns `(wire, warnings)`; each warning names an entity the rebuilt
/// board dropped (an unknown card, relic, power or potion). `combat_id` and
/// `seed` are the advisor's own, stamped as `sts2_hint.adapter.translate`
/// does. `character` is the display name (`"Ironclad"`); `None` infers it
/// from the card ids (a lane whose run state names no character).
///
/// # Errors
///
/// `UnsupportedStateError` for a board the engine cannot model (an unknown
/// monster or orb, an intent with no type, …), `ValueError` when the board
/// is not JSON-shaped.
#[pyfunction]
#[pyo3(signature = (board, character, ascension, combat_id=0, seed=0))]
pub fn combat_wire_from_bridge<'py>(
    py: Python<'py>,
    board: &Bound<'py, PyAny>,
    character: Option<&str>,
    ascension: u8,
    combat_id: i64,
    seed: u64,
) -> PyResult<(Bound<'py, PyAny>, Vec<String>)> {
    let board: serde_json::Value = pythonize::depythonize(board).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("board is not JSON-shaped: {e}"))
    })?;
    let character = match character {
        Some(c) => c,
        None => sts2_workbench::combat_import::infer_character(&board).ok_or_else(|| {
            UnsupportedStateError::new_err(
                "$.player.class: could not infer character class from state (no card id \
                 with known character prefix). Pass character= explicitly.",
            )
        })?,
    };
    let (mut wire, warnings) =
        sts2_workbench::combat_import::combat_wire_from_bridge(&board, character, ascension)
            .map_err(|e| UnsupportedStateError::new_err(e.0))?;
    wire["combat_id"] = serde_json::json!(combat_id);
    wire["rng"]["seed"] = serde_json::json!(seed);
    Ok((crate::value_to_py(py, &wire)?, warnings))
}

/// `sts2_hint.adapter.registry_card_id`: the canonical engine registry id for
/// a bridge / headless card id.
///
/// # Errors
///
/// `UnsupportedStateError` when no candidate is in the registry.
#[pyfunction]
#[pyo3(signature = (card_id, path="$"))]
pub fn registry_card_id(card_id: &str, path: &str) -> PyResult<String> {
    sts2_workbench::combat_import::registry_card_id(card_id, path)
        .map_err(|e| UnsupportedStateError::new_err(e.0))
}
