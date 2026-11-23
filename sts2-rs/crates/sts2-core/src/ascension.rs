//! Ascension-level stat scaling, ported 1:1 from C#
//! `../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Helpers/AscensionHelper.cs`.
//!
//! C# encodes per-monster ascension scaling inline at every stat via
//! `AscensionHelper.GetValueIfAscension(level, ascensionValue, fallbackValue)`,
//! which returns `ascensionValue` iff `RunManager.HasAscension(level)` —
//! and `AscensionManager.HasLevel(level)` is `_level >= (int)level`
//! (`../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Entities.Ascension/AscensionManager.cs:24`).
//! So the threshold is the enum *discriminant*: a stat tagged
//! `ToughEnemies` (discriminant 8) scales at ascension `>= 8`,
//! `DeadlyEnemies` (discriminant 9) at `>= 9`.
//!
//! Only two of the eleven `AscensionLevel` variants touch monster stats:
//! `ToughEnemies` (monster HP) and `DeadlyEnemies` (monster attack
//! damage). The remaining variants affect the run/map/shop layer, not
//! combat-side creature stats, so they are intentionally omitted here —
//! add them if a combat-side consumer appears.

/// C# `AscensionLevel` (`Entities.Ascension/AscensionLevel.cs`). The
/// discriminant is the enum ordinal, which in a standard (or Dread) run is
/// exactly the ascension level at which the tier activates (`HasLevel` is
/// `_level >= (int)level` there). A Blitz run holds a SET of levels instead:
/// see [`blitz_levels`] and [`has_level`].
///
/// `ToughEnemies` and `DeadlyEnemies` are the two that scale combat-side
/// monster stats; the rest act on the run, map, shop and reward layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AscensionLevel {
    /// More elites on the map.
    SwarmingElites = 1,
    /// Ancients heal 80% of missing HP.
    WearyTraveler = 2,
    /// Reward gold x0.75.
    Poverty = 3,
    /// One potion slot fewer.
    TightBelt = 4,
    /// Ascender's Bane in the starting deck.
    AscendersBane = 5,
    /// Dearer card removal.
    Inflation = 6,
    /// Rarer rares and upgrades.
    Scarcity = 7,
    /// `AscensionLevel.ToughEnemies` — monster HP scaling, active at
    /// ascension `>= 8`.
    ToughEnemies = 8,
    /// `AscensionLevel.DeadlyEnemies` — monster attack-damage scaling,
    /// active at ascension `>= 9`.
    DeadlyEnemies = 9,
    /// A second boss in the final act.
    DoubleBoss = 10,
}

/// C# `AscensionHelper.GetLevelForBlitz` (`Helpers/AscensionHelper.cs:66-78`,
/// v0.113.0) for Blitz levels 1..=5, in ladder order: a Blitz run at level
/// `n` holds the first `n` of these (`AscensionManager.cs:57-64`).
pub const BLITZ_LADDER: [AscensionLevel; 5] = [
    AscensionLevel::WearyTraveler,
    AscensionLevel::TightBelt,
    AscensionLevel::ToughEnemies,
    AscensionLevel::DeadlyEnemies,
    AscensionLevel::AscendersBane,
];

/// The standard levels a Blitz run at Blitz level `blitz_level` holds
/// (`AscensionManager` constructor, `AscensionManager.cs:57-64`). Levels
/// above 5 are clamped (`ClampToMode`, `:47-50`).
#[must_use]
pub fn blitz_levels(blitz_level: u8) -> &'static [AscensionLevel] {
    &BLITZ_LADDER[..usize::from(blitz_level).min(BLITZ_LADDER.len())]
}

/// C# `AscensionManager.HasLevel(level)` (`AscensionManager.cs:71-74`):
/// whether a run in `mode` at ascension `ascension` has `level` active.
///
/// Outside Blitz the manager holds levels `1..=ascension`, so this is
/// `ascension >= level` — the comparison every site made before Blitz
/// (GH #1347), bit for bit. In Blitz `ascension` is the Blitz level 0..=5
/// and membership is in [`blitz_levels`]: Blitz 3 has `ToughEnemies` but
/// none of `Poverty`, `Inflation`, `Scarcity`.
#[must_use]
pub fn has_level(mode: crate::GameMode, ascension: u8, level: AscensionLevel) -> bool {
    if mode.is_blitz() {
        blitz_levels(ascension).contains(&level)
    } else {
        ascension >= level as u8
    }
}

/// The standard-ladder ascension that answers `>= level` correctly for every
/// level in `reads`, for a run in `mode` at `ascension` — what to hand a
/// consumer that still thresholds a plain number and only ever asks about
/// `reads`.
///
/// Outside Blitz this is `ascension` itself, unchanged. In Blitz it is the
/// highest of `reads` the run holds (0 for none). That is exact only when the
/// held members of `reads` are a prefix of `reads` in ascending order, which
/// is asserted: it holds for `[ToughEnemies, DeadlyEnemies]` (combat) and for
/// any single level, the only uses.
#[must_use]
pub fn threshold_view(mode: crate::GameMode, ascension: u8, reads: &[AscensionLevel]) -> u8 {
    if !mode.is_blitz() {
        return ascension;
    }
    let mut view = 0u8;
    let mut gap = false;
    for &level in reads {
        debug_assert!(level as u8 > view, "`reads` must ascend");
        if has_level(mode, ascension, level) {
            debug_assert!(!gap, "Blitz levels are not a threshold over {reads:?}");
            view = level as u8;
        } else {
            gap = true;
        }
    }
    view
}

/// [`threshold_view`] for a combat: `CombatState::ascension` is read only
/// against `ToughEnemies` (monster HP, some blocks and attacks) and
/// `DeadlyEnemies` (attacks, buffs). A Blitz run at level 3 fights at 8, at
/// level 4 or 5 at 9, below that at 0.
#[must_use]
pub fn combat_ascension(mode: crate::GameMode, ascension: u8) -> u8 {
    threshold_view(
        mode,
        ascension,
        &[AscensionLevel::ToughEnemies, AscensionLevel::DeadlyEnemies],
    )
}

/// Port of C# `AscensionHelper.GetValueIfAscension(level, ascensionValue,
/// fallbackValue)`: return `asc_value` when the run's `ascension` has
/// reached `level`'s threshold, else `base_value`.
///
/// Faithful to the C# argument order — `asc_value` (the scaled value)
/// first, `base_value` (the non-ascension fallback) second — so a
/// transcription from a C# `GetValueIfAscension(Tier, A, B)` call maps
/// directly to `get_value_if_ascension(asc, Tier, A, B)`.
#[must_use]
pub fn get_value_if_ascension(
    ascension: u8,
    level: AscensionLevel,
    asc_value: i32,
    base_value: i32,
) -> i32 {
    if ascension >= level as u8 {
        asc_value
    } else {
        base_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tough_enemies_hp_scales_at_ascension_8() {
        // C# FuzzyWurmCrawler MinInitialHp =>
        // GetValueIfAscension(ToughEnemies, 58, 55).
        let f = |asc| get_value_if_ascension(asc, AscensionLevel::ToughEnemies, 58, 55);
        assert_eq!(f(0), 55, "A0 uses the non-ascension fallback");
        assert_eq!(f(7), 55, "A7 is still below the ToughEnemies threshold");
        assert_eq!(f(8), 58, "A8 activates ToughEnemies");
        assert_eq!(f(10), 58, "A10 (>= 8) keeps the scaled value");
    }

    #[test]
    fn deadly_enemies_damage_scales_at_ascension_9() {
        // C# FuzzyWurmCrawler AcidGoopDamage =>
        // GetValueIfAscension(DeadlyEnemies, 6, 4).
        let f = |asc| get_value_if_ascension(asc, AscensionLevel::DeadlyEnemies, 6, 4);
        assert_eq!(f(0), 4, "A0 uses the non-ascension fallback");
        assert_eq!(f(8), 4, "A8 is below the DeadlyEnemies threshold");
        assert_eq!(f(9), 6, "A9 activates DeadlyEnemies");
        assert_eq!(f(10), 6, "A10 (>= 9) keeps the scaled value");
    }

    /// `AscensionHelper.GetLevelForBlitz` (`AscensionHelper.cs:66-78`) and
    /// the `AscensionManager` constructor (`AscensionManager.cs:57-64`):
    /// Blitz level n holds the first n of WearyTraveler, TightBelt,
    /// ToughEnemies, DeadlyEnemies, AscendersBane, and nothing else.
    #[test]
    fn blitz_levels_are_a_set_not_a_threshold() {
        use crate::GameMode::{Blitz, Dread, Standard};
        use AscensionLevel::*;
        let all = [
            SwarmingElites,
            WearyTraveler,
            Poverty,
            TightBelt,
            AscendersBane,
            Inflation,
            Scarcity,
            ToughEnemies,
            DeadlyEnemies,
            DoubleBoss,
        ];
        let ladder = [
            WearyTraveler,
            TightBelt,
            ToughEnemies,
            DeadlyEnemies,
            AscendersBane,
        ];
        for blitz in 0u8..=5 {
            for level in all {
                let want = ladder[..usize::from(blitz)].contains(&level);
                assert_eq!(has_level(Blitz, blitz, level), want, "{blitz} {level:?}");
            }
        }
        // Never in Blitz, whatever the level.
        for level in [SwarmingElites, Poverty, Inflation, Scarcity, DoubleBoss] {
            assert!(!has_level(Blitz, 5, level));
        }
        // Standard and Dread stay the plain threshold on the ordinal.
        for mode in [Standard, Dread] {
            for asc in 0u8..=10 {
                for level in all {
                    assert_eq!(has_level(mode, asc, level), asc >= level as u8);
                }
            }
        }
    }

    /// The number a combat thresholds: unchanged outside Blitz; in Blitz the
    /// ToughEnemies / DeadlyEnemies part of the set.
    #[test]
    fn combat_ascension_carries_only_the_enemy_levels_in_blitz() {
        use crate::GameMode::{Blitz, Dread, Standard};
        for asc in 0u8..=10 {
            assert_eq!(combat_ascension(Standard, asc), asc);
            assert_eq!(combat_ascension(Dread, asc), asc);
        }
        let got: Vec<u8> = (0u8..=5).map(|b| combat_ascension(Blitz, b)).collect();
        assert_eq!(got, [0, 0, 0, 8, 9, 9]);
        assert_eq!(
            threshold_view(Blitz, 5, &[AscensionLevel::Scarcity]),
            0,
            "Blitz never has Scarcity"
        );
        assert_eq!(
            threshold_view(Blitz, 2, &[AscensionLevel::TightBelt]),
            AscensionLevel::TightBelt as u8
        );
    }
}
