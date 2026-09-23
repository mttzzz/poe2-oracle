//! End-to-end fixture tests against real, verbatim PoE2 clipboard text (`tests/fixtures/*.txt`,
//! copied character-for-character from `exiled-exchange-2/renderer/specs/Parser/items.ts` and
//! `ru-exceptional.test.ts` this session -- see `.tmp/research/ItemTextFormat.md`). Each test
//! asserts the exact field values that source file's own `TestItem` fixture recorded, translated
//! to this crate's field names/shapes, not just "parses without error".
//!
//! `test_catalog()` is a synthetic `StatCatalog` covering exactly the stat texts these fixtures'
//! mods resolve to -- not the real 8297-entry live catalog (impractical and non-deterministic for
//! an offline unit test), but real trade-API *shape* (`{mod_type}.{opaque id}` id strings,
//! sign-less `#`-templated text).

use item_parser::{ItemLanguage, ParseError, parse_clipboard};
use poe2_domain::{
    BlightedKind, ElementKind, ItemRarity, ModGeneration, ModifierType, StatCatalog, TradeStat,
};

fn stat(mod_type: &str, text: &str, id: &str) -> TradeStat {
    TradeStat {
        id: format!("{mod_type}.{id}"),
        text: text.to_string(),
        mod_type: mod_type.to_string(),
    }
}

fn test_catalog() -> StatCatalog {
    StatCatalog {
        stats: vec![
            // MagicItem / RareItem / RareWithImplicit / ArmourHighValueRareItem / TwoImplicitItem
            stat(
                "explicit",
                "Adds # to # Lightning Damage",
                "stat_added_lightning",
            ),
            stat("explicit", "# to Strength", "stat_str"),
            stat("explicit", "Adds # to # Fire Damage", "stat_added_fire"),
            stat("explicit", "Adds # to # Cold Damage", "stat_added_cold"),
            stat("explicit", "# to Accuracy Rating", "stat_accuracy"),
            stat("explicit", "#% increased Light Radius", "stat_light_radius"),
            // UniqueItem
            stat("explicit", "#% increased Energy Shield", "stat_inc_es"),
            stat(
                "explicit",
                "#% increased Mana Regeneration Rate while stationary",
                "stat_mana_regen_stationary",
            ),
            stat("explicit", "#% to Lightning Resistance", "stat_light_res"),
            stat(
                "explicit",
                "#% to Maximum Lightning Resistance",
                "stat_max_light_res",
            ),
            stat(
                "explicit",
                "#% increased Mana Regeneration Rate",
                "stat_mana_regen",
            ),
            // RareWithImplicit
            stat(
                "implicit",
                "#% to all Elemental Resistances",
                "stat_all_ele_res",
            ),
            stat("explicit", "# to Evasion Rating", "stat_evasion"),
            stat("explicit", "#% to Cold Resistance", "stat_cold_res"),
            // HighDamageRareItem
            stat("rune", "#% increased Physical Damage", "stat_rune_phys"),
            stat("explicit", "#% increased Physical Damage", "stat_inc_phys"),
            stat(
                "desecrated",
                "Adds # to # Physical Damage",
                "stat_desecrated_added_phys",
            ),
            stat(
                "fractured",
                "#% increased Attack Speed",
                "stat_fractured_aps",
            ),
            stat(
                "explicit",
                "# to Level of all Projectile Skills",
                "stat_projectile_gem_level",
            ),
            stat("explicit", "Loads # additional bolts", "stat_loads_bolts"),
            // ArmourHighValueRareItem
            stat(
                "rune",
                "#% increased Armour, Evasion and Energy Shield",
                "stat_rune_ares",
            ),
            stat("explicit", "#% increased Armour", "stat_inc_armour"),
            stat("explicit", "# to Armour", "stat_added_armour"),
            stat("desecrated", "# to Armour", "stat_desecrated_added_armour"),
            stat(
                "explicit",
                "#% reduced Duration of Bleeding on You",
                "stat_reduced_bleed_duration",
            ),
            stat(
                "explicit",
                "Hits against you have #% reduced Critical Damage Bonus",
                "stat_reduced_crit_damage_bonus",
            ),
            // TwoImplicitItem
            stat(
                "implicit",
                "#% reduced Charm Charges used",
                "stat_reduced_charm_charges",
            ),
            stat("implicit", "Has # Charm Slots", "stat_charm_slots"),
            stat(
                "explicit",
                "#% increased Charm Effect Duration",
                "stat_charm_duration",
            ),
            stat("explicit", "# to maximum Life", "stat_max_life"),
            stat("explicit", "#% to Fire Resistance", "stat_fire_res"),
            stat("explicit", "# to Stun Threshold", "stat_stun_threshold"),
        ],
    }
}

fn parse(fixture: &str, language: ItemLanguage) -> poe2_domain::ParsedItem {
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("reading fixture {fixture}: {e}"));
    let catalog = test_catalog();
    parse_clipboard(&text, language, &catalog)
        .unwrap_or_else(|e| panic!("parsing fixture {fixture} failed: {e}"))
}

#[test]
fn normal_item_helmet() {
    let item = parse("normal_item_en.txt", ItemLanguage::English);
    assert_eq!(
        item.name, "Divine Crown",
        "Superior prefix must be stripped"
    );
    assert_eq!(item.rarity, Some(ItemRarity::Normal));
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("armour.helmet")
    );
    assert_eq!(item.quality, Some(9));
    assert_eq!(item.armour, Some(174));
    assert_eq!(item.energy_shield, Some(60));
    assert_eq!(item.item_level, Some(81));
    let req = item.requirements.expect("requirements");
    assert_eq!((req.level, req.str, req.dex, req.int), (75, 67, 0, 67));
    assert!(item.mods.is_empty());
}

#[test]
fn magic_item_two_hand_mace() {
    let item = parse("magic_item_en.txt", ItemLanguage::English);
    assert_eq!(item.name, "Crackling Temple Maul of the Brute");
    assert_eq!(item.rarity, Some(ItemRarity::Magic));
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("weapon.twomace")
    );
    assert_eq!(item.weapon_physical, Some((35, 72)));
    assert_eq!(item.weapon_elemental, vec![(ElementKind::Lightning, 1, 50)]);
    assert_eq!(item.weapon_crit, Some(5.0));
    assert_eq!(item.weapon_aps, Some(1.2));
    assert_eq!(item.item_level, Some(32));
    let req = item.requirements.expect("requirements");
    assert_eq!((req.level, req.str, req.dex, req.int), (28, 57, 0, 0));

    assert_eq!(item.mods.len(), 2, "one prefix + one suffix");
    let prefix = &item.mods[0];
    assert_eq!(prefix.info.modifier_type, ModifierType::Explicit);
    assert_eq!(prefix.info.name.as_deref(), Some("Crackling"));
    assert_eq!(prefix.info.tier, Some(7));
    // "Adds 1(1-4) to 50(46-66) Lightning Damage": one stat, rolled at the average the trade
    // API filters these by (EE2's `getRollOrMinmaxAvg`), not one stat per number.
    assert_eq!(prefix.stats.len(), 1);
    assert_eq!(
        prefix.stats[0].stat_id.as_deref(),
        Some("explicit.stat_added_lightning")
    );
    assert_eq!(
        (
            prefix.stats[0].value,
            prefix.stats[0].min,
            prefix.stats[0].max
        ),
        (25.5, 23.5, 35.0)
    );

    let suffix = &item.mods[1];
    assert_eq!(suffix.info.name.as_deref(), Some("of the Brute"));
    assert_eq!(suffix.info.tier, Some(8));
    assert_eq!(suffix.stats.len(), 1);
    assert_eq!(
        suffix.stats[0].stat_id.as_deref(),
        Some("explicit.stat_str")
    );
    assert_eq!(
        (
            suffix.stats[0].value,
            suffix.stats[0].min,
            suffix.stats[0].max
        ),
        (8.0, 5.0, 8.0)
    );
}

#[test]
fn rare_item_bow() {
    let item = parse("rare_item_en.txt", ItemLanguage::English);
    assert_eq!(item.name, "Oblivion Strike");
    assert_eq!(item.base_type.as_deref(), Some("Rider Bow"));
    assert_eq!(item.rarity, Some(ItemRarity::Rare));
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("weapon.bow")
    );
    assert_eq!(item.weapon_physical, Some((36, 61)));
    assert_eq!(
        item.weapon_elemental,
        vec![
            (ElementKind::Fire, 27, 36),
            (ElementKind::Cold, 9, 13),
            (ElementKind::Lightning, 5, 82),
        ]
    );
    let req = item.requirements.expect("requirements");
    assert_eq!((req.level, req.str, req.dex, req.int), (51, 0, 103, 0));

    assert_eq!(item.mods.len(), 4, "3 prefixes + 1 suffix");
    assert!(
        item.mods[..3]
            .iter()
            .all(|m| m.info.modifier_type == ModifierType::Explicit && m.info.tier.is_some())
    );
    let suffix = &item.mods[3];
    assert_eq!(suffix.info.name.as_deref(), Some("of Radiance"));
    assert_eq!(
        suffix.stats.len(),
        2,
        "accuracy + light radius on one bracket"
    );
    assert_eq!(
        suffix.stats[0].stat_id.as_deref(),
        Some("explicit.stat_accuracy")
    );
    assert_eq!(
        (
            suffix.stats[0].value,
            suffix.stats[0].min,
            suffix.stats[0].max
        ),
        (57.0, 41.0, 60.0)
    );
    assert_eq!(
        suffix.stats[1].stat_id.as_deref(),
        Some("explicit.stat_light_radius")
    );
    assert_eq!(
        (
            suffix.stats[1].value,
            suffix.stats[1].min,
            suffix.stats[1].max
        ),
        (15.0, 15.0, 15.0)
    );
}

#[test]
fn unique_item_focus_with_flavour_text() {
    let item = parse("unique_item_en.txt", ItemLanguage::English);
    assert_eq!(item.name, "The Eternal Spark");
    assert_eq!(item.base_type.as_deref(), Some("Crystal Focus"));
    assert_eq!(item.rarity, Some(ItemRarity::Unique));
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("armour.focus")
    );
    assert_eq!(item.energy_shield, Some(44));
    assert_eq!(item.item_level, Some(81));

    assert_eq!(
        item.mods.len(),
        5,
        "Unique Modifier brackets never merge across blocks"
    );
    assert!(
        item.mods
            .iter()
            .all(|m| m.info.modifier_type == ModifierType::Explicit)
    );
    assert!(
        item.mods.iter().all(|m| m.info.tier.is_none()),
        "Unique mods never roll a tier"
    );
    assert_eq!(
        item.mods[0].stats[0].stat_id.as_deref(),
        Some("explicit.stat_inc_es")
    );
    assert_eq!(
        (
            item.mods[0].stats[0].value,
            item.mods[0].stats[0].min,
            item.mods[0].stats[0].max
        ),
        (56.0, 50.0, 70.0)
    );
    // The flavour-text footer is inert leftover, not an error and not a mod.
}

#[test]
fn rare_with_implicit_ring_split_sections() {
    let item = parse("rare_with_implicit_en.txt", ItemLanguage::English);
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("accessory.ring")
    );
    let req = item.requirements.expect("requirements");
    assert_eq!((req.level, req.str, req.dex, req.int), (45, 0, 0, 0));

    assert_eq!(
        item.mods.len(),
        5,
        "1 implicit + 1 prefix + 3 suffixes (Warmth's 2 stats stay one mod)"
    );
    let implicit = &item.mods[0];
    assert_eq!(implicit.info.modifier_type, ModifierType::Implicit);
    assert_eq!(
        implicit.stats[0].stat_id.as_deref(),
        Some("implicit.stat_all_ele_res")
    );
    assert_eq!(
        (
            implicit.stats[0].value,
            implicit.stats[0].min,
            implicit.stats[0].max
        ),
        (8.0, 7.0, 10.0)
    );

    let warmth = &item.mods[3];
    assert_eq!(warmth.info.name.as_deref(), Some("of Warmth"));
    assert_eq!(
        warmth.stats.len(),
        2,
        "mana regen + light radius share one bracket"
    );

    let penguin = &item.mods[4];
    assert_eq!(penguin.info.name.as_deref(), Some("of the Penguin"));
    assert_eq!(
        penguin.stats[0].stat_id.as_deref(),
        Some("explicit.stat_cold_res")
    );
}

#[test]
fn high_damage_rare_item_mixed_rune_desecrated_fractured() {
    let item = parse("high_damage_rare_item_en.txt", ItemLanguage::English);
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("weapon.crossbow")
    );
    assert_eq!(item.quality, Some(29));
    assert_eq!(
        item.weapon_physical,
        Some((414, 1043)),
        "thousands-separator comma stripped"
    );
    assert_eq!(item.weapon_aps, Some(2.07));
    assert_eq!(item.weapon_reload_time, Some(0.6));
    assert_eq!(item.item_level, Some(82));
    let req = item.requirements.expect("requirements");
    assert_eq!(
        (req.level, req.str, req.dex, req.int),
        (79, 89, 89, 0),
        "(unmet) annotation stripped"
    );
    let sockets = item.sockets.expect("rune sockets");
    assert_eq!(sockets.normal, 2);

    // Flat rune line (its own pre-bracket section) + implicit + 3 prefixes + 3 suffixes = 8 total.
    assert_eq!(item.mods.len(), 8);
    let rune = item
        .mods
        .iter()
        .find(|m| m.info.modifier_type == ModifierType::Augment)
        .expect("rune mod");
    assert_eq!(
        rune.stats[0].stat_id.as_deref(),
        Some("rune.stat_rune_phys")
    );
    assert_eq!(rune.stats[0].value, 36.0);

    let implicit = item
        .mods
        .iter()
        .find(|m| m.info.modifier_type == ModifierType::Implicit)
        .expect("implicit mod");
    assert!(
        implicit.stats[0].stat_id.is_none(),
        "no catalog entry seeded for this flavour-only implicit"
    );
    assert!(implicit.stats[0].unscalable);

    let flaring = item
        .mods
        .iter()
        .find(|m| m.info.name.as_deref() == Some("Flaring"))
        .expect("Flaring prefix");
    assert_eq!(
        flaring.info.modifier_type,
        ModifierType::Desecrated,
        "flat (desecrated) stat-line suffix overrides the plain 'Prefix Modifier' bracket type"
    );
    assert_eq!(
        flaring.stats[0].stat_id.as_deref(),
        Some("desecrated.stat_desecrated_added_phys")
    );

    let infamy = item
        .mods
        .iter()
        .find(|m| m.info.name.as_deref() == Some("of Infamy"))
        .expect("of Infamy suffix");
    assert_eq!(infamy.info.modifier_type, ModifierType::Fractured);
    assert!(
        infamy.info.tier.is_none(),
        "this mod never rolled a tier in the source text"
    );

    assert!(
        item.mods
            .iter()
            .filter(|m| m.info.modifier_type == ModifierType::Explicit)
            .any(|m| m.info.name.as_deref() == Some("Merciless"))
    );

    assert!(
        item.is_fractured,
        "derived from the Fractured-typed 'of Infamy' mod, not just the footer line"
    );
}

#[test]
fn armour_high_value_rare_item_price_note() {
    let item = parse("armour_high_value_rare_item_en.txt", ItemLanguage::English);
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("armour.chest")
    );
    assert_eq!(item.quality, Some(20));
    assert_eq!(item.armour, Some(3075));
    assert_eq!(item.item_level, Some(80));
    assert_eq!(item.note.as_deref(), Some("~b/o 10 divine"));

    let bleed = item
        .mods
        .iter()
        .find(|m| m.info.name.as_deref() == Some("of Allaying"))
        .expect("of Allaying suffix");
    assert_eq!(
        bleed.stats[0].stat_id.as_deref(),
        Some("explicit.stat_reduced_bleed_duration")
    );
    assert_eq!(
        (bleed.stats[0].min, bleed.stats[0].max),
        (46.0, 50.0),
        "48(50-46) prints its bound descending -- min/max must be swapped, not left inverted"
    );

    let unmoving = item
        .mods
        .iter()
        .find(|m| m.info.name.as_deref() == Some("Unmoving"))
        .expect("Unmoving prefix");
    assert_eq!(unmoving.info.modifier_type, ModifierType::Desecrated);
    assert_eq!(
        unmoving.stats[0].stat_id.as_deref(),
        Some("desecrated.stat_desecrated_added_armour")
    );
}

#[test]
fn a_line_worded_against_the_catalog_is_negated_and_keeps_its_own_wording() {
    // The live catalog words bleed duration only as `increased` (2026-09-23); the fixture's
    // "of Allaying" suffix prints `48(50-46)% reduced Duration of Bleeding on You`.
    let catalog = StatCatalog {
        stats: vec![stat(
            "explicit",
            "#% increased Duration of Bleeding on You",
            "stat_1692879867",
        )],
    };
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/armour_high_value_rare_item_en.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture");
    let item = parse_clipboard(&text, ItemLanguage::English, &catalog).expect("parses");
    let bleed = item
        .mods
        .iter()
        .find(|m| m.info.name.as_deref() == Some("of Allaying"))
        .expect("of Allaying suffix");
    let stat = &bleed.stats[0];
    assert_eq!(stat.stat_id.as_deref(), Some("explicit.stat_1692879867"));
    assert_eq!((stat.value, stat.min, stat.max), (-48.0, -50.0, -46.0));
    assert_eq!(
        stat.negated_text.as_deref(),
        Some("#% reduced Duration of Bleeding on You")
    );
}

#[test]
fn rare_map_waystone_properties() {
    let item = parse("rare_map_en.txt", ItemLanguage::English);
    assert_eq!(item.base_type.as_deref(), Some("Waystone (Tier 14)"));
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("map.waystone")
    );
    let waystone = item.waystone.expect("waystone properties");
    assert_eq!(
        waystone.tier,
        Some(14),
        "derived from the free-text base-type line, no local item DB"
    );
    assert_eq!(waystone.revives, Some(2));
    assert_eq!(waystone.pack_size, Some(34));
    assert_eq!(waystone.rare_monsters, Some(28));
    assert_eq!(waystone.drop_chance, Some(75));
    assert_eq!(waystone.blighted, None);

    let shocking = item
        .mods
        .iter()
        .find(|m| m.info.name.as_deref() == Some("Shocking"))
        .expect("Shocking prefix");
    assert!(shocking.stats[0].unscalable, "— Unscalable Value suffix");
    assert!(
        shocking.stats[0].stat_id.is_none(),
        "no catalog entry seeded for this flag stat"
    );
}

#[test]
fn two_implicit_item_min_max_swap_and_split_implicits() {
    let item = parse("two_implicit_item_en.txt", ItemLanguage::English);
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("accessory.belt")
    );
    let req = item.requirements.expect("requirements");
    assert_eq!((req.level, req.str, req.dex, req.int), (59, 0, 0, 0));

    let implicits: Vec<_> = item
        .mods
        .iter()
        .filter(|m| m.info.modifier_type == ModifierType::Implicit)
        .collect();
    assert_eq!(
        implicits.len(),
        2,
        "two separate Implicit Modifier brackets, not merged"
    );
    assert_eq!(
        (implicits[0].stats[0].min, implicits[0].stats[0].max),
        (10.0, 15.0),
        "14(15-10)% prints descending -- swapped, not left inverted"
    );
    assert_eq!(
        implicits[1].stats[0].stat_id.as_deref(),
        Some("implicit.stat_charm_slots")
    );

    assert_eq!(
        item.mods.len() - implicits.len(),
        5,
        "2 prefix + 3 suffix explicit mods"
    );
}

#[test]
fn uncut_gem_currency_items_have_no_gem_level() {
    for (fixture, name) in [
        ("uncut_skill_gem_en.txt", "Uncut Skill Gem (Level 19)"),
        ("uncut_spirit_gem_en.txt", "Uncut Spirit Gem (Level 16)"),
        ("uncut_support_gem_en.txt", "Uncut Support Gem (Level 5)"),
    ] {
        let item = parse(fixture, ItemLanguage::English);
        assert_eq!(item.name, name, "fixture {fixture}");
        assert_eq!(
            item.category.as_ref().map(|c| c.id.as_str()),
            Some("currency"),
            "Rarity: Currency routes category directly, Item Class: text never consulted -- fixture {fixture}"
        );
        assert_eq!(item.rarity, None);
        assert_eq!(
            item.gem_level, None,
            "the '(Level N)' text is part of the NAME, not a 'Level: ' property line -- fixture {fixture}"
        );
        assert!(item.mods.is_empty(), "fixture {fixture}");
    }
}

#[test]
fn meta_skill_gem_missing_item_class_line() {
    let item = parse("meta_skill_gem_en.txt", ItemLanguage::English);
    assert_eq!(item.name, "Mirage Archer");
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("gem"),
        "no Item Class: line at all -- forced to the generic Gem category"
    );
    assert_eq!(item.gem_level, Some(14));
    let gem_sockets = item
        .gem_sockets
        .expect("gem sockets (G tokens, not rune S tokens)");
    assert_eq!(gem_sockets.number, 4);
    assert!(
        item.sockets.is_none(),
        "G-token sockets must not also populate rune AugmentSockets"
    );
    assert!(
        item.requirements.is_none(),
        "gem-category items short-circuit Requires: parsing entirely (2 differently-shaped lines)"
    );
}

#[test]
fn ru_exceptional_gendered_prefix_normal_item() {
    let item = parse("ru_exceptional_normal_item.txt", ItemLanguage::Russian);
    assert_eq!(
        item.name, "Золочёный доспех",
        "masculine 'Образцовый' prefix stripped (issue #1033 regex, not just the neuter form)"
    );
    assert_eq!(item.rarity, Some(ItemRarity::Normal));
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("armour.chest")
    );
    assert_eq!(item.armour, Some(261));
    assert_eq!(item.evasion, Some(237));
    let req = item.requirements.expect("requirements");
    assert_eq!((req.level, req.str, req.dex, req.int), (62, 54, 54, 0));
    let sockets = item.sockets.expect("sockets");
    assert_eq!(
        (sockets.current, sockets.normal),
        (3, 2),
        "three sockets on a body armour, whose base drops with two (EE2's getMaxSockets)"
    );
    assert_eq!(item.item_level, Some(80));
}

#[test]
fn quality_wording_is_stripped_only_from_a_bare_base_name() {
    // An unidentified item's name is its base, with the quality wording a Normal one gets.
    let text = "Item Class: Wands\nRarity: Rare\nSuperior Withered Wand\n--------\nItem Level: 69\n--------\nUnidentified\n";
    let item = parse_clipboard(text, ItemLanguage::English, &test_catalog()).expect("parses");
    assert_eq!(item.name, "Withered Wand");
    // A currency's own name can open with the same word: Exceptional Verisium is an exchange
    // item of its own, not a quality Verisium.
    let text = "Item Class: Stackable Currency\nRarity: Currency\nExceptional Verisium\n--------\nStack Size: 1/20\n";
    let item = parse_clipboard(text, ItemLanguage::English, &test_catalog()).expect("parses");
    assert_eq!(item.name, "Exceptional Verisium");
}

#[test]
fn ru_live_magic_boots_resolve_the_games_own_item_class() {
    // Copied from a live Russian client on the test machine (2026-09-22), CRLF line ends as the
    // Windows clipboard holds them. The game names this class "Обувь"
    // (`Data/Balance/Russian/ItemClasses.datc64`) -- not the trade site's "Сапоги" label the RU
    // table used to carry, which rejected every Russian boot as an unknown class.
    let item = parse(
        "ru_live_zdorovye_ponozhi_vaal_trollya.txt",
        ItemLanguage::Russian,
    );
    assert_eq!(
        item.category.as_ref().map(|c| c.id.as_str()),
        Some("armour.boots")
    );
    assert_eq!(item.rarity, Some(ItemRarity::Magic));
    assert_eq!(item.armour, Some(268));
    assert_eq!(item.item_level, Some(77));
    assert_eq!(item.mods.len(), 2);
    // "{ Префикс "Здоровье" ... }" / "{ Суффикс "тролля" ... }": the affix slot each occupies.
    assert_eq!(
        item.mods
            .iter()
            .map(|m| m.info.generation)
            .collect::<Vec<_>>(),
        vec![Some(ModGeneration::Prefix), Some(ModGeneration::Suffix)]
    );
}

#[test]
fn language_mismatch_is_reported_not_misparsed() {
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/normal_item_en.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let err = parse_clipboard(&text, ItemLanguage::Russian, &test_catalog()).unwrap_err();
    assert_eq!(
        err,
        ParseError::WrongLanguage {
            found: ItemLanguage::English,
            wanted: ItemLanguage::Russian
        }
    );
}

#[test]
fn unrecognized_item_class_is_reported_not_silently_miscategorized() {
    let text = "Item Class: Definitely Not A Real Class\nRarity: Rare\nSome Name\n--------\nItem Level: 1\n";
    let err = parse_clipboard(text, ItemLanguage::English, &test_catalog()).unwrap_err();
    assert_eq!(
        err,
        ParseError::UnrecognizedItemClass("Definitely Not A Real Class".to_string())
    );
}

#[test]
fn empty_text_is_reported() {
    let err = parse_clipboard("", ItemLanguage::English, &test_catalog()).unwrap_err();
    assert_eq!(err, ParseError::Empty);
}

#[test]
fn blighted_waystone_base_type_is_detected() {
    let text = "Item Class: Waystones\nRarity: Rare\nSome Name\nBlighted Waystone (Tier 5)\n--------\nItem Level: 60\n";
    let item = parse_clipboard(text, ItemLanguage::English, &test_catalog()).expect("parses");
    let waystone = item
        .waystone
        .expect("waystone properties synthesized even with no property block");
    assert_eq!(waystone.blighted, Some(BlightedKind::Blighted));
}

/// Every fixture's nameplate, as the source suite recorded it: EE2's `items.ts`
/// (`renderer/specs/Parser/`) and Sidekick's PoE2 parser tests
/// (`github.com/Sidekick-Poe/Sidekick`, `tests/Sidekick.Apis.Poe.Tests/Poe2English/Parser`, MIT,
/// fetched 2026-09-22; texts copied verbatim into `sidekick_*_en.txt`). Several Sidekick texts are
/// plain Ctrl+C copies from an older patch (`Requirements:` blocks, mods without `{ }` headers):
/// their nameplates and properties parse, their bracket-less mods do not, as in EE2.
/// `(fixture, trade category, rarity, name, base type)`.
type Nameplate = (
    &'static str,
    Option<&'static str>,
    Option<ItemRarity>,
    &'static str,
    Option<&'static str>,
);

const NAMEPLATES: &[Nameplate] = &[
    (
        "normal_item_en.txt",
        Some("armour.helmet"),
        Some(ItemRarity::Normal),
        "Divine Crown",
        None,
    ),
    (
        "magic_item_en.txt",
        Some("weapon.twomace"),
        Some(ItemRarity::Magic),
        "Crackling Temple Maul of the Brute",
        None,
    ),
    (
        "rare_item_en.txt",
        Some("weapon.bow"),
        Some(ItemRarity::Rare),
        "Oblivion Strike",
        Some("Rider Bow"),
    ),
    (
        "unique_item_en.txt",
        Some("armour.focus"),
        Some(ItemRarity::Unique),
        "The Eternal Spark",
        Some("Crystal Focus"),
    ),
    (
        "rare_with_implicit_en.txt",
        Some("accessory.ring"),
        Some(ItemRarity::Rare),
        "Rune Loop",
        Some("Prismatic Ring"),
    ),
    (
        "uncut_skill_gem_en.txt",
        Some("currency"),
        None,
        "Uncut Skill Gem (Level 19)",
        None,
    ),
    (
        "uncut_spirit_gem_en.txt",
        Some("currency"),
        None,
        "Uncut Spirit Gem (Level 16)",
        None,
    ),
    (
        "uncut_support_gem_en.txt",
        Some("currency"),
        None,
        "Uncut Support Gem (Level 5)",
        None,
    ),
    (
        "meta_skill_gem_en.txt",
        Some("gem"),
        None,
        "Mirage Archer",
        None,
    ),
    (
        "high_damage_rare_item_en.txt",
        Some("weapon.crossbow"),
        Some(ItemRarity::Rare),
        "Dragon Core",
        Some("Siege Crossbow"),
    ),
    (
        "armour_high_value_rare_item_en.txt",
        Some("armour.chest"),
        Some(ItemRarity::Rare),
        "Hate Pelt",
        Some("Soldier Cuirass"),
    ),
    (
        "wand_rare_item_en.txt",
        Some("weapon.wand"),
        Some(ItemRarity::Rare),
        "Doom Bite",
        Some("Withered Wand"),
    ),
    (
        "normal_shield_en.txt",
        Some("armour.shield"),
        Some(ItemRarity::Normal),
        "Polished Targe",
        None,
    ),
    (
        "two_implicit_item_en.txt",
        Some("accessory.belt"),
        Some(ItemRarity::Rare),
        "Corpse Snare",
        Some("Ornate Belt"),
    ),
    (
        "two_line_one_implicit_tablet_en.txt",
        Some("map.tablet"),
        Some(ItemRarity::Rare),
        "Planar Challenge",
        Some("Delirium Precursor Tablet"),
    ),
    (
        "rare_map_en.txt",
        Some("map.waystone"),
        Some(ItemRarity::Rare),
        "Desolate Route",
        Some("Waystone (Tier 14)"),
    ),
    (
        "rare_map_all_props_en.txt",
        Some("map.waystone"),
        Some(ItemRarity::Rare),
        "Blasted Control",
        Some("Waystone (Tier 16)"),
    ),
    (
        "fractured_item_en.txt",
        Some("weapon.bow"),
        Some(ItemRarity::Rare),
        "Miracle Siege",
        Some("Obliterator Bow"),
    ),
    (
        "fractured_item_no_mod_marked_en.txt",
        Some("weapon.bow"),
        Some(ItemRarity::Rare),
        "Miracle Siege",
        Some("Obliterator Bow"),
    ),
    (
        "requires_one_attribute_en.txt",
        Some("armour.boots"),
        Some(ItemRarity::Rare),
        "Dunerunner Sandals",
        None,
    ),
    (
        "new_explicit_type_definitions_en.txt",
        Some("accessory.amulet"),
        Some(ItemRarity::Rare),
        "Brood Locket",
        Some("Gold Amulet"),
    ),
    (
        "item_all_the_modifier_types_en.txt",
        Some("weapon.crossbow"),
        Some(ItemRarity::Rare),
        "Storm Core",
        Some("Gemini Crossbow"),
    ),
    (
        "spectre_inc_spirit_en.txt",
        Some("weapon.sceptre"),
        Some(ItemRarity::Rare),
        "Skull Song",
        Some("Shrine Sceptre"),
    ),
    (
        "unidentified_base_en.txt",
        Some("weapon.wand"),
        Some(ItemRarity::Rare),
        "Volatile Wand",
        None,
    ),
    (
        "unidentified_tier_en.txt",
        Some("accessory.ring"),
        Some(ItemRarity::Magic),
        "Sapphire Ring",
        None,
    ),
    (
        "charm_quality_en.txt",
        Some("flask.charm"),
        Some(ItemRarity::Magic),
        "Sprouting Silver Charm of the Bountiful",
        None,
    ),
    (
        "bow_three_augments_en.txt",
        Some("weapon.bow"),
        Some(ItemRarity::Rare),
        "Oblivion Branch",
        Some("Obliterator Bow"),
    ),
    (
        "sidekick_gloom_lash_en.txt",
        Some("accessory.belt"),
        Some(ItemRarity::Rare),
        "Gloom Lash",
        Some("Utility Belt"),
    ),
    (
        "sidekick_thunderstep_en.txt",
        Some("armour.boots"),
        Some(ItemRarity::Unique),
        "Thunderstep",
        Some("Steeltoe Boots"),
    ),
    (
        "sidekick_buckler_en.txt",
        Some("armour.buckler"),
        Some(ItemRarity::Magic),
        "Hale Wooden Buckler",
        None,
    ),
    (
        "sidekick_unusable_quiver_en.txt",
        Some("armour.quiver"),
        Some(ItemRarity::Normal),
        "Fire Quiver",
        None,
    ),
    (
        "sidekick_desecrated_boots_en.txt",
        Some("armour.boots"),
        Some(ItemRarity::Rare),
        "Victory Hoof",
        Some("Bastion Sabatons"),
    ),
    (
        "sidekick_atziri_step_en.txt",
        Some("armour.boots"),
        Some(ItemRarity::Unique),
        "Atziri's Step",
        Some("Cinched Boots"),
    ),
    (
        "sidekick_pain_guardian_en.txt",
        Some("armour.chest"),
        Some(ItemRarity::Rare),
        "Pain Guardian",
        Some("Sacramental Robe"),
    ),
    (
        "sidekick_ironride_en.txt",
        Some("armour.helmet"),
        Some(ItemRarity::Unique),
        "Ironride",
        Some("Visored Helm"),
    ),
    (
        "sidekick_loath_tread_en.txt",
        Some("armour.boots"),
        Some(ItemRarity::Rare),
        "Loath Tread",
        Some("Runeforged Sekhema Sandals"),
    ),
    (
        "sidekick_the_knight_errant_runemastered_en.txt",
        Some("armour.boots"),
        Some(ItemRarity::Unique),
        "The Knight-errant",
        Some("Runemastered Mail Sabatons"),
    ),
    (
        "sidekick_the_knight_errant_en.txt",
        Some("armour.boots"),
        Some(ItemRarity::Unique),
        "The Knight-errant",
        Some("Mail Sabatons"),
    ),
    (
        "sidekick_essence_en.txt",
        Some("currency"),
        None,
        "Essence of Enhancement",
        None,
    ),
    (
        "sidekick_alloy_en.txt",
        Some("currency"),
        None,
        "Swift Alloy",
        None,
    ),
    (
        "sidekick_life_flask_en.txt",
        Some("flask.life"),
        Some(ItemRarity::Magic),
        "Simmering Ultimate Life Flask of the Distiller",
        None,
    ),
    (
        "sidekick_charm_en.txt",
        Some("flask.charm"),
        Some(ItemRarity::Magic),
        "Analyst's Stone Charm of the Practitioner",
        None,
    ),
    (
        "sidekick_uncut_spirit16_en.txt",
        Some("currency"),
        None,
        "Uncut Spirit Gem (Level 16)",
        None,
    ),
    (
        "sidekick_support3_en.txt",
        Some("currency"),
        None,
        "Uncut Support Gem (Level 3)",
        None,
    ),
    (
        "sidekick_skill9_en.txt",
        Some("currency"),
        None,
        "Uncut Skill Gem (Level 9)",
        None,
    ),
    (
        "sidekick_herald_of_ice_en.txt",
        Some("gem"),
        None,
        "Herald of Ice",
        None,
    ),
    (
        "sidekick_cast_on_critical_en.txt",
        Some("gem"),
        None,
        "Cast on Critical",
        None,
    ),
    (
        "sidekick_emerald_en.txt",
        Some("jewel"),
        Some(ItemRarity::Rare),
        "Soul Bliss",
        Some("Emerald"),
    ),
    (
        "sidekick_time_lost_en.txt",
        Some("jewel"),
        Some(ItemRarity::Rare),
        "Fulgent Shard",
        Some("Time-Lost Ruby"),
    ),
    (
        "sidekick_dragon_ichor_en.txt",
        Some("jewel"),
        Some(ItemRarity::Rare),
        "Dragon Ichor",
        Some("Emerald"),
    ),
    (
        "sidekick_clashing_ruby_en.txt",
        Some("jewel"),
        Some(ItemRarity::Magic),
        "Clashing Ruby of Valour",
        None,
    ),
    (
        "sidekick_megalomaniac_en.txt",
        Some("jewel"),
        Some(ItemRarity::Unique),
        "Megalomaniac",
        Some("Diamond"),
    ),
    (
        "sidekick_relic_en.txt",
        Some("sanctum.relic"),
        Some(ItemRarity::Magic),
        "Revitalising Urn Relic of Flowing",
        None,
    ),
    (
        "sidekick_dodge_relic_en.txt",
        Some("sanctum.relic"),
        Some(ItemRarity::Magic),
        "Layered Urn Relic of Eluding",
        None,
    ),
    (
        "sidekick_ritual_tablet_en.txt",
        Some("map.tablet"),
        Some(ItemRarity::Rare),
        "Voidtouched Invocation",
        Some("Ritual Tablet"),
    ),
    (
        "sidekick_freedom_of_faith_en.txt",
        Some("map.tablet"),
        Some(ItemRarity::Unique),
        "Freedom of Faith",
        Some("Ritual Tablet"),
    ),
    (
        "sidekick_irradiated_tablet_en.txt",
        Some("map.tablet"),
        Some(ItemRarity::Rare),
        "Eerie Secrets",
        Some("Irradiated Tablet"),
    ),
    (
        "sidekick_breach_tablet_en.txt",
        Some("map.tablet"),
        Some(ItemRarity::Rare),
        "Mythic Anthem",
        Some("Breach Tablet"),
    ),
    (
        "sidekick_waystone_properties_en.txt",
        Some("map.waystone"),
        Some(ItemRarity::Rare),
        "Forsaken Bearings",
        Some("Waystone (Tier 1)"),
    ),
    (
        "sidekick_waystone_tier13_en.txt",
        Some("map.waystone"),
        Some(ItemRarity::Rare),
        "Putrid Navigation",
        Some("Waystone (Tier 13)"),
    ),
    (
        "sidekick_staff_en.txt",
        Some("weapon.staff"),
        Some(ItemRarity::Magic),
        "Chalybeous Ashen Staff of the Augur",
        None,
    ),
    (
        "sidekick_cold_bow_en.txt",
        Some("weapon.bow"),
        Some(ItemRarity::Rare),
        "Brood Fletch",
        Some("Expert Composite Bow"),
    ),
    (
        "sidekick_elemental_crossbow_en.txt",
        Some("weapon.crossbow"),
        Some(ItemRarity::Rare),
        "Blood Core",
        Some("Bleak Crossbow"),
    ),
    (
        "sidekick_spirit_en.txt",
        Some("weapon.sceptre"),
        Some(ItemRarity::Magic),
        "Burning Rattling Sceptre",
        None,
    ),
    (
        "sidekick_spear_en.txt",
        Some("weapon.spear"),
        Some(ItemRarity::Magic),
        "Precise Ironhead Spear",
        None,
    ),
    (
        "sidekick_elemental_spear_en.txt",
        Some("weapon.spear"),
        Some(ItemRarity::Rare),
        "Hypnotic Edge",
        Some("Forked Spear"),
    ),
    (
        "sidekick_talisman_en.txt",
        Some("weapon.talisman"),
        Some(ItemRarity::Magic),
        "Lumbering Talisman of Consumption",
        None,
    ),
];

#[test]
fn every_fixture_parses_to_its_recorded_nameplate() {
    for &(fixture, category, rarity, name, base_type) in NAMEPLATES {
        let item = parse(fixture, ItemLanguage::English);
        assert_eq!(
            item.category.as_ref().map(|c| c.id.as_str()),
            category,
            "{fixture}"
        );
        assert_eq!(item.rarity, rarity, "{fixture}");
        assert_eq!(item.name, name, "{fixture}");
        assert_eq!(item.base_type.as_deref(), base_type, "{fixture}");
    }
}

#[test]
fn every_item_class_the_game_names_resolves_in_both_clients() {
    // `itemclasses.tsv`: every named row of the live client's ItemClasses.datc64, EN and RU
    // (`Id\tEN Name\tRU Name`, extracted 2026-09-22). A class missing from `categories.rs` would
    // fail every item of it with `UnrecognizedItemClass`.
    let rows = std::fs::read_to_string(format!(
        "{}/tests/fixtures/itemclasses.tsv",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("itemclasses.tsv");
    for row in rows.lines() {
        let mut fields = row.split('\t');
        let (id, en, ru) = (fields.next(), fields.next(), fields.next());
        for (language, class, text) in [
            (
                ItemLanguage::English,
                en,
                format!("Item Class: {}\nRarity: Rare\nName\nBase\n", en.unwrap()),
            ),
            (
                ItemLanguage::Russian,
                ru,
                format!(
                    "Класс предмета: {}\nРедкость: Редкий\nИмя\nОснова\n",
                    ru.unwrap()
                ),
            ),
        ] {
            let parsed = parse_clipboard(&text, language, &test_catalog());
            // A gamble offer's class is known, and reported as the offer it is.
            let gamble = en == Some("Hidden Items") && parsed == Err(ParseError::Unrevealed);
            assert!(
                parsed.is_ok() || gamble,
                "{id:?} class {class:?} ({language:?}): {parsed:?}"
            );
        }
    }
}

#[test]
fn classes_without_a_trade_category_parse_without_one() {
    let text = "Item Class: Wombgifts\nRarity: Normal\nLavish Wombgift\n--------\nItem Level: 81\n";
    let item = parse_clipboard(text, ItemLanguage::English, &test_catalog()).expect("parses");
    assert_eq!(item.category, None);
    assert_eq!(item.item_level, Some(81));
    let text = "Item Class: Trial Coins\nRarity: Normal\nDjinn Barya\n--------\nArea Level: 75\nNumber of Trials: 4\n";
    let item = parse_clipboard(text, ItemLanguage::English, &test_catalog()).expect("parses");
    assert_eq!(item.category.map(|c| c.id), Some("map.barya".to_string()));
    assert_eq!(item.area_level, Some(75));
    assert_eq!(item.trials.and_then(|t| t.number_of_trials), Some(4));
}

/// The live RU catalog entries (`GET https://ru.pathofexile.com/api/trade2/data/stats`,
/// 2026-09-22) the `ru_live_*` items' stat lines resolve to: `id\ttype\ttext`, line breaks in
/// a text written `\n`. Against that whole catalog (8298 entries) every line of every one of
/// these items resolved; this is the slice those lines used, plus the global stats behind the
/// local ones a weapon resolves to.
fn ru_live_catalog() -> StatCatalog {
    let rows = std::fs::read_to_string(format!(
        "{}/tests/fixtures/ru_live_stats.tsv",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("ru_live_stats.tsv");
    StatCatalog {
        stats: rows
            .lines()
            .map(|row| {
                let mut fields = row.splitn(3, '\t');
                let (id, mod_type, text) = (fields.next(), fields.next(), fields.next());
                TradeStat {
                    id: id.expect("id").to_string(),
                    mod_type: mod_type.expect("type").to_string(),
                    text: text.expect("text").replace("\\n", "\n"),
                }
            })
            .collect(),
    }
}

fn parse_ru_live(fixture: &str) -> poe2_domain::ParsedItem {
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("reading fixture {fixture}: {e}"));
    parse_clipboard(&text, ItemLanguage::Russian, &ru_live_catalog())
        .unwrap_or_else(|e| panic!("parsing fixture {fixture} failed: {e}"))
}

/// `(fixture, trade category, rarity, name, base type, stack size, item level)`.
type RuLive = (
    &'static str,
    Option<&'static str>,
    Option<ItemRarity>,
    &'static str,
    Option<&'static str>,
    Option<(u32, u32)>,
    Option<u32>,
);

/// The player's own items, copied from the live Russian client on 2026-09-22 (`ru_live_*.txt`,
/// CRLF as the Windows clipboard holds them).
const RU_LIVE: &[RuLive] = &[
    (
        "ru_live_adskiy_putevoy_kamen_ur_13_ukloneniya.txt",
        Some("map.waystone"),
        Some(ItemRarity::Magic),
        "Адский Путевой камень (Ур. 13) уклонения",
        None,
        None,
        Some(79),
    ),
    (
        "ru_live_bespamyatnoe_chistilische.txt",
        Some("weapon.crossbow"),
        Some(ItemRarity::Rare),
        "Беспамятное чистилище",
        Some("Самострел канонира"),
        None,
        Some(79),
    ),
    (
        "ru_live_bolshaya_runa_pererozhdeniya.txt",
        Some("currency"),
        None,
        "Большая руна перерождения",
        None,
        Some((1, 10)),
        None,
    ),
    (
        "ru_live_bolshaya_runa_videniya.txt",
        Some("currency"),
        None,
        "Большая руна видения",
        None,
        Some((3, 10)),
        None,
    ),
    (
        "ru_live_bolshaya_runa_zheleza.txt",
        Some("currency"),
        None,
        "Большая руна железа",
        None,
        Some((1, 10)),
        None,
    ),
    (
        "ru_live_bolshaya_sfera_prevrascheniya.txt",
        Some("currency"),
        None,
        "Большая сфера превращения",
        None,
        Some((2, 40)),
        None,
    ),
    (
        "ru_live_bolshaya_sfera_usileniya.txt",
        Some("currency"),
        None,
        "Большая сфера усиления",
        None,
        Some((4, 30)),
        None,
    ),
    (
        "ru_live_bolshaya_sfera_vozvysheniya.txt",
        Some("currency"),
        None,
        "Большая сфера возвышения",
        None,
        Some((1, 20)),
        None,
    ),
    (
        "ru_live_detal_dospeha.txt",
        Some("currency"),
        None,
        "Деталь доспеха",
        None,
        Some((4, 20)),
        None,
    ),
    (
        "ru_live_entropicheskaya_kayma.txt",
        Some("accessory.ring"),
        Some(ItemRarity::Rare),
        "Энтропическая кайма",
        Some("Кольцо без камня"),
        None,
        Some(78),
    ),
    (
        "ru_live_glificheskiy_uvyadshiy_zhezl_katastrofy.txt",
        Some("weapon.wand"),
        Some(ItemRarity::Magic),
        "Глифический Увядший жезл катастрофы",
        None,
        None,
        Some(78),
    ),
    (
        "ru_live_kopenosnyy_izumrud_prigvozhdeniya.txt",
        Some("jewel"),
        Some(ItemRarity::Magic),
        "Копьеносный Изумруд пригвождения",
        None,
        None,
        Some(78),
    ),
    (
        "ru_live_krutyaschiy_obodok.txt",
        Some("accessory.ring"),
        Some(ItemRarity::Rare),
        "Крутящий ободок",
        Some("Кольцо с аметистом"),
        None,
        Some(79),
    ),
    (
        "ru_live_malaya_sfera_zlatokuznetsa.txt",
        Some("currency"),
        None,
        "Малая сфера златокузнеца",
        None,
        Some((2, 20)),
        None,
    ),
    (
        "ru_live_nachertannyy_ultimatum.txt",
        Some("currency"),
        None,
        "Начертанный Ультиматум",
        None,
        None,
        Some(78),
    ),
    (
        "ru_live_neogranennyy_kamen_duha_uroven_17.txt",
        Some("currency"),
        None,
        "Неогранённый камень духа (уровень 17)",
        None,
        None,
        None,
    ),
    (
        "ru_live_neogranennyy_kamen_umeniya_uroven_17.txt",
        Some("currency"),
        None,
        "Неогранённый камень умения (уровень 17)",
        None,
        None,
        None,
    ),
    (
        "ru_live_oskolok_tsarey.txt",
        Some("currency"),
        None,
        "Осколок царей",
        None,
        Some((5, 10)),
        None,
    ),
    (
        "ru_live_plitka_bezdny.txt",
        Some("map.tablet"),
        Some(ItemRarity::Normal),
        "Плитка Бездны",
        None,
        None,
        Some(78),
    ),
    (
        "ru_live_predznamenovanie_chernokrovnyh.txt",
        Some("currency"),
        None,
        "Предзнаменование чернокровных",
        None,
        Some((2, 10)),
        None,
    ),
    (
        "ru_live_predznamenovanie_haotichnogo_kolichestva.txt",
        Some("currency"),
        None,
        "Предзнаменование хаотичного количества",
        None,
        Some((1, 10)),
        None,
    ),
    (
        "ru_live_predznamenovanie_otgoloskov_bezdny.txt",
        Some("currency"),
        None,
        "Предзнаменование отголосков Бездны",
        None,
        Some((2, 10)),
        None,
    ),
    (
        "ru_live_predznamenovanie_razlozheniya.txt",
        Some("currency"),
        None,
        "Предзнаменование разложения",
        None,
        Some((2, 10)),
        None,
    ),
    (
        "ru_live_predznamenovanie_sveta.txt",
        Some("currency"),
        None,
        "Предзнаменование света",
        None,
        Some((1, 10)),
        None,
    ),
    (
        "ru_live_sfera_alhimii.txt",
        Some("currency"),
        None,
        "Сфера алхимии",
        None,
        Some((2, 20)),
        None,
    ),
    (
        "ru_live_sfera_astromantii.txt",
        Some("currency"),
        None,
        "Сфера астромантии",
        None,
        Some((1, 20)),
        None,
    ),
    (
        "ru_live_sfera_haosa.txt",
        Some("currency"),
        None,
        "Сфера хаоса",
        None,
        Some((1, 20)),
        None,
    ),
    (
        "ru_live_sfera_prevrascheniya.txt",
        Some("currency"),
        None,
        "Сфера превращения",
        None,
        Some((24, 40)),
        None,
    ),
    (
        "ru_live_sfera_tsarey.txt",
        Some("currency"),
        None,
        "Сфера царей",
        None,
        Some((9, 20)),
        None,
    ),
    (
        "ru_live_sfera_usileniya.txt",
        Some("currency"),
        None,
        "Сфера усиления",
        None,
        Some((19, 30)),
        None,
    ),
    (
        "ru_live_sfera_vaal.txt",
        Some("currency"),
        None,
        "Сфера ваал",
        None,
        Some((1, 20)),
        None,
    ),
    (
        "ru_live_sfera_vozvysheniya.txt",
        Some("currency"),
        None,
        "Сфера возвышения",
        None,
        Some((13, 20)),
        None,
    ),
    (
        "ru_live_sohranivshayasya_chelyust.txt",
        Some("currency"),
        None,
        "Сохранившаяся челюсть",
        None,
        Some((2, 20)),
        None,
    ),
    (
        "ru_live_sohranivsheesya_rebro.txt",
        Some("currency"),
        None,
        "Сохранившееся ребро",
        None,
        Some((4, 20)),
        None,
    ),
    (
        "ru_live_svitok_mudrosti.txt",
        Some("currency"),
        None,
        "Свиток мудрости",
        None,
        Some((2, 40)),
        None,
    ),
    (
        "ru_live_tochilnyy_kamen.txt",
        Some("currency"),
        None,
        "Точильный камень",
        None,
        Some((1, 20)),
        None,
    ),
    (
        "ru_live_tyazhelyy_remen.txt",
        Some("accessory.belt"),
        Some(ItemRarity::Normal),
        "Тяжёлый ремень",
        None,
        None,
        Some(78),
    ),
    // A quest item (`Редкость: Задание` counts as Normal, as in EE2) of a class the trade site
    // has no category for.
    (
        "ru_live_vstrecha_s_hozyainom.txt",
        None,
        Some(ItemRarity::Normal),
        "Встреча с Хозяином",
        None,
        None,
        None,
    ),
    (
        "ru_live_zdorovye_ponozhi_vaal_trollya.txt",
        Some("armour.boots"),
        Some(ItemRarity::Magic),
        "Здоровые Поножи ваал тролля",
        None,
        None,
        Some(77),
    ),
    (
        "ru_live_zhemchuzhnoe_koltso.txt",
        Some("accessory.ring"),
        Some(ItemRarity::Normal),
        "Жемчужное кольцо",
        None,
        None,
        Some(78),
    ),
];

#[test]
fn every_ru_live_item_parses_to_its_nameplate_and_properties() {
    let on_disk = std::fs::read_dir(format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR")))
        .expect("fixtures dir")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("ru_live_") && name.ends_with(".txt"))
        .count();
    assert_eq!(
        on_disk,
        RU_LIVE.len(),
        "every ru_live_ fixture is pinned below"
    );
    for &(fixture, category, rarity, name, base_type, stack_size, item_level) in RU_LIVE {
        let item = parse_ru_live(fixture);
        assert_eq!(
            item.category.as_ref().map(|c| c.id.as_str()),
            category,
            "{fixture}"
        );
        assert_eq!(item.rarity, rarity, "{fixture}");
        assert_eq!(item.name, name, "{fixture}");
        assert_eq!(item.base_type.as_deref(), base_type, "{fixture}");
        assert_eq!(item.stack_size, stack_size, "{fixture}");
        assert_eq!(item.item_level, item_level, "{fixture}");
        // Live, every stat line of these items resolved on the RU catalog, so here too.
        assert!(
            item.unknown_mods.is_empty(),
            "{fixture}: {:?}",
            item.unknown_mods
        );
        for stat in item.mods.iter().flat_map(|m| &m.stats) {
            assert!(stat.stat_id.is_some(), "{fixture}: {stat:?}");
        }
    }
}

#[test]
fn ru_live_magic_waystone_reads_its_tier_from_its_name() {
    // A magic waystone prints no base-type line and, on this client, no `Уровень путевого
    // камня:` line: its tier is in its name only. Missing, the waystone searched every tier.
    let item = parse_ru_live("ru_live_adskiy_putevoy_kamen_ur_13_ukloneniya.txt");
    let waystone = item.waystone.expect("waystone properties");
    assert_eq!(waystone.tier, Some(13));
    assert_eq!(
        (
            waystone.revives,
            waystone.pack_size,
            waystone.effectiveness,
            waystone.drop_chance
        ),
        (Some(4), Some(6), Some(16), Some(30))
    );
    // "Монстры уклончивы": a flag stat.
    assert!(item.mods[1].stats[0].unscalable);
}

#[test]
fn ru_live_gear_reads_its_properties_and_mods() {
    let crossbow = parse_ru_live("ru_live_bespamyatnoe_chistilische.txt");
    assert_eq!(crossbow.weapon_physical, Some((23, 90)));
    assert_eq!(
        (
            crossbow.weapon_crit,
            crossbow.weapon_aps,
            crossbow.weapon_reload_time
        ),
        (Some(5.0), Some(1.65), Some(0.75))
    );
    let req = crossbow.requirements.expect("requirements");
    assert_eq!((req.level, req.str, req.dex, req.int), (59, 56, 56, 0));
    // A weapon's accuracy is the local stat, as the trade site indexes it.
    assert_eq!(
        crossbow.mods[1].stats[0].stat_id.as_deref(),
        Some("explicit.stat_691932474")
    );

    let tablet = parse_ru_live("ru_live_plitka_bezdny.txt");
    // The two-line implicit "Добавляет Бездны на карту / Осталось зарядов - 10" is one stat.
    let uses = &tablet.mods[0].stats;
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].stat_id.as_deref(), Some("implicit.stat_2369421690"));
    assert_eq!(uses[0].value, 10.0);

    let ultimatum = parse_ru_live("ru_live_nachertannyy_ultimatum.txt");
    assert_eq!(ultimatum.area_level, Some(78));
    let trials = ultimatum.trials.expect("trials");
    assert_eq!(trials.number_of_trials, Some(10));
    assert_eq!(
        trials.ultimatum_hint,
        Some(poe2_domain::UltimatumHint::Deadly)
    );

    let wand = parse_ru_live("ru_live_glificheskiy_uvyadshiy_zhezl_katastrofy.txt");
    let skill = &wand.mods[0];
    assert_eq!(skill.info.modifier_type, ModifierType::Skill);
    assert_eq!(skill.stats[0].stat_id.as_deref(), Some("skill.chaosbolt"));
    assert_eq!(skill.stats[0].value, 17.0);

    let belt = parse_ru_live("ru_live_tyazhelyy_remen.txt");
    let implicits: Vec<_> = belt
        .mods
        .iter()
        .filter(|m| m.info.modifier_type == ModifierType::Implicit)
        .collect();
    assert_eq!(implicits.len(), 2);
    let slots = &implicits[1].stats[0];
    assert_eq!(slots.stat_id.as_deref(), Some("implicit.stat_1416292992"));
    assert_eq!((slots.value, slots.min, slots.max), (1.0, 1.0, 3.0));
}

/// Lines the trade catalog's own texts miss, read the way EE2 reads them (`stat_forms`), and the
/// catalog's `+#` texts: a vendor's items (live RU, 2026-09-23) printed the first two, and each of
/// these was an unread line before.
#[test]
fn printed_forms_the_catalog_lacks_resolve_to_their_stats() {
    let found = |item: &poe2_domain::ParsedItem, id: &str| {
        item.mods
            .iter()
            .flat_map(|modifier| &modifier.stats)
            .find(|stat| stat.stat_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("no {id} in {:?}", item.mods))
            .clone()
    };

    let ru = StatCatalog {
        stats: vec![
            stat(
                "explicit",
                "#% увеличение требований к характеристикам",
                "stat_3639275092",
            ),
            stat(
                "explicit",
                "Накладывает оцепенение при нанесении удара",
                "stat_2933846633",
            ),
            stat(
                "explicit",
                "+# к уровню всех умений Вихрь стрел",
                "stat_448592698|44",
            ),
        ],
    };
    let talisman = "Класс предмета: Талисманы\nРедкость: Редкий\nТест\nЖестокий талисман\n\
        --------\nУровень предмета: 65\n--------\n\
        { Суффикс \"умения\" (Уровень: 2) }\n30% снижение требований к характеристикам\n\
        { Префикс \"Оглушающий\" (Уровень: 1) }\n40% шанс наложения оцепенения при нанесении удара\n\
        { Суффикс \"лучника\" (Уровень: 1) }\n+1 к уровню всех умений Вихрь стрел\n";
    let item = parse_clipboard(talisman, ItemLanguage::Russian, &ru).expect("parses");
    assert!(item.unknown_mods.is_empty(), "{:?}", item.unknown_mods);
    // A negative roll in words no pair of `negations` covers: the catalog's stat at -30.
    let requirements = found(&item, "explicit.stat_3639275092");
    assert_eq!(requirements.value, -30.0);
    assert!(requirements.negated_text.is_some());
    // The chance form of a stat the catalog prints as its 100% effect, in the item's own words.
    let daze = found(&item, "explicit.stat_2933846633");
    assert_eq!(daze.value, 40.0);
    assert_eq!(
        daze.printed_text.as_deref(),
        Some("#% шанс наложения оцепенения при нанесении удара")
    );
    // A signed catalog text.
    assert_eq!(found(&item, "explicit.stat_448592698|44").value, 1.0);

    let en = StatCatalog {
        stats: vec![
            stat(
                "explicit",
                "#% Global chance to Blind Enemies on Hit",
                "stat_2221570601",
            ),
            // Live EN (2026-09-23): a keystone's own stat, then the same name as another
            // stat's option.
            stat("explicit", "Blood Magic", "stat_2801937280"),
            stat("explicit", "Blood Magic", "stat_3831171903|5"),
        ],
    };
    let ring = "Item Class: Rings\nRarity: Rare\nTest Loop\nRuby Ring\n--------\n\
        Item Level: 80\n--------\n\
        { Prefix Modifier \"Blinding\" (Tier: 1) }\nBlind Enemies on Hit\n\
        { Suffix Modifier \"of Blood\" (Tier: 1) }\nBlood Magic\n";
    let item = parse_clipboard(ring, ItemLanguage::English, &en).expect("parses");
    // The 100% effect form of a chance stat.
    assert_eq!(found(&item, "explicit.stat_2221570601").value, 100.0);
    // The keystone itself, not the option sharing its name.
    found(&item, "explicit.stat_2801937280");
}

/// A gambler's offer (live RU vendor, 2026-09-23): "Random Helmet" is revealed only once bought,
/// so it is reported as such rather than searched by its name, which no listing carries.
#[test]
fn a_gamble_offer_is_reported_unrevealed() {
    let text = "Класс предмета: Сокрытые предметы\r\nРедкость: Валюта\r\nСлучайный шлем\r\n\
        --------\r\nЭнергетический щит: ?\r\n--------\r\nУровень предмета: 76\r\n";
    assert_eq!(
        parse_clipboard(text, ItemLanguage::Russian, &ru_live_catalog()),
        Err(ParseError::Unrevealed)
    );
}

/// A magic item is its one prefix and one suffix, so both are searched whatever their tier: the
/// live wand's T3 `+3 к уровню всех камней умений чар хаоса` is what it sells for (reported by
/// the player, 2026-09-23), yet the rares' top-tier rule left it out.
#[test]
fn a_magic_items_affixes_are_all_searched() {
    let wand = parse_ru_live("ru_live_glificheskiy_uvyadshiy_zhezl_katastrofy.txt");
    let filters = stat_filters::build_filters(&wand, 10, &ru_live_catalog());
    let affixes: Vec<_> = filters
        .iter()
        .filter(|filter| filter.generation.is_some())
        .map(|filter| (filter.tier, filter.enabled))
        .collect();
    assert_eq!(affixes, [(Some(2), true), (Some(3), true)]);
}
