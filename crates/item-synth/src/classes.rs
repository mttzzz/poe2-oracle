//! Which item class an item is, which a listing doesn't say: by its base type, from the game's
//! own base item table as [RePoE](https://repoe-fork.github.io/poe2/) exports it
//! (`base_items.json`, `item_classes.json`), and the class's name in each client language from
//! `item_parser::categories::ITEM_CLASSES`, the game's `ItemClasses` table.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use item_parser::ItemLanguage;
use item_parser::categories::{ITEM_CLASSES, ItemClass};
use serde::Deserialize;

/// The RePoE files [`Classes::load`] reads, and where the tool downloads them from.
pub const REPOE_FILES: [(&str, &str); 2] = [
    (
        "base_items.json",
        "https://repoe-fork.github.io/poe2/base_items.json",
    ),
    (
        "item_classes.json",
        "https://repoe-fork.github.io/poe2/item_classes.json",
    ),
];

#[derive(Deserialize)]
struct BaseItem {
    name: String,
    item_class: String,
    release_state: Option<String>,
}

#[derive(Deserialize)]
struct RepoeClass {
    name: String,
}

/// Each released base type's item class, by its English name.
pub struct Classes {
    by_base: BTreeMap<String, Vec<&'static ItemClass>>,
}

impl Classes {
    /// From the RePoE files in `dir`.
    pub fn load(dir: &Path) -> Result<Classes> {
        let read = |name: &str| {
            let path = dir.join(name);
            std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
        };
        let bases: BTreeMap<String, BaseItem> =
            serde_json::from_str(&read("base_items.json")?).context("base_items.json")?;
        let classes: BTreeMap<String, RepoeClass> =
            serde_json::from_str(&read("item_classes.json")?).context("item_classes.json")?;
        let mut by_base: BTreeMap<String, Vec<&'static ItemClass>> = BTreeMap::new();
        for base in bases.into_values() {
            if base.release_state.as_deref() != Some("released") {
                continue;
            }
            let Some(class) = classes.get(&base.item_class).and_then(|class| {
                item_parser::categories::find(ItemLanguage::English, &class.name)
            }) else {
                continue;
            };
            let found = by_base.entry(base.name).or_default();
            if !found.contains(&class) {
                found.push(class);
            }
        }
        Ok(Classes { by_base })
    }

    /// The class of the base type named `base` in English.
    pub fn of(&self, base: &str) -> Result<&'static ItemClass> {
        match self.by_base.get(base).map(Vec::as_slice) {
            Some([class]) => Ok(class),
            Some(several) if !several.is_empty() => bail!(
                "{base} is a base of several classes ({}); name its class",
                several
                    .iter()
                    .map(|class| class.en)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            _ => bail!("no item class known for {base}; name its class"),
        }
    }
}

/// The class the game calls `name` in English.
pub fn named(name: &str) -> Result<&'static ItemClass> {
    ITEM_CLASSES
        .iter()
        .find(|class| class.en == name)
        .with_context(|| format!("the game has no item class {name}"))
}
