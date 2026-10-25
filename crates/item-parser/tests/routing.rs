//! Every fixture through the app's whole pricing front half: `parse_clipboard`, then
//! `stat_filters::build_filters` and `trade_client::route_search`, the calls the Price Check
//! panel makes. The exchange catalogs here are slices of the live `/api/trade2/data/static`
//! (2026-09-22), one per site language: the fixtures' exchange-tradable names with their real
//! ids, plus kinds no EN fixture covers (an omen, a rune, a soul core, a catalyst, a fragment,
//! Verisium), met below through minimal nameplates built the way the client prints one. The
//! Russian names are the live RU client's own (`ru_live_*` fixtures); their ids are the English
//! entries' (ids are the same on every site), matched through EE2's `ru/items.ndjson` name pairs.

use item_parser::{IndexedCatalog, ItemLanguage, parse_clipboard};
use poe2_domain::{ItemRarity, ParsedItem, StatCatalog};
use stat_filters::SearchProfile;
use trade_client::catalog::StaticCurrency;
use trade_client::{RarityFilter, SearchRoute, route_search};

fn exchange(id: &str, name: &str) -> StaticCurrency {
    StaticCurrency {
        id: id.to_owned(),
        display_name: name.to_owned(),
        icon_url: None,
    }
}

fn exchange_catalog() -> Vec<StaticCurrency> {
    vec![
        exchange("uncut-skill-gem-19", "Uncut Skill Gem (Level 19)"),
        exchange("uncut-skill-gem-9", "Uncut Skill Gem (Level 9)"),
        exchange("uncut-spirit-gem-16", "Uncut Spirit Gem (Level 16)"),
        exchange("uncut-support-gem-5", "Uncut Support Gem (Level 5)"),
        exchange("uncut-support-gem-3", "Uncut Support Gem (Level 3)"),
        exchange("essence-of-enhancement", "Essence of Enhancement"),
        exchange("swift-alloy", "Swift Alloy"),
        exchange("omen-of-light", "Omen of Light"),
        exchange("greater-iron-rune", "Greater Iron Rune"),
        exchange("soul-core-of-tacati", "Soul Core of Tacati"),
        exchange("flesh-catalyst", "Flesh Catalyst"),
        exchange("kulemaks-invitation", "Kulemak's Invitation"),
        exchange("verisium", "Verisium"),
        exchange("exceptional-verisium", "Exceptional Verisium"),
    ]
}

fn ru_exchange_catalog() -> Vec<StaticCurrency> {
    vec![
        exchange("greater-rebirth-rune", "Большая руна перерождения"),
        exchange("greater-vision-rune", "Большая руна видения"),
        exchange("greater-iron-rune", "Большая руна железа"),
        exchange("greater-orb-of-transmutation", "Большая сфера превращения"),
        exchange("greater-orb-of-augmentation", "Большая сфера усиления"),
        exchange("greater-exalted-orb", "Большая сфера возвышения"),
        exchange("scrap", "Деталь доспеха"),
        exchange("lesser-jewellers-orb", "Малая сфера златокузнеца"),
        exchange(
            "uncut-spirit-gem-17",
            "Неогранённый камень духа (уровень 17)",
        ),
        exchange(
            "uncut-skill-gem-17",
            "Неогранённый камень умения (уровень 17)",
        ),
        exchange("regal-shard", "Осколок царей"),
        exchange("omen-of-the-blackblooded", "Предзнаменование чернокровных"),
        exchange(
            "omen-of-chaotic-quantity",
            "Предзнаменование хаотичного количества",
        ),
        exchange(
            "omen-of-abyssal-echoes",
            "Предзнаменование отголосков Бездны",
        ),
        exchange("omen-of-putrefaction", "Предзнаменование разложения"),
        exchange("omen-of-light", "Предзнаменование света"),
        exchange("alch", "Сфера алхимии"),
        exchange("artificers", "Сфера астромантии"),
        exchange("chaos", "Сфера хаоса"),
        exchange("transmute", "Сфера превращения"),
        exchange("regal", "Сфера царей"),
        exchange("aug", "Сфера усиления"),
        exchange("vaal", "Сфера ваал"),
        exchange("exalted", "Сфера возвышения"),
        exchange("preserved-jawbone", "Сохранившаяся челюсть"),
        exchange("preserved-rib", "Сохранившееся ребро"),
        exchange("wisdom", "Свиток мудрости"),
        exchange("whetstone", "Точильный камень"),
        exchange("an-audience-with-the-king", "Встреча с Хозяином"),
        exchange("waystone-13", "Путевой камень (Ур. 13)"),
    ]
}

/// The exchange catalog of the site a fixture's item is priced on.
fn exchange_catalog_for(fixture: &str) -> Vec<StaticCurrency> {
    if fixture.starts_with("ru_") {
        ru_exchange_catalog()
    } else {
        exchange_catalog()
    }
}

fn fixture_names() -> Vec<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("fixtures dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .into_string()
                .expect("utf-8 name")
        })
        .filter(|name| name.ends_with(".txt"))
        .collect();
    names.sort();
    names
}

fn parse_fixture(name: &str) -> ParsedItem {
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture text");
    let language = if name.starts_with("ru_") {
        ItemLanguage::Russian
    } else {
        ItemLanguage::English
    };
    parse_clipboard(&text, language, &IndexedCatalog::default())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The route an item should get, by the rules `route_search` documents: exchange names to the
/// market (logbooks and magic, rare and unique items excepted), gems to a filtered search by
/// type, other currency, fragments and cards to an exact type search, gear and everything else --
/// an unidentified unique too, by its base among unidentified uniques -- to a filtered search.
fn expected_kind(item: &ParsedItem, catalog: &[StaticCurrency]) -> &'static str {
    let category = item.category.as_ref().map(|c| c.id.as_str()).unwrap_or("");
    let tradable_as_commodity = category != "map.logbook"
        && !matches!(
            item.rarity,
            Some(ItemRarity::Magic | ItemRarity::Rare | ItemRarity::Unique)
        );
    if tradable_as_commodity && catalog.iter().any(|c| c.display_name == item.name) {
        "market"
    } else if category.starts_with("gem") {
        "filtered"
    } else if category.starts_with("currency") || category == "map.fragment" || category == "card" {
        "exact"
    } else {
        "filtered"
    }
}

#[test]
fn every_fixture_gets_filters_and_a_route() {
    let stats = StatCatalog::default();
    let mut routed = std::collections::BTreeMap::<&str, usize>::new();
    for name in fixture_names() {
        let catalog = exchange_catalog_for(&name);
        let item = parse_fixture(&name);
        // Must not panic, whatever the item.
        let filters = stat_filters::build_filters(
            &item,
            SearchProfile::default_for(&item),
            &stats,
            stat_filters::Session::SignedIn,
        );
        let route = route_search(&item, &catalog, &[]);
        let kind = match &route {
            SearchRoute::Market { trade_id } => {
                let entry = catalog
                    .iter()
                    .find(|c| &c.id == trade_id)
                    .expect("market id from the catalog");
                assert_eq!(entry.display_name, item.name, "{name}");
                "market"
            }
            SearchRoute::Exact { exact_type } => {
                assert_eq!(exact_type, &item.name, "{name}");
                "exact"
            }
            SearchRoute::Filtered { scope } => {
                assert!(
                    scope.name.is_some() || scope.base_type.is_some() || scope.category.is_some(),
                    "{name}: a filtered search scoped to nothing would list every item"
                );
                if item.rarity == Some(ItemRarity::Unique) && !item.is_unidentified {
                    assert_eq!(scope.name.as_deref(), Some(item.name.as_str()), "{name}");
                } else {
                    assert!(scope.name.is_none(), "{name}");
                }
                assert!(
                    filters.iter().all(|f| !f.trade_ids.is_empty()),
                    "{name}: every filter row names what it searches"
                );
                "filtered"
            }
        };
        assert_eq!(kind, expected_kind(&item, &catalog), "{name}");
        *routed.entry(kind).or_default() += 1;
    }
    // All three routes come up: the live RU Inscribed Ultimatum is priced by its type.
    assert!(routed.len() == 3, "{routed:?}");
}

/// How a live RU item must be priced.
enum Want {
    Market(&'static str),
    Exact,
    /// A filtered search of the trade category, with the rarity filter.
    Category(&'static str, Option<RarityFilter>),
    /// A filtered search of the item's own base type (its name, for a Normal item).
    BaseType(RarityFilter),
}

#[test]
fn ru_live_items_get_their_routes() {
    use Want::*;
    let catalog = ru_exchange_catalog();
    let wanted = [
        // A magic waystone's value is in its mods and tier, never the plain exchange tier; it is
        // compared with magic waystones, as every magic item is.
        (
            "ru_live_adskiy_putevoy_kamen_ur_13_ukloneniya.txt",
            Category("map.waystone", Some(RarityFilter::Magic)),
        ),
        (
            "ru_live_bespamyatnoe_chistilische.txt",
            Category("weapon.crossbow", Some(RarityFilter::Rare)),
        ),
        (
            "ru_live_bolshaya_runa_pererozhdeniya.txt",
            Market("greater-rebirth-rune"),
        ),
        (
            "ru_live_bolshaya_runa_videniya.txt",
            Market("greater-vision-rune"),
        ),
        (
            "ru_live_bolshaya_runa_zheleza.txt",
            Market("greater-iron-rune"),
        ),
        (
            "ru_live_bolshaya_sfera_prevrascheniya.txt",
            Market("greater-orb-of-transmutation"),
        ),
        (
            "ru_live_bolshaya_sfera_usileniya.txt",
            Market("greater-orb-of-augmentation"),
        ),
        (
            "ru_live_bolshaya_sfera_vozvysheniya.txt",
            Market("greater-exalted-orb"),
        ),
        ("ru_live_detal_dospeha.txt", Market("scrap")),
        (
            "ru_live_entropicheskaya_kayma.txt",
            Category("accessory.ring", Some(RarityFilter::Rare)),
        ),
        // A magic item is compared with magic ones: a jewel as in EE2 (`forAdornedJewel`), gear
        // too, since the player asked for it (a magic wand's buyer wants a crafting base).
        (
            "ru_live_glificheskiy_uvyadshiy_zhezl_katastrofy.txt",
            Category("weapon.wand", Some(RarityFilter::Magic)),
        ),
        (
            "ru_live_kopenosnyy_izumrud_prigvozhdeniya.txt",
            Category("jewel", Some(RarityFilter::Magic)),
        ),
        (
            "ru_live_krutyaschiy_obodok.txt",
            Category("accessory.ring", Some(RarityFilter::Rare)),
        ),
        (
            "ru_live_malaya_sfera_zlatokuznetsa.txt",
            Market("lesser-jewellers-orb"),
        ),
        ("ru_live_nachertannyy_ultimatum.txt", Exact),
        (
            "ru_live_neogranennyy_kamen_duha_uroven_17.txt",
            Market("uncut-spirit-gem-17"),
        ),
        (
            "ru_live_neogranennyy_kamen_umeniya_uroven_17.txt",
            Market("uncut-skill-gem-17"),
        ),
        ("ru_live_oskolok_tsarey.txt", Market("regal-shard")),
        ("ru_live_plitka_bezdny.txt", BaseType(RarityFilter::Normal)),
        (
            "ru_live_predznamenovanie_chernokrovnyh.txt",
            Market("omen-of-the-blackblooded"),
        ),
        (
            "ru_live_predznamenovanie_haotichnogo_kolichestva.txt",
            Market("omen-of-chaotic-quantity"),
        ),
        (
            "ru_live_predznamenovanie_otgoloskov_bezdny.txt",
            Market("omen-of-abyssal-echoes"),
        ),
        (
            "ru_live_predznamenovanie_razlozheniya.txt",
            Market("omen-of-putrefaction"),
        ),
        (
            "ru_live_predznamenovanie_sveta.txt",
            Market("omen-of-light"),
        ),
        ("ru_live_sfera_alhimii.txt", Market("alch")),
        ("ru_live_sfera_astromantii.txt", Market("artificers")),
        ("ru_live_sfera_haosa.txt", Market("chaos")),
        ("ru_live_sfera_prevrascheniya.txt", Market("transmute")),
        ("ru_live_sfera_tsarey.txt", Market("regal")),
        ("ru_live_sfera_usileniya.txt", Market("aug")),
        ("ru_live_sfera_vaal.txt", Market("vaal")),
        ("ru_live_sfera_vozvysheniya.txt", Market("exalted")),
        (
            "ru_live_sohranivshayasya_chelyust.txt",
            Market("preserved-jawbone"),
        ),
        ("ru_live_sohranivsheesya_rebro.txt", Market("preserved-rib")),
        ("ru_live_svitok_mudrosti.txt", Market("wisdom")),
        ("ru_live_tochilnyy_kamen.txt", Market("whetstone")),
        (
            "ru_live_tyazhelyy_remen.txt",
            BaseType(RarityFilter::Normal),
        ),
        // A quest-class item: no trade category, priced by its exchange name.
        (
            "ru_live_vstrecha_s_hozyainom.txt",
            Market("an-audience-with-the-king"),
        ),
        (
            "ru_live_zdorovye_ponozhi_vaal_trollya.txt",
            Category("armour.boots", Some(RarityFilter::Magic)),
        ),
        (
            "ru_live_zhemchuzhnoe_koltso.txt",
            BaseType(RarityFilter::Normal),
        ),
    ];
    let live = fixture_names()
        .into_iter()
        .filter(|name| name.starts_with("ru_live_"))
        .count();
    assert_eq!(
        live,
        wanted.len(),
        "every ru_live_ fixture has a wanted route"
    );
    for (fixture, want) in wanted {
        let item = parse_fixture(fixture);
        let route = route_search(&item, &catalog, &[]);
        match (want, route) {
            (Market(id), SearchRoute::Market { trade_id }) => assert_eq!(trade_id, id, "{fixture}"),
            (Exact, SearchRoute::Exact { exact_type }) => {
                assert_eq!(exact_type, item.name, "{fixture}")
            }
            (Category(category, rarity), SearchRoute::Filtered { scope }) => {
                assert_eq!(scope.category.as_deref(), Some(category), "{fixture}");
                assert_eq!(scope.base_type, None, "{fixture}");
                assert_eq!(scope.rarity, rarity, "{fixture}");
            }
            (BaseType(rarity), SearchRoute::Filtered { scope }) => {
                assert_eq!(
                    scope.base_type.as_deref(),
                    Some(item.name.as_str()),
                    "{fixture}"
                );
                assert_eq!(scope.category, None, "{fixture}");
                assert_eq!(scope.rarity, Some(rarity), "{fixture}");
            }
            _ => panic!("{fixture}: routed another way than wanted"),
        }
    }
}

#[test]
fn exchange_kinds_without_a_fixture_route_to_the_market() {
    let catalog = exchange_catalog();
    for (class, name, id) in [
        ("Omen", "Omen of Light", "omen-of-light"),
        ("Augment", "Greater Iron Rune", "greater-iron-rune"),
        ("Augment", "Soul Core of Tacati", "soul-core-of-tacati"),
        ("Stackable Currency", "Flesh Catalyst", "flesh-catalyst"),
        (
            "Map Fragments",
            "Kulemak's Invitation",
            "kulemaks-invitation",
        ),
        // Named like a quality Verisium, but an exchange item of its own.
        (
            "Stackable Currency",
            "Exceptional Verisium",
            "exceptional-verisium",
        ),
    ] {
        let text =
            format!("Item Class: {class}\nRarity: Currency\n{name}\n--------\nStack Size: 1/10\n");
        let item = parse_clipboard(&text, ItemLanguage::English, &IndexedCatalog::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        match route_search(&item, &catalog, &[]) {
            SearchRoute::Market { trade_id } => assert_eq!(trade_id, id),
            _ => panic!("{name}: expected the market route"),
        }
    }
}

#[test]
fn currency_missing_from_the_exchange_is_searched_by_its_type() {
    // Live 2026-09-22 (trade-client's own note): an Inscribed Ultimatum is `Rarity: Currency` and
    // no exchange entry.
    let text = "Item Class: Inscribed Ultimatum\nRarity: Currency\nInscribed Ultimatum\n--------\nArea Level: 75\nNumber of Trials: 4\nVictorious\n";
    let item =
        parse_clipboard(text, ItemLanguage::English, &IndexedCatalog::default()).expect("parses");
    assert_eq!(item.area_level, Some(75));
    match route_search(&item, &exchange_catalog(), &[]) {
        SearchRoute::Exact { exact_type } => assert_eq!(exact_type, "Inscribed Ultimatum"),
        _ => panic!("expected an exact type search"),
    }
}

/// A fixture's rows, as the panel first shows them: with the item's default search profile.
fn built(name: &str, stats: &StatCatalog) -> Vec<stat_filters::SearchFilter> {
    let item = parse_fixture(name);
    stat_filters::build_filters(
        &item,
        SearchProfile::default_for(&item),
        stats,
        stat_filters::Session::SignedIn,
    )
}

fn row<'a>(filters: &'a [stat_filters::SearchFilter], id: &str) -> &'a stat_filters::SearchFilter {
    filters
        .iter()
        .find(|filter| filter.trade_ids.first().is_some_and(|first| first == id))
        .unwrap_or_else(|| panic!("no {id} row"))
}

#[test]
fn gems_waystones_and_tablets_carry_ee2s_property_rows() {
    let stats = StatCatalog::default();

    // Herald of Ice: level 18, 20% quality, 4 sockets -- EE2 searches quality from 16 and
    // sockets from 3, level only from 19.
    let gem = built("sidekick_herald_of_ice_en.txt", &stats);
    let level = row(&gem, "misc_filters.gem_level");
    assert_eq!(level.roll.as_ref().and_then(|r| r.min), Some(18.0));
    assert!(!level.enabled);
    assert!(row(&gem, "type_filters.quality").enabled);
    assert!(row(&gem, "misc_filters.gem_sockets").enabled);

    // A tier-16 rare waystone searches its tier exactly and its properties as minimums. It
    // prints both Magic Monsters and Monster Effectiveness, which the trade site files under one
    // filter labelled Monster Effectiveness: that filter's one row is the effectiveness line.
    let waystone = built("rare_map_all_props_en.txt", &stats);
    let tier = row(&waystone, "map_filters.map_tier");
    let roll = tier.roll.as_ref().expect("tier roll");
    assert_eq!((roll.min, roll.max), (Some(16.0), Some(16.0)));
    assert!(tier.enabled);
    let pack_size = row(&waystone, "map_filters.map_packsize");
    assert_eq!(pack_size.roll.as_ref().and_then(|r| r.min), Some(20.0));
    assert!(pack_size.enabled);
    let magic: Vec<_> = waystone
        .iter()
        .filter(|filter| filter.trade_ids == ["map_filters.map_magic_monsters"])
        .map(|filter| {
            (
                filter.display_text.as_str(),
                filter.roll.as_ref().map(|r| r.value),
            )
        })
        .collect();
    assert_eq!(magic, [("Monster Effectiveness: #%", Some(45.0))]);

    // The live RU magic waystone names its tier only in its name; the tier row is its search.
    let ru_waystone = built("ru_live_adskiy_putevoy_kamen_ur_13_ukloneniya.txt", &stats);
    let tier = row(&ru_waystone, "map_filters.map_tier");
    let roll = tier.roll.as_ref().expect("tier roll");
    assert_eq!((roll.min, roll.max), (Some(13.0), Some(13.0)));
    assert!(tier.enabled);

    // A rare tablet: at least its 10 uses, every mod at exactly its roll, the implicit left out.
    // Ids and texts as the live EN catalog has them (2026-09-22).
    let catalog = IndexedCatalog::new(StatCatalog {
        stats: vec![
            poe2_domain::TradeStat {
                id: "implicit.stat_4041853756".to_owned(),
                text: "Adds Irradiated to a Map \n# use remaining".to_owned(),
                mod_type: "implicit".to_owned(),
            },
            poe2_domain::TradeStat {
                id: "explicit.stat_2306002879".to_owned(),
                text: "#% increased Rarity of Items found in Map".to_owned(),
                mod_type: "explicit".to_owned(),
            },
        ],
    });
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/sidekick_irradiated_tablet_en.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture text");
    let item = parse_clipboard(&text, ItemLanguage::English, &catalog).expect("parses");
    let tablet = stat_filters::build_filters(
        &item,
        SearchProfile::default_for(&item),
        &catalog,
        stat_filters::Session::SignedIn,
    );
    let uses = row(&tablet, "pseudo.pseudo_number_of_uses_remaining");
    assert_eq!(uses.roll.as_ref().and_then(|r| r.min), Some(10.0));
    assert!(uses.enabled);
    let implicit = row(&tablet, "implicit.stat_4041853756");
    assert!(implicit.hidden && !implicit.enabled);
    let rarity = row(&tablet, "explicit.stat_2306002879");
    assert!(rarity.enabled);
    assert_eq!(rarity.roll.as_ref().and_then(|r| r.min), Some(9.0));
}

#[test]
fn reported_ring_and_spear_get_their_quality_dps_and_crit_rows() {
    // Two reports from the app, 2026-10-06 (0.1.7, RU client): `ru_report_*.txt`, the item texts
    // they carried.
    let stats = StatCatalog::default();

    // A Breach ring with 62% attack-modifier quality from a catalyst: its typed quality is read
    // and offered as an unchecked minimum.
    let ring_item = parse_fixture("ru_report_yarostnyy_zavitok.txt");
    assert_eq!(ring_item.quality, Some(62));
    assert_eq!(ring_item.quality_type.as_deref(), Some("свойства атак"));
    let ring = built("ru_report_yarostnyy_zavitok.txt", &stats);
    let quality = row(&ring, "type_filters.quality");
    assert_eq!(quality.roll.as_ref().map(|r| r.value), Some(62.0));
    assert!(!quality.enabled && !quality.hidden);

    // A unique lightning spear at 12.52% crit: its physical DPS, 66% of the total, is listed, and
    // crit keeps both decimals in its minimum.
    let spear = built("ru_report_rukopolozhennye.txt", &stats);
    assert!(!row(&spear, "equipment_filters.pdps").hidden);
    let crit = row(&spear, "equipment_filters.crit")
        .roll
        .as_ref()
        .expect("crit roll");
    assert_eq!((crit.value, crit.min), (12.52, Some(12.52)));
}

#[test]
fn a_reported_sceptre_reads_its_own_item_level_and_is_searched_from_the_useful_maximum() {
    // The owner, 2026-10-11 (0.1.8, RU client): an item level 83 sceptre's row read
    // «Уровень предмета: 82». No sceptre affix needs more than item level 82 (poe2db, 2026-10-11),
    // so the search keeps starting there, while the row reads the item's own level.
    let stats = StatCatalog::default();
    let sceptre = built("ru_report_obraztsovyy_skipetr_stoika.txt", &stats);
    let level = row(&sceptre, "type_filters.ilvl")
        .roll
        .as_ref()
        .expect("item level roll");
    assert_eq!((level.value, level.min), (83.0, Some(82.0)));
}

/// A catalog of `(id, text)` entries, each filed under its id's own group.
fn stat_catalog(entries: &[(&str, &str)]) -> IndexedCatalog {
    IndexedCatalog::new(StatCatalog {
        stats: entries
            .iter()
            .map(|&(id, text)| poe2_domain::TradeStat {
                id: id.to_owned(),
                text: text.to_owned(),
                mod_type: id.split_once('.').map_or(id, |(kind, _)| kind).to_owned(),
            })
            .collect(),
    })
}

/// A report fixture parsed against `catalog` and its rows under `profile`.
fn reported(
    name: &str,
    catalog: &IndexedCatalog,
    profile: Option<SearchProfile>,
) -> (ParsedItem, Vec<stat_filters::SearchFilter>) {
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture text");
    let language = if name.starts_with("ru_") {
        ItemLanguage::Russian
    } else {
        ItemLanguage::English
    };
    let item = parse_clipboard(&text, language, catalog).unwrap_or_else(|e| panic!("{name}: {e}"));
    let profile = profile.unwrap_or_else(|| SearchProfile::default_for(&item));
    let filters =
        stat_filters::build_filters(&item, profile, catalog, stat_filters::Session::SignedIn);
    (item, filters)
}

/// The mod row of the stat `hash` (`stat_…`), whatever its group: not a pseudo total adding it in.
fn hash_row<'a>(
    filters: &'a [stat_filters::SearchFilter],
    hash: &str,
) -> &'a stat_filters::SearchFilter {
    filters
        .iter()
        .find(|filter| {
            filter.tag != stat_filters::FilterTag::Pseudo
                && filter.trade_ids.iter().any(|id| id.ends_with(hash))
        })
        .unwrap_or_else(|| panic!("no {hash} row"))
}

#[test]
fn a_waystone_is_searched_by_its_properties_and_how_many_modifiers_it_has() {
    // Report #16, 2026-10-20 (0.1.19, EN client): a corrupted tier-15 rare waystone, searched by
    // its tier alone among more than 10,000 listings; the player asked for its properties and
    // its count of modifiers. With them the live search found 28 (2026-10-21).
    let stats = StatCatalog::default();
    let item = parse_fixture("report_torture_control_en.txt");
    let filters = built("report_torture_control_en.txt", &stats);
    let minimum = |id: &str| {
        let filter = row(&filters, id);
        (
            filter.enabled,
            filter.roll.as_ref().and_then(|roll| roll.min),
        )
    };
    assert_eq!(minimum("map_filters.map_packsize"), (true, Some(19.0)));
    assert_eq!(minimum("map_filters.map_iir"), (true, Some(67.0)));
    assert_eq!(
        minimum("map_filters.map_magic_monsters"),
        (true, Some(13.0))
    );
    assert_eq!(minimum("map_filters.map_bonus"), (true, Some(140.0)));
    // Four prefixes and four suffixes, the two-line one counted once.
    assert_eq!(
        minimum("pseudo.pseudo_number_of_affix_mods"),
        (true, Some(8.0))
    );

    let SearchRoute::Filtered { scope } = route_search(&item, &exchange_catalog(), &[]) else {
        panic!("a waystone is searched by its filters");
    };
    let body =
        trade_client::filtered_search_request(&scope, &filters, trade_client::ListingStatus::Any);
    assert_eq!(
        body["query"]["filters"]["map_filters"]["filters"],
        serde_json::json!({
            "map_tier": { "min": 15.0, "max": 15.0 },
            "map_packsize": { "min": 19.0 },
            "map_iir": { "min": 67.0 },
            "map_magic_monsters": { "min": 13.0 },
            "map_bonus": { "min": 140.0 },
        })
    );
    let count = body["query"]["stats"][0]["filters"]
        .as_array()
        .expect("the and group")
        .iter()
        .find(|entry| entry["id"] == "pseudo.pseudo_number_of_affix_mods")
        .expect("the count of modifiers");
    assert_eq!(count["value"], serde_json::json!({ "min": 8.0 }));
    assert_eq!(count["disabled"], false);
}

#[test]
fn a_mageblood_is_searched_with_its_own_legacies_and_none_other() {
    // Report #15, 2026-10-18 (0.1.18, EN client): a Mageblood with Legacy of Quicksilver three
    // times and Legacy of Gold, searched among belts with those two among any others, and the
    // player asked for the others to be left out. The trade site counts a legacy's copies; the
    // fourteen legacies are the options of one stat in the live EN catalog.
    let legacies: Vec<(String, String)> = [
        "Amethyst",
        "Basalt",
        "Bismuth",
        "Diamond",
        "Gold",
        "Granite",
        "Jade",
        "Quicksilver",
        "Ruby",
        "Sapphire",
        "Silver",
        "Stibnite",
        "Sulphur",
        "Topaz",
    ]
    .iter()
    .enumerate()
    .map(|(index, name)| {
        (
            format!("explicit.stat_264262054|{}", index + 1),
            format!("Legacy of {name}"),
        )
    })
    .collect();
    let mut entries: Vec<(&str, &str)> = legacies
        .iter()
        .map(|(id, text)| (id.as_str(), text.as_str()))
        .collect();
    entries.extend([
        (
            "implicit.stat_462041840",
            "#% of Flask Recovery applied Instantly",
        ),
        ("implicit.stat_1416292992", "Has # Charm Slot"),
        (
            "explicit.stat_3874491706",
            "All Mage's Legacies have #% increased effect per duplicate Mage's Legacy you have",
        ),
    ]);
    let catalog = stat_catalog(&entries);
    let (item, filters) = reported("report_mageblood_duplicates_en.txt", &catalog, None);

    // Each legacy row searches the belt's copies of it at least.
    let copies = |hash: &str| {
        let row = hash_row(&filters, hash);
        let roll = row.roll.as_ref().map(|roll| (roll.value, roll.min));
        (row.enabled, row.display_text.as_str(), roll)
    };
    assert_eq!(
        copies("stat_264262054|8"),
        (true, "Legacy of Quicksilver ×#", Some((3.0, Some(3.0))))
    );
    assert_eq!(
        copies("stat_264262054|5"),
        (true, "Legacy of Gold ×#", Some((1.0, Some(1.0))))
    );
    // Right after them, checked, the row leaving out the twelve legacies the belt lacks.
    let gold = filters
        .iter()
        .position(|row| row.trade_ids == ["explicit.stat_264262054|5"])
        .expect("the Gold row");
    let others = &filters[gold + 1];
    assert!(others.none_of && others.enabled);
    let lacked: Vec<&str> = legacies
        .iter()
        .map(|(id, _)| id.as_str())
        .filter(|id| !id.ends_with("|5") && !id.ends_with("|8"))
        .collect();
    assert_eq!(others.trade_ids, lacked);

    // The search: the copies as minimums, the lacked legacies in a "not" group.
    let SearchRoute::Filtered { scope } = route_search(&item, &exchange_catalog(), &[]) else {
        panic!("a Mageblood is searched by its filters");
    };
    let body =
        trade_client::filtered_search_request(&scope, &filters, trade_client::ListingStatus::Any);
    let groups = body["query"]["stats"].as_array().expect("stat groups");
    let every = groups[0]["filters"].as_array().expect("the and group");
    for (id, min) in [
        ("explicit.stat_264262054|8", 3.0),
        ("explicit.stat_264262054|5", 1.0),
    ] {
        let entry = every.iter().find(|entry| entry["id"] == id).expect(id);
        assert_eq!(entry["value"], serde_json::json!({ "min": min }));
    }
    let not = groups
        .iter()
        .find(|group| group["type"] == "not")
        .expect("a not group");
    let not_ids: Vec<&str> = not["filters"]
        .as_array()
        .expect("its filters")
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect();
    assert_eq!(not_ids, lacked);
}

#[test]
fn a_raised_modifier_is_searched_at_the_value_the_item_has() {
    // Report #13, 2026-10-17 (0.1.16, EN client): a jewel's crafted 53% increased Effect of
    // Suffixes raises its suffixes, which the copy prints as rolled (`19(10-20)%` under `— 53%
    // Increased`) and the trade site lists raised (29%). The search went from 19, among jewels
    // whose suffixes are that weak, and the 53% row had no number. Ids and texts as the live
    // catalog has them.
    let en = stat_catalog(&[
        ("explicit.stat_3141070085", "#% increased Elemental Damage"),
        (
            "explicit.stat_2475221757",
            "#% increased Effect of Suffixes",
        ),
        ("crafted.stat_2475221757", "#% increased Effect of Suffixes"),
        (
            "explicit.stat_274716455",
            "#% increased Critical Spell Damage Bonus",
        ),
        (
            "explicit.stat_3556824919",
            "#% increased Critical Damage Bonus",
        ),
        (
            "explicit.stat_737908626",
            "#% increased Critical Hit Chance for Spells",
        ),
        (
            "desecrated.stat_737908626",
            "#% increased Critical Hit Chance for Spells",
        ),
    ]);
    let (item, jewel) = reported("report_vivid_lens_en.txt", &en, None);
    let value = |hash| hash_row(&jewel, hash).roll.as_ref().map(|roll| roll.value);
    // 19, 20 and 15 raised by 53% and truncated, as the site lists them: 29.07, 30.6, 22.95.
    assert_eq!(value("stat_274716455"), Some(29.0));
    assert_eq!(value("stat_3556824919"), Some(30.0));
    assert_eq!(value("stat_737908626"), Some(22.0));
    // The prefix is not raised, and the 53% is searched with its number.
    assert_eq!(value("stat_3141070085"), Some(5.0));
    assert_eq!(value("stat_2475221757"), Some(53.0));
    // The suffix's tier still goes by its printed roll; its rolls, for the slider and the tier
    // minimum, are raised as the row's is: T1 rolls 10-20 on jewels, all tiers 10-20, 15-30 at
    // 53%.
    let spell = hash_row(&jewel, "stat_274716455");
    assert_eq!(spell.tier, Some(1));
    let tier = spell.tier_info.expect("tier info");
    assert_eq!(
        (tier.current, tier.tier_floor, tier.range),
        (1, Some(15.0), Some((15.0, 30.0)))
    );
    let suffix = item
        .mods
        .iter()
        .find(|modifier| modifier.info.name.as_deref() == Some("of Unmaking"))
        .expect("the suffix");
    assert_eq!(suffix.info.increased, Some(53));
    assert_eq!(suffix.info.tags, ["Damage", "Caster", "Critical"]);
    assert_eq!(suffix.stats[0].value, 19.0);

    // A ring's 62% Attack quality from a catalyst (RU client, 2026-10-06), `— 62% увеличение`:
    // its fractured added fire damage `29(25-29)` to `43(37-45)` is listed 46 to 69, each number
    // raised and truncated, averaging 57.5 -- not 36 raised, 58, which would leave the very same
    // ring out.
    let fire = "Добавляет от # до # урона от огня к атакам";
    let ru = stat_catalog(&[
        ("explicit.stat_1573130764", fire),
        ("fractured.stat_1573130764", fire),
    ]);
    let (_, ring) = reported("ru_report_yarostnyy_zavitok.txt", &ru, None);
    let fire = hash_row(&ring, "stat_1573130764").roll.as_ref();
    assert_eq!(fire.map(|roll| roll.value), Some(57.5));
}

#[test]
fn reported_desecrated_and_fractured_rows_search_the_stat_of_any_kind() {
    // Reports #7 (0.1.7, EN client) and #5 (0.1.8, RU client), 2026-10-11: a desecrated mana
    // row found only desecrated mana, and a fractured row only fractured mods. The trade site's
    // explicit filter finds all kinds, so the rows search that; the Crafting Base profile keeps a
    // fractured row fractured. Ids and texts as the live catalogs have them.
    let en = stat_catalog(&[
        ("explicit.stat_1050105434", "# to maximum Mana"),
        ("desecrated.stat_1050105434", "# to maximum Mana"),
    ]);
    let (_, boots) = reported("report_invasion_urge_en.txt", &en, None);
    let mana = hash_row(&boots, "stat_1050105434");
    assert_eq!(mana.tag, stat_filters::FilterTag::Desecrated);
    assert_eq!(mana.trade_ids, ["explicit.stat_1050105434"]);

    let ru = stat_catalog(&[
        (
            "explicit.stat_4067062424",
            "Добавляет от # до # урона от холода к атакам",
        ),
        (
            "fractured.stat_4067062424",
            "Добавляет от # до # урона от холода к атакам",
        ),
    ]);
    let (_, ring) = reported("ru_report_vetrenye_uzy.txt", &ru, None);
    let cold = hash_row(&ring, "stat_4067062424");
    assert_eq!(cold.tag, stat_filters::FilterTag::Fractured);
    assert_eq!(cold.trade_ids, ["explicit.stat_4067062424"]);
    let (_, base) = reported(
        "ru_report_vetrenye_uzy.txt",
        &ru,
        Some(SearchProfile::CraftingBase),
    );
    assert_eq!(
        hash_row(&base, "stat_4067062424").trade_ids,
        ["fractured.stat_4067062424"]
    );
}

#[test]
fn a_reported_corrupted_uniques_moved_fixed_value_is_read() {
    // Report #6, 2026-10-11 (0.1.7, EN client): Adonia's Ego's «+2(3) to Level of all Spell
    // Skills», a fixed 3 its corruption moved to 2, had no row.
    let catalog = stat_catalog(&[
        ("explicit.stat_124131830", "# to Level of all Spell Skills"),
        (
            "explicit.stat_2593644209",
            "#% to all Elemental Resistances per Power Charge",
        ),
    ]);
    let (item, wand) = reported("report_adonias_ego_en.txt", &catalog, None);
    // The catalog here has only these two stats: the wand's other lines stay unread.
    assert!(
        item.unknown_mods
            .iter()
            .all(|(text, _)| !text.contains("Level of all Spell Skills")
                && !text.contains("per Power Charge")),
        "unread: {:?}",
        item.unknown_mods
    );
    let levels = hash_row(&wand, "stat_124131830")
        .roll
        .as_ref()
        .expect("levels roll");
    assert_eq!((levels.value, levels.min), (2.0, Some(2.0)));
    let resistances = hash_row(&wand, "stat_2593644209")
        .roll
        .as_ref()
        .expect("resistances roll");
    assert_eq!(resistances.value, -9.0);
}

#[test]
fn a_number_baked_into_a_stats_text_is_not_its_roll() {
    // Report #19, 2026-10-25 (0.2.0, EN client): Prism Guardian's «1% increased Spirit
    // Reservation Efficiency of Buff Skills per 100 Maximum Life» rolled at 50.5, the mean of its
    // 1 and the 100 the stat's own text has, and its row searched for at least 50.5, which no
    // shield has (live, 2026-10-25: none at 50.5, 2482 at 1). Only a number the catalog's text
    // has a `#` for is the roll, as in EE2. The other two items are live listings item-synth
    // wrote, Goregirdle's fixed lines printed as the report prints its own, without a range. Ids
    // and texts as the live catalog has them.
    let catalog = stat_catalog(&[
        (
            "explicit.stat_3581035970",
            "#% increased Spirit Reservation Efficiency of Buff Skills per 100 Maximum Life",
        ),
        (
            "explicit.stat_4186798932",
            "# to # Added Attack Fire Damage per 25 Strength",
        ),
        (
            "explicit.stat_1867725690",
            "Hits with this Weapon have #% chance to Trigger Molten Shower per 25 Strength",
        ),
        ("explicit.stat_3387008487", "Defend with 200% of Armour"),
        (
            "explicit.stat_3960211755",
            "Maximum Physical Damage Reduction is 50%",
        ),
    ]);
    let rolled = |filters: &[stat_filters::SearchFilter], hash: &str| {
        hash_row(filters, hash)
            .roll
            .as_ref()
            .map(|roll| (roll.value, roll.min))
    };
    let (_, shield) = reported("report_prism_guardian_en.txt", &catalog, None);
    assert_eq!(rolled(&shield, "stat_3581035970"), Some((1.0, Some(1.0))));
    // Two numbers with a `#` each beside a baked one roll at their mean: 5 to 10 at 7.5, not at
    // the first number alone, nor at the mean of all three.
    let (_, mace) = reported("brutus_lead_sprinkler_en.txt", &catalog, None);
    assert_eq!(rolled(&mace, "stat_4186798932"), Some((7.5, Some(7.5))));
    assert_eq!(rolled(&mace, "stat_1867725690"), Some((5.0, Some(5.0))));
    // A text without a `#` is a flag, searched as there: at least 200, a number the site never
    // stores for it (its listings carry 1), found no belt at all.
    let (_, belt) = reported("goregirdle_en.txt", &catalog, None);
    assert_eq!(rolled(&belt, "stat_3387008487"), None);
    assert_eq!(rolled(&belt, "stat_3960211755"), None);
}

#[test]
fn a_reported_unique_goes_among_uniques_cultivated_as_it_is() {
    // Report #8, 2026-10-11 (0.1.7, EN client): The Adorned at 101% was priced at 20 divine among
    // Vaal-cultivated ones («mutated» on the trade site), while those without start at hundreds.
    let (adorned, _) = reported(
        "report_the_adorned_en.txt",
        &IndexedCatalog::default(),
        None,
    );
    assert_eq!(adorned.mutated, Some(false));
    let mutated_text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/report_the_adorned_en.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture text")
    .replace("{ Unique Modifier }", "{ Vaal Unique Modifier }");
    let mutated = parse_clipboard(
        &mutated_text,
        ItemLanguage::English,
        &IndexedCatalog::default(),
    )
    .expect("parses");
    assert_eq!(mutated.mutated, Some(true));
    for (item, expected) in [(&adorned, Some(false)), (&mutated, Some(true))] {
        match route_search(item, &exchange_catalog(), &[]) {
            SearchRoute::Filtered { scope } => assert_eq!(scope.misc.mutated, expected),
            _ => panic!("expected a filtered search"),
        }
    }
}

/// The live catalog's stats the trade site files by kind of item, as in the reported charm.
fn kind_forms_catalog() -> IndexedCatalog {
    stat_catalog(&[
        (
            "implicit.stat_3310778564",
            "Used when you take Chaos damage from a Hit",
        ),
        (
            "explicit.stat_2678930256",
            "#% of Chaos damage you prevent when Hit Recouped as Life and Mana during effect",
        ),
        ("explicit.stat_1256719186", "#% increased Duration (Flask)"),
        ("explicit.stat_2541588185", "#% increased Duration (Charm)"),
        (
            "explicit.stat_1030153674",
            "Recover #% of maximum Mana on Kill",
        ),
        (
            "explicit.stat_1604736568",
            "Recover #% of maximum Mana on Kill (Jewel)",
        ),
    ])
}

#[test]
fn a_reported_charm_is_searched_by_the_charms_duration() {
    // Report #9, 2026-10-11 (0.1.10, EN client): Forsaken Bangle's «19(15-25)% increased
    // Duration» was searched as the flask's duration, which no charm listing carries, so the
    // unique's exact search found nothing (the trade site: 0, and 1013 by the charm's stat).
    let (bangle, filters) = reported("report_forsaken_bangle_en.txt", &kind_forms_catalog(), None);
    assert!(bangle.unknown_mods.is_empty(), "{:?}", bangle.unknown_mods);
    let duration = hash_row(&filters, "stat_2541588185");
    assert_eq!(duration.trade_ids, ["explicit.stat_2541588185"]);
    assert!(duration.enabled);
    assert!(
        !filters.iter().any(|filter| filter
            .trade_ids
            .iter()
            .any(|id| id.ends_with("stat_1256719186"))),
        "a charm row searches the flask's duration"
    );
}

#[test]
fn a_stat_filed_by_kind_of_item_takes_the_items_kind() {
    let catalog = kind_forms_catalog();
    let stat_of = |text: &str, line: &str| {
        let item = parse_clipboard(text, ItemLanguage::English, &catalog).expect("parses");
        item.mods
            .iter()
            .flat_map(|modifier| &modifier.stats)
            .find(|stat| stat.text.starts_with(line))
            .and_then(|stat| stat.stat_id.clone())
            .unwrap_or_else(|| panic!("no {line:?} stat"))
    };
    // Items as the app copies them, with their modifiers' headers.
    let flask = "Item Class: Life Flasks\nRarity: Magic\nUltimate Life Flask of the Sustainer\n\
        --------\nRecovers 920 Life over 3 Seconds\nConsumes 10 of 75 Charges on use\n\
        Currently has 0 Charges\n--------\nRequires: Level 60\n--------\nItem Level: 66\n\
        --------\n{ Suffix Modifier \"of the Sustainer\" (Tier: 1) }\n\
        25(21-25)% increased Duration\n--------\nRight click to drink. Can only hold charges \
        while in belt. Refill at Wells or by killing monsters.\n";
    assert_eq!(
        stat_of(flask, "#% increased Duration"),
        "explicit.stat_1256719186"
    );
    let jewel = "Item Class: Jewels\nRarity: Rare\nGrim Glimmer\nEmerald\n--------\n\
        Item Level: 80\n--------\n{ Suffix Modifier \"of Absorption\" (Tier: 1) — Mana }\n\
        Recover 2(1-2)% of maximum Mana on Kill\n--------\nPlace into an allocated Jewel \
        Socket on the Passive Skill Tree. Right click to remove from the Socket.\n";
    assert_eq!(
        stat_of(jewel, "Recover #% of maximum Mana on Kill"),
        "explicit.stat_1604736568"
    );
    let ring = "Item Class: Rings\nRarity: Rare\nDoom Loop\nAmethyst Ring\n--------\n\
        Requires: Level 40\n--------\nItem Level: 80\n--------\n\
        { Suffix Modifier \"of Absorption\" (Tier: 1) — Mana }\n\
        Recover 2(1-2)% of maximum Mana on Kill\n";
    assert_eq!(
        stat_of(ring, "Recover #% of maximum Mana on Kill"),
        "explicit.stat_1030153674"
    );
}
