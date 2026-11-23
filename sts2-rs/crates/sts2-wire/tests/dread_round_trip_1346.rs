//! GH #1346: a Dread fight's game mode and dominion survive the wire as
//! `$.game_mode` / `$.dominion`, both absent on a standard board.

use sts2_core::{Dominion, GameMode};
use sts2_wire::emit::combat_state_to_value;
use sts2_wire::state_from_json;

fn fixture() -> serde_json::Value {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/combat_action_ironclad.json"),
    )
    .expect("fixture");
    serde_json::from_str(&text).expect("fixture parses")
}

#[test]
fn a_standard_board_carries_neither() {
    let st = state_from_json(&fixture()).expect("builds");
    assert_eq!((st.game_mode, st.dominion), (GameMode::Standard, None));
    let v = combat_state_to_value(&st).expect("emits");
    assert!(v.get("game_mode").is_none() && v.get("dominion").is_none());
}

#[test]
fn game_mode_and_dominion_round_trip() {
    let mut v = fixture();
    v["game_version"] = serde_json::json!("v0.113.0");
    v["game_mode"] = serde_json::json!("dread");
    v["dominion"] = serde_json::json!("QUEEN_DOMINION");
    let st = state_from_json(&v).expect("builds");
    assert_eq!(
        (st.game_mode, st.dominion),
        (GameMode::Dread, Some(Dominion::Queen))
    );
    let out = combat_state_to_value(&st).expect("emits");
    assert_eq!(out["game_mode"], "dread");
    assert_eq!(out["dominion"], "QUEEN_DOMINION");
    let back = state_from_json(&out).expect("re-parses");
    assert_eq!(
        (back.game_mode, back.dominion),
        (GameMode::Dread, Some(Dominion::Queen))
    );
}

/// The act's boss fight: Dread, no dominion.
#[test]
fn a_dread_boss_fight_has_no_dominion() {
    let mut v = fixture();
    v["game_version"] = serde_json::json!("v0.113.0");
    v["game_mode"] = serde_json::json!("dread");
    let st = state_from_json(&v).expect("builds");
    assert_eq!((st.game_mode, st.dominion), (GameMode::Dread, None));
    let out = combat_state_to_value(&st).expect("emits");
    assert_eq!(out["game_mode"], "dread");
    assert!(out.get("dominion").is_none());
}

#[test]
fn unknown_names_are_refused() {
    for (key, bad) in [("game_mode", "blitzkrieg"), ("dominion", "NOT_A_DOMINION")] {
        let mut v = fixture();
        v[key] = serde_json::json!(bad);
        assert!(state_from_json(&v).is_err(), "{key}={bad}");
    }
}

/// The two per-combat notes `WaterfallGiantDominion` leaves
/// (`WaterfallGiantDominion.cs:44-59`): the Explosive amount of the entry
/// roster (`$.dread_explosive`) and the encounter's `IsWeak`
/// (`$.dread_weak_encounter`). Both are state a summon reads, so both ride
/// the wire, and only when set.
#[test]
fn the_waterfall_giant_notes_round_trip() {
    let mut v = fixture();
    v["game_version"] = serde_json::json!("v0.113.0");
    v["game_mode"] = serde_json::json!("dread");
    v["dominion"] = serde_json::json!("WATERFALL_GIANT_DOMINION");
    let plain = state_from_json(&v).expect("builds");
    assert_eq!(plain.combat_log.dread_explosive, 0);
    assert!(!plain.combat_log.dread_weak_encounter);
    let out = combat_state_to_value(&plain).expect("emits");
    assert!(out.get("dread_explosive").is_none());
    assert!(out.get("dread_weak_encounter").is_none());

    v["dread_explosive"] = serde_json::json!(7);
    v["dread_weak_encounter"] = serde_json::json!(true);
    let st = state_from_json(&v).expect("builds");
    assert_eq!(st.combat_log.dread_explosive, 7);
    assert!(st.combat_log.dread_weak_encounter);
    let out = combat_state_to_value(&st).expect("emits");
    assert_eq!(out["dread_explosive"], 7);
    assert_eq!(out["dread_weak_encounter"], true);
    let back = state_from_json(&out).expect("re-parses");
    assert_eq!(back.combat_log.dread_explosive, 7);
    assert!(back.combat_log.dread_weak_encounter);
    // The notes are in the digest, and the rebuilt board agrees with itself
    // across another trip. (`back` is compared with its own rebuild, not
    // with `st`: this fixture's first build does not hash like its
    // re-emitted form even as a standard board.)
    let again = state_from_json(&combat_state_to_value(&back).expect("emits")).expect("re-parses");
    assert_eq!(
        sts2_core::hashing::search_hash(&again),
        sts2_core::hashing::search_hash(&back)
    );
    let mut cleared = back.clone();
    cleared.combat_log.dread_explosive = 0;
    cleared.combat_log.dread_weak_encounter = false;
    assert_ne!(
        sts2_core::hashing::search_hash(&cleared),
        sts2_core::hashing::search_hash(&back)
    );

    // A standard board never carries them.
    let standard = state_from_json(&fixture()).expect("builds");
    let out = combat_state_to_value(&standard).expect("emits");
    assert!(out.get("dread_explosive").is_none());
    assert!(out.get("dread_weak_encounter").is_none());

    // Malformed values are refused.
    for (key, bad) in [
        ("dread_explosive", serde_json::json!("seven")),
        ("dread_explosive", serde_json::json!(-1)),
        ("dread_explosive", serde_json::json!(300)),
        ("dread_weak_encounter", serde_json::json!("yes")),
    ] {
        let mut v = fixture();
        v[key] = bad.clone();
        assert!(state_from_json(&v).is_err(), "{key}={bad}");
    }
}

// ───────────── the Dread bosses' move tables survive the wire ─────────────

mod dread_bosses {
    use sts2_content::monster_dispatch::MonsterSpawnArgs;
    use sts2_content::run_bridge::enter_combat_from_roster;
    use sts2_content::run_callbacks::{initialize_run_setup, new_run_manager_for_character};
    use sts2_core::monster_ai::MonsterState;
    use sts2_core::monsters::Intent;
    use sts2_core::runstate::RoomType;
    use sts2_core::{CharacterId, CombatState, GameMode, GameVersion, MonsterKind};
    use sts2_wire::emit::combat_state_to_value;
    use sts2_wire::state_from_json;

    /// `kinds` as the boss room of an ascension-10 v0.113.0 run in `mode`.
    fn fight(mode: GameMode, kinds: &[MonsterKind]) -> CombatState {
        let mut rm = new_run_manager_for_character(7, CharacterId::Ironclad)
            .with_game_version(GameVersion::V0_113_0);
        rm.run_state.ascension_level = 10;
        rm.run_state.game_mode = mode;
        initialize_run_setup(&mut rm);
        let roster: Vec<_> = kinds
            .iter()
            .map(|&kind| (kind, MonsterSpawnArgs::None, 400))
            .collect();
        enter_combat_from_roster(&rm.run_state, &roster, RoomType::Boss)
    }

    fn round_trip(state: &CombatState) -> CombatState {
        let v = combat_state_to_value(state).expect("emits");
        state_from_json(&v).expect("re-parses")
    }

    fn ai(state: &CombatState, index: usize) -> &sts2_core::monster_ai::MonsterAi {
        let uid = state.enemy_uids[index];
        state
            .creatures
            .iter()
            .find(|c| c.id == uid.0)
            .and_then(|c| c.ai.as_ref())
            .expect("ai")
    }

    fn first_intent(state: &CombatState, index: usize, id: &str) -> Intent {
        match ai(state, index).states.get(id) {
            Some(MonsterState::Move(m)) => m.intents[0],
            _ => panic!("no move {id}"),
        }
    }

    /// `Queen.cs:168`: a Dread Queen sits on `DREAD_INITIAL_MOVE`, a state
    /// only the Dread machine has; and `TorchHeadAmalgam.cs:32`: her Amalgam's
    /// STRONG_TACKLE is 37 (standard A10 32). A board rebuilt from the wire
    /// carries the Dread machine for both, a standard one the standard.
    #[test]
    fn a_rebuilt_dread_boss_keeps_its_dread_moves() {
        let roster = [MonsterKind::TorchHeadAmalgam, MonsterKind::Queen];
        for (mode, opener, tackle) in [
            (GameMode::Standard, "PUPPET_STRINGS_MOVE", 32),
            (GameMode::Dread, "DREAD_INITIAL_MOVE", 37),
        ] {
            let state = fight(mode, &roster);
            let back = round_trip(&state);
            assert_eq!(back.game_mode, mode);
            assert_eq!(ai(&back, 1).current_move().id, opener, "{mode}");
            assert_eq!(
                first_intent(&back, 0, "STRONG_TACKLE"),
                Intent::Attack { damage: tackle },
                "{mode}"
            );
            assert_eq!(
                ai(&back, 1).states.contains_key("DREAD_INITIAL_MOVE"),
                mode.is_dread()
            );
        }
    }

    /// `Rocket.cs:62`: ROCKET_CLAW is 40 in Dread (standard A10 35). The
    /// Surrounded facing is inferred by matching each claw's telegraph with
    /// its move table, so the table must be the Dread one: with the Rocket
    /// telegraphing ROCKET_CLAW the rebuilt board keeps the facing and the
    /// damage the live one had.
    #[test]
    fn a_rebuilt_dread_kaiser_crab_keeps_its_facing() {
        use sts2_core::hooks::{apply_effects, Effect};
        let mut state = fight(
            GameMode::Dread,
            &[MonsterKind::Crusher, MonsterKind::Rocket],
        );
        let rocket = state.enemy_uids[1];
        apply_effects(
            &mut state,
            &[Effect::ForceMonsterMove {
                target: rocket,
                state_id: "ROCKET_CLAW_MOVE",
            }],
        );
        let back = round_trip(&state);
        assert_eq!(
            first_intent(&back, 1, "ROCKET_CLAW_MOVE"),
            Intent::Attack { damage: 40 }
        );
        assert_eq!(ai(&back, 1).current_move().id, "ROCKET_CLAW_MOVE");
        let facing = |s: &CombatState| {
            s.player_power_uids()
                .iter()
                .filter_map(|p| s.powers.get_by_id(p.0))
                .find(|p| p.kind == sts2_core::PowerKind::Surrounded)
                .map(|p| p.counter)
        };
        assert_eq!(facing(&back), facing(&state));
        let mut live = state.clone();
        let mut rebuilt = back;
        let _ = sts2_core::apply(&mut live, sts2_core::Action::EndTurn);
        let _ = sts2_core::apply(&mut rebuilt, sts2_core::Action::EndTurn);
        assert_eq!(rebuilt.player.current_hp, live.player.current_hp);
    }
}
