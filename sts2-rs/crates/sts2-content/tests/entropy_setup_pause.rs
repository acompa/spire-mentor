//! Entropy's turn-start pick pauses `SetupPlayerTurn`, so `AfterSideTurnStart`
//! (Furnace's Forge) runs before the transform (GH #1642).
//!
//! `EntropyPower.AfterPlayerTurnStart` opens `CardSelectCmd.FromHand`. When the
//! hand holds more cards than Entropy's amount, the select is a real screen
//! and signals `SignalPlayerChoiceBegun` (the game always does; the headless
//! host does through its `SignalledSelection`). `CombatManager.StartTurn`
//! waits only for that pause (`WaitForPauseOrCompletionWithoutAssigningTask`,
//! v0.111.0 `CombatManager.cs:779-785`), then runs `Hook.AfterSideTurnStart`
//! while the screen is open. So with a retained Sovereign Blade first in hand,
//! Furnace forges onto that blade, and only then does Entropy transform it.
//! Transforming first left no blade, and Furnace then forged a fresh one into
//! the hand (Regent AP1000, v0.111.0).
//!
//! That is agent mode (`CombatState::interactive_turn_start_picks`, the host's
//! `STS2_INTERACTIVE_TURN_START_PICKS`). In parity mode the host patches
//! Entropy to auto-take synchronously, and a hand no larger than Entropy's
//! amount is auto-taken without a screen (`FromHand`'s `Count <= MinSelect`
//! fast path): in both, the transform still runs before Furnace.

use smallvec::SmallVec;
use std::num::NonZeroU32;

use sts2_content::cards::dispatch as content_dispatch;
use sts2_content::cards::v0111::card_pod_for;
use sts2_core::cards::CardId;
use sts2_core::potions::CharacterId;
use sts2_core::{
    CombatPhase, CombatState, Creature, CreatureUid, GameVersion, IntentHistory, MonsterId,
    MonsterKind, Power, PowerKind, PowerUid,
};

fn fresh(version: GameVersion) -> CombatState {
    content_dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();
    let mut state = CombatState {
        game_version: version,
        ..CombatState::default()
    };
    let id = NonZeroU32::new(1).expect("nz");
    state.creatures.push(Creature {
        id,
        current_hp: 500,
        max_hp: 500,
        block: 0,
        powers: SmallVec::new(),
        species: Some(MonsterKind::Nibbit),
        intent: None,
        intent_history: IntentHistory::default(),
        monster_id: MonsterId("Nibbit"),
        ai: None,
    });
    state.enemy_uids.push(CreatureUid(id));
    state.player.current_hp = 80;
    state.player.max_hp = 80;
    state.player.energy = 3;
    state.player.max_energy = 3;
    state.player.character = Some(CharacterId::Regent);
    state.player.potion_slots = 3;
    let _ = state.init_player_creature();
    state
}

fn new_card(state: &mut CombatState, id: CardId) -> sts2_core::CardUid {
    let raw = u32::try_from(state.cards.len() + 1).expect("u32");
    let uid = sts2_core::CardUid::new(raw).expect("nz");
    let mut card = card_pod_for(id, false, state.game_version).expect("spec");
    card.uid = uid;
    state.cards.push(card);
    uid
}

fn add_player_power(state: &mut CombatState, raw: u32, kind: PowerKind, amount: i32) {
    let pid = NonZeroU32::new(raw).expect("nz");
    state.powers.push(Power::new(pid, kind, amount));
    state.player.powers.push(PowerUid(pid));
}

fn blades(state: &CombatState) -> Vec<sts2_core::CardUid> {
    state
        .player
        .hand
        .iter()
        .chain(state.player.draw.iter())
        .chain(state.player.discard.iter())
        .copied()
        .filter(|&u| state.cards[u.0.get() as usize - 1].card_id == CardId::SovereignBlade)
        .collect()
}

/// A retained blade first in hand, Furnace 10 then Entropy 1, and `drawn`
/// Strikes in the draw pile. Runs the turn start.
fn turn_start(
    version: GameVersion,
    drawn: usize,
    interactive: bool,
) -> (CombatState, sts2_core::CardUid) {
    let mut state = fresh(version);
    state.interactive_turn_start_picks = interactive;
    let blade = new_card(&mut state, CardId::SovereignBlade);
    state.player.hand.push(blade);
    for _ in 0..drawn {
        let uid = new_card(&mut state, CardId::StrikeRegent);
        state.player.draw.push(uid);
    }
    add_player_power(&mut state, 40, PowerKind::Furnace, 10);
    add_player_power(&mut state, 41, PowerKind::Entropy, 1);
    state.phase = CombatPhase::PlayerTurnStart;
    state.round = 2;
    let _ = sts2_core::combat_loop::start_player_turn(&mut state);
    assert!(state.pending_choice.is_none(), "{:?}", state.pending_choice);
    (state, blade)
}

fn furnace_forges_before_entropy_transforms(version: GameVersion) {
    let (state, blade) = turn_start(version, 5, true);
    assert!(
        blades(&state).is_empty(),
        "{version:?}: Furnace forged onto the held blade, then Entropy transformed it; \
         no fresh blade (hand {:?})",
        state
            .player
            .hand
            .iter()
            .map(|u| state.cards[u.0.get() as usize - 1].card_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        state.player.hand.len(),
        6,
        "{version:?}: the blade's slot + 5 drawn"
    );
    assert_ne!(
        state.player.hand[0], blade,
        "{version:?}: the blade was transformed"
    );
}

#[test]
fn furnace_forges_before_entropy_transforms_v0111() {
    furnace_forges_before_entropy_transforms(GameVersion::V0_111_0);
}

#[test]
fn furnace_forges_before_entropy_transforms_v0107() {
    furnace_forges_before_entropy_transforms(GameVersion::V0_107_1);
}

/// Entropy transforms the blade before Furnace, which then forges a fresh
/// blade at 10 + 10, holding `hand` cards in all.
fn assert_transform_then_fresh_blade(state: &CombatState, blade: sts2_core::CardUid, hand: usize) {
    let now = blades(state);
    assert_eq!(now.len(), 1, "a fresh blade from Furnace");
    assert_ne!(now[0], blade);
    assert_eq!(state.cards[now[0].0.get() as usize - 1].base_damage, 20);
    assert_eq!(state.player.hand.len(), hand);
}

/// No screen (the hand is the blade alone).
#[test]
fn entropy_auto_take_transforms_before_furnace() {
    let (state, blade) = turn_start(GameVersion::V0_111_0, 0, true);
    assert_transform_then_fresh_blade(&state, blade, 2);
}

/// Parity mode: the host's Entropy prefix auto-takes with no pause.
#[test]
fn parity_mode_entropy_transforms_before_furnace() {
    let (state, blade) = turn_start(GameVersion::V0_111_0, 5, false);
    assert_transform_then_fresh_blade(&state, blade, 7);
}
