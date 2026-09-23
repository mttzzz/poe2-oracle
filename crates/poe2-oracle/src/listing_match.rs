//! How a trade listing measures up against the search that found it: which of the stats the
//! search asks for its mods have, rolled within the bounds -- the trade site's own reading of a
//! stat filter -- for the results rows' "3/4" and their tooltips' marks. Pure, so the native test
//! pass covers it; `ui::panel::results` only draws it.

use stat_filters::{FilterTag, SearchFilter};
use trade_client::ListedMod;

/// A stat row the search asks for: the stats (`stat_key`) of its trade ids -- a text several
/// stat ids share (`# to all Attributes`) has more than one, any of which matches -- its text, and
/// its bounds as the panel shows them (in the item's own words for an inverted row, as a listing
/// of it words them too).
#[derive(Debug, Clone, PartialEq)]
pub struct WantedStat {
    pub stats: Vec<String>,
    pub text: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl WantedStat {
    /// The rows of `filters` a listed mod can show: the enabled ones with a trade id, but not
    /// properties, pseudo totals and free slots -- a listing lists none of those among its mods.
    pub fn from_filters(filters: &[SearchFilter]) -> Vec<WantedStat> {
        filters
            .iter()
            .filter(|filter| {
                filter.enabled
                    && !filter.trade_ids.is_empty()
                    && !matches!(
                        filter.tag,
                        FilterTag::Property | FilterTag::Pseudo | FilterTag::EmptyAffix
                    )
            })
            .map(|filter| WantedStat {
                stats: filter
                    .trade_ids
                    .iter()
                    .map(|id| stat_key(id).to_owned())
                    .collect(),
                text: filter.display_text.clone(),
                min: filter.roll.as_ref().and_then(|roll| roll.min),
                max: filter.roll.as_ref().and_then(|roll| roll.max),
            })
            .collect()
    }

    /// Whether `listed` is a mod of this row's stat.
    pub fn matches(&self, listed: &ListedMod) -> bool {
        listed
            .stat_id
            .as_deref()
            .is_some_and(|id| self.stats.iter().any(|own| own == stat_key(id)))
    }

    /// Whether a mod rolled `value` within the bounds; a flag mod has none to miss.
    pub fn admits(&self, value: Option<f64>) -> bool {
        let Some(value) = value else {
            return true;
        };
        self.min.is_none_or(|min| value >= min) && self.max.is_none_or(|max| value <= max)
    }
}

/// How a listed mod stands against the search.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Asked {
    /// The search doesn't ask for its stat.
    No,
    /// Asked for and rolled within the bounds.
    Met,
    /// Asked for, rolled outside these bounds -- what a relaxed search lets through besides a
    /// stat missing outright.
    Short { min: Option<f64>, max: Option<f64> },
}

/// How `listed` stands against the stats `wanted`.
pub fn assess(listed: &ListedMod, wanted: &[WantedStat]) -> Asked {
    match wanted.iter().find(|want| want.matches(listed)) {
        None => Asked::No,
        Some(want) if want.admits(listed.value) => Asked::Met,
        Some(want) => Asked::Short {
            min: want.min,
            max: want.max,
        },
    }
}

/// The texts of the stats `wanted` that `mods` has no mod of; empty when the site gave the mods
/// without the stat ids to tell by.
pub fn missing(mods: &[ListedMod], wanted: &[WantedStat]) -> Vec<String> {
    if !has_stat_ids(mods) {
        return Vec::new();
    }
    wanted
        .iter()
        .filter(|want| !mods.iter().any(|listed| want.matches(listed)))
        .map(|want| want.text.clone())
        .collect()
}

/// How many of the stats `wanted` the mods meet -- have, rolled within the bounds, the way the
/// trade site's count group counts -- and of how many; `None` when the site gave the mods without
/// the stat ids to tell by.
pub fn matched_count(mods: &[ListedMod], wanted: &[WantedStat]) -> Option<(usize, usize)> {
    if !has_stat_ids(mods) {
        return None;
    }
    let met = wanted
        .iter()
        .filter(|want| {
            mods.iter()
                .any(|listed| want.matches(listed) && want.admits(listed.value))
        })
        .count();
    Some((met, wanted.len()))
}

fn has_stat_ids(mods: &[ListedMod]) -> bool {
    mods.iter().any(|listed| listed.stat_id.is_some())
}

/// A trade stat id's stat, without its domain: `explicit.stat_4220027924` -> `stat_4220027924`,
/// so a fractured or desecrated roll of a stat counts toward the explicit filter for it.
fn stat_key(id: &str) -> &str {
    id.split_once('.').map_or(id, |(_, stat)| stat)
}

#[cfg(test)]
mod tests {
    use stat_filters::SearchFilterRoll;
    use trade_client::ModKind;

    use super::*;

    fn row(trade_ids: &[&str], tag: FilterTag, min: Option<f64>, enabled: bool) -> SearchFilter {
        SearchFilter {
            trade_ids: trade_ids.iter().map(|&id| id.to_owned()).collect(),
            stat_ref: String::new(),
            display_text: trade_ids.first().copied().unwrap_or_default().to_owned(),
            tag,
            tier: None,
            roll: Some(SearchFilterRoll {
                value: min.unwrap_or_default(),
                min,
                max: None,
                default_min: 0.0,
                default_max: 0.0,
                dp: false,
            }),
            enabled,
            hidden: false,
            generation: None,
            inverted: false,
        }
    }

    fn listed(stat_id: Option<&str>, value: Option<f64>) -> ListedMod {
        ListedMod {
            kind: ModKind::Explicit,
            text: String::new(),
            stat_id: stat_id.map(str::to_owned),
            tier: None,
            level: None,
            value,
        }
    }

    #[test]
    fn a_listing_can_only_show_the_enabled_mod_stats() {
        let wanted = WantedStat::from_filters(&[
            row(
                &["explicit.stat_life"],
                FilterTag::Explicit,
                Some(90.0),
                true,
            ),
            row(
                &["explicit.stat_res"],
                FilterTag::Explicit,
                Some(10.0),
                false,
            ),
            row(
                &["equipment_filters.ev"],
                FilterTag::Property,
                Some(200.0),
                true,
            ),
            row(
                &["pseudo.pseudo_total_life"],
                FilterTag::Pseudo,
                Some(90.0),
                true,
            ),
            row(
                &["explicit.stat_1379411836", "explicit.stat_2897413282"],
                FilterTag::Explicit,
                None,
                true,
            ),
        ]);
        let stats: Vec<&[String]> = wanted.iter().map(|want| want.stats.as_slice()).collect();
        assert_eq!(
            stats,
            [
                &["stat_life".to_owned()][..],
                &["stat_1379411836".to_owned(), "stat_2897413282".to_owned()][..],
            ]
        );
    }

    #[test]
    fn a_mod_is_met_short_of_the_bounds_or_not_asked_for() {
        let wanted = WantedStat::from_filters(&[
            row(
                &["explicit.stat_life"],
                FilterTag::Explicit,
                Some(200.0),
                true,
            ),
            row(
                &["explicit.stat_1379411836", "explicit.stat_2897413282"],
                FilterTag::Explicit,
                Some(10.0),
                true,
            ),
            row(&["explicit.stat_blind"], FilterTag::Explicit, None, true),
        ]);
        // A roll under the minimum the relaxed search let through.
        assert_eq!(
            assess(&listed(Some("explicit.stat_life"), Some(103.0)), &wanted),
            Asked::Short {
                min: Some(200.0),
                max: None
            }
        );
        // The other id of a shared text, from a fractured mod.
        assert_eq!(
            assess(
                &listed(Some("fractured.stat_2897413282"), Some(14.0)),
                &wanted
            ),
            Asked::Met
        );
        // A flag has no roll to fall short with.
        assert_eq!(
            assess(&listed(Some("explicit.stat_blind"), None), &wanted),
            Asked::Met
        );
        assert_eq!(
            assess(&listed(Some("explicit.stat_other"), Some(5.0)), &wanted),
            Asked::No
        );
    }

    #[test]
    fn the_count_is_of_stats_met_and_needs_the_sites_stat_ids() {
        let wanted = WantedStat::from_filters(&[
            row(
                &["explicit.stat_life"],
                FilterTag::Explicit,
                Some(200.0),
                true,
            ),
            row(
                &["explicit.stat_regen"],
                FilterTag::Explicit,
                Some(8.0),
                true,
            ),
            row(
                &["explicit.stat_res"],
                FilterTag::Explicit,
                Some(10.0),
                true,
            ),
        ]);
        let mods = [
            listed(Some("explicit.stat_life"), Some(103.0)),
            listed(Some("explicit.stat_regen"), Some(9.0)),
        ];
        // The life is short, the resistance missing: one of three.
        assert_eq!(matched_count(&mods, &wanted), Some((1, 3)));
        assert_eq!(missing(&mods, &wanted), ["explicit.stat_res"]);

        let untold = [listed(None, Some(103.0))];
        assert_eq!(matched_count(&untold, &wanted), None);
        assert!(missing(&untold, &wanted).is_empty());
    }
}
