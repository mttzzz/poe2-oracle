//! Search profiles: PoE Overlay II's evaluate profiles (`main/main.js` ~218905, applied in
//! `renderer/ingame-evaluate.js` ~43359 and `9505.bundle.js`'s `gQ` ~69873). A profile picks
//! the rows a search starts with and sets every row's bounds from its roll.

use poe2_domain::{ItemRarity, ParsedItem};

use crate::{FilterTag, RollBound, SearchFilter};

/// How a search reads the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchProfile {
    /// "Quick Price": the properties and mods scoring at least 3 (`rank`), best first, at most
    /// four, of which at most two weighted sums; every minimum the item's own roll.
    QuickPrice,
    /// "Exact Match": every row not hidden -- mods, pseudo totals, free slots -- and the
    /// properties Quick Price would search; every minimum the item's own roll.
    ExactMatch,
    /// "Broad (-10%)": the rows the player had checked, each minimum 10% below the roll.
    Broad,
    /// "Crafting Base": the implicit, fractured and granted-skill rows, and the item level,
    /// searched among the item's own base type (`searches_base_type`).
    CraftingBase,
}

impl SearchProfile {
    /// The profile a price check starts with. PoE Overlay II sorts an item into one of its
    /// categories (`ingame-evaluate.js` ~51450) -- a modifiable base (a Normal item, a fractured
    /// one, exceptional quality, more rune sockets than its base has), a relic or flask, a
    /// unique, an unmodifiable item (corrupted, mirrored, sanctified, Unmodifiable; never a
    /// waystone), anything else -- and the owner's defaults give non-uniques and bases Quick
    /// Price, uniques, unmodifiable items, relics and flasks (charms among them) Exact Match.
    pub fn default_for(item: &ParsedItem) -> Self {
        let category = item.category.as_ref().map_or("", |category| &category.id);
        let unmodifiable =
            item.is_corrupted || item.is_mirrored || item.is_sanctified || item.is_unmodifiable;
        let base = !unmodifiable
            && (item.is_fractured
                || item.rarity == Some(ItemRarity::Normal)
                || item.quality.is_some_and(|quality| quality > 20)
                || item
                    .sockets
                    .is_some_and(|sockets| sockets.current > sockets.normal));
        if base {
            Self::QuickPrice
        } else if category == "sanctum.relic"
            || category.starts_with("flask")
            || item.rarity == Some(ItemRarity::Unique)
            || (unmodifiable && category != "map.waystone")
        {
            Self::ExactMatch
        } else {
            Self::QuickPrice
        }
    }

    /// Whether the search goes by the item's own base type rather than its category: Crafting
    /// Base's `includeTypeLine`.
    pub fn searches_base_type(self) -> bool {
        self == Self::CraftingBase
    }

    /// The share of a roll a searched minimum may fall below it: its `minRange`.
    fn range(self) -> f64 {
        match self {
            Self::Broad => 0.1,
            Self::QuickPrice | Self::ExactMatch | Self::CraftingBase => 0.0,
        }
    }
}

/// Sets every row's bounds to `profile`'s and leaves the checkboxes as they are: switching to
/// Broad keeps what the player had checked (its `copySelectedFromPrevious`), and switching back
/// restores the item's own minimums.
pub fn apply_profile(filters: &mut [SearchFilter], profile: SearchProfile) {
    let range = profile.range();
    for filter in filters {
        let property = filter.tag == FilterTag::Property;
        if let Some(roll) = &mut filter.roll {
            (roll.min, roll.max) = bounds(roll.value, roll.bound, range, property);
        }
    }
}

/// `value`'s search bounds under a profile's `range`, as `gQ` sets them. A ranged bound moves
/// by `|value| * range` and is rounded the way it rounds: a mod's to the nearest whole number,
/// or hundredth for a fractional value (JavaScript's `Math.round`, halves up); a property's
/// minimum down and maximum up.
fn bounds(value: f64, bound: RollBound, range: f64, property: bool) -> (Option<f64>, Option<f64>) {
    let offset = value.abs() * range;
    let whole = value.fract() == 0.0;
    let round = |raw: f64, up: bool| {
        let scale = if whole { 1.0 } else { 100.0 };
        let scaled = raw * scale;
        // A hair of slack absorbs float noise: 100 * 2.07 is 206.99999999999997.
        let rounded = match (property, up) {
            (false, _) => (scaled + 0.5).floor(),
            (true, false) => (scaled + 1e-9).floor(),
            (true, true) => (scaled - 1e-9).ceil(),
        };
        rounded / scale
    };
    match bound {
        RollBound::Higher => (Some(round(value - offset, false)), None),
        RollBound::Lower => (None, Some(round(value + offset, true))),
        RollBound::AtLeast => (Some(value), None),
        RollBound::AtMost => (None, Some(value)),
        RollBound::Exactly => (Some(value), Some(value)),
    }
}

#[cfg(test)]
mod tests {
    use poe2_domain::{AugmentSockets, ItemCategory};

    use super::*;

    #[test]
    fn bounds_move_by_the_profiles_range_and_round_as_poe_overlay_ii_rounds() {
        let quick = |value, bound, property| bounds(value, bound, 0.0, property);
        let broad = |value, bound, property| bounds(value, bound, 0.1, property);
        // Quick Price searches the item's own roll.
        assert_eq!(quick(45.0, RollBound::Higher, false), (Some(45.0), None));
        assert_eq!(quick(10.5, RollBound::Higher, false), (Some(10.5), None));
        assert_eq!(quick(2.07, RollBound::Higher, true), (Some(2.07), None));
        // Broad: 45 - 4.5 = 40.5 rounds up to 41 for a mod, down to 40 for a property.
        assert_eq!(broad(45.0, RollBound::Higher, false), (Some(41.0), None));
        assert_eq!(broad(45.0, RollBound::Higher, true), (Some(40.0), None));
        // A fractional value keeps two places: 10.5 - 1.05 = 9.45.
        assert_eq!(broad(10.5, RollBound::Higher, false), (Some(9.45), None));
        // A negative roll moves by its size, still downward.
        assert_eq!(broad(-10.0, RollBound::Higher, false), (Some(-11.0), None));
        // Lower is better: the maximum moves up, -4 + 0.4 = -3.6 rounding to -4.
        assert_eq!(broad(-4.0, RollBound::Lower, false), (None, Some(-4.0)));
        assert_eq!(broad(0.6, RollBound::Lower, true), (None, Some(0.66)));
        // Fixed and exact rolls ignore the range.
        assert_eq!(broad(3.0, RollBound::AtLeast, false), (Some(3.0), None));
        assert_eq!(broad(3.0, RollBound::AtMost, false), (None, Some(3.0)));
        assert_eq!(
            broad(16.0, RollBound::Exactly, true),
            (Some(16.0), Some(16.0))
        );
    }

    fn item(rarity: ItemRarity, category: &str) -> ParsedItem {
        ParsedItem {
            rarity: Some(rarity),
            category: Some(ItemCategory {
                id: category.to_owned(),
                display_name: String::new(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn non_uniques_and_bases_start_with_quick_price_the_rest_with_exact_match() {
        use SearchProfile::{ExactMatch, QuickPrice};
        let default = |item: &ParsedItem| SearchProfile::default_for(item);
        assert_eq!(
            default(&item(ItemRarity::Rare, "accessory.ring")),
            QuickPrice
        );
        assert_eq!(default(&item(ItemRarity::Magic, "weapon.wand")), QuickPrice);
        assert_eq!(
            default(&item(ItemRarity::Unique, "armour.chest")),
            ExactMatch
        );
        assert_eq!(default(&item(ItemRarity::Magic, "flask.life")), ExactMatch);
        assert_eq!(default(&item(ItemRarity::Rare, "flask.charm")), ExactMatch);
        assert_eq!(
            default(&item(ItemRarity::Rare, "sanctum.relic")),
            ExactMatch
        );

        let mut corrupted = item(ItemRarity::Rare, "armour.boots");
        corrupted.is_corrupted = true;
        assert_eq!(default(&corrupted), ExactMatch);
        // A corrupted waystone still goes by its mods.
        let mut waystone = item(ItemRarity::Rare, "map.waystone");
        waystone.is_corrupted = true;
        assert_eq!(default(&waystone), QuickPrice);

        // A base goes by its mods, even where its kind would be matched exactly: a white
        // flask, a fractured charm, a magic flask with a rune socket more than its base has.
        assert_eq!(default(&item(ItemRarity::Normal, "flask.life")), QuickPrice);
        let mut fractured = item(ItemRarity::Rare, "flask.charm");
        fractured.is_fractured = true;
        assert_eq!(default(&fractured), QuickPrice);
        let mut extra = item(ItemRarity::Magic, "flask.mana");
        extra.sockets = Some(AugmentSockets {
            empty: 0,
            current: 2,
            normal: 1,
        });
        assert_eq!(default(&extra), QuickPrice);
    }
}
