//! v0.113.0 Dread mode (GH #1346): the twelve dominions
//! (`Models.Dread.Dominions/<Name>Dominion.cs`), the powers they apply
//! (`Models.Powers/{AeonglassDominion,InsatiableDominion,
//! LagavulinMatriarchDominion,YouAreMine,Amnesia,Explosive}Power.cs`) and the
//! `Amnesia` card (`Models.Cards/Amnesia.cs`).
#![allow(dead_code)]

use smallvec::smallvec;

use sts2_content::cards::dispatch as content_dispatch;
use sts2_content::cards::v0111::card_pod_for;
use sts2_content::monster_dispatch::MonsterSpawnArgs;
use sts2_content::run_bridge::spawn_roster;
use sts2_core::afflictions::{classify, AfflictionKind};
use sts2_core::cards::CardId;
use sts2_core::combat_loop::{start_combat, start_player_turn};
use sts2_core::hooks::{dispatch_after_turn_end_late, TurnSideCtx};
use sts2_core::potions::CharacterId;
use sts2_core::{
    apply, Action, ChoiceKind, CombatPhase, CombatState, CreatureUid, Dominion, GameMode,
    GameVersion, MonsterKind, PlayerCombatState, PowerKind, Side, StepResult,
};

const V113: GameVersion = GameVersion::V0_113_0;

/// The six powers Dread adds, with the id the C# model gives them
/// (`ModelId.Entry` minus the `_POWER` suffix, as for every other power).
const DREAD_POWERS: [(PowerKind, &str); 6] = [
    (PowerKind::AeonglassDominion, "AEONGLASS_DOMINION"),
    (PowerKind::InsatiableDominion, "INSATIABLE_DOMINION"),
    (
        PowerKind::LagavulinMatriarchDominion,
        "LAGAVULIN_MATRIARCH_DOMINION",
    ),
    (PowerKind::YouAreMine, "YOU_ARE_MINE"),
    (PowerKind::Amnesia, "AMNESIA"),
    (PowerKind::Explosive, "EXPLOSIVE"),
];

/// `Models.Powers/*Power.cs` class names: the six Dread powers exist from
/// v0.113.0 on only, under their C# ids.
#[test]
fn dread_powers_are_v0113_only() {
    for (kind, name) in DREAD_POWERS {
        assert_eq!(kind.python_name(), name);
        assert!(kind.exists_in(V113), "{kind:?}");
        assert!(!kind.exists_in(GameVersion::V0_111_0), "{kind:?}");
        assert!(PowerKind::all_for(V113).contains(&kind), "{kind:?}");
        assert!(!PowerKind::all().contains(&kind), "{kind:?}");
    }
    assert_ne!(PowerKind::Explosive, PowerKind::ExplosiveArrows);
}

/// `PowerType`: the five player powers are `Debuff`
/// (`AeonglassDominionPower.cs:16`, `InsatiableDominionPower.cs:18`,
/// `LagavulinMatriarchDominionPower.cs:15`, `YouAreMinePower.cs:19`,
/// `AmnesiaPower.cs:16`); `ExplosivePower.cs:17` is a `Buff`.
#[test]
fn dread_player_powers_are_debuffs() {
    for (kind, _) in &DREAD_POWERS[..5] {
        assert_eq!(classify(*kind).kind, AfflictionKind::Debuff, "{kind:?}");
    }
    assert_eq!(classify(PowerKind::Explosive).kind, AfflictionKind::Buff);
}

/// `Models.Cards/Amnesia.cs`: a Status card, cost -1, no upgrade, v0.113.0.
#[test]
fn amnesia_card_is_a_v0113_status() {
    content_dispatch::ensure_installed();
    assert_eq!(CardId::Amnesia.to_python_name(), "AMNESIA");
    assert_eq!(CardId::Amnesia.introduced_in(), V113);
    let card = card_pod_for(CardId::Amnesia, false, V113).expect("Amnesia has a spec");
    assert_eq!(card.card_type, sts2_core::cards::CardType::Status);
    let spec = sts2_content::cards::spec_index::spec_for_card_id(CardId::Amnesia).expect("spec");
    assert_eq!(spec.cost, -1);
    assert!(spec.is_unplayable());
}

// ───────────────────────────── fixtures ─────────────────────────────

/// A v0.113.0 Dread combat before `start_combat`: an 80-HP Ironclad with
/// twelve Strikes, `roster` spawned with its own start powers (as
/// `run_bridge::enter_combat_from_roster_in_slots` does), then the dominion.
fn dread(dominion: Option<Dominion>, roster: &[MonsterKind]) -> CombatState {
    content_dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    let mut state = CombatState {
        game_version: V113,
        game_mode: GameMode::Dread,
        run_rng: sts2_core::run_rng_set::RunRngSet::new(4242_i32),
        ..CombatState::default()
    };
    state.player.current_hp = 80;
    state.player.max_hp = 80;
    state.player.max_energy = 3;
    state.player.character = Some(CharacterId::Ironclad);
    let _ = state.init_player_creature();
    for _ in 0..12 {
        let card = card_pod_for(CardId::StrikeIronclad, false, V113).expect("spec");
        let _ = state.add_card_to_draw_pile(card);
    }
    let roster: Vec<_> = roster
        .iter()
        .map(|&kind| (kind, MonsterSpawnArgs::None, 200))
        .collect();
    let _ = spawn_roster(&mut state, &roster);
    state.dominion = dominion;
    state
}

/// The sleeping Lagavulin Matriarch (own start powers Plating then Asleep,
/// `LagavulinMatriarch.cs`): an enemy that does not act for three turns.
const SLEEPER: [MonsterKind; 1] = [MonsterKind::LagavulinMatriarch];

/// Combat entry: `start_combat`, then the first `start_player_turn`.
fn enter(state: &mut CombatState) -> StepResult {
    assert_eq!(start_combat(state), StepResult::Advanced);
    start_player_turn(state)
}

fn entered(dominion: Option<Dominion>, roster: &[MonsterKind]) -> CombatState {
    let mut state = dread(dominion, roster);
    assert_eq!(enter(&mut state), StepResult::Advanced);
    assert_eq!(state.phase, CombatPhase::PlayerPlayPhase);
    state
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

fn player_powers(state: &CombatState) -> Vec<(PowerKind, i32)> {
    state
        .player_power_uids()
        .iter()
        .filter_map(|p| state.powers.get_by_id(p.0))
        .map(|p| (p.kind, p.amount))
        .collect()
}

fn hand_ids(state: &CombatState) -> Vec<CardId> {
    state
        .player
        .hand
        .iter()
        .map(|u| state.cards[u.0.get() as usize - 1].card_id)
        .collect()
}

fn give_player(state: &mut CombatState, kind: PowerKind, amount: i32) {
    let uid = state.player.creature_uid.expect("player creature");
    sts2_core::card_effects::apply_power_to_creature(state, uid, kind, amount);
}

// ───────────────────── enemy-buff dominions, room entry ─────────────────────

/// `CeremonialBeastDominion.cs:25-28` (Ritual 1), `KaiserCrabDominion.cs:
/// 25-28` (Plating 7), `TestSubjectDominion.cs:23-26` (Strength 3),
/// `VantomDominion.cs:25-37` (Slippery 1): every enemy in the room gets the
/// power, and it sits BEFORE the monster's own `AfterAddedToRoom` powers
/// because `AfterRoomEntered` runs first (`CombatRoom.cs:200-234`).
#[test]
fn enemy_dominions_buff_the_roster_ahead_of_its_own_powers() {
    let own = enemy_powers(&entered(None, &SLEEPER), 0);
    assert_eq!(
        own.iter().map(|p| p.0).collect::<Vec<_>>(),
        [PowerKind::Plating, PowerKind::Asleep]
    );
    for (dominion, kind, amount) in [
        (Dominion::CeremonialBeast, PowerKind::Ritual, 1),
        (Dominion::TestSubject, PowerKind::Strength, 3),
        (Dominion::Vantom, PowerKind::Slippery, 1),
    ] {
        let state = entered(Some(dominion), &SLEEPER);
        let mut want = vec![(kind, amount)];
        want.extend(own.iter().copied());
        assert_eq!(enemy_powers(&state, 0), want, "{dominion:?}");
    }
    // Plating stacks with the Matriarch's own into the first instance.
    let state = entered(Some(Dominion::KaiserCrab), &SLEEPER);
    assert_eq!(
        enemy_powers(&state, 0),
        [(PowerKind::Plating, own[0].1 + 7), own[1]]
    );
}

/// `KaiserCrabDominion.cs:25-28` on a monster with no Plating of its own;
/// `PlatingPower.cs:39-55` then gives an enemy its Plating as block before
/// the first player turn.
#[test]
fn kaiser_crab_plating_blocks_on_round_one() {
    let state = entered(Some(Dominion::KaiserCrab), &[MonsterKind::Nibbit]);
    assert_eq!(enemy_powers(&state, 0), [(PowerKind::Plating, 7)]);
    assert_eq!(state.creatures[1].block, 7);
}

/// `VantomDominion.cs:30-35`: Slippery 2 when the encounter's room type is
/// Elite.
#[test]
fn vantom_gives_two_slippery_in_an_elite_room() {
    let mut state = dread(Some(Dominion::Vantom), &[MonsterKind::Nibbit]);
    state.is_elite = true;
    assert_eq!(enter(&mut state), StepResult::Advanced);
    assert_eq!(enemy_powers(&state, 0), [(PowerKind::Slippery, 2)]);
}

/// `WaterfallGiantDominion.cs:44-59`: Explosive 3 with more than one
/// monster, else 4 in a weak encounter, else 7 in an elite room, else 5.
#[test]
fn waterfall_giant_explosive_amount_follows_the_encounter() {
    let solo = [MonsterKind::Nibbit];
    let pair = [MonsterKind::Nibbit, MonsterKind::Nibbit];
    for (roster, weak, elite, want) in [
        (&solo[..], false, false, 5),
        (&solo[..], true, false, 4),
        (&solo[..], false, true, 7),
        (&solo[..], true, true, 4),
        (&pair[..], false, false, 3),
        (&pair[..], true, true, 3),
    ] {
        let mut state = dread(Some(Dominion::WaterfallGiant), roster);
        state.combat_log.dread_weak_encounter = weak;
        state.is_elite = elite;
        assert_eq!(enter(&mut state), StepResult::Advanced);
        for i in 0..roster.len() {
            assert_eq!(
                enemy_powers(&state, i),
                [(PowerKind::Explosive, want)],
                "weak={weak} elite={elite} n={}",
                roster.len()
            );
        }
    }
}

/// `WaterfallGiantDominion.cs:22-28`: the room-entry path takes
/// `CombatState.Enemies` with no `MinionPower` filter.
#[test]
fn waterfall_giant_entry_includes_minions() {
    let state = entered(
        Some(Dominion::WaterfallGiant),
        &[MonsterKind::Nibbit, MonsterKind::GasBomb],
    );
    assert_eq!(
        enemy_powers(&state, 1),
        [(PowerKind::Explosive, 3), (PowerKind::Minion, 1)]
    );
}

/// A combat with no dominion (a standard run, or the act's boss fights,
/// `DreadRunModel.cs:53-69`) gets nothing.
#[test]
fn no_dominion_no_effect() {
    let state = entered(None, &[MonsterKind::Nibbit]);
    assert_eq!(enemy_powers(&state, 0), []);
    assert_eq!(player_powers(&state), []);
    assert_eq!(hand_ids(&state), [CardId::StrikeIronclad; 5]);
    assert_eq!(state.player.draw.len(), 7);
}

// ───────────────────── enemy-buff dominions, summons ─────────────────────

/// `DominionModel.cs:73-80` + the four `AfterEnemiesEnteredCombat`
/// overrides: an enemy summoned mid-combat gets the dominion's power after
/// its own start powers. `VantomDominion.cs:39-46`: a summon gets Slippery 1
/// even in an elite room.
#[test]
fn enemy_dominions_buff_a_summon() {
    for (dominion, kind, amount) in [
        (Dominion::CeremonialBeast, PowerKind::Ritual, 1),
        (Dominion::KaiserCrab, PowerKind::Plating, 7),
        (Dominion::TestSubject, PowerKind::Strength, 3),
        (Dominion::Vantom, PowerKind::Slippery, 1),
    ] {
        let mut state = dread(Some(dominion), &[MonsterKind::Nibbit]);
        state.is_elite = true;
        assert_eq!(enter(&mut state), StepResult::Advanced);
        let summon = state.spawn_enemy(MonsterKind::Nibbit);
        assert_eq!(powers_of(&state, summon), [(kind, amount)], "{dominion:?}");
        // A minion's own Minion comes first: `CreatureCmd.Add` runs the
        // monster's `AfterAddedToRoom` before the hook (`CreatureCmd.cs:69-82`).
        let bomb = state.spawn_enemy(MonsterKind::GasBomb);
        assert_eq!(
            powers_of(&state, bomb),
            [(PowerKind::Minion, 1), (kind, amount)],
            "{dominion:?}"
        );
    }
}

/// An enemy summoned during the enemy turn keeps its full Plating through
/// that turn: `PlatingPower.AfterSideTurnStart` (`PlatingPower.cs:68-80`)
/// ticked before the summon existed, and `BeforeSideTurnEndEarly` (`:59-66`)
/// then blocks for the whole 7. Rust ticks an enemy's Plating at turn end,
/// which took 1 off the eggs the Ovicopter had just laid (Dread act23
/// Defect:1003 v0.113.0: cs Plating 7 / block 7, rs 6 / 6).
#[test]
fn kaiser_crab_plating_does_not_tick_the_turn_a_summon_arrives() {
    let mut state = entered(Some(Dominion::KaiserCrab), &[MonsterKind::Ovicopter]);
    state.player.current_hp = 9_999;
    state.player.max_hp = 9_999;
    let ovicopter = state.enemy_uids[0];
    let mut late_summons = 0;
    for _ in 0..8 {
        let before = state.enemy_uids.clone();
        let round = state.round_no();
        let _ = apply(&mut state, Action::EndTurn);
        assert_eq!(state.phase, CombatPhase::PlayerPlayPhase);
        for &uid in state.enemy_uids.iter().filter(|u| !before.contains(u)) {
            let plating: Vec<i32> = powers_of(&state, uid)
                .into_iter()
                .filter(|(k, _)| *k == PowerKind::Plating)
                .map(|(_, n)| n)
                .collect();
            let block = state
                .creatures
                .iter()
                .find(|c| c.id == uid.0)
                .expect("summon")
                .block;
            assert_eq!((plating, block), (vec![7], 7), "summoned in round {round}");
            late_summons += usize::from(round > 1);
        }
        // Clear the eggs so the Ovicopter has free slots to lay again.
        for c in &mut state.creatures {
            if c.id != ovicopter.0 && state.enemy_uids.iter().any(|u| u.0 == c.id) {
                c.current_hp = 0;
            }
        }
    }
    assert!(late_summons > 0, "the Ovicopter lays eggs after round 1");
}

/// `WaterfallGiantDominion.cs:30-42`: a summon gets the encounter's
/// Explosive amount (`MonstersWithSlots` does not grow) unless it already
/// holds `MinionPower`.
#[test]
fn waterfall_giant_summon_skips_minions() {
    let mut state = dread(Some(Dominion::WaterfallGiant), &[MonsterKind::Nibbit]);
    state.is_elite = true;
    assert_eq!(enter(&mut state), StepResult::Advanced);
    let summon = state.spawn_enemy(MonsterKind::Nibbit);
    assert_eq!(powers_of(&state, summon), [(PowerKind::Explosive, 7)]);
    let bomb = state.spawn_enemy(MonsterKind::GasBomb);
    assert_eq!(powers_of(&state, bomb), [(PowerKind::Minion, 1)]);
}

/// The Wrigglers a Phrog Parasite spawns when Doom kills it at the enemy
/// `BeforeSideTurnEnd` (`DoomPower.cs:63-69`) are not participants of that
/// enemy turn end: `EndEnemyTurnInternal` hands both hooks the
/// `CreaturesOnCurrentSide` list it took before `BeforeSideTurnEnd`
/// (`CombatManager.cs:1810-1819`). Their Ritual (`RitualPower.cs:36-61`) was
/// applied while `CurrentSide == Enemy`, so it skips one tick, and that skip
/// is spent at the NEXT enemy turn end, not this one: no Strength until the
/// third. Rust spent it at once and granted Strength a round early (Dread
/// act23 Necrobinder:1001 v0.113.0, step 164: cs [Ritual 1], rs [Ritual 1,
/// Strength 1]).
#[test]
fn ceremonial_beast_ritual_on_a_doom_spawned_wriggler_skips_the_next_turn_end() {
    let mut state = entered(
        Some(Dominion::CeremonialBeast),
        &[MonsterKind::PhrogParasite],
    );
    state.player.current_hp = 9_999;
    state.player.max_hp = 9_999;
    let phrog = state.enemy_uids[0];
    let phrog_hp = state
        .creatures
        .iter()
        .find(|c| c.id == phrog.0)
        .expect("phrog")
        .current_hp;
    sts2_core::card_effects::apply_power_to_creature(&mut state, phrog, PowerKind::Doom, phrog_hp);
    let wriggler_powers = |state: &CombatState| -> Vec<Vec<(PowerKind, i32)>> {
        state
            .enemy_uids
            .iter()
            .filter(|u| {
                state
                    .creatures
                    .iter()
                    .any(|c| c.id == u.0 && c.species == Some(MonsterKind::Wriggler))
            })
            .map(|&u| powers_of(state, u))
            .collect()
    };

    // Enemy turn 1: Doom kills the Phrog at BeforeSideTurnEnd, the Wrigglers
    // arrive with Ritual 1.
    let _ = apply(&mut state, Action::EndTurn);
    assert_eq!(state.phase, CombatPhase::PlayerPlayPhase);
    let ritual_only = vec![(PowerKind::Ritual, 1)];
    assert_eq!(wriggler_powers(&state), vec![ritual_only.clone(); 4]);

    // Enemy turn 2 (the Wrigglers' stunned SPAWNED_MOVE): their first turn
    // end as participants spends the skip.
    let _ = apply(&mut state, Action::EndTurn);
    assert_eq!(state.phase, CombatPhase::PlayerPlayPhase);
    assert_eq!(wriggler_powers(&state), vec![ritual_only; 4]);

    // Enemy turn 3: Ritual grants its Strength (on top of WRIGGLE_MOVE's 2
    // for the even slots).
    let _ = apply(&mut state, Action::EndTurn);
    for powers in wriggler_powers(&state) {
        assert!(
            powers
                .iter()
                .any(|&(k, n)| k == PowerKind::Strength && n >= 1),
            "{powers:?}"
        );
    }
}

/// No dominion: a summon gets only its own powers.
#[test]
fn no_dominion_summon_is_untouched() {
    let mut state = entered(None, &[MonsterKind::Nibbit]);
    let summon = state.spawn_enemy(MonsterKind::Nibbit);
    assert_eq!(powers_of(&state, summon), []);
}

// ───────────────────────────── Explosive ─────────────────────────────

/// `ExplosivePower.cs:21-32`: when the owner dies, every living player takes
/// `Amount` unpowered damage, which block absorbs.
#[test]
fn explosive_hits_the_player_when_its_owner_dies() {
    let mut state = entered(Some(Dominion::WaterfallGiant), &[MonsterKind::Nibbit; 2]);
    let target = state.enemy_uids[0];
    state.player.block = 1;
    let hp = state.player.current_hp;
    sts2_core::card_effects::deal_damage_player_to_enemy(&mut state, target, 999);
    assert_eq!(state.creatures[1].current_hp, 0);
    assert_eq!(state.player.block, 0);
    assert_eq!(state.player.current_hp, hp - 2, "Explosive 3 minus 1 block");
}

/// `ExplosivePower.cs:23`: `creature == Owner && !wasRemovalPrevented`.
/// C# passes `wasRemovalPrevented: true` only when `Hook.ShouldDie` kept the
/// creature alive (`CreatureCmd.cs:613`, `:674`), which no enemy power does;
/// a death whose corpse stays for a revive (`IllusionPower.
/// ShouldCreatureBeRemovedFromCombatAfterDeath`) is `wasRemovalPrevented:
/// false` (`CreatureCmd.cs:616-627`) and explodes. The Illusion keeps its
/// buffs through the death (`IllusionPower.cs:53-65`), Explosive among them.
#[test]
fn explosive_fires_when_an_illusion_dies_and_stays_on_it() {
    let mut state = entered(
        Some(Dominion::WaterfallGiant),
        &[MonsterKind::EyeWithTeeth, MonsterKind::Nibbit],
    );
    let eye = state.enemy_uids[0];
    assert!(powers_of(&state, eye).contains(&(PowerKind::Explosive, 3)));
    let hp = state.player.current_hp;
    sts2_core::card_effects::deal_damage_player_to_enemy(&mut state, eye, 999);
    assert_eq!(state.player.current_hp, hp - 3);
    assert!(!state.is_over);
    let kept = powers_of(&state, eye);
    assert!(kept.contains(&(PowerKind::Illusion, 1)), "{kept:?}");
    assert!(kept.contains(&(PowerKind::Explosive, 3)), "{kept:?}");
}

// ───────────────────── per-combat notes and the digests ─────────────────────

/// Every combat digest of `state`.
fn digests(state: &CombatState) -> [u64; 3] {
    [
        sts2_core::hashing::state_hash(state),
        sts2_core::hashing::search_hash(state),
        sts2_core::hashing::tt_hash_v2(state),
    ]
}

fn strip_enemy_powers(state: &mut CombatState) {
    let uids: Vec<_> = state.enemy_uids.to_vec();
    for uid in uids {
        let held: Vec<_> = state
            .creatures
            .iter()
            .find(|c| c.id == uid.0)
            .map(|c| c.powers.to_vec())
            .unwrap_or_default();
        for p in held {
            state.drop_power(p.0);
        }
    }
}

/// `WaterfallGiantDominion.GetExplosiveAmount` (`WaterfallGiantDominion.cs:
/// 44-59`) reads the encounter's `MonstersWithSlots.Count`, fixed when the
/// combat starts, so a summon gets the amount of the ENTRY roster. The
/// engine keeps that amount in `TurnEvents::dread_explosive`; once no enemy
/// on the board holds Explosive nothing else records it, and two boards that
/// differ only there give a summon different amounts. So it is state, and
/// every digest tells them apart.
#[test]
fn the_explosive_stamp_is_state_the_digests_see() {
    let mut a = entered(Some(Dominion::WaterfallGiant), &[MonsterKind::Nibbit]);
    assert_eq!(a.combat_log.dread_explosive, 5);
    strip_enemy_powers(&mut a);
    let mut b = a.clone();
    b.combat_log.dread_explosive = 7;
    let (da, db) = (digests(&a), digests(&b));
    let (sa, sb) = (
        a.spawn_enemy(MonsterKind::Nibbit),
        b.spawn_enemy(MonsterKind::Nibbit),
    );
    assert_eq!(powers_of(&a, sa), [(PowerKind::Explosive, 5)]);
    assert_eq!(powers_of(&b, sb), [(PowerKind::Explosive, 7)]);
    for i in 0..3 {
        assert_ne!(da[i], db[i], "digest {i}");
    }
}

/// `EncounterModel.IsWeak` (`WaterfallGiantDominion.cs:50-53`): a board with
/// no stamp (one rebuilt from a wire that carries none) and no Explosive on
/// it falls back to the encounter's own terms, `IsWeak` among them. So the
/// run layer's note is state too.
#[test]
fn the_weak_encounter_note_is_state_the_digests_see() {
    let mut a = entered(Some(Dominion::WaterfallGiant), &[MonsterKind::Nibbit]);
    strip_enemy_powers(&mut a);
    a.combat_log.dread_explosive = 0;
    // The lone monster is gone; the summon is the roster.
    a.enemy_uids.clear();
    let mut b = a.clone();
    b.combat_log.dread_weak_encounter = true;
    let (da, db) = (digests(&a), digests(&b));
    let (sa, sb) = (
        a.spawn_enemy(MonsterKind::Nibbit),
        b.spawn_enemy(MonsterKind::Nibbit),
    );
    assert_eq!(powers_of(&a, sa), [(PowerKind::Explosive, 5)]);
    assert_eq!(powers_of(&b, sb), [(PowerKind::Explosive, 4)]);
    for i in 0..3 {
        assert_ne!(da[i], db[i], "digest {i}");
    }
}

/// A standard combat has neither note, and a Dread one under another
/// dominion has no stamp: their digests fold nothing in for them (the fold
/// is conditional, so every standard digest keeps its bytes).
#[test]
fn the_dread_notes_are_default_outside_the_waterfall_giant_dominion() {
    for dominion in [None, Some(Dominion::Vantom), Some(Dominion::KnowledgeDemon)] {
        let mut state = dread(dominion, &[MonsterKind::Nibbit]);
        let _ = enter(&mut state);
        assert_eq!(state.combat_log.dread_explosive, 0, "{dominion:?}");
        assert!(!state.combat_log.dread_weak_encounter, "{dominion:?}");
    }
}

/// `TurnEvents::dread_knowledge_choice_opened` is a latch of the turn-1
/// hand draw, not state: it is raised when `KnowledgeDemonDominion`'s screen
/// opens and read once, when the parked draw resumes. It is the same on
/// every board the search can reach at a given point (true from the screen
/// on, in a `KnowledgeDemonDominion` combat), so it is in no digest.
#[test]
fn the_knowledge_choice_latch_is_not_in_the_digests() {
    let mut state = at_knowledge_choice();
    assert!(state.combat_log.dread_knowledge_choice_opened);
    let _ = apply(
        &mut state,
        Action::ResolveChoice {
            selection: smallvec![0],
        },
    );
    assert_eq!(state.phase, CombatPhase::PlayerPlayPhase);
    assert!(state.combat_log.dread_knowledge_choice_opened);
    let mut other = state.clone();
    other.combat_log.dread_knowledge_choice_opened = false;
    assert_eq!(digests(&state), digests(&other));
    // Nothing reads it again: the two boards play the same turn.
    let _ = apply(&mut state, Action::EndTurn);
    let _ = apply(&mut other, Action::EndTurn);
    assert!(state.pending_choice.is_none() && other.pending_choice.is_none());
    assert_eq!(hand_ids(&state), hand_ids(&other));
    assert_eq!(player_powers(&state), player_powers(&other));
}

// ───────────────────────── player dominions ─────────────────────────

/// `KinDominion.cs:28-35`: Weak 2, then Frail 2, on every player at room
/// entry.
#[test]
fn kin_weakens_and_frails_the_player() {
    let state = entered(Some(Dominion::Kin), &SLEEPER);
    assert_eq!(
        player_powers(&state),
        [(PowerKind::Weak, 2), (PowerKind::Frail, 2)]
    );
}

/// `QueenDominion.cs:30-37` + `YouAreMinePower.cs:33-45`: YouAreMine 3 at
/// entry does nothing on turn 1; at the start of turn 2 (after the hand
/// draw) it becomes Weak 3, Vulnerable 3, Frail 3 and removes itself.
#[test]
fn queen_you_are_mine_fires_on_turn_two() {
    let mut state = entered(Some(Dominion::Queen), &SLEEPER);
    assert_eq!(player_powers(&state), [(PowerKind::YouAreMine, 3)]);
    let _ = apply(&mut state, Action::EndTurn);
    assert_eq!(state.round, 2);
    assert_eq!(
        player_powers(&state),
        [
            (PowerKind::Weak, 3),
            (PowerKind::Vulnerable, 3),
            (PowerKind::Frail, 3)
        ]
    );
    assert_eq!(state.player.hand.len(), 5);
}

/// `LagavulinMatriarchDominion.cs:28-34` + `LagavulinMatriarchDominionPower
/// .cs:28-38`: the power sits through turn 1; at the player side's first turn
/// end the owner loses 1 Strength and 1 Dexterity and the power is removed.
#[test]
fn lagavulin_matriarch_drains_strength_and_dexterity_after_turn_one() {
    let mut state = entered(Some(Dominion::LagavulinMatriarch), &SLEEPER);
    assert_eq!(
        player_powers(&state),
        [(PowerKind::LagavulinMatriarchDominion, 1)]
    );
    let _ = apply(&mut state, Action::EndTurn);
    assert_eq!(
        player_powers(&state),
        [(PowerKind::Strength, -1), (PowerKind::Dexterity, -1)]
    );
    let _ = apply(&mut state, Action::EndTurn);
    assert_eq!(
        player_powers(&state),
        [(PowerKind::Strength, -1), (PowerKind::Dexterity, -1)]
    );
}

/// `LagavulinMatriarchDominionPower.cs:33-35`: the two losses are
/// `PowerCmd.Apply` of a negative Strength / Dexterity, which Artifact reads
/// as debuffs (`ArtifactPower.cs:24-28`): one stack eats the Strength loss.
#[test]
fn artifact_eats_the_lagavulin_strength_loss() {
    let mut state = entered(Some(Dominion::LagavulinMatriarch), &SLEEPER);
    give_player(&mut state, PowerKind::Artifact, 1);
    let _ = apply(&mut state, Action::EndTurn);
    assert_eq!(player_powers(&state), [(PowerKind::Dexterity, -1)]);
}

/// `InsatiableDominion.cs:27-33` + `InsatiableDominionPower.cs:22-55`: at the
/// owner's turn end (the `AfterSideTurnEndLate` walk, `Hook.cs:1480-1489`)
/// they take `2 + TurnNumber` damage: 3 on turn 1, 4 on turn 2. The props are
/// `Unpowered` only, so block absorbs it (`CreatureCmd.cs:321-339`).
#[test]
fn insatiable_deals_two_plus_turn_number_at_turn_end() {
    let mut state = entered(Some(Dominion::Insatiable), &SLEEPER);
    assert_eq!(player_powers(&state), [(PowerKind::InsatiableDominion, 1)]);
    let _ = apply(&mut state, Action::EndTurn);
    assert_eq!(state.player.current_hp, 80 - 3);
    let _ = apply(&mut state, Action::EndTurn);
    assert_eq!(state.player.current_hp, 80 - 3 - 4);
    assert_eq!(player_powers(&state), [(PowerKind::InsatiableDominion, 1)]);

    // Blockable, and silent on the enemy side's turn end.
    state.player.block = 3;
    let hp = state.player.current_hp;
    dispatch_after_turn_end_late(&mut state, &TurnSideCtx { side: Side::Enemy });
    assert_eq!((state.player.current_hp, state.player.block), (hp, 3));
    dispatch_after_turn_end_late(&mut state, &TurnSideCtx { side: Side::Player });
    assert_eq!(state.round, 3);
    assert_eq!((state.player.current_hp, state.player.block), (hp - 2, 0));
}

/// `AeonglassDominion.cs:26-32` + `AeonglassDominionPower.cs:30-41`: before
/// every hand draw a Wither goes on top of the owner's draw pile, so it is
/// the first card drawn, every turn.
#[test]
fn aeonglass_draws_a_wither_every_turn() {
    let mut state = entered(Some(Dominion::Aeonglass), &SLEEPER);
    assert_eq!(player_powers(&state), [(PowerKind::AeonglassDominion, 1)]);
    let hand = hand_ids(&state);
    assert_eq!(hand.len(), 5);
    assert_eq!(hand[0], CardId::Wither);
    assert_eq!(hand.iter().filter(|&&c| c == CardId::Wither).count(), 1);
    let _ = apply(&mut state, Action::EndTurn);
    let hand = hand_ids(&state);
    assert_eq!(hand[0], CardId::Wither);
    assert_eq!(hand.iter().filter(|&&c| c == CardId::Wither).count(), 1);
}

/// `PowerCmd.Apply` + `ArtifactPower.cs:17-41`: Artifact on a player eats a
/// dominion debuff and loses a stack. (Kin's two applies eat two.)
#[test]
fn artifact_eats_the_dominion_debuffs() {
    for (dominion, stacks, want) in [
        (Dominion::Queen, 1, vec![]),
        (Dominion::Aeonglass, 2, vec![(PowerKind::Artifact, 1)]),
        (Dominion::Insatiable, 1, vec![]),
        (Dominion::LagavulinMatriarch, 1, vec![]),
        (Dominion::Kin, 1, vec![(PowerKind::Frail, 2)]),
        (Dominion::Kin, 3, vec![(PowerKind::Artifact, 1)]),
    ] {
        let mut state = dread(Some(dominion), &SLEEPER);
        give_player(&mut state, PowerKind::Artifact, stacks);
        assert_eq!(enter(&mut state), StepResult::Advanced);
        assert_eq!(player_powers(&state), want, "{dominion:?} x{stacks}");
    }
}

// ───────────────────────────── SoulFysh ─────────────────────────────

/// `SoulFyshDominion.cs:23-30`: one Beckon into the draw pile at
/// `CardPilePosition.Random`, which is one `Rng.Shuffle.NextInt(Count + 1)`
/// (`CardPileCmd.cs:510`) taken after the opening shuffle.
#[test]
fn soul_fysh_adds_a_beckon_at_a_shuffle_rng_position() {
    // The same combat without the dominion gives the shuffled pile and the
    // shuffle stream right after the opening shuffle.
    let mut plain = dread(None, &SLEEPER);
    assert_eq!(start_combat(&mut plain), StepResult::Advanced);
    let at = plain
        .run_rng
        .shuffle
        .next_int(0, plain.player.draw.len() as i32) as usize;

    let mut state = dread(Some(Dominion::SoulFysh), &SLEEPER);
    assert_eq!(start_combat(&mut state), StepResult::Advanced);
    assert_eq!(state.player.draw.len(), 13);
    let ids: Vec<CardId> = state
        .player
        .draw
        .iter()
        .map(|u| state.cards[u.0.get() as usize - 1].card_id)
        .collect();
    assert_eq!(ids.iter().filter(|&&c| c == CardId::Beckon).count(), 1);
    assert_eq!(ids[at], CardId::Beckon);
    // Exactly one read: both shuffle streams now agree.
    assert_eq!(
        state.run_rng.shuffle.next_int(0, 1000),
        plain.run_rng.shuffle.next_int(0, 1000)
    );
    // The original cards keep their relative order.
    let rest: Vec<_> = state
        .player
        .draw
        .iter()
        .copied()
        .filter(|u| state.cards[u.0.get() as usize - 1].card_id != CardId::Beckon)
        .collect();
    assert_eq!(rest.as_slice(), plain.player.draw.as_slice());
}

// ─────────────────────────── KnowledgeDemon ───────────────────────────

/// A KnowledgeDemon-dominion combat parked on the turn-1 choice.
fn at_knowledge_choice() -> CombatState {
    let mut state = dread(Some(Dominion::KnowledgeDemon), &SLEEPER);
    assert_eq!(enter(&mut state), StepResult::PendingChoice);
    state
}

/// `KnowledgeDemonDominion.cs:26-41`: before the first hand draw the player
/// must choose between Sloth and Amnesia (`FromChooseACardScreen`, no skip);
/// the draw waits for the pick.
#[test]
fn knowledge_demon_asks_sloth_or_amnesia_before_the_first_draw() {
    let state = at_knowledge_choice();
    let pc = state.pending_choice.as_ref().expect("choice");
    assert_eq!(pc.kind, ChoiceKind::MonsterCurseSelect);
    assert_eq!(
        pc.options.iter().map(|o| o.card_id).collect::<Vec<_>>(),
        [CardId::SlothStatus, CardId::Amnesia]
    );
    assert_eq!((pc.min_select, pc.max_select, pc.can_skip), (1, 1, false));
    assert!(state.player.hand.is_empty(), "the hand draw waits");
    assert_eq!(state.player.draw.len(), 12);
}

/// `Amnesia.cs:29-32` + `AmnesiaPower.cs:25-49`: picking Amnesia applies
/// AmnesiaPower 1: the hand draw is 2 smaller (`ModifyHandDraw`), 1 energy is
/// lost after the draw (`AfterPlayerTurnStart`), and the power is removed at
/// the player side's turn end. Turn 2 has no choice and a full hand.
#[test]
fn knowledge_demon_amnesia_costs_two_draws_and_one_energy_once() {
    let mut state = at_knowledge_choice();
    let r = apply(
        &mut state,
        Action::ResolveChoice {
            selection: smallvec![1],
        },
    );
    assert_eq!(r, StepResult::Advanced);
    assert!(state.pending_choice.is_none());
    assert_eq!(state.phase, CombatPhase::PlayerPlayPhase);
    assert_eq!(player_powers(&state), [(PowerKind::Amnesia, 1)]);
    assert_eq!(state.player.hand.len(), 3);
    assert_eq!(state.player.energy, 2);

    let _ = apply(&mut state, Action::EndTurn);
    assert_eq!(state.round, 2);
    assert!(state.pending_choice.is_none(), "turn 1 only");
    assert_eq!(player_powers(&state), []);
    assert_eq!(state.player.hand.len(), 5);
    assert_eq!(state.player.energy, 3);
}

/// `Sloth.cs:24-27`: picking Sloth applies SlothPower 3, which stays for the
/// combat; the hand and energy are untouched.
#[test]
fn knowledge_demon_sloth_stays_for_the_combat() {
    let mut state = at_knowledge_choice();
    let _ = apply(
        &mut state,
        Action::ResolveChoice {
            selection: smallvec![0],
        },
    );
    assert_eq!(player_powers(&state), [(PowerKind::Sloth, 3)]);
    assert_eq!(state.player.hand.len(), 5);
    assert_eq!(state.player.energy, 3);
    let _ = apply(&mut state, Action::EndTurn);
    assert!(state.pending_choice.is_none());
    assert_eq!(player_powers(&state), [(PowerKind::Sloth, 3)]);
}

/// `Amnesia.OnChosen` is a `PowerCmd.Apply` of a Debuff (`AmnesiaPower.cs:
/// 16`), so Artifact eats it: full hand, full energy.
#[test]
fn artifact_eats_amnesia() {
    let mut state = dread(Some(Dominion::KnowledgeDemon), &SLEEPER);
    give_player(&mut state, PowerKind::Artifact, 1);
    assert_eq!(enter(&mut state), StepResult::PendingChoice);
    let _ = apply(
        &mut state,
        Action::ResolveChoice {
            selection: smallvec![1],
        },
    );
    assert_eq!(player_powers(&state), []);
    assert_eq!(state.player.hand.len(), 5);
    assert_eq!(state.player.energy, 3);
}

// ───────────────────────────── co-op ─────────────────────────────

/// `KinDominion.cs:32-33` targets `combatRoom.CombatState.Players`: every
/// player gets the debuffs.
#[test]
fn player_dominions_reach_every_player() {
    let mut state = dread(Some(Dominion::Kin), &SLEEPER);
    state.allies.push(PlayerCombatState {
        current_hp: 70,
        max_hp: 70,
        ..PlayerCombatState::default()
    });
    sts2_core::coop::open_seats(&mut state);
    state.coop.full = true;
    assert_eq!(start_combat(&mut state), StepResult::Advanced);
    let want = [(PowerKind::Weak, 2), (PowerKind::Frail, 2)];
    assert_eq!(player_powers(&state), want);
    sts2_core::coop::with_seat(&mut state, 1, |state| {
        assert_eq!(player_powers(state), want);
    });
}

// ───────────────────────── order against relics ─────────────────────────

fn give_relic(state: &mut CombatState, kind: sts2_core::RelicKind) {
    let raw = u32::try_from(state.relics.len() + 1).expect("u32");
    state.relics.push(sts2_core::Relic {
        id: std::num::NonZeroU32::new(raw).expect("nz"),
        kind,
        counter: kind.initial_counter(),
        is_wax: false,
        is_melted: false,
    });
}

/// `RunState.cs:129-162`: the dominion is the last `AfterRoomEntered`
/// listener, so a relic that applies its power there (`Vajra.cs:21`) comes
/// before the dominion's, and one that applies in `BeforeCombatStart`
/// (`SneckoEye.cs:49`) after, whatever the obtain order.
#[test]
fn player_dominion_powers_sit_between_room_entry_and_combat_start_relics() {
    let mut state = dread(Some(Dominion::Kin), &SLEEPER);
    give_relic(&mut state, sts2_core::RelicKind::SneckoEye);
    give_relic(&mut state, sts2_core::RelicKind::Vajra);
    assert_eq!(enter(&mut state), StepResult::Advanced);
    assert_eq!(
        player_powers(&state),
        [
            (PowerKind::Strength, 1),
            (PowerKind::Weak, 2),
            (PowerKind::Frail, 2),
            (PowerKind::Confused, 1)
        ]
    );
    // Without a dominion the channel is one equip-order walk, as before.
    let mut state = dread(None, &SLEEPER);
    give_relic(&mut state, sts2_core::RelicKind::SneckoEye);
    give_relic(&mut state, sts2_core::RelicKind::Vajra);
    assert_eq!(enter(&mut state), StepResult::Advanced);
    assert_eq!(
        player_powers(&state),
        [(PowerKind::Confused, 1), (PowerKind::Strength, 1)]
    );
}

/// `PhilosophersStone.cs:52-60` applies its Strength to the enemies in
/// `AfterRoomEntered`, ahead of the dominion (the last listener) and of the
/// monster's own `AfterAddedToRoom` powers.
#[test]
fn enemy_dominion_power_follows_a_room_entry_relics() {
    let mut state = dread(Some(Dominion::CeremonialBeast), &SLEEPER);
    give_relic(&mut state, sts2_core::RelicKind::PhilosophersStone);
    assert_eq!(enter(&mut state), StepResult::Advanced);
    let got: Vec<PowerKind> = enemy_powers(&state, 0).iter().map(|p| p.0).collect();
    assert_eq!(
        got,
        [
            PowerKind::Strength,
            PowerKind::Ritual,
            PowerKind::Plating,
            PowerKind::Asleep
        ]
    );
    // TestSubject's Strength 3 stacks onto the Stone's 1.
    let mut state = dread(Some(Dominion::TestSubject), &SLEEPER);
    give_relic(&mut state, sts2_core::RelicKind::PhilosophersStone);
    assert_eq!(enter(&mut state), StepResult::Advanced);
    assert_eq!(enemy_powers(&state, 0)[0], (PowerKind::Strength, 4));
    assert_eq!(enemy_powers(&state, 0).len(), 3);
}

/// `KnowledgeDemonDominion.BeforeHandDraw` runs for each player's own hand
/// draw: a teammate answers theirs on the spot (the first option, Sloth,
/// without a teammate policy) while ours is the open choice.
#[test]
fn knowledge_demon_asks_every_player() {
    let mut state = dread(Some(Dominion::KnowledgeDemon), &SLEEPER);
    let mut mate = PlayerCombatState {
        current_hp: 70,
        max_hp: 70,
        max_energy: 3,
        ..PlayerCombatState::default()
    };
    mate.character = Some(CharacterId::Ironclad);
    state.allies.push(mate);
    sts2_core::coop::open_seats(&mut state);
    state.coop.full = true;
    assert_eq!(enter(&mut state), StepResult::PendingChoice);
    assert_eq!(player_powers(&state), []);
    sts2_core::coop::with_seat(&mut state, 1, |state| {
        assert_eq!(player_powers(state), [(PowerKind::Sloth, 3)]);
    });
    let _ = apply(
        &mut state,
        Action::ResolveChoice {
            selection: smallvec![1],
        },
    );
    assert_eq!(player_powers(&state), [(PowerKind::Amnesia, 1)]);
    assert_eq!(state.player.hand.len(), 3);
}
