//! The checked item in Craft of Exile's crafting simulator (beta.craftofexile.com): its base, item
//! level, rarity, implicits and mods, whatever the game client's language.
//!
//! The site imports an item from the JSON its own export writes (`GameItem.exportData` and
//! `importData` in its `package_poe2.js`, read 2026-09-23): the base by the game's metadata id,
//! each mod by the game's mod id with its rolls in the game's order of the mod's stats. Its text
//! import reads the English advanced copy alone -- a Russian one loses every mod -- so the link
//! names the item by those ids, which the tables hold for either language: `item_refs` for the
//! base and its implicits, `stat_filters::game_mod` for a mod. A mod whose id isn't known for sure
//! is left out rather than guessed.
//!
//! The address is `?game=poe2&eimport=<JSON>`: the site's importer takes an export there as well
//! as a text, and then shows the item under an address of its own, `details` (the same JSON in
//! base64) beside the site's numeric ids of the item's group, class and base. `details` without
//! those ids opens the site's start page, not the item (checked in the browser 2026-09-23), and
//! this app doesn't know the site's numbering.

use item_parser::roll::{NumericRun, find_numeric_runs};
use percent_encoding::utf8_percent_encode;
use poe2_domain::{
    ItemRarity, ModGeneration, ModifierType, ParsedItem, ParsedModifier, ParsedStat,
};
use serde::Serialize;
use serde_json::Number;
use stat_filters::Roll;

use crate::bug_report::QUERY_VALUE;
use crate::i18n::Lang;
use crate::item_refs::{self, Base, Implicit};

/// Craft of Exile's current site; the old craftofexile.com is a patch behind.
const SITE: &str = "https://beta.craftofexile.com/";

/// Mods the site doesn't list (its data for 4.5.5.3, checked 2026-09-23): the desecrated mods of
/// radius jewels. A mod it doesn't know fails the whole import, so these stay out of the link.
const UNLISTED_MODS: &[&str] = &["AbyssModRadiusJewel"];

/// The site's interface language for the app's, `lang` (`i18n::lang`): its Russian one for a
/// Russian interface, its default, English, otherwise -- whatever the item's language, which the
/// link doesn't depend on.
pub fn site_language(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::Russian => Some("ru"),
        Lang::English => None,
    }
}

/// The address that opens `item` in Craft of Exile, the site in `language` (`site_language`).
/// `None` for an item the site doesn't craft -- a unique, currency, a gem, a relic or a tablet --
/// for an unidentified one, whose mods it hides, and for one whose base the tables don't name for
/// sure.
pub fn url(item: &ParsedItem, language: Option<&str>) -> Option<String> {
    let export = export(item)?;
    let json = serde_json::to_string(&export).expect("an export serializes");
    let mut url = format!(
        "{SITE}?game=poe2&eimport={}",
        utf8_percent_encode(&json, QUERY_VALUE)
    );
    if let Some(language) = language {
        url.push_str("&language=");
        url.push_str(language);
    }
    Some(url)
}

/// The site's item export, as `exportData` writes it, with the keys the item fills: `importData`
/// needs `m`, `s`, `if` and `f`, and `ip` when the base has implicits; the rest default.
#[derive(Serialize)]
struct Export {
    /// The base's metadata id.
    i: &'static str,
    m: Vec<Mod>,
    /// Rune sockets.
    ns: u32,
    /// What fills them: the runes' ids aren't known here, so nothing.
    s: [u8; 0],
    /// Quality.
    q: u32,
    /// Item level.
    l: u32,
    /// `normal`, `magic` or `rare`.
    r: &'static str,
    /// Path of Exile 1's influences: none.
    #[serde(rename = "if")]
    influences: [u8; 0],
    /// The rolls of each of the base's implicits in the game's order of its stats; `null` shows
    /// the implicit's ranges.
    ip: Vec<Option<Vec<Number>>>,
    /// Corrupted `c`, mirrored `m`, sanctified `s`.
    f: Vec<&'static str>,
}

/// A mod of the export.
#[derive(Serialize)]
struct Mod {
    /// The mod's id; none for an unrevealed desecrated mod.
    k: Option<&'static str>,
    /// Its rolls in the game's order of its stats; none for an unrevealed desecrated mod.
    v: Option<Vec<Number>>,
    /// Fractured `f`, desecrated (revealed) `r`; an unrevealed desecrated mod `v` and its slot,
    /// `pt` or `st`.
    f: Vec<&'static str>,
}

fn export(item: &ParsedItem) -> Option<Export> {
    let rarity = match item.rarity? {
        ItemRarity::Normal => "normal",
        ItemRarity::Magic => "magic",
        ItemRarity::Rare => "rare",
        ItemRarity::Unique => return None,
    };
    if item.is_unidentified || !crafted(&item.category.as_ref()?.id) {
        return None;
    }
    let implicits: Vec<&ParsedModifier> = item
        .mods
        .iter()
        .filter(|modifier| modifier.info.modifier_type == ModifierType::Implicit)
        .collect();
    let base = base(item, &implicits)?;
    let flags = [
        (item.is_corrupted, "c"),
        (item.is_mirrored, "m"),
        (item.is_sanctified, "s"),
    ];
    Some(Export {
        i: base.id,
        m: item
            .mods
            .iter()
            .filter_map(|modifier| export_mod(item, modifier))
            .collect(),
        ns: item.sockets.map_or(0, |sockets| sockets.current),
        s: [],
        q: item.quality.unwrap_or(0),
        l: item.item_level?,
        r: rarity,
        influences: [],
        ip: base
            .implicits
            .iter()
            .enumerate()
            .map(|(index, implicit)| implicit_rolls(item, implicit, implicits.get(index).copied()?))
            .collect(),
        f: flags
            .into_iter()
            .filter_map(|(set, flag)| set.then_some(flag))
            .collect(),
    })
}

/// Whether the site crafts items of trade `category`, by its item groups: Body Armour, Boots,
/// Gloves, Helmet; Offhand (shields, bucklers, foci, quivers); Jewellery; One- and Two-Handed
/// Weapon; Flask; Charm; Jewel; Waystone. Its Relic and Tablet groups too, but no table here
/// names their mods, and a relic or a tablet is its mods.
fn crafted(category: &str) -> bool {
    matches!(
        category,
        "armour.chest"
            | "armour.boots"
            | "armour.gloves"
            | "armour.helmet"
            | "armour.shield"
            | "armour.buckler"
            | "armour.focus"
            | "armour.quiver"
            | "accessory.amulet"
            | "accessory.ring"
            | "accessory.belt"
            | "weapon.claw"
            | "weapon.dagger"
            | "weapon.wand"
            | "weapon.onesword"
            | "weapon.oneaxe"
            | "weapon.onemace"
            | "weapon.sceptre"
            | "weapon.spear"
            | "weapon.flail"
            | "weapon.bow"
            | "weapon.staff"
            | "weapon.twosword"
            | "weapon.twoaxe"
            | "weapon.twomace"
            | "weapon.warstaff"
            | "weapon.crossbow"
            | "weapon.talisman"
            | "flask.life"
            | "flask.mana"
            | "flask.charm"
            | "jewel"
            | "map.waystone"
    )
}

/// The item's base among those of its name: the only one, or the one whose implicits the item
/// has -- bases sharing a name differ by them (`Two-Stone Ring`, one per pair of resistances).
fn base(item: &ParsedItem, implicits: &[&ParsedModifier]) -> Option<Base> {
    let mut bases = item_refs::refs_for(item)?.bases();
    if bases.len() > 1 {
        bases.retain(|base| {
            base.implicits.len() == implicits.len()
                && base
                    .implicits
                    .iter()
                    .zip(implicits)
                    .all(|(implicit, modifier)| implicit.lines_in(modifier).is_some())
        });
    }
    <[Base; 1]>::try_from(bases).ok().map(|[base]| base)
}

/// `modifier` as the site takes it: an unrevealed desecrated mod by its slot, any other by its id
/// (`stat_filters::game_mod`) with its rolls; `None` for a mod whose id isn't known for sure,
/// whose rolls the item's text doesn't give -- a mod without its rolls breaks the site's item
/// (a waystone's, 2026-09-23) -- or that the site doesn't list.
fn export_mod(item: &ParsedItem, modifier: &ParsedModifier) -> Option<Mod> {
    if modifier.info.modifier_type == ModifierType::Veiled {
        let slot = match modifier.info.generation? {
            ModGeneration::Prefix => "pt",
            ModGeneration::Suffix => "st",
        };
        return Some(Mod {
            k: None,
            v: None,
            f: vec!["v", slot],
        });
    }
    let game_mod = stat_filters::game_mod(item, modifier)?;
    if UNLISTED_MODS
        .iter()
        .any(|prefix| game_mod.id.starts_with(prefix))
    {
        return None;
    }
    let flag = match modifier.info.modifier_type {
        ModifierType::Fractured => Some("f"),
        ModifierType::Desecrated => Some("r"),
        _ => None,
    };
    Some(Mod {
        k: Some(game_mod.id),
        v: Some(rolls_of(item, modifier, &game_mod.rolls?)?),
        f: flag.into_iter().collect(),
    })
}

/// The rolls of `implicit` the item prints in `modifier`, in the game's order of its stats;
/// `None` when `modifier` is another implicit, or its text doesn't give every roll.
fn implicit_rolls(
    item: &ParsedItem,
    implicit: &Implicit,
    modifier: &ParsedModifier,
) -> Option<Vec<Number>> {
    let lines = implicit.lines_in(modifier)?;
    let mut taken = vec![0; lines.len()];
    let rolls: Vec<Roll> = implicit
        .order
        .as_ref()?
        .iter()
        .map(|&line| {
            taken[line] += 1;
            Roll::Printed {
                line: lines[line],
                number: taken[line] - 1,
            }
        })
        .collect();
    rolls_of(item, modifier, &rolls)
}

/// `modifier`'s `rolls` (`stat_filters::GameMod::rolls`) as the item has them; `None` when a
/// line printing several numbers can't be read back (`printed_numbers`).
fn rolls_of(item: &ParsedItem, modifier: &ParsedModifier, rolls: &[Roll]) -> Option<Vec<Number>> {
    let mut counts = vec![0; modifier.stats.len()];
    for roll in rolls {
        if let Roll::Printed { line, .. } = *roll {
            counts[line] += 1;
        }
    }
    let numbers: Vec<Option<Vec<f64>>> = modifier
        .stats
        .iter()
        .zip(counts)
        .map(|(stat, count)| match count {
            0 => Some(Vec::new()),
            1 => Some(vec![stat_filters::printed(stat).0]),
            count => printed_numbers(item, stat, count),
        })
        .collect();
    rolls
        .iter()
        .map(|&roll| match roll {
            Roll::Printed { line, number } => json_number(*numbers[line].as_ref()?.get(number)?),
            Roll::Fixed(value) => json_number(value),
        })
        .collect()
}

/// The numbers of the line in the item's text that rolls to `stat`, a line printing `count` of
/// them: the parser keeps only their mean (`Adds 1(1-4) to 50(46-66) Lightning Damage` rolls
/// 25.5), so the line is found again by its numbers rolling to the stat's value and range, as
/// `item_parser::catalog_match` rolls them. `None` when no line does, or lines of other numbers
/// do too.
fn printed_numbers(item: &ParsedItem, stat: &ParsedStat, count: usize) -> Option<Vec<f64>> {
    let (value, min, max) = stat_filters::printed(stat);
    let mut found: Option<Vec<f64>> = None;
    for line in item.raw_text.lines() {
        let runs = find_numeric_runs(line);
        if runs.len() != count {
            continue;
        }
        let mean =
            |field: fn(&NumericRun) -> f64| runs.iter().map(field).sum::<f64>() / count as f64;
        let rolls_alike = |mean: f64, of: f64| (mean - of).abs() < 1e-9;
        if !(rolls_alike(mean(|run| run.value), value)
            && rolls_alike(mean(|run| run.min), min)
            && rolls_alike(mean(|run| run.max), max))
        {
            continue;
        }
        let numbers: Vec<f64> = runs.iter().map(|run| run.value).collect();
        if found.as_ref().is_some_and(|found| *found != numbers) {
            return None;
        }
        found = Some(numbers);
    }
    found
}

/// `value` as JSON writes a roll: whole numbers without a fraction.
fn json_number(value: f64) -> Option<Number> {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        Some(Number::from(value as i64))
    } else {
        Number::from_f64(value)
    }
}

#[cfg(test)]
mod tests {
    use item_parser::{ItemLanguage, parse_clipboard};
    use poe2_domain::{StatCatalog, TradeStat};
    use serde_json::{Value, json};

    use super::*;

    /// A slice of a live trade stat catalog, `id\ttype\ttext` per line, a text's line breaks
    /// written `\n`: the Russian one of `item-parser`'s live fixtures, and the English stats the
    /// English fixtures here resolve to (2026-09-23, with every entry sharing their texts).
    fn catalog(path: &str) -> StatCatalog {
        let rows = std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR")))
            .expect("the catalog slice");
        StatCatalog {
            stats: rows
                .lines()
                .map(|row| {
                    let mut fields = row.splitn(3, '\t');
                    let (id, mod_type, text) = (fields.next(), fields.next(), fields.next());
                    TradeStat {
                        id: id.expect("id").to_owned(),
                        mod_type: mod_type.expect("type").to_owned(),
                        text: text.expect("text").replace("\\n", "\n"),
                    }
                })
                .collect(),
        }
    }

    /// `item-parser`'s fixture `name`, parsed in its client's language -- which its name tells.
    fn parsed(name: &str) -> ParsedItem {
        let (language, slice) = if name.starts_with("ru_") {
            (
                ItemLanguage::Russian,
                "../item-parser/tests/fixtures/ru_live_stats.tsv",
            )
        } else {
            (
                ItemLanguage::English,
                "tests/fixtures/craft-link-stats-en.tsv",
            )
        };
        let text = std::fs::read_to_string(format!(
            "{}/../item-parser/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("the fixture");
        parse_clipboard(&text, language, &catalog(slice)).expect("the item parses")
    }

    /// The link of fixture `name` for a player whose interface speaks its client's language, and
    /// the export the link carries.
    fn link(name: &str) -> Option<(String, Value)> {
        let lang = if name.starts_with("ru_") {
            Lang::Russian
        } else {
            Lang::English
        };
        let url = url(&parsed(name), site_language(lang))?;
        let export = url
            .strip_prefix("https://beta.craftofexile.com/?game=poe2&eimport=")
            .and_then(|query| query.split('&').next())
            .expect("the site's importer");
        let export = percent_encoding::percent_decode_str(export)
            .decode_utf8()
            .expect("utf-8");
        let export = serde_json::from_str(&export).expect("an export");
        Some((url, export))
    }

    /// `export`'s mods as `(id, rolls)`.
    fn mods(export: &Value) -> Vec<(&str, Value)> {
        export["m"]
            .as_array()
            .expect("mods")
            .iter()
            .map(|entry| (entry["k"].as_str().expect("an id"), entry["v"].clone()))
            .collect()
    }

    #[test]
    fn craft_link_names_a_rare_ring_its_implicit_and_every_mod_by_the_games_ids() {
        let (url, export) = link("rare_with_implicit_en.txt").expect("a link");
        assert!(
            !url.contains("&language="),
            "the English interface keeps the site's default"
        );
        assert_eq!(export["i"], "Metadata/Items/Rings/FourRing9");
        assert_eq!((&export["l"], &export["r"]), (&json!(79), &json!("rare")));
        assert_eq!(export["ip"], json!([[8]]));
        // The Warmth suffix prints its mana regeneration first; the game lists light radius
        // first. The evasion prefix, T3, rolled 143: the current T3's 142-174 has it.
        assert_eq!(
            mods(&export),
            [
                ("IncreasedEvasionRating7", json!([143])),
                ("Strength2", json!([12])),
                ("LightRadiusAndManaRegeneration1", json!([5, 8])),
                ("ColdResist2", json!([15])),
            ]
        );
    }

    #[test]
    fn craft_link_takes_a_russian_item_by_the_same_ids_and_opens_the_russian_site() {
        let (url, export) = link("ru_live_krutyaschiy_obodok.txt").expect("a link");
        assert!(url.ends_with("&language=ru"));
        assert_eq!(export["i"], "Metadata/Items/Rings/FourRing6");
        assert_eq!((&export["l"], &export["r"]), (&json!(79), &json!("rare")));
        assert_eq!(export["ip"], json!([[8]]));
        assert_eq!(
            mods(&export),
            [
                ("LightningDamagePercent5", json!([26])),
                ("CastSpeedJewellery3", json!([17])),
                ("Strength1", json!([7])),
                ("ColdResist1", json!([6])),
            ]
        );
    }

    #[test]
    fn craft_link_opens_the_site_in_the_interface_language_whatever_the_items() {
        let russian_item = parsed("ru_live_krutyaschiy_obodok.txt");
        let english_item = parsed("rare_with_implicit_en.txt");
        let in_english = url(&russian_item, site_language(Lang::English)).expect("a link");
        assert!(!in_english.contains("&language="));
        let in_russian = url(&english_item, site_language(Lang::Russian)).expect("a link");
        assert!(in_russian.ends_with("&language=ru"));
    }

    #[test]
    fn craft_link_finds_a_magic_items_base_in_its_name_and_leaves_out_a_roll_its_tier_cant_have() {
        // Crackling, T7, rolled `Adds 1(1-4) to 50(46-66)`, an older game's T7: today's is
        // 1-4 to 53-76, so its id would be a guess.
        let (_, export) = link("magic_item_en.txt").expect("a link");
        assert_eq!(
            export["i"],
            "Metadata/Items/Weapons/TwoHandWeapons/TwoHandMaces/FourTwoHandMace6"
        );
        assert_eq!((&export["l"], &export["r"]), (&json!(32), &json!("magic")));
        assert_eq!(mods(&export), [("Strength1", json!([8]))]);

        let (_, jewel) = link("ru_live_kopenosnyy_izumrud_prigvozhdeniya.txt").expect("a link");
        assert_eq!(jewel["i"], "Metadata/Items/Jewels/JewelDex");
        assert_eq!((&jewel["l"], &jewel["r"]), (&json!(78), &json!("magic")));
        assert_eq!(
            mods(&jewel),
            [
                ("JewelSpearDamage", json!([10])),
                ("JewelPinBuildup", json!([16])),
            ]
        );
    }

    #[test]
    fn craft_link_gives_a_charm_its_quality_and_leaves_a_flag_implicits_roll_to_the_site() {
        let (_, export) = link("charm_quality_en.txt").expect("a link");
        assert_eq!(export["i"], "Metadata/Items/Flasks/FourCharm7");
        assert_eq!((&export["l"], &export["q"]), (&json!(80), &json!(14)));
        assert_eq!(export["ip"], json!([null]));
        assert_eq!(
            mods(&export),
            [
                ("CharmGainLifeOnUse4", json!([106])),
                ("FlaskExtraCharges4__", json!([51])),
            ]
        );
    }

    #[test]
    fn craft_link_takes_a_waystones_mods_by_its_tier_with_their_unprinted_shares() {
        // Tier 13: its own fire damage prefix, `MapMonsterDamageAsFire3` (15-19, with 15% more
        // waystones and 16% more monster effectiveness the item adds to its properties). Its
        // Evasive suffix prints no number the site could take, so it stays out.
        let (_, export) =
            link("ru_live_adskiy_putevoy_kamen_ur_13_ukloneniya.txt").expect("a link");
        assert_eq!(export["i"], "Metadata/Items/Maps/MapKeyTier13");
        assert_eq!((&export["l"], &export["r"]), (&json!(79), &json!("magic")));
        assert_eq!(
            mods(&export),
            [("MapMonsterDamageAsFire3", json!([17, 15, 16]))]
        );
    }

    #[test]
    fn craft_link_leaves_uniques_out() {
        assert_eq!(link("unique_item_en.txt"), None);
    }
}
