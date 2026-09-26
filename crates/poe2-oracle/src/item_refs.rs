//! The game's items by name in both client languages, with their English reference name and art
//! -- from Exiled Exchange 2's generated item database (`assets/data/item-refs.tsv`, built by
//! `packaging/data/generate_item_refs.py`; EE2 is MIT, see `assets/data/NOTICE`). The English
//! name is what poe2db and the wiki name their pages by, a Russian item's included; the art is
//! the trade site's own. A base Craft of Exile crafts also carries the game's own ids of the bases
//! so named and the trade stats of their implicits, from RePoE's export of the game's tables
//! (`bases`).
//!
//! The table is built in, and a game data pack (`data_pack`) may bring a newer one for a run:
//! [`read_table`] reads a pack's table, refusing a malformed one, and [`use_table`] puts it in
//! place before the table is first read.

use std::collections::HashMap;
use std::sync::LazyLock;

use poe2_domain::pack_table::{PackTable, TableInUse};
use poe2_domain::{ItemRarity, ParsedItem, ParsedModifier};

const DATA: &str = include_str!("../assets/data/item-refs.tsv");

/// Every art URL in the database starts with this; the table stores the rest.
const ICON_PREFIX: &str = "https://web.poecdn.com/gen/image/";

/// The database's item kinds -- names are unique only within one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RefKind {
    Gem,
    Item,
    Unique,
}

/// One item: its English reference name and art, in a table whose text lives for `'a`; the table
/// in use is `'static` ([`ItemRef`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemRefOf<'a> {
    pub ref_name: &'a str,
    icon_tail: &'a str,
    /// The table's fifth field: the bases of the name Craft of Exile crafts (`bases` reads it).
    bases: &'a str,
}

/// One item: its English reference name and art.
pub type ItemRef = ItemRefOf<'static>;

/// One of the game's bases: its metadata id and its implicits, from a table whose text lives for
/// `'a`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseOf<'a> {
    /// The game's own id of the base (`Metadata/Items/Rings/FourRing9`).
    pub id: &'a str,
    pub implicits: Vec<ImplicitOf<'a>>,
}

/// One of the game's bases ([`BaseOf`]).
pub type Base = BaseOf<'static>;

/// One implicit of a base, from a table whose text lives for `'a`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImplicitOf<'a> {
    /// Each of its lines' trade stat hashes (`stat_2901986750`), two where two stats print alike,
    /// none for a line no trade stat prints.
    pub lines: Vec<Vec<&'a str>>,
    /// The line printing each of its stats, in the game's order of them (a line of two numbers
    /// twice); `None` when the item's text can't give each of them its roll.
    pub order: Option<Vec<usize>>,
}

/// One implicit of a base ([`ImplicitOf`]).
pub type Implicit = ImplicitOf<'static>;

impl ImplicitOf<'_> {
    /// Which of `modifier`'s stats prints each of the implicit's lines: `None` unless each line
    /// has its own, one of the trade stats the line prints as.
    pub fn lines_in(&self, modifier: &ParsedModifier) -> Option<Vec<usize>> {
        if self.lines.len() != modifier.stats.len() {
            return None;
        }
        let mut taken = vec![false; modifier.stats.len()];
        self.lines
            .iter()
            .map(|hashes| {
                let stat = modifier
                    .stats
                    .iter()
                    .enumerate()
                    .position(|(index, stat)| {
                        !taken[index]
                            && stat.stat_id.as_deref().is_some_and(|stat_id| {
                                hashes.contains(&stat_filters::stat_hash(stat_id))
                            })
                    })?;
                taken[stat] = true;
                Some(stat)
            })
            .collect()
    }
}

/// A base as the table writes it: the metadata id, then `;` and each implicit.
fn parse_base(field: &str) -> Option<BaseOf<'_>> {
    let mut parts = field.split(';');
    let id = parts.next().filter(|id| !id.is_empty())?;
    let implicits = parts
        .map(|implicit| {
            let (lines, order) = match implicit.split_once('=') {
                Some((lines, order)) => (lines, Some(order)),
                None => (implicit, None),
            };
            let lines: Vec<Vec<&str>> = lines
                .split(',')
                .map(|line| match line {
                    "?" => Vec::new(),
                    hashes => hashes.split('|').collect(),
                })
                .collect();
            let order = match order {
                Some(order) => Some(
                    order
                        .split(',')
                        .map(|line| line.parse().ok().filter(|&line| line < lines.len()))
                        .collect::<Option<_>>()?,
                ),
                None => None,
            };
            Some(ImplicitOf { lines, order })
        })
        .collect::<Option<_>>()?;
    Some(BaseOf { id, implicits })
}

impl ItemRef {
    /// The item's art on the trade site's CDN; `None` where the database has none.
    pub fn icon_url(&self) -> Option<String> {
        match self.icon_tail {
            "" => None,
            tail if tail.starts_with("http") => Some(tail.to_owned()),
            tail => Some(format!("{ICON_PREFIX}{tail}")),
        }
    }

    /// The item's poe2db page, in Russian or English. poe2db names every page after the English
    /// name, apostrophes dropped and spaces as underscores (`Atziris_Rule`,
    /// `The_Knight-errant`), whatever the page's language.
    pub fn poe2db_url(&self, russian: bool) -> String {
        let slug: String = self
            .ref_name
            .chars()
            .filter(|&c| c != '\'')
            .map(|c| if c == ' ' { '_' } else { c })
            .collect();
        let language = if russian { "ru" } else { "us" };
        format!("https://poe2db.tw/{language}/{slug}")
    }

    /// The item's page on the community wiki, which is English only.
    pub fn wiki_url(&self) -> String {
        format!(
            "https://www.poe2wiki.net/wiki/{}",
            self.ref_name.replace(' ', "_")
        )
    }

    /// The game's bases of the item's name that Craft of Exile crafts: none for anything else,
    /// several where bases share a name (`Two-Stone Ring`, one per pair of resistances).
    pub fn bases(&self) -> Vec<Base> {
        self.bases
            .split(' ')
            .filter(|base| !base.is_empty())
            .filter_map(parse_base)
            .collect()
    }
}

/// A data pack's table, waiting for the first read of [`INDEX`].
static PACK: PackTable<ItemRefs<'static>> = PackTable::new();

/// Both languages' names of every item, by kind. Built on first use: ~3,600 table rows.
static INDEX: LazyLock<HashMap<(RefKind, &'static str), ItemRef>> = LazyLock::new(|| {
    PACK.take().map_or_else(
        || {
            read_table(DATA)
                .expect("assets/data/item-refs.tsv is well-formed")
                .0
        },
        |pack| pack.0,
    )
});

/// The item reference table, read ([`read_table`]) from a text that lives for `'a`.
pub struct ItemRefs<'a>(HashMap<(RefKind, &'a str), ItemRefOf<'a>>);

/// Reads an item reference table: one row per item -- its kind (`gem`, `item` or `unique`), its
/// English and Russian names, its art (empty for none) and its bases, each as `parse_base` reads
/// one. An error names the first row that isn't one; a table without rows is one too.
pub fn read_table(table: &str) -> Result<ItemRefs<'_>, String> {
    let mut index = HashMap::new();
    for (number, line) in table.lines().enumerate() {
        let bad = |what: &str| format!("line {}: bad {what}", number + 1);
        let mut fields = line.split('\t');
        let (Some(kind), Some(ref_name), Some(ru_name), Some(icon_tail), Some(bases), None) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            let count = line.split('\t').count();
            return Err(format!("line {}: {count} fields", number + 1));
        };
        let kind = match kind {
            "gem" => RefKind::Gem,
            "item" => RefKind::Item,
            "unique" => RefKind::Unique,
            _ => return Err(bad("kind")),
        };
        if ref_name.is_empty() || ru_name.is_empty() {
            return Err(bad("name"));
        }
        if bases
            .split(' ')
            .filter(|base| !base.is_empty())
            .any(|base| parse_base(base).is_none())
        {
            return Err(bad("base"));
        }
        let item = ItemRefOf {
            ref_name,
            icon_tail,
            bases,
        };
        // A repeated name keeps its first row, the one EE2 lists first.
        index.entry((kind, ref_name)).or_insert(item);
        index.entry((kind, ru_name)).or_insert(item);
    }
    if index.is_empty() {
        return Err("no rows".to_owned());
    }
    Ok(ItemRefs(index))
}

/// Makes `table`, a data pack's ([`read_table`]), the one items are looked up in for the rest of
/// the run. Refused once the table has been read: `data_pack::activate` puts a pack's table in
/// place before anything reads it.
pub fn use_table(table: ItemRefs<'static>) -> Result<(), TableInUse> {
    PACK.set(table)
}

/// The item named `name` -- in English or Russian -- among `kind`.
pub fn lookup(kind: RefKind, name: &str) -> Option<ItemRef> {
    INDEX.get(&(kind, name)).copied()
}

/// What a copied item is in the database: a unique by its own name, a gem by its name, anything
/// else by its base type (a magic or rare item's name is made up) -- an unidentified unique,
/// whose name is its base, falls back the same way; a magic item's name holds its base
/// (`magic_base`).
pub fn refs_for(item: &ParsedItem) -> Option<ItemRef> {
    let base = item.base_type.as_deref().unwrap_or(&item.name);
    let is_gem = item
        .category
        .as_ref()
        .is_some_and(|category| category.id == "gem" || category.id.starts_with("gem."));
    if item.rarity == Some(ItemRarity::Unique)
        && let Some(unique) = lookup(RefKind::Unique, &item.name)
    {
        return Some(unique);
    }
    if is_gem && let Some(gem) = lookup(RefKind::Gem, &item.name) {
        return Some(gem);
    }
    lookup(RefKind::Item, base)
        .or_else(|| lookup(RefKind::Item, &item.name))
        .or_else(|| magic_base(item))
}

/// A magic item's base: its name wraps the base in affixes ("Crackling Temple Maul of the
/// Brute", "Копьеносный Изумруд пригвождения"), so the base is the longest run of whole words
/// naming an item, the earliest of the longest -- EE2's `magicBasetype`, which `trade_client`
/// follows in the trade site's own catalog for the search.
fn magic_base(item: &ParsedItem) -> Option<ItemRef> {
    if item.rarity != Some(ItemRarity::Magic) {
        return None;
    }
    let name = item.name.as_str();
    let mut words = Vec::new();
    let mut start = 0;
    for word in name.split(' ') {
        words.push((start, start + word.len()));
        start += word.len() + 1;
    }
    let mut best: Option<(usize, ItemRef)> = None;
    for (first, &(start, _)) in words.iter().enumerate() {
        for &(_, end) in &words[first..] {
            let run = &name[start..end];
            let length = run.chars().count();
            if best.is_some_and(|(best, _)| best >= length) {
                continue;
            }
            if let Some(item) = lookup(RefKind::Item, run) {
                best = Some((length, item));
            }
        }
    }
    best.map(|(_, item)| item)
}

#[cfg(test)]
mod tests {
    use poe2_domain::ItemCategory;

    use super::*;

    fn item(
        rarity: Option<ItemRarity>,
        category: &str,
        name: &str,
        base: Option<&str>,
    ) -> ParsedItem {
        ParsedItem {
            rarity,
            category: Some(ItemCategory {
                id: category.to_owned(),
                display_name: String::new(),
            }),
            name: name.to_owned(),
            base_type: base.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn a_russian_item_links_to_its_english_named_pages() {
        let ring = refs_for(&item(
            Some(ItemRarity::Rare),
            "accessory.ring",
            "Руна кольцо",
            Some("Радужное кольцо"),
        ))
        .expect("the Russian base is in the database");
        assert_eq!(ring.ref_name, "Prismatic Ring");
        assert_eq!(ring.poe2db_url(true), "https://poe2db.tw/ru/Prismatic_Ring");
        assert_eq!(
            ring.wiki_url(),
            "https://www.poe2wiki.net/wiki/Prismatic_Ring"
        );
        assert!(
            ring.icon_url()
                .is_some_and(|url| url.starts_with(ICON_PREFIX) && url.ends_with(".png"))
        );
    }

    #[test]
    fn poe2db_drops_apostrophes_and_keeps_hyphens() {
        let unique = lookup(RefKind::Unique, "Atziri's Acuity").expect("a known unique");
        assert_eq!(
            unique.poe2db_url(false),
            "https://poe2db.tw/us/Atziris_Acuity"
        );
        let knight = lookup(RefKind::Unique, "The Knight-errant").expect("a known unique");
        assert_eq!(
            knight.poe2db_url(false),
            "https://poe2db.tw/us/The_Knight-errant"
        );
    }

    #[test]
    fn uniques_go_by_name_and_everything_else_by_base() {
        // An identified unique's page is its own, not its base's.
        let unique = refs_for(&item(
            Some(ItemRarity::Unique),
            "accessory.ring",
            "Дары небес",
            Some("Радужное кольцо"),
        ));
        assert_eq!(unique.map(|found| found.ref_name), Some("Gifts from Above"));
        // A gem by its name, in either language.
        let gem = refs_for(&item(None, "gem", "Вестник льда", None));
        assert_eq!(gem.map(|found| found.ref_name), Some("Herald of Ice"));
        // Nothing to link for a name the database doesn't know.
        assert_eq!(refs_for(&item(None, "currency", "Не предмет", None)), None);
    }

    #[test]
    fn a_table_with_a_malformed_row_is_refused() {
        let good = "item\tAbsent Amulet\tАмулет отсутствия\tabsent.png\t\
            Metadata/Items/Amulets/FourAmuletB1c;stat_3182714256,stat_718638445=0,1\n\
            gem\tHerald of Ice\tВестник льда\t\t\n";
        let table = read_table(good).expect("a well-formed table");
        let amulet = table.0[&(RefKind::Item, "Амулет отсутствия")];
        assert_eq!(amulet.ref_name, "Absent Amulet");
        assert!(table.0.contains_key(&(RefKind::Gem, "Herald of Ice")));

        for (bad, why) in [
            ("gem\tHerald of Ice\tВестник льда\t\n", "4 fields"),
            ("gem\tHerald of Ice\tВестник льда\t\t\tmore\n", "6 fields"),
            (
                "skill\tHerald of Ice\tВестник льда\t\t\n",
                "an unknown kind",
            ),
            ("gem\t\tВестник льда\t\t\n", "no English name"),
            ("gem\tHerald of Ice\t\t\t\n", "no Russian name"),
            (
                "item\tAbsent Amulet\tАмулет отсутствия\t\t;stat_3182714256\n",
                "a base without its id",
            ),
            (
                "item\tAbsent Amulet\tАмулет отсутствия\t\tMetadata/Items/A;stat_1=2\n",
                "an order naming a line the implicit hasn't",
            ),
            ("", "no rows"),
        ] {
            assert!(read_table(bad).is_err(), "{why}");
        }
    }

    #[test]
    fn a_pack_table_comes_too_late_once_the_built_in_one_is_read() {
        // Whatever ran first, the built-in table is in use from here.
        LazyLock::force(&INDEX);
        let pack = read_table("gem\tHerald of Ice\tВестник льда\t\t\n").unwrap();
        assert_eq!(use_table(pack), Err(TableInUse));
        assert!(INDEX.len() > 2, "the built-in table stays");
    }
}
