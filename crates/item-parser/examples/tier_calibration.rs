//! Throwaway: every affix row `build_filters` makes for real item texts, with the mods behind it,
//! as JSON lines for the relative-tier calibration.

use std::path::Path;

use item_parser::{ItemLanguage, detect_item_language, parse_clipboard};
use poe2_domain::StatCatalog;

fn catalog(dir: &Path, suffix: &str) -> StatCatalog {
    let text = std::fs::read_to_string(dir.join(format!("stat-catalog{suffix}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = Path::new(&args[1]);
    let (en, ru) = (catalog(cache, ""), catalog(cache, "-ru"));
    let mut texts = Vec::new();
    for source in &args[2..] {
        let path = Path::new(source);
        if path.is_dir() {
            let mut names: Vec<_> = std::fs::read_dir(path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "txt"))
                .collect();
            names.sort();
            for name in names {
                texts.push((
                    name.file_name().unwrap().to_string_lossy().into_owned(),
                    std::fs::read_to_string(&name).unwrap(),
                ));
            }
        } else {
            let all = std::fs::read_to_string(path).unwrap();
            let mut chunk = String::new();
            let mut n = 0;
            for line in all.lines().chain(std::iter::once("####")) {
                if line.starts_with("####") {
                    if !chunk.trim().is_empty() {
                        n += 1;
                        texts.push((
                            format!("{}#{n}", path.file_name().unwrap().to_string_lossy()),
                            std::mem::take(&mut chunk),
                        ));
                    }
                    chunk.clear();
                } else {
                    chunk.push_str(line);
                    chunk.push('\n');
                }
            }
        }
    }
    for (source, text) in texts {
        let Some(language) = detect_item_language(&text) else {
            continue;
        };
        let stats = match language {
            ItemLanguage::English => &en,
            ItemLanguage::Russian => &ru,
        };
        let Ok(item) = parse_clipboard(&text, language, stats) else {
            continue;
        };
        let filters = stat_filters::build_filters(&item, 10, stats);
        for filter in filters
            .iter()
            .filter(|f| f.generation.is_some() && f.tag != stat_filters::FilterTag::EmptyAffix)
        {
            let first = filter.trade_ids.first().map(String::as_str);
            let mods: Vec<_> = item
                .mods
                .iter()
                .filter(|m| {
                    m.info.generation == filter.generation
                        && m.stats.iter().any(|s| s.stat_id.as_deref() == first)
                })
                .map(|m| {
                    let mut hashes: Vec<_> = m
                        .stats
                        .iter()
                        .map(|s| {
                            s.stat_id.as_deref().map(|id| {
                                let hash = id.split_once('.').map_or(id, |(_, h)| h);
                                hash.split_once('|').map_or(hash, |(h, _)| h).to_owned()
                            })
                        })
                        .collect::<Option<Vec<_>>>()
                        .unwrap_or_default();
                    hashes.sort();
                    hashes.dedup();
                    serde_json::json!({
                        "tier": m.info.tier,
                        "key": hashes.join("+"),
                        "type": format!("{:?}", m.info.modifier_type),
                        "lines": m.stats.iter().map(|s| s.text.clone()).collect::<Vec<_>>(),
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "source": source,
                    "name": item.name,
                    "base": item.base_type,
                    "category": item.category.as_ref().map(|c| c.id.clone()),
                    "rarity": format!("{:?}", item.rarity),
                    "ilvl": item.item_level,
                    "text": filter.display_text,
                    "value": filter.roll.as_ref().map(|r| r.value),
                    "trade_id": first,
                    "generation": filter.generation,
                    "tier": filter.tier,
                    "enabled": filter.enabled,
                    "hidden": filter.hidden,
                    "mods": mods,
                })
            );
        }
    }
}
