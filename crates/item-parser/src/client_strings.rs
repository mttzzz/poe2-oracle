//! Literal markers and locale-specific regexes the PoE2 game client itself writes into clipboard
//! item text, one table per supported language. Every value here is copied verbatim from the
//! real, working reference's own shipped localization dictionary
//! (`exiled-exchange-2/renderer/public/data/{en,ru}/client_strings.js`, read directly this
//! session) -- cited as ground truth dictated by the PoE2 client's own localization, never as
//! code to copy. Only the subset `item-parser`'s section/nameplate/modifier parsers actually
//! consume is ported (client_strings.js has ~230 keys total; most are chat-log/passive-tree/
//! metamorph-organ concerns this crate has no parser for).
//!
//! Regex named-capture-group syntax differs between the source (JS `(?<name>...)`) and Rust's
//! `regex` crate (`(?P<name>...)`) -- translated mechanically, patterns are otherwise unchanged.

use std::sync::LazyLock;

use regex::Regex;

use crate::stat_forms::{self, StatForms};

pub struct ClientStrings {
    pub item_class: &'static str,
    pub rarity: &'static str,
    pub rarity_normal: &'static str,
    pub rarity_magic: &'static str,
    pub rarity_rare: &'static str,
    pub rarity_unique: &'static str,
    pub rarity_gem: &'static str,
    pub rarity_currency: &'static str,
    pub rarity_divcard: &'static str,
    pub rarity_quest: &'static str,
    pub item_level: &'static str,
    pub sockets: &'static str,
    pub quality: &'static str,
    pub armour: &'static str,
    pub evasion: &'static str,
    pub energy_shield: &'static str,
    pub runic_ward: &'static str,
    pub block_chance: &'static str,
    pub physical_damage: &'static str,
    pub elemental_damage: &'static str,
    pub fire_damage: &'static str,
    pub cold_damage: &'static str,
    pub lightning_damage: &'static str,
    pub crit_chance: &'static str,
    pub attack_speed: &'static str,
    pub reload_speed: &'static str,
    pub base_spirit: &'static str,
    /// `GEM_LEVEL: 'Level: '` -- Meta/Skill Gem's own gem-level property line (distinct from
    /// `ITEM_LEVEL`'s `Item Level: `).
    pub gem_level: &'static str,
    pub stack_size: &'static str,
    pub corrupted: &'static str,
    pub double_corrupted: &'static str,
    pub unmodifiable: &'static str,
    pub mirrored: &'static str,
    pub sanctified: &'static str,
    pub fractured_item: &'static str,
    pub price_note: &'static str,
    pub waystone_tier: &'static str,
    pub revives: &'static str,
    /// `WAYSTONE_PACK_SIZE`, in every form the client has printed it: English waystones read
    /// `Pack Size: +34%` (EE2's fixtures) and, since a later patch, `Monster Pack Size: +12%`
    /// (Sidekick's PoE2 fixtures, which also add the `Waystone Tier: N` line).
    pub pack_size: &'static [&'static str],
    pub magic_monsters: &'static str,
    pub rare_monsters: &'static str,
    pub waystone_drop_chance: &'static str,
    pub item_rarity: &'static str,
    pub monster_rarity: &'static str,
    pub monster_effectiveness: &'static str,
    /// `AREA_LEVEL` -- an Expedition Logbook's, a Barya's or an Inscribed Ultimatum's.
    pub area_level: &'static str,
    /// `TRIAL_COUNT` -- a Barya's or an Inscribed Ultimatum's number of trials.
    pub trial_count: &'static str,
    /// `ULTIMATUM_VICTORIOUS`/`COWARDLY`/`DEADLY` -- an Inscribed Ultimatum's hint line.
    pub ultimatum_victorious: &'static str,
    pub ultimatum_cowardly: &'static str,
    pub ultimatum_deadly: &'static str,
    /// `VEILED_PREFIX`/`VEILED_SUFFIX` -- the stat line an unrevealed desecrated affix prints in
    /// place of its stats.
    pub veiled_prefix: &'static str,
    pub veiled_suffix: &'static str,
    /// Bare `Fractured`/`Desecrated`/`Crafted` words -- bracket-mod `<Type>` text compounds them
    /// with `prefix_modifier`/`suffix_modifier` (e.g. `"Fractured Suffix Modifier"`), they never
    /// appear standalone as a property-line marker.
    pub prefix_modifier: &'static str,
    pub suffix_modifier: &'static str,
    pub implicit_modifier: &'static str,
    /// `ENCHANT_MODIFIER: 'Enhancement'` -- standalone bracket-mod `<Type>` value for Enchant.
    pub enchant_modifier: &'static str,
    /// `CORRUPTED_MODIFIER: 'Corruption Enhancement'` -- standalone bracket-mod `<Type>` value
    /// for Scourge (not the item-level `corrupted` flag marker, despite the name).
    pub corrupted_modifier: &'static str,
    pub crafted_modifier: &'static str,
    pub fractured_modifier: &'static str,
    pub desecrated_modifier: &'static str,
    /// Standalone bracket-mod `<Type>` values for Unique-item fixed mods; both map to
    /// `ModifierType::Explicit` (Unique mods are non-prefix/suffix but still roll-bearing).
    pub unique_modifier: &'static str,
    pub vaal_unique_modifier: &'static str,
    pub grants_skill: &'static str,
    pub cannot_use_item: &'static str,
    /// `UNSCALABLE_VALUE: ' — Unscalable Value'` -- trailing marker on a mod stat line with no
    /// numeric roll at all (a pure flag effect), e.g. `"Area has patches of Shocked Ground —
    /// Unscalable Value"`.
    pub unscalable_value: &'static str,
    /// `(positive, negative)` words the client swaps to print a stat's negative roll (`48%
    /// reduced X` is the catalog's `#% increased X` at -48). Not a client_strings key: the pairs
    /// behind the `negate` matchers of EE2's `stats.ndjson` (`renderer/public/data/{en,ru}`),
    /// counted 2026-09-22 -- these cover 631 of the 678 English and 623 of the 682 Russian ones.
    pub negations: &'static [(&'static str, &'static str)],
    /// Words the client prints for a count of one where the catalog text has `#` (`Bow Attacks
    /// fire an additional Arrow` is `Bow Attacks fire # additional Arrows`). Not a client_strings
    /// key: read off EE2's `stats.ndjson` matchers; the Russian client prints no such word.
    pub number_words: &'static [&'static str],
    /// The client's printed stat forms the trade catalog lacks (`stat_forms`), EE2's, for this
    /// language.
    pub stat_forms: &'static LazyLock<StatForms>,

    /// `REQUIRES_LINE` -- named groups `level`/`str`/`dex`/`int`, each optional.
    pub requires_line: Regex,
    /// `UNIDENTIFIED` -- named group `tier`, optional.
    pub unidentified: Regex,
    /// `ITEM_EXCEPTIONAL` -- RU is a 4-way gendered alternation (issue #1033 fix); named group
    /// `1` (the remainder after the decorative prefix/suffix is stripped).
    pub item_exceptional: Regex,
    /// `ITEM_SUPERIOR` -- EN is a prefix (`"Superior X"`), RU is a suffix (`"X высокого
    /// качества"`); both capture group `1` as the remainder.
    pub item_superior: Regex,
    pub flask_charges: Regex,
    /// `MODIFIER_LINE` -- the bracket `{ <type> ["<name>"] (Tier: N) (Rank: N) }` grammar, named
    /// groups `type`/`name`/`tier`/`rank`, all but `type` optional.
    pub modifier_line: Regex,
    /// `WAYSTONE_TIER_NAME` -- not a real client_strings key; the `"Waystone (Tier N)"` base type
    /// `item-parser` itself must recognize since this project ships no local item database to
    /// resolve a Waystone's tier the way the reference does (`info.map.tier`). Unanchored: a magic
    /// waystone prints it inside its affixed name (`Адский Путевой камень (Ур. 13) уклонения`,
    /// live RU client 2026-09-22, which printed no `Waystone Tier:` line). The Russian wording
    /// is that live copy's, and EE2's `ru/items.ndjson` names `Waystone (Tier 13)` the same.
    /// Named group `tier`.
    pub waystone_tier_name: Regex,
    /// `MAP_BLIGHTED`/`MAP_BLIGHT_RAVAGED` -- checked against `base_type`, not a property line;
    /// named group `rest` (unused -- only presence of a match matters).
    pub map_blighted: Regex,
    pub map_blight_ravaged: Regex,
}

pub static EN: LazyLock<ClientStrings> = LazyLock::new(|| {
    ClientStrings {
    item_class: "Item Class: ",
    rarity: "Rarity: ",
    rarity_normal: "Normal",
    rarity_magic: "Magic",
    rarity_rare: "Rare",
    rarity_unique: "Unique",
    rarity_gem: "Gem",
    rarity_currency: "Currency",
    rarity_divcard: "Divination Card",
    rarity_quest: "Quest",
    item_level: "Item Level: ",
    sockets: "Sockets: ",
    quality: "Quality: ",
    armour: "Armour: ",
    evasion: "Evasion Rating: ",
    energy_shield: "Energy Shield: ",
    runic_ward: "Runic Ward: ",
    block_chance: "Block chance: ",
    physical_damage: "Physical Damage: ",
    elemental_damage: "Elemental Damage: ",
    fire_damage: "Fire Damage: ",
    cold_damage: "Cold Damage: ",
    lightning_damage: "Lightning Damage: ",
    crit_chance: "Critical Hit Chance: ",
    attack_speed: "Attacks per Second: ",
    reload_speed: "Reload Time: ",
    base_spirit: "Spirit: ",
    gem_level: "Level: ",
    stack_size: "Stack Size: ",
    corrupted: "Corrupted",
    double_corrupted: "Twice Corrupted",
    unmodifiable: "Unmodifiable",
    mirrored: "Mirrored",
    sanctified: "Sanctified",
    fractured_item: "Fractured Item",
    price_note: "Note: ",
    waystone_tier: "Waystone Tier: ",
    revives: "Revives Available: ",
    pack_size: &["Pack Size: ", "Monster Pack Size: "],
    magic_monsters: "Magic Monsters: ",
    rare_monsters: "Rare Monsters: ",
    waystone_drop_chance: "Waystone Drop Chance: ",
    item_rarity: "Item Rarity: ",
    monster_rarity: "Monster Rarity: ",
    monster_effectiveness: "Monster Effectiveness: ",
    area_level: "Area Level: ",
    trial_count: "Number of Trials: ",
    ultimatum_victorious: "Victorious",
    ultimatum_cowardly: "Cowardly",
    ultimatum_deadly: "Deadly",
    veiled_prefix: "Desecrated Prefix",
    veiled_suffix: "Desecrated Suffix",
    prefix_modifier: "Prefix Modifier",
    suffix_modifier: "Suffix Modifier",
    implicit_modifier: "Implicit Modifier",
    enchant_modifier: "Enhancement",
    corrupted_modifier: "Corruption Enhancement",
    crafted_modifier: "Crafted",
    fractured_modifier: "Fractured",
    desecrated_modifier: "Desecrated",
    unique_modifier: "Unique Modifier",
    vaal_unique_modifier: "Vaal Unique Modifier",
    grants_skill: "Grants Skill: ",
    cannot_use_item: "You cannot use this item. Its stats will be ignored",
    unscalable_value: " — Unscalable Value",
    negations: &[("increased", "reduced"), ("more", "less"), ("faster", "slower")],
    number_words: &["a", "an"],
    stat_forms: &stat_forms::EN,
    requires_line: Regex::new(
        r"^Requires: \s*(?:Level[^\d,]*(?P<level>\d+))?\D*(?:(?P<str>\d+)[^\d,]*(?:Strength|Str))?\D*(?:(?P<dex>\d+)[^\d,]*(?:Dexterity|Dex))?\D*(?:(?P<int>\d+)[^\d,]*(?:Intelligence|Int))?$",
    )
    .expect("EN requires_line regex"),
    unidentified: Regex::new(r"^Unidentified(?:\s*\(Tier\s*(?P<tier>\d+)\))?$")
        .expect("EN unidentified regex"),
    item_exceptional: Regex::new(r"^Exceptional (?P<rest>.*)$").expect("EN item_exceptional regex"),
    item_superior: Regex::new(r"^Superior (?P<rest>.*)$").expect("EN item_superior regex"),
    flask_charges: Regex::new(r"^Currently has \d+ Charges$").expect("EN flask_charges regex"),
    modifier_line: Regex::new(
        r#"^(?P<type>[^"]+)(?:\s+"(?P<name>[^"]*)")?(?:\s*\(Tier: (?P<tier>\d+)\))?(?:\s*\(Rank: (?P<rank>\d+)\))?$"#,
    )
    .expect("EN modifier_line regex"),
    waystone_tier_name: Regex::new(r"Waystone \(Tier (?P<tier>\d+)\)")
        .expect("EN waystone_tier_name regex"),
    map_blighted: Regex::new(r"^Blighted (?P<rest>.*)$").expect("EN map_blighted regex"),
    map_blight_ravaged: Regex::new(r"^Blight-ravaged (?P<rest>.*)$")
        .expect("EN map_blight_ravaged regex"),
}
});

pub static RU: LazyLock<ClientStrings> = LazyLock::new(|| {
    ClientStrings {
    item_class: "Класс предмета: ",
    rarity: "Редкость: ",
    rarity_normal: "Обычный",
    rarity_magic: "Волшебный",
    rarity_rare: "Редкий",
    rarity_unique: "Уникальный",
    rarity_gem: "Камень",
    rarity_currency: "Валюта",
    rarity_divcard: "Гадальная карта",
    rarity_quest: "Задание",
    item_level: "Уровень предмета: ",
    sockets: "Гнезда: ",
    quality: "Качество: ",
    armour: "Броня: ",
    evasion: "Уклонение: ",
    energy_shield: "Энергетический щит: ",
    runic_ward: "Рунический барьер: ",
    block_chance: "Шанс блока: ",
    physical_damage: "Физический урон: ",
    elemental_damage: "Урон от стихий: ",
    fire_damage: "Урон от огня: ",
    cold_damage: "Урон от холода: ",
    lightning_damage: "Урон от молнии: ",
    crit_chance: "Шанс крит. попадания: ",
    attack_speed: "Атак в секунду: ",
    reload_speed: "Время перезарядки: ",
    base_spirit: "Дух: ",
    gem_level: "Уровень: ",
    stack_size: "Размер стопки: ",
    corrupted: "Осквернено ",
    double_corrupted: "Дважды осквернено",
    unmodifiable: "Неизменяемый",
    mirrored: "Отражено",
    sanctified: "Освящено",
    fractured_item: "Расколотый предмет",
    price_note: "Примечание: ",
    waystone_tier: "Уровень путевого камня: ",
    revives: "Доступно возрождений: ",
    pack_size: &["Размер групп монстров: "],
    magic_monsters: "Волшебные монстры: ",
    rare_monsters: "Редкие монстры: ",
    waystone_drop_chance: "Шанс выпадения путевого камня: ",
    item_rarity: "Редкость предметов: ",
    monster_rarity: "Редкость монстров: ",
    monster_effectiveness: "Эффективность монстров: ",
    area_level: "Уровень области: ",
    trial_count: "Число испытаний: ",
    ultimatum_victorious: "Победный",
    ultimatum_cowardly: "Трусливый",
    ultimatum_deadly: "Смертельный",
    veiled_prefix: "Очернённый префикс",
    veiled_suffix: "Очернённый суффикс",
    prefix_modifier: "Префикс",
    suffix_modifier: "Суффикс",
    implicit_modifier: "Собственное свойство",
    enchant_modifier: "Улучшение",
    corrupted_modifier: "Улучшение от осквернения",
    crafted_modifier: "Ремесленное",
    fractured_modifier: "Расколотое",
    desecrated_modifier: "Очернённое",
    unique_modifier: "Уникальное свойство",
    vaal_unique_modifier: "Уникальное свойство ваал",
    grants_skill: "Дарует умение: ",
    cannot_use_item: "Вы не можете использовать этот предмет, его параметры не будут учтены",
    unscalable_value: " — Неизменяемое значение",
    negations: &[
        ("увеличение", "уменьшение"),
        ("повышение", "снижение"),
        ("усиление", "ослабление"),
        ("увеличенным", "уменьшенным"),
        ("больше", "меньше"),
        ("увеличенный", "уменьшенный"),
        ("увеличенное", "уменьшенное"),
        ("быстрее", "медленнее"),
    ],
    number_words: &[],
    stat_forms: &stat_forms::RU,
    requires_line: Regex::new(
        r"^Требуется: \s*(?:Уровень[^\d,]*(?P<level>\d+))?\D*(?:(?P<str>\d+)[^\d,]*(?:Сила|Сила))?\D*(?:(?P<dex>\d+)[^\d,]*(?:Ловкость|Ловк))?\D*(?:(?P<int>\d+)[^\d,]*(?:Интеллект|Инт))?$",
    )
    .expect("RU requires_line regex"),
    unidentified: Regex::new(r"^Неопознано(?:\s*\(Ранг\s*(?P<tier>\d+)\))?$")
        .expect("RU unidentified regex"),
    item_exceptional: Regex::new(
        r"^(?:Образцовый|Образцовая|Образцовое|Образцовые) (?P<rest>.*)$",
    )
    .expect("RU item_exceptional regex"),
    item_superior: Regex::new(r"^(?P<rest>.*) высокого качества$").expect("RU item_superior regex"),
    flask_charges: Regex::new(r"^Содержит зарядов: \d+$").expect("RU flask_charges regex"),
    modifier_line: Regex::new(
        r#"^(?P<type>[^"]+)(?:\s+"(?P<name>[^"]*)")?(?:\s*\(Уровень: (?P<tier>\d+)\))?(?:\s*\(Ранг: (?P<rank>\d+)\))?$"#,
    )
    .expect("RU modifier_line regex"),
    waystone_tier_name: Regex::new(r"Путевой камень \(Ур\. (?P<tier>\d+)\)")
        .expect("RU waystone_tier_name regex"),
    map_blighted: Regex::new(r"^Заражённая (?P<rest>.*)$").expect("RU map_blighted regex"),
    map_blight_ravaged: Regex::new(r"^Разорённая Скверной (?P<rest>.*)$")
        .expect("RU map_blight_ravaged regex"),
}
});
