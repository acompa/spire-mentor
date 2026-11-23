//! GH #1693: a rebuilt fight keeps what a Thieving Hopper stole. The engine
//! emits each thief's `stolen_cards`; the parse put none back, so the
//! rebuilt fight (the bench's mirror) showed the thief holding nothing.

use sts2_wire::emit::combat::combat_state_to_value;

#[test]
fn a_rebuilt_fight_keeps_the_thiefs_card() {
    let def = (0..sts2_content::encounters::n_encounters())
        .filter_map(sts2_content::encounters::by_index)
        .find(|d| d.id.contains("THIEVING_HOPPER"))
        .expect("a Thieving Hopper encounter");
    let mut state = sts2_wire::scenario::build_scenario_state_for_version(
        sts2_core::GameVersion::V0_111_0,
        7,
        "Ironclad",
        &vec!["STRIKE_IRONCLAD".to_owned(); 10],
        80,
        80,
        &[],
        &[],
        def.id,
        10,
        None,
        None,
    )
    .expect("scenario");
    let thief = state.enemy_uids[0];
    let card = sts2_wire::state_dict::card_id_from_python_name("STRIKE_IRONCLAD").expect("card");
    state.record_swipe_steal(thief, card, true);
    let wire = combat_state_to_value(&state).expect("serializes");
    let back = sts2_wire::state_from_json(&wire).expect("round-trips");
    let held: Vec<_> = back
        .swipe_stolen
        .iter()
        .map(|s| (s.thief, s.card_id, s.upgraded, s.reclaimed))
        .collect();
    assert_eq!(held, vec![(back.enemy_uids[0], card, true, false)]);
}
