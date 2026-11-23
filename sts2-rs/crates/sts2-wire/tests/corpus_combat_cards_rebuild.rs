//! Three facts a rebuilt board lost that the real game (and the engine) keep,
//! found by the 2026-11-23 Spire Bench corpus compare (Ironclad A10,
//! v0.111.0): a card's single-turn Retain after it left the hand, the
//! combat's draw count (Murder), and a local cost shaved below zero (Stomp
//! under Borrowed Time).

use sts2_core::cards::CardId;
use sts2_core::{CardEnergyCost, CardUid, CombatState, CostExpiry, Power, PowerKind, PowerUid};
use sts2_wire::emit::combat::combat_state_to_value;

fn board(deck: &[&str]) -> CombatState {
    let deck: Vec<String> = deck.iter().map(|s| (*s).to_owned()).collect();
    sts2_wire::scenario::build_scenario_state_for_version(
        sts2_core::GameVersion::V0_111_0,
        7,
        "Ironclad",
        &deck,
        80,
        80,
        &[],
        &[],
        "AXEBOTS_NORMAL",
        10,
        None,
        None,
    )
    .expect("scenario")
}

/// Take the card `id` out of whatever pile holds it and put it in the discard.
fn to_discard(s: &mut CombatState, id: CardId) -> CardUid {
    let uid = s
        .cards
        .iter()
        .find(|c| c.card_id == id)
        .expect("card on the board")
        .uid;
    s.player.hand.retain(|u| *u != uid);
    s.player.draw.retain(|u| *u != uid);
    s.player.discard.push(uid);
    uid
}

fn round_trip(s: &CombatState) -> CombatState {
    let v = combat_state_to_value(s).expect("emits");
    sts2_wire::state_from_json(&v).expect("re-parses")
}

/// C# `HasSingleTurnRetain` stays on the card until
/// `PlayerCombatState.EndOfTurnCleanup` walks every pile: an Expertise-drawn
/// Strike played this turn is still flagged in the discard. The emitter
/// writes it under `pile_flags`; the parse read only hand cards' `flags`.
#[test]
fn a_discarded_cards_single_turn_retain_survives_the_rebuild() {
    let mut s = board(&["STRIKE_IRONCLAD", "DEFEND_IRONCLAD"]);
    let strike = to_discard(&mut s, CardId::StrikeIronclad);
    s.player.retain_this_turn.push(strike);
    let back = round_trip(&s);
    let flagged: Vec<CardId> = back
        .player
        .discard
        .iter()
        .filter(|u| back.player.retain_this_turn.contains(u))
        .map(|u| back.cards[u.0.get() as usize - 1].card_id)
        .collect();
    assert_eq!(flagged, vec![CardId::StrikeIronclad]);
}

/// Murder deals 1 + the cards drawn this combat; the rebuild re-zeroed the
/// count and the card read 1 damage.
#[test]
fn the_combats_draw_count_survives_the_rebuild() {
    let mut s = board(&["MURDER", "STRIKE_IRONCLAD"]);
    s.cards_drawn = 20;
    assert_eq!(round_trip(&s).cards_drawn, 20);
}

/// The host's shape for a Stomp shaved to -1 by four Attacks this turn, under
/// Borrowed Time's +1: `cost` 0, `base_cost` 0 (clamped), `persistent_cost`
/// 3, `played_cost` 0, `stable_cost` 3 and no `cost_mods`. C# skips the
/// global hook for a negative local cost, so the card is free; rebuilt at a
/// local 0 it cost 1.
#[test]
fn a_stomp_shaved_below_zero_stays_free_under_borrowed_time() {
    let mut s = board(&["STOMP", "STRIKE_IRONCLAD"]);
    let stomp = to_discard(&mut s, CardId::Stomp);
    let idx = stomp.0.get() as usize - 1;
    let mut cost = CardEnergyCost::new(3);
    cost.add(-4, CostExpiry::Turn);
    s.cards[idx].cost = cost;
    let pid = PowerUid::new(u32::try_from(s.powers.len() + 1).expect("fits")).expect("non-zero");
    s.powers.push(Power {
        id: pid.0,
        kind: PowerKind::BorrowedTime,
        amount: 1,
        counter: 0,
        applier: None,
        skip_next_tick: false,
    });
    s.player.powers.push(pid);
    assert_eq!(
        sts2_core::hooks::effective_card_cost(&s, stomp),
        0,
        "native"
    );

    let mut wire = combat_state_to_value(&s).expect("emits");
    let card = wire["deck"]["discard_pile"]
        .as_array_mut()
        .expect("discard")
        .iter_mut()
        .find(|c| c["id"] == "STOMP")
        .expect("Stomp in the discard");
    let m = card.as_object_mut().expect("card dict");
    m.remove("cost_mods");
    for (k, v) in [
        ("cost_override", 0),
        ("base_cost", 0),
        ("persistent_cost", 3),
        ("played_cost", 0),
        ("stable_cost", 3),
    ] {
        m.insert(k.to_owned(), serde_json::json!(v));
    }
    let back = sts2_wire::state_from_json(&wire).expect("parses");
    let rebuilt = back
        .player
        .discard
        .iter()
        .copied()
        .find(|u| back.cards[u.0.get() as usize - 1].card_id == CardId::Stomp)
        .expect("rebuilt Stomp");
    assert_eq!(sts2_core::hooks::effective_card_cost(&back, rebuilt), 0);
}
