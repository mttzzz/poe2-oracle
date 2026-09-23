//! Pseudo stat rows: trade-site totals that fold several real stats into one searchable number
//! (every elemental resistance into `+#% total Elemental Resistance`, Strength into `+# total
//! maximum Life`, ...). A port of EE2's `filterPseudo` and its active `PSEUDO_RULES`
//! (`exiled-exchange-2/renderer/src/web/price-check/filters/pseudo/index.ts`), minus the Tablet
//! `# uses remaining` rule: its inputs are two-line stats, which `item-parser` (resolving one line
//! at a time) never matches to a trade id.
//!
//! Stats match by the language-independent half of their trade id (`stat_3372524247` of
//! `explicit.stat_3372524247`), never by text, so a Russian item folds exactly like an English
//! one. Every modifier type counts (implicit, rune, fractured, ...), as in EE2.
//!
//! EE2 removes the per-mod rows a rule reads; here they stay, unselected and hidden
//! (`SearchFilter::hidden`), so every folded stat is still one click away behind the panel's
//! "show hidden" toggle. The pseudo rows EE2 hides are hidden the same way. A total starts
//! unselected: the search profile picks it by the best score of the mods it sums (`rank`), as
//! PoE Overlay II's `finalizeModMetadata` scores a pseudo total.

use poe2_domain::{ItemRarity, ModifierType, ParsedItem, StatCatalog};

use crate::property::{folded_into_properties, uses_exact_preset};
use crate::{FilterTag, RollBound, SearchFilter, SearchFilterRoll, catalog_text};

// Resistance bits of `Stat::Resistance`: an EE2 `RESISTANCES_INFO` entry's elements and chaos.
const FIRE: u8 = 1;
const COLD: u8 = 1 << 1;
const LIGHTNING: u8 = 1 << 2;
const CHAOS: u8 = 1 << 3;
const ELEMENTS: u8 = FIRE | COLD | LIGHTNING;

// Attribute bits of `Stat::Attributes`: an EE2 `ATTRIBUTES_INFO` entry's attributes.
const STR: u8 = 1;
const DEX: u8 = 1 << 1;
const INT: u8 = 1 << 2;
const ALL_ATTRIBUTES: u8 = STR | DEX | INT;

/// A stat some rule reads. EE2 matches its stat refs; this port matches the trade-id hashes those
/// refs resolve to (`STATS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stat {
    /// Grants the resistances in the bit set.
    Resistance(u8),
    /// Grants the attributes in the bit set.
    Attributes(u8),
    MaximumLife,
    MaximumMana,
    IncreasedMaximumEnergyShield,
    MaximumEnergyShield,
    MovementSpeed,
}

/// Every stat a rule reads, by trade-id hash. Resolved 2026-09-22 through EE2's own mapping: each
/// ref in its rule tables, looked up in EE2's `renderer/public/data/en/stats.ndjson`, lists the
/// trade ids of every mod type it appears under; each hash below was then confirmed under that
/// same text in the live EN catalog (`GET https://www.pathofexile.com/api/trade2/data/stats`). A
/// ref resolving to two hashes keeps both: EE2 folds either line into the total.
static STATS: &[(&str, Stat)] = &[
    // #% to All Resistances (sanctum relics only)
    ("stat_3128852541", Stat::Resistance(ELEMENTS | CHAOS)),
    // #% to all Elemental Resistances
    ("stat_2901986750", Stat::Resistance(ELEMENTS)),
    // #% to Fire / Cold / Lightning Resistance
    ("stat_3372524247", Stat::Resistance(FIRE)),
    ("stat_4220027924", Stat::Resistance(COLD)),
    ("stat_1671376347", Stat::Resistance(LIGHTNING)),
    // #% to Fire and Lightning / Fire and Cold / Cold and Lightning Resistances
    ("stat_3441501978", Stat::Resistance(FIRE | LIGHTNING)),
    ("stat_2915988346", Stat::Resistance(FIRE | COLD)),
    ("stat_4277795662", Stat::Resistance(COLD | LIGHTNING)),
    // #% to Chaos Resistance
    ("stat_2923486259", Stat::Resistance(CHAOS)),
    // #% to Fire and Chaos / Cold and Chaos / Lightning and Chaos Resistances
    ("stat_378817135", Stat::Resistance(FIRE | CHAOS)),
    ("stat_3393628375", Stat::Resistance(COLD | CHAOS)),
    ("stat_3465022881", Stat::Resistance(LIGHTNING | CHAOS)),
    // # to all Attributes (two ids, same text)
    ("stat_1379411836", Stat::Attributes(ALL_ATTRIBUTES)),
    ("stat_2897413282", Stat::Attributes(ALL_ATTRIBUTES)),
    // # to Strength / Dexterity / Intelligence
    ("stat_4080418644", Stat::Attributes(STR)),
    ("stat_3261801346", Stat::Attributes(DEX)),
    ("stat_328541901", Stat::Attributes(INT)),
    // # to Strength and Intelligence / Strength and Dexterity / Dexterity and Intelligence
    ("stat_1535626285", Stat::Attributes(STR | INT)),
    ("stat_538848803", Stat::Attributes(STR | DEX)),
    ("stat_2300185227", Stat::Attributes(DEX | INT)),
    // # to maximum Life / # to maximum Mana
    ("stat_3299347043", Stat::MaximumLife),
    ("stat_1050105434", Stat::MaximumMana),
    // #% increased maximum Energy Shield (gear, sanctum)
    ("stat_2482852589", Stat::IncreasedMaximumEnergyShield),
    ("stat_1707887759", Stat::IncreasedMaximumEnergyShield),
    // # to maximum Energy Shield (global, local)
    ("stat_3489782002", Stat::MaximumEnergyShield),
    ("stat_4052037485", Stat::MaximumEnergyShield),
    // #% increased Movement Speed (gear, sanctum)
    ("stat_2250533757", Stat::MovementSpeed),
    ("stat_1416455556", Stat::MovementSpeed),
];

/// Whether a row starts hidden: EE2's `hidden` and its `mutate` hooks. A hidden row is never
/// selected.
#[derive(Clone, Copy)]
enum Hide {
    Never,
    /// EE2's all-elemental total (`filters.hide_total_all_res`).
    Always,
    /// When the row's only source is one of these types: EE2's chaos resistance granted by a lone
    /// rune (`filters.hide_crafted_chaos`).
    WhenSoleSourceIs(&'static [ModifierType]),
}

/// EE2's rule `group`s, each settled by a pass over its rows once every rule has run
/// (`settle_groups`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    /// `to_all_res`: re-totalled as what every element is guaranteed.
    AllElementalResistances,
    /// `to_x_ele_res`: only a strictly strongest element's total stays, hidden.
    ElementalResistance,
    /// `to_all_attrs`: stays only for an even three-way attribute spread.
    AllAttributes,
    /// `to_x_attr`: dropped for an even three-way spread, in favour of `AllAttributes`.
    Attribute,
}

struct Rule {
    /// Pseudo trade id; every one confirmed in the live EN catalog's `pseudo` group, 2026-09-22.
    id: &'static str,
    /// That catalog's English text for `id`: the display fallback when the site's catalog lacks
    /// the entry.
    english: &'static str,
    /// The multiplier a stat counts at, `None` when it is no input (EE2's rule `stats` list).
    weight: fn(Stat) -> Option<f64>,
    /// A stat the item must carry for the rule to fire (EE2's `required: true`).
    required: Option<Stat>,
    hide: Hide,
    group: Option<Group>,
}

/// EE2's active rules that apply to gear, in its order, which is also the row order.
static RULES: &[Rule] = &[
    Rule {
        id: "pseudo.pseudo_total_all_elemental_resistances",
        english: "+#% total to all Elemental Resistances",
        weight: |stat| resists(stat, ELEMENTS),
        required: None,
        hide: Hide::Always,
        group: Some(Group::AllElementalResistances),
    },
    Rule {
        id: "pseudo.pseudo_total_elemental_resistance",
        english: "+#% total Elemental Resistance",
        // Counted once per element granted: `+10% to all Elemental Resistances` adds 30.
        weight: |stat| match stat {
            Stat::Resistance(granted) if granted & ELEMENTS != 0 => {
                Some(f64::from((granted & ELEMENTS).count_ones()))
            }
            _ => None,
        },
        required: None,
        hide: Hide::Never,
        group: None,
    },
    Rule {
        id: "pseudo.pseudo_total_fire_resistance",
        english: "+#% total to Fire Resistance",
        weight: |stat| resists(stat, FIRE),
        required: None,
        hide: Hide::Never,
        group: Some(Group::ElementalResistance),
    },
    Rule {
        id: "pseudo.pseudo_total_cold_resistance",
        english: "+#% total to Cold Resistance",
        weight: |stat| resists(stat, COLD),
        required: None,
        hide: Hide::Never,
        group: Some(Group::ElementalResistance),
    },
    Rule {
        id: "pseudo.pseudo_total_lightning_resistance",
        english: "+#% total to Lightning Resistance",
        weight: |stat| resists(stat, LIGHTNING),
        required: None,
        hide: Hide::Never,
        group: Some(Group::ElementalResistance),
    },
    Rule {
        id: "pseudo.pseudo_total_chaos_resistance",
        english: "+#% total to Chaos Resistance",
        weight: |stat| resists(stat, CHAOS),
        required: None,
        hide: Hide::WhenSoleSourceIs(&[ModifierType::Augment, ModifierType::AddedAugment]),
        group: None,
    },
    Rule {
        id: "pseudo.pseudo_total_all_attributes",
        english: "+# total to all Attributes",
        // `# to all Attributes` alone; EE2 deliberately leaves the other attribute stats out.
        weight: |stat| only(stat, Stat::Attributes(ALL_ATTRIBUTES)),
        required: None,
        hide: Hide::Never,
        group: Some(Group::AllAttributes),
    },
    Rule {
        id: "pseudo.pseudo_total_strength",
        english: "+# total to Strength",
        weight: |stat| attribute(stat, STR),
        required: None,
        hide: Hide::Never,
        group: Some(Group::Attribute),
    },
    Rule {
        id: "pseudo.pseudo_total_dexterity",
        english: "+# total to Dexterity",
        weight: |stat| attribute(stat, DEX),
        required: None,
        hide: Hide::Never,
        group: Some(Group::Attribute),
    },
    Rule {
        id: "pseudo.pseudo_total_intelligence",
        english: "+# total to Intelligence",
        weight: |stat| attribute(stat, INT),
        required: None,
        hide: Hide::Never,
        group: Some(Group::Attribute),
    },
    Rule {
        id: "pseudo.pseudo_total_life",
        english: "+# total maximum Life",
        // EE2 counts every point of Strength as 2 Life.
        weight: |stat| match stat {
            Stat::MaximumLife => Some(1.0),
            Stat::Attributes(granted) if granted & STR != 0 => Some(2.0),
            _ => None,
        },
        required: Some(Stat::MaximumLife),
        hide: Hide::Never,
        group: None,
    },
    Rule {
        id: "pseudo.pseudo_total_mana",
        english: "+# total maximum Mana",
        // EE2 counts every point of Intelligence as 2 Mana.
        weight: |stat| match stat {
            Stat::MaximumMana => Some(1.0),
            Stat::Attributes(granted) if granted & INT != 0 => Some(2.0),
            _ => None,
        },
        required: Some(Stat::MaximumMana),
        hide: Hide::Never,
        group: None,
    },
    Rule {
        id: "pseudo.pseudo_increased_energy_shield",
        english: "#% total increased maximum Energy Shield",
        weight: |stat| only(stat, Stat::IncreasedMaximumEnergyShield),
        required: None,
        hide: Hide::Never,
        group: None,
    },
    Rule {
        id: "pseudo.pseudo_total_energy_shield",
        english: "+# total maximum Energy Shield",
        weight: |stat| only(stat, Stat::MaximumEnergyShield),
        required: None,
        hide: Hide::Never,
        group: None,
    },
    Rule {
        id: "pseudo.pseudo_increased_movement_speed",
        english: "#% increased Movement Speed",
        weight: |stat| only(stat, Stat::MovementSpeed),
        required: None,
        hide: Hide::Never,
        group: None,
    },
];

/// Weight 1 for a resistance granting any of `bits`.
fn resists(stat: Stat, bits: u8) -> Option<f64> {
    matches!(stat, Stat::Resistance(granted) if granted & bits != 0).then_some(1.0)
}

/// Weight 1 for an attribute stat granting `bit`.
fn attribute(stat: Stat, bit: u8) -> Option<f64> {
    matches!(stat, Stat::Attributes(granted) if granted & bit != 0).then_some(1.0)
}

/// Weight 1 for exactly `wanted`.
fn only(stat: Stat, wanted: Stat) -> Option<f64> {
    (stat == wanted).then_some(1.0)
}

/// Trade categories whose bases take rune sockets: EE2's `getMaxSockets(item) > 0`
/// (`parser/Parser.ts:2043-2073`) through its category-to-trade-id table
/// (`trade/pathofexile-trade.ts:38-87`).
const SOCKETED_CATEGORIES: &[&str] = &[
    "armour.chest",
    "armour.helmet",
    "armour.gloves",
    "armour.boots",
    "armour.shield",
    "armour.buckler",
    "armour.focus",
    "weapon.oneaxe",
    "weapon.onemace",
    "weapon.onesword",
    "weapon.claw",
    "weapon.dagger",
    "weapon.spear",
    "weapon.flail",
    "weapon.wand",
    "weapon.sceptre",
    "weapon.twoaxe",
    "weapon.twomace",
    "weapon.twosword",
    "weapon.bow",
    "weapon.crossbow",
    "weapon.warstaff",
    "weapon.staff",
    "weapon.talisman",
];

/// Split Personality as the EN and RU clients name it (RU from EE2's game-extracted
/// `renderer/public/data/ru/items.ndjson`): EE2 exempts it from pseudo rows by name
/// (`create-stat-filters.ts:249`).
const SPLIT_PERSONALITY: [&str; 2] = ["Split Personality", "Раздвоение личности"];

/// Summed value and roll range (EE2's `statSourcesTotal`).
#[derive(Debug, Clone, Copy, Default)]
struct Total {
    value: f64,
    min: f64,
    max: f64,
}

impl Total {
    fn add(&mut self, other: Total, weight: f64) {
        self.value += other.value * weight;
        self.min += other.min * weight;
        self.max += other.max * weight;
    }
}

/// One stat line on the item that some rule reads (EE2's `StatSource`), and its mod's index in
/// `item.mods`.
struct Source {
    stat: Stat,
    modifier_type: ModifierType,
    modifier: usize,
    roll: Total,
}

struct Row {
    rule: &'static Rule,
    total: Total,
    hidden: bool,
    /// The mods whose lines the total counts.
    sources: Vec<usize>,
}

/// EE2's `filterPseudo`: the pseudo rows for `item`, in rule order, each with the mods it sums
/// (indexes into `item.mods`), and with every row in `mod_filters` that a rule reads deselected
/// and hidden. Each row searches its pseudo trade id and reads as that id's template in `catalog`
/// (the site's language), or the English one when `catalog` lacks it. Stats a property row
/// already counts feed no total: EE2 removes them first (`item-property.ts:165-173`).
///
/// EE2 runs it only in its default preset (`create-presets.ts:45-83`), so an item it prices with
/// the exact preset (`uses_exact_preset`) gets neither; nor do Split Personality and uniques that
/// take rune sockets (`create-stat-filters.ts:249-254`): listings of one unique carry whatever
/// runes their owners socketed, so a total counting this copy's runes would not describe them.
pub(crate) fn pseudo_filters(
    item: &ParsedItem,
    catalog: &StatCatalog,
    mod_filters: &mut [SearchFilter],
) -> Vec<(SearchFilter, Vec<usize>)> {
    if uses_exact_preset(item)
        || (item.rarity == Some(ItemRarity::Unique)
            && (takes_rune_sockets(item) || SPLIT_PERSONALITY.contains(&item.name.as_str())))
    {
        return Vec::new();
    }

    // EE2 drops these rows whether or not their rule produced one (`index.ts:434-439`).
    for filter in mod_filters.iter_mut() {
        if filter
            .trade_ids
            .first()
            .is_some_and(|id| classify(id).is_some())
        {
            filter.enabled = false;
            filter.hidden = true;
        }
    }

    let sources = sources(item);
    let mut rows: Vec<Row> = RULES
        .iter()
        .filter_map(|rule| evaluate(rule, &sources))
        .collect();
    settle_groups(&mut rows, &sources);
    rows.into_iter()
        .map(|row| {
            // A total with no range to roll in is searched at its value whatever the profile.
            let bound = if row.total.min == row.total.max {
                RollBound::AtLeast
            } else {
                RollBound::Higher
            };
            let filter = SearchFilter {
                trade_ids: vec![row.rule.id.to_owned()],
                stat_ref: row.rule.english.to_owned(),
                display_text: catalog_text(catalog, row.rule.id, row.rule.english).to_owned(),
                tag: FilterTag::Pseudo,
                // A total spans mods of different tiers; no single tier describes it.
                tier: None,
                // EE2's pseudo stats carry no decimals.
                roll: Some(SearchFilterRoll {
                    value: row.total.value,
                    min: None,
                    max: None,
                    dp: false,
                    bound,
                }),
                enabled: false,
                hidden: row.hidden,
                generation: None,
                inverted: false,
                score: None,
                tier_info: None,
                weighted_sum: false,
            };
            (filter, row.sources)
        })
        .collect()
}

/// Whether `item`'s base takes rune sockets (EE2's `getMaxSockets(item) > 0`). EE2 also names
/// uniques that add sockets to a base without any (Darkness Enthroned, a belt); names are
/// localized, so here any item printing a rune socket counts too.
fn takes_rune_sockets(item: &ParsedItem) -> bool {
    item.sockets.is_some_and(|sockets| sockets.normal > 0)
        || item
            .category
            .as_ref()
            .is_some_and(|category| SOCKETED_CATEGORIES.contains(&category.id.as_str()))
}

/// The rule input behind a full trade id (`explicit.stat_3372524247`), by its hash.
fn classify(trade_id: &str) -> Option<Stat> {
    let (_, hash) = trade_id.split_once('.')?;
    STATS
        .iter()
        .find(|&&(known, _)| known == hash)
        .map(|&(_, stat)| stat)
}

/// Every stat line on `item` that some rule reads and no property row counts.
fn sources(item: &ParsedItem) -> Vec<Source> {
    item.mods
        .iter()
        .enumerate()
        .flat_map(|(index, modifier)| {
            modifier.stats.iter().filter_map(move |stat| {
                let stat_id = stat.stat_id.as_deref()?;
                if folded_into_properties(item, stat_id) {
                    return None;
                }
                Some(Source {
                    stat: classify(stat_id)?,
                    modifier_type: modifier.info.modifier_type,
                    modifier: index,
                    roll: Total {
                        value: stat.value,
                        min: stat.min,
                        max: stat.max,
                    },
                })
            })
        })
        .collect()
}

/// One rule over the item's sources (EE2's `rulesLoop` body): `None` when nothing feeds it or its
/// required stat is missing.
fn evaluate(rule: &'static Rule, sources: &[Source]) -> Option<Row> {
    let mut total = Total::default();
    let mut count = 0;
    let mut first_type = None;
    let mut has_required = rule.required.is_none();
    let mut modifiers = Vec::new();
    for source in sources {
        let Some(weight) = (rule.weight)(source.stat) else {
            continue;
        };
        total.add(source.roll, weight);
        count += 1;
        first_type.get_or_insert(source.modifier_type);
        has_required |= rule.required == Some(source.stat);
        if !modifiers.contains(&source.modifier) {
            modifiers.push(source.modifier);
        }
    }
    if count == 0 || !has_required {
        return None;
    }
    let hidden = match rule.hide {
        Hide::Never => false,
        Hide::Always => true,
        Hide::WhenSoleSourceIs(types) => {
            count == 1 && first_type.is_some_and(|first| types.contains(&first))
        }
    };
    Some(Row {
        rule,
        total,
        hidden,
        sources: modifiers,
    })
}

/// EE2's group passes (`index.ts:441-586`), once every rule has produced its row.
fn settle_groups(rows: &mut Vec<Row>, sources: &[Source]) {
    if let Some(row) = rows
        .iter_mut()
        .find(|row| row.rule.group == Some(Group::AllElementalResistances))
    {
        row.total = guaranteed_to_every_element(sources);
    }
    rows.retain(|row| {
        row.rule.group != Some(Group::AllElementalResistances) || row.total.value != 0.0
    });

    let strongest = strictly_largest(rows, Group::ElementalResistance);
    rows.retain(|row| {
        row.rule.group != Some(Group::ElementalResistance) || Some(row.total.value) == strongest
    });
    // The one kept is EE2's `filters.hide_ele_res`: an option behind the toggle, never a default
    // row.
    for row in rows
        .iter_mut()
        .filter(|row| row.rule.group == Some(Group::ElementalResistance))
    {
        row.hidden = true;
    }

    if group_values(rows, Group::Attribute).count() == 3 {
        let first = group_values(rows, Group::Attribute).next();
        let even = group_values(rows, Group::Attribute).all(|value| Some(value) == first);
        let has_all = rows
            .iter()
            .any(|row| row.rule.group == Some(Group::AllAttributes));
        if even && has_all {
            rows.retain(|row| row.rule.group != Some(Group::Attribute));
        } else {
            rows.retain(|row| row.rule.group != Some(Group::AllAttributes));
            hide_minor_attributes(rows);
        }
    }
}

/// EE2's `hide_attr_*`: of the three attribute totals, ranked, a third under 30% of the first is
/// hidden, and the second with it when the two are equal.
fn hide_minor_attributes(rows: &mut [Row]) {
    let mut ranked: Vec<&mut Row> = rows
        .iter_mut()
        .filter(|row| row.rule.group == Some(Group::Attribute))
        .collect();
    // Stable, as EE2's sort is: equal totals keep rule order.
    ranked.sort_by(|a, b| b.total.value.total_cmp(&a.total.value));
    if let [first, second, third] = ranked.as_mut_slice()
        && third.total.value / first.total.value < 0.3
    {
        third.hidden = true;
        if second.total.value == third.total.value {
            second.hidden = true;
        }
    }
}

/// EE2's `to_all_res` total: sources granting every element, plus the weakest element's single-
/// and dual-element sources -- the resistance each element is sure to get.
fn guaranteed_to_every_element(sources: &[Source]) -> Total {
    let mut all = Total::default();
    let mut per_element = [Total::default(); 3];
    for source in sources {
        let Stat::Resistance(granted) = source.stat else {
            continue;
        };
        if granted & ELEMENTS == ELEMENTS {
            all.add(source.roll, 1.0);
            continue;
        }
        for (element, bit) in per_element.iter_mut().zip([FIRE, COLD, LIGHTNING]) {
            if granted & bit != 0 {
                element.add(source.roll, 1.0);
            }
        }
    }
    // EE2's strict `<` scan: a tie keeps the earlier element, whose roll range then bounds the row.
    let [fire, cold, lightning] = per_element;
    let mut weakest = fire;
    for element in [cold, lightning] {
        if element.value < weakest.value {
            weakest = element;
        }
    }
    all.add(weakest, 1.0);
    all
}

/// `group`'s largest total when exactly one row holds it; EE2 keeps none on a tie.
fn strictly_largest(rows: &[Row], group: Group) -> Option<f64> {
    let largest = group_values(rows, group).reduce(f64::max)?;
    (group_values(rows, group)
        .filter(|&value| value == largest)
        .count()
        == 1)
        .then_some(largest)
}

fn group_values(rows: &[Row], group: Group) -> impl Iterator<Item = f64> + '_ {
    rows.iter()
        .filter(move |row| row.rule.group == Some(group))
        .map(|row| row.total.value)
}

#[cfg(test)]
mod tests {
    use poe2_domain::{ItemCategory, ModifierInfo, ParsedModifier, ParsedStat, TradeStat};

    use super::*;
    use crate::{SearchProfile, build_filters};

    fn stat(trade_id: &str, text: &str, value: f64) -> ParsedStat {
        ParsedStat {
            stat_id: Some(trade_id.to_owned()),
            text: text.to_owned(),
            value,
            min: value,
            max: value,
            dp: false,
            unscalable: false,
            negated_text: None,
            printed_text: None,
        }
    }

    fn modifier(modifier_type: ModifierType, stat: ParsedStat) -> ParsedModifier {
        ParsedModifier {
            info: ModifierInfo {
                modifier_type,
                generation: None,
                name: None,
                tier: None,
                rank: None,
                tags: Vec::new(),
            },
            stats: vec![stat],
        }
    }

    fn rare(mods: Vec<ParsedModifier>) -> ParsedItem {
        ParsedItem {
            rarity: Some(ItemRarity::Rare),
            mods,
            ..Default::default()
        }
    }

    fn filters_without_catalog(item: &ParsedItem) -> Vec<SearchFilter> {
        build_filters(item, SearchProfile::QuickPrice, &StatCatalog::default())
    }

    fn row<'a>(filters: &'a [SearchFilter], trade_id: &str) -> Option<&'a SearchFilter> {
        filters
            .iter()
            .find(|filter| filter.trade_ids.first().is_some_and(|id| id == trade_id))
    }

    fn value(filters: &[SearchFilter], trade_id: &str) -> f64 {
        row(filters, trade_id)
            .and_then(|filter| filter.roll.as_ref())
            .unwrap_or_else(|| panic!("{trade_id} row with a roll"))
            .value
    }

    #[test]
    fn russian_resistances_fold_into_a_selected_russian_total() {
        let item = rare(vec![
            modifier(
                ModifierType::Explicit,
                stat("explicit.stat_3372524247", "#% к сопротивлению огню", 30.0),
            ),
            modifier(
                ModifierType::Explicit,
                stat(
                    "explicit.stat_4220027924",
                    "#% к сопротивлению холоду",
                    25.0,
                ),
            ),
        ]);
        let russian = StatCatalog {
            stats: vec![TradeStat {
                id: "pseudo.pseudo_total_elemental_resistance".to_owned(),
                text: "Всего +#% сопротивления стихиям".to_owned(),
                mod_type: "pseudo".to_owned(),
            }],
        };

        let filters = build_filters(&item, SearchProfile::QuickPrice, &russian);

        let total = row(&filters, "pseudo.pseudo_total_elemental_resistance")
            .expect("total elemental resistance row");
        assert_eq!(total.tag, FilterTag::Pseudo);
        assert_eq!(total.display_text, "Всего +#% сопротивления стихиям");
        assert_eq!(
            value(&filters, "pseudo.pseudo_total_elemental_resistance"),
            55.0
        );
        assert!(
            !total.enabled && !total.hidden,
            "the total is listed, unselected: its mods score under Quick Price's 3"
        );
        for folded in ["explicit.stat_3372524247", "explicit.stat_4220027924"] {
            let filter = row(&filters, folded).expect("a folded row stays listed");
            assert!(
                !filter.enabled && filter.hidden,
                "{folded} is already in the total"
            );
        }
    }

    #[test]
    fn english_resistances_fold_the_same_way_and_read_in_english_without_a_catalog_entry() {
        let item = rare(vec![
            modifier(
                ModifierType::Explicit,
                stat("explicit.stat_3372524247", "#% to Fire Resistance", 30.0),
            ),
            modifier(
                ModifierType::Explicit,
                stat("explicit.stat_4220027924", "#% to Cold Resistance", 25.0),
            ),
        ]);

        let filters = filters_without_catalog(&item);

        let total = row(&filters, "pseudo.pseudo_total_elemental_resistance")
            .expect("total elemental resistance row");
        assert_eq!(total.display_text, "+#% total Elemental Resistance");
        assert_eq!(
            value(&filters, "pseudo.pseudo_total_elemental_resistance"),
            55.0
        );
        assert!(!total.enabled);
        assert!(
            filters
                .iter()
                .filter(|filter| filter.tag == FilterTag::Explicit)
                .all(|filter| !filter.enabled && filter.hidden)
        );
    }

    #[test]
    fn items_without_folded_stats_get_no_pseudo_rows() {
        let item = rare(vec![modifier(
            ModifierType::Explicit,
            stat("explicit.stat_803737631", "# to Accuracy Rating", 120.0),
        )]);

        let filters = filters_without_catalog(&item);

        assert!(filters.iter().all(|filter| filter.tag != FilterTag::Pseudo));
        let accuracy = row(&filters, "explicit.stat_803737631").expect("accuracy row");
        assert!(!accuracy.hidden);
    }

    #[test]
    fn life_total_needs_a_life_roll_and_counts_strength_twice() {
        let strength = || {
            modifier(
                ModifierType::Explicit,
                stat("explicit.stat_4080418644", "# to Strength", 25.0),
            )
        };
        let life = modifier(
            ModifierType::Implicit,
            stat("implicit.stat_3299347043", "# to maximum Life", 40.0),
        );

        let strength_only = filters_without_catalog(&rare(vec![strength()]));
        assert!(row(&strength_only, "pseudo.pseudo_total_life").is_none());

        let filters = filters_without_catalog(&rare(vec![strength(), life]));
        assert_eq!(value(&filters, "pseudo.pseudo_total_life"), 90.0);
        assert!(row(&filters, "pseudo.pseudo_total_life").is_some_and(|life| !life.hidden));
    }

    #[test]
    fn resistances_count_per_element_and_a_strictly_strongest_element_total_stays_hidden() {
        let mods = vec![
            modifier(
                ModifierType::Explicit,
                stat(
                    "explicit.stat_2901986750",
                    "#% to all Elemental Resistances",
                    10.0,
                ),
            ),
            modifier(
                ModifierType::Explicit,
                stat(
                    "explicit.stat_2915988346",
                    "#% to Fire and Cold Resistances",
                    20.0,
                ),
            ),
            modifier(
                ModifierType::Augment,
                stat("rune.stat_1671376347", "#% to Lightning Resistance", 15.0),
            ),
            modifier(
                ModifierType::Explicit,
                stat("explicit.stat_3372524247", "#% to Fire Resistance", 5.0),
            ),
        ];

        let filters = filters_without_catalog(&rare(mods.clone()));

        // 10 x 3 elements + 20 x 2 + 15 + 5.
        assert_eq!(
            value(&filters, "pseudo.pseudo_total_elemental_resistance"),
            90.0
        );
        // The all-element 10 plus the weakest element's own 15 (lightning).
        assert_eq!(
            value(&filters, "pseudo.pseudo_total_all_elemental_resistances"),
            25.0
        );
        // Fire 35 beats cold 30 and lightning 25.
        assert_eq!(value(&filters, "pseudo.pseudo_total_fire_resistance"), 35.0);
        assert!(row(&filters, "pseudo.pseudo_total_cold_resistance").is_none());
        assert!(row(&filters, "pseudo.pseudo_total_lightning_resistance").is_none());
        let hidden = |id: &str| row(&filters, id).expect("pseudo row").hidden;
        assert!(!hidden("pseudo.pseudo_total_elemental_resistance"));
        assert!(hidden("pseudo.pseudo_total_all_elemental_resistances"));
        assert!(hidden("pseudo.pseudo_total_fire_resistance"));

        // Without the lone fire roll, fire and cold tie at 30: no single-element total at all.
        let tied = filters_without_catalog(&rare(mods[..3].to_vec()));
        for element in ["fire", "cold", "lightning"] {
            let id = format!("pseudo.pseudo_total_{element}_resistance");
            assert!(row(&tied, &id).is_none(), "{id} on a tie");
        }
    }

    #[test]
    fn an_even_attribute_spread_reads_as_all_attributes_and_an_uneven_one_per_attribute() {
        let all = || {
            modifier(
                ModifierType::Explicit,
                stat("explicit.stat_1379411836", "# to all Attributes", 10.0),
            )
        };

        let even = filters_without_catalog(&rare(vec![all()]));
        assert_eq!(value(&even, "pseudo.pseudo_total_all_attributes"), 10.0);
        assert!(row(&even, "pseudo.pseudo_total_strength").is_none());

        let uneven = filters_without_catalog(&rare(vec![
            all(),
            modifier(
                ModifierType::Explicit,
                stat(
                    "explicit.stat_1535626285",
                    "# to Strength and Intelligence",
                    5.0,
                ),
            ),
        ]));
        assert!(row(&uneven, "pseudo.pseudo_total_all_attributes").is_none());
        assert_eq!(value(&uneven, "pseudo.pseudo_total_strength"), 15.0);
        assert_eq!(value(&uneven, "pseudo.pseudo_total_dexterity"), 10.0);
        assert_eq!(value(&uneven, "pseudo.pseudo_total_intelligence"), 15.0);
    }

    #[test]
    fn a_minor_third_attribute_is_hidden_and_an_equal_second_with_it() {
        let intelligence = modifier(
            ModifierType::Explicit,
            stat("explicit.stat_328541901", "# to Intelligence", 30.0),
        );
        let all = modifier(
            ModifierType::Explicit,
            stat("explicit.stat_1379411836", "# to all Attributes", 5.0),
        );
        let hidden = |filters: &[SearchFilter], attribute: &str| {
            row(filters, &format!("pseudo.pseudo_total_{attribute}"))
                .expect("attribute total")
                .hidden
        };

        // Intelligence 35, the last rule, ranks first; strength and dexterity tie at 5, under 30%
        // of it.
        let tied = filters_without_catalog(&rare(vec![intelligence.clone(), all.clone()]));
        assert!(!hidden(&tied, "intelligence"));
        assert!(hidden(&tied, "strength") && hidden(&tied, "dexterity"));

        // Strength 8 now outranks dexterity 5, which alone stays hidden.
        let strength = modifier(
            ModifierType::Explicit,
            stat("explicit.stat_4080418644", "# to Strength", 3.0),
        );
        let ranked = filters_without_catalog(&rare(vec![intelligence, all, strength]));
        assert!(!hidden(&ranked, "strength"));
        assert!(hidden(&ranked, "dexterity"));
    }

    #[test]
    fn chaos_total_is_hidden_when_a_lone_rune_grants_it() {
        let chaos = |modifier_type, trade_id: &str| {
            rare(vec![modifier(
                modifier_type,
                stat(trade_id, "#% to Chaos Resistance", 7.0),
            )])
        };
        // (selected, hidden) -- a total's mods scoring under 3 leave it unselected; a rune-only
        // chaos total is also folded away, as EE2 hides it.
        let state = |item: &ParsedItem| {
            let filters = filters_without_catalog(item);
            let total =
                row(&filters, "pseudo.pseudo_total_chaos_resistance").expect("chaos total row");
            (total.enabled, total.hidden)
        };

        assert_eq!(
            state(&chaos(ModifierType::Augment, "rune.stat_2923486259")),
            (false, true)
        );
        assert_eq!(
            state(&chaos(ModifierType::Explicit, "explicit.stat_2923486259")),
            (false, false)
        );
    }

    #[test]
    fn exempt_items_keep_their_own_rows_instead_of_totals() {
        let item = |rarity, category: &str| ParsedItem {
            rarity: Some(rarity),
            category: Some(ItemCategory {
                id: category.to_owned(),
                display_name: String::new(),
            }),
            mods: vec![modifier(
                ModifierType::Explicit,
                stat("explicit.stat_3372524247", "#% to Fire Resistance", 30.0),
            )],
            ..Default::default()
        };

        // A unique taking runes, and a Normal item (EE2's exact preset).
        for exempt in [
            item(ItemRarity::Unique, "armour.chest"),
            item(ItemRarity::Normal, "accessory.ring"),
        ] {
            let filters = filters_without_catalog(&exempt);
            assert!(filters.iter().all(|filter| filter.tag != FilterTag::Pseudo));
            let fire = row(&filters, "explicit.stat_3372524247").expect("fire resistance row");
            assert!(!fire.hidden, "no total hides it");
        }

        let unique_ring = filters_without_catalog(&item(ItemRarity::Unique, "accessory.ring"));
        assert!(row(&unique_ring, "pseudo.pseudo_total_elemental_resistance").is_some());
    }

    #[test]
    fn energy_shield_a_property_row_counts_feeds_no_total() {
        let item = |category: &str, energy_shield| ParsedItem {
            rarity: Some(ItemRarity::Rare),
            category: Some(ItemCategory {
                id: category.to_owned(),
                display_name: String::new(),
            }),
            energy_shield,
            mods: vec![modifier(
                ModifierType::Explicit,
                stat(
                    "explicit.stat_3489782002",
                    "# to maximum Energy Shield",
                    40.0,
                ),
            )],
            ..Default::default()
        };

        let chest = filters_without_catalog(&item("armour.chest", Some(160)));
        assert!(row(&chest, "pseudo.pseudo_total_energy_shield").is_none());

        let ring = filters_without_catalog(&item("accessory.ring", None));
        assert_eq!(value(&ring, "pseudo.pseudo_total_energy_shield"), 40.0);
    }
}
