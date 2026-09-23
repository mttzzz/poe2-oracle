//! Which way a mod stat's roll is better, for the few stats where it isn't "higher": EE2's
//! `StatBetter` -- `stats.ndjson`'s `better`, -1 (`NegativeRoll`) or 0 (`NotComparable`) --
//! keyed by trade stat hash, the id past its `explicit.`/`implicit.`/... domain, since EE2 gives
//! a stat one direction whatever its domain. Taken on 2026-09-23 from EE2's
//! `renderer/public/data/en/stats.ndjson` (commit 0c9c9ba017b1b9b0a3779fe7f8700255753e2e39, MIT:
//! see `crates/poe2-oracle/assets/data/NOTICE`): 15 of its 21 `NegativeRoll` hashes and its 10
//! `NotComparable` ones, of 2526 stats.
//!
//! EE2 rolls a stat in the game's own terms, this crate in the trade catalog's: the parser reads
//! an item line against the catalog's text (`item-parser`'s `catalog_match`), negating only a
//! line worded the other way round. Where the catalog words a stat the good way round -- `#%
//! reduced Flask Charges used`, not the game's `increased` -- a bigger number is a better roll
//! here, so EE2's six such `NegativeRoll` stats are left out: both charm and flask charges
//! used, ignite, chill and shock on you, and `#% less Damage taken if you have not been Hit
//! Recently` (EE2 flags the first three `trade.inverted`; the live catalog words all six so).

use crate::SearchFilterRoll;

/// Stats a lower roll is better on (`NegativeRoll`). "Require 4 fewer enemies to be Surrounded"
/// is `Require # additional enemies to be Surrounded` at -4, and a listing at -2 is worse.
const LOWER: [&str; 15] = [
    "stat_3423694372", // #% chance to be inflicted with Bleeding when Hit
    "stat_3639275092", // #% increased Attribute Requirements
    "stat_388617051",  // #% increased Charges per use
    "stat_3691641145", // #% increased Damage taken
    "stat_1692879867", // #% increased Duration of Bleeding on You
    "stat_2920970371", // #% increased Duration of Curses on you
    "stat_3096446459", // #% increased Merchant Prices
    "stat_2590797182", // #% increased Movement Speed Penalty from using Skills while moving
    "stat_924253255",  // #% increased Slowing Potency of Debuffs on You
    "stat_978111083",  // #% increased Slowing Potency of Debuffs on You (sanctum)
    "stat_1345835998", // Deferring Favours at Ritual Altars in Map costs #% increased Tribute
    "stat_2267564181", // Require # additional enemies to be Surrounded
    "stat_2282052746", // Rerolling Favours at Ritual Altars in Map costs #% increased Tribute
    "stat_396200591",  // Skills have # seconds to Cooldown
    "stat_2905515354", // You take #% of damage from Blocked Hits
];

/// Stats whose number compares to nothing (`NotComparable`): it names something -- a timeless
/// jewel's legend, the ring a jewel affects -- and is searched exactly.
const EXACT: [&str; 10] = [
    "stat_2954116742",    // Allocates #
    "stat_3418580811|21", // Remembrancing # songworthy deeds by the line of Vorana
    "stat_3418580811|22", // Remembrancing # songworthy deeds by the line of Medved
    "stat_3418580811|23", // Remembrancing # songworthy deeds by the line of Olroth
    "stat_3418580811|24", // Glorifying the defilement of # souls in tribute to Amanamu
    "stat_3418580811|25", // Glorifying the defilement of # souls in tribute to Kulemak
    "stat_3418580811|26", // Glorifying the defilement of # souls in tribute to Kurgal
    "stat_3418580811|27", // Glorifying the defilement of # souls in tribute to Tecrod
    "stat_3418580811|28", // Glorifying the defilement of # souls in tribute to Ulaman
    "stat_3642528642",    // Only affects Passives in # Ring
];

/// Presets `roll`'s search bounds the way `trade_id`'s stat is better, as EE2's
/// `filterFillMinMax` (`create-stat-filters.ts:597-613`) does: `build_roll` leaves the lower
/// bound set, right for the stats a higher roll is better on; a stat a lower roll is better on
/// keeps listings at most the tolerance above the item's roll instead, and a stat with nothing
/// to compare is searched at exactly the item's roll.
pub(crate) fn orient(roll: &mut SearchFilterRoll, trade_id: &str) {
    let hash = trade_id.split_once('.').map_or(trade_id, |(_, hash)| hash);
    if LOWER.contains(&hash) {
        roll.min = None;
        roll.max = Some(roll.default_max);
    } else if EXACT.contains(&hash) {
        roll.default_min = roll.value;
        roll.default_max = roll.value;
        roll.min = Some(roll.value);
        roll.max = Some(roll.value);
    }
}
