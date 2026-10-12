//! The library: every item made, kept for the tests and for trying in the game, one folder per
//! item under `crates/item-synth/library/`:
//!
//! - `entry.json` ([`Meta`]): what was ordered, from which search and league, its item class, what
//!   the texts make up and whether the app's search found the listing again;
//! - `item.en.json`, `item.ru.json`: the listing's item from each trade site (the seller and the
//!   price are left out);
//! - `en.txt`, `ru.txt`: the synthesized copies, with the game's own line breaks.
//!
//! `catalog/stats.{en,ru}.tsv` holds the part of each trade site's stat catalog the library's lines
//! can resolve to ([`catalog_subset`]): the parser needs a catalog, and the whole one is a megabyte
//! a language.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use item_parser::ItemLanguage;
use poe2_domain::{StatCatalog, TradeStat};
use serde::{Deserialize, Serialize};

use crate::listing::Item;

/// The library's folder in this checkout.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("library")
}

/// A value for each client language.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Languages<T> {
    pub en: T,
    pub ru: T,
}

impl<T> Languages<T> {
    pub fn get(&self, language: ItemLanguage) -> &T {
        match language {
            ItemLanguage::English => &self.en,
            ItemLanguage::Russian => &self.ru,
        }
    }

    pub fn get_mut(&mut self, language: ItemLanguage) -> &mut T {
        match language {
            ItemLanguage::English => &mut self.en,
            ItemLanguage::Russian => &mut self.ru,
        }
    }
}

/// Both client languages, English first.
pub const LANGUAGES: [ItemLanguage; 2] = [ItemLanguage::English, ItemLanguage::Russian];

pub fn code(language: ItemLanguage) -> &'static str {
    match language {
        ItemLanguage::English => "en",
        ItemLanguage::Russian => "ru",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// What was asked for, in words.
    pub order: String,
    /// The trade search that found the listing (`query` of the request body).
    pub query: serde_json::Value,
    pub league: String,
    /// When the listing was fetched, UTC (`2026-10-12T10:20:00Z`).
    pub fetched: String,
    /// The item's class as the English client names it (`Rings`).
    pub class: String,
    /// Whether the texts carry the seller's note, as a copy from a stash tab would.
    #[serde(default)]
    pub note: bool,
    /// What each text makes up ([`crate::Synthesized::guesses`]).
    pub guesses: Languages<Vec<String>>,
    /// The app's own search for the item, run on each site with the listing's seller: `found`,
    /// or what happened instead. Empty until checked.
    #[serde(default)]
    pub finds_itself: Languages<String>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub slug: String,
    pub meta: Meta,
    pub items: Languages<Item>,
    pub texts: Languages<String>,
}

impl Entry {
    pub fn load(dir: &Path) -> Result<Entry> {
        let read = |name: &str| {
            let path = dir.join(name);
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
        };
        let slug = dir
            .file_name()
            .and_then(|name| name.to_str())
            .context("an entry folder without a name")?
            .to_owned();
        Ok(Entry {
            meta: serde_json::from_str(&read("entry.json")?)
                .with_context(|| format!("{slug}/entry.json"))?,
            items: Languages {
                en: serde_json::from_str(&read("item.en.json")?)
                    .with_context(|| format!("{slug}/item.en.json"))?,
                ru: serde_json::from_str(&read("item.ru.json")?)
                    .with_context(|| format!("{slug}/item.ru.json"))?,
            },
            texts: Languages {
                en: read("en.txt")?,
                ru: read("ru.txt")?,
            },
            slug,
        })
    }

    pub fn save(&self, library: &Path) -> Result<()> {
        let dir = library.join(&self.slug);
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let write = |name: &str, content: &[u8]| {
            let path = dir.join(name);
            fs::write(&path, content).with_context(|| format!("writing {}", path.display()))
        };
        write("entry.json", &pretty(&self.meta)?)?;
        write("item.en.json", &pretty(&self.items.en)?)?;
        write("item.ru.json", &pretty(&self.items.ru)?)?;
        write("en.txt", self.texts.en.as_bytes())?;
        write("ru.txt", self.texts.ru.as_bytes())?;
        Ok(())
    }

    /// The class's name in `language`'s client.
    pub fn class(&self, language: ItemLanguage) -> Result<&'static str> {
        Ok(crate::classes::named(&self.meta.class)?.name(language))
    }
}

/// Pretty JSON, a line break at its end, as the library's files are written.
fn pretty<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    Ok(text.into_bytes())
}

/// Every entry in `library`, by slug.
pub fn entries(library: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let listing =
        fs::read_dir(library).with_context(|| format!("reading {}", library.display()))?;
    for dir in listing {
        let dir = dir?.path();
        if dir.join("entry.json").is_file() {
            entries.push(Entry::load(&dir)?);
        }
    }
    entries.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(entries)
}

/// A slug for a new entry: lower-case ASCII letters, digits and dashes.
pub fn check_slug(slug: &str) -> Result<()> {
    if slug.is_empty()
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("an entry's name takes lower-case letters, digits and dashes: {slug:?}");
    }
    Ok(())
}

fn catalog_path(library: &Path, language: ItemLanguage) -> PathBuf {
    library
        .join("catalog")
        .join(format!("stats.{}.tsv", code(language)))
}

/// The library's stat catalog for `language`: `id`, mod type and text, a tab apart.
pub fn load_catalog(library: &Path, language: ItemLanguage) -> Result<StatCatalog> {
    let path = catalog_path(library, language);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut stats = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let mut fields = line.splitn(3, '\t');
        let (Some(id), Some(mod_type), Some(text)) = (fields.next(), fields.next(), fields.next())
        else {
            bail!("{}: a line without three fields: {line:?}", path.display());
        };
        stats.push(TradeStat {
            id: id.to_owned(),
            mod_type: mod_type.to_owned(),
            text: text.replace("\\n", "\n"),
        });
    }
    Ok(StatCatalog { stats })
}

pub fn save_catalog(library: &Path, language: ItemLanguage, catalog: &StatCatalog) -> Result<()> {
    let path = catalog_path(library, language);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut stats: Vec<&TradeStat> = catalog.stats.iter().collect();
    stats.sort_by(|a, b| a.id.cmp(&b.id).then(a.text.cmp(&b.text)));
    let mut text = String::new();
    for stat in stats {
        text.push_str(&format!(
            "{}\t{}\t{}\n",
            stat.id,
            stat.mod_type,
            stat.text.replace('\n', "\\n")
        ));
    }
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

/// The words of a stat text, numbers, signs and whatever is in brackets left out -- a line's roll
/// ranges and `(rune)`, a catalog text's `(Local)` or `(На предмете)` -- and without the words a
/// stat turns into its opposite with (`increased`/`reduced`), each cut to its first three letters,
/// where a word's forms mostly agree (`use remaining` in the catalog, `uses remaining` on a tablet
/// with several): two texts with the same signature are one stat however the line rolled and
/// wherever it sits, and the few the stems join wrongly only make the catalog a little bigger.
fn signature(text: &str, language: ItemLanguage) -> Vec<String> {
    let negations = match language {
        ItemLanguage::English => item_parser::client_strings::EN.negations,
        ItemLanguage::Russian => item_parser::client_strings::RU.negations,
    };
    let mut unbracketed = String::with_capacity(text.len());
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => unbracketed.push(c),
            _ => {}
        }
    }
    let lower = unbracketed.to_lowercase();
    let mut words: Vec<String> = lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|word| !word.is_empty())
        .filter(|word| {
            !negations
                .iter()
                .any(|(one, other)| word == one || word == other)
        })
        .map(|word| word.chars().take(3).collect::<String>())
        .collect();
    words.sort();
    words.dedup();
    words
}

/// The entries of `full` the parser can resolve `lines` to: those whose text is a line's
/// [`signature`], or, for a text ending in a value (`Allocates #`), opens the line.
pub fn catalog_subset(full: &StatCatalog, lines: &[String], language: ItemLanguage) -> StatCatalog {
    let signatures: BTreeSet<Vec<String>> =
        lines.iter().map(|line| signature(line, language)).collect();
    let lowered: Vec<String> = lines.iter().map(|line| line.to_lowercase()).collect();
    let stats = full
        .stats
        .iter()
        .filter(|stat| {
            signatures.contains(&signature(&stat.text, language))
                || stat.text.strip_suffix('#').is_some_and(|head| {
                    let head = head.to_lowercase();
                    lowered.iter().any(|line| line.starts_with(&head))
                })
        })
        .cloned()
        .collect();
    StatCatalog { stats }
}
