//! Every card's `CardType`, in every game build the engine plays, is the
//! decomp's: the `CardType` its C# constructor passes to `CardModel`
//! (`base(cost, CardType.X, rarity, target)`).
//!
//! The type gates more than the card's own pile routing: an affliction's
//! `CanAfflictCardType` (Tainted is Skill-only), `Hook`s that filter on
//! Attacks / Skills / Powers, and the legality of a Power play. A card whose
//! type changed in a build (v0.111.0 Outbreak: Power -> Skill) has to read
//! the build's type through [`sts2_content::cards::v0111::spec_for`].
//!
//! Reads the sibling `sts2-wiki` checkout's per-build decomp (or
//! `STS2_DECOMP_ROOT`, the directory holding `v0.107.1/`, `v0.111.0/`, …) and
//! skips a build whose tree is absent.

use std::path::{Path, PathBuf};

use sts2_content::cards::spec_index::spec_for_card_id;
use sts2_content::cards::v0111::spec_for;
use sts2_content::names::card_id_from_str;
use sts2_core::cards::{CardId, CardType};
use sts2_core::GameVersion;

fn decomp_root() -> PathBuf {
    std::env::var_os("STS2_DECOMP_ROOT").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../sts2-wiki/decompiled"),
        PathBuf::from,
    )
}

/// `(class, CardType)` of every card class in `dir` whose constructor names
/// its type.
fn decomp_card_types(dir: &Path) -> Vec<(String, CardType)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("decomp dir").flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("cs") {
            continue;
        }
        let class = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let src = std::fs::read_to_string(&path).expect("read .cs");
        let Some(at) = src.find(": base(") else {
            continue;
        };
        let Some(ty) = src[at..].find("CardType.").map(|i| &src[at + i + 9..]) else {
            continue;
        };
        let name: String = ty.chars().take_while(char::is_ascii_alphabetic).collect();
        let ct = match name.as_str() {
            "Attack" => CardType::Attack,
            "Skill" => CardType::Skill,
            "Power" => CardType::Power,
            "Curse" => CardType::Curse,
            "Status" => CardType::Status,
            "Quest" => CardType::Quest,
            other => panic!("{class}: unknown CardType.{other}"),
        };
        out.push((class, ct));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn card_types_match_the_decomp_in_every_build() {
    let builds = [
        ("v0.107.1", GameVersion::V0_107_1),
        ("v0.111.0", GameVersion::V0_111_0),
        ("v0.113.0", GameVersion::V0_113_0),
    ];
    let mut checked_any = false;
    let mut mismatches = Vec::new();
    for (dir, version) in builds {
        let cards = decomp_root()
            .join(dir)
            .join("MegaCrit.Sts2.Core.Models.Cards");
        if !cards.is_dir() {
            eprintln!("skipping {dir}: no decompiled cards at {}", cards.display());
            continue;
        }
        let types = decomp_card_types(&cards);
        assert!(types.len() > 400, "{dir}: only {} cards", types.len());
        checked_any = true;
        let mut matched = 0;
        for (class, decomp) in &types {
            let Some(id) = card_id_from_str(class) else {
                continue;
            };
            // MadScience's type is per instance (`TinkerTimeType`); the spec
            // holds its default-constructed Attack build.
            if id == CardId::MadScience {
                continue;
            }
            let Some(spec) = spec_for_card_id(id) else {
                continue;
            };
            matched += 1;
            let ours = spec_for(spec, version).card_type;
            if ours != *decomp {
                mismatches.push(format!("{dir} {class}: ours {ours:?}, decomp {decomp:?}"));
            }
        }
        assert!(matched > 400, "{dir}: only {matched} cards resolved");
    }
    if !checked_any {
        eprintln!("skipping: no decompiled cards (set STS2_DECOMP_ROOT)");
        return;
    }
    assert!(
        mismatches.is_empty(),
        "card types drifted:\n{}",
        mismatches.join("\n")
    );
}
