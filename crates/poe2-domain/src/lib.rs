//! Plain data types shared across the price-check pipeline: `Item`, `Mod`, `StatFilter`,
//! `Currency`, `ItemCategory`. Zero crate-local dependencies and no I/O -- every other crate in
//! this workspace that needs these shapes (`item-parser`, `trade-client`, `crates/poe2-oracle`)
//! depends on this one, never the reverse.
//!
//! Field shapes are informed by two real, verified sources rather than guessed:
//!
//! 1. The request/response structs already proven against the live trade API in the original
//!    POC (`crates/poe2-oracle/examples/trade_api.rs`, now `trade-client`'s `FetchedItem`) --
//!    deliberately **not** duplicated here. `FetchedItem` is a trade-API *listing* shape (an
//!    item plus its price and seller); `Item` here is the item's own definition, a different
//!    concept serving different callers (`item-parser`'s clipboard-text output,
//!    `data-pipeline`'s structural extraction). Only what's genuinely new lives in this crate.
//! 2. `crates/data-pipeline`'s localization validation spike
//!    (`SPIKE_FINDINGS.md`), which settled `Mod`/`StatFilter`'s text-storage question with real
//!    data rather than the plan's two hypothetical branches: PoE2's real `Mods.datc64` (73
//!    columns) and `Stats.datc64` (19 columns) carry **zero** localized columns between them --
//!    checked exhaustively against the live schema, not sampled. Mod/stat *display* text, in
//!    every language including English, therefore always comes from `trade-client`'s
//!    `/api/trade2/data/stats` endpoint, never from local extraction. `Mod`/`StatRoll` below
//!    hold exactly the structural fields the spike found real, verified values for
//!    (`Stat1`..`Stat8`/`Stat1Value`..`Stat8Value` collapsed into `Vec<StatRoll>`, `Tags`,
//!    `Level`); `StatText` holds the trade-API-sourced side, kept as a clearly separate type
//!    rather than folded into `Mod` itself, matching how the two are genuinely populated from
//!    two different sources at two different times.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A display language for stat/mod text. Scoped to what this project actually ships (the
/// feasibility POC's Cyrillic-rendering capability was specifically about English+Russian) --
/// the trade API supports more languages than this, but nothing in this project needs them yet;
/// extending this enum is the follow-up plan's problem to have if it ever arises, not a
/// speculative one to solve now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    English,
    Russian,
}

/// One stat a mod grants, and its roll range at the mod's tier. `stat_id` is the stat's internal
/// identifier (`Stats.datc64`'s own `Id` column, e.g. `"additional_strength"`) -- an
/// programmer-facing id, not display text; see the module doc comment for why no text lives here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatRoll {
    pub stat_id: String,
    pub min: i32,
    pub max: i32,
}

/// A stat's localized display template (e.g. `"# to Strength"` English / `"# к силе"` Russian),
/// sourced from `trade-client`'s `/api/trade2/data/stats` -- confirmed live and reachable for
/// both languages during the validation spike. Never populated from local `.datc64` extraction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatText {
    pub stat_id: String,
    pub text: HashMap<Language, String>,
}

/// One mod, as defined by PoE2's real `Mods.datc64` table (field shapes verified against a live
/// PoE2 install -- see `crates/data-pipeline/SPIKE_FINDINGS.md`). `name` is the mod's real but
/// partial name suffix/prefix (`Mods.datc64`'s own `Name` column, e.g. `"of the Wrestler"`) --
/// not the full stat-description sentence a player sees, which isn't stored locally at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mod {
    /// `Mods.datc64`'s own `Id` column (e.g. `"Strength2"`), not display text.
    pub id: String,
    pub name: String,
    pub level: i32,
    pub stats: Vec<StatRoll>,
    pub tags: Vec<String>,
}

/// A price-check search constraint on one stat: an id to match, and an optional min/max range.
/// `text` is populated from `trade-client` when available (for rendering a human-readable filter
/// row), never required for the filter to function -- matching still happens on `stat_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatFilter {
    pub stat_id: String,
    pub min: Option<i32>,
    pub max: Option<i32>,
    pub text: Option<StatText>,
}

/// A currency type usable in a trade listing price (e.g. Chaos Orb, Divine Orb). Deliberately not
/// an exhaustive enum of every PoE2 currency -- that catalog is real game data (a `.datc64` table
/// of its own), not a fixed set this crate should hardcode and have to keep in sync by hand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Currency {
    /// The trade API's own currency id (e.g. `"divine"`, matching `FetchedItem`'s
    /// `price: Option<(f64, String)>` currency string in `trade-client`).
    pub id: String,
    pub display_name: String,
}

/// An item category, as used by the trade API's category filter (e.g. `"weapon.crossbow"`, the
/// exact string the POC's `trade_api.rs` proved against `trade2`). A thin id/name pair, not an
/// exhaustive enum of PoE2's full category tree -- that tree is itself real game data, not a
/// fixed set worth hardcoding here ahead of the feature that will actually need to browse it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemCategory {
    pub id: String,
    pub display_name: String,
}

/// An item's own definition -- distinct from `trade-client::FetchedItem` (a trade *listing*: an
/// item plus its price and seller). Populated either by `item-parser` (from real clipboard item
/// text) or by `data-pipeline`'s structural extraction; never by `trade-client` directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub type_line: String,
    pub category: Option<ItemCategory>,
    pub mods: Vec<Mod>,
}
