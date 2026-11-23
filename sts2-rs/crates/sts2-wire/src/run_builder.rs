//! Building a `RunManager` the way every host asks for one (GH #1426): the
//! one constructor `sts2_rs.RunManager(...)` and the native advisor share.
//! Each knob is the Python keyword of the same name.

use sts2_content::run_callbacks::{
    new_ironclad_run_manager_with_deck_and_hp, new_run_manager_for_character,
    new_run_manager_for_character_from_string_seed, new_run_manager_for_character_with_ascension,
};
use sts2_core::act_selection::ActVariant;
use sts2_core::cards::CardId;
use sts2_core::potions::CharacterId;
use sts2_core::run_manager::RunManager;

use crate::emit::names as sts2_wire_names;

/// `sts2_rs.RunManager(...)`'s keyword arguments, defaults included.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // one flag per Python keyword
pub struct RunOptions<'a> {
    pub seed: i32,
    pub character: &'a str,
    pub deck: Option<Vec<String>>,
    pub max_hp: Option<i32>,
    pub string_seed: Option<&'a str>,
    pub ascension_level: u8,
    pub neow_start: bool,
    pub interactive_card_removal: Option<bool>,
    pub direct_event_relic_grant: Option<bool>,
    pub interactive_card_upgrade: Option<bool>,
    pub interactive_card_transform: Option<bool>,
    pub interactive_potion_swap: bool,
    /// The potion popup (GH #1606): `RunState::ooc_potion_actions`.
    pub ooc_potion_actions: bool,
    /// The game's deck select screens (GH #1619, #1621):
    /// `RunState::deck_select_screens`.
    pub deck_select_screens: bool,
    /// The game's reward screen (GH #1620): `RunState::reward_screen`.
    pub reward_screen: bool,
    pub act1: &'a str,
    pub game_version: Option<&'a str>,
    /// The deck-decision bundle: `"parity"` / `"agent"` (GH #981).
    pub mode: Option<&'a str>,
    /// The run's game mode: `"standard"` / `"dread"` / `"blitz"` (GH #1346).
    pub game_mode: Option<&'a str>,
}

impl Default for RunOptions<'_> {
    fn default() -> Self {
        Self {
            seed: 0,
            character: "Ironclad",
            deck: None,
            max_hp: None,
            string_seed: None,
            ascension_level: 0,
            neow_start: false,
            interactive_card_removal: None,
            direct_event_relic_grant: None,
            interactive_card_upgrade: None,
            interactive_card_transform: None,
            interactive_potion_swap: false,
            ooc_potion_actions: false,
            deck_select_screens: false,
            reward_screen: false,
            act1: "roll",
            game_version: None,
            mode: None,
            game_mode: None,
        }
    }
}

/// A card id as the run constructor's `deck` takes it: `CARD.STRIKE` (the
/// sts2log / scraped `.run` shape) or `STRIKE`; `names::card_id_from_str`
/// also tolerates the `_CARD` suffix of some sts2log entries.
fn card_id_from_str(s: &str) -> Result<CardId, String> {
    let stripped = s.strip_prefix("CARD.").unwrap_or(s);
    sts2_content::names::card_id_from_str(stripped).ok_or_else(|| format!("unknown card id {s:?}"))
}

/// Build the run `o` describes: `(run, effective_seed)`, the seed being the
/// i32 view of the run's master seed.
///
/// # Errors
///
/// A knob the engine refuses, with the message `sts2_rs` raises as
/// `ValueError`.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
pub fn build_run(o: &RunOptions<'_>) -> Result<(RunManager, i32), String> {
    let RunOptions {
        seed,
        character,
        ref deck,
        max_hp,
        string_seed,
        ascension_level,
        neow_start,
        interactive_card_removal,
        direct_event_relic_grant,
        interactive_card_upgrade,
        interactive_card_transform,
        interactive_potion_swap,
        ooc_potion_actions,
        deck_select_screens,
        reward_screen,
        act1,
        game_version,
        mode,
        game_mode,
    } = *o;
    // GH #981: `mode` names the whole deck-decision bundle at once, so a
    // host cannot turn on three flags and forget the fourth. It and the
    // individual flags are mutually exclusive — mixing them would make
    // "which one wins" a silent rule.
    let bundle = [
        ("interactive_card_removal", interactive_card_removal),
        ("interactive_card_upgrade", interactive_card_upgrade),
        ("interactive_card_transform", interactive_card_transform),
        ("direct_event_relic_grant", direct_event_relic_grant),
    ];
    let agent_mode = match mode {
        None => None,
        Some(m @ ("parity" | "agent")) => {
            if let Some((name, _)) = bundle.iter().find(|(_, v)| v.is_some()) {
                return Err(format!(
                    "pass either mode={m:?} or {name}=…, not both: the mode \
                     sets the whole deck-decision bundle"
                ));
            }
            Some(m == "agent")
        }
        Some(other) => {
            return Err(format!(
                "unknown mode {other:?} (expected \"parity\" or \"agent\")"
            ))
        }
    };
    let cid = sts2_wire_names::character_from_str(character)?;
    let game_version = match game_version {
        None => sts2_core::GameVersion::default(),
        Some(s) => s
            .parse::<sts2_core::GameVersion>()
            .map_err(|e| e.to_string())?,
    };
    // GH #1346: the run's game mode (`mode` above is the deck-decision
    // bundle, GH #981). Dread is always an ascension-10 run on v0.113.0
    // or later: the game's Dread screen embarks with the literal 10
    // (`NDreadScreen.cs:584`), so the requested level is overridden, as
    // the C# headless host does.
    let game_mode = match game_mode {
        None => sts2_core::GameMode::Standard,
        Some(raw) => sts2_core::GameMode::parse(raw).ok_or_else(|| {
            format!("unknown game_mode {raw:?} (expected \"standard\", \"dread\" or \"blitz\")")
        })?,
    };
    if game_version < game_mode.min_game_version() {
        return Err(format!(
            "game_mode \"{game_mode}\" needs game_version {} or later (got {game_version})",
            game_mode.min_game_version()
        ));
    }
    let ascension_level = game_mode.forced_ascension().unwrap_or(ascension_level);
    // GH #1347: in Blitz `ascension_level` is the Blitz level 0..=5
    // (`AscensionManager.maxAscensionAllowedBlitz`), a set of standard
    // levels rather than a threshold; the C# host refuses a higher one.
    if game_mode.is_blitz() && ascension_level > game_mode.max_ascension() {
        return Err(format!(
            "game_mode \"blitz\": ascension_level is the Blitz level 0..={} (got {ascension_level})",
            game_mode.max_ascension()
        ));
    }
    // Bead `sts2-rs-livck`: Ironclad / Silent / Necrobinder / Regent
    // / Defect now route through the character-parameterised
    // factories in `sts2-content::run_callbacks`. Defect's
    // starter deck currently falls back to the Ironclad starter
    // deck because the Defect starter-card factory isn't ported
    // yet — runs will succeed but the deck won't match the C# pin
    // until `starters::starter_deck_for(Defect)` lands real content.
    // When `string_seed` is provided, it takes precedence — this is
    // the C#-game-style 10-char alphanumeric seed (e.g. "3ACCFREXKR")
    // as emitted by scraped .run files. We canonicalise + hash to the
    // same i32 the C# game uses, ignoring any explicit `seed` arg.
    //
    // GH #788: the hash is the game build's `RunRngSet` rule — the old
    // 32-bit hash in v0.107.1, XxHash64 in v0.111.0 (whose 64-bit master
    // is applied below via `with_run_seed`; `effective_seed` is its i32
    // view). An integer `seed` is a `uint` master in both builds.
    let master_seed: u64 = match string_seed {
        Some(s) => game_version.run_seed_hash(&sts2_core::canonicalize_seed(s)),
        None => u64::from(seed as u32),
    };
    let effective_seed = master_seed as i32;
    let inner = match deck.as_deref() {
        None => {
            // When `ascension_level > 0`, route through the ascension
            // factory so A3 gold, A4 potion-slot, A5 AscendersBane,
            // A7 Scarcity (etc.) are applied. String-seed canonicalisation
            // is preserved by passing the already-hashed `effective_seed`.
            let mut rm = if ascension_level > 0 {
                new_run_manager_for_character_with_ascension(effective_seed, cid, ascension_level)
            } else {
                match string_seed {
                    Some(s) => new_run_manager_for_character_from_string_seed(s, cid),
                    None => new_run_manager_for_character(effective_seed, cid),
                }
            };
            if let Some(hp) = max_hp {
                rm.run_state.max_hp = hp;
                rm.run_state.current_hp = hp;
            }
            rm
        }
        Some(d) => {
            if ascension_level > 0 {
                return Err(String::from(
                    "ascension_level + custom deck override is not yet supported",
                ));
            }
            if !matches!(cid, CharacterId::Ironclad) {
                return Err(format!(
                    "custom `deck` override is currently Ironclad-only \
                     (factory `new_ironclad_run_manager_with_deck_and_hp` \
                     hardcodes the character); character {character:?} \
                     must use the starter-deck path. Bead sts2-rs-livck \
                     follow-up."
                ));
            }
            let cards = d
                .iter()
                .map(|s| card_id_from_str(s))
                .collect::<Result<Vec<_>, String>>()?;
            new_ironclad_run_manager_with_deck_and_hp(effective_seed, cards, max_hp)
        }
    };
    // GH #787: the game build, threaded like ascension. Must precede
    // Neow-start / act-1 selection / run setup — everything the run
    // rolls from here on may depend on it. A no-op at the default.
    let inner = inner
        .with_game_version(game_version)
        .with_game_mode(game_mode)
        .with_run_seed(master_seed);
    // Bead `sts2-rs-g24zv`: opt-in Neow-start. When `neow_start=True`,
    // the run lands in `Phase::Event` with `current_event=Neow` and
    // the 3-relic offers cached on `run_state.neow_offers`. Required
    // for corpus replay (every scraped v0.103.2 run begins at
    // `first_room_type=ancient`).
    let mut inner = if neow_start {
        inner.with_neow_start()
    } else {
        inner
    };
    // Opt-in interactive deck removal: on-obtain card-removal relics
    // (Precise Scissors, …) present a player card_select instead of the
    // default front-of-deck auto-removal. The workbench sets this for
    // C#-faithful Neow; default off keeps the parity-fuzz behavior.
    inner.run_state.interactive_card_removal = interactive_card_removal.unwrap_or(false);
    // Opt-in interactive deck upgrade: on-obtain card-upgrade relics
    // (Pomander) present a player card_select instead of the default
    // random rewards-stream auto-upgrade, matching C#
    // `Pomander.AfterObtained` → `CardSelectCmd.FromDeckForUpgrade`. The
    // workbench / `run_parity` lockstep set this; default off keeps the
    // parity-fuzz behavior. Bead `sts2-rs-1hiht`.
    inner.run_state.interactive_card_upgrade = interactive_card_upgrade.unwrap_or(false);
    // Opt-in interactive deck transform: on-obtain card-transform relics
    // (NewLeaf) present a player card_select whose replacement is rolled
    // off `Rng.Niche` only after the pick, matching C#
    // `NewLeaf.AfterObtained` → `CardSelectCmd.FromDeckForTransformation`
    // → `CardCmd.TransformToRandom`. Bead 6ynt.4.deck-add / GH #34.
    inner.run_state.interactive_card_transform = interactive_card_transform.unwrap_or(false);
    // Opt-in interactive full-belt potion swap: a potion reward landing
    // on a full belt surfaces a `potion_swap` discard-to-claim decision
    // instead of being silently dropped, matching C#'s TooFull reward +
    // `DiscardPotionGameAction` flow. Default off keeps the historical
    // (and headless-oracle) silent drop. GH #252.
    inner.run_state.interactive_potion_swap = interactive_potion_swap;
    // The potion popup (use out of combat, discard): off by default, the
    // parity mode. GH #1606.
    inner.run_state.ooc_potion_actions = ooc_potion_actions;
    // Cancelable Smith / Cook and pick-N deck selects as one select: off by
    // default, the parity mode. GH #1619, #1621.
    inner.run_state.deck_select_screens = deck_select_screens;
    // Every reward of a set taken or left on its own: off by default, the
    // parity mode. GH #1620.
    inner.run_state.reward_screen = reward_screen;
    // Grant Neow blessing relics directly (no extra Take relic/Skip
    // confirmation), matching C#. See `RunState::direct_event_relic_grant`.
    inner.run_state.direct_event_relic_grant = direct_event_relic_grant.unwrap_or(false);
    // `mode="agent"` is `RunState::set_agent_decisions` — the same one
    // definition the arena and the workbench use. `mode="parity"` (and
    // no mode at all) leaves the bundle off, the headless-C# behaviour
    // the parity suite pins.
    if agent_mode == Some(true) {
        inner.run_state.set_agent_decisions();
    }
    // GH #544: which of the two Act-1 models this run walks. Must be set
    // BEFORE `initialize_run_setup`, which pre-generates every act's
    // rooms off `up_front`. Defaults to `"roll"`, the real game's
    // seed-driven pick with everything unlocked and discovered; the C#
    // host defaults the same way, so a parity pair that passes neither
    // still agrees. `"overgrowth"` / `"underdocks"` pin a variant (the
    // default was `"overgrowth"` before this change).
    let mut inner = match act1 {
        "overgrowth" => inner.with_act1(ActVariant::Overgrowth),
        "underdocks" => inner.with_act1(ActVariant::Underdocks),
        "roll" => inner.with_rolled_act1(),
        other => {
            return Err(format!(
                "unknown act1 {other:?} (expected \"overgrowth\", \"underdocks\" or \"roll\")"
            ))
        }
    };
    // C#-faithful run setup: populate the relic grab bags (player +
    // shared) and pre-roll all acts' room contents off the `up_front`
    // stream, then re-pin act 1's boss to the generated identity. The bare
    // run factories omit this; every full-run driver (this binding, the
    // run-AI arena / labeler / exporter / calibrator) calls the one shared
    // function so they cannot diverge. See `initialize_run_setup` for the
    // verified `up_front` read order (beads mcesz / 7e04v / jenwp).
    sts2_content::run_callbacks::initialize_run_setup(&mut inner);
    Ok((inner, effective_seed))
}
