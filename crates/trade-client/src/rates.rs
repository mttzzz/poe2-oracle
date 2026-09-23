//! A robust price estimate from a trade result page -- the data behind a PoE Overlay II-style
//! "≈ 1.72 div, range 0.75–3.97 div, confidence: high" line -- with each listing's price valued
//! at poe.ninja's market ([`Market`]).
//!
//! A listing's `price.currency` is a trade id, and so is every poe.ninja line id (see `ninja`'s
//! module doc), so a listing's value is looked up directly -- EE2 detours through English item
//! names (`getCurrencyDetailsId`), which after its `CONVERT_CURRENCY` renames finds no price at
//! all for greater/perfect orbs.

use crate::ninja::{DIVINE, EXALTED, Market};

/// The cheapest ten rows -- one trade `fetch` page -- set the price. Fixed, so an estimate doesn't
/// drift with how many pages the caller fetched (EE2's first page is two).
const CHEAPEST_LISTINGS: usize = 10;

/// A listing under a third or over three times the page median is not the market: a price
/// fixer's bait, a mistyped currency (chaos for divine is 8x at 2026-09-22 rates), an over-ask.
/// Honest spreads stay inside it -- PoE Overlay II's own example quotes 0.75–3.97 div around 1.72.
const OUTLIER_FACTOR: f64 = 3.0;

/// EE2's `isLikelyPriceFixed` (`TradeListing.vue`): a page of more than 15 rows with fewer than 5
/// priced in common currencies. EE2 counts seller-grouped rows (its `groupedResults`).
const PRICE_FIXED_MIN_ROWS: usize = 15;
const PRICE_FIXED_MIN_COMMON: usize = 5;

/// A median needs three listings to outvote one stray.
const MEDIUM_MIN_LISTINGS: usize = 3;
/// High confidence wants six listings typically (by median) within 1.5x of the value.
const HIGH_MIN_LISTINGS: usize = 6;
const HIGH_MAX_DEVIATION: f64 = 1.5;
/// Listings typically more than 2x from the value disagree too much for it to mean much.
const MEDIUM_MAX_DEVIATION: f64 = 2.0;

/// The currency a [`PriceEstimate`] is quoted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceUnit {
    Divine,
    Exalted,
}

impl PriceUnit {
    /// The trade API currency id -- the same key a listing's `price.currency` carries.
    pub fn trade_id(self) -> &'static str {
        match self {
            PriceUnit::Divine => DIVINE,
            PriceUnit::Exalted => EXALTED,
        }
    }
}

/// How far a [`PriceEstimate`] can be trusted, from how many listings back it and how closely
/// they agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// A trade result page summed up. `value`, `low` and `high` share `unit`, so the range reads in
/// one currency even where it dips below the unit's cutover (PoE Overlay II: "0,75–3,97 div").
#[derive(Debug, Clone, PartialEq)]
pub struct PriceEstimate {
    /// Median of the listings the estimate kept.
    pub value: f64,
    pub unit: PriceUnit,
    /// Cheapest and dearest kept listing.
    pub low: f64,
    pub high: f64,
    pub confidence: Confidence,
    /// EE2's "likely price fixed" warning: the page is mostly bait priced in odd currencies, so
    /// the estimate rests on its few common-currency listings, if any, and is always
    /// [`Confidence::Low`]. EE2 pairs the warning with a re-search restricted to Exalted/Divine
    /// prices.
    pub likely_price_fixed: bool,
}

/// Sums up a trade result page: the `(amount, trade currency id)` price of each row of a
/// price-ascending search, in any order -- seller-grouped like EE2's `groupedResults` (the rows
/// `group_listings` builds), which its price-fixing check counts.
///
/// 1. EE2's price-fixing check: a page of more than 15 rows with fewer than 5 in common
///    currencies (see `is_common_currency`) is presumed bait, and only its common-currency rows
///    are used -- what EE2's "Exalted/Divine" re-search would show -- at [`Confidence::Low`].
/// 2. Each price is converted to divines at `market` ([`Market::value_in_divines`]); a row
///    without a price or in a currency poe.ninja doesn't price is skipped.
/// 3. Only the cheapest ten count.
/// 4. Rows more than 3x away from their median are outliers and dropped.
/// 5. `value` is the median of the rest, `low`/`high` their extremes, quoted in divines or, for
///    cheap items, exalted ([`Market::in_display_unit`] of the value).
/// 6. Confidence: under three rows is low; otherwise it follows the typical deviation, the
///    median factor between a row and the value -- within 1.5x on six or more rows is high,
///    within 2x medium, beyond that low.
///
/// `None` when no row has a known price.
pub fn estimate(prices: &[(f64, &str)], market: &Market) -> Option<PriceEstimate> {
    let common = prices
        .iter()
        .filter(|&&(amount, currency)| is_common_currency(amount, currency))
        .count();
    let likely_price_fixed = prices.len() > PRICE_FIXED_MIN_ROWS && common < PRICE_FIXED_MIN_COMMON;
    // With no common-currency row at all there is nothing better than the bait itself.
    let common_only = likely_price_fixed && common > 0;
    let mut kept: Vec<f64> = prices
        .iter()
        .filter(|&&(amount, currency)| !common_only || is_common_currency(amount, currency))
        .filter_map(|&(amount, currency)| {
            let divines = amount * market.value_in_divines(currency)?;
            (divines.is_finite() && divines > 0.0).then_some(divines)
        })
        .collect();
    kept.sort_by(f64::total_cmp);
    kept.truncate(CHEAPEST_LISTINGS);

    // Never empties `kept`: the upper middle listing is within 2x of the median.
    let page_median = median(&kept)?;
    kept.retain(|&divines| {
        divines >= page_median / OUTLIER_FACTOR && divines <= page_median * OUTLIER_FACTOR
    });
    let value = median(&kept)?;
    let (low, high) = (*kept.first()?, *kept.last()?);

    let (shown_value, unit) = market.in_display_unit(value);
    let per_divine = shown_value / value;
    Some(PriceEstimate {
        value: shown_value,
        unit,
        low: low * per_divine,
        high: high * per_divine,
        confidence: confidence(&kept, value, likely_price_fixed),
        likely_price_fixed,
    })
}

/// EE2's "common currency" test from `isLikelyPriceFixed` (`TradeListing.vue`): Chaos, Exalted
/// or Divine Orbs of any tier (its `/chaos|exalted|divine/i` matches the greater/perfect ids
/// too), or under 30 Augmentation, Regal or Transmutation Orbs -- the cheap currencies honest
/// low-value listings use.
fn is_common_currency(amount: f64, currency: &str) -> bool {
    ["chaos", "exalted", "divine"]
        .into_iter()
        .any(|core| currency.contains(core))
        || (matches!(currency, "aug" | "regal" | "transmute") && amount < 30.0)
}

/// Step 6 of [`estimate`]; `kept` ascending, in divines, as is `value`.
fn confidence(kept: &[f64], value: f64, likely_price_fixed: bool) -> Confidence {
    if likely_price_fixed || kept.len() < MEDIUM_MIN_LISTINGS {
        return Confidence::Low;
    }
    let mut deviations: Vec<f64> = kept
        .iter()
        .map(|&divines| (divines / value).ln().abs())
        .collect();
    deviations.sort_by(f64::total_cmp);
    let typical_deviation = median(&deviations).map_or(f64::INFINITY, f64::exp);
    if kept.len() >= HIGH_MIN_LISTINGS && typical_deviation <= HIGH_MAX_DEVIATION {
        Confidence::High
    } else if typical_deviation <= MEDIUM_MAX_DEVIATION {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

/// Median of an ascending slice; `None` when empty.
fn median(sorted: &[f64]) -> Option<f64> {
    let mid = sorted.len() / 2;
    if sorted.is_empty() {
        None
    } else if sorted.len().is_multiple_of(2) {
        Some((sorted[mid - 1] + sorted[mid]) / 2.0)
    } else {
        Some(sorted[mid])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 500 exalted to the divine; the divine and the exalted go by the core rates.
    fn market() -> Market {
        Market::from_values(500.0, 8.0, &[("chance", 0.016), ("transmute", 0.0025)])
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0),
            "{actual} != {expected}"
        );
    }

    #[test]
    fn mixed_divine_and_exalted_prices_are_estimated_in_one_unit() {
        let prices = [
            (250.0, "exalted"),
            (1.0, "divine"),
            (400.0, "exalted"),
            (1.2, "divine"),
            (600.0, "exalted"),
        ];
        let est = estimate(&prices, &market()).expect("priced listings");
        assert_eq!(est.unit, PriceUnit::Divine);
        assert_close(est.value, 1.0);
        assert_close(est.low, 0.5);
        assert_close(est.high, 1.2);
    }

    #[test]
    fn a_value_under_a_divine_is_quoted_in_exalted() {
        let prices = [(20.0, "exalted"), (0.05, "divine"), (30.0, "exalted")];
        let est = estimate(&prices, &market()).expect("priced listings");
        assert_eq!(est.unit, PriceUnit::Exalted);
        assert_close(est.value, 25.0);
        assert_close(est.low, 20.0);
        assert_close(est.high, 30.0);
    }

    #[test]
    fn listings_far_from_the_page_median_do_not_move_the_estimate() {
        let prices = [
            (1.0, "exalted"), // bait: 0.002 div
            (1.5, "divine"),
            (1.6, "divine"),
            (1.7, "divine"),
            (1.8, "divine"),
            (2.0, "divine"),
            (2.1, "divine"),
            (2.2, "divine"),
            (30.0, "divine"), // over-ask
        ];
        let est = estimate(&prices, &market()).expect("priced listings");
        assert_close(est.value, 1.8);
        assert_close(est.low, 1.5);
        assert_close(est.high, 2.2);
        assert_eq!(est.confidence, Confidence::High);
        assert!(!est.likely_price_fixed);
    }

    #[test]
    fn a_single_listing_is_low_confidence() {
        let est = estimate(&[(2.0, "divine")], &market()).expect("one priced listing");
        assert_close(est.value, 2.0);
        assert_eq!(est.confidence, Confidence::Low);
    }

    #[test]
    fn listings_in_unknown_currencies_are_skipped() {
        let prices = [
            (1.0, "divine"),
            (0.0, ""), // unpriced listing
            (1.1, "divine"),
            (5.0, "some-new-orb"),
            (1.2, "divine"),
        ];
        let est = estimate(&prices, &market()).expect("priced listings");
        assert_close(est.value, 1.1);
        assert_close(est.high, 1.2);
        assert_eq!(estimate(&[(5.0, "some-new-orb")], &market()), None);
    }

    #[test]
    fn a_page_of_odd_currency_bait_rests_on_its_common_currency_listings() {
        // EE2's first page: 16 odd-currency bait rows ahead of the only honest prices.
        let mut prices = vec![(1.0, "chance"); 16];
        prices.extend([(2.0, "divine"), (2.4, "divine")]);
        let est = estimate(&prices, &market()).expect("priced listings");
        assert!(est.likely_price_fixed);
        assert_close(est.value, 2.2);
        assert_eq!(est.confidence, Confidence::Low);

        // A full page of a few transmutes each is an honest cheap item, not bait.
        let transmutes = vec![(2.0, "transmute"); 18];
        let est = estimate(&transmutes, &market()).expect("priced listings");
        assert!(!est.likely_price_fixed);
    }
}
