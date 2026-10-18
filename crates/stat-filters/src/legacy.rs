//! A Mageblood's legacies (`Legacy of Quicksilver(Amethyst-Topaz) — Unscalable Value`, four on
//! every belt, the same one up to four times): what the belt is worth, so its search finds belts
//! with the same legacies. The trade site files each legacy as an option of one stat and counts
//! its copies, so a legacy row searches at least as many copies as the belt has, and one more row
//! leaves out every listing with a legacy the belt lacks -- the row player report #15
//! (2026-10-18) asked for. EE2 searches a Mageblood's copies too (`buildMageBloodNotFilter`).

use poe2_domain::{ParsedItem, StatCatalog};

use crate::{FilterTag, RollBound, SearchFilter, SearchFilterRoll};

/// The trade id of every legacy: `Legacy of #`, with the legacy's option after the bar (`|1`
/// Amethyst ... `|14` Topaz in the live catalogs). A listing's value of one is how many of its
/// legacies are that one: `|8` at least 3 with `|5` at least 1 found 22 Magebloods on
/// 2026-10-18, every one with Quicksilver three times and Gold once.
const LEGACY: &str = "explicit.stat_264262054|";

/// Turns each of `rows`' legacy rows into a count of the belt's copies of it -- searched as a
/// minimum and written into its text, `Legacy of Quicksilver ×3` -- and adds after the last one
/// the row listing every legacy of `catalog` the belt lacks, searched as none of them
/// (`SearchFilter::none_of`). That row has the legacies' mods for its sources, so a profile
/// checks it whenever it checks them. Nothing on an item without a legacy. `sources` stays one
/// to one with `rows`.
pub(crate) fn settle(
    item: &ParsedItem,
    catalog: &StatCatalog,
    rows: &mut Vec<SearchFilter>,
    sources: &mut Vec<Vec<usize>>,
) {
    let mut last = None;
    let mut own: Vec<String> = Vec::new();
    let mut mods: Vec<usize> = Vec::new();
    for (index, (row, row_sources)) in rows.iter_mut().zip(sources.iter()).enumerate() {
        if !row.trade_ids.iter().any(|id| id.starts_with(LEGACY)) {
            continue;
        }
        let copies = item
            .mods
            .iter()
            .flat_map(|modifier| &modifier.stats)
            .filter(|stat| {
                stat.stat_id
                    .as_ref()
                    .is_some_and(|id| row.trade_ids.contains(id))
            })
            .count();
        row.roll = Some(SearchFilterRoll {
            value: copies as f64,
            min: None,
            max: None,
            dp: false,
            bound: RollBound::AtLeast,
        });
        row.display_text.push_str(" ×#");
        own.extend(row.trade_ids.iter().cloned());
        mods.extend(row_sources);
        last = Some(index);
    }
    let Some(last) = last else {
        return;
    };
    let others: Vec<String> = catalog
        .stats
        .iter()
        .map(|stat| &stat.id)
        .filter(|id| id.starts_with(LEGACY) && !own.contains(id))
        .cloned()
        .collect();
    if others.is_empty() {
        return;
    }
    rows.insert(
        last + 1,
        SearchFilter {
            trade_ids: others,
            stat_ref: String::new(),
            display_text: "No other Mage's Legacies".to_owned(),
            tag: FilterTag::Explicit,
            tier: None,
            roll: None,
            enabled: false,
            hidden: false,
            generation: None,
            inverted: false,
            score: None,
            tier_info: None,
            weighted_sum: false,
            none_of: true,
        },
    );
    sources.insert(last + 1, mods);
}
