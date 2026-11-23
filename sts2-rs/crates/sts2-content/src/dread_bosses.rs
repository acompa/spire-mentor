//! v0.113.0 Dread mode: the `AscensionHelper.IsDread()` values of the bosses
//! (GH #1346).
//!
//! C# reads `IsDread()` (`Helpers/AscensionHelper.cs:61-64`, a run-global
//! `RunManager.Instance.IsDread()`) inside 13 monster models and
//! `TheKinBoss`. Each read adds to, or replaces, a value the standard
//! ascension-10 run already has, so the engine keeps the standard tables and
//! applies the Dread difference on top:
//!
//! * a move's numbers, [`DREAD_MOVE_DELTAS`]: [`apply_dread_values`] patches
//!   the telegraph when [`crate::monster_dispatch::build_ai_for_mode`] builds
//!   the AI, and the move effects read the same row through [`power_bonus`] /
//!   [`block_bonus`], so the two cannot drift;
//! * HP and `AfterAddedToRoom` powers:
//!   [`sts2_core::MonsterKind::dread_hp_bonus`],
//!   [`sts2_core::MonsterKind::on_spawn_self_buff_amount_in`],
//!   [`sts2_core::MonsterKind::dread_on_spawn_self_buffs`];
//! * the Queen's opener, the third Kin follower, SoulFysh's hand Beckon,
//!   Aeonglass's Withers, the beast's Ritual and KnowledgeDemon's
//!   Disintegration values: at their own sites, each reading
//!   `CombatState::game_mode`.
//!
//! Everything here is a no-op in a standard run.

use sts2_core::monster_ai::{MonsterAi, MonsterState};
use sts2_core::monsters::Intent;
use sts2_core::{CombatState, GameMode, MonsterKind, PowerKind};

/// What a Dread run changes about one move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delta {
    /// Added to the move's attack damage.
    Attack(i32),
    /// Added to the move's base hit count.
    Hits(u8),
    /// Added to the block the move gains.
    Block(i32),
    /// Added to the amount of the power the move gives its user or its
    /// teammates.
    Buff(PowerKind, i32),
    /// Added to the magnitude of the power the move puts on the players.
    Debuff(PowerKind, i32),
}

/// Every per-move `IsDread()` number, by move id of the v0.113.0 table.
/// "Standard" in the comments is the ascension-10 value.
pub(crate) const DREAD_MOVE_DELTAS: &[(MonsterKind, &str, Delta)] = &[
    // `Vantom.cs:72-82` `PrepareStrength`: 2 -> 3.
    (
        MonsterKind::Vantom,
        "PREPARE",
        Delta::Buff(PowerKind::Strength, 1),
    ),
    // `CeremonialBeast.cs:63-73` `PlowStrength`: 2 -> 3.
    (
        MonsterKind::CeremonialBeast,
        "PLOW",
        Delta::Buff(PowerKind::Strength, 1),
    ),
    // `WaterfallGiant.cs:85-95` `SteamBoostAmount`: 3 -> 4, in the five moves
    // that apply it (`:251`, `:262`, `:272`, `:281`, `:292`). PRESSURIZE
    // applies `PressurizeAmount`, which has no Dread term.
    (
        MonsterKind::WaterfallGiant,
        "PRESSURE_UP",
        Delta::Buff(PowerKind::SteamEruption, 1),
    ),
    (
        MonsterKind::WaterfallGiant,
        "STOMP",
        Delta::Buff(PowerKind::SteamEruption, 1),
    ),
    (
        MonsterKind::WaterfallGiant,
        "RAM",
        Delta::Buff(PowerKind::SteamEruption, 1),
    ),
    (
        MonsterKind::WaterfallGiant,
        "SIPHON",
        Delta::Buff(PowerKind::SteamEruption, 1),
    ),
    (
        MonsterKind::WaterfallGiant,
        "PRESSURE_GUN",
        Delta::Buff(PowerKind::SteamEruption, 1),
    ),
    // `LagavulinMatriarch.cs:55` `Slash2Damage`: 14 -> 16.
    (MonsterKind::LagavulinMatriarch, "SLASH2", Delta::Attack(2)),
    // `LagavulinMatriarch.cs:57` `Slash2Block`: 14 -> 16.
    (MonsterKind::LagavulinMatriarch, "SLASH2", Delta::Block(2)),
    // `LagavulinMatriarch.cs:63-73` `DebuffAmount`: -2 -> -3, on the
    // player's Strength and Dexterity. The self Strength stays 2.
    (
        MonsterKind::LagavulinMatriarch,
        "SOUL_SIPHON",
        Delta::Debuff(PowerKind::Strength, 1),
    ),
    // `TheInsatiable.cs:68-78` `SandpitAmount`: 4 -> 3 (a countdown).
    (
        MonsterKind::TheInsatiable,
        "LIQUIFY_GROUND",
        Delta::Buff(PowerKind::Sandpit, -1),
    ),
    // `Rocket.cs:62` `RocketClawDamage`: 35 -> 40.
    (MonsterKind::Rocket, "ROCKET_CLAW_MOVE", Delta::Attack(5)),
    // `Queen.cs:205` BURN_BRIGHT_FOR_ME teammate Strength: 1 -> 2.
    (
        MonsterKind::Queen,
        "BURN_BRIGHT_FOR_ME_MOVE",
        Delta::Buff(PowerKind::Strength, 1),
    ),
    // `TorchHeadAmalgam.cs:32` `StrongTackleDamage`: 32 -> 37.
    (
        MonsterKind::TorchHeadAmalgam,
        "STRONG_TACKLE",
        Delta::Attack(5),
    ),
    // `TestSubject.cs:95-105` `BaseMultiClawCount`: 3 -> 4.
    (MonsterKind::TestSubject, "MULTI_CLAW", Delta::Hits(1)),
    // `TestSubject.cs:113` `BurningGrowlStrengthGain`: 3 -> 6.
    (
        MonsterKind::TestSubject,
        "BURNING_GROWL",
        Delta::Buff(PowerKind::Strength, 3),
    ),
];

fn rows(kind: MonsterKind, move_id: &str) -> impl Iterator<Item = Delta> + '_ {
    DREAD_MOVE_DELTAS
        .iter()
        .filter(move |r| r.0 == kind && r.1 == move_id)
        .map(|r| r.2)
}

/// Patch `ai` to the moves C# telegraphs in a Dread run: every
/// [`DREAD_MOVE_DELTAS`] row of `kind`, on top of the ascension values
/// already in the table. A no-op outside Dread and for a kind with no row
/// (the shared prototype table is then left alone).
pub fn apply_dread_values(ai: &mut MonsterAi, kind: MonsterKind, mode: GameMode) {
    if !mode.is_dread() || !DREAD_MOVE_DELTAS.iter().any(|r| r.0 == kind) {
        return;
    }
    for state in std::sync::Arc::make_mut(&mut ai.states).values_mut() {
        let MonsterState::Move(m) = state else {
            continue;
        };
        for delta in rows(kind, m.id) {
            for intent in &mut m.intents {
                match (delta, intent) {
                    (
                        Delta::Attack(d),
                        Intent::Attack { damage } | Intent::MultiAttack { damage, .. },
                    ) => *damage += d,
                    (Delta::Hits(d), Intent::MultiAttack { hits, .. }) => {
                        *hits = hits.saturating_add(d);
                    }
                    (Delta::Block(d), Intent::Defend { block }) => *block += d,
                    (Delta::Buff(k, d), Intent::Buff { kind, amount }) if *kind == k => {
                        *amount += d;
                    }
                    (Delta::Debuff(k, d), Intent::Debuff { kind, amount }) if *kind == k => {
                        *amount += d;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// What a Dread fight adds to the `power` amount move `move_id` of `kind`
/// applies (its [`Delta::Buff`] or [`Delta::Debuff`] row); `0` in a standard
/// fight. A debuff's bonus is a magnitude: the Matriarch's SOUL_SIPHON
/// applies `-(2 + bonus)`.
#[must_use]
pub(crate) fn power_bonus(
    state: &CombatState,
    kind: MonsterKind,
    move_id: &str,
    power: PowerKind,
) -> i32 {
    if !state.game_mode.is_dread() {
        return 0;
    }
    rows(kind, move_id)
        .find_map(|d| match d {
            Delta::Buff(k, v) | Delta::Debuff(k, v) if k == power => Some(v),
            _ => None,
        })
        .unwrap_or(0)
}

/// What a Dread fight adds to the block move `move_id` of `kind` gains (its
/// [`Delta::Block`] row); `0` in a standard fight.
#[must_use]
pub(crate) fn block_bonus(state: &CombatState, kind: MonsterKind, move_id: &str) -> i32 {
    if !state.game_mode.is_dread() {
        return 0;
    }
    rows(kind, move_id)
        .find_map(|d| match d {
            Delta::Block(v) => Some(v),
            _ => None,
        })
        .unwrap_or(0)
}
