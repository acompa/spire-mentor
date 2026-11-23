//! A live board as the engine's fight (GH #1426; was
//! `realgame_adapter.combat_wire` / `run_ascension`).

use serde_json::{json, Value};

use crate::py::{self, get};

/// Why a board has no engine fight. Each names the Python exception it
/// was, which the advisor's declines quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardError {
    /// A build outside the supported set (`UnsupportedGameVersionError`).
    GameVersion(String),
    /// A board the engine cannot model (`UnsupportedStateError`).
    Unsupported(String),
    /// An ascension the engine's `u8` cannot hold (`OverflowError`).
    Overflow(String),
}

impl BoardError {
    /// The Python exception's class name.
    #[must_use]
    pub const fn py_type(&self) -> &'static str {
        match self {
            Self::GameVersion(_) => "UnsupportedGameVersionError",
            Self::Unsupported(_) => "UnsupportedStateError",
            Self::Overflow(_) => "OverflowError",
        }
    }
}

impl std::fmt::Display for BoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GameVersion(m) | Self::Unsupported(m) | Self::Overflow(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for BoardError {}

/// `run_ascension`: the mod's `run_state.ascension`, else `default`.
#[must_use]
pub fn run_ascension(run_state: &Value, default: i64) -> i64 {
    match get(run_state, "ascension") {
        Value::Null => default,
        v => py::int_of(v),
    }
}

/// The answer path's board import (`realgame_advisor_mirror._wire_for`):
/// [`combat_wire`] as a [`crate::advisor_mirror::WireFor`], its error as
/// `(python exception type, message)`.
///
/// # Errors
///
/// As [`combat_wire`].
pub fn wire_for(
    combat: &Value,
    character: Option<&str>,
    ascension: i64,
    combat_id: i64,
    seed: u64,
) -> Result<Value, (String, String)> {
    combat_wire(combat, character, ascension, combat_id, seed)
        .map(|(wire, _warnings)| wire)
        .map_err(|e| (e.py_type().to_string(), e.to_string()))
}

/// `combat_wire`: a bridge `combat_action` board as the engine wire dict,
/// plus the warnings for everything dropped on the way. `character` is the
/// display name (`"Ironclad"`); `None` infers it from the card ids.
///
/// # Errors
///
/// An unsupported build (checked first), a board the engine cannot model,
/// no character to build it for.
pub fn combat_wire(
    state: &Value,
    character: Option<&str>,
    ascension: i64,
    combat_id: i64,
    seed: u64,
) -> Result<(Value, Vec<String>), BoardError> {
    crate::headless::game_version(state).map_err(|e| BoardError::GameVersion(e.to_string()))?;
    let ascension = u8::try_from(ascension)
        .map_err(|_| BoardError::Overflow(format!("can't convert {ascension} to u8")))?;
    let character = match character {
        Some(c) => c,
        None => sts2_workbench::combat_import::infer_character(state).ok_or_else(|| {
            BoardError::Unsupported(
                "$.player.class: could not infer character class from state (no card id \
                 with known character prefix). Pass character= explicitly."
                    .into(),
            )
        })?,
    };
    let (mut wire, warnings) =
        sts2_workbench::combat_import::combat_wire_from_bridge(state, character, ascension)
            .map_err(|e| BoardError::Unsupported(e.0))?;
    wire["combat_id"] = json!(combat_id);
    wire["rng"]["seed"] = json!(seed);
    Ok((wire, warnings))
}
