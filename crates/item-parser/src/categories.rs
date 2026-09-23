//! `Item Class:` clipboard text -> trade-API category id. This project ships no local item
//! database (see the plan's Context section), so unlike the real reference -- which discards the
//! `Item Class:` line's text entirely and resolves `category` via a database lookup keyed by the
//! item's *name* (`Parser.ts:539-629`'s `parseNamePlate` only checks the line's *prefix* to skip
//! it, see `.tmp/research/ItemTextFormat.md`) -- this crate resolves `category` directly from
//! the `Item Class:` line's own value text, the one category signal real clipboard text actually
//! carries without a database.
//!
//! Only ever consulted for items whose `Rarity:` is Normal/Magic/Rare/Unique/Quest (`nameplate.rs`
//! resolves Gem/Currency/DivinationCard rarity directly from the `Rarity:` value itself, mirroring
//! the reference's own `parseNamePlate` switch -- those items' `Item Class:` text, even when
//! present, is never consulted for category, e.g. the Uncut-Gem fixtures show `Item Class: Uncut
//! Skill Gems` / `Rarity: Currency`, and category comes from `Currency`).
//!
//! [`ITEM_CLASSES`] holds every class the game names: all 93 rows of the live client's
//! `Data/Balance/ItemClasses.datc64` (EN) and `Data/Balance/Russian/ItemClasses.datc64` (RU) with a
//! non-empty `Name`, extracted 2026-09-22 with this workspace's `data-pipeline` and paired by the
//! table's language-independent `Id` (`tests/fixtures/itemclasses.tsv` is that dump, and a test
//! holds this table to it). Rows sharing both names are listed once. The names are the client's,
//! not the trade site's category labels, which differ (trade `Сапоги`/`Нательная броня` vs game
//! `Обувь`/`Нательные доспехи`; English `Quarterstaves`, whose `Id` is still `Warstaff`). An
//! `Item Class:` value missing here is still NOT guessed at -- `nameplate.rs` surfaces
//! `ParseError::UnrecognizedItemClass`, the signal that a patch added a class.
//!
//! Trade ids are the category options of the live `GET /api/trade2/data/filters` (2026-09-22).
//! Classes the trade site has no category for map to `None`: quest items, the PoE1 league
//! classes the PoE2 client still carries (Heist, Delve, Sentinel, Incubators, ...), and PoE2's
//! Wombgifts and Transcendent limbs -- `trade-client` prices those by base type instead. A few
//! classes map to the closest category the trade site does list, each checked against where the
//! live `/data/items` and `/data/static` catalogs file the class's items: every relic size to
//! `sanctum.relic`, Vault Keys (Reliquary Keys, which `/data/items` files under Currency) and the
//! Uncut Gem classes to `currency`, `Augment` (runes and soul cores) to `currency.socketable`,
//! Trial Coins (Barya) to `map.barya`.

use crate::ItemLanguage;

/// One class of the game's `ItemClasses` table: its `Name` in each supported client language, and
/// the trade category its items are listed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemClass {
    pub en: &'static str,
    pub ru: &'static str,
    /// A `/api/trade2/data/filters` `category` option id; `None` where the trade site has none.
    pub trade_id: Option<&'static str>,
}

impl ItemClass {
    /// The class's `Name` as `language`'s client prints it after `Item Class: `.
    pub fn name(&self, language: ItemLanguage) -> &'static str {
        match language {
            ItemLanguage::English => self.en,
            ItemLanguage::Russian => self.ru,
        }
    }
}

const fn class(en: &'static str, ru: &'static str, trade_id: Option<&'static str>) -> ItemClass {
    ItemClass { en, ru, trade_id }
}

/// Every named `ItemClasses` row, in the table's own order.
pub static ITEM_CLASSES: &[ItemClass] = &[
    class("Life Flasks", "Флаконы жизни", Some("flask.life")),
    class("Mana Flasks", "Флаконы маны", Some("flask.mana")),
    // `Currency` and `SkillGemToken`.
    class("Currency", "Вещи на обмен", Some("currency")),
    class("Amulets", "Амулеты", Some("accessory.amulet")),
    class("Rings", "Кольца", Some("accessory.ring")),
    class("Claws", "Когти", Some("weapon.claw")),
    class("Daggers", "Кинжалы", Some("weapon.dagger")),
    class("Wands", "Жезлы", Some("weapon.wand")),
    class(
        "One Hand Swords",
        "Одноручные мечи",
        Some("weapon.onesword"),
    ),
    class("One Hand Axes", "Одноручные топоры", Some("weapon.oneaxe")),
    class(
        "One Hand Maces",
        "Одноручные булавы",
        Some("weapon.onemace"),
    ),
    class("Bows", "Луки", Some("weapon.bow")),
    class("Staves", "Посохи", Some("weapon.staff")),
    class("Two Hand Swords", "Двуручные мечи", Some("weapon.twosword")),
    class("Two Hand Axes", "Двуручные топоры", Some("weapon.twoaxe")),
    class("Two Hand Maces", "Двуручные булавы", Some("weapon.twomace")),
    class("Skill Gems", "Камни умений", Some("gem.activegem")),
    class("Support Gems", "Камни поддержки", Some("gem.supportgem")),
    class("Quivers", "Колчаны", Some("armour.quiver")),
    class("Belts", "Пояса", Some("accessory.belt")),
    class("Gloves", "Перчатки", Some("armour.gloves")),
    class("Boots", "Обувь", Some("armour.boots")),
    class("Body Armours", "Нательные доспехи", Some("armour.chest")),
    class("Helmets", "Шлемы", Some("armour.helmet")),
    class("Shields", "Щиты", Some("armour.shield")),
    class("Small Relics", "Малые реликвии", Some("sanctum.relic")),
    class("Medium Relics", "Средние реликвии", Some("sanctum.relic")),
    class("Large Relics", "Большие реликвии", Some("sanctum.relic")),
    class("Stackable Currency", "Валюта", Some("currency")),
    class("Quest Items", "Вещи для заданий", None),
    class("Sceptres", "Скипетры", Some("weapon.sceptre")),
    class("Charms", "Обереги", Some("flask.charm")),
    class("Waystones", "Путевые камни", Some("map.waystone")),
    class("Fishing Rods", "Удочки", Some("weapon.rod")),
    // `MapFragment` and `AtlasCurrency`.
    class("Map Fragments", "Обрывки карт", Some("map.fragment")),
    class("Hideout Doodads", "Предметы убежища", None),
    class("Microtransactions", "Микротранзакции", None),
    class("Jewels", "Самоцветы", Some("jewel")),
    class("Divination Cards", "Гадальные карты", Some("card")),
    class("Misc Map Items", "Прочие предметы карт", None),
    class("Leaguestones", "Камни лиги", None),
    class("Pantheon Souls", "Души Пантеона", None),
    class("Pieces", "Фрагменты", None),
    class("Abyss Jewels", "Самоцветы Бездны", Some("jewel")),
    class("Incursion Items", "Предметы Вмешательства", None),
    class("Delve Socketable Currency", "Валюта Спуска", None),
    // `Incubator` and `IncubatorStackable`.
    class("Incubators", "Инкубаторы", None),
    class("Shards", "Осколки", None),
    class("Shard Hearts", "Стержни осколков", None),
    class("Quarterstaves", "Боевые посохи", Some("weapon.warstaff")),
    class("Delve Stackable Socketable Currency", "Валюта Спуска", None),
    class("Atlas Upgrade Items", "Предметы улучшения Атласа", None),
    class("Hidden Items", "Сокрытые предметы", None),
    class("Contracts", "Контракты", None),
    class("Heist Gear", "Разбойничьи принадлежности", None),
    class("Heist Tools", "Разбойничий инструмент", None),
    class("Heist Cloaks", "Разбойничьи накидки", None),
    class("Heist Brooches", "Разбойничьи броши", None),
    class("Blueprints", "Чертежи", None),
    class("Trinkets", "Украшения", None),
    class("Heist Targets", "Предметы кражи", None),
    // `ExpeditionLogbook` (legacy) and PoE2's own `Expedition2Logbooks`.
    class(
        "Expedition Logbooks",
        "Журналы экспедиции",
        Some("map.logbook"),
    ),
    class(
        "Expedition Logbook",
        "Журнал экспедиции",
        Some("map.logbook"),
    ),
    class("Archnemesis Mods", "Свойства Возмездия", None),
    class("Spears", "Копья", Some("weapon.spear")),
    class("Crossbows", "Самострелы", Some("weapon.crossbow")),
    class("Foci", "Фокусы", Some("armour.focus")),
    class("Instance Local Items", "Местные предметы области", None),
    class("Sentinels", "Часовые", None),
    class("Memories", "Воспоминания", None),
    class("Flails", "Кистени", Some("weapon.flail")),
    class("Relics", "Реликвии", Some("sanctum.relic")),
    class(
        "Sanctified Relics",
        "Священные реликвии",
        Some("sanctum.relic"),
    ),
    class("Breachstones", "Камни Разлома", Some("map.breachstone")),
    class("Vault Keys", "Ключи от хранилищ", Some("currency")),
    class("Trial Coins", "Монеты Испытания", Some("map.barya")),
    class("Bucklers", "Баклеры", Some("armour.buckler")),
    class("Traps", "Ловушки", None),
    class(
        "Inscribed Ultimatum",
        "Начертанные Ультиматумы",
        Some("map.ultimatum"),
    ),
    class("Augment", "Усилители", Some("currency.socketable")),
    class("Tablet", "Плитки", Some("map.tablet")),
    class("Omen", "Предзнаменования", Some("currency.omen")),
    class("Talismans", "Талисманы", Some("weapon.talisman")),
    class(
        "Uncut Skill Gems",
        "Неогранённые камни умений",
        Some("currency"),
    ),
    class(
        "Uncut Support Gems",
        "Неогранённые камни поддержки",
        Some("currency"),
    ),
    class(
        "Uncut Spirit Gems",
        "Неогранённые камни духа",
        Some("currency"),
    ),
    class("Transcendent Arm", "Вознесённая рука", None),
    class("Transcendent Leg", "Вознесённая нога", None),
    class("Wombgifts", "Дары утробы", None),
    class("Pinnacle Keys", "Верховные ключи", Some("map.bosskey")),
];

/// The class an `Item Class:` value names in `language`'s client.
pub fn find(language: ItemLanguage, name: &str) -> Option<&'static ItemClass> {
    ITEM_CLASSES
        .iter()
        .find(|class| class.name(language) == name)
}
