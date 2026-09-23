//! PoE Overlay II's mod ranking, ported one to one: its `modRanking` enricher (`9505.bundle.js`,
//! class `ew`, ~9946: `getModRanking`, `computeTagsExplanation`, the weight tables `eg`/`ef`/
//! `ey`/`eM`/`ex`) and its Quick Price pick (~65955). A mod scores its tier against the best one
//! the item's level lets roll, its tags by the weights its kind of item gives them, its roll within
//! its range, and fixed bonuses for a few stats; Quick Price searches the four best-scoring rows
//! scoring at least 3, of which at most two weighted sums.
//!
//! Its tiers and tags come from the item's printed header and PoE Overlay II's own mod database;
//! here from the RePoE tier table (`tiers`). Its bonuses name the game's stat ids; here the trade
//! stats those print as (EE2's `stats.ndjson` and RePoE's `mods.json`).

use poe2_domain::{ItemRarity, ModifierType, ParsedItem, ParsedModifier, ParsedStat};

use crate::tiers::{Family, Tag, printed, stat_hash};

/// Whether PoE Overlay II ranks `item`'s mods at all: never a unique's or a map item's (its
/// `enrich` returns for a unique frame and every `map*` category).
pub(crate) fn ranks(item: &ParsedItem) -> bool {
    item.rarity != Some(ItemRarity::Unique)
        && !item
            .category
            .as_ref()
            .is_some_and(|category| category.id.starts_with("map"))
}

/// The mods it ranks: its `ev`, explicit, desecrated and crafted ones -- where its parser files
/// fractured and sanctum mods too (`search`'s type map: a fractured mod is an explicit one plus a
/// hidden fractured copy).
fn ranked(modifier_type: ModifierType) -> bool {
    matches!(
        modifier_type,
        ModifierType::Explicit
            | ModifierType::Fractured
            | ModifierType::Desecrated
            | ModifierType::Crafted
            | ModifierType::Sanctum
    )
}

/// `eg`: attack weapons.
const ATTACK_WEAPON: &[(Tag, f64)] = &[
    (Tag::Damage, 3.0),
    (Tag::Critical, 3.0),
    (Tag::Attack, 2.0),
    (Tag::Elemental, 2.0),
    (Tag::Physical, 2.0),
    (Tag::Caster, 2.0),
    (Tag::Speed, 2.0),
    (Tag::Life, 1.0),
    (Tag::Mana, -1.0),
    (Tag::Defences, -1.0),
    (Tag::Minion, -2.0),
];
/// `ef`: shields and bucklers.
const SHIELD: &[(Tag, f64)] = &[
    (Tag::Defences, 3.0),
    (Tag::Resistance, 3.0),
    (Tag::Life, 2.0),
    (Tag::Attribute, 2.0),
    (Tag::Mana, 1.0),
    (Tag::Critical, -1.0),
    (Tag::Attack, -1.0),
    (Tag::Minion, -2.0),
];
/// `ey`: foci and the weapons it takes for casters' -- daggers, wands, staves, quarterstaves
/// (`weapon.warstaff`) and fishing rods.
const CASTER: &[(Tag, f64)] = &[
    (Tag::Caster, 3.0),
    (Tag::Critical, 3.0),
    (Tag::Elemental, 2.0),
    (Tag::Life, 1.0),
    (Tag::Defences, 1.0),
    (Tag::Mana, -1.0),
    (Tag::Attack, -2.0),
    (Tag::Minion, -2.0),
];
/// `eM`: belts, body armours, helmets and flasks.
const DEFENSIVE: &[(Tag, f64)] = &[
    (Tag::Resistance, 3.0),
    (Tag::Attribute, 2.0),
    (Tag::Life, 2.0),
    (Tag::Defences, 2.0),
    (Tag::Mana, -1.0),
    (Tag::Critical, -1.0),
    (Tag::Minion, -2.0),
];
const AMULET: &[(Tag, f64)] = &[
    (Tag::Gem, 3.0),
    (Tag::Damage, 3.0),
    (Tag::Critical, 2.0),
    (Tag::Resistance, 2.0),
    (Tag::Defences, 2.0),
    (Tag::Attribute, 1.0),
    (Tag::Mana, -1.0),
];
const RING: &[(Tag, f64)] = &[
    (Tag::Resistance, 3.0),
    (Tag::Life, 3.0),
    (Tag::Attribute, 2.0),
    (Tag::Critical, 2.0),
    (Tag::Damage, 2.0),
    (Tag::Speed, 1.0),
    (Tag::Mana, -1.0),
    (Tag::Minion, -2.0),
];
const BOOTS: &[(Tag, f64)] = &[
    (Tag::Speed, 3.0),
    (Tag::Resistance, 2.0),
    (Tag::Life, 2.0),
    (Tag::Defences, 2.0),
    (Tag::Attribute, 1.0),
    (Tag::Mana, -1.0),
    (Tag::Minion, -2.0),
    (Tag::Caster, -2.0),
];
const GLOVES: &[(Tag, f64)] = &[
    (Tag::Damage, 3.0),
    (Tag::Attack, 3.0),
    (Tag::Elemental, 2.0),
    (Tag::Critical, 2.0),
    (Tag::Speed, 2.0),
    (Tag::Life, 1.0),
    (Tag::Resistance, 1.0),
    (Tag::Defences, 1.0),
    (Tag::Mana, -1.0),
    (Tag::Minion, -2.0),
];
const QUIVER: &[(Tag, f64)] = &[
    (Tag::Damage, 3.0),
    (Tag::Critical, 3.0),
    (Tag::Attack, 2.0),
    (Tag::Elemental, 2.0),
    (Tag::Speed, 2.0),
    (Tag::Life, 2.0),
    (Tag::Resistance, 1.0),
    (Tag::Mana, -1.0),
    (Tag::Minion, -2.0),
];
const JEWEL: &[(Tag, f64)] = &[
    (Tag::Damage, 3.0),
    (Tag::Critical, 3.0),
    (Tag::Resistance, 2.0),
    (Tag::Life, 2.0),
    (Tag::Attribute, 2.0),
    (Tag::Attack, 1.0),
    (Tag::Speed, 1.0),
    (Tag::Mana, -1.0),
    (Tag::Minion, -1.0),
];
const SCEPTRE: &[(Tag, f64)] = &[
    (Tag::Minion, 3.0),
    (Tag::Aura, 3.0),
    (Tag::Damage, 2.0),
    (Tag::Resistance, 2.0),
    (Tag::Attribute, 1.0),
    (Tag::Mana, -1.0),
    (Tag::Attack, -2.0),
];

/// Its tag weights for a trade category (`ex`); none where it has none. Its one `flask`
/// category holds every flask and charm.
fn weights(category: &str) -> &'static [(Tag, f64)] {
    match category {
        "accessory.amulet" => AMULET,
        "accessory.ring" => RING,
        "accessory.belt" | "armour.chest" | "armour.helmet" => DEFENSIVE,
        "armour.boots" => BOOTS,
        "armour.buckler" | "armour.shield" => SHIELD,
        "armour.focus" => CASTER,
        "armour.gloves" => GLOVES,
        "armour.quiver" => QUIVER,
        "jewel" => JEWEL,
        "weapon.sceptre" => SCEPTRE,
        "weapon.dagger" | "weapon.rod" | "weapon.staff" | "weapon.wand" | "weapon.warstaff" => {
            CASTER
        }
        "weapon.bow" | "weapon.claw" | "weapon.crossbow" | "weapon.flail" | "weapon.oneaxe"
        | "weapon.onemace" | "weapon.onesword" | "weapon.spear" | "weapon.talisman"
        | "weapon.twoaxe" | "weapon.twomace" | "weapon.twosword" => ATTACK_WEAPON,
        _ if category.starts_with("flask") => DEFENSIVE,
        _ => &[],
    }
}

/// Its fixed bonuses, per line: the game stat each names, by the trade stat it prints as. Two it
/// also names roll on no PoE2 affix (RePoE 4.5.5.2) and have no trade stat: `local_spirit` (+3)
/// and `trap_skill_gem_level_+` (+3).
const BONUSES: [(&str, f64); 9] = [
    // melee_skill_gem_level_+: # to Level of all Melee Skills
    ("stat_9187492", 3.0),
    // projectile_skill_gem_level_+: # to Level of all Projectile Skills
    ("stat_1202301673", 3.0),
    // local_spirit_+%: #% increased Spirit, which as an affix only sceptres roll; the catalog
    // prints its global twin alike
    ("stat_1416406066", 2.0),
    ("stat_3984865854", 2.0),
    // local_jewel_display_radius_change: Upgrades Radius to #
    ("stat_3891355829", 2.0),
    // base_item_found_rarity_+%: #% increased Rarity of Items found
    ("stat_3917489142", 2.0),
    // local_energy_shield: # to maximum Energy Shield (Local)
    ("stat_4052037485", 0.5),
    // local_energy_shield_+%: #% increased Energy Shield
    ("stat_4015621042", 0.5),
    // base_life_regeneration_rate_per_minute: # Life Regeneration per second
    ("stat_3325883026", -2.0),
];

/// `modifier`'s score (`getModRanking`), `None` for a mod PoE Overlay II doesn't rank. `family`
/// is its family on the item (`tiers::family`).
///
/// - Tier: `(count - current + 1) / (count - best + 1) * 2`, 2 for the best tier the item's level
///   lets roll; 1 for a mod without a printed tier, or whose tier the table doesn't know, unless
///   it is T1, which counts as the only tier.
/// - Tags: weighted by the item's kind, the three heaviest at 0.75, 0.5 and 0.25; a lone tag
///   weighing 2 or more adds 0.5.
/// - Roll: half the share of its range the mod rolled (its lines averaged), 0.25 for a fixed one.
/// - Bonuses: `BONUSES`, once per line.
pub(crate) fn score(
    item: &ParsedItem,
    modifier: &ParsedModifier,
    family: Option<&Family>,
) -> Option<f64> {
    if !ranks(item) || !ranked(modifier.info.modifier_type) {
        return None;
    }
    let mut score = 0.0;

    let current = modifier.info.tier;
    let fit = match (current, family) {
        (Some(current), Some(family)) => Some(family.fit(current, item.item_level)),
        (Some(1), None) => Some((1, 1)),
        _ => None,
    };
    score += match (current, fit) {
        (Some(current), Some((count, best))) => {
            f64::from(count - current + 1) / f64::from(count - best + 1) * 2.0
        }
        _ => 1.0,
    };

    let mut tags: Vec<Tag> = family
        .and_then(|family| {
            let index = current.map_or(0, |tier| tier.saturating_sub(1) as usize);
            family.tiers.get(index).or(family.tiers.first())
        })
        .map(|tier| tier.tags.clone())
        .unwrap_or_default();
    if modifier.info.modifier_type == ModifierType::Crafted {
        tags.push(Tag::Crafted);
    }
    let category = item.category.as_ref().map_or("", |category| &category.id);
    score += tags_score(&tags, weights(category));

    let rolled: Vec<&ParsedStat> = modifier.stats.iter().filter(|stat| rolls(stat)).collect();
    if !rolled.is_empty() {
        let count = rolled.len() as f64;
        let mean = |field: fn(&ParsedStat) -> (f64, f64, f64)| {
            let (value, min, max) = rolled
                .iter()
                .map(|stat| field(stat))
                .fold((0.0, 0.0, 0.0), |(value, min, max), (v, lo, hi)| {
                    (value + v, min + lo, max + hi)
                });
            (value / count, min / count, max / count)
        };
        let (value, min, max) = mean(printed);
        score += if min == max {
            0.25
        } else {
            0.5 * (value - min) / (max - min)
        };
    }

    for stat in &modifier.stats {
        let hash = stat.stat_id.as_deref().map(stat_hash);
        if let Some(&(_, bonus)) = BONUSES.iter().find(|(stat, _)| Some(*stat) == hash) {
            score += bonus;
        }
    }
    Some(score)
}

/// `computeTagsExplanation`: the three heaviest tags at 0.75, 0.5 and 0.25 of their weight, a
/// tag its weights don't name weighing 0; a lone tag of weight 2 or more adds 0.5.
fn tags_score(tags: &[Tag], weights: &[(Tag, f64)]) -> f64 {
    let mut weighed: Vec<f64> = tags
        .iter()
        .map(|tag| {
            weights
                .iter()
                .find(|(named, _)| named == tag)
                .map_or(0.0, |&(_, weight)| weight)
        })
        .collect();
    weighed.sort_by(|a, b| b.total_cmp(a));
    let score: f64 = weighed
        .iter()
        .zip([0.75, 0.5, 0.25])
        .map(|(w, f)| w * f)
        .sum();
    match weighed[..] {
        [lone] if lone >= 2.0 => score + 0.5,
        _ => score,
    }
}

/// Whether a line has a roll: a number, not a flag (which the parser rolls at 0, unscalable).
fn rolls(stat: &ParsedStat) -> bool {
    !(stat.unscalable && stat.value == 0.0 && stat.min == 0.0 && stat.max == 0.0)
}

/// The lowest score Quick Price searches.
pub(crate) const MIN_SCORE: f64 = 3.0;
/// How many rows Quick Price searches at most.
pub(crate) const MAX_PICKS: usize = 4;
/// How many of them may be weighted sums.
pub(crate) const MAX_WEIGHTED_SUMS: usize = 2;

/// Something Quick Price can search: a row of its own (a property, a pseudo total), or a mod,
/// whose rows it then searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    Row(usize),
    Mod(usize),
}

/// A candidate: what, its score, and whether it is a weighted sum.
pub(crate) struct Candidate {
    pub(crate) pick: Pick,
    pub(crate) score: f64,
    pub(crate) weighted_sum: bool,
}

/// Quick Price's pick (`9505.bundle.js` ~65955): the candidates scoring at least `MIN_SCORE`,
/// best first -- a stable sort, so a tie keeps their order: properties, then mods in the item's
/// order, then pseudo totals -- the first `MAX_PICKS`, skipping weighted sums past the
/// `MAX_WEIGHTED_SUMS`th.
pub(crate) fn quick_picks(mut candidates: Vec<Candidate>) -> Vec<Pick> {
    candidates.retain(|candidate| candidate.score >= MIN_SCORE);
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut picks = Vec::with_capacity(MAX_PICKS);
    let mut weighted_sums = 0;
    for candidate in candidates {
        if candidate.weighted_sum {
            if weighted_sums == MAX_WEIGHTED_SUMS {
                continue;
            }
            weighted_sums += 1;
        }
        picks.push(candidate.pick);
        if picks.len() == MAX_PICKS {
            break;
        }
    }
    picks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(pick: Pick, score: f64, weighted_sum: bool) -> Candidate {
        Candidate {
            pick,
            score,
            weighted_sum,
        }
    }

    #[test]
    fn quick_price_picks_four_rows_scoring_three_best_first_two_weighted_sums_at_most() {
        let picks = quick_picks(vec![
            candidate(Pick::Row(0), 2.99, false),
            candidate(Pick::Mod(0), 3.0, false),
            candidate(Pick::Row(1), 5.0, true),
            candidate(Pick::Row(2), 4.5, true),
            candidate(Pick::Row(3), 4.0, true),
            candidate(Pick::Mod(1), 3.5, false),
            candidate(Pick::Mod(2), 3.25, false),
        ]);
        // The third weighted sum (4.0) is skipped and a lower-scoring mod takes its place; 3.0
        // is past the fourth pick, 2.99 under the bar.
        assert_eq!(
            picks,
            [Pick::Row(1), Pick::Row(2), Pick::Mod(1), Pick::Mod(2)]
        );

        // A tie keeps the order the candidates came in.
        let tied = quick_picks(vec![
            candidate(Pick::Row(0), 3.5, false),
            candidate(Pick::Mod(0), 3.5, false),
        ]);
        assert_eq!(tied, [Pick::Row(0), Pick::Mod(0)]);
    }

    #[test]
    fn tags_weigh_the_three_heaviest_and_a_lone_heavy_tag_counts_extra() {
        // A ring: Resistance 3, Elemental and Fire unweighted. 3 * 0.75 + 0 + 0 = 2.25.
        assert_eq!(
            tags_score(&[Tag::Elemental, Tag::Fire, Tag::Resistance], RING),
            2.25
        );
        // A lone Life tag on a ring: 3 * 0.75 + 0.5 = 2.75; on gloves (Life 1) no bonus: 0.75.
        assert_eq!(tags_score(&[Tag::Life], RING), 2.75);
        assert_eq!(tags_score(&[Tag::Life], GLOVES), 0.75);
        // Four tags on gloves: Damage 3, Attack 3, Elemental 2, Lightning 0 -> the top three:
        // 3 * 0.75 + 3 * 0.5 + 2 * 0.25 = 4.25.
        assert_eq!(
            tags_score(
                &[Tag::Damage, Tag::Elemental, Tag::Lightning, Tag::Attack],
                GLOVES
            ),
            4.25
        );
        // A crafted mod's own tag weighs nothing but takes the lone tag's bonus away.
        assert_eq!(tags_score(&[Tag::Life, Tag::Crafted], RING), 2.25);
        assert_eq!(tags_score(&[], RING), 0.0);
    }
}
