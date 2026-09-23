//! `ParsedItem` and everything it's built from -- the output shape of `item-parser`'s real
//! clipboard-text parser, and the input shape `stat-filters`/`trade-client` consume for the
//! Price Check feature. Field shapes are ported field-for-field from the real, working
//! reference's `ParsedItem` interface (`exiled-exchange-2/renderer/src/parser/ParsedItem.ts:42-135`,
//! cited exhaustively in `.tmp/research/ItemTextFormat.md` section 3) -- cited as the format's
//! ground truth (dictated by the PoE2 game client itself), never as code to copy.
//!
//! Two deliberate departures from the reference, both already settled by the Price Check plan's
//! own architecture decision (see `data-pipeline/SPIKE_FINDINGS.md` and this crate's own module
//! doc comment): no `info: BaseType` field. The reference resolves a parsed name/base-type
//! string against its own bundled local item database to get a canonical `BaseType` record
//! (icon, tags, craftable/unique/map/gem/armour info); this project deliberately ships no local
//! item database (every stat/item catalog is trade-API-sourced, see the plan's Context section),
//! so `ParsedItem` keeps the raw parsed `name`/`base_type` strings directly instead, and
//! `category: Option<ItemCategory>` (resolved from `item-parser`'s own `Item Class:` table, not
//! a local database) stands in for classification. `statsByType: StatCalculated[]` (the
//! reference's pre-grouped/aggregated stat view) has no field here either: `stat-filters`'
//! `build_filters` computes the equivalent grouping on demand straight from `mods`, rather than
//! caching it on the parsed item -- nothing in this workspace needs it stored.
//!
//! Every other field of the reference interface is ported here even where this plan's own
//! `item-parser` steps have no parser function that populates it yet (`heist`, `trials`,
//! `logbook_area_mods`, `sentinel_charge`, `talisman_tier`, `area_level`, `base_percentile`) --
//! per the plan's own instruction, this interface is the exhaustive, verified reference for
//! everything PoE2's real clipboard text can structurally contain, not a list to trim down to
//! only what's parsed today. Fields with no populating parser simply stay at their `Default`
//! value (`None`/empty/`false`) after parsing; that is expected, not a bug, until a future plan
//! adds the parser for that item type. `ParsedItem` derives `Default` specifically so
//! `item-parser`'s ~15 independent, ordered section-parser functions can each mutate only the
//! 1-2 fields they own, matching the reference's own `parsers` array architecture
//! (`Parser.ts:132-187`).

use serde::{Deserialize, Serialize};

use crate::ItemCategory;

/// `ItemRarity` -- `ParsedItem.ts:6-11`. Currency/Gem/DivinationCard/Quest are not rarities in
/// the reference (they only ever set `category`); mirrored here by simply leaving `rarity: None`
/// for those categories rather than adding fake enum variants for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemRarity {
    Normal,
    Magic,
    Rare,
    Unique,
}

/// Where a stat/mod came from. `trade_key()` is the exact string used as the trade catalog's
/// per-mod-type namespace and as the search-request id prefix (verified live: `explicit.stat_*`,
/// `implicit.stat_*`, `fractured.stat_*`, `enchant.stat_*`, `rune.stat_*`, `desecrated.stat_*`,
/// `crafted.stat_*`, `pseudo.pseudo_*` -- confirmed via a real `curl` of
/// `/api/trade2/data/stats`; `sanctum`/`skill` confirmed via `dataParser`'s own `MOD_TYPES`
/// constant, `.tmp/research/StatIdMapping.md` section 3). `Augment`/`AddedAugment` (PoE2's
/// socket-rune system) both map to `"rune"`, not `"augment"` -- confirmed via
/// `exiled-exchange-2`'s `dataParser/src/services/nd_builder_service.py`'s `AUGMENT -> RUNE`
/// output-key special case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModifierType {
    Pseudo,
    Explicit,
    Implicit,
    Crafted,
    Enchant,
    Scourge,
    Necropolis,
    Veiled,
    Fractured,
    Augment,
    AddedAugment,
    Sanctum,
    Desecrated,
    Skill,
}

impl ModifierType {
    pub fn trade_key(&self) -> &'static str {
        match self {
            ModifierType::Pseudo => "pseudo",
            ModifierType::Explicit => "explicit",
            ModifierType::Implicit => "implicit",
            ModifierType::Crafted => "crafted",
            ModifierType::Enchant => "enchant",
            ModifierType::Scourge => "scourge",
            ModifierType::Necropolis => "necropolis",
            ModifierType::Veiled => "veiled",
            ModifierType::Fractured => "fractured",
            ModifierType::Augment | ModifierType::AddedAugment => "rune",
            ModifierType::Sanctum => "sanctum",
            ModifierType::Desecrated => "desecrated",
            ModifierType::Skill => "skill",
        }
    }
}

/// Which affix slot a mod occupies -- the bracket header's `Prefix Modifier`/`Suffix Modifier`
/// (`Префикс`/`Суффикс` on the Russian client), EE2's `ModifierInfo.generation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModGeneration {
    Prefix,
    Suffix,
}

/// Parsed from a bracketed `{ Prefix Modifier "Name" (Tier: N) — tag, tag }` line
/// (`advanced-mod-desc.ts:parseModInfoLine`) or inferred from a flat line's trailing
/// `" (rune)"`/`" (enchant)"`/etc. suffix (`advanced-mod-desc.ts:parseModType`) when no bracket
/// is present. `generation`/`name`/`tier`/`rank`/`tags` are only ever populated by the bracket
/// form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModifierInfo {
    pub modifier_type: ModifierType,
    /// `None` for anything that isn't an explicit prefix/suffix (implicits, runes, enchants...).
    #[serde(default)]
    pub generation: Option<ModGeneration>,
    pub name: Option<String>,
    pub tier: Option<u32>,
    pub rank: Option<u32>,
    pub tags: Vec<String>,
}

/// One numeric stat line, after roll-placeholder resolution against a `StatCatalog` (step 4).
/// `stat_id` is `None` for a line that matched a known TYPE marker but no known stat text
/// (mirrors the reference's `unknownModifiers`, but scoped per-stat here rather than
/// item-level, since a mod block genuinely can mix resolved and unresolved lines).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedStat {
    pub stat_id: Option<String>,
    pub text: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub dp: bool,
    pub unscalable: bool,
    /// The item's own wording, numbers templated to `#`, when it words the stat the other way
    /// round from the catalog's `text` (`#% reduced Attribute Requirements` for `#% increased
    /// Attribute Requirements`): `value`, `min` and `max` are negated into the catalog's terms,
    /// and this is how the item says them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negated_text: Option<String>,
    /// The item's own wording, numbers templated to `#`, when it prints the stat another way than
    /// the catalog's `text` without counting it the other way round (`#% шанс наложения
    /// оцепенения при нанесении удара` for `Накладывает оцепенение при нанесении удара`): the
    /// roll is in the catalog's terms already, and this is how the item says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printed_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedModifier {
    pub info: ModifierInfo,
    pub stats: Vec<ParsedStat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Requirements {
    pub level: u32,
    pub str: u32,
    pub dex: u32,
    pub int: u32,
}

/// Rune/soul-core sockets on gear (letter `S` in clipboard text). Distinct from `GemSockets`
/// (letter `G`, Meta/Skill Gem's own skill-gem sockets) -- confirmed two different concepts,
/// two different parsers in the reference (`Parser.ts:830` vs `Parser.ts:886`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct AugmentSockets {
    pub empty: u32,
    pub current: u32,
    pub normal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct GemSockets {
    pub number: u32,
    pub linked: Option<u32>,
    pub white: u32,
}

/// `ItemInfluence` -- `ParsedItem.ts:13-20`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Influence {
    Crusader,
    Elder,
    Hunter,
    Redeemer,
    Shaper,
    Warlord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ElementKind {
    Fire,
    Cold,
    Lightning,
    Chaos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlightedKind {
    Blighted,
    BlightRavaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct WaystoneProperties {
    pub tier: Option<u32>,
    pub pack_size: Option<i32>,
    pub item_rarity: Option<i32>,
    pub monster_rarity: Option<i32>,
    pub drop_chance: Option<i32>,
    pub revives: Option<i32>,
    pub magic_monsters: Option<i32>,
    pub rare_monsters: Option<i32>,
    pub gold: Option<i32>,
    pub effectiveness: Option<i32>,
    pub blighted: Option<BlightedKind>,
}

/// Heist Blueprint-only info -- `ParsedItem.ts`'s `heist?: { wingsRevealed?, target? }`. No
/// `item-parser` step in this plan populates this (no Heist Blueprint fixture exists to verify
/// against, per `.tmp/research/ItemTextFormat.md` section 8); kept for interface parity, stays
/// `None` until a future plan adds the parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HeistTarget {
    Enchants,
    Trinkets,
    Gems,
    Replicas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct HeistInfo {
    pub wings_revealed: Option<u32>,
    pub target: Option<HeistTarget>,
}

/// Trial/Ultimatum-only info -- `ParsedItem.ts`'s `trials?: { numberOfTrials?, ultimatumHint? }`.
/// Same status as `HeistInfo`: no populating parser yet, kept for interface parity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UltimatumHint {
    Victorious,
    Cowardly,
    Deadly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct TrialsInfo {
    pub number_of_trials: Option<u32>,
    pub ultimatum_hint: Option<UltimatumHint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ParsedItem {
    pub rarity: Option<ItemRarity>,
    /// Resolved trade category: `id` = trade filter string (e.g. `"weapon.crossbow"`),
    /// `display_name` = the item's own localized `Item Class:` text. Reuses the existing
    /// `ItemCategory{id, display_name}` shape as-is (step 2 builds the resolving table).
    pub category: Option<ItemCategory>,
    pub name: String,
    pub base_type: Option<String>,
    pub item_level: Option<u32>,
    /// PoE1-legacy Talisman tier and Incursion/Logbook-style area level. No `item-parser` step
    /// in this plan parses either (no confirmed live PoE2 marker for Talismans; area level is
    /// only relevant to the also-unparsed `logbook_area_mods`); kept for interface parity.
    pub talisman_tier: Option<u32>,
    pub area_level: Option<u32>,
    pub armour: Option<u32>,
    pub evasion: Option<u32>,
    pub energy_shield: Option<u32>,
    pub runic_ward: Option<u32>,
    pub block_chance: Option<u32>,
    /// UI-only in the reference (`calc-base.ts`'s `calcBasePercentile`): where this item's base
    /// AR/EV/ES roll sits within its base type's possible range. Requires a local base-item
    /// roll-range table this architecture deliberately doesn't ship (see the plan's Context
    /// section); always `None` here.
    pub base_percentile: Option<f64>,
    pub weapon_crit: Option<f64>,
    pub weapon_aps: Option<f64>,
    pub weapon_reload_time: Option<f64>,
    pub weapon_physical: Option<(u32, u32)>,
    pub weapon_elemental: Vec<(ElementKind, u32, u32)>,
    pub spirit: Option<u32>,
    pub quality: Option<u32>,
    pub quality_type: Option<String>,
    pub sockets: Option<AugmentSockets>,
    pub gem_sockets: Option<GemSockets>,
    pub gem_level: Option<u32>,
    pub waystone: Option<WaystoneProperties>,
    pub stack_size: Option<(u32, u32)>,
    pub is_unidentified: bool,
    pub unidentified_tier: Option<u32>,
    pub is_corrupted: bool,
    pub is_mirrored: bool,
    pub is_fractured: bool,
    pub is_synthesised: bool,
    pub is_veiled: bool,
    pub is_unmodifiable: bool,
    pub is_sanctified: bool,
    /// PoE2-specific alternate-art unique variant (`ParsedItem.ts`'s `isFoil?`).
    pub is_foil: bool,
    pub influences: Vec<Influence>,
    /// Expedition Logbook per-area mod lists (`ParsedItem.ts`'s `logbookAreaMods?`). No parser
    /// in this plan populates this; stays empty.
    pub logbook_area_mods: Vec<Vec<ParsedModifier>>,
    /// Sentinel item charge count. No parser in this plan populates this; stays `None`.
    pub sentinel_charge: Option<u32>,
    pub requirements: Option<Requirements>,
    pub mods: Vec<ParsedModifier>,
    /// Lines that matched a mod-type marker but resolved zero known stats at all (every
    /// `ParsedStat.stat_id` in that block is `None`) -- kept so the UI can show "N unrecognized
    /// modifiers" without failing the whole parse.
    pub unknown_mods: Vec<(String, ModifierType)>,
    pub heist: Option<HeistInfo>,
    pub trials: Option<TrialsInfo>,
    pub note: Option<String>,
    pub raw_text: String,
}

/// One trade-API stat entry, verbatim from `GET /api/trade2/data/stats` (verified live:
/// `{"id":"explicit.stat_4080418644","text":"# to Strength","type":"explicit"}`). `id` is used
/// directly, unmodified, as a search-request filter id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeStat {
    pub id: String,
    pub text: String,
    pub mod_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StatCatalog {
    pub stats: Vec<TradeStat>,
}
