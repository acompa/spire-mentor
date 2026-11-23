//! The run's game mode and, in Dread, the act's dominion (GH #1346).
//!
//! C# `Runs/GameMode.cs` (v0.113.0): `None, Standard, Daily, Dread, Blitz`.
//! The engine models Standard, Dread and Blitz (GH #1347; see
//! [`crate::blitz`]). A Dread run is an ascension-10 run
//! (`NDreadScreen.cs:584`) with a `DreadRunModel` (`Models.Dread/
//! DreadRunModel.cs`): each act has a **dominion**, taken from the act's boss
//! (`SecondBossEncounter ?? BossEncounter`, `EncounterModel.GetDominion()`),
//! which acts on every combat of the act except the act's boss fights.

use core::fmt;

/// C# `GameMode`, as far as the engine plays it.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum GameMode {
    /// `GameMode.Standard`. Default.
    #[default]
    #[serde(rename = "standard")]
    Standard,
    /// `GameMode.Dread` (v0.113.0): ascension 10, a per-act dominion, one
    /// more Ascender's Bane on entering acts 2 and 3, and the
    /// `AscensionHelper.IsDread()` boss changes.
    #[serde(rename = "dread")]
    Dread,
    /// `GameMode.Blitz` (v0.113.0): a fixed 8-node line per act, its own
    /// room rewards, merchant and chest (`Models.Blitz/BlitzRunModel.cs`),
    /// and an ascension ladder of 0..=5 that names a SET of standard levels
    /// ([`crate::ascension::blitz_levels`]).
    #[serde(rename = "blitz")]
    Blitz,
}

impl GameMode {
    /// The wire spelling (`"standard"` / `"dread"` / `"blitz"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            GameMode::Standard => "standard",
            GameMode::Dread => "dread",
            GameMode::Blitz => "blitz",
        }
    }

    /// The C# `GameMode` member name (`"Dread"`), for messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            GameMode::Standard => "Standard",
            GameMode::Dread => "Dread",
            GameMode::Blitz => "Blitz",
        }
    }

    /// Parse the wire spelling, case-insensitively.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "standard" => Some(GameMode::Standard),
            "dread" => Some(GameMode::Dread),
            "blitz" => Some(GameMode::Blitz),
            _ => None,
        }
    }

    /// `true` for the mode every pre-Dread state is in. Digests and the wire
    /// carry the mode only when it is not the default, so they are unchanged
    /// for standard runs.
    #[must_use]
    pub fn is_default(self) -> bool {
        self == GameMode::default()
    }

    /// C# `AscensionHelper.IsDread()`.
    #[must_use]
    pub const fn is_dread(self) -> bool {
        matches!(self, GameMode::Dread)
    }

    /// C# `RunState.GameMode == GameMode.Blitz`.
    #[must_use]
    pub const fn is_blitz(self) -> bool {
        matches!(self, GameMode::Blitz)
    }

    /// The ascension level the game starts this mode at, when it fixes one:
    /// Dread is always 10 (`StartRunLobby.cs:793-797`, `NDreadScreen.cs:584`).
    #[must_use]
    pub const fn forced_ascension(self) -> Option<u8> {
        match self {
            GameMode::Standard | GameMode::Blitz => None,
            GameMode::Dread => Some(10),
        }
    }

    /// C# `AscensionManager.GetMaxAllowed(gameMode)`
    /// (`Entities.Ascension/AscensionManager.cs:24-41`): 10, or 5 in Blitz
    /// (`maxAscensionAllowedBlitz`).
    #[must_use]
    pub const fn max_ascension(self) -> u8 {
        match self {
            GameMode::Standard | GameMode::Dread => 10,
            GameMode::Blitz => 5,
        }
    }

    /// The first game build that has this mode.
    #[must_use]
    pub const fn min_game_version(self) -> crate::GameVersion {
        match self {
            GameMode::Standard => crate::GameVersion::V0_107_1,
            GameMode::Dread | GameMode::Blitz => crate::GameVersion::V0_113_0,
        }
    }
}

impl fmt::Display for GameMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A Dread dominion (`Models.Dread.Dominions/<Name>Dominion.cs`), one per
/// boss encounter.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Dominion {
    /// `VantomBoss`: Slippery 1 on every enemy (2 in an elite room).
    Vantom,
    /// `CeremonialBeastBoss`: Ritual 1 on every enemy.
    CeremonialBeast,
    /// `TheKinBoss`: Weak 2 and Frail 2 on every player.
    Kin,
    /// `WaterfallGiantBoss`: Explosive on every enemy.
    WaterfallGiant,
    /// `SoulFyshBoss`: a Beckon at a random draw-pile position.
    SoulFysh,
    /// `LagavulinMatriarchBoss`: `LagavulinMatriarchDominionPower` 1.
    LagavulinMatriarch,
    /// `TheInsatiableBoss`: `InsatiableDominionPower` 1.
    Insatiable,
    /// `KnowledgeDemonBoss`: a Sloth-or-Amnesia pick before the first hand.
    KnowledgeDemon,
    /// `KaiserCrabBoss`: Plating 7 on every enemy.
    KaiserCrab,
    /// `QueenBoss`: `YouAreMinePower` 3.
    Queen,
    /// `TestSubjectBoss`: Strength 3 on every enemy.
    TestSubject,
    /// `AeonglassBoss`: `AeonglassDominionPower` 1.
    Aeonglass,
}

impl Dominion {
    /// Every dominion, in `Models.Dread.Dominions` file order.
    pub const ALL: [Dominion; 12] = [
        Dominion::Aeonglass,
        Dominion::CeremonialBeast,
        Dominion::Insatiable,
        Dominion::KaiserCrab,
        Dominion::Kin,
        Dominion::KnowledgeDemon,
        Dominion::LagavulinMatriarch,
        Dominion::Queen,
        Dominion::SoulFysh,
        Dominion::TestSubject,
        Dominion::Vantom,
        Dominion::WaterfallGiant,
    ];

    /// The boss encounter whose `GetDominion()` returns this dominion
    /// (`Models.Encounters/<Name>Boss.cs`), as its `ModelId.Entry`.
    #[must_use]
    pub const fn boss_id(self) -> &'static str {
        match self {
            Dominion::Vantom => "VANTOM_BOSS",
            Dominion::CeremonialBeast => "CEREMONIAL_BEAST_BOSS",
            Dominion::Kin => "THE_KIN_BOSS",
            Dominion::WaterfallGiant => "WATERFALL_GIANT_BOSS",
            Dominion::SoulFysh => "SOUL_FYSH_BOSS",
            Dominion::LagavulinMatriarch => "LAGAVULIN_MATRIARCH_BOSS",
            Dominion::Insatiable => "THE_INSATIABLE_BOSS",
            Dominion::KnowledgeDemon => "KNOWLEDGE_DEMON_BOSS",
            Dominion::KaiserCrab => "KAISER_CRAB_BOSS",
            Dominion::Queen => "QUEEN_BOSS",
            Dominion::TestSubject => "TEST_SUBJECT_BOSS",
            Dominion::Aeonglass => "AEONGLASS_BOSS",
        }
    }

    /// The dominion model's `ModelId.Entry` (`<NAME>_DOMINION`), the name
    /// the oracle and the wire use.
    #[must_use]
    pub const fn entry(self) -> &'static str {
        match self {
            Dominion::Vantom => "VANTOM_DOMINION",
            Dominion::CeremonialBeast => "CEREMONIAL_BEAST_DOMINION",
            Dominion::Kin => "KIN_DOMINION",
            Dominion::WaterfallGiant => "WATERFALL_GIANT_DOMINION",
            Dominion::SoulFysh => "SOUL_FYSH_DOMINION",
            Dominion::LagavulinMatriarch => "LAGAVULIN_MATRIARCH_DOMINION",
            Dominion::Insatiable => "INSATIABLE_DOMINION",
            Dominion::KnowledgeDemon => "KNOWLEDGE_DEMON_DOMINION",
            Dominion::KaiserCrab => "KAISER_CRAB_DOMINION",
            Dominion::Queen => "QUEEN_DOMINION",
            Dominion::TestSubject => "TEST_SUBJECT_DOMINION",
            Dominion::Aeonglass => "AEONGLASS_DOMINION",
        }
    }

    /// C# `EncounterModel.GetDominion()` for the boss encounter `boss_id`;
    /// `None` for any other encounter (the base returns null).
    #[must_use]
    pub fn for_boss(boss_id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.boss_id() == boss_id)
    }

    /// The dominion named `entry` ([`Self::entry`]).
    #[must_use]
    pub fn from_entry(entry: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.entry() == entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_mode_round_trips_its_wire_name() {
        for mode in [GameMode::Standard, GameMode::Dread, GameMode::Blitz] {
            assert_eq!(GameMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(GameMode::parse("Dread"), Some(GameMode::Dread));
        assert_eq!(GameMode::parse("Blitz"), Some(GameMode::Blitz));
        assert_eq!(GameMode::parse("daily"), None);
        assert!(GameMode::Standard.is_default() && !GameMode::Dread.is_default());
        assert!(!GameMode::Blitz.is_default());
    }

    /// `AscensionManager.cs:24-41`: Blitz tops out at 5 and fixes no level;
    /// both v0.113.0 modes are refused on older builds.
    #[test]
    fn blitz_is_a_v0113_mode_with_a_five_level_ladder() {
        assert!(GameMode::Blitz.is_blitz() && !GameMode::Blitz.is_dread());
        assert_eq!(GameMode::Blitz.forced_ascension(), None);
        assert_eq!(GameMode::Blitz.max_ascension(), 5);
        assert_eq!(GameMode::Standard.max_ascension(), 10);
        assert_eq!(
            GameMode::Blitz.min_game_version(),
            crate::GameVersion::V0_113_0
        );
        assert_eq!(
            GameMode::Standard.min_game_version(),
            crate::GameVersion::V0_107_1
        );
    }

    #[test]
    fn dread_is_an_ascension_10_run() {
        assert_eq!(GameMode::Dread.forced_ascension(), Some(10));
        assert_eq!(GameMode::Standard.forced_ascension(), None);
    }

    /// One dominion per boss, each found by its boss and by its entry name
    /// (`Models.Encounters/*Boss.cs` `GetDominion()`).
    #[test]
    fn every_boss_has_its_dominion() {
        for d in Dominion::ALL {
            assert_eq!(Dominion::for_boss(d.boss_id()), Some(d));
            assert_eq!(Dominion::from_entry(d.entry()), Some(d));
            assert!(d.entry().ends_with("_DOMINION"));
        }
        assert_eq!(Dominion::for_boss("QUEEN_BOSS"), Some(Dominion::Queen));
        assert_eq!(Dominion::for_boss("THE_KIN_BOSS"), Some(Dominion::Kin));
        assert_eq!(
            Dominion::for_boss("THE_INSATIABLE_BOSS"),
            Some(Dominion::Insatiable)
        );
        assert_eq!(Dominion::for_boss("SLIMES_NORMAL"), None);
    }
}
