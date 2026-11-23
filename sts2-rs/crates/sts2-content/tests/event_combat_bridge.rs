//! Event→combat bridge content wiring (bead `sts2-rs-ck9z6`, GH #166).
//!
//! Covers the content half of the bridge: the affected event handlers
//! stage the right [`EventCombatRequest`], `build_event_combat` maps each
//! encounter to the correct monster roster (with its spawn powers), and the
//! BattlewornDummy resume grants the per-setting reward only on a clean
//! victory. The engine-side phase routing is covered in
//! `sts2-core::run_manager` tests; the timer itself in
//! `sts2-core::combat_loop`.

use sts2_content::events::dispatch;
use sts2_content::run_bridge::build_event_combat;
use sts2_core::events::{BattlewornDummySetting, EventEncounter};
use sts2_core::monsters::MonsterKind;
use sts2_core::{EventId, PowerKind, RelicId, RunState};

fn enemy_has_power(combat: &sts2_core::CombatState, kind: PowerKind) -> bool {
    combat
        .creatures
        .iter()
        .filter(|c| c.species.is_some())
        .any(|c| {
            c.powers.iter().any(|p| {
                combat
                    .powers
                    .get_by_id(p.0)
                    .is_some_and(|pw| pw.kind == kind)
            })
        })
}

fn first_enemy_kind(combat: &sts2_core::CombatState) -> MonsterKind {
    combat
        .creatures
        .iter()
        .find_map(|c| c.species)
        .expect("an enemy was spawned")
}

#[test]
fn battleworn_settings_stage_resuming_dummy_fight() {
    for (idx, setting) in [
        BattlewornDummySetting::Setting1,
        BattlewornDummySetting::Setting2,
        BattlewornDummySetting::Setting3,
    ]
    .into_iter()
    .enumerate()
    {
        let mut rs = RunState::new();
        rs.enter_event_room(EventId::BattlewornDummy);
        let out = dispatch::resolve_by_index(&mut rs, idx).expect("event active");
        assert!(!out.finished, "fight defers event finish to post-combat");
        let req = rs.event_combat.as_ref().expect("combat staged");
        assert!(req.resume, "BattlewornDummy resumes after combat");
        assert_eq!(req.parent_event, EventId::BattlewornDummy);
        assert!(req.victory_rewards.is_empty(), "reward rolled at resume");
        assert_eq!(req.encounter, EventEncounter::BattlewornDummy(setting));
        // current_event must stay open across the fight.
        assert_eq!(rs.current_event, Some(EventId::BattlewornDummy));
    }
}

#[test]
fn build_event_combat_spawns_correct_dummy_with_timer() {
    for (setting, kind, hp) in [
        (
            BattlewornDummySetting::Setting1,
            MonsterKind::BattleFriendV1,
            75,
        ),
        (
            BattlewornDummySetting::Setting2,
            MonsterKind::BattleFriendV2,
            150,
        ),
        (
            BattlewornDummySetting::Setting3,
            MonsterKind::BattleFriendV3,
            300,
        ),
    ] {
        let mut rs = RunState::new();
        let req = sts2_core::events::EventCombatRequest::resuming(
            EventEncounter::BattlewornDummy(setting),
            EventId::BattlewornDummy,
        );
        let combat = build_event_combat(&mut rs, &req);
        assert_eq!(first_enemy_kind(&combat), kind);
        let enemy = combat
            .creatures
            .iter()
            .find(|c| c.species == Some(kind))
            .expect("dummy present");
        assert_eq!(enemy.max_hp, hp, "fixed HP per setting");
        assert!(
            enemy_has_power(&combat, PowerKind::BattlewornDummyTimeLimit),
            "dummy carries the 3-turn time limit"
        );
    }
}

#[test]
fn build_event_combat_advances_niche_exactly_like_a_real_hp_roll() {
    // C# rolls each event-combat enemy's HP off the shared `niche` stream
    // (CreateCreature → SetUniqueMonsterHpValue), even when Min == Max.
    // build_event_combat must consume `niche` identically so the cursor stays
    // aligned with C# for everything drawn afterward.
    use sts2_core::monsters::roll_unique_max_hps;
    let kind = MonsterKind::BattleFriendV3;

    // Reference: roll HP directly off a fresh state's niche, then sample.
    let mut ref_rs = RunState::new();
    let ref_hp = roll_unique_max_hps(&[kind], &mut ref_rs.rng.niche, ref_rs.ascension_level)[0];
    let ref_after = ref_rs.rng.niche.next_int(0, 1_000_000);

    // build_event_combat on an identical fresh state must match both.
    let mut rs = RunState::new();
    let req = sts2_core::events::EventCombatRequest::resuming(
        EventEncounter::BattlewornDummy(BattlewornDummySetting::Setting3),
        EventId::BattlewornDummy,
    );
    let combat = build_event_combat(&mut rs, &req);
    let built_hp = combat
        .creatures
        .iter()
        .find(|c| c.species == Some(kind))
        .expect("dummy")
        .max_hp;
    let built_after = rs.rng.niche.next_int(0, 1_000_000);

    assert_eq!(built_hp, ref_hp, "HP value matches the canonical roll");
    assert_eq!(
        built_after, ref_after,
        "niche cursor advanced by exactly the HP roll (C# parity)"
    );
}

#[test]
fn build_event_combat_mysterious_knight_carries_spawn_buffs() {
    let mut rs = RunState::new();
    let req = sts2_core::events::EventCombatRequest::no_resume(
        EventEncounter::MysteriousKnight,
        EventId::TheLanternKey,
        Vec::new(),
    );
    let niche_before = rs.rng.niche.counter();
    let combat = build_event_combat(&mut rs, &req);
    assert_eq!(first_enemy_kind(&combat), MonsterKind::MysteriousKnight);
    // The knight was created (and its HP rolled) when TheLanternKey's room
    // was entered; the fight reuses it (EventModel.cs:624-627).
    assert_eq!(rs.rng.niche.counter(), niche_before, "no second niche roll");
    // MysteriousKnight.AfterAddedToRoom applies Strength(6) + Plating(6).
    assert!(enemy_has_power(&combat, PowerKind::Strength));
    assert!(enemy_has_power(&combat, PowerKind::Plating));
}

#[test]
fn lantern_key_fight_stages_card_reward_no_resume() {
    let mut rs = RunState::new();
    rs.enter_event_room(EventId::TheLanternKey);
    // Page 0: KeepTheKey opens the Fight page (multi-page).
    let _ = dispatch::resolve(&mut rs, "keep_key").expect("event active");
    assert!(rs.event_combat.is_none(), "page 0 stages no fight");
    // Page 1: Fight stages the real combat.
    let out = dispatch::resolve(&mut rs, "fight").expect("event active");
    assert!(!out.finished);
    let req = rs.event_combat.as_ref().expect("combat staged");
    assert!(!req.resume, "TheLanternKey does not resume");
    assert_eq!(req.encounter, EventEncounter::MysteriousKnight);
    assert_eq!(req.victory_rewards.len(), 1, "LanternKey card reward");
    assert!(matches!(
        req.victory_rewards[0],
        sts2_core::runstate::Reward::Card(_)
    ));
}

#[test]
fn battleworn_resume_grants_only_on_clean_victory() {
    // Setting1: clean victory queues a potion reward; a timeout grants nothing.
    let mut win = RunState::new();
    sts2_content::events::shared::battleworn_dummy_resume(
        &mut win,
        BattlewornDummySetting::Setting1,
        true,
    );
    assert_eq!(win.pending_rewards.len(), 1, "potion queued on victory");

    let mut timeout = RunState::new();
    sts2_content::events::shared::battleworn_dummy_resume(
        &mut timeout,
        BattlewornDummySetting::Setting1,
        false,
    );
    assert!(
        timeout.pending_rewards.is_empty(),
        "RanOutOfTime grants no reward"
    );

    // Setting2: clean victory upgrades cards in place; timeout leaves the
    // deck untouched. (Setting3's relic grant needs a populated grab bag —
    // exercised end-to-end via the run loop, not in this bare-state unit.)
    let mut up_win = RunState::new();
    up_win
        .master_deck
        .push(sts2_core::cards::Card::pod_defaults());
    sts2_content::events::shared::battleworn_dummy_resume(
        &mut up_win,
        BattlewornDummySetting::Setting2,
        false,
    );
    assert!(
        !up_win.master_deck[0].upgraded,
        "no upgrade on RanOutOfTime"
    );
}

/// A real run's state at `version` (populated relic grab bag).
fn initialized_run(version: sts2_core::GameVersion) -> RunState {
    let mut rm = sts2_content::run_callbacks::new_run_manager_for_character(
        7,
        sts2_core::CharacterId::Ironclad,
    )
    .with_game_version(version);
    sts2_content::run_callbacks::initialize_run_setup(&mut rm);
    rm.run_state
}

/// v0.107.1 `BattlewornDummy.cs:105-107`: Setting3 obtains the pulled relic
/// outright (`RelicCmd.Obtain`). Under the C#-faithful
/// `direct_event_relic_grant` (parity mode) it lands in `relics` at resume time.
#[test]
fn battleworn_setting3_obtains_relic() {
    let mut rs = initialized_run(sts2_core::GameVersion::V0_107_1);
    rs.direct_event_relic_grant = true;
    let before = rs.relics.len();
    sts2_content::events::shared::battleworn_dummy_resume(
        &mut rs,
        BattlewornDummySetting::Setting3,
        true,
    );
    assert_eq!(rs.relics.len(), before + 1, "relic obtained");
    assert!(rs.relic_rewards.is_empty(), "nothing offered");
}

/// v0.111.0 `BattlewornDummy.cs:85-88,114`: the same pull becomes a
/// `RelicReward` in the `OfferCustom` list — offered, not obtained, even under
/// `direct_event_relic_grant` (the run loop drains it with the other rewards).
#[test]
fn battleworn_setting3_offers_relic_reward_v0111() {
    let mut rs = initialized_run(sts2_core::GameVersion::V0_111_0);
    rs.direct_event_relic_grant = true;
    let before = rs.relics.clone();
    sts2_content::events::shared::battleworn_dummy_resume(
        &mut rs,
        BattlewornDummySetting::Setting3,
        true,
    );
    assert_eq!(rs.relics, before, "nothing obtained before the pick");
    assert_eq!(rs.relic_rewards.len(), 1, "one relic offered");
    assert!(
        !before.contains(&rs.relic_rewards[0].relic_id),
        "the offer is an unowned grab-bag relic"
    );
    // A timeout still yields nothing.
    let mut timeout = initialized_run(sts2_core::GameVersion::V0_111_0);
    sts2_content::events::shared::battleworn_dummy_resume(
        &mut timeout,
        BattlewornDummySetting::Setting3,
        false,
    );
    assert!(timeout.relic_rewards.is_empty());
}

// ── DenseVegetation (GH #817) ──────────────────────────────────────────

/// C# `DenseVegetation.GenerateInitialOptions` offers exactly TrudgeOn and
/// Rest; the page-1 FIGHT option must not leak onto the first page.
#[test]
fn dense_vegetation_page0_offers_trudge_and_rest() {
    let mut rs = RunState::new();
    rs.enter_event_room(EventId::DenseVegetation);
    let opts = dispatch::present(&rs).expect("event active");
    let ids: Vec<&str> = opts.iter().map(|o| o.option_id).collect();
    assert_eq!(ids, ["trudge_on", "rest"]);
}

/// C# `Rest`: the mimicked rest-site heal, then `SetEventState` with the
/// single FIGHT option. The event stays open and no fight is staged yet.
#[test]
fn dense_vegetation_rest_heals_and_opens_fight_page() {
    let mut rs = RunState::new();
    rs.max_hp = 80;
    rs.current_hp = 30;
    rs.enter_event_room(EventId::DenseVegetation);
    let out = dispatch::resolve(&mut rs, "rest").expect("event active");
    assert!(!out.finished, "Rest does not finish the event");
    assert_eq!(rs.current_hp, 30 + 80 * 3 / 10, "30% max-HP rest heal");
    assert!(rs.event_combat.is_none(), "the fight waits for FIGHT");
    assert_eq!(rs.current_event, Some(EventId::DenseVegetation));
    let opts = dispatch::present(&rs).expect("event still active");
    assert_eq!(opts.len(), 1, "page 1 is the single FIGHT option");
    assert_eq!(opts[0].option_id, "fight");
    assert_eq!(opts[0].loc_key, "DENSE_VEGETATION.pages.REST.options.FIGHT");
}

/// C# `Fight`: `EnterCombatWithoutExitingEvent<DenseVegetationEventEncounter>`
/// with no extra rewards and `shouldResumeAfterCombat: false`.
#[test]
fn dense_vegetation_fight_stages_no_resume_wriggler_combat() {
    let mut rs = RunState::new();
    rs.enter_event_room(EventId::DenseVegetation);
    let _ = dispatch::resolve(&mut rs, "rest").expect("event active");
    let out = dispatch::resolve(&mut rs, "fight").expect("event active");
    assert!(!out.finished, "the event stays open across the fight");
    let req = rs.event_combat.as_ref().expect("combat staged");
    assert_eq!(req.encounter, EventEncounter::DenseVegetation);
    assert_eq!(req.parent_event, EventId::DenseVegetation);
    assert!(!req.resume, "DenseVegetation does not resume after combat");
    assert!(req.victory_rewards.is_empty(), "no extraRewards");
    assert!(
        req.encounter.grants_base_room_rewards(),
        "the fight grants its Monster-room rewards"
    );
    assert_eq!(rs.current_event, Some(EventId::DenseVegetation));
}

/// `DenseVegetationEventEncounter`: four Wrigglers in slots
/// `wriggler1..4`, `StartStunned = false`, so slots 1/3 open on NASTY_BITE
/// and 2/4 on WRIGGLE with no spawn-turn stun. HP is one `Niche` roll per
/// Wriggler over the whole roster (C# `CreateCreature` →
/// `SetUniqueMonsterHpValue`).
#[test]
fn build_event_combat_dense_vegetation_spawns_four_slotted_wrigglers() {
    use sts2_core::monsters::roll_unique_max_hps_for;
    let kind = MonsterKind::DenseVegetationWriggler;
    for version in [
        sts2_core::GameVersion::V0_107_1,
        sts2_core::GameVersion::V0_111_0,
    ] {
        let mut ref_rs = RunState::new();
        ref_rs.game_version = version;
        let ref_hps = roll_unique_max_hps_for(
            &[kind; 4],
            &mut ref_rs.rng.niche,
            ref_rs.ascension_level,
            version,
        );
        let ref_after = ref_rs.rng.niche.next_int(0, 1_000_000);

        let mut rs = RunState::new();
        rs.game_version = version;
        let req = sts2_core::events::EventCombatRequest::no_resume(
            EventEncounter::DenseVegetation,
            EventId::DenseVegetation,
            Vec::new(),
        );
        let combat = build_event_combat(&mut rs, &req);
        let wrigglers: Vec<&sts2_core::Creature> = combat
            .creatures
            .iter()
            .filter(|c| c.species.is_some())
            .collect();
        assert_eq!(wrigglers.len(), 4, "four Wrigglers");
        assert!(wrigglers.iter().all(|c| c.species == Some(kind)));
        let hps: Vec<i32> = wrigglers.iter().map(|c| c.max_hp).collect();
        assert_eq!(hps, ref_hps, "slot-order HP rolls ({version:?})");
        assert!(hps.iter().all(|hp| (17..=21).contains(hp)), "A0 HP 17-21");
        assert_eq!(
            rs.rng.niche.next_int(0, 1_000_000),
            ref_after,
            "niche advanced by exactly the four HP rolls"
        );
        let opening: Vec<&str> = wrigglers
            .iter()
            .map(|c| c.ai.as_ref().expect("wriggler ai").current_move().id)
            .collect();
        assert_eq!(
            opening,
            ["NASTY_BITE", "WRIGGLE", "NASTY_BITE", "WRIGGLE"],
            "slot-conditional INIT_MOVE, no SPAWNED_MOVE stun"
        );
        assert!(!combat.is_elite && !combat.is_boss, "a Monster room");
    }
}

// ── Event-fight room rewards + mid-event offers, end to end (GH #817) ──

use sts2_core::run_manager::{ActionResult, Phase, RunAction, RunManager};

/// A content-wired run parked in `Phase::Event` on `event` (entered over the
/// Neow room, which is how a run reaches `Phase::Event` from outside the
/// crate), at `version`, with the relic registry installed.
fn run_in_event(event: EventId, version: sts2_core::GameVersion) -> RunManager {
    sts2_content::relics::dispatch::ensure_installed();
    let mut rm = sts2_content::run_callbacks::new_run_manager_for_character(
        7,
        sts2_core::CharacterId::Ironclad,
    )
    .with_game_version(version);
    sts2_content::run_callbacks::initialize_run_setup(&mut rm);
    let mut rm = rm.with_neow_start();
    rm.run_state.direct_event_relic_grant = true;
    rm.run_state.enter_event_room(event);
    assert_eq!(rm.phase(), Phase::Event);
    rm
}

fn choose(rm: &mut RunManager, option_id: &str) {
    let opts = dispatch::present(&rm.run_state).expect("event active");
    let idx = opts
        .iter()
        .position(|o| o.option_id == option_id)
        .unwrap_or_else(|| panic!("option {option_id} not offered"));
    assert_eq!(
        rm.take_action(RunAction::EventChoice { idx }),
        ActionResult::Advanced
    );
}

const VERSIONS: [sts2_core::GameVersion; 2] = [
    sts2_core::GameVersion::V0_107_1,
    sts2_core::GameVersion::V0_111_0,
];

/// C# `TheLanternKey.Fight` → MysteriousKnight in a Monster `CombatRoom`:
/// the win grants Monster gold (10-20), the card reward, and the
/// `SpecialCardReward(LanternKey)` extra, which the headless auto-collects
/// before the card reward it presents.
#[test]
fn lantern_key_fight_win_grants_room_rewards_and_the_key() {
    for version in VERSIONS {
        let mut rm = run_in_event(EventId::TheLanternKey, version);
        choose(&mut rm, "keep_key");
        assert_eq!(rm.phase(), Phase::Event, "page 1: FIGHT");
        choose(&mut rm, "fight");
        assert_eq!(rm.phase(), Phase::Combat);
        let gold_before = rm.run_state.gold;
        let deck_before = rm.run_state.master_deck.len();
        assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);

        let gained = rm.run_state.gold - gold_before;
        assert!((10..=20).contains(&gained), "Monster gold, got {gained}");
        assert_eq!(rm.run_state.master_deck.len(), deck_before + 1);
        assert!(rm
            .run_state
            .master_deck
            .iter()
            .any(|c| c.card_id == sts2_core::cards::CardId::LanternKey));
        assert_eq!(rm.phase(), Phase::CardReward);
        assert_eq!(rm.card_reward_options().len(), 3, "{version:?}");
        assert!(!rm
            .card_reward_options()
            .contains(&sts2_core::cards::CardId::LanternKey));
        assert!(rm.run_state.current_event.is_none());
        rm.take_action(RunAction::PickCardReward { idx: None });
        assert_eq!(rm.phase(), Phase::MapChoice);
    }
}

/// C# `FakeMerchant.FoulPotionThrown` → `FakeMerchantEventEncounter`
/// (`MinGoldReward = MaxGoldReward = 300`, no Poverty), plus the Rug and the
/// stocked relics as extras, obtained before the card reward.
#[test]
fn fake_merchant_fight_win_grants_300_gold_relics_and_card_reward() {
    for version in VERSIONS {
        let mut rm = run_in_event(EventId::FakeMerchant, version);
        // Poverty would cut a base-getter bound to 0.75x; the override skips it.
        rm.run_state.ascension_level = 3;
        rm.run_state.ooc_potion_actions = true;
        rm.run_state.potions = vec![sts2_core::potions::PotionKind::FoulPotion];
        rm.run_state.potion_slot_indices.clear();
        // The Fake Merchant is a shop page (GH #1607); the potion is thrown
        // from it.
        dispatch::begin_event_in_entered_room(&mut rm.run_state, EventId::FakeMerchant);
        rm.bind_pending_event();
        assert_eq!(rm.phase(), Phase::Shop, "{version:?}");
        assert_eq!(
            rm.take_action(RunAction::UsePotionOutOfCombat { potion_slot: 0 }),
            ActionResult::Advanced
        );
        assert_eq!(rm.phase(), Phase::Combat);
        let req = rm.run_state.event_combat.clone().expect("fight staged");
        let extras: Vec<RelicId> = req
            .victory_rewards
            .iter()
            .filter_map(|r| match r {
                sts2_core::runstate::Reward::Relic(rr) => Some(rr.relic_id),
                _ => None,
            })
            .collect();
        assert_eq!(extras.len(), 7, "Rug + 6 stocked relics");
        let gold_before = rm.run_state.gold;
        assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);

        assert_eq!(rm.run_state.gold - gold_before, 300, "{version:?}");
        assert_eq!(rm.run_state.combat_gold_bounds_override, None);
        for id in &extras {
            assert!(rm.run_state.relics.contains(id), "{id:?} obtained");
        }
        assert!(rm.run_state.relic_rewards.is_empty());
        assert_eq!(rm.phase(), Phase::CardReward);
        assert_eq!(rm.card_reward_options().len(), 3);
        rm.take_action(RunAction::PickCardReward { idx: Some(0) });
        assert_eq!(rm.phase(), Phase::MapChoice);
    }
}

/// C# `DenseVegetation.Rest` awaits `PlayerCmd.MimicRestSiteHeal`, whose
/// `ExecuteRestSiteHeal` offers Dream Catcher's card reward
/// (`RewardsCmd.OfferCustom`) BEFORE `SetEventState` shows FIGHT. The card
/// reward therefore surfaces during Rest, then the event resumes on FIGHT,
/// and the Wriggler win offers only its own combat card reward.
#[test]
fn dense_vegetation_rest_offers_dream_catcher_card_before_fight() {
    for version in VERSIONS {
        let mut rm = run_in_event(EventId::DenseVegetation, version);
        rm.run_state.relics.push(RelicId::DreamCatcher);
        rm.run_state.current_hp = 30;
        choose(&mut rm, "rest");
        assert_eq!(
            rm.phase(),
            Phase::CardReward,
            "Dream Catcher's card comes during Rest ({version:?})"
        );
        assert_eq!(rm.card_reward_options().len(), 3);
        let deck_before = rm.run_state.master_deck.len();
        rm.take_action(RunAction::PickCardReward { idx: Some(0) });
        assert_eq!(rm.run_state.master_deck.len(), deck_before + 1);
        assert_eq!(rm.phase(), Phase::Event, "back to the FIGHT page");
        assert_eq!(rm.run_state.current_event, Some(EventId::DenseVegetation));

        choose(&mut rm, "fight");
        assert_eq!(rm.phase(), Phase::Combat);
        assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);
        assert_eq!(rm.phase(), Phase::CardReward);
        assert!(rm.run_state.pending_rewards.is_empty(), "nothing deferred");
        rm.take_action(RunAction::PickCardReward { idx: None });
        assert_eq!(rm.phase(), Phase::MapChoice, "one card reward only");
    }
}

// ── PunchOff: the "I Can Take Them" fight ──

/// C# `PunchOffEventEncounter`: two Punch Constructs, the first opening on
/// FAST_PUNCH and the second on READY, each `StartingHpReduction =
/// NextInt(2, 10)` off the encounter's own Rng (seeded like a map fight's at
/// this floor), applied to current HP after its Artifact. The HP itself was
/// rolled off `niche` when the Combat-layout room was entered, so the fight
/// rolls none.
#[test]
fn build_event_combat_punch_off_spawns_two_staggered_worn_constructs() {
    for version in VERSIONS {
        let mut rs = RunState::new();
        rs.game_version = version;
        rs.total_floor = 7;
        let req = sts2_core::events::EventCombatRequest::no_resume(
            EventEncounter::PunchOff,
            EventId::PunchOff,
            Vec::new(),
        );
        let niche_before = rs.rng.niche.counter();
        let combat = build_event_combat(&mut rs, &req);
        assert_eq!(rs.rng.niche.counter(), niche_before, "no second niche roll");
        let constructs: Vec<&sts2_core::Creature> = combat
            .creatures
            .iter()
            .filter(|c| c.species.is_some())
            .collect();
        assert_eq!(constructs.len(), 2);
        assert!(constructs
            .iter()
            .all(|c| c.species == Some(MonsterKind::PunchConstruct)));
        let opening: Vec<&str> = constructs
            .iter()
            .map(|c| c.ai.as_ref().expect("construct ai").current_move().id)
            .collect();
        assert_eq!(opening, ["FAST_PUNCH", "READY"], "{version:?}");

        let mut enc = rs.rng.named_at(
            i64::from(rs.total_floor + i32::from(rs.act_index)),
            "PUNCH_OFF_EVENT_ENCOUNTER",
        );
        for c in &constructs {
            let reduction = enc.next_int_exclusive(2, 10);
            assert!((2..=9).contains(&reduction));
            assert_eq!(c.max_hp, 55, "A0 max HP (60 from Tough Enemies), untouched");
            assert_eq!(c.current_hp, c.max_hp - reduction, "{version:?}");
        }
        assert!(enemy_has_power(&combat, PowerKind::Artifact));
    }
}

#[test]
fn punch_off_take_them_opens_the_fight_page_and_fight_stages_the_combat() {
    let mut rs = RunState::new();
    rs.enter_event_room(EventId::PunchOff);
    let page0: Vec<&str> = dispatch::present(&rs)
        .expect("event active")
        .iter()
        .map(|o| o.option_id)
        .collect();
    assert_eq!(page0, ["nab", "take_them"]);
    let out = dispatch::resolve_by_index(&mut rs, 1).expect("event active");
    assert!(!out.finished);
    assert!(rs.event_combat.is_none(), "no fight before FIGHT");
    let page1: Vec<&str> = dispatch::present(&rs)
        .expect("event active")
        .iter()
        .map(|o| o.option_id)
        .collect();
    assert_eq!(page1, ["fight"]);
    let out = dispatch::resolve_by_index(&mut rs, 0).expect("event active");
    assert!(!out.finished, "the fight defers the event's finish");
    let req = rs.event_combat.as_ref().expect("combat staged");
    assert_eq!(req.encounter, EventEncounter::PunchOff);
    assert!(!req.resume);
    assert_eq!(req.parent_event, EventId::PunchOff);
    assert!(matches!(
        req.victory_rewards.as_slice(),
        [sts2_core::runstate::Reward::Potion(_)]
    ));
    assert_eq!(EventEncounter::PunchOff.extra_random_relics(), 1);
    assert!(rs.pending_rewards.is_empty(), "nothing granted up front");
}

/// The won fight grants the Monster-room rewards plus PunchOff's extras: a
/// relic rolled at populate time and a potion.
#[test]
fn punch_off_fight_win_grants_room_rewards_a_rolled_relic_and_a_potion() {
    for version in VERSIONS {
        let mut rm = run_in_event(EventId::PunchOff, version);
        choose(&mut rm, "take_them");
        assert_eq!(rm.phase(), Phase::Event, "page 1: FIGHT");
        choose(&mut rm, "fight");
        assert_eq!(rm.phase(), Phase::Combat);
        assert_eq!(
            rm.run_state.event_combat.as_ref().map(|r| r.encounter),
            Some(EventEncounter::PunchOff)
        );
        let relics_before = rm.run_state.relics.len();
        let gold_before = rm.run_state.gold;
        assert_eq!(rm.resolve_combat_fast(0.0, true), ActionResult::Advanced);

        let gained = rm.run_state.gold - gold_before;
        assert!((10..=20).contains(&gained), "Monster gold, got {gained}");
        assert_eq!(
            rm.run_state.relics.len(),
            relics_before + 1,
            "one rolled relic ({version:?})"
        );
        assert!(!rm.run_state.relics.contains(&RelicId::Astrolabe));
        assert!(rm.run_state.current_event.is_none());
        assert_eq!(rm.phase(), Phase::CardReward);
        assert_eq!(rm.card_reward_options().len(), 3);
    }
}
