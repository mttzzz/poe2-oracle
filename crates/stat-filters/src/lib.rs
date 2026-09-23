//! Search-filter construction from a parsed item: `poe2_domain::ParsedItem` -> `Vec<SearchFilter>`
//! ready for `trade-client::search_with_filters`. Ports the per-mod aggregation shape of the
//! real, working reference's `calculatedStatToFilter`
//! (`exiled-exchange-2/renderer/src/web/price-check/filters/create-stat-filters.ts:352-470`) and
//! its `pseudo/index.ts` summed pseudo-stat rules (see the `pseudo` module) -- cited as the
//! aggregation algorithm's ground truth, never as code to copy. Pure logic, no I/O: this crate
//! depends on nothing but `poe2-domain` by design (see `Cargo.toml`).
//!
//! Property rows (the `property` module) search the trade query's own item filters -- defences,
//! DPS, item level, sockets, quality -- rather than a stat: their single trade id names that
//! filter as `<group>.<key>`.

mod better;
mod property;
mod pseudo;

use poe2_domain::{
    ItemRarity, ModGeneration, ModifierInfo, ModifierType, ParsedItem, ParsedStat, StatCatalog,
};
use property::property_filters;
pub use property::uses_exact_preset;

/// Where a `SearchFilter` came from -- drives the reference screenshot's tag pill, and, for
/// `Explicit`/`Fractured`/`Desecrated`, the default-enabled heuristic in `per_mod_filters`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterTag {
    Pseudo,
    Explicit,
    Implicit,
    Crafted,
    Enchant,
    Fractured,
    Rune,
    Desecrated,
    Property,
    /// Free prefix or suffix slots (`SearchFilter::generation` says which): EE2's
    /// `item.has_empty_modifier` row, split per slot kind.
    EmptyAffix,
}

/// The roll band for one `SearchFilter`. `min`/`max` are the CURRENT editable search bounds (what
/// a UI numeric input would bind to; `None` leaves that side of the search open);
/// `default_min`/`default_max` are the band's edges, computed from `value` and the caller's
/// `search_percent` tolerance band -- `min` starts at `default_min`, `max` starts open (see
/// `build_roll`). Kept distinct from `min`/`max` so a UI can offer a "reset to default"
/// affordance later without recomputing.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchFilterRoll {
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub default_min: f64,
    pub default_max: f64,
    pub dp: bool,
}

/// One row of a Price Check filter panel: a stat aggregated from every contributing mod on the
/// item, a pseudo total, one of the item's own properties (`FilterTag::Property`, whose trade id
/// names a trade query filter rather than a stat -- see the `property` module), or its free affix
/// slots of one kind (`FilterTag::EmptyAffix`).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchFilter {
    pub trade_ids: Vec<String>,
    pub stat_ref: String,
    pub display_text: String,
    pub tag: FilterTag,
    /// Minimum tier across every contributing mod source that has one (`ModifierInfo.tier`);
    /// `None` if no source carries a tier (a Unique item's fixed mods never do; a rune-granted
    /// stat never does). Never looked up against a local tier table -- see the workspace Price
    /// Check plan's Context section for why.
    pub tier: Option<u32>,
    pub roll: Option<SearchFilterRoll>,
    /// `true` = checkbox checked / included in the search -- the NON-inverted sense, deliberately
    /// opposite of the reference's own confusingly-named `StatFilter.disabled`.
    pub enabled: bool,
    /// `true` = listed only behind the panel's "show hidden" toggle, where EE2 hides the row
    /// (its `StatFilter.hidden`): a stat a pseudo total already counts, a total restating
    /// another, a minor share of a weapon's DPS. A hidden row is never `enabled`. EE2's toggle
    /// swaps the list to the hidden rows alone and only shows while some row is hidden
    /// (`FiltersBlock.vue`'s `filteredStats`).
    pub hidden: bool,
    /// The affix slot behind the row, for grouping it the way the game lists mods: the prefix or
    /// suffix every source mod of a per-mod row occupies (`None` when they differ, and for
    /// implicits, runes and enchants), or the kind of free slot an `EmptyAffix` row counts.
    /// `None` on every other row.
    pub generation: Option<ModGeneration>,
    /// The row reads in the item's own words, the other way round from the catalog's
    /// (`word_like_the_item`): `display_text` is the item's, and `roll` is in its terms -- a
    /// search negates the bounds back and swaps them (EE2's `tradeInvert`).
    pub inverted: bool,
}

impl SearchFilter {
    /// Whether a trade search can use the row at all: it has a trade id -- a line the catalog
    /// doesn't know has none -- and a roll, unless it's a flag stat, which a listing matches by
    /// having it. A property is always a range.
    pub fn searchable(&self) -> bool {
        !self.trade_ids.is_empty() && (self.roll.is_some() || self.tag != FilterTag::Property)
    }
}

/// Builds every `SearchFilter` row for `item`, in EE2's order (`initUiModFilters`,
/// `create-stat-filters.ts:222-297`): one per non-`None` base-item property (`property_filters`),
/// then the pseudo totals (`pseudo::pseudo_filters`), then one per distinct trade-searchable stat
/// (grouped across mods sharing a stat id and a compatible `ModifierType`, see
/// `per_mod_filters`), then the free affix slots (`empty_affix_filters`, last like EE2's
/// `finalFilterTweaks` row) -- with `select_by_tier`'s default selection on top.
/// `search_percent` is the +/- tolerance band (0-50) applied around each filter's own rolled
/// value, matching the reference's own default of 10 (`PriceCheckWindow.vue:197`/
/// `DevWidget.vue:180`). `catalog` is the searched site's stat catalog: pseudo and free-slot rows
/// read in its language.
pub fn build_filters(
    item: &ParsedItem,
    search_percent: u8,
    catalog: &StatCatalog,
) -> Vec<SearchFilter> {
    let mut filters = property_filters(item, search_percent);
    let mut mod_filters = per_mod_filters(item, search_percent, catalog);
    filters.extend(pseudo::pseudo_filters(
        item,
        search_percent,
        catalog,
        &mut mod_filters,
    ));
    filters.extend(mod_filters);
    filters.extend(empty_affix_filters(item, catalog));
    select_by_tier(&mut filters);
    settle_exact_kinds(item, catalog, &mut filters);
    word_like_the_item(item, &mut filters);
    filters
}

/// Turns each mod row the item words the other way round from the catalog -- `15% reduced
/// Attribute Requirements`, which the parser read as the catalog's `#% increased Attribute
/// Requirements` at -15 (`ParsedStat::negated_text`) -- into the item's own words: its text, the
/// value and bounds negated and swapped into its terms ("at least 15% reduced"), and `inverted`
/// set so the search turns them back: EE2's `filterAdjustmentForNegate`
/// (`create-stat-filters.ts:615-631`). Only a row that still totals below zero turns: a reduced
/// roll that an increased one outweighs reads the catalog's way. Runs last, once every rule
/// above has worked in the catalog's terms.
fn word_like_the_item(item: &ParsedItem, filters: &mut [SearchFilter]) {
    for filter in filters.iter_mut() {
        if matches!(
            filter.tag,
            FilterTag::Property | FilterTag::Pseudo | FilterTag::EmptyAffix
        ) {
            continue;
        }
        let Some(roll) = &mut filter.roll else {
            continue;
        };
        let Some(stat_id) = filter.trade_ids.first() else {
            continue;
        };
        if roll.value >= 0.0 {
            continue;
        }
        let Some(text) = item
            .mods
            .iter()
            .flat_map(|modifier| &modifier.stats)
            .find(|stat| stat.stat_id.as_ref() == Some(stat_id))
            .and_then(|stat| stat.negated_text.clone())
        else {
            continue;
        };
        *roll = SearchFilterRoll {
            value: -roll.value,
            min: roll.max.map(|max| -max),
            max: roll.min.map(|min| -min),
            default_min: -roll.default_max,
            default_max: -roll.default_min,
            dp: roll.dp,
        };
        filter.display_text = text;
        filter.inverted = true;
    }
}

/// Trade hashes of every tablet implicit, `Adds <league> to a Map \n# use remaining`
/// (`implicit.*` in the live catalog, 2026-09-22): the inputs of EE2's `# uses remaining` pseudo
/// rule (`pseudo/index.ts:354-366`).
const TABLET_USES: [&str; 8] = [
    "stat_4041853756",
    "stat_3879011313",
    "stat_2219129443",
    "stat_3166002380",
    "stat_3376302538",
    "stat_2369421690",
    "stat_1714888636",
    "stat_3035440454",
];

/// EE2's selection for the item kinds its exact preset searches whole
/// (`createExactStatFilters`, `create-stat-filters.ts:38-220`), applied over `select_by_tier`:
/// a tablet searches its `# uses remaining (Tablets)` pseudo total (at least as many uses, shown
/// on uniques too) and every mod at exactly its roll, leaving its implicit out; a non-unique relic
/// searches every mod (`enableAllFilters`). A waystone follows EE2's map rule
/// (`finalFilterTweaks`, `create-stat-filters.ts:718-726`): its tier and properties set the
/// price, its modifiers only make the map harder -- they start unselected, a desecrated one
/// excepted. Unlike EE2 they stay listed: the player marks them there.
fn settle_exact_kinds(item: &ParsedItem, catalog: &StatCatalog, filters: &mut Vec<SearchFilter>) {
    let category = item.category.as_ref().map_or("", |c| c.id.as_str());
    let unique = item.rarity == Some(ItemRarity::Unique);
    if category == "map.tablet" {
        let uses = item
            .mods
            .iter()
            .filter(|m| m.info.modifier_type == ModifierType::Implicit)
            .flat_map(|m| &m.stats)
            .find(|stat| {
                stat.stat_id
                    .as_deref()
                    .and_then(|id| id.split_once('.'))
                    .is_some_and(|(_, hash)| TABLET_USES.contains(&hash))
            })
            .map(|stat| stat.value);
        if !unique {
            for filter in filters.iter_mut() {
                match filter.tag {
                    FilterTag::Property | FilterTag::EmptyAffix | FilterTag::Pseudo => {}
                    FilterTag::Implicit => {
                        filter.enabled = false;
                        filter.hidden = true;
                    }
                    _ => {
                        filter.enabled = true;
                        if let Some(roll) = &mut filter.roll {
                            roll.min = Some(roll.value);
                            roll.default_min = roll.value;
                        }
                    }
                }
            }
        }
        if let Some(uses) = uses {
            let id = "pseudo.pseudo_number_of_uses_remaining";
            let at = filters
                .iter()
                .position(|filter| filter.tag != FilterTag::Property)
                .unwrap_or(filters.len());
            filters.insert(
                at,
                SearchFilter {
                    trade_ids: vec![id.to_owned()],
                    stat_ref: "# uses remaining (Tablets)".to_owned(),
                    display_text: catalog_text(catalog, id, "# uses remaining (Tablets)")
                        .to_owned(),
                    tag: FilterTag::Pseudo,
                    tier: None,
                    roll: Some(SearchFilterRoll {
                        value: uses,
                        min: Some(uses),
                        max: None,
                        default_min: uses,
                        default_max: uses,
                        dp: false,
                    }),
                    enabled: true,
                    hidden: false,
                    generation: None,
                    inverted: false,
                },
            );
        }
    } else if category == "sanctum.relic" && !unique {
        for filter in filters.iter_mut() {
            if !matches!(
                filter.tag,
                FilterTag::Property | FilterTag::EmptyAffix | FilterTag::Pseudo
            ) {
                filter.enabled = true;
            }
        }
    } else if category == "map.waystone" {
        for filter in filters.iter_mut() {
            if !matches!(filter.tag, FilterTag::Property | FilterTag::Desecrated) {
                filter.enabled = false;
            }
        }
    }
}

/// The best tier number still searched by default (T1 is a mod's best tier).
const TOP_TIERS: u32 = 2;

/// The default selection a player expects, by the game's own measure of a mod: an explicit
/// prefix or suffix is searched when it rolled in one of its `TOP_TIERS` best tiers and left out
/// otherwise -- a low-tier mod doesn't set the price, and requiring it only empties the search.
/// Pseudo totals start unselected: they restate the mods listed with them -- unless no affix made
/// the cut, when the totals (EE2's own default) are what the item offers: without them the search
/// would price the bare base type. Everything else keeps the EE2 default it was built with
/// (defences/DPS on, implicits and free slots off). Replaces EE2's pseudo-first selection, which
/// searched a T9 roll while skipping the T1 ones a pseudo total happened to cover.
fn select_by_tier(filters: &mut [SearchFilter]) {
    let is_affix =
        |filter: &SearchFilter| filter.generation.is_some() && filter.tag != FilterTag::EmptyAffix;
    for filter in filters.iter_mut() {
        if filter.tag == FilterTag::Pseudo {
            filter.enabled = false;
        } else if is_affix(filter) {
            filter.enabled = filter.tier.is_some_and(|tier| tier <= TOP_TIERS);
        }
    }
    let has_affixes = filters.iter().any(is_affix);
    let affix_selected = filters
        .iter()
        .any(|filter| is_affix(filter) && filter.enabled);
    if has_affixes && !affix_selected {
        for filter in filters
            .iter_mut()
            .filter(|filter| filter.tag == FilterTag::Pseudo && !filter.hidden)
        {
            filter.enabled = true;
        }
    }
}

/// Two `ModifierType`s land in the same aggregation group when they're equal, or when both are
/// members of the "explicit-like" set the reference calls `EXPLICIT_MOD_TYPES`/
/// `typesCanBeGrouped()` (`modifiers.ts:209-216`) -- a Fractured/Desecrated/Crafted/Veiled/
/// Sanctum copy of an Explicit stat still merges into the same filter row. Every other type
/// (Implicit, Enchant, Scourge, Necropolis, Augment/AddedAugment, Skill) only merges with itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupBucket {
    ExplicitLike,
    Solo(ModifierType),
}

fn group_bucket(modifier_type: ModifierType) -> GroupBucket {
    match modifier_type {
        ModifierType::Explicit
        | ModifierType::Fractured
        | ModifierType::Veiled
        | ModifierType::Desecrated
        | ModifierType::Crafted
        | ModifierType::Sanctum => GroupBucket::ExplicitLike,
        other => GroupBucket::Solo(other),
    }
}

/// Maps one contributing `ModifierType` to the `FilterTag` it would produce alone. `Scourge`/
/// `Necropolis`/`Skill` have no dedicated tag in this pass -- they fall back to `Explicit` as a
/// documented simplification. `Pseudo` never actually appears on a real `ParsedItem.mods` entry
/// (only the `pseudo` module ever produces it); handled here only so this match stays
/// exhaustive.
fn filter_tag_for(modifier_type: ModifierType) -> FilterTag {
    match modifier_type {
        ModifierType::Fractured => FilterTag::Fractured,
        ModifierType::Desecrated => FilterTag::Desecrated,
        ModifierType::Explicit
        | ModifierType::Veiled
        | ModifierType::Crafted
        | ModifierType::Sanctum
        | ModifierType::Scourge
        | ModifierType::Necropolis
        | ModifierType::Skill => FilterTag::Explicit,
        ModifierType::Implicit => FilterTag::Implicit,
        ModifierType::Enchant => FilterTag::Enchant,
        ModifierType::Augment | ModifierType::AddedAugment => FilterTag::Rune,
        ModifierType::Pseudo => FilterTag::Pseudo,
    }
}

/// The tag for a whole aggregation group, from every distinct `ModifierType` that contributed a
/// stat to it. `Fractured` takes priority over `Desecrated`, which takes priority over the plain
/// `Explicit`-like remainder -- `contributors` is never empty by construction (a group only
/// exists once at least one stat has been added to it).
fn group_tag(contributors: &[ModifierType]) -> FilterTag {
    debug_assert!(!contributors.is_empty());
    if contributors.contains(&ModifierType::Fractured) {
        FilterTag::Fractured
    } else if contributors.contains(&ModifierType::Desecrated) {
        FilterTag::Desecrated
    } else {
        filter_tag_for(contributors[0])
    }
}

/// Accumulates every `ParsedStat` sharing one trade stat id and `GroupBucket` into the totals
/// `into_filter` turns into one `SearchFilter` row -- the Rust equivalent of the reference's
/// `calculatedStatToFilter` grouping step (`create-stat-filters.ts:352`).
struct ModStatGroup {
    stat_id: String,
    bucket: GroupBucket,
    text: String,
    /// The first stat's own wording where an EE2 printed form gave it
    /// (`ParsedStat::printed_text`).
    printed: Option<String>,
    /// How many stats the row totals.
    sources: usize,
    value: f64,
    min: f64,
    max: f64,
    dp: bool,
    any_scalable: bool,
    modifier_types: Vec<ModifierType>,
    tier: Option<u32>,
    /// The affix slot every contributing mod shares; `None` once two disagree.
    generation: Option<ModGeneration>,
}

impl ModStatGroup {
    fn new(
        stat_id: String,
        bucket: GroupBucket,
        stat: &ParsedStat,
        generation: Option<ModGeneration>,
    ) -> Self {
        Self {
            stat_id,
            bucket,
            text: stat.text.clone(),
            printed: stat.printed_text.clone(),
            sources: 0,
            value: 0.0,
            min: 0.0,
            max: 0.0,
            dp: false,
            any_scalable: false,
            modifier_types: Vec::new(),
            tier: None,
            generation,
        }
    }

    fn add(&mut self, info: &ModifierInfo, stat: &ParsedStat) {
        self.sources += 1;
        self.value += stat.value;
        self.min += stat.min;
        self.max += stat.max;
        self.dp |= stat.dp;
        self.any_scalable |= !stat.unscalable;
        if !self.modifier_types.contains(&info.modifier_type) {
            self.modifier_types.push(info.modifier_type);
        }
        self.tier = match (self.tier, info.tier) {
            (Some(a), Some(b)) => Some(a.min(b)),
            _ => self.tier.or(info.tier),
        };
        if self.generation != info.generation {
            self.generation = None;
        }
    }

    fn into_filter(self, search_percent: u8, catalog: &StatCatalog) -> SearchFilter {
        let tag = group_tag(&self.modifier_types);
        let bounds = Some((self.min, self.max));
        let roll = self.any_scalable.then(|| {
            let mut roll = build_roll(self.value, bounds, self.dp, search_percent);
            better::orient(&mut roll, &self.stat_id);
            roll
        });
        // A flag stat (no roll) starts unchecked: a unique's fixed flags add nothing to its
        // search. An affix's own tier decides for it afterwards (`select_by_tier`).
        let enabled = roll.is_some()
            && matches!(
                tag,
                FilterTag::Explicit | FilterTag::Fractured | FilterTag::Desecrated
            );
        // The item's own words when one line it prints another way than the catalog is the whole
        // row (`40% шанс наложения оцепенения...` for `Накладывает оцепенение...`); a total of
        // several reads the catalog's way.
        let display_text = match self.printed {
            Some(printed) if self.sources == 1 => printed,
            _ => self.text.clone(),
        };
        SearchFilter {
            trade_ids: same_text_ids(catalog, &self.stat_id),
            stat_ref: self.text,
            display_text,
            tag,
            tier: self.tier,
            roll,
            enabled,
            hidden: false,
            generation: self.generation,
            inverted: false,
        }
    }
}

/// Groups every stat-id-bearing `ParsedStat` across `item.mods` (see `group_bucket`) into one
/// `SearchFilter` per group, in first-seen order. Stats with `stat_id: None` are skipped --
/// unmatched against the trade catalog, they already surface via `item.unknown_mods` for display
/// elsewhere, not this crate's concern -- and so are stats a property row already counts
/// (`property::folded_into_properties`). A row carries every trade id its text has
/// (`same_text_ids`).
fn per_mod_filters(
    item: &ParsedItem,
    search_percent: u8,
    catalog: &StatCatalog,
) -> Vec<SearchFilter> {
    let mut groups: Vec<ModStatGroup> = Vec::new();
    for modifier in &item.mods {
        let bucket = group_bucket(modifier.info.modifier_type);
        for stat in &modifier.stats {
            let Some(stat_id) = stat.stat_id.as_deref() else {
                continue;
            };
            if property::folded_into_properties(item, stat_id) {
                continue;
            }
            let idx = groups
                .iter()
                .position(|g| g.bucket == bucket && g.stat_id == stat_id)
                .unwrap_or_else(|| {
                    groups.push(ModStatGroup::new(
                        stat_id.to_owned(),
                        bucket,
                        stat,
                        modifier.info.generation,
                    ));
                    groups.len() - 1
                });
            groups[idx].add(&modifier.info, stat);
        }
    }
    groups
        .into_iter()
        .map(|group| group.into_filter(search_percent, catalog))
        .collect()
}

/// The trade site's pseudo stats counting free affix slots, with their English templates (EE2's
/// `TOTAL_MODS_TEXT.EMPTY_MODIFIERS`, `pathofexile-trade.ts:89-94`); both ids and texts were in
/// the live EN and RU catalogs on 2026-09-22.
const EMPTY_AFFIX_STATS: [(ModGeneration, &str, &str); 2] = [
    (
        ModGeneration::Prefix,
        "pseudo.pseudo_number_of_empty_prefix_mods",
        "# Empty Prefix Modifiers",
    ),
    (
        ModGeneration::Suffix,
        "pseudo.pseudo_number_of_empty_suffix_mods",
        "# Empty Suffix Modifiers",
    ),
];

/// The hashes of `# Prefix Modifier allowed` and `# Suffix Modifier allowed`, stats that move an
/// item's slot maxima (EE2's `itemMaxModifiersBySlot`; ids from its `stats.ndjson`).
const PREFIXES_ALLOWED: &str = "stat_3182714256";
const SUFFIXES_ALLOWED: &str = "stat_718638445";

/// One row per slot kind with a free slot, searching listings with at least that many free
/// (EE2's `showHasEmptyModifier`, `create-stat-filters.ts:845-887`, which offers the same counts
/// as one row with an any/prefix/suffix switch): what a buyer who means to craft looks for. Only
/// on a Magic or Rare item that can still be crafted (`property::is_modifiable`, EE2's
/// `itemIsModifiable`) and has at least one explicit mod, so an unidentified item gets none.
fn empty_affix_filters(item: &ParsedItem, catalog: &StatCatalog) -> Vec<SearchFilter> {
    let category = item
        .category
        .as_ref()
        .map_or("", |category| category.id.as_str());
    // EE2's `itemBaseMaxModifiersOfType`: the slots of each kind a base allows at this rarity.
    let base = match item.rarity {
        Some(ItemRarity::Magic) => 1.0,
        Some(ItemRarity::Rare) if matches!(category, "jewel" | "map.tablet" | "sanctum.relic") => {
            2.0
        }
        Some(ItemRarity::Rare) => 3.0,
        _ => return Vec::new(),
    };
    if !property::is_modifiable(item) {
        return Vec::new();
    }
    // EE2's `explicitModifierCount`: every explicit-like mod (`EXPLICIT_MOD_TYPES`) in the slot.
    let occupied = |generation| {
        item.mods
            .iter()
            .filter(|modifier| {
                group_bucket(modifier.info.modifier_type) == GroupBucket::ExplicitLike
                    && modifier.info.generation == Some(generation)
            })
            .count() as f64
    };
    if occupied(ModGeneration::Prefix) + occupied(ModGeneration::Suffix) == 0.0 {
        return Vec::new();
    }
    let allowed = |hash: &str| -> f64 {
        item.mods
            .iter()
            .flat_map(|modifier| &modifier.stats)
            .filter(|stat| {
                stat.stat_id
                    .as_deref()
                    .and_then(|id| id.split_once('.'))
                    .is_some_and(|(_, stat_hash)| stat_hash == hash)
            })
            .map(|stat| stat.value)
            .sum()
    };
    EMPTY_AFFIX_STATS
        .iter()
        .filter_map(|&(generation, trade_id, english)| {
            let extra = allowed(match generation {
                ModGeneration::Prefix => PREFIXES_ALLOWED,
                ModGeneration::Suffix => SUFFIXES_ALLOWED,
            });
            let free = (base + extra).max(0.0) - occupied(generation);
            (free > 0.0).then(|| SearchFilter {
                trade_ids: vec![trade_id.to_owned()],
                stat_ref: english.to_owned(),
                display_text: catalog_text(catalog, trade_id, english).to_owned(),
                tag: FilterTag::EmptyAffix,
                tier: None,
                roll: Some(SearchFilterRoll {
                    value: free,
                    min: Some(free),
                    max: None,
                    default_min: free,
                    default_max: free,
                    dp: false,
                }),
                enabled: false,
                hidden: false,
                generation: Some(generation),
                inverted: false,
            })
        })
        .collect()
}

/// `trade_id`'s template in `catalog`'s language, else `english`.
pub(crate) fn catalog_text<'a>(
    catalog: &'a StatCatalog,
    trade_id: &str,
    english: &'a str,
) -> &'a str {
    catalog
        .stats
        .iter()
        .find(|stat| stat.id == trade_id)
        .map_or(english, |stat| stat.text.as_str())
}

/// `trade_id`, then every other stat `catalog` prints the same way under the same mod type, in
/// catalog order. The parser matches a text several ids share (`# to all Attributes` is both
/// `explicit.stat_1379411836` and `explicit.stat_2897413282`; the live EN catalog had 40 such
/// texts on 2026-09-23, `# to Spirit` among them) to just one of them, while the trade site files
/// a listing's mod under any: EE2 keeps them all (`stats.ndjson`'s `trade.ids`) and searches for
/// either (`pathofexile-trade.ts:1215-1224`). The parser's own id stays first, the one every
/// single-id reader (`trade_ids.first()`) keeps using.
fn same_text_ids(catalog: &StatCatalog, trade_id: &str) -> Vec<String> {
    let mut ids = vec![trade_id.to_owned()];
    let Some(own) = catalog.stats.iter().find(|stat| stat.id == trade_id) else {
        return ids;
    };
    let mod_type = trade_id.split_once('.').map(|(mod_type, _)| mod_type);
    ids.extend(
        catalog
            .stats
            .iter()
            .filter(|stat| {
                stat.id != trade_id
                    && stat.text == own.text
                    && stat.id.split_once('.').map(|(kind, _)| kind) == mod_type
            })
            .map(|stat| stat.id.clone()),
    );
    ids
}

/// Computes `value`'s +/- `search_percent`% tolerance band, clamped into `natural_bounds` (the
/// summed stat's own `[min, max]`) when one is available, and rounded to whole numbers when `dp`
/// is `false`. Shared by `per_mod_filters`, `property_filters`, and `pseudo::pseudo_filters` so
/// every filter kind computes `default_min`/`default_max` identically.
///
/// Only the lower bound is preset as a search bound: `max` starts `None`, an open-ended search,
/// because EE2's `filterFillMinMax` (`create-stat-filters.ts:597-613`) presets only `roll.min`
/// for a positive stat -- a higher roll is never a reason to exclude a listing. A mod row whose
/// stat is better lower, or compares to nothing, is turned around afterwards
/// (`better::orient`). `default_max` is still computed: it is the upper bound a UI offers once
/// the player opts into one.
pub(crate) fn build_roll(
    value: f64,
    natural_bounds: Option<(f64, f64)>,
    dp: bool,
    search_percent: u8,
) -> SearchFilterRoll {
    // `abs`: a drawback stat's value is negative, and a negative band would put `lo` above `hi`
    // -- a floor above the item's own roll that excludes the item itself.
    let band = value.abs() * f64::from(search_percent) / 100.0;
    let mut lo = value - band;
    let mut hi = value + band;
    if let Some((a, b)) = natural_bounds {
        let (real_lo, real_hi) = if a <= b { (a, b) } else { (b, a) };
        lo = lo.clamp(real_lo, real_hi);
        hi = hi.clamp(real_lo, real_hi);
    }
    if !dp {
        lo = lo.floor();
        hi = hi.ceil();
    }
    SearchFilterRoll {
        value,
        min: Some(lo),
        max: None,
        default_min: lo,
        default_max: hi,
        dp,
    }
}

#[cfg(test)]
mod tests {
    use poe2_domain::ModGeneration::{Prefix, Suffix};
    use poe2_domain::{ItemCategory, ParsedModifier, TradeStat, WaystoneProperties};

    use super::*;

    fn stat_with_id(stat_id: Option<&str>, text: &str, value: f64) -> ParsedStat {
        ParsedStat {
            stat_id: stat_id.map(str::to_owned),
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

    fn modifier(
        modifier_type: ModifierType,
        tier: Option<u32>,
        stats: Vec<ParsedStat>,
    ) -> ParsedModifier {
        ParsedModifier {
            info: ModifierInfo {
                modifier_type,
                generation: None,
                name: None,
                tier,
                rank: None,
                tags: Vec::new(),
            },
            stats,
        }
    }

    /// An explicit prefix or suffix granting `stat`.
    fn affix(generation: ModGeneration, stat: ParsedStat) -> ParsedModifier {
        let mut affix = modifier(ModifierType::Explicit, Some(1), vec![stat]);
        affix.info.generation = Some(generation);
        affix
    }

    fn item(rarity: ItemRarity, category: &str, mods: Vec<ParsedModifier>) -> ParsedItem {
        ParsedItem {
            rarity: Some(rarity),
            category: Some(ItemCategory {
                id: category.to_owned(),
                display_name: String::new(),
            }),
            mods,
            ..Default::default()
        }
    }

    /// The free slot rows' kinds and counts.
    fn free_slots(item: &ParsedItem) -> Vec<(Option<ModGeneration>, f64)> {
        build_filters(item, 10, &StatCatalog::default())
            .into_iter()
            .filter(|filter| filter.tag == FilterTag::EmptyAffix)
            .map(|filter| (filter.generation, filter.roll.expect("roll").value))
            .collect()
    }

    #[test]
    fn a_full_magic_item_has_no_free_slot_rows_and_a_missing_suffix_shows_as_one() {
        // The live Russian magic boots fixture: a life prefix and a life regeneration suffix.
        let mut boots = item(
            ItemRarity::Magic,
            "armour.boots",
            vec![
                affix(
                    Prefix,
                    stat_with_id(
                        Some("explicit.stat_3299347043"),
                        "# к максимуму здоровья",
                        29.0,
                    ),
                ),
                affix(
                    Suffix,
                    stat_with_id(
                        Some("explicit.stat_3325883026"),
                        "Регенерация # здоровья в секунду",
                        10.4,
                    ),
                ),
            ],
        );
        assert!(free_slots(&boots).is_empty());

        boots.mods.pop();
        assert_eq!(free_slots(&boots), [(Some(Suffix), 1.0)]);
    }

    #[test]
    fn a_rare_missing_a_prefix_gets_one_prefix_row_and_every_affix_row_its_slot() {
        let explicit = |generation, trade_id: &str, text: &str, value| {
            affix(generation, stat_with_id(Some(trade_id), text, value))
        };
        let ring = item(
            ItemRarity::Rare,
            "accessory.ring",
            vec![
                modifier(
                    ModifierType::Implicit,
                    None,
                    vec![stat_with_id(
                        Some("implicit.stat_2901986750"),
                        "#% to all Elemental Resistances",
                        8.0,
                    )],
                ),
                explicit(
                    Prefix,
                    "explicit.stat_3299347043",
                    "# to maximum Life",
                    60.0,
                ),
                explicit(
                    Prefix,
                    "explicit.stat_1050105434",
                    "# to maximum Mana",
                    40.0,
                ),
                explicit(
                    Suffix,
                    "explicit.stat_3372524247",
                    "#% to Fire Resistance",
                    30.0,
                ),
                explicit(
                    Suffix,
                    "explicit.stat_3917489142",
                    "#% increased Rarity of Items found",
                    15.0,
                ),
                explicit(Suffix, "explicit.stat_4080418644", "# to Strength", 20.0),
            ],
        );
        let russian = StatCatalog {
            stats: vec![TradeStat {
                id: "pseudo.pseudo_number_of_empty_prefix_mods".to_owned(),
                text: "# пустых свойств-префиксов".to_owned(),
                mod_type: "pseudo".to_owned(),
            }],
        };

        let filters = build_filters(&ring, 10, &russian);

        let free: Vec<&SearchFilter> = filters
            .iter()
            .filter(|filter| filter.tag == FilterTag::EmptyAffix)
            .collect();
        assert_eq!(free.len(), 1, "every suffix slot is taken");
        let prefixes = free[0];
        assert_eq!(
            prefixes.trade_ids,
            ["pseudo.pseudo_number_of_empty_prefix_mods"]
        );
        assert_eq!(prefixes.display_text, "# пустых свойств-префиксов");
        assert_eq!(prefixes.generation, Some(Prefix));
        let roll = prefixes.roll.as_ref().expect("roll");
        assert_eq!((roll.value, roll.min, roll.max), (1.0, Some(1.0), None));
        assert!(!prefixes.enabled && !prefixes.hidden);

        let generation = |trade_id: &str| {
            filters
                .iter()
                .find(|filter| filter.trade_ids == [trade_id])
                .expect("row")
                .generation
        };
        assert_eq!(generation("explicit.stat_3299347043"), Some(Prefix));
        assert_eq!(generation("explicit.stat_3917489142"), Some(Suffix));
        assert_eq!(generation("implicit.stat_2901986750"), None);
    }

    #[test]
    fn free_slots_count_mods_by_ee2s_maxima_and_skip_items_that_cannot_be_crafted() {
        let explicit =
            |generation, trade_id: &str| affix(generation, stat_with_id(Some(trade_id), "", 1.0));
        // Rare jewels take two of each. One stat in both a prefix and a suffix still fills two
        // slots, though its merged row belongs to neither.
        let jewel = item(
            ItemRarity::Rare,
            "jewel",
            vec![
                explicit(Prefix, "explicit.stat_a"),
                explicit(Suffix, "explicit.stat_a"),
                explicit(Suffix, "explicit.stat_b"),
            ],
        );
        assert_eq!(free_slots(&jewel), [(Some(Prefix), 1.0)]);
        let merged = build_filters(&jewel, 10, &StatCatalog::default())
            .into_iter()
            .find(|filter| filter.trade_ids == ["explicit.stat_a"])
            .expect("merged row");
        assert_eq!(merged.generation, None);

        let mut corrupted = jewel.clone();
        corrupted.is_corrupted = true;
        assert!(free_slots(&corrupted).is_empty());

        // A stat allowing one more prefix leaves a slot after three.
        let mut ring = item(
            ItemRarity::Rare,
            "accessory.ring",
            vec![modifier(
                ModifierType::Implicit,
                None,
                vec![stat_with_id(
                    Some("implicit.stat_3182714256"),
                    "# Prefix Modifier allowed",
                    1.0,
                )],
            )],
        );
        for (generation, trade_id) in [
            (Prefix, "explicit.stat_a"),
            (Prefix, "explicit.stat_b"),
            (Prefix, "explicit.stat_c"),
            (Suffix, "explicit.stat_d"),
            (Suffix, "explicit.stat_e"),
            (Suffix, "explicit.stat_f"),
        ] {
            ring.mods.push(explicit(generation, trade_id));
        }
        assert_eq!(free_slots(&ring), [(Some(Prefix), 1.0)]);
    }

    #[test]
    fn affixes_are_selected_by_tier_and_pseudo_totals_start_unselected() {
        // The live ring the player reported: a T9 evasion prefix was searched while the T1 mana
        // prefix was not, because a pseudo mana total "covered" it.
        let tiered = |generation, tier, trade_id: &str, text: &str| {
            let mut modifier = affix(generation, stat_with_id(Some(trade_id), text, 50.0));
            modifier.info.tier = Some(tier);
            modifier
        };
        let ring = item(
            ItemRarity::Rare,
            "accessory.ring",
            vec![
                tiered(Prefix, 9, "explicit.stat_2144192055", "# to Evasion Rating"),
                tiered(Prefix, 1, "explicit.stat_1050105434", "# to maximum Mana"),
                tiered(Suffix, 2, "explicit.stat_1379411836", "# to all Attributes"),
            ],
        );

        let filters = build_filters(&ring, 10, &StatCatalog::default());

        let enabled = |trade_id: &str| {
            filters
                .iter()
                .find(|filter| filter.trade_ids == [trade_id])
                .map(|filter| filter.enabled)
        };
        assert_eq!(enabled("explicit.stat_2144192055"), Some(false));
        assert_eq!(enabled("explicit.stat_1050105434"), Some(true));
        assert_eq!(enabled("explicit.stat_1379411836"), Some(true));
        assert!(
            filters
                .iter()
                .filter(|filter| filter.tag == FilterTag::Pseudo)
                .all(|filter| !filter.enabled)
        );
    }

    #[test]
    fn a_waystone_searches_its_tier_but_not_its_modifiers() {
        // The live T16 waystone: its T1 map modifiers were searched on top of the tier and the
        // search found nothing -- a map modifier's tier only says how hard the map is.
        let mut damage = affix(
            Prefix,
            stat_with_id(
                Some("explicit.stat_1890519597"),
                "#% increased Monster Damage",
                30.0,
            ),
        );
        damage.info.tier = Some(1);
        let mut waystone = item(ItemRarity::Rare, "map.waystone", vec![damage]);
        waystone.waystone = Some(WaystoneProperties {
            tier: Some(16),
            ..Default::default()
        });

        let filters = build_filters(&waystone, 10, &StatCatalog::default());

        let enabled = |trade_id: &str| {
            filters
                .iter()
                .find(|filter| filter.trade_ids == [trade_id])
                .map(|filter| filter.enabled)
        };
        assert_eq!(enabled("map_filters.map_tier"), Some(true));
        assert_eq!(enabled("explicit.stat_1890519597"), Some(false));
    }

    #[test]
    fn a_rare_without_a_top_tier_affix_searches_its_pseudo_totals() {
        // The live English ring (EE2's `RareWithImplicit` sample): T3 and T7 affixes only, so the
        // tier rule alone selected nothing and the search priced any Prismatic Ring.
        let low_tier = |generation, trade_id: &str, text: &str| {
            let mut modifier = affix(generation, stat_with_id(Some(trade_id), text, 15.0));
            modifier.info.tier = Some(7);
            modifier
        };
        let ring = item(
            ItemRarity::Rare,
            "accessory.ring",
            vec![
                low_tier(Prefix, "explicit.stat_2144192055", "# to Evasion Rating"),
                low_tier(Suffix, "explicit.stat_4220027924", "+#% to Cold Resistance"),
            ],
        );

        let filters = build_filters(&ring, 10, &StatCatalog::default());

        let totals: Vec<_> = filters
            .iter()
            .filter(|filter| filter.tag == FilterTag::Pseudo && !filter.hidden)
            .collect();
        assert!(!totals.is_empty(), "cold resistance has pseudo totals");
        assert!(totals.iter().all(|filter| filter.enabled));
        assert!(
            filters
                .iter()
                .filter(|filter| filter.generation.is_some() && filter.tag != FilterTag::EmptyAffix)
                .all(|filter| !filter.enabled)
        );
    }

    #[test]
    fn build_filters_produces_a_trade_ready_explicit_filter() {
        let rarity = stat_with_id(
            Some("explicit.stat_3917489142"),
            "#% increased Rarity of Items found",
            25.0,
        );
        let item = ParsedItem {
            mods: vec![modifier(ModifierType::Explicit, None, vec![rarity])],
            ..Default::default()
        };

        let filters = build_filters(&item, 10, &StatCatalog::default());

        let rarity_filter = filters
            .iter()
            .find(|f| f.trade_ids == vec!["explicit.stat_3917489142".to_owned()])
            .expect("explicit rarity filter should be produced");
        assert_eq!(rarity_filter.tag, FilterTag::Explicit);
        assert!(rarity_filter.enabled, "explicit filters default to enabled");
        let roll = rarity_filter.roll.as_ref().expect("roll");
        assert_eq!(
            roll.min,
            Some(roll.default_min),
            "the lower bound is preset"
        );
        assert_eq!(roll.max, None, "the upper bound starts open, like EE2's");
    }

    #[test]
    fn a_text_several_trade_ids_share_searches_all_of_them_the_matched_one_first() {
        // The live EN catalog's `# to all Attributes` pair (2026-09-23), with the implicit
        // stat of the same text, which an explicit mod must not pick up.
        let catalog = StatCatalog {
            stats: [
                "explicit.stat_1379411836",
                "explicit.stat_2897413282",
                "implicit.stat_1379411836",
            ]
            .into_iter()
            .map(|id| TradeStat {
                id: id.to_owned(),
                text: "# to all Attributes".to_owned(),
                mod_type: "explicit".to_owned(),
            })
            .collect(),
        };
        let attributes = stat_with_id(
            Some("explicit.stat_2897413282"),
            "# to all Attributes",
            13.0,
        );
        let item = ParsedItem {
            mods: vec![modifier(ModifierType::Explicit, None, vec![attributes])],
            ..Default::default()
        };

        let filters = build_filters(&item, 10, &catalog);

        let row = filters
            .iter()
            .find(|f| f.tag == FilterTag::Explicit)
            .expect("the attributes row");
        assert_eq!(
            row.trade_ids,
            ["explicit.stat_2897413282", "explicit.stat_1379411836"]
        );
    }

    #[test]
    fn a_stat_better_lower_keeps_worse_rolls_out_and_a_named_one_is_searched_exactly() {
        // The live Russian helmet's "4(2-4) fewer enemies to be Surrounded", the catalog's
        // `Require # additional enemies` at -4 in [-4, -2]; and a jewel's ring number, which
        // names a ring.
        let surrounded = ParsedStat {
            min: -4.0,
            max: -2.0,
            ..stat_with_id(
                Some("explicit.stat_2267564181"),
                "Для окружения требуется на # врагов больше",
                -4.0,
            )
        };
        let ring = stat_with_id(
            Some("explicit.stat_3642528642"),
            "Only affects Passives in # Ring",
            2.0,
        );
        // EE2 has this one better lower in the game's terms; the catalog words it the good way
        // round, where more is better.
        let flask = stat_with_id(
            Some("explicit.stat_644456512"),
            "#% reduced Flask Charges used",
            20.0,
        );
        let item = ParsedItem {
            mods: vec![modifier(
                ModifierType::Explicit,
                None,
                vec![surrounded, ring, flask],
            )],
            ..Default::default()
        };

        let filters = build_filters(&item, 10, &StatCatalog::default());
        let bounds = |trade_id: &str| {
            let roll = filters
                .iter()
                .find(|filter| filter.trade_ids == [trade_id])
                .and_then(|filter| filter.roll.as_ref())
                .expect("the row and its roll");
            (roll.min, roll.max)
        };

        // At most 3 more: a listing at -2 (2 fewer) is worse and stays out.
        assert_eq!(bounds("explicit.stat_2267564181"), (None, Some(-3.0)));
        assert_eq!(bounds("explicit.stat_3642528642"), (Some(2.0), Some(2.0)));
        assert_eq!(bounds("explicit.stat_644456512"), (Some(20.0), None));
    }

    #[test]
    fn a_row_the_item_words_the_other_way_reads_and_bounds_in_its_words() {
        // The helmet's "4(2-4) fewer enemies to be Surrounded" as the parser leaves it: the
        // catalog's `additional` at -4, with the item's own wording beside it.
        let surrounded = ParsedStat {
            min: -4.0,
            max: -2.0,
            negated_text: Some("Для окружения требуется на # врага меньше".to_owned()),
            ..stat_with_id(
                Some("explicit.stat_2267564181"),
                "Для окружения требуется на # врагов больше",
                -4.0,
            )
        };
        let item = ParsedItem {
            mods: vec![modifier(ModifierType::Explicit, None, vec![surrounded])],
            ..Default::default()
        };

        let filters = build_filters(&item, 10, &StatCatalog::default());
        let row = &filters[0];
        let roll = row.roll.as_ref().expect("roll");

        assert!(row.inverted);
        assert_eq!(
            row.display_text,
            "Для окружения требуется на # врага меньше"
        );
        // "At most 3 more" in the catalog's words is "at least 3 fewer" in the item's.
        assert_eq!((roll.value, roll.min, roll.max), (4.0, Some(3.0), None));
    }

    #[test]
    fn a_row_one_printed_form_gives_reads_in_the_items_words() {
        // A mace's "40% шанс наложения оцепенения при нанесении удара" as the parser leaves it
        // (live vendor item, 2026-09-23): the catalog's `Накладывает оцепенение при нанесении
        // удара` at 40, with the item's own wording beside it.
        let daze = |value| ParsedStat {
            printed_text: Some("#% шанс наложения оцепенения при нанесении удара".to_owned()),
            ..stat_with_id(
                Some("implicit.stat_2933846633"),
                "Накладывает оцепенение при нанесении удара",
                value,
            )
        };
        let row = |mods: Vec<ParsedModifier>| {
            let item = ParsedItem {
                mods,
                ..Default::default()
            };
            build_filters(&item, 10, &StatCatalog::default())
                .into_iter()
                .find(|filter| filter.trade_ids[0] == "implicit.stat_2933846633")
                .expect("the daze row")
        };

        let one = row(vec![modifier(
            ModifierType::Implicit,
            None,
            vec![daze(40.0)],
        )]);
        assert_eq!(
            one.display_text,
            "#% шанс наложения оцепенения при нанесении удара"
        );
        assert!(!one.inverted);

        // Two lines of the stat total one row, which reads the catalog's way.
        let two = row(vec![
            modifier(ModifierType::Implicit, None, vec![daze(40.0)]),
            modifier(ModifierType::Implicit, None, vec![daze(20.0)]),
        ]);
        assert_eq!(
            two.display_text,
            "Накладывает оцепенение при нанесении удара"
        );
    }

    #[test]
    fn explicit_and_fractured_sources_of_one_stat_id_merge_into_a_fractured_filter() {
        let explicit_stat = stat_with_id(
            Some("explicit.stat_phys"),
            "#% increased Physical Damage",
            10.0,
        );
        let fractured_stat = stat_with_id(
            Some("explicit.stat_phys"),
            "#% increased Physical Damage",
            15.0,
        );
        let item = ParsedItem {
            mods: vec![
                modifier(ModifierType::Explicit, Some(3), vec![explicit_stat]),
                modifier(ModifierType::Fractured, Some(5), vec![fractured_stat]),
            ],
            ..Default::default()
        };

        let filters = build_filters(&item, 10, &StatCatalog::default());

        let merged = filters
            .iter()
            .find(|f| f.trade_ids == vec!["explicit.stat_phys".to_owned()])
            .expect("merged filter should be produced");
        assert_eq!(merged.tag, FilterTag::Fractured);
        assert_eq!(
            merged.tier,
            Some(3),
            "tier is the min across contributing sources"
        );
        assert_eq!(merged.roll.as_ref().expect("roll").value, 25.0);
    }

    #[test]
    fn unscalable_stat_produces_a_filter_with_no_roll() {
        let unscalable = ParsedStat {
            stat_id: Some("explicit.stat_unscalable".to_owned()),
            text: "Unscalable Value".to_owned(),
            value: 0.0,
            min: 0.0,
            max: 0.0,
            dp: false,
            unscalable: true,
            negated_text: None,
            printed_text: None,
        };
        let item = ParsedItem {
            mods: vec![modifier(ModifierType::Explicit, None, vec![unscalable])],
            ..Default::default()
        };

        let filters = build_filters(&item, 10, &StatCatalog::default());

        let filter = filters
            .iter()
            .find(|f| f.trade_ids == vec!["explicit.stat_unscalable".to_owned()])
            .expect("unscalable filter should still be produced");
        assert!(filter.roll.is_none());
        assert!(!filter.enabled, "a flag starts unchecked");
    }
}
