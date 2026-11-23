//! The Fake Merchant's shop against the C# headless host (GH #1607): the
//! stock and prices `RunSimulator` (the real game DLL) shows for
//! `start(seed)` → `skip_neow` → `enter_room event FAKE_MERCHANT`, recorded
//! 2026-11-20. They pin the shuffle off the `FAKE_MERCHANT` stream
//! (`FakeMerchant.BeforeEventStarted`, `UnstableShuffle(base.Rng).Take(6)`)
//! and the six `Shops` price rolls (`MerchantRelicEntry.CalcCost`, 50 ×
//! `NextFloat(0.85, 1.15)`). The Rust run is built by the constructor the
//! Python binding uses, then walked through Neow on its first option.

use sts2_core::run_manager::{Phase, RunManager};
use sts2_wire::emit::run_manager as emit;
use sts2_wire::run_builder::{build_run, RunOptions};

type Stock = [(&'static str, i64); 6];

const V0_111_0: [(&str, Stock); 3] = [
    (
        "FAKEM",
        [
            ("FAKE_SNECKO_EYE", 47),
            ("FAKE_ORICHALCUM", 57),
            ("FAKE_MANGO", 44),
            ("FAKE_BLOOD_VIAL", 46),
            ("FAKE_STRIKE_DUMMY", 45),
            ("FAKE_ANCHOR", 43),
        ],
    ),
    (
        "SEED2",
        [
            ("FAKE_MANGO", 46),
            ("FAKE_LEES_WAFFLE", 45),
            ("FAKE_ORICHALCUM", 52),
            ("FAKE_SNECKO_EYE", 46),
            ("FAKE_HAPPY_FLOWER", 56),
            ("FAKE_STRIKE_DUMMY", 44),
        ],
    ),
    (
        "ABC123",
        [
            ("FAKE_VENERABLE_TEA_SET", 53),
            ("FAKE_LEES_WAFFLE", 50),
            ("FAKE_STRIKE_DUMMY", 56),
            ("FAKE_ANCHOR", 44),
            ("FAKE_ORICHALCUM", 50),
            ("FAKE_HAPPY_FLOWER", 52),
        ],
    ),
];

/// v0.113.1 builds a `MerchantInventory` of relic entries only; its
/// `Setup` draws nothing more, so the stream use is v0.111.0's (the relic
/// list order differs, hence the ids).
const V0_113_0: [(&str, Stock); 2] = [
    (
        "FAKEM",
        [
            ("FAKE_VENERABLE_TEA_SET", 47),
            ("FAKE_STRIKE_DUMMY", 57),
            ("FAKE_LEES_WAFFLE", 44),
            ("FAKE_SNECKO_EYE", 46),
            ("FAKE_BLOOD_VIAL", 45),
            ("FAKE_ANCHOR", 43),
        ],
    ),
    (
        "SEED2",
        [
            ("FAKE_LEES_WAFFLE", 46),
            ("FAKE_VENERABLE_TEA_SET", 45),
            ("FAKE_HAPPY_FLOWER", 52),
            ("FAKE_ORICHALCUM", 46),
            ("FAKE_MANGO", 56),
            ("FAKE_STRIKE_DUMMY", 44),
        ],
    ),
];

fn at_the_fake_merchant(seed: &str, version: &str) -> RunManager {
    let (mut rm, _) = build_run(&RunOptions {
        seed: 0,
        character: "Ironclad",
        deck: None,
        max_hp: None,
        string_seed: Some(seed),
        ascension_level: 0,
        neow_start: true,
        interactive_card_removal: None,
        direct_event_relic_grant: None,
        interactive_card_upgrade: None,
        interactive_card_transform: None,
        interactive_potion_swap: false,
        ooc_potion_actions: false,
        deck_select_screens: false,
        reward_screen: false,
        act1: "roll",
        game_version: Some(version),
        mode: None,
        game_mode: None,
    })
    .expect("builds");
    for _ in 0..50 {
        if rm.phase() == Phase::MapChoice {
            break;
        }
        rm.bind_pending_event();
        let first = rm.get_available_actions()[0];
        let _ = rm.take_action(first);
    }
    assert_eq!(rm.phase(), Phase::MapChoice, "{seed}: through Neow");
    rm.run_state.gold = 500;
    sts2_wire::graft::enter_event(&mut rm, "FAKE_MERCHANT").expect("enters");
    rm
}

fn check(version: &str, fixture: &[(&str, Stock)]) {
    for (seed, want) in fixture {
        let rm = at_the_fake_merchant(seed, version);
        assert!(rm.in_fake_merchant_shop(), "{seed}");
        let inv = emit::shop_inventory(&rm);
        let got: Vec<(String, i64)> = inv["relics"]
            .as_array()
            .expect("relics")
            .iter()
            .map(|r| {
                let id = r["id"].as_str().expect("id");
                (
                    id.strip_prefix("RELIC.").unwrap_or(id).to_owned(),
                    r["price"].as_i64().expect("price"),
                )
            })
            .collect();
        let want: Vec<(String, i64)> = want.iter().map(|&(r, p)| (r.to_owned(), p)).collect();
        assert_eq!(got, want, "{version} {seed}");
    }
}

#[test]
fn v0_111_0_stock_and_prices_match_the_csharp_host() {
    check("v0.111.0", &V0_111_0);
}

#[test]
fn v0_113_0_stock_and_prices_match_the_csharp_host() {
    check("v0.113.0", &V0_113_0);
}
