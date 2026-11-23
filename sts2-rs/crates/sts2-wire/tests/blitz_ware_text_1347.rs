//! GH #1347: the Blitz merchant's wares are titled and described with the
//! game's own words.
//!
//! `blitz_ware_text` carries the English `merchant_room` rows of v0.113.0
//! (`MERCHANT.blitz.<snake_kind>_entry.title` / `.description`,
//! `BlitzMerchantEntry.cs:57-59`) so both workbench backends print one copy.
//! This pins all twenty against the vendored loc table, with the variables
//! filled the way the three C# entries that have any fill them.

use std::path::Path;

use serde_json::Value;
use sts2_core::blitz::{MerchantEntry, MerchantEntryType};
use sts2_core::cards::EnchantmentId;
use sts2_core::CharacterId;
use sts2_wire::emit::run_manager::blitz_ware_text;

/// `StringHelper.SnakeCase` of an enum member: `TwoRelics` -> `two_relics`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// The vendored v0.113.0 loc table, or `None` where the sibling checkout
/// (`sts2-pycli/localization_eng_v0.113.0/`) is not present.
fn loc(table: &str) -> Option<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../sts2-pycli/localization_eng_v0.113.0")
        .join(format!("{table}.json"));
    let text = std::fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&text).expect("loc table parses"))
}

fn entry(kind: MerchantEntryType) -> MerchantEntry {
    MerchantEntry::new(kind)
}

#[test]
fn every_ware_reads_as_the_game_writes_it() {
    let (Some(merchant), Some(enchantments), Some(characters)) =
        (loc("merchant_room"), loc("enchantments"), loc("characters"))
    else {
        eprintln!("skip: no vendored v0.113.0 localization beside this checkout");
        return;
    };
    let row = |table: &Value, key: &str| -> String {
        table[key]
            .as_str()
            .unwrap_or_else(|| panic!("no loc row {key}"))
            .to_string()
    };
    for kind in MerchantEntryType::ALL {
        let stem = format!("MERCHANT.blitz.{}_entry", snake(kind.name()));
        let mut want_title = row(&merchant, &format!("{stem}.title"));
        let mut want_desc = row(&merchant, &format!("{stem}.description"));
        let mut ware = entry(kind);
        let mut amount = 1;
        match kind {
            // `BlitzMerchantCardEnchantEntry`: the rolled enchantment's
            // title and amount.
            MerchantEntryType::CardEnchant => {
                ware.enchantment = Some((EnchantmentId::Sharp, 2));
                amount = 2;
                let name = row(&enchantments, "SHARP.title");
                want_title = want_title.replace("{Enchantment}", &name);
                want_desc = want_desc.replace("{Enchantment}", &name);
            }
            // `BlitzMerchantGlamEntry`: always Glam, amount 1.
            MerchantEntryType::Glam => {
                let name = row(&enchantments, "GLAM.title");
                want_title = want_title.replace("{Enchantment}", &name);
                want_desc = want_desc.replace("{Enchantment}", &name);
            }
            // `BlitzMerchantPrismaticCardRewardEntry`: the other
            // character's `ShortTitle`.
            MerchantEntryType::PrismaticCardReward => {
                ware.character = Some(CharacterId::Silent);
                let name = row(&characters, "SILENT.shortTitle");
                want_title = want_title.replace("{Character}", &name);
                want_desc = want_desc.replace("{Character}", &name);
            }
            _ => {}
        }
        // `{Amount:plural:| {}}`: nothing at 1, else the number.
        let plural = if amount == 1 {
            String::new()
        } else {
            format!(" {amount}")
        };
        want_desc = want_desc.replace("{Amount:plural:| {}}", &plural);
        assert!(
            !want_title.contains('{') && !want_desc.contains('{'),
            "{kind:?}"
        );
        assert_eq!(blitz_ware_text(&ware), (want_title, want_desc), "{kind:?}");
    }
}

/// The variables, without the loc table: every enchantment the merchant can
/// roll and every character, an amount of 1 printed as no number, and the
/// cursed wares leading with the Curse.
#[test]
fn the_variables_are_filled() {
    for (id, amount, title, tail) in [
        (EnchantmentId::Sharp, 2, "Sharp", "Sharp 2"),
        (EnchantmentId::Nimble, 3, "Nimble", "Nimble 3"),
        (EnchantmentId::Adroit, 3, "Adroit", "Adroit 3"),
        (EnchantmentId::Steady, 1, "Steady", "Steady"),
        (EnchantmentId::Swift, 2, "Swift", "Swift 2"),
        (EnchantmentId::Vigorous, 5, "Vigorous", "Vigorous 5"),
    ] {
        let mut ware = entry(MerchantEntryType::CardEnchant);
        ware.enchantment = Some((id, amount));
        let (got_title, got_desc) = blitz_ware_text(&ware);
        assert_eq!(got_title, title);
        assert_eq!(
            got_desc,
            format!("[gold]Enchant[/gold] a card with [purple]{tail}[/purple].")
        );
    }
    for (who, name) in [
        (CharacterId::Ironclad, "Ironclad"),
        (CharacterId::Silent, "Silent"),
        (CharacterId::Defect, "Defect"),
        (CharacterId::Regent, "Regent"),
        (CharacterId::Necrobinder, "Necrobinder"),
    ] {
        let mut ware = entry(MerchantEntryType::PrismaticCardReward);
        ware.character = Some(who);
        assert_eq!(blitz_ware_text(&ware).0, format!("{name} Card Reward"));
    }
    for kind in MerchantEntryType::ALL {
        let (title, desc) = blitz_ware_text(&entry(kind));
        assert!(!title.is_empty() && !desc.is_empty(), "{kind:?}");
        assert!(!title.contains('{') && !desc.contains('{'), "{kind:?}");
        assert_eq!(
            desc.starts_with("Receive [red]1[/red] random [red]Curse[/red].\n"),
            kind.is_cursed(),
            "{kind:?}"
        );
    }
}
