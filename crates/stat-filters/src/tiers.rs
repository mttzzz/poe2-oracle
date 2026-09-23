//! The game's mod tiers on each kind of item: `data/mod-tiers.tsv`, which
//! `packaging/data/generate_mod_tiers.py` builds from RePoE's export of the game's mod and base
//! item tables (see `data/NOTICE`). A family is one mod type in one affix slot on one trade
//! category, keyed by the trade stats its lines print as -- the stats `item-parser` resolves a
//! modifier's lines to -- and lists every tier, T1 first: the item level it needs, its RePoE mod
//! id, its tags, each stat's roll range and the order the game lists its stats in. Desecrated
//! (Abyssal) mods have families of their own: the client numbers their tiers apart from the
//! ordinary ones of the same stats. So have waystones, one set per waystone tier
//! (`map.waystone:<tier>`), which only `game_mod` looks up: their mods roll by the tier.

use std::collections::HashMap;
use std::sync::LazyLock;

use poe2_domain::{ModGeneration, ModifierType, ParsedItem, ParsedModifier, ParsedStat};

use crate::TierInfo;

/// A mod tag PoE Overlay II's weight tables name: its tag list (`9398.bundle.js`, module 81020),
/// each the id of the `Tag<Name>` client string it reads an item's printed tags by, `Crafted`
/// being the one it gives every crafted mod itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tag {
    Gem,
    Caster,
    Fire,
    Cold,
    Lightning,
    Chaos,
    Physical,
    Life,
    Defences,
    Elemental,
    Attack,
    Minion,
    Aura,
    Mana,
    Speed,
    Critical,
    Damage,
    Resistance,
    Attribute,
    Ailment,
    Curse,
    Crafted,
}

impl Tag {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "Gem" => Self::Gem,
            "Caster" => Self::Caster,
            "Fire" => Self::Fire,
            "Cold" => Self::Cold,
            "Lightning" => Self::Lightning,
            "Chaos" => Self::Chaos,
            "Physical" => Self::Physical,
            "Life" => Self::Life,
            "Defences" => Self::Defences,
            "Elemental" => Self::Elemental,
            "Attack" => Self::Attack,
            "Minion" => Self::Minion,
            "Aura" => Self::Aura,
            "Mana" => Self::Mana,
            "Speed" => Self::Speed,
            "Critical" => Self::Critical,
            "Damage" => Self::Damage,
            "Resistance" => Self::Resistance,
            "Attribute" => Self::Attribute,
            "Ailment" => Self::Ailment,
            "Curse" => Self::Curse,
            _ => return None,
        })
    }
}

/// One tier of a family.
#[derive(Debug)]
pub(crate) struct Tier {
    /// The item level the tier needs.
    pub(crate) level: u32,
    /// RePoE's id of the tier's mod (`IncreasedLife7`), the game's own.
    pub(crate) mod_id: &'static str,
    pub(crate) tags: Vec<Tag>,
    /// Each key stat's roll range as the item prints it, in `Family::stats` order; `None` for a
    /// flag.
    ranges: Vec<Option<(f64, f64)>>,
    /// Where each of the mod's stats is printed, in the game's own order: a key stat (an index
    /// into `Family::stats`, a line printing two numbers twice) or a fixed number the item doesn't
    /// print; `None` when the item's text can't give each stat its roll -- a line without a
    /// number, a hidden stat.
    order: Option<Vec<Source>>,
}

/// Where one of a tier's stats is printed.
#[derive(Debug, Clone, Copy)]
enum Source {
    Key(usize),
    Fixed(f64),
}

/// Which mods a family's tiers are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pool {
    /// The ordinary prefixes and suffixes.
    Affix,
    /// The desecrated (Abyssal) ones.
    Desecrated,
}

/// One mod family on one kind of item.
#[derive(Debug)]
pub(crate) struct Family {
    slot: ModGeneration,
    category: &'static str,
    pool: Pool,
    /// The key's trade stat hashes, sorted.
    stats: Vec<&'static str>,
    /// T1 first.
    pub(crate) tiers: Vec<Tier>,
}

impl Family {
    /// How many tiers the family has for a mod of tier `current` and the best one `item_level`
    /// lets roll, as PoE Overlay II's `getMod` counts them (`main/main.js` ~101100): at least
    /// `current` tiers, and never a best tier above `current`. An unknown item level lets every
    /// tier roll, as its `level > undefined` never excludes one.
    pub(crate) fn fit(&self, current: u32, item_level: Option<u32>) -> (u32, u32) {
        let count = (self.tiers.len() as u32).max(current);
        let available = self
            .tiers
            .iter()
            .filter(|tier| item_level.is_none_or(|level| tier.level <= level))
            .count() as u32;
        (count, (1 + count - available).min(current))
    }

    /// Where tier `current` of the family sits for its key stat `stat_hash`. `None` when the
    /// table has fewer tiers than the item prints: it describes another version of the game.
    pub(crate) fn tier_info(
        &self,
        current: u32,
        item_level: Option<u32>,
        stat_hash: &str,
    ) -> Option<TierInfo> {
        let tier = self.tiers.get(current.checked_sub(1)? as usize)?;
        let (count, best_available) = self.fit(current, item_level);
        let index = self.stats.iter().position(|&stat| stat == stat_hash);
        let range = index.and_then(|index| {
            self.tiers
                .iter()
                .filter_map(|tier| tier.ranges[index])
                .reduce(|(lo, hi), (tier_lo, tier_hi)| (lo.min(tier_lo), hi.max(tier_hi)))
        });
        Some(TierInfo {
            current,
            count,
            best_available,
            min_level: tier.level,
            tier_floor: index.and_then(|index| tier.ranges[index]).map(|(lo, _)| lo),
            range,
        })
    }
}

/// The table, by family key.
static FAMILIES: LazyLock<HashMap<&'static str, Vec<Family>>> = LazyLock::new(|| {
    parse(include_str!("../data/mod-tiers.tsv")).expect("data/mod-tiers.tsv is well-formed")
});

/// `stat_id`'s hash: the trade id without its mod type and option (`explicit.stat_3891355829|2`
/// is `stat_3891355829`).
pub fn stat_hash(stat_id: &str) -> &str {
    let hash = stat_id.split_once('.').map_or(stat_id, |(_, hash)| hash);
    hash.split_once('|').map_or(hash, |(hash, _)| hash)
}

/// A line's value and range as the item prints them: a line the parser negated into the
/// catalog's terms (`15(10-20)% reduced` read as -15 in [-20, -10]) turned back, as PoE Overlay
/// II reads a roll's share from the printed numbers -- and as the table's ranges read.
pub fn printed(stat: &ParsedStat) -> (f64, f64, f64) {
    if stat.negated_text.is_some() {
        (-stat.value, -stat.max, -stat.min)
    } else {
        (stat.value, stat.min, stat.max)
    }
}

/// `modifier`'s family on an item of trade `category`: by its slot and the trade stats of its
/// lines, a desecrated mod's own pool before the ordinary one (a desecration can reveal an
/// ordinary mod, which the client prints as desecrated too). `None` for a mod no family has: an
/// implicit, a unique's, one with a line the parser could not resolve.
pub(crate) fn family(modifier: &ParsedModifier, category: &str) -> Option<&'static Family> {
    let slot = modifier.info.generation?;
    let mut hashes = Vec::with_capacity(modifier.stats.len());
    for stat in &modifier.stats {
        hashes.push(stat_hash(stat.stat_id.as_deref()?));
    }
    hashes.sort_unstable();
    hashes.dedup();
    let families = FAMILIES.get(hashes.join("+").as_str())?;
    let pools: &[Pool] = if modifier.info.modifier_type == ModifierType::Desecrated {
        &[Pool::Desecrated, Pool::Affix]
    } else {
        &[Pool::Affix]
    };
    pools.iter().find_map(|&pool| {
        families.iter().find(|family| {
            family.slot == slot && family.category == category && family.pool == pool
        })
    })
}

/// How far a roll may print outside its tier's range and still be read as in it: the ranges are
/// the printed numbers, or means of two, which a float can miss by a rounding.
const ROLL_SLACK: f64 = 1e-6;

/// A mod as the game knows it, for a tool that takes an item by the game's own ids (Craft of
/// Exile's item import).
#[derive(Debug, Clone, PartialEq)]
pub struct GameMod {
    /// RePoE's id of the mod (`IncreasedLife7`), the game's own.
    pub id: &'static str,
    /// The roll of each of the mod's stats, in the game's order of them; `None` when the item's
    /// text can't give each its roll: a line without a number, a hidden stat.
    pub rolls: Option<Vec<Roll>>,
}

/// Where the item has the roll of one of a mod's stats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Roll {
    /// The `number`th number (from 0) the modifier's `stats[line]` prints: `Adds 1 to 50
    /// Lightning Damage` prints two.
    Printed { line: usize, number: usize },
    /// A number the item doesn't print: a waystone mod's share of its properties.
    Fixed(f64),
}

/// `modifier`'s mod on `item` as the game knows it: the tier the item prints, once each of its
/// rolls lies in that tier's range -- a roll outside it says the table describes another version
/// of the game than the item does, and the id would be a guess. `None` too for a mod no family
/// has, or with a tier the table doesn't list, and for a crafted one, which no family's tiers are.
/// A waystone's mods roll by its tier: its families are the tier's own (`map.waystone:<tier>`),
/// which the search never meets.
pub fn game_mod(item: &ParsedItem, modifier: &ParsedModifier) -> Option<GameMod> {
    if modifier.info.modifier_type == ModifierType::Crafted {
        return None;
    }
    let category = item.category.as_ref().map_or("", |category| &category.id);
    let waystone_tier = item.waystone.as_ref().and_then(|waystone| waystone.tier);
    let family = match waystone_tier {
        Some(tier) if category == "map.waystone" => {
            family(modifier, &format!("map.waystone:{tier}"))?
        }
        _ => family(modifier, category)?,
    };
    let tier = family
        .tiers
        .get(modifier.info.tier?.checked_sub(1)? as usize)?;
    // The key stat each of the modifier's lines prints.
    let keys = modifier
        .stats
        .iter()
        .map(|stat| {
            let hash = stat_hash(stat.stat_id.as_deref()?);
            family.stats.iter().position(|&stat| stat == hash)
        })
        .collect::<Option<Vec<usize>>>()?;
    let fits = modifier.stats.iter().zip(&keys).all(|(stat, &key)| {
        tier.ranges[key]
            .is_none_or(|(lo, hi)| (lo - ROLL_SLACK..=hi + ROLL_SLACK).contains(&printed(stat).0))
    });
    if !fits {
        return None;
    }
    let rolls = tier.order.as_ref().and_then(|order| {
        let mut taken = vec![0; family.stats.len()];
        order
            .iter()
            .map(|&source| match source {
                Source::Fixed(value) => Some(Roll::Fixed(value)),
                Source::Key(key) => {
                    let line = keys.iter().position(|&line_key| line_key == key)?;
                    taken[key] += 1;
                    Some(Roll::Printed {
                        line,
                        number: taken[key] - 1,
                    })
                }
            })
            .collect()
    });
    Some(GameMod {
        id: tier.mod_id,
        rolls,
    })
}

/// Reads the table: one row per tier, `key slot category pool level mod_id tags ranges order`, a
/// family's rows consecutive and T1 first.
fn parse(table: &'static str) -> Result<HashMap<&'static str, Vec<Family>>, String> {
    let mut families: HashMap<&'static str, Vec<Family>> = HashMap::new();
    let mut last: Option<(&str, &str, &str, &str)> = None;
    for (number, line) in table.lines().enumerate() {
        let fields: Vec<&'static str> = line.split('\t').collect();
        let [
            key,
            slot,
            category,
            pool,
            level,
            mod_id,
            tags,
            ranges,
            order,
        ] = fields[..]
        else {
            return Err(format!("line {}: {} fields", number + 1, fields.len()));
        };
        let bad = |what: &str| format!("line {}: bad {what}", number + 1);
        let stats: Vec<&'static str> = key.split('+').collect();
        let tier = Tier {
            level: level.parse().map_err(|_| bad("level"))?,
            mod_id,
            tags: tags
                .split(',')
                .filter(|tag| !tag.is_empty())
                .map(|tag| Tag::parse(tag).ok_or_else(|| bad("tag")))
                .collect::<Result<_, _>>()?,
            ranges: ranges
                .split(',')
                .map(|range| parse_range(range).ok_or_else(|| bad("range")))
                .collect::<Result<_, _>>()?,
            order: match order {
                "_" => None,
                order => Some(
                    order
                        .split(',')
                        .map(|source| match source.strip_prefix('=') {
                            Some(value) => value.parse().ok().map(Source::Fixed),
                            None => source
                                .parse()
                                .ok()
                                .filter(|&key| key < stats.len())
                                .map(Source::Key),
                        })
                        .collect::<Option<_>>()
                        .ok_or_else(|| bad("order"))?,
                ),
            },
        };
        if tier.ranges.len() != stats.len() {
            return Err(bad("range count"));
        }
        let family_key = (key, slot, category, pool);
        let same = last == Some(family_key);
        last = Some(family_key);
        let entries = families.entry(key).or_default();
        if same {
            let family = entries.last_mut().ok_or_else(|| bad("family"))?;
            if family
                .tiers
                .last()
                .is_some_and(|above| above.level < tier.level)
            {
                return Err(bad("tier order"));
            }
            family.tiers.push(tier);
            continue;
        }
        let family = Family {
            slot: match slot {
                "p" => ModGeneration::Prefix,
                "s" => ModGeneration::Suffix,
                _ => return Err(bad("slot")),
            },
            category,
            pool: match pool {
                "a" => Pool::Affix,
                "d" => Pool::Desecrated,
                _ => return Err(bad("pool")),
            },
            stats,
            tiers: vec![tier],
        };
        if entries.iter().any(|other| {
            other.slot == family.slot && other.category == category && other.pool == family.pool
        }) {
            return Err(bad("family: listed twice"));
        }
        entries.push(family);
    }
    Ok(families)
}

/// A range `min:max`, or `_` for a flag's none; `None` when it is neither.
fn parse_range(range: &str) -> Option<Option<(f64, f64)>> {
    if range == "_" {
        return Some(None);
    }
    let (lo, hi) = range.split_once(':')?;
    Some(Some((lo.parse().ok()?, hi.parse().ok()?)))
}

#[cfg(test)]
mod tests {
    use poe2_domain::{ModifierInfo, ParsedStat};

    use super::*;

    #[test]
    fn the_table_parses_with_one_block_per_family_and_its_tiers_best_first() {
        // `parse` rejects a malformed row, a family listed twice or split, and a tier needing a
        // higher level than the one above it.
        let families = parse(include_str!("../data/mod-tiers.tsv")).expect("the table parses");
        let tiers: usize = families.values().flatten().map(|f| f.tiers.len()).sum();
        assert_eq!(tiers, include_str!("../data/mod-tiers.tsv").lines().count());
        for family in families.values().flatten() {
            assert!(!family.tiers.is_empty());
            let mut stats = family.stats.clone();
            stats.sort_unstable();
            stats.dedup();
            assert_eq!(stats, family.stats, "a key lists its stats sorted, once");
            for tier in &family.tiers {
                assert!(
                    tier.ranges.iter().flatten().all(|(lo, hi)| lo <= hi),
                    "{}: ranges ascend",
                    tier.mod_id
                );
            }
        }
        assert!(
            parse("stat_1\tp\tjewel\ta\t10\tA\t\t1:2\t0\nstat_1\tp\tjewel\ta\t20\tB\t\t3:4\t0\n")
                .is_err()
        );
        assert!(parse("stat_1\tp\tjewel\ta\t10\tA\tLife\t1:2,3:4\t0\n").is_err());
        assert!(
            parse("stat_1\tp\tjewel\ta\t10\tA\t\t1:2\t1\n").is_err(),
            "an order naming a stat the key hasn't"
        );
        assert!(
            parse(
                "stat_1\tp\tjewel\ta\t10\tA\t\t1:2\t0\nstat_1\ts\tjewel\ta\t10\tB\t\t1:2\t0\n\
                 stat_1\tp\tjewel\ta\t1\tC\t\t1:2\t0\n"
            )
            .is_err(),
            "a family split in two"
        );
        assert!(
            parse("stat_1\tp\tjewel\ta\t10\tA\t\t1:2\t0,0\nstat_1\tp\tjewel\ta\t1\tB\t\t1:2\t_\n")
                .is_ok()
        );
    }

    fn life_prefix(stat_id: &str, modifier_type: ModifierType) -> ParsedModifier {
        ParsedModifier {
            info: ModifierInfo {
                modifier_type,
                generation: Some(ModGeneration::Prefix),
                name: None,
                tier: Some(3),
                rank: None,
                tags: Vec::new(),
            },
            stats: vec![ParsedStat {
                stat_id: Some(stat_id.to_owned()),
                text: String::new(),
                value: 0.0,
                min: 0.0,
                max: 0.0,
                dp: false,
                unscalable: false,
                negated_text: None,
                printed_text: None,
            }],
        }
    }

    #[test]
    fn a_body_armours_life_prefix_has_thirteen_tiers_and_tells_where_its_roll_sits() {
        // RePoE 4.5.5.2: `IncreasedLife1`-`IncreasedLife13` roll on body armours, T1 (200-214)
        // at item level 80, T13 (10-19) at 1.
        let family = family(
            &life_prefix("explicit.stat_3299347043", ModifierType::Explicit),
            "armour.chest",
        )
        .expect("the life family");
        assert_eq!(family.tiers.len(), 13);
        assert_eq!(family.tiers[0].mod_id, "IncreasedLife13");
        assert_eq!(family.tiers[0].tags, [Tag::Life]);

        // An item level 75 body armour: T1 (level 80) is out of its reach, T2 (75) is not.
        assert_eq!(family.fit(3, Some(75)), (13, 2));
        let info = family
            .tier_info(3, Some(75), "stat_3299347043")
            .expect("tier 3");
        assert_eq!(info.min_level, 70);
        assert_eq!(info.tier_floor, Some(175.0));
        assert_eq!(info.range, Some((10.0, 214.0)));

        // An unresolved line has no family; nor has a desecrated copy of a stat no desecrated
        // family prints -- it is the ordinary family's.
        assert!(family_of("explicit.stat_1", ModifierType::Explicit).is_none());
        assert!(std::ptr::eq(
            family_of("explicit.stat_3299347043", ModifierType::Desecrated).expect("ordinary"),
            family
        ));
    }

    fn family_of(stat_id: &str, modifier_type: ModifierType) -> Option<&'static Family> {
        family(&life_prefix(stat_id, modifier_type), "armour.chest")
    }

    /// A mod of printed `tier` in `generation` whose lines roll `stats` (trade id, value).
    fn rolled_mod(generation: ModGeneration, tier: u32, stats: &[(&str, f64)]) -> ParsedModifier {
        let mut modifier = life_prefix("", ModifierType::Explicit);
        modifier.info.generation = Some(generation);
        modifier.info.tier = Some(tier);
        modifier.stats = stats
            .iter()
            .map(|&(stat_id, value)| ParsedStat {
                stat_id: Some(stat_id.to_owned()),
                value,
                ..modifier.stats[0].clone()
            })
            .collect();
        modifier
    }

    fn item_of(category: &str) -> ParsedItem {
        ParsedItem {
            category: Some(poe2_domain::ItemCategory {
                id: category.to_owned(),
                display_name: String::new(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_mod_is_its_printed_tier_while_its_rolls_fit_it_with_its_stats_in_the_games_order() {
        // T3 of the body armour life prefixes, `IncreasedLife11`, rolls 175-189.
        let chest = item_of("armour.chest");
        let life = |value| {
            rolled_mod(
                ModGeneration::Prefix,
                3,
                &[("explicit.stat_3299347043", value)],
            )
        };
        assert_eq!(
            game_mod(&chest, &life(180.0)),
            Some(GameMod {
                id: "IncreasedLife11",
                rolls: Some(vec![Roll::Printed { line: 0, number: 0 }]),
            })
        );
        // A roll T3 can't have: the table describes another game than the item, no id.
        assert_eq!(game_mod(&chest, &life(150.0)), None);
        // Nor for a crafted copy, which no family lists.
        let mut crafted = life(180.0);
        crafted.info.modifier_type = ModifierType::Crafted;
        assert_eq!(game_mod(&chest, &crafted), None);

        // A ring's light radius and mana regeneration suffix prints its mana line first; the
        // game lists light radius first (`LightRadiusAndManaRegeneration1`, T3: 5, 8-12).
        let hybrid = rolled_mod(
            ModGeneration::Suffix,
            3,
            &[
                ("explicit.stat_789117908", 8.0),
                ("explicit.stat_1263695895", 5.0),
            ],
        );
        assert_eq!(
            game_mod(&item_of("accessory.ring"), &hybrid),
            Some(GameMod {
                id: "LightRadiusAndManaRegeneration1",
                rolls: Some(vec![
                    Roll::Printed { line: 1, number: 0 },
                    Roll::Printed { line: 0, number: 0 },
                ]),
            })
        );

        // A two-hand mace's added lightning prints both its numbers on one line, which the
        // parser rolls at their mean (T7, `LocalAddedLightningDamageTwoHand4`: 1-4 to 53-76,
        // rolling 27-40).
        let lightning = rolled_mod(
            ModGeneration::Prefix,
            7,
            &[("explicit.stat_3336890334", 30.0)],
        );
        assert_eq!(
            game_mod(&item_of("weapon.twomace"), &lightning),
            Some(GameMod {
                id: "LocalAddedLightningDamageTwoHand4",
                rolls: Some(vec![
                    Roll::Printed { line: 0, number: 0 },
                    Roll::Printed { line: 0, number: 1 },
                ]),
            })
        );
    }
}
