//! A mod row's roll slider: a track from the lowest roll the row's mod family can have on this
//! kind of item to the highest (`stat_filters::TierInfo::range`), a mark at the item's own roll,
//! and a handle that sets the row's search bound -- its minimum, or its maximum where a lower
//! roll is better. Also the bound the panel's «минимум тира» gives a row. Pure, so the native
//! test pass covers it; `price_check` writes the bounds into the rows' min/max boxes, and
//! `ui::panel::filters` draws the slider.

use stat_filters::{RollBound, SearchFilter};

/// Which search bound a slider's handle sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// The minimum: a higher roll is better.
    Min,
    /// The maximum: a lower roll is better.
    Max,
}

/// A row's slider: the rolls its track spans and the bound its handle sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slider {
    /// The lowest roll, at the track's left end.
    pub low: f64,
    /// The highest roll, at the track's right end.
    pub high: f64,
    pub handle: Handle,
    /// The row's value has decimals: the handle moves by hundredths rather than whole numbers.
    dp: bool,
}

impl Slider {
    /// `filter`'s slider: a row with a roll and a range of rolls across its family's tiers wider
    /// than one value. `None` for a row without tiers, and for a number searched exactly -- a
    /// jewel's legend, a map tier -- which has no better side to bound.
    pub fn of(filter: &SearchFilter) -> Option<Slider> {
        let roll = filter.roll.as_ref()?;
        let (low, high) = filter.tier_info?.range?;
        let handle = match roll.bound {
            RollBound::Higher | RollBound::AtLeast => Handle::Min,
            RollBound::Lower | RollBound::AtMost => Handle::Max,
            RollBound::Exactly => return None,
        };
        (high > low).then_some(Slider {
            low,
            high,
            handle,
            dp: roll.dp,
        })
    }

    /// Where `value` sits along the track: 0 at the lowest roll, 1 at the highest, clamped to
    /// the track.
    pub fn fraction(&self, value: f64) -> f64 {
        ((value - self.low) / (self.high - self.low)).clamp(0.0, 1.0)
    }

    /// Where the handle sits for the row's bound, `None` when the search leaves that side open:
    /// then at the end that admits every roll -- the lowest for a minimum, the highest for a
    /// maximum.
    pub fn handle_fraction(&self, bound: Option<f64>) -> f64 {
        match (bound, self.handle) {
            (Some(value), _) => self.fraction(value),
            (None, Handle::Min) => 0.0,
            (None, Handle::Max) => 1.0,
        }
    }

    /// The bound the handle sets at `fraction` of the track: the roll there in whole numbers, or
    /// hundredths for a value with decimals -- and at either end exactly that end's roll, so a
    /// dragged-out handle admits every tier (a two-number roll's range can end on a half).
    pub fn value_at(&self, fraction: f64) -> f64 {
        let fraction = fraction.clamp(0.0, 1.0);
        if fraction == 0.0 {
            return self.low;
        }
        if fraction == 1.0 {
            return self.high;
        }
        let scale = if self.dp { 100.0 } else { 1.0 };
        let raw = self.low + (self.high - self.low) * fraction;
        ((raw * scale).round() / scale).clamp(self.low, self.high)
    }
}

/// The minimum «минимум тира» gives `filter`: the bottom of its tier's roll range
/// (`TierInfo::tier_floor`), so the search admits that tier and better. Only for a row whose search
/// sets a minimum: where a lower roll is better the floor is the tier's best end, not its worst.
pub fn tier_minimum(filter: &SearchFilter) -> Option<f64> {
    let roll = filter.roll.as_ref()?;
    if !matches!(roll.bound, RollBound::Higher | RollBound::AtLeast) {
        return None;
    }
    filter.tier_info?.tier_floor
}

#[cfg(test)]
mod tests {
    use stat_filters::{FilterTag, SearchFilterRoll, TierInfo};

    use super::*;

    /// A one-mod row rolled at `value`, bounded as `bound` says, whose family rolls `range` across
    /// its tiers, the current tier's from `tier_floor`.
    fn row(
        value: f64,
        bound: RollBound,
        range: (f64, f64),
        tier_floor: f64,
        dp: bool,
    ) -> SearchFilter {
        SearchFilter {
            trade_ids: vec!["explicit.stat_3372524247".to_owned()],
            stat_ref: String::new(),
            display_text: "+#% to Fire Resistance".to_owned(),
            tag: FilterTag::Explicit,
            tier: Some(4),
            roll: Some(SearchFilterRoll {
                value,
                min: None,
                max: None,
                dp,
                bound,
            }),
            enabled: true,
            hidden: false,
            generation: None,
            inverted: false,
            score: None,
            tier_info: Some(TierInfo {
                current: 4,
                count: 8,
                best_available: 2,
                min_level: 48,
                tier_floor: Some(tier_floor),
                range: Some(range),
            }),
            weighted_sum: false,
        }
    }

    #[test]
    fn a_higher_is_better_row_slides_its_minimum_along_every_tier() {
        let fire = row(28.0, RollBound::Higher, (6.0, 45.0), 26.0, false);
        let slider = Slider::of(&fire).expect("a ranged row");
        assert_eq!(slider.handle, Handle::Min);
        // The item's own roll and a minimum there sit at the same place.
        assert_eq!(slider.fraction(28.0), 22.0 / 39.0);
        assert_eq!(slider.handle_fraction(Some(28.0)), 22.0 / 39.0);
        // An open minimum admits every roll: the handle rests at the lowest.
        assert_eq!(slider.handle_fraction(None), 0.0);
        // Whole numbers in between, the range's own rolls at its ends, past them clamped.
        assert_eq!(slider.value_at(0.5), 26.0);
        assert_eq!(slider.value_at(0.0), 6.0);
        assert_eq!(slider.value_at(1.0), 45.0);
        assert_eq!(slider.value_at(-0.3), 6.0);
        assert_eq!(slider.value_at(1.7), 45.0);
        assert_eq!(slider.fraction(60.0), 1.0);
        // «минимум тира»: the bottom of the item's tier.
        assert_eq!(tier_minimum(&fire), Some(26.0));
    }

    #[test]
    fn a_lower_is_better_row_slides_its_maximum() {
        let requirements = row(12.0, RollBound::Lower, (10.0, 30.0), 10.0, false);
        let slider = Slider::of(&requirements).expect("a ranged row");
        assert_eq!(slider.handle, Handle::Max);
        assert_eq!(slider.handle_fraction(Some(12.0)), 0.1);
        // An open maximum admits every roll: the handle rests at the highest.
        assert_eq!(slider.handle_fraction(None), 1.0);
        assert_eq!(slider.value_at(0.25), 15.0);
        // Its tier's floor is its best end: «минимум тира» leaves the row alone.
        assert_eq!(tier_minimum(&requirements), None);
    }

    #[test]
    fn a_handle_moves_by_whole_numbers_or_hundredths_and_reaches_a_half_end_exactly() {
        // `Adds (5-8) to (12-15)`, rolled at the mean of its two numbers.
        let added = Slider::of(&row(10.0, RollBound::Higher, (8.5, 11.5), 8.5, false)).unwrap();
        assert_eq!(added.value_at(0.0), 8.5);
        assert_eq!(added.value_at(0.01), 9.0);
        assert_eq!(added.value_at(1.0), 11.5);
        let leech = Slider::of(&row(0.4, RollBound::Higher, (0.2, 0.6), 0.3, true)).unwrap();
        assert_eq!(leech.value_at(0.123), 0.25);
    }

    #[test]
    fn a_number_searched_exactly_or_one_rolling_a_single_value_gets_no_slider() {
        let legend = row(3.0, RollBound::Exactly, (1.0, 8.0), 1.0, false);
        assert_eq!(Slider::of(&legend), None);
        assert_eq!(tier_minimum(&legend), None);
        let one_bolt = row(1.0, RollBound::AtLeast, (1.0, 1.0), 1.0, false);
        assert_eq!(Slider::of(&one_bolt), None);
        let untiered = SearchFilter {
            tier_info: None,
            ..row(28.0, RollBound::Higher, (6.0, 45.0), 26.0, false)
        };
        assert_eq!(Slider::of(&untiered), None);
        assert_eq!(tier_minimum(&untiered), None);
    }
}
