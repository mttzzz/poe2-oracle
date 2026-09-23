//! Search-filter construction from a parsed item: `poe2_domain::ParsedItem` -> `Vec<SearchFilter>`
//! ready for `trade-client::search_with_filters`. The rows follow the real, working reference's
//! per-mod aggregation, `calculatedStatToFilter`
//! (`exiled-exchange-2/renderer/src/web/price-check/filters/create-stat-filters.ts:352-470`), and
//! its `pseudo/index.ts` summed pseudo-stat rules (see the `pseudo` module); which rows a search
//! starts with and how far below a roll it searches follow PoE Overlay II: its mod ranking
//! (`rank`), weighted sums (`weighted`) and search profiles (`profile`). Both are cited as the
//! algorithms' ground truth, never as code to copy. Pure logic, no I/O: this crate depends on
//! nothing but `poe2-domain` by design (see `Cargo.toml`); the RePoE tier table it ranks mods by
//! is compiled in (`tiers`).
//!
//! Property rows (the `property` module) search the trade query's own item filters -- defences,
//! DPS, item level, sockets, quality -- rather than a stat: their single trade id names that
//! filter as `<group>.<key>`.

mod better;
mod profile;
mod property;
mod pseudo;
mod rank;
mod tiers;
mod weighted;

use poe2_domain::{
    ItemRarity, ModGeneration, ModifierInfo, ModifierType, ParsedItem, ParsedStat, StatCatalog,
};
pub use profile::{SearchProfile, apply_profile};
use property::property_filters;
pub use property::uses_exact_preset;
use rank::{Candidate, Pick};
pub use tiers::{GameMod, Roll, game_mod, printed, stat_hash};

/// Where a `SearchFilter` came from -- drives the panel's tag pill.
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

/// Which bound a search profile sets on a roll, and from what (PoE Overlay II's `gQ`,
/// `9505.bundle.js` ~69873; `apply_profile`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollBound {
    /// A higher roll is better: the minimum is the value less the profile's range, the maximum
    /// open.
    Higher,
    /// A lower roll is better: the maximum is the value plus the profile's range, the minimum
    /// open.
    Lower,
    /// The minimum is the value whatever the profile: a fixed roll, a count, a level.
    AtLeast,
    /// The maximum is the value whatever the profile: a fixed roll a lower one beats.
    AtMost,
    /// Both bounds are the value: a map tier, a number naming something.
    Exactly,
}

/// The roll a `SearchFilter` searches around. `min`/`max` are the CURRENT search bounds (what a
/// UI numeric input binds to; `None` leaves that side of the search open), which the search
/// profile sets from `value` the way `bound` says (`apply_profile`).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchFilterRoll {
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub dp: bool,
    pub bound: RollBound,
}

/// Where a mod row's tier sits in its family on this kind of item, from the RePoE tier table
/// (`data/mod-tiers.tsv`, see the `tiers` module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TierInfo {
    /// The tier the item prints, T1 the best.
    pub current: u32,
    /// How many tiers the family has here, at least `current`.
    pub count: u32,
    /// The best tier the item's level lets roll, never above `current`.
    pub best_available: u32,
    /// The item level the current tier needs.
    pub min_level: u32,
    /// The bottom of the current tier's roll range, in the row's terms; `None` for a flag.
    pub tier_floor: Option<f64>,
    /// The lowest to the highest roll of every tier, in the row's terms; `None` for a flag.
    pub range: Option<(f64, f64)>,
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
    /// Minimum tier across every contributing mod source that has one (`ModifierInfo.tier`, the
    /// game's own number from the advanced copy); `None` if no source carries a tier (a Unique
    /// item's fixed mods never do; a rune-granted stat never does). `tier_info` places it among
    /// its family's tiers.
    pub tier: Option<u32>,
    pub roll: Option<SearchFilterRoll>,
    /// `true` = checkbox checked / included in the search -- the NON-inverted sense, deliberately
    /// opposite of the reference's own confusingly-named `StatFilter.disabled`.
    pub enabled: bool,
    /// `true` = listed only behind the panel's "show hidden" toggle, where EE2 hides the row
    /// (its `StatFilter.hidden`): a stat a pseudo total already counts, a total restating
    /// another, a minor share of a weapon's DPS -- and where PoE Overlay II merges a mod into a
    /// property or a total. A hidden row is never `enabled`. EE2's toggle swaps the list to the
    /// hidden rows alone and only shows while some row is hidden (`FiltersBlock.vue`'s
    /// `filteredStats`).
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
    /// PoE Overlay II's score for what the row searches (`rank`): a mod row's best contributing
    /// mod's, a property's or a total's best mod merged into it; `None` where it ranks nothing
    /// (implicits, runes, uniques, map items, free slots). Quick Price searches the best rows
    /// scoring at least 3.
    pub score: Option<f64>,
    /// Where the row's tier sits among its family's on this kind of item: a row of one mod with a
    /// printed tier the tier table knows.
    pub tier_info: Option<TierInfo>,
    /// A weighted sum: a total the trade site has no pseudo stat for, searched as a `weight2`
    /// stat group adding up `trade_ids` at weight 1 each (see the `weighted` module).
    pub weighted_sum: bool,
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
/// then the pseudo totals (`pseudo::pseudo_filters`) and weighted sums (`weighted`), then one per
/// distinct trade-searchable stat (grouped across mods sharing a stat id and a compatible
/// `ModifierType`, see `per_mod_filters`), then the free affix slots (`empty_affix_filters`, last
/// like EE2's `finalFilterTweaks` row). Every row gets its score (`rank`); `profile` picks the rows
/// searched -- Broad keeps checkboxes, so built fresh it keeps those of the item's own default
/// profile -- and sets every row's bounds (`apply_profile`). `catalog` is the searched site's stat
/// catalog: pseudo, weighted-sum and free-slot rows read in its language.
pub fn build_filters(
    item: &ParsedItem,
    profile: SearchProfile,
    catalog: &StatCatalog,
) -> Vec<SearchFilter> {
    let picking = match profile {
        SearchProfile::Broad => SearchProfile::default_for(item),
        other => other,
    };
    let mut rows = Rows::build(item, catalog);
    rows.rank(item);
    rows.pick(item, picking);
    let mut filters = rows.filters;
    settle_exact_kinds(item, picking, catalog, &mut filters);
    word_like_the_item(item, &mut filters);
    apply_profile(&mut filters, profile);
    filters
}

/// `build_filters`'s rows, each with the item's mods behind it (indexes into `item.mods`): the
/// mods a mod row totals, the local mods a property row counts, the mods a pseudo total or a
/// weighted sum adds up.
struct Rows {
    filters: Vec<SearchFilter>,
    sources: Vec<Vec<usize>>,
    /// Each mod's score (`rank::score`).
    scores: Vec<Option<f64>>,
    /// The mods merged into a property or a shown total, which take their score: never searched
    /// by themselves (PoE Overlay II's `markBaseModsAsMerged`).
    merged: Vec<bool>,
}

impl Rows {
    fn build(item: &ParsedItem, catalog: &StatCatalog) -> Self {
        let mut filters = property_filters(item);
        let mut sources: Vec<Vec<usize>> = filters
            .iter()
            .map(|row| property::merged_mods(item, &row.trade_ids[0]))
            .collect();
        let (mut mod_rows, mod_sources) = per_mod_filters(item, catalog);
        let totals = pseudo::pseudo_filters(item, catalog, &mut mod_rows)
            .into_iter()
            .chain(weighted::weighted_sums(item, catalog));
        for (row, row_sources) in totals {
            filters.push(row);
            sources.push(row_sources);
        }
        filters.extend(mod_rows);
        sources.extend(mod_sources);
        for row in empty_affix_filters(item, catalog) {
            filters.push(row);
            sources.push(Vec::new());
        }
        Self {
            filters,
            sources,
            scores: Vec::new(),
            merged: Vec::new(),
        }
    }

    /// Scores every mod and row, merges mods into the properties and shown totals counting them
    /// -- hiding a mod row every mod of which is merged -- and places each one-mod row's tier.
    fn rank(&mut self, item: &ParsedItem) {
        let category = item.category.as_ref().map_or("", |category| &category.id);
        let families: Vec<_> = item
            .mods
            .iter()
            .map(|modifier| tiers::family(modifier, category))
            .collect();
        self.scores = item
            .mods
            .iter()
            .zip(&families)
            .map(|(modifier, &family)| rank::score(item, modifier, family))
            .collect();
        self.merged = vec![false; item.mods.len()];
        for (filter, sources) in self.filters.iter().zip(&self.sources) {
            let merges = filter.tag == FilterTag::Property
                || (filter.tag == FilterTag::Pseudo && !filter.hidden);
            if merges {
                for &modifier in sources {
                    self.merged[modifier] = true;
                }
            }
        }
        for (filter, sources) in self.filters.iter_mut().zip(&self.sources) {
            filter.score = sources
                .iter()
                .filter_map(|&modifier| self.scores[modifier])
                .reduce(f64::max);
            if !is_mod_row(filter) {
                continue;
            }
            if !sources.is_empty() && sources.iter().all(|&modifier| self.merged[modifier]) {
                filter.hidden = true;
            }
            if let [modifier] = sources[..]
                && let (Some(family), Some(tier)) =
                    (families[modifier], item.mods[modifier].info.tier)
            {
                filter.tier_info = filter
                    .trade_ids
                    .first()
                    .and_then(|id| family.tier_info(tier, item.item_level, tiers::stat_hash(id)));
            }
        }
    }

    /// What Quick Price can search, in PoE Overlay II's order: the scored properties, the mods
    /// neither merged nor rowless, in the item's order, then the shown totals.
    fn candidates(&self) -> Vec<Candidate> {
        let rows = |tag| {
            self.filters
                .iter()
                .enumerate()
                .filter(move |(_, filter)| filter.tag == tag && !filter.hidden)
                .filter_map(|(index, filter)| {
                    Some(Candidate {
                        pick: Pick::Row(index),
                        score: filter.score?,
                        weighted_sum: filter.weighted_sum,
                    })
                })
        };
        let has_row = |modifier: usize| {
            self.filters
                .iter()
                .zip(&self.sources)
                .any(|(filter, sources)| is_mod_row(filter) && sources.contains(&modifier))
        };
        let mods = self
            .scores
            .iter()
            .enumerate()
            .filter(|&(modifier, _)| !self.merged[modifier] && has_row(modifier))
            .filter_map(|(modifier, score)| {
                Some(Candidate {
                    pick: Pick::Mod(modifier),
                    score: (*score)?,
                    weighted_sum: false,
                })
            });
        rows(FilterTag::Property)
            .chain(mods)
            .chain(rows(FilterTag::Pseudo))
            .collect()
    }

    /// Checks the rows `profile` starts with (PoE Overlay II's `gQ` selection):
    /// - Quick Price: its pick (`rank::quick_picks`) -- a picked mod checks every row it feeds;
    /// - Exact Match: every row not hidden, and the properties Quick Price would pick;
    /// - Crafting Base: the implicit, fractured and granted-skill rows, and the item level.
    ///
    /// A property row no score reaches (item level, sockets, quality, gem and waystone rows)
    /// keeps EE2's own checkbox in Quick Price and Exact Match. Then every profile checks the
    /// rows PoE Overlay II always searches (`always_searched`). A checked row is shown.
    fn pick(&mut self, item: &ParsedItem, profile: SearchProfile) {
        let picks = rank::quick_picks(self.candidates());
        for (index, (filter, sources)) in self.filters.iter_mut().zip(&self.sources).enumerate() {
            let picked = || picks.contains(&Pick::Row(index));
            let on = match (filter.tag, profile) {
                (FilterTag::Property, _) if !property::scored(&filter.trade_ids[0]) => {
                    if profile == SearchProfile::CraftingBase {
                        filter.trade_ids[0] == "type_filters.ilvl"
                    } else {
                        filter.enabled
                    }
                }
                (FilterTag::Property, SearchProfile::CraftingBase) => false,
                (FilterTag::Property, _) => picked(),
                (FilterTag::Pseudo | FilterTag::EmptyAffix, SearchProfile::CraftingBase) => false,
                (FilterTag::Pseudo, SearchProfile::QuickPrice | SearchProfile::Broad) => picked(),
                (FilterTag::EmptyAffix, SearchProfile::QuickPrice | SearchProfile::Broad) => false,
                (_, SearchProfile::ExactMatch) => !filter.hidden,
                (_, SearchProfile::QuickPrice | SearchProfile::Broad) => sources
                    .iter()
                    .any(|&modifier| picks.contains(&Pick::Mod(modifier))),
                (_, SearchProfile::CraftingBase) => sources.iter().any(|&modifier| {
                    matches!(
                        item.mods[modifier].info.modifier_type,
                        ModifierType::Implicit | ModifierType::Fractured | ModifierType::Skill
                    )
                }),
            };
            filter.enabled = on || always_searched(item, filter, sources);
            if filter.enabled {
                filter.hidden = false;
            }
        }
    }
}

/// Whether a row is a mod's own: neither a property, a total nor a free-slot count.
fn is_mod_row(filter: &SearchFilter) -> bool {
    !matches!(
        filter.tag,
        FilterTag::Property | FilterTag::Pseudo | FilterTag::EmptyAffix
    )
}

/// The stats PoE Overlay II searches in every profile (`9505.bundle.js`'s `et` and `ea`): base
/// implicits naming what the base is (grenade projectiles, chaining, piercing, extra arrows, an
/// explosion on critical kills, an extra bolt, maximum elemental resistances, spirit, movement
/// speed), unrevealed mods, and a timeless jewel's legend. Every id checked in the live EN catalog
/// (2026-09-23).
const ALWAYS_SEARCHED: [&str; 18] = [
    "implicit.stat_1980802737",
    "implicit.stat_1028592286",
    "implicit.stat_2321178454",
    "implicit.stat_3885405204",
    "implicit.stat_1541903247",
    "implicit.stat_1967051901",
    "implicit.stat_1978899297",
    "implicit.stat_3981240776",
    "implicit.stat_2250533757",
    "pseudo.pseudo_number_of_unrevealed_mods",
    "explicit.stat_3418580811|21",
    "explicit.stat_3418580811|22",
    "explicit.stat_3418580811|23",
    "explicit.stat_3418580811|24",
    "explicit.stat_3418580811|25",
    "explicit.stat_3418580811|26",
    "explicit.stat_3418580811|27",
    "explicit.stat_3418580811|28",
];

/// Whether PoE Overlay II searches `filter` whatever the profile: an `ALWAYS_SEARCHED` stat, or a
/// granted skill at level 19 or more -- any on an amulet.
fn always_searched(item: &ParsedItem, filter: &SearchFilter, sources: &[usize]) -> bool {
    let skill = sources
        .iter()
        .any(|&modifier| item.mods[modifier].info.modifier_type == ModifierType::Skill);
    let amulet = item
        .category
        .as_ref()
        .is_some_and(|category| category.id == "accessory.amulet");
    filter
        .trade_ids
        .first()
        .is_some_and(|id| ALWAYS_SEARCHED.contains(&id.as_str()))
        || (skill && (amulet || filter.roll.as_ref().is_some_and(|roll| roll.value >= 19.0)))
}

/// Turns each mod row the item words the other way round from the catalog -- `15% reduced
/// Attribute Requirements`, which the parser read as the catalog's `#% increased Attribute
/// Requirements` at -15 (`ParsedStat::negated_text`) -- into the item's own words: its text, the
/// value negated and the bound turned into its terms ("at least 15% reduced"), and `inverted`
/// set so the search turns them back: EE2's `filterAdjustmentForNegate`
/// (`create-stat-filters.ts:615-631`). Only a row that still totals below zero turns: a reduced
/// roll that an increased one outweighs reads the catalog's way. Runs once every rule above has
/// worked in the catalog's terms, before the profile sets the bounds.
fn word_like_the_item(item: &ParsedItem, filters: &mut [SearchFilter]) {
    for filter in filters.iter_mut() {
        if !is_mod_row(filter) {
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
        roll.value = -roll.value;
        roll.bound = match roll.bound {
            RollBound::Higher => RollBound::Lower,
            RollBound::Lower => RollBound::Higher,
            RollBound::AtLeast => RollBound::AtMost,
            RollBound::AtMost => RollBound::AtLeast,
            RollBound::Exactly => RollBound::Exactly,
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

/// EE2's selection for the map items PoE Overlay II ranks nothing on, applied over the profile's
/// pick. A tablet searches its `# uses remaining (Tablets)` pseudo total (at least as many uses,
/// shown on uniques too) and every mod at exactly its roll, leaving its implicit out
/// (`createExactStatFilters`, `create-stat-filters.ts:38-220`). A waystone priced quickly follows
/// EE2's map rule (`finalFilterTweaks`, `create-stat-filters.ts:718-726`): its tier and
/// properties set the price, its modifiers only make the map harder -- they start unselected, a
/// desecrated one excepted. Unlike EE2 they stay listed: the player marks them there. Crafting
/// Base leaves both to its own pick.
fn settle_exact_kinds(
    item: &ParsedItem,
    profile: SearchProfile,
    catalog: &StatCatalog,
    filters: &mut Vec<SearchFilter>,
) {
    let category = item.category.as_ref().map_or("", |c| c.id.as_str());
    let unique = item.rarity == Some(ItemRarity::Unique);
    let crafting = profile == SearchProfile::CraftingBase;
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
                        filter.enabled |= !crafting;
                        if let Some(roll) = &mut filter.roll {
                            roll.bound = RollBound::AtLeast;
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
                        min: None,
                        max: None,
                        dp: false,
                        bound: RollBound::AtLeast,
                    }),
                    enabled: !crafting,
                    hidden: false,
                    generation: None,
                    inverted: false,
                    score: None,
                    tier_info: None,
                    weighted_sum: false,
                },
            );
        }
    } else if category == "map.waystone" && profile == SearchProfile::QuickPrice {
        for filter in filters.iter_mut() {
            if !matches!(filter.tag, FilterTag::Property | FilterTag::Desecrated) {
                filter.enabled = false;
            }
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
    count: usize,
    /// The mods the row totals (indexes into `item.mods`), in first-seen order.
    sources: Vec<usize>,
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
            count: 0,
            sources: Vec::new(),
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

    fn add(&mut self, modifier: usize, info: &ModifierInfo, stat: &ParsedStat) {
        self.count += 1;
        if !self.sources.contains(&modifier) {
            self.sources.push(modifier);
        }
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

    /// The row, starting unchecked -- the search profile picks it -- and the mods it totals.
    fn into_filter(self, catalog: &StatCatalog) -> (SearchFilter, Vec<usize>) {
        let tag = group_tag(&self.modifier_types);
        // A granted skill's level is searched at least as it is whatever the profile, as PoE
        // Overlay II searches it; a roll no mod can roll otherwise likewise.
        let bound = if self.bucket == GroupBucket::Solo(ModifierType::Skill) {
            RollBound::AtLeast
        } else {
            better::bound(&self.stat_id, self.min == self.max)
        };
        let roll = self.any_scalable.then_some(SearchFilterRoll {
            value: self.value,
            min: None,
            max: None,
            dp: self.dp,
            bound,
        });
        // The item's own words when one line it prints another way than the catalog is the whole
        // row (`40% шанс наложения оцепенения...` for `Накладывает оцепенение...`); a total of
        // several reads the catalog's way.
        let display_text = match self.printed {
            Some(printed) if self.count == 1 => printed,
            _ => self.text.clone(),
        };
        let filter = SearchFilter {
            trade_ids: same_text_ids(catalog, &self.stat_id),
            stat_ref: self.text,
            display_text,
            tag,
            tier: self.tier,
            roll,
            enabled: false,
            hidden: false,
            generation: self.generation,
            inverted: false,
            score: None,
            tier_info: None,
            weighted_sum: false,
        };
        (filter, self.sources)
    }
}

/// Groups every stat-id-bearing `ParsedStat` across `item.mods` (see `group_bucket`) into one
/// `SearchFilter` per group, in first-seen order, each with the mods it totals. Stats with
/// `stat_id: None` are skipped -- unmatched against the trade catalog, they already surface via
/// `item.unknown_mods` for display elsewhere, not this crate's concern -- and so are stats a
/// property row already counts (`property::folded_into_properties`). A row carries every trade id
/// its text has (`same_text_ids`).
fn per_mod_filters(
    item: &ParsedItem,
    catalog: &StatCatalog,
) -> (Vec<SearchFilter>, Vec<Vec<usize>>) {
    let mut groups: Vec<ModStatGroup> = Vec::new();
    for (index, modifier) in item.mods.iter().enumerate() {
        let bucket = group_bucket(modifier.info.modifier_type);
        for stat in &modifier.stats {
            let Some(stat_id) = stat.stat_id.as_deref() else {
                continue;
            };
            if property::folded_into_properties(item, stat_id) {
                continue;
            }
            let group = groups
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
            groups[group].add(index, &modifier.info, stat);
        }
    }
    groups
        .into_iter()
        .map(|group| group.into_filter(catalog))
        .unzip()
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
                    min: None,
                    max: None,
                    dp: false,
                    bound: RollBound::AtLeast,
                }),
                enabled: false,
                hidden: false,
                generation: Some(generation),
                inverted: false,
                score: None,
                tier_info: None,
                weighted_sum: false,
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

    /// `stat_with_id` rolled `value` within `min..=max`.
    fn rolled(stat_id: &str, value: f64, min: f64, max: f64) -> ParsedStat {
        ParsedStat {
            min,
            max,
            ..stat_with_id(Some(stat_id), "", value)
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

    /// An explicit prefix or suffix of tier `tier` granting `stat`.
    fn tiered(generation: ModGeneration, tier: u32, stat: ParsedStat) -> ParsedModifier {
        let mut affix = affix(generation, stat);
        affix.info.tier = Some(tier);
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

    fn quick(item: &ParsedItem) -> Vec<SearchFilter> {
        build_filters(item, SearchProfile::QuickPrice, &StatCatalog::default())
    }

    /// The row whose first trade id is `trade_id`.
    fn row<'a>(filters: &'a [SearchFilter], trade_id: &str) -> &'a SearchFilter {
        filters
            .iter()
            .find(|filter| filter.trade_ids.first().is_some_and(|id| id == trade_id))
            .unwrap_or_else(|| panic!("no {trade_id} row"))
    }

    /// The free slot rows' kinds and counts.
    fn free_slots(item: &ParsedItem) -> Vec<(Option<ModGeneration>, f64)> {
        quick(item)
            .into_iter()
            .filter(|filter| filter.tag == FilterTag::EmptyAffix)
            .map(|filter| (filter.generation, filter.roll.expect("roll").value))
            .collect()
    }

    /// An item level 80 rare ring with a life prefix, two added attack damage prefixes, and fire
    /// resistance and rarity suffixes, rolled so their scores come out round (tiers and ranges
    /// from the RePoE table: 8 life tiers up to item level 54, 8 fire resistance tiers up to 82,
    /// 3 rarity tiers up to 40, 9 tiers of each added damage up to 75).
    fn ring() -> ParsedItem {
        let mut ring = item(
            ItemRarity::Rare,
            "accessory.ring",
            vec![
                // T1 (100-119) at the top of its range.
                tiered(
                    Prefix,
                    1,
                    rolled("explicit.stat_3299347043", 119.0, 100.0, 119.0),
                ),
                // T1 of physical damage to attacks, 17-25.5 on average, halfway.
                tiered(
                    Prefix,
                    1,
                    rolled("explicit.stat_3032590688", 21.25, 17.0, 25.5),
                ),
                // T1 of fire damage to attacks, 31-37 on average, 40% of the way.
                tiered(
                    Prefix,
                    1,
                    rolled("explicit.stat_1573130764", 33.4, 31.0, 37.0),
                ),
                // T4 of fire resistance (26-30) at its bottom.
                tiered(
                    Suffix,
                    4,
                    rolled("explicit.stat_3372524247", 26.0, 26.0, 30.0),
                ),
                // T1 of rarity (15-18) at its bottom.
                tiered(
                    Suffix,
                    1,
                    rolled("explicit.stat_3917489142", 15.0, 15.0, 18.0),
                ),
            ],
        );
        ring.item_level = Some(80);
        ring
    }

    #[test]
    fn quick_price_searches_the_four_best_scores_of_three_and_two_weighted_sums_at_most() {
        let filters = quick(&ring());
        let score = |trade_id| row(&filters, trade_id).score.expect("scored");

        // Life, merged into the life total: tier 8 of 8 tiers, all at item level 80 or under,
        // (8 - 1 + 1) / (8 - 1 + 1) * 2 = 2; a lone Life tag on a ring 3 * 0.75 + 0.5 = 2.75;
        // its roll at the top of its range 0.5. 2 + 2.75 + 0.5 = 5.25.
        assert_eq!(score("pseudo.pseudo_total_life"), 5.25);
        // Rarity, merged into its weighted sum: tier 2, no tag it weighs, a bottom roll 0, the
        // rarity bonus 2: 4.
        assert_eq!(score("explicit.stat_3917489142"), 4.0);
        // Added physical damage: tier 2, Damage 2 * 0.75 = 1.5 (Physical and Attack weigh
        // nothing on a ring), halfway 0.25: 3.75.
        assert_eq!(score("explicit.stat_3032590688"), 3.75);
        // Added fire damage: 2 + 1.5 + 0.5 * 0.4 = 3.7.
        assert!((score("explicit.stat_1573130764") - 3.7).abs() < 1e-9);
        // Fire resistance, merged into the elemental total: tier 4 of 8 at an item level short of
        // T1 (level 82), (8 - 4 + 1) / (8 - 2 + 1) * 2 = 10/7; Resistance 3 * 0.75 = 2.25; a
        // bottom roll 0: 3.68.
        assert!(
            (score("pseudo.pseudo_total_elemental_resistance") - (10.0 / 7.0 + 2.25)).abs() < 1e-9
        );

        // Best first: life 5.25, rarity 4 and physical 3.75 (the two weighted sums allowed), fire
        // 3.7 skipped as a third weighted sum, resistance 3.68 the fourth.
        let searched: Vec<&str> = filters
            .iter()
            .filter(|filter| filter.enabled)
            .map(|filter| filter.stat_ref.as_str())
            .collect();
        assert_eq!(
            searched,
            [
                "+#% total Elemental Resistance",
                "+# total maximum Life",
                "#% increased Rarity of Items found",
                "Adds # to # Physical Damage to Attacks",
            ]
        );
        let fire = row(&filters, "explicit.stat_1573130764");
        assert!(fire.weighted_sum && !fire.enabled && !fire.hidden);
        // A merged mod's own row hides, its total searched in its stead.
        assert!(
            filters
                .iter()
                .filter(|filter| is_mod_row(filter))
                .all(|filter| filter.hidden && !filter.enabled)
        );
        // Every minimum is the item's own roll, the maximum open.
        let life = row(&filters, "pseudo.pseudo_total_life")
            .roll
            .as_ref()
            .expect("roll");
        assert_eq!((life.value, life.min, life.max), (119.0, Some(119.0), None));
    }

    #[test]
    fn exact_match_searches_every_shown_row_crafting_base_the_base_and_broad_a_tenth_lower() {
        let mut ring = ring();
        ring.mods.insert(
            0,
            modifier(
                ModifierType::Implicit,
                None,
                vec![stat_with_id(
                    Some("implicit.stat_2250533757"),
                    "#% increased Movement Speed",
                    5.0,
                )],
            ),
        );
        ring.mods.pop();

        let exact = build_filters(&ring, SearchProfile::ExactMatch, &StatCatalog::default());
        // Every shown row but the item level, which keeps EE2's own checkbox (unchecked on a
        // rare).
        let shown: Vec<&SearchFilter> = exact
            .iter()
            .filter(|filter| !filter.hidden && filter.tag != FilterTag::Property)
            .collect();
        assert!(!shown.is_empty());
        assert!(shown.iter().all(|filter| filter.enabled));
        assert!(!row(&exact, "type_filters.ilvl").enabled);
        assert!(
            exact
                .iter()
                .filter(|filter| filter.hidden)
                .all(|filter| !filter.enabled)
        );
        // A free suffix slot is searched too.
        assert!(row(&exact, "pseudo.pseudo_number_of_empty_suffix_mods").enabled);

        // Crafting Base: the item level and the implicit, which every profile searches besides
        // (PoE Overlay II's base-defining implicits).
        let base = build_filters(&ring, SearchProfile::CraftingBase, &StatCatalog::default());
        let searched: Vec<&str> = base
            .iter()
            .filter(|filter| filter.enabled)
            .map(|filter| filter.trade_ids[0].as_str())
            .collect();
        assert_eq!(searched, ["type_filters.ilvl", "implicit.stat_2250533757"]);

        // Broad, built fresh, keeps Quick Price's checkboxes and searches from 10% below each
        // roll: life 119 - 11.9 = 107.1, rounded to 107.
        let quick = quick(&ring);
        let broad = build_filters(&ring, SearchProfile::Broad, &StatCatalog::default());
        let checked = |filters: &[SearchFilter]| -> Vec<bool> {
            filters.iter().map(|filter| filter.enabled).collect()
        };
        assert_eq!(checked(&broad), checked(&quick));
        let life = row(&broad, "pseudo.pseudo_total_life")
            .roll
            .as_ref()
            .expect("roll");
        assert_eq!((life.min, life.max), (Some(107.0), None));

        // Switching back restores the item's own roll, the checkboxes untouched.
        let mut switched = broad.clone();
        switched[0].enabled = !switched[0].enabled;
        apply_profile(&mut switched, SearchProfile::QuickPrice);
        let life = row(&switched, "pseudo.pseudo_total_life")
            .roll
            .as_ref()
            .expect("roll");
        assert_eq!(life.min, Some(119.0));
        assert_eq!(switched[0].enabled, !broad[0].enabled);
    }

    #[test]
    fn a_local_defence_mod_merges_into_its_property_which_takes_its_score() {
        // A body armour's T1 energy shield increase: tier 8 of 8 (all at item level 75 or
        // under) 2; a lone Defences tag on a body armour 2 * 0.75 + 0.5 = 2; its roll a tenth of
        // the way through 101-110, 0.5 * 0.1 = 0.05; the local energy shield bonus 0.5: 4.55.
        let mut chest = item(
            ItemRarity::Rare,
            "armour.chest",
            vec![tiered(
                Prefix,
                1,
                rolled("explicit.stat_4015621042", 101.9, 101.0, 110.0),
            )],
        );
        chest.item_level = Some(75);
        chest.energy_shield = Some(300);

        let filters = quick(&chest);

        let energy_shield = row(&filters, "equipment_filters.es");
        assert!((energy_shield.score.expect("scored") - 4.55).abs() < 1e-9);
        assert!(energy_shield.enabled);
        assert!(
            filters
                .iter()
                .all(|filter| filter.trade_ids[0] != "explicit.stat_4015621042"),
            "the local mod folds into the property"
        );
    }

    #[test]
    fn a_one_mod_row_places_its_tier_among_its_familys() {
        let filters = quick(&ring());
        let info = row(&filters, "explicit.stat_3372524247")
            .tier_info
            .expect("fire resistance tiers");
        // T4 of 8 fire resistance tiers on a ring (FireResist5, item level 48, 26-30); T1 needs
        // item level 82, out of an item level 80 ring's reach; every tier together 6-45.
        assert_eq!(
            info,
            TierInfo {
                current: 4,
                count: 8,
                best_available: 2,
                min_level: 48,
                tier_floor: Some(26.0),
                range: Some((6.0, 45.0)),
            }
        );
        // A total spans mods, which no tier describes.
        assert_eq!(row(&filters, "pseudo.pseudo_total_life").tier_info, None);
    }

    #[test]
    fn a_unique_scores_nothing_and_starts_with_every_shown_row() {
        let mut unique = ring();
        unique.rarity = Some(ItemRarity::Unique);
        assert_eq!(
            SearchProfile::default_for(&unique),
            SearchProfile::ExactMatch
        );
        let filters = build_filters(
            &unique,
            SearchProfile::default_for(&unique),
            &StatCatalog::default(),
        );
        assert!(filters.iter().all(|filter| filter.score.is_none()));
        assert!(filters.iter().all(|filter| filter.enabled != filter.hidden));
        // A unique gets no weighted sum: its rarity is searched as a mod row.
        assert!(filters.iter().all(|filter| !filter.weighted_sum));
        assert!(row(&filters, "explicit.stat_3917489142").enabled);
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

        let filters = build_filters(&ring, SearchProfile::QuickPrice, &russian);

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
        let merged = quick(&jewel)
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
    fn a_waystone_searches_its_tier_but_not_its_modifiers() {
        // The live T16 waystone: its T1 map modifiers were searched on top of the tier and the
        // search found nothing -- a map modifier's tier only says how hard the map is.
        let damage = tiered(
            Prefix,
            1,
            stat_with_id(
                Some("explicit.stat_1890519597"),
                "#% increased Monster Damage",
                30.0,
            ),
        );
        let mut waystone = item(ItemRarity::Rare, "map.waystone", vec![damage]);
        waystone.waystone = Some(WaystoneProperties {
            tier: Some(16),
            ..Default::default()
        });

        let filters = quick(&waystone);

        let tier = row(&filters, "map_filters.map_tier");
        let roll = tier.roll.as_ref().expect("tier roll");
        assert!(tier.enabled);
        assert_eq!((roll.min, roll.max), (Some(16.0), Some(16.0)));
        let monster_damage = row(&filters, "explicit.stat_1890519597");
        assert!(!monster_damage.enabled);
        assert_eq!(monster_damage.score, None, "a map item ranks nothing");
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

        let filters = build_filters(&item, SearchProfile::QuickPrice, &catalog);

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
        let flask = ParsedStat {
            min: 10.0,
            max: 20.0,
            ..stat_with_id(
                Some("explicit.stat_644456512"),
                "#% reduced Flask Charges used",
                20.0,
            )
        };
        let item = ParsedItem {
            mods: vec![modifier(
                ModifierType::Explicit,
                None,
                vec![surrounded, ring, flask],
            )],
            ..Default::default()
        };

        let bounds = |profile| {
            let filters = build_filters(&item, profile, &StatCatalog::default());
            [
                "explicit.stat_2267564181",
                "explicit.stat_3642528642",
                "explicit.stat_644456512",
            ]
            .map(|trade_id| {
                let roll = row(&filters, trade_id).roll.clone().expect("roll");
                (roll.min, roll.max)
            })
        };

        // At most 4 more (-4); a listing at -2 (2 fewer) is worse and stays out. Broad lets
        // -4 + 0.4 = -3.6 through, rounding to -4 all the same; 20 - 2 = 18 of flask charges.
        assert_eq!(
            bounds(SearchProfile::QuickPrice),
            [
                (None, Some(-4.0)),
                (Some(2.0), Some(2.0)),
                (Some(20.0), None)
            ]
        );
        assert_eq!(
            bounds(SearchProfile::Broad),
            [
                (None, Some(-4.0)),
                (Some(2.0), Some(2.0)),
                (Some(18.0), None)
            ]
        );
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

        let filters = quick(&item);
        let row = &filters[0];
        let roll = row.roll.as_ref().expect("roll");

        assert!(row.inverted);
        assert_eq!(
            row.display_text,
            "Для окружения требуется на # врага меньше"
        );
        // "At most 4 more" in the catalog's words is "at least 4 fewer" in the item's.
        assert_eq!((roll.value, roll.min, roll.max), (4.0, Some(4.0), None));
        assert_eq!(roll.bound, RollBound::Higher);
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
            quick(&item)
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

        let filters = quick(&item);

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
        assert_eq!(merged.tier_info, None, "two mods, no one tier");
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

        let filters = quick(&item);

        let filter = filters
            .iter()
            .find(|f| f.trade_ids == vec!["explicit.stat_unscalable".to_owned()])
            .expect("unscalable filter should still be produced");
        assert!(filter.roll.is_none());
        // A mod without a tier 1, without a tag or a roll nothing more: under Quick Price's 3.
        assert_eq!(filter.score, Some(1.0));
        assert!(!filter.enabled);
    }
}
