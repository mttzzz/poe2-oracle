//! Parses, filters and routes a batch of real clipboard item texts against a trade site's real
//! catalogs, the way the Price Check panel does, and lists every text it couldn't fully handle:
//! the coverage check for texts swept from a vendor, the stash or an inventory after a patch or a
//! new league.
//!
//! ```text
//! cargo run -p item-parser --example sweep -- <cache dir> <texts file>
//! ```
//!
//! `<cache dir>` holds the app's catalog caches (`%LOCALAPPDATA%\poe2-oracle\cache`:
//! `stat-catalog[-ru].json`, `static-items[-ru].json`, `item-types[-ru].json`); each text goes
//! through its own language's set. The texts file holds the texts separated by lines starting with
//! `####`. Prints one line per text, then the troubled ones in full; exits non-zero if any.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use item_parser::{ItemLanguage, ParseError, detect_item_language, parse_clipboard};
use poe2_domain::{ParsedItem, StatCatalog};
use trade_client::catalog::{ItemTypeEntry, StaticCurrency};
use trade_client::{SearchRoute, route_search};

struct Catalogs {
    stats: StatCatalog,
    currencies: Vec<StaticCurrency>,
    item_types: Vec<ItemTypeEntry>,
}

fn read(dir: &Path, name: &str) -> String {
    let path = dir.join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

/// One cached catalog file, deserialized into whatever the field needs.
macro_rules! cached {
    ($dir:expr, $name:expr) => {{
        let name = $name;
        serde_json::from_str(&read($dir, &name)).unwrap_or_else(|err| panic!("{name}: {err}"))
    }};
}

fn catalogs(dir: &Path, suffix: &str) -> Catalogs {
    Catalogs {
        stats: cached!(dir, format!("stat-catalog{suffix}.json")),
        currencies: cached!(dir, format!("static-items{suffix}.json")),
        item_types: cached!(dir, format!("item-types{suffix}.json")),
    }
}

/// The lines the panel would show as unread: whole modifiers none of whose lines resolved, and
/// single lines without a trade stat id inside resolved ones.
fn unread_lines(item: &ParsedItem) -> Vec<String> {
    let whole = item.unknown_mods.iter().map(|(text, _)| text.clone());
    let single = item
        .mods
        .iter()
        .flat_map(|modifier| &modifier.stats)
        .filter(|stat| stat.stat_id.is_none())
        .map(|stat| stat.text.clone());
    whole.chain(single).collect()
}

fn route_label(route: &SearchRoute) -> String {
    match route {
        SearchRoute::Market { trade_id } => format!("market {trade_id}"),
        SearchRoute::Exact { exact_type } => format!("exact {exact_type}"),
        SearchRoute::Filtered { .. } => "filtered".to_owned(),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, cache_dir, texts] = args.as_slice() else {
        eprintln!("usage: sweep <cache dir> <texts file>");
        return ExitCode::FAILURE;
    };
    let cache_dir = Path::new(cache_dir);
    let international = catalogs(cache_dir, "");
    let russian = catalogs(cache_dir, "-ru");
    let texts = std::fs::read_to_string(texts).expect("the texts file");

    let mut routes = BTreeMap::<String, usize>::new();
    let mut troubled = Vec::new();
    let mut count = 0;
    let mut chunk = String::new();
    let mut chunks = Vec::new();
    for line in texts.lines() {
        if line.starts_with("####") {
            chunks.push(std::mem::take(&mut chunk));
        } else {
            chunk.push_str(line);
            chunk.push('\n');
        }
    }
    chunks.push(chunk);
    for text in chunks.iter().filter(|text| !text.trim().is_empty()) {
        count += 1;
        let Some(language) = detect_item_language(text) else {
            troubled.push(("no item language".to_owned(), text));
            continue;
        };
        let catalogs = match language {
            ItemLanguage::English => &international,
            ItemLanguage::Russian => &russian,
        };
        let item = match parse_clipboard(text, language, &catalogs.stats) {
            Ok(item) => item,
            // A gamble offer: understood, with nothing to price.
            Err(ParseError::Unrevealed) => {
                *routes.entry("unrevealed".to_owned()).or_default() += 1;
                continue;
            }
            Err(err) => {
                troubled.push((format!("not parsed: {err:?}"), text));
                continue;
            }
        };
        let filters = stat_filters::build_filters(
            &item,
            stat_filters::SearchProfile::default_for(&item),
            &catalogs.stats,
        );
        let route = route_search(&item, &catalogs.currencies, &catalogs.item_types);
        let kind = route_label(&route);
        *routes
            .entry(kind.split(' ').next().unwrap_or_default().to_owned())
            .or_default() += 1;
        let category = item
            .category
            .as_ref()
            .map_or("-", |category| category.id.as_str());
        println!(
            "{} / {} [{category}] -> {kind} ({} rows)",
            item.name,
            item.base_type.as_deref().unwrap_or("-"),
            filters.len()
        );
        let unread = unread_lines(&item);
        // A filtered search without a category searches every kind of item.
        if item.category.is_none() && matches!(route, SearchRoute::Filtered { .. }) {
            troubled.push(("filtered search without a category".to_owned(), text));
        }
        if !unread.is_empty() {
            troubled.push((format!("unread: {}", unread.join(" | ")), text));
        }
    }

    println!(
        "\n{count} texts; routes {routes:?}; {} troubled",
        troubled.len()
    );
    for (why, text) in &troubled {
        println!("\n==== {why}\n{text}");
    }
    if troubled.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
