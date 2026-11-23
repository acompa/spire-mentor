//! v0.113.0 Dread mode (GH #1346): every `AscensionHelper.IsDread()` branch
//! of the bosses (`Models.Monsters/*.cs`, `Models.Encounters/TheKinBoss.cs`).
//!
//! A Dread run is an ascension-10 run, so each test compares the Dread value
//! with the standard ascension-10 value of the same build, and asserts the
//! standard one did not move. Combats are entered the run's way
//! (`run_bridge` / `run_callbacks`), so the game mode the run carries is
//! what selects the values.

use smallvec::smallvec;

use sts2_content::monster_dispatch::{build_spawned_ai, MonsterSpawnArgs};
use sts2_content::run_bridge::enter_combat_from_roster;
use sts2_content::run_callbacks::{
    dump_roster_for_encounter, enter_combat_for_encounter, initialize_run_setup,
    new_run_manager_for_character,
};
use sts2_core::cards::CardId;
use sts2_core::hooks::{apply_effects, Effect};
use sts2_core::monster_ai::MonsterState;
use sts2_core::monsters::Intent;
use sts2_core::runstate::{RoomType, RunState};
use sts2_core::{
    apply, Action, CharacterId, CombatState, CreatureUid, GameMode, GameVersion, MonsterKind,
    PowerKind,
};

const V113: GameVersion = GameVersion::V0_113_0;

// ───────────────────────────── fixtures ─────────────────────────────

/// An ascension-10 v0.113.0 Ironclad run in `mode`, with enough HP to sit
/// through any number of boss turns.
fn run(mode: GameMode) -> RunState {
    let mut rm = new_run_manager_for_character(7, CharacterId::Ironclad).with_game_version(V113);
    rm.run_state.ascension_level = 10;
    rm.run_state.game_mode = mode;
    initialize_run_setup(&mut rm);
    let mut rs = rm.run_state;
    rs.max_hp = 5000;
    rs.current_hp = 5000;
    rs
}

/// `kinds` entered as a boss room of a `mode` run, 400 HP each.
fn fight(mode: GameMode, kinds: &[MonsterKind]) -> CombatState {
    let roster: Vec<_> = kinds
        .iter()
        .map(|&kind| (kind, MonsterSpawnArgs::None, 400))
        .collect();
    let state = enter_combat_from_roster(&run(mode), &roster, RoomType::Boss);
    assert_eq!((state.game_mode, state.ascension), (mode, 10));
    state
}

/// `f` in a standard run and in a Dread run: `(standard, dread)`.
fn both<T>(mut f: impl FnMut(GameMode) -> T) -> (T, T) {
    (f(GameMode::Standard), f(GameMode::Dread))
}

fn enemy(state: &CombatState, index: usize) -> &sts2_core::Creature {
    let uid = state.enemy_uids[index];
    state
        .creatures
        .iter()
        .find(|c| c.id == uid.0)
        .expect("enemy")
}

fn powers_of(state: &CombatState, uid: CreatureUid) -> Vec<(PowerKind, i32)> {
    let c = state
        .creatures
        .iter()
        .find(|c| c.id == uid.0)
        .expect("creature");
    c.powers
        .iter()
        .filter_map(|p| state.powers.get_by_id(p.0))
        .map(|p| (p.kind, p.amount))
        .collect()
}

fn enemy_powers(state: &CombatState, index: usize) -> Vec<(PowerKind, i32)> {
    powers_of(state, state.enemy_uids[index])
}

fn enemy_power(state: &CombatState, index: usize, kind: PowerKind) -> i32 {
    enemy_powers(state, index)
        .iter()
        .filter(|p| p.0 == kind)
        .map(|p| p.1)
        .sum()
}

fn player_power(state: &CombatState, kind: PowerKind) -> i32 {
    state
        .player_power_uids()
        .iter()
        .filter_map(|p| state.powers.get_by_id(p.0))
        .filter(|p| p.kind == kind)
        .map(|p| p.amount)
        .sum()
}

/// The intents of move `id` of enemy `index`.
fn intents(state: &CombatState, index: usize, id: &str) -> Vec<Intent> {
    let ai = enemy(state, index).ai.as_ref().expect("ai");
    match ai.states.get(id) {
        Some(MonsterState::Move(m)) => m.intents.to_vec(),
        _ => panic!("no move {id}"),
    }
}

/// The attack `(damage, hits)` move `id` of enemy `index` telegraphs.
fn attack(state: &CombatState, index: usize, id: &str) -> (i32, u8) {
    intents(state, index, id)
        .iter()
        .find_map(|i| match i {
            Intent::Attack { damage } => Some((*damage, 1)),
            Intent::MultiAttack { damage, hits } => Some((*damage, *hits)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{id} has no attack"))
}

/// The amount of the `kind` buff or debuff move `id` of enemy `index`
/// telegraphs.
fn telegraphed(state: &CombatState, index: usize, id: &str, kind: PowerKind) -> i32 {
    intents(state, index, id)
        .iter()
        .find_map(|i| match i {
            Intent::Buff { kind: k, amount } | Intent::Debuff { kind: k, amount } if *k == kind => {
                Some(*amount)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{id} telegraphs no {kind:?}"))
}

fn current_move(state: &CombatState, index: usize) -> &'static str {
    enemy(state, index)
        .ai
        .as_ref()
        .expect("ai")
        .current_move()
        .id
}

/// Make `id` the next move of enemy `index` (C# `SetMoveImmediate`).
fn force(state: &mut CombatState, index: usize, id: &'static str) {
    let target = state.enemy_uids[index];
    apply_effects(
        state,
        &[Effect::ForceMonsterMove {
            target,
            state_id: id,
        }],
    );
    assert_eq!(current_move(state, index), id);
}

fn end_turn(state: &mut CombatState) {
    let _ = apply(state, Action::EndTurn);
}

/// Force `id` on enemy `index` and let the enemy side play its turn.
fn perform(state: &mut CombatState, index: usize, id: &'static str) {
    force(state, index, id);
    end_turn(state);
}

fn count_in(state: &CombatState, pile: &[sts2_core::CardUid], id: CardId) -> usize {
    pile.iter()
        .filter(|u| state.cards[u.0.get() as usize - 1].card_id == id)
        .count()
}

/// `(hand, draw, discard)` copies of card `id`.
fn spread(state: &CombatState, id: CardId) -> (usize, usize, usize) {
    (
        count_in(state, &state.player.hand, id),
        count_in(state, &state.player.draw, id),
        count_in(state, &state.player.discard, id),
    )
}

/// `(monster id, max hp)` of every member of `encounter` as the run rolls it.
fn roster_hp(mode: GameMode, encounter: &str) -> Vec<(&'static str, i32)> {
    dump_roster_for_encounter(&mut run(mode), encounter)
        .expect("encounter")
        .into_iter()
        .map(|(name, max_hp, hp)| {
            assert_eq!(max_hp, hp);
            (name, max_hp)
        })
        .collect()
}

// ───────────────────────────── act 1, Overgrowth ─────────────────────────────

/// `Vantom.cs:62`, `:104`: `SlipperyAmt` = `(ToughEnemies, 9, 8)` + 2 in
/// Dread, applied in `AfterAddedToRoom`. Standard A10 9, Dread 11.
#[test]
fn vantom_starts_with_two_more_slippery() {
    let got = both(|mode| enemy_powers(&fight(mode, &[MonsterKind::Vantom]), 0));
    assert_eq!(got.0, [(PowerKind::Slippery, 9)]);
    assert_eq!(got.1, [(PowerKind::Slippery, 11)]);
}

/// `Vantom.cs:72-82`, `:205`: PREPARE's self Strength. Standard A10 2,
/// Dread 3, in the telegraph and in the move.
#[test]
fn vantom_prepare_gains_three_strength() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::Vantom]);
        let shown = telegraphed(&state, 0, "PREPARE", PowerKind::Strength);
        perform(&mut state, 0, "PREPARE");
        (shown, enemy_power(&state, 0, PowerKind::Strength))
    });
    assert_eq!(got, ((2, 2), (3, 3)));
}

/// `CeremonialBeast.cs:55`: `MinInitialHp` = `(ToughEnemies, 262, 252)` + 30
/// in Dread. Standard A10 262, Dread 292.
#[test]
fn ceremonial_beast_has_thirty_more_hp() {
    let got = both(|mode| roster_hp(mode, "CEREMONIAL_BEAST_BOSS"));
    assert_eq!(got.0, [("CeremonialBeast", 262)]);
    assert_eq!(got.1, [("CeremonialBeast", 292)]);
}

/// `CeremonialBeast.cs:63-73`, `:227-230`: the Strength PLOW gains while
/// `PlowPower` is up. Standard A10 2, Dread 3.
#[test]
fn ceremonial_beast_plow_gains_three_strength() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::CeremonialBeast]);
        let shown = telegraphed(&state, 0, "PLOW", PowerKind::Strength);
        end_turn(&mut state); // STAMP: PlowPower
        assert!(enemy_power(&state, 0, PowerKind::Plow) > 0);
        end_turn(&mut state); // PLOW
        (shown, enemy_power(&state, 0, PowerKind::Strength))
    });
    assert_eq!(got, ((2, 2), (3, 3)));
}

/// `CeremonialBeast.cs:75-85`, `:233-243`: `SetStunned` (the plow broken by
/// unblocked damage, `PlowPower.cs:28-47`) gives the beast `RitualPower`
/// `RitualAmount`: standard none, Dread 2. It lands after the Strength strip,
/// on the player's turn, so the stunned turn's end already pays 2 Strength.
#[test]
fn ceremonial_beast_gains_ritual_when_its_plow_breaks() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::CeremonialBeast]);
        end_turn(&mut state); // STAMP
        end_turn(&mut state); // PLOW
        let plow = enemy_power(&state, 0, PowerKind::Plow);
        assert_eq!(plow, 160, "PlowAmount at A10");
        let uid = state.enemy_uids[0];
        let beast = state
            .creatures
            .iter_mut()
            .find(|c| c.id == uid.0)
            .expect("beast");
        beast.current_hp = plow + 1;
        beast.block = 0;
        let strike = state
            .player
            .hand
            .iter()
            .position(|u| state.cards[u.0.get() as usize - 1].card_id == CardId::StrikeIronclad)
            .expect("a Strike in hand");
        let _ = apply(
            &mut state,
            Action::PlayCard {
                hand_index: u8::try_from(strike).expect("hand index"),
                target: Some(0),
            },
        );
        assert_eq!(current_move(&state, 0), "STUN");
        let at_stun = enemy_powers(&state, 0);
        end_turn(&mut state); // the stunned turn
        (at_stun, enemy_power(&state, 0, PowerKind::Strength))
    });
    assert_eq!(got.0, (vec![], 0));
    assert_eq!(got.1, (vec![(PowerKind::Ritual, 2)], 2));
}

/// `TheKinBoss.cs:28-113`, `KinFollower.cs:104-122`: Dread adds a third
/// `KinFollower` (`Index` 2, `slot3`, opening on BOOMERANG) ahead of the
/// priest. Standard A10: followers `Index` 0 (POWER_DANCE) and 1
/// (QUICK_SLASH) in `slot1`, `slot2`, then the priest in `leaderSlot`.
#[test]
fn the_kin_brings_a_third_follower() {
    let got = both(|mode| {
        let state = enter_combat_for_encounter(&mut run(mode), "THE_KIN_BOSS", RoomType::Boss)
            .expect("the kin");
        (0..state.enemy_uids.len())
            .map(|i| {
                let c = enemy(&state, i);
                let ai = c.ai.as_ref().expect("ai");
                (c.species.expect("species"), ai.current_move().id, ai.slot())
            })
            .collect::<Vec<_>>()
    });
    use MonsterKind::{KinFollower, KinPriest};
    let priest_opener = got.0[2].1;
    assert_eq!(
        got.0,
        [
            (KinFollower, "POWER_DANCE", Some(0)),
            (KinFollower, "QUICK_SLASH", Some(1)),
            (KinPriest, priest_opener, Some(2)),
        ]
    );
    assert_eq!(
        got.1,
        [
            (KinFollower, "POWER_DANCE", Some(0)),
            (KinFollower, "QUICK_SLASH", Some(1)),
            (KinFollower, "BOOMERANG", Some(2)),
            (KinPriest, priest_opener, Some(3)),
        ]
    );
}

/// `KinFollower.cs:104-122`: the third follower runs the same ring from its
/// own entry: BOOMERANG, POWER_DANCE, QUICK_SLASH, BOOMERANG.
#[test]
fn the_third_kin_follower_cycles_from_boomerang() {
    let mut state =
        enter_combat_for_encounter(&mut run(GameMode::Dread), "THE_KIN_BOSS", RoomType::Boss)
            .expect("the kin");
    let mut seen = vec![current_move(&state, 2)];
    for _ in 0..3 {
        end_turn(&mut state);
        seen.push(current_move(&state, 2));
    }
    assert_eq!(
        seen,
        ["BOOMERANG", "POWER_DANCE", "QUICK_SLASH", "BOOMERANG"]
    );
    assert_eq!(enemy_powers(&state, 2)[0], (PowerKind::Minion, 1));
}

// ───────────────────────────── act 1, Underdocks ─────────────────────────────

/// `WaterfallGiant.cs:85-95` and the five moves that apply
/// `SteamBoostAmount` (`:251`, `:262`, `:272`, `:281`, `:292`: PRESSURE_UP,
/// STOMP, RAM, SIPHON, PRESSURE_GUN). Standard A10 3 each, Dread 4.
/// PRESSURIZE's `PressurizeAmount` is not a Dread value.
#[test]
fn waterfall_giant_boosts_steam_by_four() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::WaterfallGiant]);
        let shown: Vec<i32> = ["STOMP", "RAM", "SIPHON", "PRESSURE_GUN", "PRESSURE_UP"]
            .iter()
            .map(|id| telegraphed(&state, 0, id, PowerKind::SteamEruption))
            .collect();
        let pressurize = telegraphed(&state, 0, "PRESSURIZE", PowerKind::SteamEruption);
        force(&mut state, 0, "STOMP");
        let mut steam = Vec::new();
        for _ in 0..5 {
            end_turn(&mut state);
            steam.push(enemy_power(&state, 0, PowerKind::SteamEruption));
        }
        (shown, pressurize, steam)
    });
    assert_eq!(got.0, (vec![3; 5], got.1 .1, vec![3, 6, 9, 12, 15]));
    assert_eq!(got.1 .0, vec![4; 5]);
    assert_eq!(got.1 .2, vec![4, 8, 12, 16, 20]);
}

/// `SoulFysh.cs:60-70`, `:148-162`: BECKON_MOVE adds a Beckon to the draw
/// pile at a random position and one to the discard pile; Dread adds a third
/// to the HAND between the two, with no RNG. Standard A10 2 Beckons, Dread 3.
#[test]
fn soul_fysh_beckons_into_the_hand_too() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::SoulFysh]);
        assert_eq!(current_move(&state, 0), "BECKON");
        let shuffle = state.run_rng.shuffle.counter();
        end_turn(&mut state);
        let (hand, draw, discard) = spread(&state, CardId::Beckon);
        (
            hand + draw + discard,
            discard,
            state.player.hand.len(),
            state.run_rng.shuffle.counter() - shuffle,
        )
    });
    // (Beckons, in discard, next hand, Shuffle reads)
    assert_eq!(got.0, (2, 1, 5, 1));
    assert_eq!(got.1, (3, 1, 6, 1));
}

/// `LagavulinMatriarch.cs:49`: `MinInitialHp` = `(ToughEnemies, 233, 222)` +
/// 22 in Dread. Standard A10 233, Dread 255.
#[test]
fn lagavulin_matriarch_has_twenty_two_more_hp() {
    let got = both(|mode| roster_hp(mode, "LAGAVULIN_MATRIARCH_BOSS"));
    assert_eq!(got.0, [("LagavulinMatriarch", 233)]);
    assert_eq!(got.1, [("LagavulinMatriarch", 255)]);
}

/// `LagavulinMatriarch.cs:55-57`: SLASH2's `Slash2Damage` `(DeadlyEnemies,
/// 14, 12)` and `Slash2Block` `(ToughEnemies, 14, 12)`, each + 2 in Dread.
/// Standard A10 14 / 14, Dread 16 / 16.
#[test]
fn lagavulin_matriarch_slash2_hits_and_blocks_for_sixteen() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::LagavulinMatriarch]);
        let shown = attack(&state, 0, "SLASH2").0;
        let block_shown = intents(&state, 0, "SLASH2")
            .iter()
            .find_map(|i| match i {
                Intent::Defend { block } => Some(*block),
                _ => None,
            })
            .expect("a Defend intent");
        let before = enemy(&state, 0).block;
        let hp = state.player.current_hp;
        perform(&mut state, 0, "SLASH2");
        (
            shown,
            block_shown,
            hp - state.player.current_hp,
            enemy(&state, 0).block - before,
        )
    });
    assert_eq!(got.0, (14, 14, 14, 14));
    assert_eq!(got.1, (16, 16, 16, 16));
}

/// `LagavulinMatriarch.cs:63-73`, `:252-254`: SOUL_SIPHON's `DebuffAmount`
/// on the player's Strength and Dexterity; the self Strength stays 2.
/// Standard A10 -2 / -2, Dread -3 / -3.
#[test]
fn lagavulin_matriarch_soul_siphon_drains_three() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::LagavulinMatriarch]);
        let shown = telegraphed(&state, 0, "SOUL_SIPHON", PowerKind::Strength);
        perform(&mut state, 0, "SOUL_SIPHON");
        (
            shown,
            player_power(&state, PowerKind::Strength),
            player_power(&state, PowerKind::Dexterity),
            enemy_power(&state, 0, PowerKind::Strength),
        )
    });
    assert_eq!(got.0, (2, -2, -2, 2));
    assert_eq!(got.1, (3, -3, -3, 2));
}

// ───────────────────────────── act 2, Hive ─────────────────────────────

/// `TheInsatiable.cs:58`: `MinInitialHp` = `(ToughEnemies, 341, 321)` + 20
/// in Dread. Standard A10 341, Dread 361.
#[test]
fn the_insatiable_has_twenty_more_hp() {
    let got = both(|mode| roster_hp(mode, "THE_INSATIABLE_BOSS"));
    assert_eq!(got.0, [("TheInsatiable", 341)]);
    assert_eq!(got.1, [("TheInsatiable", 361)]);
}

/// `TheInsatiable.cs:68-78`, `:137`: LIQUIFY's `SandpitAmount`, the countdown
/// to the player's death (lower is harder). Standard A10 4, Dread 3.
#[test]
fn the_insatiable_sandpit_counts_from_three() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::TheInsatiable]);
        let opener = current_move(&state, 0);
        let shown = telegraphed(&state, 0, opener, PowerKind::Sandpit);
        end_turn(&mut state);
        (shown, enemy_power(&state, 0, PowerKind::Sandpit))
    });
    assert_eq!(got, ((4, 4), (3, 3)));
}

/// `KnowledgeDemon.cs:82-92`, `:187`: `DisintegrationDamageValues`, indexed
/// by the curse counter. Standard `{6, 7, 8}`, Dread `{8, 9, 10}`.
#[test]
fn knowledge_demon_disintegration_is_two_higher() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::KnowledgeDemon]);
        let mut totals = Vec::new();
        for _ in 0..3 {
            perform(&mut state, 0, "CURSE_OF_KNOWLEDGE");
            assert!(state.pending_choice.is_some(), "the curse screen");
            let _ = apply(
                &mut state,
                Action::ResolveChoice {
                    selection: smallvec![0],
                },
            );
            totals.push(player_power(&state, PowerKind::Disintegration));
        }
        totals
    });
    assert_eq!(got.0, [6, 6 + 7, 6 + 7 + 8]);
    assert_eq!(got.1, [8, 8 + 9, 8 + 9 + 10]);
}

/// `Crusher.cs:91-101`, `:129-132` and `Rocket.cs:66-76`, `:107-110`:
/// `AfterAddedToRoom` ends with `PlatingPower` `PlatingAmount`, standard
/// none, Dread 14, after each claw's own back-attack power and `CrabRage`.
#[test]
fn kaiser_crab_claws_start_plated() {
    let got = both(|mode| {
        let state = enter_combat_for_encounter(&mut run(mode), "KAISER_CRAB_BOSS", RoomType::Boss)
            .expect("crab");
        (enemy_powers(&state, 0), enemy_powers(&state, 1))
    });
    let crusher = vec![(PowerKind::BackAttackLeft, 1), (PowerKind::CrabRage, 1)];
    let rocket = vec![(PowerKind::BackAttackRight, 1), (PowerKind::CrabRage, 1)];
    assert_eq!(got.0, (crusher.clone(), rocket.clone()));
    let plated = |mut own: Vec<(PowerKind, i32)>| {
        own.push((PowerKind::Plating, 14));
        own
    };
    assert_eq!(got.1, (plated(crusher), plated(rocket)));
}

/// `Rocket.cs:62`, `:85`, `:180`: `RocketClawDamage` = `(DeadlyEnemies, 35,
/// 31)` + 5 in Dread. Standard A10 35, Dread 40.
#[test]
fn rocket_claw_hits_for_forty() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::Rocket]);
        let shown = attack(&state, 0, "ROCKET_CLAW_MOVE");
        let hp = state.player.current_hp;
        perform(&mut state, 0, "ROCKET_CLAW_MOVE");
        (shown, hp - state.player.current_hp)
    });
    // Surrounded (`Rocket.cs:104`) is no back attack on the first turn: the
    // player faces the Rocket, the only enemy.
    assert_eq!(got.0 .0, (35, 1));
    assert_eq!(got.1 .0, (40, 1));
    assert_eq!(got.1 .1 - got.0 .1, 5);
}

// ───────────────────────────── act 3, Glory ─────────────────────────────

/// `Queen.cs:139-168`, `:189-199`: in Dread the Queen opens on
/// `DREAD_INITIAL_MOVE` (`ChainsOfBindingPower` 3, then Frail, Weak and
/// Vulnerable 99 in one turn) and goes straight to `YOURE_MINE_NOW_BRANCH`.
/// Standard A10 opens on `PUPPET_STRINGS_MOVE` and needs `YOU_ARE_MINE_MOVE`
/// for the three debuffs.
#[test]
fn queen_opens_with_chains_and_the_three_debuffs_at_once() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::TorchHeadAmalgam, MonsterKind::Queen]);
        let opener = current_move(&state, 1);
        end_turn(&mut state);
        let debuffs = [
            PowerKind::ChainsOfBinding,
            PowerKind::Frail,
            PowerKind::Weak,
            PowerKind::Vulnerable,
        ]
        .map(|k| player_power(&state, k));
        (opener, debuffs, current_move(&state, 1))
    });
    assert_eq!(
        got.0,
        ("PUPPET_STRINGS_MOVE", [3, 0, 0, 0], "YOURE_MINE_MOVE")
    );
    assert_eq!(
        got.1,
        (
            "DREAD_INITIAL_MOVE",
            [3, 99, 99, 99],
            "BURN_BRIGHT_FOR_ME_MOVE"
        )
    );
}

/// `Queen.cs:142`: `DREAD_INITIAL_MOVE` telegraphs a `CardDebuffIntent` and a
/// `DebuffIntent`; and with the Amalgam dead `YOURE_MINE_NOW_BRANCH` leads to
/// `OFF_WITH_YOUR_HEAD_MOVE` (`:150-151`), as it does after
/// `YOU_ARE_MINE_MOVE`.
#[test]
fn queen_dread_opener_telegraph_and_branch() {
    let mut state = fight(GameMode::Dread, &[MonsterKind::Queen]);
    assert_eq!(
        intents(&state, 0, "DREAD_INITIAL_MOVE"),
        [
            Intent::Debuff {
                kind: PowerKind::ChainsOfBinding,
                amount: 3
            },
            Intent::Debuff {
                kind: PowerKind::Frail,
                amount: 99
            },
        ]
    );
    let standard = fight(GameMode::Standard, &[MonsterKind::Queen]);
    let ai = enemy(&standard, 0).ai.as_ref().expect("ai");
    assert!(
        !ai.states.contains_key("DREAD_INITIAL_MOVE"),
        "the standard table is unchanged"
    );
    // The Amalgam died before the Queen's first move.
    let uid = state.enemy_uids[0];
    let queen = state
        .creatures
        .iter_mut()
        .find(|c| c.id == uid.0)
        .expect("queen");
    queen.ai.as_mut().expect("ai").set_counter(1);
    end_turn(&mut state);
    assert_eq!(current_move(&state, 0), "OFF_WITH_YOUR_HEAD_MOVE");
}

/// `Queen.cs:201-212`: BURN_BRIGHT_FOR_ME gives each teammate Strength,
/// standard A10 1, Dread 2; the Queen's 20 block is unchanged.
#[test]
fn queen_burn_bright_gives_two_strength() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::TorchHeadAmalgam, MonsterKind::Queen]);
        let shown = telegraphed(&state, 1, "BURN_BRIGHT_FOR_ME_MOVE", PowerKind::Strength);
        perform(&mut state, 1, "BURN_BRIGHT_FOR_ME_MOVE");
        (
            shown,
            enemy_power(&state, 0, PowerKind::Strength),
            enemy_power(&state, 1, PowerKind::Strength),
            enemy(&state, 1).block,
        )
    });
    assert_eq!(got.0, (1, 1, 0, 20));
    assert_eq!(got.1, (2, 2, 0, 20));
}

/// `TorchHeadAmalgam.cs:32`, `:51`, `:85`: `StrongTackleDamage` =
/// `(DeadlyEnemies, 32, 26)` + 5 in Dread. Standard A10 32, Dread 37.
#[test]
fn torch_head_amalgam_strong_tackle_hits_for_thirty_seven() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::TorchHeadAmalgam]);
        assert_eq!(current_move(&state, 0), "STRONG_TACKLE");
        let shown = attack(&state, 0, "STRONG_TACKLE");
        let hp = state.player.current_hp;
        end_turn(&mut state);
        (shown, hp - state.player.current_hp)
    });
    assert_eq!(got, (((32, 1), 32), ((37, 1), 37)));
}

/// `TestSubject.cs:87`, `:188`: `EnrageAmount` = `(DeadlyEnemies, 3, 2)` + 1
/// in Dread, applied in `AfterAddedToRoom` after `AdaptablePower`. Standard
/// A10 3, Dread 4.
#[test]
fn test_subject_starts_with_four_enrage() {
    let got = both(|mode| enemy_powers(&fight(mode, &[MonsterKind::TestSubject]), 0));
    assert_eq!(got.0, [(PowerKind::Adaptable, 1), (PowerKind::Enrage, 3)]);
    assert_eq!(got.1, [(PowerKind::Adaptable, 1), (PowerKind::Enrage, 4)]);
}

/// `TestSubject.cs:95-105`, `:172`, `:208`: MULTI_CLAW's
/// `BaseMultiClawCount`. Standard A10 3 hits, Dread 4 (each 11 at A10).
#[test]
fn test_subject_multi_claw_starts_at_four_hits() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::TestSubject]);
        let shown = attack(&state, 0, "MULTI_CLAW");
        let hp = state.player.current_hp;
        perform(&mut state, 0, "MULTI_CLAW");
        (shown, hp - state.player.current_hp)
    });
    assert_eq!(got.0, ((11, 3), 33));
    assert_eq!(got.1, ((11, 4), 44));
}

/// `TestSubject.cs:113`, `:339`: `BurningGrowlStrengthGain` =
/// `(DeadlyEnemies, 3, 2)` + 3 in Dread. Standard A10 3, Dread 6.
#[test]
fn test_subject_burning_growl_gains_six_strength() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::TestSubject]);
        let shown = telegraphed(&state, 0, "BURNING_GROWL", PowerKind::Strength);
        perform(&mut state, 0, "BURNING_GROWL");
        (shown, enemy_power(&state, 0, PowerKind::Strength))
    });
    assert_eq!(got, ((3, 3), (6, 6)));
}

/// `Aeonglass.cs:58`, `:162-163`: INCREASING_INTENSITY adds `WitherAmount`
/// = `(DeadlyEnemies, 2, 1)` + 1 in Dread Withers at `CardPilePosition.
/// Random` (one `Rng.Shuffle.NextInt(Count + 1)` each), into the discard pile
/// in a standard run and into the DRAW pile in Dread. Standard A10: 2 in the
/// discard. Dread: 3 in the draw pile, so they are drawn at once.
#[test]
fn aeonglass_withers_three_into_the_draw_pile() {
    let got = both(|mode| {
        let mut state = fight(mode, &[MonsterKind::Aeonglass]);
        let shuffle = state.run_rng.shuffle.counter();
        perform(&mut state, 0, "INCREASING_INTENSITY_MOVE");
        let (hand, draw, discard) = spread(&state, CardId::Wither);
        (
            hand + draw,
            discard,
            state.run_rng.shuffle.counter() - shuffle,
        )
    });
    // (Withers in hand or draw pile, in discard, Shuffle reads)
    assert_eq!(got.0, (0, 2, 2));
    assert_eq!(got.1, (3, 0, 3));
}

// ───────────────────────────── plumbing ─────────────────────────────

/// A summon is built at the combat's game mode (C# `IsDread()` is a
/// run-global read, `AscensionHelper.cs:61-64`), and the per-thread memo of
/// `build_spawned_ai` keeps the two modes apart: standard A10 STRONG_TACKLE
/// 32, Dread 37, in either order.
#[test]
fn summons_are_built_for_the_combats_game_mode() {
    let strong_tackle = |state: &CombatState| {
        let ai = build_spawned_ai(state, MonsterKind::TorchHeadAmalgam, MonsterSpawnArgs::None);
        match ai.states.get("STRONG_TACKLE") {
            Some(MonsterState::Move(m)) => m.intents[0],
            _ => panic!("no STRONG_TACKLE"),
        }
    };
    let standard = fight(GameMode::Standard, &[MonsterKind::Nibbit]);
    let dread = fight(GameMode::Dread, &[MonsterKind::Nibbit]);
    for _ in 0..2 {
        assert_eq!(strong_tackle(&standard), Intent::Attack { damage: 32 });
        assert_eq!(strong_tackle(&dread), Intent::Attack { damage: 37 });
    }
}

/// No hallway or elite monster has a Dread branch (`grep IsDread()` over the
/// decomp: 13 boss files and `TheKinBoss`): outside the kinds above, a Dread
/// fight builds the standard A10 move table.
#[test]
fn other_monsters_are_the_same_in_dread() {
    const DREAD_KINDS: [MonsterKind; 14] = [
        MonsterKind::Vantom,
        MonsterKind::CeremonialBeast,
        MonsterKind::KinFollower,
        MonsterKind::WaterfallGiant,
        MonsterKind::SoulFysh,
        MonsterKind::LagavulinMatriarch,
        MonsterKind::TheInsatiable,
        MonsterKind::KnowledgeDemon,
        MonsterKind::Crusher,
        MonsterKind::Rocket,
        MonsterKind::Queen,
        MonsterKind::TorchHeadAmalgam,
        MonsterKind::TestSubject,
        MonsterKind::Aeonglass,
    ];
    let standard = fight(GameMode::Standard, &[MonsterKind::Nibbit]);
    let dread = fight(GameMode::Dread, &[MonsterKind::Nibbit]);
    let mut checked = 0;
    for &kind in MonsterKind::ALL_KNOWN {
        if DREAD_KINDS.contains(&kind) {
            continue;
        }
        let a = build_spawned_ai(&standard, kind, MonsterSpawnArgs::None);
        let b = build_spawned_ai(&dread, kind, MonsterSpawnArgs::None);
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "{kind:?}");
        checked += 1;
    }
    assert!(checked > 80, "{checked} kinds compared");
}
