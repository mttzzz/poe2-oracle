//! Base-item property rows: the item filters EE2 builds from the item itself
//! (`create-item-filters.ts`: item level, rune sockets, quality) and the defence and damage
//! properties it derives in `pseudo/item-property.ts` (`armourProps`/`weaponProps`). Each row is
//! a `FilterTag::Property` whose single trade id is `<group>.<key>`, the trade query filter it
//! searches (`query.filters.<group>.filters.<key>`, where `trade-client` sends it). Every key used
//! here is one `GET /api/trade2/data/filters` lists (verified live 2026-09-22 on www and ru).
//!
//! A property's mods are matched by stat hash, the trade id without its `explicit.`/`rune.`/...
//! prefix, so every mod type counts, as EE2's by-ref matching does. The hashes are the ids EE2's
//! `stats.ndjson` gives each stat ref it names in `calc-q20.ts` -- a local and a global id for
//! several of them, since the clipboard text can't tell the two apart -- each checked against
//! the live `/api/trade2/data/stats` (2026-09-22).
//!
//! The defence and damage rows are what PoE Overlay II merges an item's local mods into: each
//! takes the best score of the mods scaling it (its property pseudo totals and
//! `syncPropertyRanking`, `9505.bundle.js` ~18100 and ~25299), and a search profile picks it by
//! that score. The other rows keep EE2's own checkbox in every profile.

use poe2_domain::{ElementKind, ItemRarity, ParsedItem};

use crate::{FilterTag, RollBound, SearchFilter, SearchFilterRoll};

/// Whether EE2 prices `item` with its exact preset -- by base type, item level enabled, no
/// defence or damage rows -- rather than its default "pseudo" preset (`create-presets.ts:45-72`):
/// unidentified and Normal items, and every non-unique life/mana flask, relic and tablet. EE2's
/// list also names tinctures, memory lines, invitations, heist items, sentinels and wombgifts
/// (none has a PoE2 trade category, verified live 2026-09-22) and bases missing from its item
/// database, which holds no non-unique base without a `craftable` record.
pub fn uses_exact_preset(item: &ParsedItem) -> bool {
    item.is_unidentified
        || item.rarity == Some(ItemRarity::Normal)
        || (item.rarity != Some(ItemRarity::Unique)
            && matches!(
                category_id(item),
                "flask.life" | "flask.mana" | "sanctum.relic" | "map.tablet"
            ))
}

/// Every property row for `item`, in EE2's order: its item filters in `FiltersBlock.vue`'s order,
/// then -- in the default preset only, which alone calls `filterItemProp` -- the armour, weapon or
/// waystone rows that head the stat list. A gem gets EE2's gem filters alone
/// (`createGemFilters`, `create-item-filters.ts:489-566`).
pub(crate) fn property_filters(item: &ParsedItem) -> Vec<SearchFilter> {
    let exact = uses_exact_preset(item);
    let category = category_id(item);
    let mut rows = Vec::new();
    if category.starts_with("gem") {
        gem_rows(item, &mut rows);
        return rows;
    }
    rows.extend(item_level_row(item, category, exact));
    rows.extend(rune_sockets_row(item));
    rows.extend(quality_row(item, category));
    rows.extend(area_level_row(item, category));
    if category == "map.waystone" {
        waystone_rows(item, exact, &mut rows);
    }
    if !exact {
        if is_armour(category) {
            armour_rows(item, &mut rows);
        }
        if is_weapon(category) {
            weapon_rows(item, &mut rows);
        }
    }
    rows
}

/// Whether a property row is one of the defence and damage rows a search profile picks by
/// score (`merged_mods`), rather than one keeping EE2's own checkbox.
pub(crate) fn scored(trade_id: &str) -> bool {
    trade_id.starts_with("equipment_filters.") && trade_id != "equipment_filters.rune_sockets"
}

/// The mods of `item` a property row counts, whose best score it takes (PoE Overlay II's
/// property pseudo totals, ~18100): the local ones scaling it, which fold into it
/// (`folded_into_properties`). Total DPS and reload time have none; block takes the local block
/// chance mods EE2 folds into it, which PoE Overlay II leaves listed as mods of their own.
pub(crate) fn merged_mods(item: &ParsedItem, trade_id: &str) -> Vec<usize> {
    let scaling: &[&[&str]] = match trade_id {
        "equipment_filters.ar" => &[ARMOUR.flat, ARMOUR.incr],
        "equipment_filters.ev" => &[EVASION.flat, EVASION.incr],
        "equipment_filters.es" => &[ENERGY_SHIELD.flat, ENERGY_SHIELD.incr],
        "equipment_filters.ward" => &[RUNIC_WARD.flat, RUNIC_WARD.incr],
        "equipment_filters.block" => &[BLOCK.incr],
        "equipment_filters.pdps" => &[PHYSICAL_DAMAGE.flat, PHYSICAL_DAMAGE.incr],
        "equipment_filters.edps" => &[ELEMENTAL_DAMAGE.flat],
        "equipment_filters.aps" => &[ATTACK_SPEED.incr],
        // Its `local_critical_strike_chance` alone: the flat local roll.
        "equipment_filters.crit" => &[CRIT_CHANCE.flat],
        "equipment_filters.spirit" => &[SPIRIT.incr],
        _ => return Vec::new(),
    };
    let merged = |stat_id: &str| {
        folded_into_properties(item, stat_id)
            && scaling
                .iter()
                .any(|hashes| hashes.contains(&stat_hash(stat_id)))
    };
    item.mods
        .iter()
        .enumerate()
        .filter(|(_, modifier)| {
            modifier
                .stats
                .iter()
                .any(|stat| stat.stat_id.as_deref().is_some_and(merged))
        })
        .map(|(index, _)| index)
        .collect()
}

/// EE2's `createGemFilters` for a gem: its socket count (enabled from 3), quality (from 16%) and
/// level (from 19), each a minimum.
fn gem_rows(item: &ParsedItem, rows: &mut Vec<SearchFilter>) {
    if let Some(sockets) = item.gem_sockets {
        rows.push(property_row(
            "Gem Sockets: #",
            "misc_filters.gem_sockets",
            f64::from(sockets.number),
            false,
            sockets.number >= 3,
            RollBound::AtLeast,
        ));
    }
    if let Some(quality) = present(item.quality) {
        rows.push(property_row(
            "Quality: #%",
            "type_filters.quality",
            quality,
            false,
            quality >= 16.0,
            RollBound::AtLeast,
        ));
    }
    if let Some(level) = item.gem_level {
        rows.push(property_row(
            "Gem Level: #",
            "misc_filters.gem_level",
            f64::from(level),
            false,
            level >= 19,
            RollBound::AtLeast,
        ));
    }
}

/// `create-item-filters.ts:162-172`: an Expedition Logbook's area level, a searched minimum.
fn area_level_row(item: &ParsedItem, category: &str) -> Option<SearchFilter> {
    let area_level = item.area_level.filter(|_| category == "map.logbook")?;
    Some(property_row(
        "Area Level: #",
        "misc_filters.area_level",
        f64::from(area_level),
        false,
        true,
        RollBound::AtLeast,
    ))
}

/// A waystone's tier, searched exactly (`create-item-filters.ts:158-161` sets both bounds), and --
/// in the default preset -- EE2's `mapProps` (`item-property.ts:449-633`): every other waystone
/// property as a disabled minimum, the revives one hidden. EE2 files Monster Rarity under the
/// trade site's rare-monsters filter and Monster Effectiveness under its magic-monsters one, which
/// the live `/data/filters` labels exactly so (2026-09-22).
fn waystone_rows(item: &ParsedItem, exact: bool, rows: &mut Vec<SearchFilter>) {
    let Some(waystone) = item.waystone else {
        return;
    };
    if let Some(tier) = waystone.tier {
        rows.push(property_row(
            "Waystone Tier: #",
            "map_filters.map_tier",
            f64::from(tier),
            false,
            true,
            RollBound::Exactly,
        ));
    }
    if exact {
        return;
    }
    let properties = [
        (
            waystone.revives,
            "Revives Available: #",
            "map_filters.map_revives",
        ),
        (
            waystone.pack_size,
            "Monster Pack Size: #%",
            "map_filters.map_packsize",
        ),
        (
            waystone.magic_monsters,
            "Magic Monsters: #%",
            "map_filters.map_magic_monsters",
        ),
        (
            waystone.rare_monsters,
            "Rare Monsters: #%",
            "map_filters.map_rare_monsters",
        ),
        (
            waystone.drop_chance,
            "Waystone Drop Chance: #%",
            "map_filters.map_bonus",
        ),
        (
            waystone.item_rarity,
            "Item Rarity: #%",
            "map_filters.map_iir",
        ),
        (waystone.gold, "Gold Found: #%", "map_filters.map_gold"),
        (
            waystone.monster_rarity,
            "Monster Rarity: #%",
            "map_filters.map_rare_monsters",
        ),
        (
            waystone.effectiveness,
            "Monster Effectiveness: #%",
            "map_filters.map_magic_monsters",
        ),
    ];
    for (value, label, trade_id) in properties {
        let Some(value) = value.filter(|&value| value != 0) else {
            continue;
        };
        let mut row = property_row(
            label,
            trade_id,
            f64::from(value),
            false,
            false,
            RollBound::AtLeast,
        );
        row.hidden = trade_id == "map_filters.map_revives";
        rows.push(row);
    }
}

/// Whether the stat with trade id `stat_id` is folded into one of `item`'s property rows, so the
/// mod and pseudo rows must leave it out -- EE2's `removeUsedStats` (`item-property.ts:165-173`,
/// `438-446`), which drops every stat of `ARMOUR_STATS`/`WEAPON_STATS` once an armour piece or
/// weapon has any property row, before its pseudo and mod rows are built. Left in, a local
/// "#% increased Armour" would constrain the search a second time, and under the global id at
/// that: `item-parser` resolves the clipboard line to it because the local stat's catalog text
/// ends in " (Local)".
pub(crate) fn folded_into_properties(item: &ParsedItem, stat_id: &str) -> bool {
    if uses_exact_preset(item) {
        return false;
    }
    let hash = stat_hash(stat_id);
    let listed = |stats: &[&[&str]]| stats.iter().any(|hashes| hashes.contains(&hash));
    let category = category_id(item);
    (is_armour(category) && has_armour_properties(item) && listed(&ARMOUR_STATS))
        || (is_weapon(category) && has_weapon_properties(item) && listed(&WEAPON_STATS))
}

/// The stats scaling one property: flat additions and percentage increases, by stat hash (EE2's
/// `{ flat, incr }` ref pairs, `calc-q20.ts`).
struct Scaling {
    flat: &'static [&'static str],
    incr: &'static [&'static str],
}

/// "#% increased Armour and Energy Shield"
const INC_AR_ES: &str = "stat_3321629045";
/// "#% increased Armour and Evasion"
const INC_AR_EV: &str = "stat_2451402625";
/// "#% increased Evasion and Energy Shield"
const INC_EV_ES: &str = "stat_1999113824";
/// "#% increased Armour, Evasion and Energy Shield"
const INC_AR_EV_ES: &str = "stat_3523867985";

const ARMOUR: Scaling = Scaling {
    // "# to Armour", local and global.
    flat: &["stat_3484657501", "stat_809229260"],
    // "#% increased Armour", local and global.
    incr: &[
        "stat_1062208444",
        "stat_2866361420",
        INC_AR_ES,
        INC_AR_EV,
        INC_AR_EV_ES,
    ],
};
const EVASION: Scaling = Scaling {
    // "# to Evasion Rating", local and global.
    flat: &["stat_53045048", "stat_2144192055"],
    // "#% increased Evasion Rating", local and global.
    incr: &[
        "stat_124859000",
        "stat_2106365538",
        INC_AR_EV,
        INC_EV_ES,
        INC_AR_EV_ES,
    ],
};
const ENERGY_SHIELD: Scaling = Scaling {
    // "# to maximum Energy Shield", local and global.
    flat: &["stat_4052037485", "stat_3489782002"],
    // "#% increased Energy Shield".
    incr: &["stat_4015621042", INC_AR_ES, INC_EV_ES, INC_AR_EV_ES],
};
const RUNIC_WARD: Scaling = Scaling {
    // "# to maximum Runic Ward".
    flat: &["stat_3336230913", "stat_774059442"],
    // "#% increased Runic Ward".
    incr: &["stat_830161081"],
};
const BLOCK: Scaling = Scaling {
    flat: &[],
    // "#% increased Block chance", local and global.
    incr: &["stat_2481353198", "stat_4147897060"],
};
const PHYSICAL_DAMAGE: Scaling = Scaling {
    // "Adds # to # Physical Damage".
    flat: &["stat_1940865751"],
    // "#% increased Physical Damage".
    incr: &["stat_1509134228"],
};
const ELEMENTAL_DAMAGE: Scaling = Scaling {
    // "Adds # to # Lightning Damage", "... Cold Damage", "... Fire Damage".
    flat: &["stat_3336890334", "stat_1037193709", "stat_709508406"],
    incr: &[],
};
const ATTACK_SPEED: Scaling = Scaling {
    flat: &[],
    // "#% increased Attack Speed", local and global.
    incr: &["stat_210067635", "stat_681332047"],
};
const CRIT_CHANCE: Scaling = Scaling {
    // "#% to Critical Hit Chance".
    flat: &["stat_518292764"],
    incr: &[],
};
const SPIRIT: Scaling = Scaling {
    flat: &[],
    // "#% increased Spirit".
    incr: &["stat_1416406066", "stat_3984865854"],
};

/// EE2's `ARMOUR_STATS` (`item-property.ts:53-63`).
const ARMOUR_STATS: [&[&str]; 9] = [
    ARMOUR.flat,
    ARMOUR.incr,
    EVASION.flat,
    EVASION.incr,
    ENERGY_SHIELD.flat,
    ENERGY_SHIELD.incr,
    RUNIC_WARD.flat,
    RUNIC_WARD.incr,
    BLOCK.incr,
];
/// EE2's `WEAPON_STATS` (`item-property.ts:176-187`).
const WEAPON_STATS: [&[&str]; 6] = [
    PHYSICAL_DAMAGE.flat,
    PHYSICAL_DAMAGE.incr,
    ATTACK_SPEED.incr,
    CRIT_CHANCE.flat,
    ELEMENTAL_DAMAGE.flat,
    SPIRIT.incr,
];

/// The item's summed flat and increased rolls of `scaling`'s stats -- EE2's `calcPropBase`.
fn scaling_rolls(item: &ParsedItem, scaling: &Scaling) -> (f64, f64) {
    let mut flat = 0.0;
    let mut incr = 0.0;
    for stat in item.mods.iter().flat_map(|modifier| &modifier.stats) {
        let Some(hash) = stat.stat_id.as_deref().map(stat_hash) else {
            continue;
        };
        if scaling.flat.contains(&hash) {
            flat += stat.value;
        } else if scaling.incr.contains(&hash) {
            incr += stat.value;
        }
    }
    (flat, incr)
}

/// `calcFlat`: what `total` was before `incr`% increased and `more`% more (quality).
fn without_increases(total: f64, incr: f64, more: f64) -> f64 {
    total / (1.0 + more / 100.0) / (1.0 + incr / 100.0)
}

/// `calcIncreased`: `flat` after `incr`% increased and `more`% more (quality).
fn with_increases(flat: f64, incr: f64, more: f64) -> f64 {
    flat * (1.0 + incr / 100.0) * (1.0 + more / 100.0)
}

/// `total` recomputed at 20% quality -- EE2's `propAt20Quality`, which prices a modifiable
/// item's defences and physical damage as if already raised to 20%, where quality currency takes
/// any modifiable item. Quality multiplies separately from the item's own increases.
fn at_20_quality(item: &ParsedItem, total: f64, scaling: &Scaling) -> f64 {
    let (flat, incr) = scaling_rolls(item, scaling);
    let quality = f64::from(item.quality.unwrap_or(0));
    let base = without_increases(total, incr, quality) - flat;
    let quality = if is_modifiable(item) {
        quality.max(20.0)
    } else {
        quality
    };
    with_increases(base + flat, incr, quality)
}

/// EE2's `itemIsModifiable` (`ParsedItem.ts`): not corrupted, mirrored, sanctified or marked
/// Unmodifiable (EE2's parser sets `isCorrupted` for the latter), and not an identified unique
/// (EE2's item record for one has no `craftable` part).
pub(crate) fn is_modifiable(item: &ParsedItem) -> bool {
    let identified_unique = item.rarity == Some(ItemRarity::Unique) && !item.is_unidentified;
    !identified_unique
        && !item.is_corrupted
        && !item.is_mirrored
        && !item.is_sanctified
        && !item.is_unmodifiable
}

/// The trade id without its mod-type prefix: `explicit.stat_1509134228` -> `stat_1509134228`.
fn stat_hash(stat_id: &str) -> &str {
    stat_id.split_once('.').map_or(stat_id, |(_, hash)| hash)
}

fn category_id(item: &ParsedItem) -> &str {
    item.category
        .as_ref()
        .map_or("", |category| category.id.as_str())
}

/// EE2's `ARMOUR` categories (`parser/meta.ts`): every `armour.*` one but quivers.
fn is_armour(category: &str) -> bool {
    category.starts_with("armour.") && category != "armour.quiver"
}

/// EE2's `WEAPON` categories (`parser/meta.ts`): every `weapon.*` one.
fn is_weapon(category: &str) -> bool {
    category.starts_with("weapon.")
}

fn is_flask(category: &str) -> bool {
    matches!(category, "flask.life" | "flask.mana")
}

/// A property line's value, `None` when absent or zero -- EE2 tests these for truthiness.
fn present(value: Option<u32>) -> Option<f64> {
    value.filter(|&value| value != 0).map(f64::from)
}

fn present_f64(value: Option<f64>) -> Option<f64> {
    value.filter(|&value| value != 0.0)
}

/// The "Physical Damage: lo-hi" line at its average, as EE2's `getRollOrMinmaxAvg` reads it.
fn physical_damage(item: &ParsedItem) -> f64 {
    item.weapon_physical
        .map_or(0.0, |(lo, hi)| (f64::from(lo) + f64::from(hi)) / 2.0)
}

/// The fire, cold and lightning damage lines, each at its average, summed: EE2's
/// `weaponELEMENTAL`. Chaos damage is no elemental damage and EE2 reads no chaos line.
fn elemental_damage(item: &ParsedItem) -> f64 {
    item.weapon_elemental
        .iter()
        .filter(|&&(kind, ..)| kind != ElementKind::Chaos)
        .map(|&(_, lo, hi)| (f64::from(lo) + f64::from(hi)) / 2.0)
        .sum()
}

fn has_armour_properties(item: &ParsedItem) -> bool {
    [
        item.armour,
        item.evasion,
        item.energy_shield,
        item.runic_ward,
        item.block_chance,
    ]
    .into_iter()
    .any(|value| present(value).is_some())
}

fn has_weapon_properties(item: &ParsedItem) -> bool {
    present_f64(item.weapon_aps).is_some()
        || present_f64(item.weapon_crit).is_some()
        || elemental_damage(item) != 0.0
        || physical_damage(item) != 0.0
        || present(item.spirit).is_some()
}

/// EE2's `roundRoll` (`filters/util.ts`): the value as its row shows it -- truncated to a whole
/// number, or, for a decimal property, to 2 places below 2.3 and 1 place below 10. Unlike
/// `roundRoll`, float noise is absorbed first: 2.07 attacks per second recomputed through 25%
/// increased attack speed lands a hair below 2.07, which plain truncation shows as 2.06 (the
/// `high_damage_rare_item_en` fixture's crossbow).
fn shown_value(value: f64, dp: bool) -> f64 {
    let places = if !dp || value.abs() >= 10.0 {
        0
    } else if value.abs() < 2.3 {
        2
    } else {
        1
    };
    let scale = 10f64.powi(places);
    (value * scale + 1e-9).trunc() / scale
}

/// One property row searching `trade_id` (`<group>.<key>`), labelled with an English template
/// whose `#` stands for the value, bounded the way `bound` says once the search profile applies
/// (`apply_profile`). `enabled` is EE2's checkbox for a row keeping it (`scored`); a scored row
/// starts unchecked, for the profile to pick. Not hidden: the DPS rows EE2 hides set that
/// themselves.
fn property_row(
    label: &str,
    trade_id: &str,
    value: f64,
    dp: bool,
    enabled: bool,
    bound: RollBound,
) -> SearchFilter {
    SearchFilter {
        trade_ids: vec![trade_id.to_owned()],
        stat_ref: label.to_owned(),
        display_text: label.to_owned(),
        tag: FilterTag::Property,
        tier: None,
        roll: Some(SearchFilterRoll {
            value: shown_value(value, dp),
            min: None,
            max: None,
            dp,
            bound,
        }),
        enabled,
        hidden: false,
        generation: None,
        inverted: false,
        score: None,
        tier_info: None,
        weighted_sum: false,
    }
}

/// EE2's `maxUsefulItemLevel` (`filters/common.ts`): the item level past which a higher one
/// unlocks nothing more on this kind of item; 1 means item level doesn't matter at all.
fn max_useful_item_level(category: &str) -> u32 {
    match category {
        "weapon.wand" | "weapon.staff" => 81,
        "sanctum.relic" => 80,
        "map.tablet" | "jewel" | "map.waystone" => 1,
        _ => 82,
    }
}

/// `create-item-filters.ts:379-405`: a minimum item level, capped at the useful maximum, for
/// every non-unique whose item level matters -- enabled in the exact preset except on flasks and
/// charms, and on any veiled item (`create-item-filters.ts:468-472`).
fn item_level_row(item: &ParsedItem, category: &str, exact: bool) -> Option<SearchFilter> {
    let item_level = item.item_level?;
    let max_useful = max_useful_item_level(category);
    if max_useful == 1 || item.rarity == Some(ItemRarity::Unique) || category == "map.logbook" {
        return None;
    }
    let enabled = (exact && !is_flask(category) && category != "flask.charm") || item.is_veiled;
    Some(property_row(
        "Item Level: #",
        "type_filters.ilvl",
        f64::from(item_level.min(max_useful)),
        false,
        enabled,
        RollBound::AtLeast,
    ))
}

/// `create-item-filters.ts:269-278`: a minimum rune-socket count, enabled once the item has more
/// sockets than its base normally does, or is corrupted.
fn rune_sockets_row(item: &ParsedItem) -> Option<SearchFilter> {
    let sockets = item.sockets.filter(|sockets| sockets.current != 0)?;
    let enabled = sockets.current > sockets.normal || item.is_corrupted;
    Some(property_row(
        "Sockets: #",
        "equipment_filters.rune_sockets",
        f64::from(sockets.current),
        false,
        enabled,
        RollBound::AtLeast,
    ))
}

/// `create-item-filters.ts:229-253`: a minimum quality, only where quality says something about
/// the item: a flask at 20% or more (enabled above 20%), exceptional quality above 20% on gear
/// (enabled unless rare: EE2 takes a rare's crafting as mostly done, its quality as no selling
/// point), and any charm (enabled from 10%).
fn quality_row(item: &ParsedItem, category: &str) -> Option<SearchFilter> {
    let quality = item.quality.filter(|&quality| quality != 0)?;
    let charm = category == "flask.charm";
    let enabled = if quality >= 20 && is_flask(category) {
        quality > 20
    } else if quality > 20
        && (is_flask(category) || charm || is_armour(category) || is_weapon(category))
    {
        item.rarity != Some(ItemRarity::Rare)
    } else if charm {
        quality >= 10
    } else {
        return None;
    };
    Some(property_row(
        "Quality: #%",
        "type_filters.quality",
        f64::from(quality),
        false,
        enabled,
        RollBound::AtLeast,
    ))
}

/// `armourProps` (`item-property.ts:65-174`): armour, evasion and energy shield at 20% quality;
/// block and runic ward as they are.
fn armour_rows(item: &ParsedItem, rows: &mut Vec<SearchFilter>) {
    let defences = [
        (item.armour, &ARMOUR, "Armour: #", "equipment_filters.ar"),
        (
            item.evasion,
            &EVASION,
            "Evasion Rating: #",
            "equipment_filters.ev",
        ),
        (
            item.energy_shield,
            &ENERGY_SHIELD,
            "Energy Shield: #",
            "equipment_filters.es",
        ),
    ];
    for (value, scaling, label, trade_id) in defences {
        if let Some(value) = present(value) {
            let value = at_20_quality(item, value, scaling);
            rows.push(property_row(
                label,
                trade_id,
                value,
                false,
                false,
                RollBound::Higher,
            ));
        }
    }
    if let Some(block) = present(item.block_chance) {
        rows.push(property_row(
            "Block: #%",
            "equipment_filters.block",
            block,
            false,
            false,
            RollBound::Higher,
        ));
    }
    if let Some(ward) = present(item.runic_ward) {
        rows.push(property_row(
            "Runic Ward: #",
            "equipment_filters.ward",
            ward,
            false,
            false,
            RollBound::Higher,
        ));
    }
}

/// `weaponProps` (`item-property.ts:189-447`). Physical DPS is the physical damage at 20% quality
/// times attacks per second, elemental DPS the fire, cold and lightning damage times attacks per
/// second, total DPS their sum. Total and elemental DPS only exist on a weapon with elemental
/// damage. EE2 hides the elemental DPS row under 15% of the total and the physical one under 67%.
/// Reload time is better lower.
fn weapon_rows(item: &ParsedItem, rows: &mut Vec<SearchFilter>) {
    let attacks_per_second = item.weapon_aps.unwrap_or(0.0);
    let physical = physical_damage(item);
    let elemental = elemental_damage(item);
    let physical_dps = at_20_quality(item, physical, &PHYSICAL_DAMAGE) * attacks_per_second;
    let elemental_dps = elemental * attacks_per_second;
    let total_dps = physical_dps + elemental_dps;
    let row = |label, trade_id, value, dp| {
        property_row(label, trade_id, value, dp, false, RollBound::Higher)
    };

    if elemental != 0.0 {
        rows.push(row(
            "Total DPS: #",
            "equipment_filters.dps",
            total_dps,
            false,
        ));
        rows.push(SearchFilter {
            hidden: elemental_dps / total_dps < 0.15,
            ..row(
                "Elemental DPS: #",
                "equipment_filters.edps",
                elemental_dps,
                false,
            )
        });
    }
    if physical != 0.0 {
        rows.push(SearchFilter {
            hidden: physical_dps / total_dps < 0.67,
            ..row(
                "Physical DPS: #",
                "equipment_filters.pdps",
                physical_dps,
                false,
            )
        });
    }
    if present_f64(item.weapon_aps).is_some() {
        rows.push(row(
            "Attacks per Second: #",
            "equipment_filters.aps",
            attacks_per_second,
            true,
        ));
    }
    if let Some(crit) = present_f64(item.weapon_crit) {
        rows.push(row(
            "Critical Hit Chance: #%",
            "equipment_filters.crit",
            crit,
            true,
        ));
    }
    if let Some(reload) = present_f64(item.weapon_reload_time) {
        // Lower is better, so EE2 presets the upper bound instead (`calculatedStatToFilter`'s
        // `item.reload_time` case).
        rows.push(property_row(
            "Reload Time: #",
            "equipment_filters.reload_time",
            reload,
            true,
            false,
            RollBound::Lower,
        ));
    }
    if let Some(spirit) = present(item.spirit) {
        rows.push(row("Spirit: #", "equipment_filters.spirit", spirit, false));
    }
}

#[cfg(test)]
mod tests {
    use poe2_domain::{
        AugmentSockets, ItemCategory, ModifierInfo, ModifierType, ParsedModifier, ParsedStat,
    };

    use super::*;

    fn category(id: &str) -> Option<ItemCategory> {
        Some(ItemCategory {
            id: id.to_owned(),
            display_name: id.to_owned(),
        })
    }

    /// One explicit mod with one stat rolled `value` within `min..=max`.
    fn explicit(stat_id: &str, value: f64, min: f64, max: f64) -> ParsedModifier {
        ParsedModifier {
            info: ModifierInfo {
                modifier_type: ModifierType::Explicit,
                generation: None,
                name: None,
                tier: None,
                rank: None,
                tags: Vec::new(),
            },
            stats: vec![ParsedStat {
                stat_id: Some(stat_id.to_owned()),
                text: String::new(),
                value,
                min,
                max,
                dp: false,
                unscalable: false,
                negated_text: None,
                printed_text: None,
            }],
        }
    }

    fn row<'a>(rows: &'a [SearchFilter], trade_id: &str) -> &'a SearchFilter {
        rows.iter()
            .find(|row| row.trade_ids == [trade_id])
            .unwrap_or_else(|| panic!("no {trade_id} row"))
    }

    /// A rare two-hand mace at 10% quality: 100-200 physical and 20-40 fire damage, 1.2 attacks
    /// per second; 50% (40-60) increased physical damage, 10% (8-12) increased attack speed and
    /// its added fire damage averaging 30 (25-35).
    fn rare_mace() -> ParsedItem {
        ParsedItem {
            rarity: Some(ItemRarity::Rare),
            category: category("weapon.twomace"),
            quality: Some(10),
            weapon_physical: Some((100, 200)),
            weapon_elemental: vec![(ElementKind::Fire, 20, 40)],
            weapon_aps: Some(1.2),
            mods: vec![
                explicit("explicit.stat_1509134228", 50.0, 40.0, 60.0),
                explicit("explicit.stat_210067635", 10.0, 8.0, 12.0),
                explicit("explicit.stat_709508406", 30.0, 25.0, 35.0),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn weapon_dps_follows_ee2_at_20_quality_and_broad_bounds_drop_it_by_a_tenth() {
        let mut rows = property_filters(&rare_mace());
        crate::apply_profile(&mut rows, crate::SearchProfile::Broad);

        // Physical: 150 average at 10% quality with 50% increased is 90.9 base, 163.6 at 20%
        // quality, times 1.2 attacks per second: 196.4, shown 196; Broad searches from
        // 196 - 19.6 = 176.4, rounded down: 176. Elemental: 30 * 1.2 = 36, from 32.4: 32. Total:
        // 232.4, shown 232, from 208.8: 208. Attacks per second 1.2, from 1.08.
        let min = |trade_id| {
            let roll = row(&rows, trade_id).roll.as_ref().expect("roll");
            (roll.value, roll.min, roll.max)
        };
        assert_eq!(min("equipment_filters.pdps"), (196.0, Some(176.0), None));
        assert_eq!(min("equipment_filters.edps"), (36.0, Some(32.0), None));
        assert_eq!(min("equipment_filters.dps"), (232.0, Some(208.0), None));
        assert_eq!(min("equipment_filters.aps"), (1.2, Some(1.08), None));
        // 84% physical, 15.5% elemental: neither share is minor. A scored row starts unchecked,
        // for the search profile to pick.
        assert!(rows.iter().all(|row| !row.hidden && !row.enabled));
    }

    #[test]
    fn attack_speed_shows_as_the_item_reads() {
        let crossbow = ParsedItem {
            rarity: Some(ItemRarity::Rare),
            category: category("weapon.crossbow"),
            weapon_aps: Some(2.07),
            mods: vec![explicit("fractured.stat_210067635", 25.0, 23.0, 25.0)],
            ..Default::default()
        };
        let rows = property_filters(&crossbow);
        let attack_speed = row(&rows, "equipment_filters.aps");
        assert_eq!(attack_speed.roll.as_ref().expect("roll").value, 2.07);
    }

    #[test]
    fn minor_dps_shares_are_hidden() {
        let mut mostly_fire = rare_mace();
        mostly_fire.weapon_elemental = vec![(ElementKind::Fire, 200, 400)];
        let rows = property_filters(&mostly_fire);
        assert!(
            row(&rows, "equipment_filters.pdps").hidden,
            "below 67% of the total"
        );

        let mut barely_fire = rare_mace();
        barely_fire.weapon_elemental = vec![(ElementKind::Fire, 10, 20)];
        // Its 15 fire damage is all base: no added fire damage mod.
        barely_fire.mods.truncate(2);
        let rows = property_filters(&barely_fire);
        assert!(
            row(&rows, "equipment_filters.edps").hidden,
            "below 15% of the total"
        );
    }

    #[test]
    fn corrupted_armour_keeps_its_quality_while_modifiable_armour_is_priced_at_20() {
        let body_armour = ParsedItem {
            rarity: Some(ItemRarity::Rare),
            category: category("armour.chest"),
            armour: Some(300),
            mods: vec![explicit("explicit.stat_1062208444", 50.0, 40.0, 60.0)],
            ..Default::default()
        };
        let rows = property_filters(&body_armour);
        let armour = row(&rows, "equipment_filters.ar");
        // 300 / 1.5 = 200 base; at 20% quality 200 * 1.5 * 1.2 = 360.
        assert_eq!(armour.roll.as_ref().expect("roll").value, 360.0);

        let corrupted = ParsedItem {
            is_corrupted: true,
            ..body_armour
        };
        let rows = property_filters(&corrupted);
        let armour = row(&rows, "equipment_filters.ar");
        assert_eq!(armour.roll.as_ref().expect("roll").value, 300.0);
    }

    #[test]
    fn normal_items_get_the_exact_preset_rows_only() {
        let body_armour = ParsedItem {
            rarity: Some(ItemRarity::Normal),
            category: category("armour.chest"),
            armour: Some(300),
            item_level: Some(84),
            sockets: Some(AugmentSockets {
                empty: 3,
                current: 3,
                normal: 2,
            }),
            ..Default::default()
        };
        let rows = property_filters(&body_armour);

        let item_level = row(&rows, "type_filters.ilvl");
        assert!(item_level.enabled);
        let item_level_roll = item_level.roll.as_ref().expect("roll");
        assert_eq!(
            (item_level_roll.value, item_level_roll.bound),
            (82.0, RollBound::AtLeast),
            "a minimum, capped at the useful maximum"
        );
        assert!(
            row(&rows, "equipment_filters.rune_sockets").enabled,
            "3 sockets on a 2-socket base"
        );
        assert!(
            rows.iter()
                .all(|row| row.trade_ids != ["equipment_filters.ar"]),
            "EE2's exact preset has no defence rows"
        );

        let rare = ParsedItem {
            rarity: Some(ItemRarity::Rare),
            ..body_armour
        };
        let rows = property_filters(&rare);
        assert!(!row(&rows, "type_filters.ilvl").enabled);
        assert!(
            rows.iter()
                .any(|row| row.trade_ids == ["equipment_filters.ar"])
        );
    }

    #[test]
    fn only_properties_of_the_items_own_kind_fold_its_stats() {
        let local_armour = "explicit.stat_1062208444";
        let body_armour = ParsedItem {
            rarity: Some(ItemRarity::Rare),
            category: category("armour.chest"),
            armour: Some(300),
            ..Default::default()
        };
        assert!(folded_into_properties(&body_armour, local_armour));
        assert!(folded_into_properties(&body_armour, "rune.stat_1062208444"));
        assert!(!folded_into_properties(
            &body_armour,
            "explicit.stat_1509134228"
        ));

        let ring = ParsedItem {
            category: category("accessory.ring"),
            ..body_armour.clone()
        };
        assert!(!folded_into_properties(&ring, local_armour));

        let normal = ParsedItem {
            rarity: Some(ItemRarity::Normal),
            ..body_armour
        };
        assert!(
            !folded_into_properties(&normal, local_armour),
            "no property rows to fold into"
        );
    }
}
