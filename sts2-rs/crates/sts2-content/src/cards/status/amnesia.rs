//! Amnesia — v0.113.0 Dread token (`Models.Cards/Amnesia.cs`, GH #1346).
use super::prelude::*;
use crate::cards::types::{CardKeywords, CardRarity};
use sts2_core::cards::{CardType, TargetType};

/// Amnesia (Status, cost -1, `MaxUpgradeLevel 0`, `CanBeGeneratedInCombat
/// false`). Never in a pile: `KnowledgeDemonDominion.BeforeHandDraw` offers
/// it beside Sloth and calls `OnChosen` on the pick, which applies
/// `AmnesiaPower` 1 (`sts2_core::dread`). Playing it is a no-op, like the
/// other `KnowledgeDemon.IChoosable` tokens.
pub(crate) fn play_amnesia(_s: &mut CombatState, _t: Option<CreatureUid>) -> EffectResult {
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Amnesia,
        name: "AMNESIA",
        cost: -1,
        card_type: CardType::Status,
        target_type: TargetType::None,
        rarity: CardRarity::Status,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::UNPLAYABLE,
        ..CardSpec::base()
    }
}
