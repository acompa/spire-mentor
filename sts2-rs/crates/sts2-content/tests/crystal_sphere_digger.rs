//! The Crystal Sphere digger (`events::crystal_sphere_digger`): a click
//! policy for the minigame. The golden file holds the clicks the reference
//! implementation makes (Big tool only) on 200 boards with 3 and 6
//! divinations; the port must make the same clicks.

use sts2_content::events::crystal_sphere::{
    CrystalSphereItemKind, CrystalSphereMinigame, CrystalSphereTool, HEADLESS_SWEEP_CENTERS,
};
use sts2_content::events::crystal_sphere_digger::{pick, ItemClass, SphereView};
use sts2_core::dotnet_rng::Rng;
use sts2_core::PotionRarity;

/// Plays the digger on seed `seed` with `digs` divinations.
fn play(seed: u64, digs: u32) -> (Vec<(i32, i32)>, CrystalSphereMinigame) {
    let mut game = CrystalSphereMinigame::new(&mut Rng::from_seed(seed), digs);
    game.set_tool(CrystalSphereTool::Big);
    let mut clicks = Vec::new();
    while let Some((x, y)) = pick(&game.view(digs)) {
        assert!(
            game.is_hidden(x, y),
            "seed {seed}: picked a cleared cell ({x},{y})"
        );
        clicks.push((x, y));
        game.cell_clicked(x, y);
    }
    (clicks, game)
}

/// Gold-equivalents of a revealed item, by kind and shape.
fn value(kind: &CrystalSphereItemKind) -> f64 {
    match kind {
        CrystalSphereItemKind::Relic => 200.0,
        CrystalSphereItemKind::Potion(PotionRarity::Rare) => 100.0,
        CrystalSphereItemKind::Potion(_) => 50.0,
        CrystalSphereItemKind::CardReward(_) => 92.0,
        CrystalSphereItemKind::Curse => -100.0,
        CrystalSphereItemKind::Gold { big: false } => 10.0,
        CrystalSphereItemKind::Gold { big: true } => 30.0,
    }
}

fn total_value(game: &CrystalSphereMinigame) -> f64 {
    game.revealed().iter().map(value).sum()
}

#[test]
fn golden_clicks_match_the_reference() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/crystal_sphere_digger_golden.json"
    );
    let text = std::fs::read_to_string(path).expect("golden file");
    let golden: serde_json::Value = serde_json::from_str(&text).expect("golden json");
    let entries = golden.as_array().expect("an array");
    assert_eq!(entries.len(), 400);
    for entry in entries {
        let seed = entry["seed"].as_u64().unwrap();
        let digs = u32::try_from(entry["digs"].as_u64().unwrap()).unwrap();
        let want: Vec<(i32, i32)> = entry["clicks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let xy = c.as_array().unwrap();
                (
                    i32::try_from(xy[0].as_i64().unwrap()).unwrap(),
                    i32::try_from(xy[1].as_i64().unwrap()).unwrap(),
                )
            })
            .collect();
        assert_eq!(play(seed, digs).0, want, "seed {seed}, {digs} divinations");
    }
}

#[test]
fn six_divinations_always_reveal_the_relic() {
    for seed in 9_100_000..9_100_500 {
        let (_, game) = play(seed, 6);
        assert!(
            game.revealed().contains(&CrystalSphereItemKind::Relic),
            "seed {seed}: no relic"
        );
    }
}

#[test]
fn three_divinations_beat_the_sweep() {
    let seeds = 9_100_000..9_100_500u64;
    let n = seeds.clone().count() as f64;
    let (mut digger, mut sweep) = (0.0, 0.0);
    for seed in seeds {
        digger += total_value(&play(seed, 3).1);
        let mut game = CrystalSphereMinigame::new(&mut Rng::from_seed(seed), 3);
        game.set_tool(CrystalSphereTool::Big);
        for &(x, y) in &HEADLESS_SWEEP_CENTERS[..3] {
            game.cell_clicked(x, y);
        }
        sweep += total_value(&game);
    }
    let (digger, sweep) = (digger / n, sweep / n);
    assert!(
        digger >= sweep + 40.0,
        "digger mean {digger:.1} vs sweep mean {sweep:.1}"
    );
}

#[test]
fn pick_is_none_when_nothing_to_do() {
    let game = CrystalSphereMinigame::new(&mut Rng::from_seed(9_000_000), 6);
    let mut view = game.view(6);
    view.hidden = 0;
    assert_eq!(pick(&view), None);
    let mut view = game.view(6);
    view.remaining = 0;
    assert_eq!(pick(&view), None);
}

#[test]
fn hand_played_board_still_gets_a_hidden_cell() {
    let mut game = CrystalSphereMinigame::new(&mut Rng::from_seed(9_000_001), 6);
    game.set_tool(CrystalSphereTool::Small);
    game.cell_clicked(5, 5);
    game.set_tool(CrystalSphereTool::Big);
    game.cell_clicked(3, 2);
    assert_eq!(game.divinations(), 4);
    for total in [6, 4] {
        let view = game.view(total);
        assert_eq!(view.remaining, 4);
        let (x, y) = pick(&view).expect("a cell");
        assert!(game.is_hidden(x, y), "total {total}: ({x},{y}) is cleared");
    }
}

#[test]
fn inconsistent_pieces_do_not_panic() {
    let game = CrystalSphereMinigame::new(&mut Rng::from_seed(9_000_002), 6);
    let fresh = game.view(6).hidden;
    let mut shown = vec![((2, 2), ItemClass::Relic), ((9, 9), ItemClass::Relic)];
    for cell in [(5, 5), (5, 6), (6, 5), (6, 6), (7, 7)] {
        shown.push((cell, ItemClass::Potion));
    }
    let view = SphereView {
        hidden: fresh,
        shown,
        remaining: 6,
        total: 6,
    };
    let (x, y) = pick(&view).expect("a cell");
    assert!(game.is_hidden(x, y), "({x},{y}) is cleared");
}

#[test]
fn same_view_same_pick() {
    // Two boards' fresh screens are the same screen: equal picks.
    let a = CrystalSphereMinigame::new(&mut Rng::from_seed(9_000_003), 6);
    let mut b = CrystalSphereMinigame::new(&mut Rng::from_seed(9_000_004), 6);
    assert_eq!(a.view(6), b.view(6));
    assert_eq!(pick(&a.view(6)), pick(&b.view(6)));
    // Mid-game: one board's screen, copied field by field (the exhaustive
    // destructure fails to compile if SphereView grows a field), picks alike.
    b.set_tool(CrystalSphereTool::Big);
    let (x, y) = pick(&b.view(6)).unwrap();
    b.cell_clicked(x, y);
    let view = b.view(6);
    let SphereView {
        hidden,
        shown,
        remaining,
        total,
    } = view.clone();
    let copy = SphereView {
        hidden,
        shown,
        remaining,
        total,
    };
    assert_eq!(pick(&view), pick(&copy));
}
