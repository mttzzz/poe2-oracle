//! The game's items by name in both client languages, with their English reference name and art
//! -- from Exiled Exchange 2's generated item database (`assets/data/item-refs.tsv`, built by
//! `packaging/data/generate_item_refs.py`; EE2 is MIT, see `assets/data/NOTICE`). The English
//! name is what poe2db and the wiki name their pages by, a Russian item's included; the art is
//! the trade site's own.

use std::collections::HashMap;
use std::sync::LazyLock;

use poe2_domain::{ItemRarity, ParsedItem};

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

/// One item: its English reference name and art.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemRef {
    pub ref_name: &'static str,
    icon_tail: &'static str,
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
}

/// Both languages' names of every item, by kind. Built on first use: ~3,600 table rows.
static INDEX: LazyLock<HashMap<(RefKind, &'static str), ItemRef>> = LazyLock::new(|| {
    let mut index = HashMap::new();
    for line in DATA.lines() {
        let mut fields = line.split('\t');
        let (Some(kind), Some(ref_name), Some(ru_name), Some(icon_tail)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let kind = match kind {
            "gem" => RefKind::Gem,
            "item" => RefKind::Item,
            "unique" => RefKind::Unique,
            _ => continue,
        };
        let item = ItemRef {
            ref_name,
            icon_tail,
        };
        // A repeated name keeps its first row, the one EE2 lists first.
        index.entry((kind, ref_name)).or_insert(item);
        index.entry((kind, ru_name)).or_insert(item);
    }
    index
});

/// The item named `name` -- in English or Russian -- among `kind`.
pub fn lookup(kind: RefKind, name: &str) -> Option<ItemRef> {
    INDEX.get(&(kind, name)).copied()
}

/// What a copied item is in the database: a unique by its own name, a gem by its name, anything
/// else by its base type (a magic or rare item's name is made up) -- an unidentified unique,
/// whose name is its base, falls back the same way.
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
    lookup(RefKind::Item, base).or_else(|| lookup(RefKind::Item, &item.name))
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
}
