//! Weighted-sum rows: PoE Overlay II's pseudo totals the trade site has no pseudo stat for -- its
//! `weightedSum` definitions (`9505.bundle.js` ~14965) -- searched as a `weight2` stat group that
//! sums, at weight 1 each, the trade stats its `R` table lists for the total (~69100). A visible
//! one merges the mods it sums: their own rows hide and the total takes their best score, as its
//! `markBaseModsAsMerged` and `finalizeModMetadata` do. Quick Price searches at most two.
//!
//! Its elemental spell damage total adds up its fire, cold and lightning spell totals, counting
//! the elemental and spell damage mods once per element; the site sums each stat once, so the
//! total here is the site's sum, which the item itself meets.

use poe2_domain::{ItemRarity, ParsedItem, StatCatalog};

use crate::property::uses_exact_preset;
use crate::tiers::stat_hash;
use crate::{FilterTag, RollBound, SearchFilter, SearchFilterRoll};

/// One weighted-sum total.
struct Sum {
    english: &'static str,
    /// PoE Overlay II's own Russian label.
    russian: &'static str,
    /// The trade ids the site sums, weight 1 each.
    stats: &'static [&'static str],
    /// Stat hashes the item must carry for the total to exist, one of each group (its `required`
    /// inputs); none: any stat of `stats`.
    requires: &'static [&'static [&'static str]],
    /// Kept behind the panel's "show hidden" toggle, merging nothing: an element's spell damage
    /// and the spell damage total, which the elemental and chaos ones cover.
    hidden: bool,
}

const FIRE_DAMAGE: &str = "stat_3962278098";
const COLD_DAMAGE: &str = "stat_3291658075";
const LIGHTNING_DAMAGE: &str = "stat_2231156303";
const CHAOS_DAMAGE: &str = "stat_736967255";

/// Its weighted sums, in its order. Every trade id's text checked in the live EN catalog
/// (2026-09-23).
const SUMS: [Sum; 13] = [
    Sum {
        english: "#% increased Rarity of Items found",
        russian: "+#% к редкости найденных предметов",
        stats: &[
            "explicit.stat_3917489142",
            "implicit.stat_3917489142",
            "enchant.stat_3917489142",
        ],
        requires: &[],
        hidden: false,
    },
    Sum {
        english: "Adds # to # Physical Damage to Attacks",
        russian: "Добавляет #–# Физического Урона к Атакам",
        stats: &["explicit.stat_3032590688", "implicit.stat_3032590688"],
        requires: &[],
        hidden: false,
    },
    Sum {
        english: "Adds # to # Elemental Damage to Attacks",
        russian: "Добавляет #–# Элементального Урона к Атакам",
        stats: &[
            "explicit.stat_1573130764",
            "implicit.stat_1573130764",
            "explicit.stat_4067062424",
            "explicit.stat_1754445556",
            "implicit.stat_1754445556",
        ],
        requires: &[],
        hidden: false,
    },
    Sum {
        english: "Adds # to # Damage to Attacks",
        russian: "Добавляет #–# Урона к Атакам",
        stats: &[
            "explicit.stat_3032590688",
            "implicit.stat_3032590688",
            "explicit.stat_1573130764",
            "implicit.stat_1573130764",
            "explicit.stat_4067062424",
            "explicit.stat_1754445556",
            "implicit.stat_1754445556",
            "explicit.stat_674553446",
        ],
        requires: &[&["stat_674553446"]],
        hidden: false,
    },
    Sum {
        english: "Allies in your Presence deal # to # added Physical Attack Damage",
        russian: "Союзники в вашем присутствии наносят от # до # дополнительного физического урона атаками",
        stats: &["explicit.stat_1574590649"],
        requires: &[],
        hidden: false,
    },
    Sum {
        english: "Allies in your Presence deal # to # added Elemental Attack Damage",
        russian: "Союзники в вашем присутствии наносят от # до # дополнительного стихийного урона атаками",
        stats: &[
            "explicit.stat_849987426",
            "explicit.stat_2347036682",
            "explicit.stat_2854751904",
        ],
        requires: &[],
        hidden: false,
    },
    Sum {
        english: "Allies in your Presence deal # to # added Attack Damage",
        russian: "Союзники в вашем присутствии наносят от # до # дополнительного урона атаками",
        stats: &[
            "explicit.stat_1574590649",
            "explicit.stat_849987426",
            "explicit.stat_2347036682",
            "explicit.stat_2854751904",
            "explicit.stat_262946222",
        ],
        requires: &[&["stat_262946222"]],
        hidden: false,
    },
    Sum {
        english: "#% increased Fire Spell Damage",
        russian: "+#% к урону от огненных заклинаний",
        stats: &[
            "explicit.stat_3141070085",
            "explicit.stat_3962278098",
            "explicit.stat_2974417149",
            "enchant.stat_2974417149",
        ],
        requires: &[&[FIRE_DAMAGE]],
        hidden: true,
    },
    Sum {
        english: "#% increased Cold Spell Damage",
        russian: "+#% к урону от ледяных заклинаний",
        stats: &[
            "explicit.stat_3141070085",
            "explicit.stat_3291658075",
            "explicit.stat_2974417149",
            "enchant.stat_2974417149",
        ],
        requires: &[&[COLD_DAMAGE]],
        hidden: true,
    },
    Sum {
        english: "#% increased Lightning Spell Damage",
        russian: "+#% к урону от молниеносных заклинаний",
        stats: &[
            "explicit.stat_3141070085",
            "explicit.stat_2231156303",
            "implicit.stat_2231156303",
            "explicit.stat_2974417149",
            "enchant.stat_2974417149",
        ],
        requires: &[&[LIGHTNING_DAMAGE]],
        hidden: true,
    },
    Sum {
        english: "#% increased Elemental Spell Damage",
        russian: "+#% к урону от стихийных заклинаний",
        stats: &[
            "explicit.stat_3141070085",
            "explicit.stat_3962278098",
            "explicit.stat_3291658075",
            "explicit.stat_2231156303",
            "implicit.stat_2231156303",
            "explicit.stat_2974417149",
            "enchant.stat_2974417149",
        ],
        requires: &[&[FIRE_DAMAGE, COLD_DAMAGE, LIGHTNING_DAMAGE]],
        hidden: false,
    },
    Sum {
        english: "#% increased Chaos Spell Damage",
        russian: "+#% к урону от хаотических заклинаний",
        stats: &[
            "explicit.stat_736967255",
            "implicit.stat_736967255",
            "explicit.stat_2974417149",
            "enchant.stat_2974417149",
        ],
        requires: &[&[CHAOS_DAMAGE]],
        hidden: false,
    },
    Sum {
        english: "#% increased Spell Damage",
        russian: "+#% к урону от заклинаний",
        stats: &[
            "explicit.stat_3141070085",
            "explicit.stat_3962278098",
            "explicit.stat_3291658075",
            "explicit.stat_2231156303",
            "implicit.stat_2231156303",
            "explicit.stat_736967255",
            "implicit.stat_736967255",
            "explicit.stat_2974417149",
            "enchant.stat_2974417149",
        ],
        requires: &[
            &[FIRE_DAMAGE, COLD_DAMAGE, LIGHTNING_DAMAGE],
            &[CHAOS_DAMAGE],
        ],
        hidden: true,
    },
];

/// Every weighted-sum row `item` has, each with the mods it sums (indexes into `item.mods`), in
/// PoE Overlay II's order. A total counts every line of its stats whatever the mod type, as its
/// own does. Labelled in Russian where `catalog`, the searched site's, is Russian. None on a
/// unique -- PoE Overlay II sums a unique's rarity alone, which its own row says as well -- nor
/// on an item EE2 prices with its exact preset, which gets no pseudo totals either.
pub(crate) fn weighted_sums(
    item: &ParsedItem,
    catalog: &StatCatalog,
) -> Vec<(SearchFilter, Vec<usize>)> {
    if uses_exact_preset(item) || item.rarity == Some(ItemRarity::Unique) {
        return Vec::new();
    }
    let russian = catalog
        .stats
        .iter()
        .find(|stat| stat.id == "pseudo.pseudo_total_life")
        .is_some_and(|stat| stat.text.chars().any(|c| matches!(c, 'А'..='я')));
    let carries = |hash: &str| {
        item.mods
            .iter()
            .flat_map(|modifier| &modifier.stats)
            .any(|stat| stat.stat_id.as_deref().map(stat_hash) == Some(hash))
    };
    SUMS.iter()
        .filter_map(|sum| {
            let summed = |hash: &str| sum.stats.iter().any(|&id| stat_hash(id) == hash);
            let (mut value, mut min, mut max, mut dp) = (0.0, 0.0, 0.0, false);
            let mut sources = Vec::new();
            for (index, modifier) in item.mods.iter().enumerate() {
                let mut sums_it = false;
                for stat in &modifier.stats {
                    if stat
                        .stat_id
                        .as_deref()
                        .is_some_and(|id| summed(stat_hash(id)))
                    {
                        (value, min, max) = (value + stat.value, min + stat.min, max + stat.max);
                        dp |= stat.dp;
                        sums_it = true;
                    }
                }
                if sums_it {
                    sources.push(index);
                }
            }
            let required = sum
                .requires
                .iter()
                .all(|any| any.iter().any(|&hash| carries(hash)));
            if sources.is_empty() || !required {
                return None;
            }
            let bound = if min == max {
                RollBound::AtLeast
            } else {
                RollBound::Higher
            };
            let row = SearchFilter {
                trade_ids: sum.stats.iter().map(|&id| id.to_owned()).collect(),
                stat_ref: sum.english.to_owned(),
                display_text: if russian { sum.russian } else { sum.english }.to_owned(),
                tag: FilterTag::Pseudo,
                tier: None,
                roll: Some(SearchFilterRoll {
                    value,
                    min: None,
                    max: None,
                    dp,
                    bound,
                }),
                enabled: false,
                hidden: sum.hidden,
                generation: None,
                inverted: false,
                score: None,
                tier_info: None,
                weighted_sum: true,
            };
            Some((row, sources))
        })
        .collect()
}
