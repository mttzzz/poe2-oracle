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
/// type, other currency, fragments, cards and unidentified uniques to an exact type search, gear
/// and everything else to a filtered search.
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
    } else if category.starts_with("currency")
        || category == "map.fragment"
        || category == "card"
        || (item.is_unidentified && item.rarity == Some(ItemRarity::Unique))
    {
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

    // A tier-16 rare waystone searches its tier exactly; its properties are offered, unchecked.
    let waystone = built("rare_map_all_props_en.txt", &stats);
    let tier = row(&waystone, "map_filters.map_tier");
    let roll = tier.roll.as_ref().expect("tier roll");
    assert_eq!((roll.min, roll.max), (Some(16.0), Some(16.0)));
    assert!(tier.enabled);
    let pack_size = row(&waystone, "map_filters.map_packsize");
    assert_eq!(pack_size.roll.as_ref().and_then(|r| r.min), Some(20.0));
    assert!(!pack_size.enabled);

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
