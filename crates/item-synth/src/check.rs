//! The round trip: a synthesized text parsed the way the app parses a copied item, held against
//! the listing it was made from. Everything the app reads from a copy -- the nameplate, the item
//! level, the states, quality, defences, requirements, sockets, a stack's size and every stat line
//! with its trade stat id and value -- must come back as the listing says.

use std::collections::BTreeMap;

use item_parser::{IndexedCatalog, ItemLanguage, parse_clipboard};
use poe2_domain::{ItemRarity, ParsedItem, StatCatalog};

use crate::listing::{Frame, Item, ModLine};
use crate::render::strip_markup;

/// What the app reads from `text` that the listing `item` contradicts, one sentence each; empty
/// when it reads the item back whole. `stats` is the catalog `catalog` was indexed from.
pub fn round_trip(
    item: &Item,
    text: &str,
    language: ItemLanguage,
    catalog: &IndexedCatalog,
    stats: &StatCatalog,
) -> Vec<String> {
    let parsed = match parse_clipboard(text, language, catalog) {
        Ok(parsed) => parsed,
        Err(err) => return vec![format!("the app can't parse it: {err}")],
    };
    let mut problems = Vec::new();
    let mut expect = |what: &str, wanted: String, got: String| {
        if wanted != got {
            problems.push(format!(
                "{what}: the listing says {wanted}, the app reads {got}"
            ));
        }
    };

    let frame = match item.frame() {
        Ok(frame) => frame,
        Err(err) => return vec![format!("{err:#}")],
    };
    let rarity = match frame {
        Frame::Normal => Some(ItemRarity::Normal),
        Frame::Magic => Some(ItemRarity::Magic),
        Frame::Rare => Some(ItemRarity::Rare),
        Frame::Unique => Some(ItemRarity::Unique),
        Frame::Gem | Frame::Currency => None,
    };
    expect(
        "rarity",
        format!("{rarity:?}"),
        format!("{:?}", parsed.rarity),
    );
    let (name, base) = match frame {
        Frame::Rare | Frame::Unique if item.identified => {
            (item.name.as_str(), Some(item.type_line.as_str()))
        }
        // The parser drops the quality wording (`Superior`, `Exceptional`) from a name that is the
        // item's base -- a normal item's, or an unidentified one's -- as the game's base name has none.
        Frame::Normal => (item.base_type.as_str(), None),
        Frame::Magic | Frame::Rare | Frame::Unique if !item.identified => {
            (item.base_type.as_str(), None)
        }
        _ => (item.type_line.as_str(), None),
    };
    expect("name", name.to_owned(), parsed.name.clone());
    expect(
        "base",
        format!("{base:?}"),
        format!("{:?}", parsed.base_type.as_deref()),
    );
    let item_level = (rarity.is_some() && item.ilvl > 0).then_some(item.ilvl);
    expect(
        "item level",
        format!("{item_level:?}"),
        format!("{:?}", parsed.item_level),
    );
    for (what, wanted, got) in [
        (
            "corrupted",
            item.corrupted || item.double_corrupted,
            parsed.is_corrupted,
        ),
        ("mirrored", item.duplicated, parsed.is_mirrored),
        ("sanctified", item.sanctified, parsed.is_sanctified),
        ("fractured", item.fractured, parsed.is_fractured),
        ("unidentified", !item.identified, parsed.is_unidentified),
    ] {
        expect(what, wanted.to_string(), got.to_string());
    }
    if !item.identified {
        expect(
            "unidentified tier",
            format!("{:?}", item.unidentified_tier),
            format!("{:?}", parsed.unidentified_tier),
        );
    }
    let mutated = item.explicit_mods.iter().any(
        |line| matches!(line, ModLine::Described(line) if line.flags.get("mutated") == Some(&true)),
    );
    if frame == Frame::Unique && item.identified {
        expect(
            "Vaal unique",
            format!("{:?}", Some(mutated)),
            format!("{:?}", parsed.mutated),
        );
    }

    properties(item, &parsed, &mut expect);
    stat_lines(item, &parsed, stats, &mut problems);
    problems
}

/// A property's first value as a number: `+20%` -> 20, `1.20` -> 1.2.
fn number(value: &str) -> Option<f64> {
    let digits: String = value
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse().ok()
}

fn properties(item: &Item, parsed: &ParsedItem, expect: &mut impl FnMut(&str, String, String)) {
    let value_of = |kind: u32| {
        item.properties
            .iter()
            .find(|property| property.kind == Some(kind))
            .and_then(|property| property.values.first())
            .map(|(value, _)| value.as_str())
    };
    let whole = |kind: u32| value_of(kind).and_then(number).map(|value| value as u32);
    expect(
        "quality",
        format!("{:?}", whole(6)),
        format!("{:?}", parsed.quality),
    );
    for (what, kind, got) in [
        ("armour", 16, parsed.armour),
        ("evasion", 17, parsed.evasion),
        ("energy shield", 18, parsed.energy_shield),
        ("runic ward", 54, parsed.runic_ward),
        ("spirit", 25, parsed.spirit),
    ] {
        expect(what, format!("{:?}", whole(kind)), format!("{got:?}"));
    }
    let damage = |value: &str| -> Option<(u32, u32)> {
        let (low, high) = value.split_once('-')?;
        Some((low.trim().parse().ok()?, high.trim().parse().ok()?))
    };
    expect(
        "physical damage",
        format!("{:?}", value_of(9).and_then(damage)),
        format!("{:?}", parsed.weapon_physical),
    );
    let elemental: Vec<(u32, u32)> = item
        .properties
        .iter()
        .filter(|property| matches!(property.kind, Some(10 | 11)))
        .flat_map(|property| &property.values)
        .filter_map(|(value, _)| damage(value))
        .collect();
    let read: Vec<(u32, u32)> = parsed
        .weapon_elemental
        .iter()
        .map(|(_, low, high)| (*low, *high))
        .collect();
    expect(
        "elemental damage",
        format!("{elemental:?}"),
        format!("{read:?}"),
    );
    let rate = |kind: u32| {
        value_of(kind)
            .and_then(number)
            .map(|value| (value * 100.0).round())
    };
    let hundredths = |value: Option<f64>| value.map(|value| (value * 100.0).round());
    expect(
        "critical hit chance",
        format!("{:?}", rate(12)),
        format!("{:?}", hundredths(parsed.weapon_crit)),
    );
    expect(
        "attacks per second",
        format!("{:?}", rate(13)),
        format!("{:?}", hundredths(parsed.weapon_aps)),
    );
    expect(
        "reload time",
        format!("{:?}", rate(97)),
        format!("{:?}", hundredths(parsed.weapon_reload_time)),
    );
    let stack = item.stack_size.zip(item.max_stack_size);
    expect(
        "stack size",
        format!("{stack:?}"),
        format!("{:?}", parsed.stack_size),
    );
    let mut wanted = poe2_domain::Requirements::default();
    for requirement in &item.requirements {
        let Some(value) = requirement
            .values
            .first()
            .and_then(|(value, _)| number(value))
        else {
            continue;
        };
        let value = value as u32;
        match requirement.kind {
            Some(62) => wanted.level = value,
            Some(63) => wanted.str = value,
            Some(64) => wanted.dex = value,
            Some(65) => wanted.int = value,
            _ => {}
        }
    }
    let wanted = (!item.requirements.is_empty()).then_some(wanted);
    expect(
        "requirements",
        format!("{wanted:?}"),
        format!("{:?}", parsed.requirements),
    );
    let sockets = parsed
        .sockets
        .map(|sockets| sockets.current)
        .or(parsed.gem_sockets.map(|sockets| sockets.number))
        .unwrap_or(0) as usize;
    expect(
        "sockets",
        item.sockets.len().to_string(),
        sockets.to_string(),
    );
}

/// The value a stat line rolls at, the way the parser and the trade site read it: the mean of two
/// or four numbers, else the first; unsigned, since a `reduced` line reads its number either way.
fn line_value(text: &str) -> f64 {
    let numbers: Vec<f64> = text
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter_map(|part| part.parse().ok())
        .collect();
    match numbers.len() {
        0 => 0.0,
        2 | 4 => numbers.iter().sum::<f64>() / numbers.len() as f64,
        _ => numbers[0],
    }
}

fn stat_lines(item: &Item, parsed: &ParsedItem, stats: &StatCatalog, problems: &mut Vec<String>) {
    let text_of: BTreeMap<&str, &str> = stats
        .stats
        .iter()
        .map(|stat| (stat.id.as_str(), stat.text.as_str()))
        .collect();
    // Two ids the catalog gives one text count as one: the app's search asks for either.
    let same = |a: &str, b: &str| {
        a == b
            || a.split('.').next() == b.split('.').next()
                && text_of
                    .get(a)
                    .is_some_and(|text| text_of.get(b) == Some(text))
    };

    let mut wanted: Vec<(String, f64, String)> = Vec::new();
    let mut want = |hash: &str, text: String| {
        let id = hash.strip_prefix("stat.").unwrap_or(hash);
        let id = id.split('|').next().unwrap_or(id).to_owned();
        let value = line_value(&text);
        match wanted.iter_mut().find(|(other, _, _)| *other == id) {
            Some(entry) => entry.1 += value,
            None => wanted.push((id, value, text)),
        }
    };
    let lists = [
        &item.enchant_mods,
        &item.rune_mods,
        &item.implicit_mods,
        &item.explicit_mods,
    ];
    for line in lists.into_iter().flatten() {
        if let ModLine::Described(line) = line
            && let Some(hash) = &line.hash
        {
            want(hash, strip_markup(&line.description));
        }
    }
    // A granted skill is a stat too (`skill.malice`), rolled at its level.
    for skill in &item.granted_skills {
        if let Some(hash) = &skill.hash {
            let level = skill
                .values
                .first()
                .map(|(value, _)| strip_markup(value))
                .unwrap_or_default();
            want(hash, level);
        }
    }

    let mut read: Vec<(String, f64, String)> = Vec::new();
    for modifier in &parsed.mods {
        for stat in &modifier.stats {
            let Some(id) = &stat.stat_id else {
                problems.push(format!("the app can't read {:?}", stat.text));
                continue;
            };
            match read.iter_mut().find(|(other, _, _)| other == id) {
                Some(entry) => entry.1 += stat.value,
                None => read.push((id.clone(), stat.value, stat.text.clone())),
            }
        }
    }
    for (text, kind) in &parsed.unknown_mods {
        problems.push(format!("the app can't read {kind:?} line {text:?}"));
    }

    for (id, value, text) in &wanted {
        match read.iter().position(|(other, _, _)| same(id, other)) {
            Some(at) => {
                let (_, got, _) = read.remove(at);
                if (got.abs() - value).abs() > 0.011 {
                    problems.push(format!(
                        "{text:?} ({id}): the listing says {value}, the app reads {}",
                        got.abs()
                    ));
                }
            }
            None => problems.push(format!("the app doesn't read {text:?} ({id})")),
        }
    }
    for (id, _, text) in read {
        problems.push(format!(
            "the app reads {text:?} ({id}), which the listing doesn't have"
        ));
    }
}
