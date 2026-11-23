//! Dread mode's dominions in combat (v0.113.0, GH #1346).
//!
//! C# `Models.Dread/DominionModel.cs` and `Models.Dread.Dominions/*.cs`: the
//! act's dominion is the LAST hook listener of the run and of the combat
//! (`RunState.cs:129-162`, `CombatState.cs:115-188`). It acts at four points:
//!
//! * `AfterRoomEntered` (run-level walk, after every relic, BEFORE the
//!   monsters' own `AfterAddedToRoom` powers and every `BeforeCombatStart`):
//!   [`after_room_entered_enemies`] and [`after_room_entered_player`];
//! * `AfterCreatureAddedToCombat`, for an enemy summoned mid-combat:
//!   [`after_enemy_added_to_combat`];
//! * `BeforeCombatStart` (after every other listener's, before the `Late`
//!   pass): [`before_combat_start`];
//! * `BeforeHandDraw` on the first turn: [`knowledge_demon_before_hand_draw`].
//!
//! Everything here returns at once when [`CombatState::dominion`] is `None`,
//! which it is in a standard run and in the act's boss fights.

use smallvec::SmallVec;

use crate::card_effects::apply_power_to_creature;
use crate::cards::CardId;
use crate::{CombatPhase, CombatState, CreatureUid, Dominion, PowerKind, PowerUid};

/// `WaterfallGiantDominion.GetExplosiveAmount` (`WaterfallGiantDominion.cs:
/// 44-59`): 3 when the encounter starts with more than one monster
/// (`MonstersWithSlots.Count`, fixed at `GenerateMonstersWithSlots`), else 4
/// for an `IsWeak` encounter, else 7 in an Elite room, else 5.
#[must_use]
pub const fn explosive_amount(monsters_at_entry: usize, is_weak: bool, is_elite: bool) -> i32 {
    if monsters_at_entry > 1 {
        3
    } else if is_weak {
        4
    } else if is_elite {
        7
    } else {
        5
    }
}

/// The enemy buff a dominion gives an enemy that is in the room at combat
/// entry (`entry == true`) or summoned later.
///
/// * `CeremonialBeastDominion.cs:25-28`, `KaiserCrabDominion.cs:25-28`,
///   `TestSubjectDominion.cs:23-26`: `AfterEnemiesEnteredCombat`, the same
///   amount both times.
/// * `VantomDominion.cs:25-46`: Slippery 1, or 2 at entry when the
///   encounter's `RoomType` is Elite; a summon always gets 1.
/// * `WaterfallGiantDominion.cs:22-42`: Explosive
///   [`explosive_amount`]; the entry path takes every enemy, the summon path
///   skips a creature that already holds `MinionPower`.
fn enemy_buff(state: &CombatState, uid: CreatureUid, entry: bool) -> Option<(PowerKind, i32)> {
    match state.dominion? {
        Dominion::CeremonialBeast => Some((PowerKind::Ritual, 1)),
        Dominion::KaiserCrab => Some((PowerKind::Plating, 7)),
        Dominion::TestSubject => Some((PowerKind::Strength, 3)),
        Dominion::Vantom => Some((
            PowerKind::Slippery,
            if entry && state.is_elite { 2 } else { 1 },
        )),
        Dominion::WaterfallGiant => {
            if !entry && creature_has(state, uid, PowerKind::Minion) {
                return None;
            }
            Some((PowerKind::Explosive, summon_explosive_amount(state, uid)))
        }
        _ => None,
    }
}

fn creature_has(state: &CombatState, uid: CreatureUid, kind: PowerKind) -> bool {
    crate::creature_by_id(&state.creatures, uid.0).is_some_and(|c| {
        c.powers
            .iter()
            .any(|p| state.powers.get_by_id(p.0).is_some_and(|p| p.kind == kind))
    })
}

/// The Explosive amount of this combat. `start_combat` stamps it at entry
/// ([`crate::TurnEvents::dread_explosive`]); a state rebuilt mid-combat
/// without it reads the amount off an enemy that already holds the power, and
/// failing that off the live roster.
fn summon_explosive_amount(state: &CombatState, new: CreatureUid) -> i32 {
    let stamped = i32::from(state.combat_log.dread_explosive);
    if stamped > 0 {
        return stamped;
    }
    let held = state
        .enemy_uids
        .iter()
        .filter(|u| **u != new)
        .filter_map(|u| crate::creature_by_id(&state.creatures, u.0))
        .flat_map(|c| c.powers.iter())
        .filter_map(|p| state.powers.get_by_id(p.0))
        .find(|p| p.kind == PowerKind::Explosive && p.amount > 0)
        .map(|p| p.amount);
    held.unwrap_or_else(|| {
        let others = state.enemy_uids.iter().filter(|u| **u != new).count();
        explosive_amount(
            others,
            state.combat_log.dread_weak_encounter,
            state.is_elite,
        )
    })
}

/// Each enemy's powers and their amounts, read before the room-entry walk:
/// what [`after_room_entered_enemies`] compares against to find the powers
/// that walk applied.
pub(crate) type EnemyPowerMarks = SmallVec<[(CreatureUid, SmallVec<[(PowerUid, i32); 4]>); 8]>;

/// See [`EnemyPowerMarks`].
pub(crate) fn enemy_power_marks(state: &CombatState) -> EnemyPowerMarks {
    state
        .enemy_uids
        .iter()
        .filter_map(|&uid| {
            let c = crate::creature_by_id(&state.creatures, uid.0)?;
            let marks = c
                .powers
                .iter()
                .filter_map(|p| Some((*p, state.powers.get_by_id(p.0)?.amount)))
                .collect();
            Some((uid, marks))
        })
        .collect()
}

/// Move the power instance `power` of `uid` to index `front` of its list
/// (a no-op when it already sits before `front`), and return the next front.
fn hoist(state: &mut CombatState, uid: CreatureUid, power: PowerUid, front: usize) -> usize {
    let Some(idx) = crate::creature_pos(&state.creatures, uid.0) else {
        return front;
    };
    let list = &mut state.creatures[idx].powers;
    match list.iter().position(|p| *p == power) {
        Some(at) if at >= front => {
            let p = list.remove(at);
            list.insert(front.min(list.len()), p);
            front + 1
        }
        _ => front,
    }
}

/// `DominionModel.AfterRoomEntered` for the enemies in the room
/// (`DominionModel.cs:62-71` and the `VantomDominion` / `WaterfallGiant
/// Dominion` overrides). Called once from `start_combat`, after the
/// room-entry relics.
///
/// C# runs `AfterRoomEntered` (relics, then the dominion) BEFORE the
/// monsters' own `AfterAddedToRoom` (`CombatRoom.cs:200-234`,
/// `CombatManager.cs:608-643`), while this engine gives a monster its start
/// powers when it spawns. So the powers the walk applied are moved ahead of
/// the monster's own, in application order: those a relic changed since
/// `marks` (Philosopher's Stone), then the dominion's. A start power of the
/// same kind has stacked into the one instance (C# stacks the monster's
/// amount onto the earlier apply), which moves too.
pub(crate) fn after_room_entered_enemies(state: &mut CombatState, marks: &EnemyPowerMarks) {
    let Some(dominion) = state.dominion else {
        return;
    };
    if dominion == Dominion::WaterfallGiant {
        let n = explosive_amount(
            state.enemy_uids.len(),
            state.combat_log.dread_weak_encounter,
            state.is_elite,
        );
        state.combat_log.dread_explosive = u8::try_from(n).unwrap_or(u8::MAX);
    }
    let enemies: SmallVec<[CreatureUid; 8]> = state.enemy_uids.iter().copied().collect();
    for uid in enemies {
        let before = marks.iter().find(|(u, _)| *u == uid).map(|(_, m)| m);
        let now: SmallVec<[(PowerUid, i32); 4]> = enemy_power_marks(state)
            .into_iter()
            .find(|(u, _)| *u == uid)
            .map(|(_, m)| m)
            .unwrap_or_default();
        let mut front = 0;
        for mark in now {
            if before.is_none_or(|b| !b.contains(&mark)) {
                front = hoist(state, uid, mark.0, front);
            }
        }
        if let Some((kind, amount)) = enemy_buff(state, uid, true) {
            apply_power_to_creature(state, uid, kind, amount);
            let applied = crate::creature_by_id(&state.creatures, uid.0).and_then(|c| {
                c.powers
                    .iter()
                    .copied()
                    .find(|p| state.powers.get_by_id(p.0).is_some_and(|p| p.kind == kind))
            });
            if let Some(power) = applied {
                let _ = hoist(state, uid, power, front);
            }
        }
    }
}

/// The relics whose combat-start power C# applies in `AfterRoomEntered`
/// (`Models.Relics/<Name>.cs`, v0.113.0) and which this engine fires on its
/// `before_combat_start` channel. In a Dread combat `start_combat` fires
/// these before the dominion and the rest of the channel after it, the C#
/// order of the powers on a creature. (Ringing Triangle is the fourteenth;
/// `start_combat` fires it by itself, first.)
#[must_use]
pub(crate) fn is_room_entry_relic(id: crate::RelicId) -> bool {
    use crate::RelicId as R;
    matches!(
        id,
        R::BronzeScales
            | R::DataDisk
            | R::EmberTea
            | R::Girya
            | R::Gorget
            | R::OddlySmoothStone
            | R::PhilosophersStone
            | R::RedSkull
            | R::SlingOfCourage
            | R::SongBow
            | R::Storyteller
            | R::SwordOfJade
            | R::Vajra
    )
}

/// The dominion's `AfterRoomEntered` powers on the seated player:
/// `KinDominion.cs:28-35` (Weak 2, then Frail 2), `QueenDominion.cs:30-37`
/// (YouAreMine 3), `LagavulinMatriarchDominion.cs:28-34`,
/// `InsatiableDominion.cs:27-33`, `AeonglassDominion.cs:26-32` (their
/// dominion power, 1). Each is a `PowerCmd.Apply` with no applier, so
/// Artifact eats it like any debuff. `start_combat` calls this for every
/// player (`combatRoom.CombatState.Players`).
pub(crate) fn after_room_entered_player(state: &mut CombatState) {
    let Some(dominion) = state.dominion else {
        return;
    };
    let Some(uid) = state.player.creature_uid else {
        return;
    };
    let powers: &[(PowerKind, i32)] = match dominion {
        Dominion::Kin => &[(PowerKind::Weak, 2), (PowerKind::Frail, 2)],
        Dominion::Queen => &[(PowerKind::YouAreMine, 3)],
        Dominion::LagavulinMatriarch => &[(PowerKind::LagavulinMatriarchDominion, 1)],
        Dominion::Insatiable => &[(PowerKind::InsatiableDominion, 1)],
        Dominion::Aeonglass => &[(PowerKind::AeonglassDominion, 1)],
        _ => &[],
    };
    for &(kind, amount) in powers {
        apply_power_to_creature(state, uid, kind, amount);
    }
}

/// `DominionModel.AfterCreatureAddedToCombat` (`DominionModel.cs:73-80`) for
/// an enemy added to a combat already under way: the enemy-buff dominions
/// apply to it, after its own `AfterAddedToRoom` powers and the relics'
/// (`CreatureCmd.cs:69-82` fires the hook last; the dominion is its last
/// listener). A no-op during combat setup, where the roster is handled by
/// [`after_room_entered_enemies`].
pub(crate) fn after_enemy_added_to_combat(state: &mut CombatState, uid: CreatureUid) {
    if state.dominion.is_none() || state.phase == CombatPhase::Setup {
        return;
    }
    if let Some((kind, amount)) = enemy_buff(state, uid, false) {
        let fresh_plating = kind == PowerKind::Plating && !creature_has(state, uid, kind);
        apply_power_to_creature(state, uid, kind, amount);
        if fresh_plating
            && matches!(
                state.phase,
                CombatPhase::EnemyTurnStart | CombatPhase::EnemyTurnExec
            )
        {
            stamp_plating_applied_this_enemy_turn(state, uid);
        }
    }
}

/// Note on a Plating that an enemy gained during the enemy turn: its
/// `counter` holds that round. C# ticks an enemy's Plating at the enemy side's
/// turn START (`PlatingPower.cs:68-80`), which had passed before the power
/// existed; Rust ticks at the turn END and reads this note to skip the one
/// tick C# never made (`hooks::plating_skips_tick_this_round`).
fn stamp_plating_applied_this_enemy_turn(state: &mut CombatState, uid: CreatureUid) {
    let round = i32::from(state.round_no());
    let Some(creature) = crate::creature_by_id(&state.creatures, uid.0) else {
        return;
    };
    let ids: Vec<_> = creature.powers.iter().map(|p| p.0).collect();
    for id in ids {
        if let Some(power) = state.powers.get_mut_by_id(id) {
            if power.kind == PowerKind::Plating {
                power.counter = round;
            }
        }
    }
}

/// `DominionModel.BeforeCombatStart` -> `SoulFyshDominion.AfterPlayersEntered
/// Combat` (`SoulFyshDominion.cs:23-30`) for the seated player: one Beckon
/// into their draw pile at `CardPilePosition.Random`, i.e. one
/// `Rng.Shuffle.NextInt(pile.Count + 1)` (`CardPileCmd.cs:510`), with the
/// player as creator. `AddToCombatAndPreview` skips a dead player
/// (`CardPileCmd.cs:1237`).
pub(crate) fn before_combat_start(state: &mut CombatState) {
    if state.dominion != Some(Dominion::SoulFysh) || state.player.current_hp <= 0 {
        return;
    }
    if let Some(card) = crate::build_card_for_version(CardId::Beckon, false, state.game_version) {
        let _ = crate::card_effects::spawn_card_into_draw_pile_random_position(state, card);
    }
}

/// The two cards `KnowledgeDemonDominion.BeforeHandDraw` offers
/// (`KnowledgeDemonDominion.cs:30-34`), in screen order.
fn knowledge_demon_options() -> Vec<crate::ChoiceOption> {
    [CardId::SlothStatus, CardId::Amnesia]
        .into_iter()
        .map(|card_id| crate::ChoiceOption {
            card_id,
            upgraded: false,
            card_uid: None,
            source_pile: None,
        })
        .collect()
}

/// `KnowledgeDemonDominion.BeforeHandDraw` (`KnowledgeDemonDominion.cs:
/// 26-41`): on a living player's first turn, a forced choice between Sloth
/// and Amnesia, whose `OnChosen` then runs. The dominion is the last
/// `BeforeHandDraw` listener, so the caller runs this after the power and
/// relic walks and before `ModifyHandDraw` and the draw.
///
/// Our own pick opens a [`crate::ChoiceKind::MonsterCurseSelect`] (the
/// KnowledgeDemon boss's screen) and returns `true`: the caller parks the
/// hand draw on it. A teammate's is answered on the spot
/// ([`crate::coop::seated_player_picks`]). `false` when nothing is open.
pub(crate) fn knowledge_demon_before_hand_draw(state: &mut CombatState) -> bool {
    if state.dominion != Some(Dominion::KnowledgeDemon)
        || state.turn_number() > 1
        || state.player.current_hp <= 0
        || state.pending_choice.is_some()
    {
        return false;
    }
    if state.coop.seated == 0 {
        if state.combat_log.dread_knowledge_choice_opened {
            return false;
        }
        state.combat_log.dread_knowledge_choice_opened = true;
    }
    let options = knowledge_demon_options();
    if state.coop.seated != 0 {
        if let Some(pick) = crate::coop::seated_player_picks(state, &options) {
            knowledge_demon_on_chosen(state, options[pick].card_id);
        }
        return false;
    }
    state.pending_choice = Some(knowledge_demon_choice(options));
    true
}

/// The dominion's screen over `options`: pick exactly one, no skip
/// (`CardSelectCmd.FromChooseACardScreen`, `KnowledgeDemonDominion.cs:35`).
fn knowledge_demon_choice(options: Vec<crate::ChoiceOption>) -> crate::PendingChoice {
    crate::PendingChoice {
        kind: crate::ChoiceKind::MonsterCurseSelect,
        options,
        min_select: 1,
        max_select: 1,
        can_skip: false,
        post_effect: None,
        picked: SmallVec::new(),
    }
}

/// The turn-1 hand draw parked on our own `KnowledgeDemonDominion` screen:
/// the two halves of [`CombatState::pending_relic_hand_draw_resume`] the
/// resume needs. The wire carries it as `$.dread_knowledge_choice`
/// (GH #1346), so a board rebuilt at that screen answers it and draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParkedKnowledgeChoice {
    /// The hand-draw count stashed when the screen opened. The resume reads
    /// `Hook.ModifyHandDraw` again ([`knowledge_demon_first_turn`]), so this
    /// only keeps the rebuilt marker equal to the live one.
    pub draw_count: i32,
    /// The pause already fired `AfterSideTurnStart`; the resumed setup skips
    /// it.
    pub side_fired: bool,
}

/// `Some` while our own Sloth-or-Amnesia screen is open with the hand draw
/// parked behind it; `None` on every other board.
#[must_use]
pub fn parked_knowledge_choice(state: &CombatState) -> Option<ParkedKnowledgeChoice> {
    if state.dominion != Some(Dominion::KnowledgeDemon)
        || !state.combat_log.dread_knowledge_choice_opened
        || state.pending_curse_choice.is_some()
    {
        return None;
    }
    let pc = state.pending_choice.as_ref()?;
    let offered = pc.options.iter().map(|o| o.card_id);
    if pc.kind != crate::ChoiceKind::MonsterCurseSelect
        || !offered.eq(knowledge_demon_options().iter().map(|o| o.card_id))
    {
        return None;
    }
    match state.pending_relic_hand_draw_resume {
        Some((draw_count, u8::MAX, side_fired)) => Some(ParkedKnowledgeChoice {
            draw_count,
            side_fired,
        }),
        _ => None,
    }
}

/// Put a rebuilt board back on the dominion's screen: the choice open, the
/// latch raised and the hand draw parked behind it, as
/// [`knowledge_demon_before_hand_draw`] leaves the live one. `false`, and
/// nothing changes, unless this is a `KnowledgeDemonDominion` combat with no
/// choice open.
pub fn park_knowledge_choice(state: &mut CombatState, parked: ParkedKnowledgeChoice) -> bool {
    if state.dominion != Some(Dominion::KnowledgeDemon) || state.pending_choice.is_some() {
        return false;
    }
    state.combat_log.dread_knowledge_choice_opened = true;
    state.pending_choice = Some(knowledge_demon_choice(knowledge_demon_options()));
    state.pending_relic_hand_draw_resume = Some((parked.draw_count, u8::MAX, parked.side_fired));
    true
}

/// `true` on the first turn of a `KnowledgeDemonDominion` combat: the hand
/// draw count is then read after the `BeforeHandDraw` walk, as C# does.
pub(crate) fn knowledge_demon_first_turn(state: &CombatState) -> bool {
    state.dominion == Some(Dominion::KnowledgeDemon) && state.turn_number() <= 1
}

/// `Sloth.OnChosen` / `Amnesia.OnChosen` from the dominion's choice
/// (`Sloth.cs:24-27`: SlothPower 3; `Amnesia.cs:29-32`: AmnesiaPower 1), a
/// full `PowerCmd.Apply` on the owner: Artifact eats either.
pub(crate) fn knowledge_demon_on_chosen(state: &mut CombatState, card_id: CardId) {
    let Some(uid) = state.player.creature_uid else {
        return;
    };
    match card_id {
        CardId::SlothStatus => apply_power_to_creature(state, uid, PowerKind::Sloth, 3),
        CardId::Amnesia => apply_power_to_creature(state, uid, PowerKind::Amnesia, 1),
        _ => {}
    }
}

/// `AeonglassDominionPower.BeforeHandDraw` (`AeonglassDominionPower.cs:
/// 30-41`): a Wither on top of the owner's draw pile, every turn, with no
/// creator (`AddGeneratedCardToCombat(wither, Draw, null, Top)`).
pub(crate) fn aeonglass_dominion_before_hand_draw(state: &mut CombatState) {
    crate::event_log::flash_player_power(state, PowerKind::AeonglassDominion);
    let _ = crate::card_effects::spawn_wither_into_pile(state, crate::card_effects::PileKind::Draw);
}

/// `YouAreMinePower.AfterPlayerTurnStart` (`YouAreMinePower.cs:33-45`): from
/// the owner's second turn on, `Amount` Weak, Vulnerable and Frail, then the
/// power removes itself.
pub(crate) fn you_are_mine_after_player_turn_start(state: &mut CombatState, power: PowerUid) {
    if state.turn_number() < 2 {
        return;
    }
    let Some(amount) = state.powers.get_by_id(power.0).map(|p| p.amount) else {
        return;
    };
    if let Some(uid) = state.player.creature_uid {
        for kind in [PowerKind::Weak, PowerKind::Vulnerable, PowerKind::Frail] {
            apply_power_to_creature(state, uid, kind, amount);
        }
    }
    state.drop_power(power.0);
}

/// `AmnesiaPower.AfterPlayerTurnStart` (`AmnesiaPower.cs:25-31`):
/// `PlayerCmd.LoseEnergy(1)`, floored at 0.
pub(crate) fn amnesia_after_player_turn_start(state: &mut CombatState) {
    state.player.energy = state.player.energy.saturating_sub(1).max(0);
}

/// `ExplosivePower.AfterDeath` (`ExplosivePower.cs:21-32`): every living
/// player takes `amount`, `Unpowered` (so block absorbs it), with no dealer
/// and no card source.
pub(crate) fn explosive_after_death(state: &mut CombatState, amount: i32) {
    crate::coop::each_player(state, |state| {
        if state.player.current_hp <= 0 {
            return;
        }
        // No card source: Rupture answers at once (see `cardless_player_damage`).
        let prev = std::mem::replace(&mut state.combat_log.cardless_player_damage, true);
        let _ = crate::combat_loop::apply_damage_to_player(
            state,
            crate::hooks::CreatureRef::Player,
            amount,
            crate::hooks::ValueProps::UNPOWERED,
            None,
        );
        state.combat_log.cardless_player_damage = prev;
    });
}
