//! Ordered, first-match-consumes section parsers for everything that isn't the nameplate or a
//! modifier block: property lines (Quality/Armour/weapon stats/Waystone stats), `Requires:`,
//! `Item Level:`, `Sockets:`, `Area Level:` with its trial lines, flags (Corrupted/Mirrored/...),
//! `Grants Skill:`, `Unidentified`, the price `Note:`. Ported from the shape of `Parser.ts`'s
//! per-purpose parser functions (`.tmp/research/ItemTextFormat.md`).
//!
//! Real fixtures show property-style lines physically bundled into ONE `--------`-delimited
//! section together (Quality/Armour/Energy Shield in one block; Quality/flask-charge lines in
//! one block for Charms) but flag lines (`Corrupted`, `Mirrored`, ...) each occupy their OWN
//! separate single-line section -- so `parse_properties_block` below is deliberately one wide
//! parser recognizing every numeric/property marker at once (folding in `stack_size`/
//! `flask_charges`/`gem_level`, which the plan's prose lists separately but real clipboard text
//! bundles with the rest), while each flag gets its own tiny dedicated parser so it can
//! independently claim its own section (a single combined "any flag" parser would only ever
//! claim the FIRST flag section it saw, since this module's orchestration removes a claimed
//! section and moves to the *next parser* rather than looping the same parser over every
//! remaining section).
//!
//! `SectionParser`'s signature includes `&IndexedCatalog` (the plan's own prose omits it) because
//! `parse_grants_skill` needs catalog access to resolve its skill-grant stat like any other mod
//! -- Rust's `fn` pointers stored in one array must share an identical signature, so every
//! parser takes it even though most ignore it.

use poe2_domain::{
    AugmentSockets, BlightedKind, ElementKind, GemSockets, ModifierInfo, ModifierType, ParsedItem,
    ParsedModifier, Requirements, TrialsInfo, UltimatumHint,
};

use crate::catalog_match::{self, IndexedCatalog};
use crate::client_strings::ClientStrings;
use crate::roll::find_numeric_runs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionResult {
    Parsed,
    Skipped,
}

pub type SectionParser =
    fn(&[String], &mut ParsedItem, &ClientStrings, &IndexedCatalog) -> SectionResult;

pub const PARSERS: &[SectionParser] = &[
    parse_properties_block,
    parse_requires,
    parse_item_level,
    parse_sockets,
    parse_waystone_block,
    parse_area_level,
    parse_grants_skill,
    parse_unidentified,
    parse_corrupted,
    parse_double_corrupted,
    parse_mirrored,
    parse_sanctified,
    parse_fractured_item,
    parse_unmodifiable,
    parse_price_note,
];

/// Thousands separators the client groups a property value's digits with: `,` in English
/// (`Physical Damage: 414-1,043`), a space or a no-break space in other languages -- EE2's
/// stack-size parser (`Parser.ts:parseStackSize`) drops "[localized separator]"s the same way.
const THOUSANDS_SEPARATORS: [char; 4] = [',', ' ', '\u{a0}', '\u{202f}'];

/// The number a property value starts with: an optional sign, digits (thousands separators
/// between groups of three skipped), an optional `.` fraction. Whatever follows is not part of
/// it -- a `%`, ` (augmented)`, ` (unmet)`, a max-level gem's ` (Max)` (EE2 `parseInt`s these
/// lines for the same reason).
fn leading_number(value: &str) -> Option<f64> {
    let chars: Vec<char> = value.trim_start().chars().collect();
    let mut number = String::new();
    let mut i = 0;
    if let Some(&sign @ ('+' | '-')) = chars.first() {
        if sign == '-' {
            number.push('-');
        }
        i = 1;
    }
    let is_group = |at: usize| {
        chars.len() >= at + 3
            && chars[at..at + 3].iter().all(char::is_ascii_digit)
            && chars.get(at + 3).is_none_or(|c| !c.is_ascii_digit())
    };
    while let Some(&c) = chars.get(i) {
        if c.is_ascii_digit() {
            number.push(c);
        } else if THOUSANDS_SEPARATORS.contains(&c) && !number.is_empty() && is_group(i + 1) {
            // A separator between digit groups, not the space ending the value.
        } else if c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
            number.push(c);
        } else {
            break;
        }
        i += 1;
    }
    number.parse().ok()
}

fn u32_after(line: &str, prefix: &str) -> Option<u32> {
    let value = leading_number(line.strip_prefix(prefix)?)?;
    (value >= 0.0 && value.fract() == 0.0).then_some(value as u32)
}

fn i32_after(line: &str, prefix: &str) -> Option<i32> {
    let value = leading_number(line.strip_prefix(prefix)?)?;
    (value.fract() == 0.0).then_some(value as i32)
}

fn f64_after(line: &str, prefix: &str) -> Option<f64> {
    leading_number(line.strip_prefix(prefix)?)
}

fn range_after(line: &str, prefix: &str) -> Option<(u32, u32)> {
    let (lo, hi) = line.strip_prefix(prefix)?.split_once('-')?;
    Some((u32_after(lo, "")?, u32_after(hi, "")?))
}

fn element_of(tag: &str) -> Option<ElementKind> {
    match tag {
        "fire" => Some(ElementKind::Fire),
        "cold" => Some(ElementKind::Cold),
        "lightning" => Some(ElementKind::Lightning),
        "chaos" => Some(ElementKind::Chaos),
        _ => None,
    }
}

/// One `lo-hi (tag)` segment of a damage line: `element` where the line's own label names it
/// (`Fire Damage: 47-92 (fire)`, or `Cold Damage: 39-75 (augmented)` as older clients print
/// it), else the element tag among the segment's parenthesised words (`Elemental Damage: 27-36
/// (fire), 9-13 (cold)`).
fn parse_damage_segment(
    seg: &str,
    element: Option<ElementKind>,
) -> Option<(ElementKind, u32, u32)> {
    let tagged = || {
        seg.split('(')
            .skip(1)
            .filter_map(|part| part.split_once(')'))
            .find_map(|(tag, _)| element_of(tag.trim()))
    };
    let kind = element.or_else(tagged)?;
    let (lo, hi) = range_after(seg.trim(), "")?;
    Some((kind, lo, hi))
}

/// Every segment of a damage line's `remainder`. Segments are comma-space-delimited (`"27-36
/// (fire), 9-13 (cold)"`): a thousands separator inside one never sits before a space, so the
/// `", "` delimiters survive.
fn push_damage(item: &mut ParsedItem, remainder: &str, element: Option<ElementKind>) {
    for seg in remainder.split(", ") {
        if let Some(entry) = parse_damage_segment(seg, element) {
            item.weapon_elemental.push(entry);
        }
    }
}

/// `Stack Size: 2,448/40`: every character but digits and the `/` dropped first, as EE2's
/// `parseStackSize` does for the localized thousands separator.
fn stack_size_after(line: &str, prefix: &str) -> Option<(u32, u32)> {
    let digits: String = line
        .strip_prefix(prefix)?
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '/')
        .collect();
    let (value, max) = digits.split_once('/')?;
    Some((value.parse().ok()?, max.parse().ok()?))
}

fn parse_properties_block(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    let recognized = section.iter().any(|line| {
        line.starts_with(cs.quality)
            || line.starts_with(cs.armour)
            || line.starts_with(cs.evasion)
            || line.starts_with(cs.energy_shield)
            || line.starts_with(cs.runic_ward)
            || line.starts_with(cs.block_chance)
            || line.starts_with(cs.physical_damage)
            || line.starts_with(cs.elemental_damage)
            || line.starts_with(cs.fire_damage)
            || line.starts_with(cs.cold_damage)
            || line.starts_with(cs.lightning_damage)
            || line.starts_with(cs.crit_chance)
            || line.starts_with(cs.attack_speed)
            || line.starts_with(cs.reload_speed)
            || line.starts_with(cs.base_spirit)
            || (line.starts_with(cs.gem_level) && is_gem_family(item))
            || line.starts_with(cs.stack_size)
            || cs.flask_charges.is_match(line)
    });
    if !recognized {
        return SectionResult::Skipped;
    }

    for line in section {
        if let Some(v) = u32_after(line, cs.quality) {
            item.quality = Some(v);
        } else if let Some(v) = u32_after(line, cs.armour) {
            item.armour = Some(v);
        } else if let Some(v) = u32_after(line, cs.evasion) {
            item.evasion = Some(v);
        } else if let Some(v) = u32_after(line, cs.energy_shield) {
            item.energy_shield = Some(v);
        } else if let Some(v) = u32_after(line, cs.runic_ward) {
            item.runic_ward = Some(v);
        } else if let Some(v) = u32_after(line, cs.block_chance) {
            item.block_chance = Some(v);
        } else if let Some(v) = range_after(line, cs.physical_damage) {
            item.weapon_physical = Some(v);
        } else if let Some(rest) = line.strip_prefix(cs.elemental_damage) {
            push_damage(item, rest, None);
        } else if let Some(rest) = line.strip_prefix(cs.fire_damage) {
            push_damage(item, rest, Some(ElementKind::Fire));
        } else if let Some(rest) = line.strip_prefix(cs.cold_damage) {
            push_damage(item, rest, Some(ElementKind::Cold));
        } else if let Some(rest) = line.strip_prefix(cs.lightning_damage) {
            push_damage(item, rest, Some(ElementKind::Lightning));
        } else if let Some(v) = f64_after(line, cs.crit_chance) {
            item.weapon_crit = Some(v);
        } else if let Some(v) = f64_after(line, cs.attack_speed) {
            item.weapon_aps = Some(v);
        } else if let Some(v) = f64_after(line, cs.reload_speed) {
            item.weapon_reload_time = Some(v);
        } else if let Some(v) = u32_after(line, cs.base_spirit) {
            item.spirit = Some(v);
        } else if let Some(v) = u32_after(line, cs.gem_level).filter(|_| is_gem_family(item)) {
            // Gem-family only, as EE2's `parseGem`: the older clients' `Requirements:` block
            // prints the item's required level as `Level: 58` too.
            item.gem_level = Some(v);
        } else if let Some(v) = stack_size_after(line, cs.stack_size) {
            item.stack_size = Some(v);
        }
        // `flask_charges` lines carry no field of their own on `ParsedItem` beyond having been
        // recognized (matches the reference's own treatment -- they exist to keep this line from
        // being mistaken for an unrecognized/unknown line, nothing more; the charge *count* they
        // show is UI chrome, not a search-relevant fact).
    }
    SectionResult::Parsed
}

fn is_gem_family(item: &ParsedItem) -> bool {
    item.category
        .as_ref()
        .is_some_and(|c| c.id.starts_with("gem"))
}

fn parse_requires(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    // Gems can carry a SECOND, differently-shaped `Requires:` line (allowed weapon types, e.g.
    // `Requires: Spear, Bow, Crossbow`) that would otherwise silently overwrite good level/attr
    // data with all-zeros if matched against `requires_line` too -- short-circuit entirely for
    // gem-family items, matching the reference's own `GEM.has(item.category)` guard.
    if is_gem_family(item) {
        return SectionResult::Skipped;
    }
    let Some(line) = section.iter().find(|l| cs.requires_line.is_match(l)) else {
        return SectionResult::Skipped;
    };
    let caps = cs.requires_line.captures(line).expect("just matched");
    let get = |name: &str| {
        caps.name(name)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0)
    };
    item.requirements = Some(Requirements {
        level: get("level"),
        str: get("str"),
        dex: get("dex"),
        int: get("int"),
    });
    SectionResult::Parsed
}

fn parse_item_level(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    let Some(v) = section.iter().find_map(|l| u32_after(l, cs.item_level)) else {
        return SectionResult::Skipped;
    };
    item.item_level = Some(v);
    SectionResult::Parsed
}

/// The rune sockets a base of this trade category has when it drops -- EE2's `getMaxSockets`
/// (`Parser.ts:2014-2074`) by category. EE2 also overrides it for eleven uniques by English
/// name; a Russian client names them differently, so no override applies here.
fn base_rune_sockets(category: &str) -> u32 {
    match category {
        "armour.chest" | "weapon.twoaxe" | "weapon.twomace" | "weapon.twosword"
        | "weapon.crossbow" | "weapon.bow" | "weapon.warstaff" | "weapon.staff"
        | "weapon.talisman" => 2,
        "armour.helmet" | "armour.shield" | "armour.gloves" | "armour.boots" | "weapon.oneaxe"
        | "weapon.onemace" | "weapon.onesword" | "weapon.claw" | "weapon.dagger"
        | "armour.focus" | "weapon.spear" | "weapon.flail" | "weapon.wand" | "armour.buckler"
        | "weapon.sceptre" => 1,
        _ => 0,
    }
}

/// `Sockets:` -- a gem's skill-gem sockets (`G`), anything else's rune sockets (`S`), told apart
/// by the item's category like EE2's `parseSockets`/`parseAugmentSockets`. A rune-socket count
/// against the base's own (`normal`) is what shows added sockets; which sockets are filled needs
/// the item's rune mods, so `count_empty_rune_sockets` settles `empty` once they are parsed.
fn parse_sockets(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    let Some(line) = section.iter().find(|l| l.starts_with(cs.sockets)) else {
        return SectionResult::Skipped;
    };
    let sockets = line[cs.sockets.len()..].trim_end();
    if is_gem_family(item) {
        item.gem_sockets = Some(GemSockets {
            number: sockets.chars().filter(|c| !matches!(c, ' ' | '-')).count() as u32,
            linked: None,
            white: sockets.chars().filter(|&c| c == 'W').count() as u32,
        });
    } else {
        let current = sockets.chars().filter(|&c| c == 'S').count() as u32;
        let normal = item
            .category
            .as_ref()
            .map_or(0, |category| base_rune_sockets(&category.id));
        item.sockets = Some(AugmentSockets {
            empty: current,
            current,
            normal,
        });
    }
    SectionResult::Parsed
}

/// How many of the item's rune sockets hold nothing. Whether a socket is filled is not in the
/// `Sockets:` line -- it prints `S` either way (`SpectreIncSpirit`'s single `S` is known empty,
/// `.tmp/research/ItemTextFormat.md`) -- and the reference tells runes apart by summing its local
/// rune value table (`determineAugments`), which this architecture does not ship. Best available
/// signal without it: every socket counts as filled once the item prints any rune-granted
/// (`Augment`/`AddedAugment`) modifier, else every one as empty. Called from `lib.rs` after the
/// modifier pass, which is where rune mods come from.
pub fn count_empty_rune_sockets(item: &mut ParsedItem) {
    let has_rune_mod = item.mods.iter().any(|m| {
        matches!(
            m.info.modifier_type,
            ModifierType::Augment | ModifierType::AddedAugment
        )
    });
    if let Some(sockets) = &mut item.sockets {
        sockets.empty = if has_rune_mod { 0 } else { sockets.current };
    }
}

fn parse_waystone_block(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    let pack_size = |line: &str| {
        cs.pack_size
            .iter()
            .find_map(|prefix| i32_after(line, prefix))
    };
    let recognized = section.iter().any(|line| {
        line.starts_with(cs.revives)
            || pack_size(line).is_some()
            || line.starts_with(cs.magic_monsters)
            || line.starts_with(cs.rare_monsters)
            || line.starts_with(cs.waystone_drop_chance)
            || line.starts_with(cs.item_rarity)
            || line.starts_with(cs.monster_rarity)
            || line.starts_with(cs.monster_effectiveness)
            || line.starts_with(cs.waystone_tier)
    });
    if !recognized {
        return SectionResult::Skipped;
    }
    let mut props = item.waystone.take().unwrap_or_default();
    for line in section {
        if let Some(v) = i32_after(line, cs.revives) {
            props.revives = Some(v);
        } else if let Some(v) = pack_size(line) {
            props.pack_size = Some(v);
        } else if let Some(v) = i32_after(line, cs.magic_monsters) {
            props.magic_monsters = Some(v);
        } else if let Some(v) = i32_after(line, cs.rare_monsters) {
            props.rare_monsters = Some(v);
        } else if let Some(v) = i32_after(line, cs.waystone_drop_chance) {
            props.drop_chance = Some(v);
        } else if let Some(v) = i32_after(line, cs.item_rarity) {
            props.item_rarity = Some(v);
        } else if let Some(v) = i32_after(line, cs.monster_rarity) {
            props.monster_rarity = Some(v);
        } else if let Some(v) = i32_after(line, cs.monster_effectiveness) {
            props.effectiveness = Some(v);
        } else if let Some(v) = u32_after(line, cs.waystone_tier) {
            props.tier = Some(v);
        }
    }
    item.waystone = Some(props);
    SectionResult::Parsed
}

/// Derives `waystone.tier`/`waystone.blighted` from the base-type NAME line, unconditionally --
/// decoupled from `parse_waystone_block` above (a section-scoped parser) because a Blighted
/// Waystone with no other rolled property at all would otherwise never get a `waystone` value:
/// this runs once per item regardless of which (if any) section carried recognized property
/// lines. A magic waystone has no base-type line; its name carries the base between its affixes.
/// Called from `lib.rs` after every section parser has run. No-op for anything that isn't
/// `map.waystone`-categorized.
pub fn derive_waystone_name_fields(item: &mut ParsedItem, cs: &ClientStrings) {
    if !item
        .category
        .as_ref()
        .is_some_and(|c| c.id == "map.waystone")
    {
        return;
    }
    let mut props = item.waystone.take().unwrap_or_default();
    if props.tier.is_none()
        && let Some(caps) = cs
            .waystone_tier_name
            .captures(item.base_type.as_deref().unwrap_or(&item.name))
    {
        props.tier = caps["tier"].parse().ok();
    }
    if props.blighted.is_none()
        && let Some(base_type) = &item.base_type
    {
        if cs.map_blighted.is_match(base_type) {
            props.blighted = Some(BlightedKind::Blighted);
        } else if cs.map_blight_ravaged.is_match(base_type) {
            props.blighted = Some(BlightedKind::BlightRavaged);
        }
    }
    item.waystone = Some(props);
}

/// `Area Level: N` -- an Expedition Logbook's, a Barya's or an Inscribed Ultimatum's -- with the
/// trial lines the latter two print beside it (EE2's `parseAreaLevel`/`parseTrials`, which pick
/// those items out by their English name; the markers themselves are unique to them).
fn parse_area_level(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    let Some(area_level) = section.iter().find_map(|l| u32_after(l, cs.area_level)) else {
        return SectionResult::Skipped;
    };
    item.area_level = Some(area_level);
    let number_of_trials = section.iter().find_map(|l| u32_after(l, cs.trial_count));
    let ultimatum_hint = section.iter().find_map(|line| {
        [
            (cs.ultimatum_victorious, UltimatumHint::Victorious),
            (cs.ultimatum_cowardly, UltimatumHint::Cowardly),
            (cs.ultimatum_deadly, UltimatumHint::Deadly),
        ]
        .into_iter()
        .find(|(marker, _)| line.starts_with(marker))
        .map(|(_, hint)| hint)
    });
    if number_of_trials.is_some() || ultimatum_hint.is_some() {
        item.trials = Some(TrialsInfo {
            number_of_trials,
            ultimatum_hint,
        });
    }
    SectionResult::Parsed
}

fn parse_grants_skill(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    index: &IndexedCatalog,
) -> SectionResult {
    if !section.iter().any(|l| l.starts_with(cs.grants_skill)) {
        return SectionResult::Skipped;
    }
    for line in section {
        if !line.starts_with(cs.grants_skill) {
            continue;
        }
        // A skill the base itself grants (`Grants Skill: Raise Shield` on every shield) prints no
        // level and says nothing about this item; EE2 skips its stat outright
        // (`parseStatsFromMod`'s built-in skill list). The mod stays, without a stat.
        let stats = if find_numeric_runs(line).is_empty() {
            Vec::new()
        } else {
            let category = item.category.as_ref().map(|c| c.id.as_str());
            let stats = catalog_match::resolve_stat_lines(
                &[line.as_str()],
                ModifierType::Skill,
                category,
                cs,
                index,
            );
            if stats.iter().all(|s| s.stat_id.is_none()) {
                item.unknown_mods.push((line.clone(), ModifierType::Skill));
            }
            stats
        };
        item.mods.push(ParsedModifier {
            info: ModifierInfo {
                modifier_type: ModifierType::Skill,
                generation: None,
                name: None,
                tier: None,
                rank: None,
                tags: Vec::new(),
            },
            stats,
        });
    }
    SectionResult::Parsed
}

fn parse_unidentified(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    let Some(caps) = section.iter().find_map(|l| cs.unidentified.captures(l)) else {
        return SectionResult::Skipped;
    };
    item.is_unidentified = true;
    item.unidentified_tier = caps.name("tier").and_then(|m| m.as_str().parse().ok());
    SectionResult::Parsed
}

/// Whether `section` carries the flag line `marker`, surrounding whitespace aside: EE2's Russian
/// `CORRUPTED` is `"Осквернено "`, trailing space included.
fn single_line_flag(section: &[String], marker: &str) -> bool {
    section.iter().any(|l| l.trim() == marker.trim())
}

fn parse_corrupted(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    if !single_line_flag(section, cs.corrupted) {
        return SectionResult::Skipped;
    }
    item.is_corrupted = true;
    SectionResult::Parsed
}

fn parse_double_corrupted(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    if !single_line_flag(section, cs.double_corrupted) {
        return SectionResult::Skipped;
    }
    item.is_corrupted = true;
    SectionResult::Parsed
}

fn parse_mirrored(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    if !single_line_flag(section, cs.mirrored) {
        return SectionResult::Skipped;
    }
    item.is_mirrored = true;
    SectionResult::Parsed
}

fn parse_sanctified(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    if !single_line_flag(section, cs.sanctified) {
        return SectionResult::Skipped;
    }
    item.is_sanctified = true;
    SectionResult::Parsed
}

fn parse_fractured_item(
    section: &[String],
    _item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    if !single_line_flag(section, cs.fractured_item) {
        return SectionResult::Skipped;
    }
    // The item-level "Fractured Item" footer alone does NOT imply `is_fractured` -- only a mod
    // whose own type resolved to `Fractured` does (see `modifiers.rs`; the reference pins this
    // exact distinction with `FracturedItem`/`FracturedItemNoModMarked`). This parser only marks
    // the section consumed; `lib.rs` sets `is_fractured` from `item.mods` after every section has
    // been parsed.
    SectionResult::Parsed
}

fn parse_unmodifiable(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    if !single_line_flag(section, cs.unmodifiable) {
        return SectionResult::Skipped;
    }
    item.is_unmodifiable = true;
    SectionResult::Parsed
}

fn parse_price_note(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    _index: &IndexedCatalog,
) -> SectionResult {
    let Some(line) = section.iter().find(|l| l.starts_with(cs.price_note)) else {
        return SectionResult::Skipped;
    };
    item.note = Some(line[cs.price_note.len()..].to_string());
    SectionResult::Parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_values_read_past_their_annotations_and_thousands_separators() {
        // `(Max)` follows a max-level gem's level (EE2's `Parser.ts` comment "Level: 20 (Max)").
        assert_eq!(u32_after("Level: 20 (Max)", "Level: "), Some(20));
        assert_eq!(
            u32_after("Quality: +20% (augmented)", "Quality: "),
            Some(20)
        );
        assert_eq!(u32_after("Броня: 3 075", "Броня: "), Some(3075));
        assert_eq!(
            range_after(
                "Physical Damage: 414-1,043 (augmented)",
                "Physical Damage: "
            ),
            Some((414, 1043))
        );
        assert_eq!(
            i32_after("Revives Available: 0 (augmented)", "Revives Available: "),
            Some(0)
        );
        assert_eq!(
            f64_after("Reload Time: 0.60 (augmented)", "Reload Time: "),
            Some(0.6)
        );
        assert_eq!(
            stack_size_after("Stack Size: 2,448/40", "Stack Size: "),
            Some((2448, 40))
        );
    }
}
