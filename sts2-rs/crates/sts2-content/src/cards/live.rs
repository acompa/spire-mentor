// HAND-MAINTAINED — edit directly.
//! A card's live attack against one enemy: the base damage it would deal now,
//! how many of its hits land on that enemy, and who deals them (GH #1197).
//!
//! The e2e net's damage bound used to read a hand card's printed
//! `base_damage` times a static per-card hit count, always dealt by the
//! player. That is wrong for every card whose damage or hit count is
//! calculated at play time (Body Slam, Finisher, Rattle, …) and for the Osty
//! attacks, whose dealer is the Osty. [`live_attack`] answers the same question
//! the card's `OnPlay` would answer if it were played now.
//!
//! `base` is **before** modifiers: the caller folds Strength / Vigor /
//! Vulnerable through `compute_modified_damage` with [`LiveAttack::dealer`].
//! So the calculated cards go through their raw `preview_*` (the formula the
//! play handler itself calls, see [`super::preview`]), never through
//! [`super::preview::calculated_value`], which already folds those hooks.
//! `base` stays signed: Precise Cut can go negative, and the game adds
//! Strength to the negative base before it floors.
//!
//! Hit counts follow each card's `OnPlay` in the v0.111.0 decomp
//! (`WithHitCount(...)`, `RepeatVar` + `Repeat.UpgradeValueBy`). The engine
//! card carries no Repeat value of its own — every handler derives it from
//! `upgraded` — so [`fixed_hits`] holds the decomp's static repeat counts.
//! X-cost attacks (Whirlwind, Skewer, Eradicate, Volley) report one hit: their
//! hit count is the energy spent, which the consumer supplies. Heavenly Drill
//! reports its hits *per energy*: 2 once X reaches its threshold, else 1.
//!
//! Replays multiply everything: C# `GeneratePlayCount` re-runs the whole
//! `OnPlay` `1 + GetEnchantedReplayCount()` times (`BaseReplayCount` — Sword
//! Sage on Sovereign Blade, Hidden Gem, Soldier's Stew — plus Spiral / Glam).

use sts2_core::card_effects::osty_dealer_ref;
use sts2_core::cards::{Card, CardId, CardType, TargetType};
use sts2_core::hooks::CreatureRef;
use sts2_core::{CardUid, CombatState, CreatureUid};

use crate::cards::preview::{preview_of, ValueKind};

/// What one card would deal to one enemy if it were played now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveAttack {
    /// Damage per hit before any modifier hook (Strength, Vulnerable, …).
    pub base: i32,
    /// Hits landing on the asked-about target. For a random-target attack
    /// this is the expectation: all hits when the target is the only living
    /// enemy, else `hits / living enemies`.
    pub hits: f32,
    /// Who deals the hits: the player, or the Osty for an Osty attack.
    pub dealer: CreatureRef,
}

/// `card_uid`'s live attack against `target`, or `None` when the card deals
/// no attack damage now: not an Attack, or an Osty attack with no living Osty
/// (C# `Osty.CheckMissingWithAnim` skips the attack).
///
/// Every attack without a calculated value and not named below deals its
/// printed damage once per printed hit ([`fixed_hits`]).
///
/// `target` should be a living enemy: the random-target share divides by the
/// living enemies whether or not `target` is one of them.
#[must_use]
pub fn live_attack(
    state: &CombatState,
    card_uid: CardUid,
    target: CreatureUid,
) -> Option<LiveAttack> {
    let card = state.cards.get(card_uid.0.get() as usize - 1)?;
    if card.card_type != CardType::Attack {
        return None;
    }
    let dealer = if card.card_id.is_osty_attack() {
        osty_dealer_ref(state)?
    } else {
        CreatureRef::Player
    };
    let t = Some(target);
    // The printed per-hit damage: Osty attacks hold their (upgraded)
    // `OstyDamageVar` in the handler, not in `base_damage`.
    let printed = if card.card_id.is_osty_attack() {
        osty_damage(card)
    } else {
        card.base_damage
    };
    let (base, hits): (i32, i32) = match preview_of(card.card_id, state.game_version) {
        // `CalculatedDamageVar` attacks: one hit of the calculated value.
        Some((f, ValueKind::Damage | ValueKind::OstyDamage)) => (f(state, card, t), 1),
        // `CalculatedHits` attacks: the printed damage, a calculated count.
        Some((f, ValueKind::Hits)) => (printed, f(state, card, t)),
        // Other counts (Compile Driver's cards, Beat Into Shape's Forge) are
        // not hits; such a card falls through to its printed attack.
        _ => match card.card_id {
            // Hit counts decided in `OnPlay` itself (no `CalculatedVar` to share).
            // Spite.cs:34: `LostHpThisTurn ? Repeat : 1`, RepeatVar(2) +1 upgraded.
            CardId::Spite => {
                let lost_hp = state.combat_log.hp_lost_this_turn > 0;
                (card.base_damage, if lost_hp { fixed_hits(card) } else { 1 })
            }
            // Dismantle.cs:32: `target.HasPower<VulnerablePower>() ? 2 : 1`.
            CardId::Dismantle => {
                let vulnerable = super::ironclad::helpers::enemy_vulnerable(state, target) > 0;
                (card.base_damage, if vulnerable { 2 } else { 1 })
            }
            // FiendFire.cs:32-41: one hit per card exhausted from the hand, which
            // by then no longer holds Fiend Fire itself.
            CardId::FiendFire => {
                let own = i32::from(state.player.hand.contains(&card.uid));
                let hand = i32::try_from(state.player.hand.len()).unwrap_or(i32::MAX);
                (card.base_damage, hand - own)
            }
            // Stardust.cs:24: `WithHitCount(ResolveStarXValue())`, the Stars the
            // play spends folded through `Hook.ModifyXValue` (Chemical X).
            CardId::Stardust => (
                card.base_damage,
                sts2_core::relic_effects::dispatch_modify_x_value(
                    state,
                    i32::from(state.player.stars),
                ),
            ),
            // HeavenlyDrill.cs: `num = ResolveEnergyXValue(); if (num >=
            // Energy(4)) num *= 2` — hits per energy spent, judged at the energy
            // available now (folded through `Hook.ModifyXValue`, Chemical X).
            CardId::HeavenlyDrill => {
                let x =
                    sts2_core::relic_effects::dispatch_modify_x_value(state, state.player.energy);
                (card.base_damage, if x >= 4 { 2 } else { 1 })
            }
            // Rampage.cs: `Damage += Increase` after each play; the engine keeps
            // the growth in `VarKey::RampageDamage`, not in `base_damage`.
            CardId::Rampage => (
                card.base_damage
                    .saturating_add(card.combat_var(sts2_core::cards::VarKey::RampageDamage)),
                1,
            ),
            _ => (printed, fixed_hits(card)),
        },
    };
    // Sovereign Blade's Sword Sage stacks arrive here too: SovereignBlade.cs
    // hits `Repeat` (1) times per play and SwordSagePower adds replays.
    // Each replay is assumed to deal what the first play deals: a card whose
    // play changes its own next play (Fiend Fire empties the hand, Rampage
    // grows) is overstated / understated on its replays.
    let plays = 1
        + card.base_replay_count.max(0)
        + sts2_core::enchantment_hooks::enchant_play_count_bonus(card);
    #[allow(clippy::cast_precision_loss)]
    let mut hits = hits.max(0).saturating_mul(plays) as f32;
    if card.target_type == TargetType::Random {
        hits /= living_enemies(state).max(1) as f32;
    }
    Some(LiveAttack { base, hits, dealer })
}

/// The printed hit count of a plain attack: its `RepeatVar` (upgraded) or the
/// literal `WithHitCount(n)` in its v0.111.0 `OnPlay`, else 1.
fn fixed_hits(card: &Card) -> i32 {
    let up = i32::from(card.upgraded);
    match card.card_id {
        // `RepeatVar(n)` with `Repeat.UpgradeValueBy(1m)`.
        CardId::Conflagration | CardId::Ricochet => 4 + up,
        CardId::SwordBoomerang | CardId::CelestialMight | CardId::Peck => 3 + up,
        CardId::Spite => 2 + up,
        // `RepeatVar(n)`, never upgraded.
        CardId::SevenStars => 7,
        CardId::Exterminate => 4,
        CardId::GunkUp => 3,
        CardId::FightMe => 2,
        // Literal `WithHitCount(2)`.
        CardId::AstralPulse
        | CardId::DaggerSpray
        | CardId::Maul
        | CardId::Refract
        | CardId::RipAndTear
        | CardId::Thrash
        | CardId::TwinStrike
        | CardId::Uproar => 2,
        _ => 1,
    }
}

/// An Osty attack's `OstyDamageVar` (base, upgraded), from the v0.111.0
/// decomp. The play handlers hold the upgraded value themselves (most of
/// these cards carry no upgrade delta, so `base_damage` stays un-upgraded);
/// this mirrors them.
fn osty_damage(card: &Card) -> i32 {
    let (base, upgraded) = match card.card_id {
        CardId::BoneShards => (9, 12),
        CardId::Fetch => (3, 6),
        CardId::Flatten => (12, 16),
        CardId::HighFive => (11, 13),
        CardId::Poke => (6, 9),
        CardId::Rattle => (7, 9),
        CardId::RightHandHand => (4, 6),
        CardId::SicEm => (5, 6),
        CardId::Snap => (7, 10),
        _ => return card.base_damage,
    };
    if card.upgraded {
        upgraded
    } else {
        base
    }
}

/// The enemies a random-target hit picks from: `CombatState::get_random_enemy`'s
/// candidate set, the living members of `enemy_uids`.
fn living_enemies(state: &CombatState) -> usize {
    state
        .enemy_uids
        .iter()
        .filter(|u| {
            state
                .creatures
                .iter()
                .any(|c| c.id == u.0 && c.current_hp > 0)
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::SmallVec;
    use sts2_core::cards::CardTag;
    use sts2_core::{
        Creature, GameVersion, IntentHistory, MonsterId, MonsterKind, Power, PowerKind, PowerUid,
    };

    #[derive(Clone, Copy)]
    enum Pile {
        Hand,
        Draw,
        Discard,
        Exhaust,
    }

    fn push_creature(state: &mut CombatState, hp: i32, max_hp: i32) -> CreatureUid {
        let raw = u32::try_from(state.creatures.len() + 1).expect("fits");
        let id = std::num::NonZeroU32::new(raw).expect("non-zero");
        state.creatures.push(Creature {
            id,
            current_hp: hp,
            max_hp,
            block: 0,
            powers: SmallVec::new(),
            species: Some(MonsterKind::Nibbit),
            intent: None,
            intent_history: IntentHistory::default(),
            monster_id: MonsterId("Nibbit"),
            ai: None,
        });
        CreatureUid(id)
    }

    /// A v0.111.0 board with `enemies` living enemies; returns them.
    fn board(enemies: usize) -> (CombatState, Vec<CreatureUid>) {
        crate::cards::dispatch::ensure_installed();
        let mut state = CombatState {
            game_version: GameVersion::V0_111_0,
            ..CombatState::default()
        };
        state.player.current_hp = 80;
        state.player.max_hp = 80;
        let uids: Vec<_> = (0..enemies)
            .map(|_| push_creature(&mut state, 50, 50))
            .collect();
        state.enemy_uids.extend(uids.iter().copied());
        (state, uids)
    }

    /// Mint `id` from the v0.111.0 pods into `pile`.
    fn add(state: &mut CombatState, id: CardId, upgraded: bool, pile: Pile) -> CardUid {
        let mut card = crate::cards::v0111::card_pod_for(id, upgraded, GameVersion::V0_111_0)
            .expect("spec exists");
        let raw = u32::try_from(state.cards.len() + 1).expect("fits");
        let uid = CardUid::new(raw).expect("non-zero");
        card.uid = uid;
        state.cards.push(card);
        let p = &mut state.player;
        match pile {
            Pile::Hand => p.hand.push(uid),
            Pile::Draw => p.draw.push(uid),
            Pile::Discard => p.discard.push(uid),
            Pile::Exhaust => p.exhaust.push(uid),
        }
        uid
    }

    fn give_power(state: &mut CombatState, who: CreatureUid, kind: PowerKind, amount: i32) {
        let raw = u32::try_from(state.powers.len() + 1).expect("fits");
        let pid = PowerUid::new(raw).expect("non-zero");
        state.powers.push(Power {
            id: pid.0,
            kind,
            amount,
            counter: 0,
            applier: None,
            skip_next_tick: false,
        });
        let c = state
            .creatures
            .iter_mut()
            .find(|c| c.id == who.0)
            .expect("creature");
        c.powers.push(pid);
    }

    /// Summon a living Osty with `hp` / `max_hp`.
    fn summon_osty(state: &mut CombatState, hp: i32, max_hp: i32) -> CreatureUid {
        let uid = push_creature(state, hp, max_hp);
        state.player.pet_uid = Some(uid);
        uid
    }

    fn live(state: &CombatState, card: CardUid, target: CreatureUid) -> (i32, f32, CreatureRef) {
        let a = live_attack(state, card, target).expect("an attack");
        (a.base, a.hits, a.dealer)
    }

    const P: CreatureRef = CreatureRef::Player;

    #[test]
    fn default_is_printed() {
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::StrikeIronclad, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (6, 1.0, P));
        let twin = add(&mut s, CardId::TwinStrike, false, Pile::Hand);
        assert_eq!(live(&s, twin, e[0]), (5, 2.0, P));
    }

    #[test]
    fn replays_multiply_hits() {
        // CardModel.GeneratePlayCount: 1 + BaseReplayCount + enchant plays.
        let (mut s, e) = board(1);
        let twin = add(&mut s, CardId::TwinStrike, false, Pile::Hand);
        s.cards[twin.0.get() as usize - 1].base_replay_count = 2;
        assert_eq!(live(&s, twin, e[0]), (5, 6.0, P));
        let strike = add(&mut s, CardId::StrikeIronclad, false, Pile::Hand);
        s.cards[strike.0.get() as usize - 1]
            .add_enchantment(sts2_core::cards::EnchantmentId::Spiral, 1);
        assert_eq!(live(&s, strike, e[0]), (6, 2.0, P));
    }

    #[test]
    fn heavenly_drill_doubles_at_four() {
        // HeavenlyDrill.cs: X >= Energy(4) doubles the hit count.
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::HeavenlyDrill, false, Pile::Hand);
        s.player.energy = 3;
        assert_eq!(live(&s, c, e[0]), (8, 1.0, P));
        s.player.energy = 4;
        assert_eq!(live(&s, c, e[0]), (8, 2.0, P));
    }

    #[test]
    fn rampage_adds_its_growth() {
        // Rampage.cs:20-21: DamageVar(10), Damage += Increase(5) per play;
        // the engine keeps the growth in RampageDamage.
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::Rampage, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (10, 1.0, P));
        s.cards[c.0.get() as usize - 1].set_combat_var(sts2_core::cards::VarKey::RampageDamage, 10);
        assert_eq!(live(&s, c, e[0]), (20, 1.0, P));
    }

    #[test]
    fn ice_lance_hits_once() {
        // IceLance.cs OnPlay: one `DamageCmd.Attack`; its RepeatVar(3) counts
        // the Frost orbs channelled after it, not hits. (The old static HITS
        // table said 3.)
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::IceLance, false, Pile::Hand);
        let a = live_attack(&s, c, e[0]).expect("attack");
        assert_eq!(a.hits, 1.0);
    }

    #[test]
    fn non_attack_is_none() {
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::DefendIronclad, false, Pile::Hand);
        assert_eq!(live_attack(&s, c, e[0]), None);
    }

    // ---- Ironclad ----

    #[test]
    fn body_slam_uses_block() {
        let (mut s, e) = board(1);
        s.player.block = 13;
        let c = add(&mut s, CardId::BodySlam, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (13, 1.0, P));
    }

    #[test]
    fn perfected_strike_counts_strikes() {
        // PerfectedStrike.cs:25: ExtraDamage(2, +1 upgraded) x Strike-tagged
        // cards in AllCards — Perfected Strike itself is Strike-tagged.
        let (mut s, e) = board(1);
        let ps = add(&mut s, CardId::PerfectedStrike, false, Pile::Hand);
        let ps_up = add(&mut s, CardId::PerfectedStrike, true, Pile::Draw);
        add(&mut s, CardId::StrikeIronclad, false, Pile::Draw);
        add(&mut s, CardId::StrikeIronclad, false, Pile::Discard);
        add(&mut s, CardId::StrikeIronclad, false, Pile::Exhaust);
        add(&mut s, CardId::DefendIronclad, false, Pile::Hand);
        let tagged = s
            .cards
            .iter()
            .filter(|c| c.has_tag(CardTag::Strike))
            .count();
        assert_eq!(tagged, 5);
        assert_eq!(live(&s, ps, e[0]), (6 + 2 * 5, 1.0, P));
        assert_eq!(live(&s, ps_up, e[0]), (6 + 3 * 5, 1.0, P));
    }

    #[test]
    fn ashen_strike_times_exhaust() {
        // AshenStrike.cs:20-22,42: 6 + ExtraDamage(3, 4 upgraded) x Exhaust.
        let (mut s, e) = board(1);
        for _ in 0..3 {
            add(&mut s, CardId::DefendIronclad, false, Pile::Exhaust);
        }
        let c = add(&mut s, CardId::AshenStrike, false, Pile::Hand);
        let up = add(&mut s, CardId::AshenStrike, true, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (15, 1.0, P));
        assert_eq!(live(&s, up, e[0]), (18, 1.0, P));
    }

    #[test]
    fn bully_times_target_vulnerable() {
        // Bully.cs:19-21: 4 + ExtraDamage(2) x target Vulnerable.
        let (mut s, e) = board(2);
        give_power(&mut s, e[0], PowerKind::Vulnerable, 2);
        let c = add(&mut s, CardId::Bully, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (8, 1.0, P));
        assert_eq!(live(&s, c, e[1]), (4, 1.0, P));
    }

    #[test]
    fn spite_hits_after_hp_loss() {
        // Spite.cs:34: LostHpThisTurn ? Repeat(2, 3 upgraded) : 1.
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::Spite, false, Pile::Hand);
        let up = add(&mut s, CardId::Spite, true, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (5, 1.0, P));
        assert_eq!(live(&s, up, e[0]), (5, 1.0, P));
        s.combat_log.hp_lost_this_turn = 1;
        assert_eq!(live(&s, c, e[0]), (5, 2.0, P));
        assert_eq!(live(&s, up, e[0]), (5, 3.0, P));
    }

    #[test]
    fn dismantle_two_hits_on_vulnerable() {
        // Dismantle.cs:32: target Vulnerable ? 2 : 1.
        let (mut s, e) = board(2);
        give_power(&mut s, e[1], PowerKind::Vulnerable, 1);
        let c = add(&mut s, CardId::Dismantle, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (8, 1.0, P));
        assert_eq!(live(&s, c, e[1]), (8, 2.0, P));
    }

    #[test]
    fn fiend_fire_hits_cards_in_hand() {
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::FiendFire, false, Pile::Hand);
        for _ in 0..4 {
            add(&mut s, CardId::DefendIronclad, false, Pile::Hand);
        }
        assert_eq!(live(&s, c, e[0]), (7, 4.0, P));
    }

    #[test]
    fn tear_asunder_calculated_hits() {
        // TearAsunder.cs:26-29: 1 + unblocked damage entries this combat.
        let (mut s, e) = board(1);
        s.combat_log.unblocked_damage_received_this_combat = 3;
        let c = add(&mut s, CardId::TearAsunder, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (5, 4.0, P));
    }

    #[test]
    fn conflagration_upgraded_five_hits() {
        // Conflagration.cs:22,45: RepeatVar(4), +1 upgraded; hits all enemies.
        let (mut s, e) = board(2);
        let c = add(&mut s, CardId::Conflagration, false, Pile::Hand);
        let up = add(&mut s, CardId::Conflagration, true, Pile::Hand);
        assert_eq!(live(&s, c, e[1]), (2, 4.0, P));
        assert_eq!(live(&s, up, e[1]), (2, 5.0, P));
    }

    #[test]
    fn sword_boomerang_random_share() {
        // SwordBoomerang.cs:16,26-27: RepeatVar(3) at random opponents.
        let (mut s, e) = board(2);
        let c = add(&mut s, CardId::SwordBoomerang, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (3, 1.5, P));
        s.creatures[1].current_hp = 0;
        assert_eq!(live(&s, c, e[0]), (3, 3.0, P));
    }

    // ---- Silent ----

    #[test]
    fn murder_cards_drawn() {
        // Murder.cs:20-25: 1 + cards drawn this combat.
        let (mut s, e) = board(1);
        s.cards_drawn = 12;
        let c = add(&mut s, CardId::Murder, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (13, 1.0, P));
    }

    #[test]
    fn memento_mori_discards() {
        // MementoMori.cs:20-25,44-45: 9 (+2) + ExtraDamage(4, +1) x discards
        // this turn.
        let (mut s, e) = board(1);
        s.combat_log.cards_discarded_this_turn = 2;
        let c = add(&mut s, CardId::MementoMori, false, Pile::Hand);
        let up = add(&mut s, CardId::MementoMori, true, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (17, 1.0, P));
        assert_eq!(live(&s, up, e[0]), (21, 1.0, P));
    }

    #[test]
    fn precise_cut_hand_size() {
        // PreciseCut.cs: 13 - 2 x other cards in hand; signed (no floor).
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::PreciseCut, false, Pile::Hand);
        for _ in 0..3 {
            add(&mut s, CardId::DefendSilent, false, Pile::Hand);
        }
        assert_eq!(live(&s, c, e[0]), (7, 1.0, P));
        for _ in 0..4 {
            add(&mut s, CardId::DefendSilent, false, Pile::Hand);
        }
        assert_eq!(live(&s, c, e[0]), (-1, 1.0, P));
    }

    #[test]
    fn finisher_attacks_played() {
        // Finisher.cs:26-29: hits = Attacks finished this turn (0 is 0 hits).
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::Finisher, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (6, 0.0, P));
        s.player.cards_played_this_turn[CardType::Attack.index()] = 3;
        assert_eq!(live(&s, c, e[0]), (6, 3.0, P));
    }

    #[test]
    fn flechettes_skills_in_hand() {
        // Flechettes.cs:23: one hit per Skill in hand.
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::Flechettes, false, Pile::Hand);
        add(&mut s, CardId::DefendSilent, false, Pile::Hand);
        add(&mut s, CardId::DefendSilent, false, Pile::Hand);
        add(&mut s, CardId::StrikeSilent, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (5, 2.0, P));
    }

    #[test]
    fn ricochet_random_share() {
        // Ricochet.cs:16,28-29,36: RepeatVar(4), +1 upgraded, random targets.
        let (mut s, e) = board(2);
        let c = add(&mut s, CardId::Ricochet, false, Pile::Hand);
        let up = add(&mut s, CardId::Ricochet, true, Pile::Hand);
        assert_eq!(live(&s, c, e[1]), (3, 2.0, P));
        assert_eq!(live(&s, up, e[1]), (3, 2.5, P));
        s.creatures[0].current_hp = 0;
        assert_eq!(live(&s, up, e[1]), (3, 5.0, P));
    }

    // ---- Defect ----

    #[test]
    fn barrage_orb_count() {
        // Barrage.cs:25: one hit per channelled orb.
        let (mut s, e) = board(1);
        s.player.orbs.ensure_defect_base();
        s.player.orbs.channel(sts2_core::orbs::Orb::Lightning);
        s.player.orbs.channel(sts2_core::orbs::Orb::Frost);
        let c = add(&mut s, CardId::Barrage, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (5, 2.0, P));
    }

    #[test]
    fn flak_cannon_hits() {
        // FlakCannon.cs:24,42-43: one hit per Status outside Exhaust, at
        // random opponents.
        let (mut s, e) = board(2);
        let c = add(&mut s, CardId::FlakCannon, false, Pile::Hand);
        add(&mut s, CardId::Wound, false, Pile::Hand);
        add(&mut s, CardId::Dazed, false, Pile::Draw);
        add(&mut s, CardId::Slimed, false, Pile::Discard);
        add(&mut s, CardId::Wound, false, Pile::Exhaust);
        assert_eq!(live(&s, c, e[0]), (8, 1.5, P));
    }

    #[test]
    fn helix_drill_hits() {
        // HelixDrill.cs:26-35: energy spent this turn (own cost not yet paid
        // for a card in hand).
        let (mut s, e) = board(1);
        s.combat_log.energy_spent_this_turn = 3;
        let c = add(&mut s, CardId::HelixDrill, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (3, 3.0, P));
    }

    // ---- Regent ----

    #[test]
    fn radiate_stars_gained() {
        // Radiate.cs:25-41: one all-enemy hit per Star gained this turn.
        let (mut s, e) = board(2);
        s.player.stars_gained_this_turn = 4;
        let c = add(&mut s, CardId::Radiate, false, Pile::Hand);
        assert_eq!(live(&s, c, e[1]), (3, 4.0, P));
    }

    #[test]
    fn stardust_x_stars() {
        // Stardust.cs:24-25: ResolveStarXValue() hits at random opponents.
        let (mut s, e) = board(1);
        s.player.stars = 3;
        let c = add(&mut s, CardId::Stardust, false, Pile::Hand);
        assert_eq!(live(&s, c, e[0]), (5, 3.0, P));
        let (mut s2, e2) = board(2);
        s2.player.stars = 3;
        let c2 = add(&mut s2, CardId::Stardust, true, Pile::Hand);
        assert_eq!(live(&s2, c2, e2[0]), (7, 1.5, P));
    }

    #[test]
    fn sovereign_blade_replays() {
        // SovereignBlade.cs:127 + SwordSagePower.TryAddReplays: one hit per
        // play, 1 + BaseReplayCount plays; forge grows the damage.
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::SovereignBlade, false, Pile::Hand);
        let idx = c.0.get() as usize - 1;
        s.cards[idx].base_damage = 16;
        assert_eq!(live(&s, c, e[0]), (16, 1.0, P));
        s.cards[idx].base_replay_count = 2;
        assert_eq!(live(&s, c, e[0]), (16, 3.0, P));
    }

    // ---- Necrobinder ----

    #[test]
    fn poke_dealer_is_osty() {
        let (mut s, e) = board(1);
        let osty = summon_osty(&mut s, 10, 10);
        let c = add(&mut s, CardId::Poke, false, Pile::Hand);
        let up = add(&mut s, CardId::Poke, true, Pile::Hand);
        let o = CreatureRef::Enemy(osty);
        assert_eq!(live(&s, c, e[0]), (6, 1.0, o));
        assert_eq!(live(&s, up, e[0]), (9, 1.0, o));
    }

    #[test]
    fn osty_attack_none_when_dead() {
        let (mut s, e) = board(1);
        let c = add(&mut s, CardId::Poke, false, Pile::Hand);
        assert_eq!(live_attack(&s, c, e[0]), None, "no Osty summoned");
        summon_osty(&mut s, 0, 10);
        assert_eq!(live_attack(&s, c, e[0]), None, "Osty dead");
    }

    #[test]
    fn unleash_calculated() {
        // Unleash.cs:22-24,49: 6 (+3) + Osty current HP.
        let (mut s, e) = board(1);
        let osty = summon_osty(&mut s, 7, 12);
        let c = add(&mut s, CardId::Unleash, false, Pile::Hand);
        let up = add(&mut s, CardId::Unleash, true, Pile::Hand);
        let o = CreatureRef::Enemy(osty);
        assert_eq!(live(&s, c, e[0]), (13, 1.0, o));
        assert_eq!(live(&s, up, e[0]), (16, 1.0, o));
    }

    #[test]
    fn protector_scales() {
        // Protector.cs:22-24,50: 10 (+5) + Osty max HP.
        let (mut s, e) = board(1);
        let osty = summon_osty(&mut s, 7, 12);
        let c = add(&mut s, CardId::Protector, false, Pile::Hand);
        let up = add(&mut s, CardId::Protector, true, Pile::Hand);
        let o = CreatureRef::Enemy(osty);
        assert_eq!(live(&s, c, e[0]), (22, 1.0, o));
        assert_eq!(live(&s, up, e[0]), (27, 1.0, o));
    }

    #[test]
    fn squeeze_scales() {
        // Squeeze.cs:23-28,50-51: 25 (+5) + ExtraDamage(5, +1) x OTHER
        // OstyAttack cards (`c != card2`).
        let (mut s, e) = board(1);
        let osty = summon_osty(&mut s, 7, 12);
        let c = add(&mut s, CardId::Squeeze, false, Pile::Hand);
        let up = add(&mut s, CardId::Squeeze, true, Pile::Draw);
        add(&mut s, CardId::Poke, false, Pile::Hand);
        add(&mut s, CardId::Rattle, false, Pile::Discard);
        let o = CreatureRef::Enemy(osty);
        // Others of `c`: Poke, Rattle, the upgraded Squeeze = 3.
        assert_eq!(live(&s, c, e[0]), (25 + 5 * 3, 1.0, o));
        assert_eq!(live(&s, up, e[0]), (30 + 6 * 3, 1.0, o));
    }

    #[test]
    fn rattle_hits() {
        // Rattle.cs:27-33,56: 7 (+2) x (1 + Osty attacks this turn).
        let (mut s, e) = board(1);
        let osty = summon_osty(&mut s, 7, 12);
        s.combat_log.osty_attacks_this_turn = 2;
        let c = add(&mut s, CardId::Rattle, false, Pile::Hand);
        let up = add(&mut s, CardId::Rattle, true, Pile::Hand);
        let o = CreatureRef::Enemy(osty);
        assert_eq!(live(&s, c, e[0]), (7, 3.0, o));
        assert_eq!(live(&s, up, e[0]), (9, 3.0, o));
    }

    // ---- Beyond the brief: the other calculated-damage attacks ----

    #[test]
    fn calculated_damage_cards_use_their_preview() {
        // Every other calculated attack reports its raw preview: the
        // target's Vulnerable (x1.5) folds into the face, never into `base`.
        // Each multiplier is seeded non-zero, so a 0 == 0 pass is impossible.
        let (mut s, e) = board(1);
        give_power(&mut s, e[0], PowerKind::Vulnerable, 2);
        give_power(&mut s, e[0], PowerKind::Doom, 6); // Time's Up
        s.player.block = 4;
        s.combat_log.player_card_plays_this_combat = 3; // Gold Axe
        s.player.cards_played_this_turn[CardType::Skill.index()] = 2; // Lunar Blast
        s.combat_log.ethereal_cards_played_this_combat = 3; // Pull From Below
        for _ in 0..4 {
            add(&mut s, CardId::DefendIronclad, false, Pile::Draw); // Mind Blast
        }
        for id in [
            CardId::CrescentSpear,
            CardId::DeathMarch,
            CardId::GangUp,
            CardId::GoldAxe,
            CardId::MindBlast,
            CardId::Rend,
            CardId::SoulStorm,
            CardId::Supermassive,
            CardId::TimesUp,
            CardId::LunarBlast,
            CardId::PullFromBelow,
        ] {
            let c = add(&mut s, id, false, Pile::Hand);
            let card = s.cards[c.0.get() as usize - 1];
            let a = live_attack(&s, c, e[0]).expect("attack");
            let face = crate::cards::preview::calculated_value(&s, &card, Some(e[0]))
                .expect("calculated card");
            if matches!(id, CardId::LunarBlast | CardId::PullFromBelow) {
                assert!(face > 0, "{id:?}: seed its multiplier");
                assert_eq!((a.base, a.hits), (card.base_damage, face as f32), "{id:?}");
            } else {
                assert!(a.base > 0, "{id:?}: seed its multiplier");
                assert_eq!(a.hits, 1.0, "{id:?}");
                assert_eq!(face, a.base * 3 / 2, "{id:?}");
            }
        }
    }
}
