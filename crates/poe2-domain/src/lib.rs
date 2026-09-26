//! Plain data types shared across the price-check pipeline: `ParsedItem` and everything it's
//! built from (see [`parsed_item`]), the trade API's stat catalog (`StatCatalog`) and
//! `ItemCategory`; and the slot where a data pack's copy of a built-in game table waits for the
//! table's first read ([`pack_table`]). Zero crate-local dependencies and no I/O -- every other
//! crate in this workspace that needs these shapes (`item-parser`, `stat-filters`,
//! `trade-client`, `crates/poe2-oracle`) depends on this one, never the reverse.

use serde::{Deserialize, Serialize};

/// An item category, as used by the trade API's category filter (e.g. `"weapon.crossbow"`). A
/// thin id/name pair, not an exhaustive enum of PoE2's full category tree -- that tree is itself
/// real game data, not a fixed set worth hardcoding here ahead of the feature that will actually
/// need to browse it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemCategory {
    pub id: String,
    pub display_name: String,
}

pub mod parsed_item;
pub use parsed_item::*;

pub mod pack_table;
