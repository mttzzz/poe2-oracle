//! The fixed texts the game writes into a copied item, in each client language, as the client's
//! own `ClientStrings` table has them (`Data/Balance/ClientStrings.datc64` and its Russian twin,
//! dumped from the live client on 2026-09-23; each field names its row's `Id`). They are kept here
//! rather than taken from `item_parser::client_strings`: a text the parser expects wrongly would
//! then be written the same wrong way, and the round trip would pass without proving anything.

use item_parser::ItemLanguage;

pub(crate) struct Texts {
    /// `ItemDisplayStringClass` + `: `.
    pub item_class: &'static str,
    /// `ItemDisplayStringRarity` + `: `.
    pub rarity: &'static str,
    /// `ItemDisplayStringNormal`, `...Magic`, `...Rare`, `...Unique`, `...Gem`, `...Currency`,
    /// `...DivinationCard`: by GGG's `frameType`, 0 to 6.
    pub frames: [&'static str; 7],
    /// `ItemRequirementsLabel`.
    pub requires: &'static str,
    /// `ItemDisplayStringSockets` + `: `.
    pub sockets: &'static str,
    /// `ItemDisplayStringItemLevel` + `: `.
    pub item_level: &'static str,
    /// `ItemDisplayStringNote` + `: `.
    pub note: &'static str,
    /// `ItemPopupUnidentified`.
    pub unidentified: &'static str,
    /// `Tier`: an unidentified item's tier, `Unidentified (Tier 4)`.
    pub tier_word: &'static str,
    /// `ItemPopupCorrupted`, link markup stripped.
    pub corrupted: &'static str,
    /// `ItemPopupDoubleCorrupted`, link markup stripped.
    pub twice_corrupted: &'static str,
    /// `ItemPopupMirrored`.
    pub mirrored: &'static str,
    /// `ItemPopupSanctified`.
    pub sanctified: &'static str,
    /// `ItemPopupFracturedItem`.
    pub fractured_item: &'static str,
    /// `ModDescriptionLineImplicit`.
    pub implicit: &'static str,
    /// `ModDescriptionLineEnchantmentImplicit`.
    pub enchantment: &'static str,
    /// `ModDescriptionLineCorruptedImplicit`.
    pub corruption_enchantment: &'static str,
    /// `ModDescriptionLineUnique`.
    pub unique: &'static str,
    /// `ModDescriptionLineVaalUnique`.
    pub vaal_unique: &'static str,
    /// `ModDescriptionLinePrefix` (`Prefix Modifier "{0}"`), around the name.
    pub prefix: (&'static str, &'static str),
    /// `ModDescriptionLineSuffix`, around the name.
    pub suffix: (&'static str, &'static str),
    /// `ModDescriptionLineCrafted` (`Crafted {0}`), before the prefix or suffix.
    pub crafted: &'static str,
    /// `ModDescriptionLineFractured`, before the prefix or suffix.
    pub fractured: &'static str,
    /// `ModDescriptionLineDesecrated`, before the prefix or suffix.
    pub desecrated: &'static str,
    /// `ModDescriptionLineTier` (` (Tier: {0})`), around the tier.
    pub tier: (&'static str, &'static str),
}

pub(crate) static EN: Texts = Texts {
    item_class: "Item Class: ",
    rarity: "Rarity: ",
    frames: [
        "Normal",
        "Magic",
        "Rare",
        "Unique",
        "Gem",
        "Currency",
        "Divination Card",
    ],
    requires: "Requires: ",
    sockets: "Sockets: ",
    item_level: "Item Level: ",
    note: "Note: ",
    unidentified: "Unidentified",
    tier_word: "Tier",
    corrupted: "Corrupted",
    twice_corrupted: "Twice Corrupted",
    mirrored: "Mirrored",
    sanctified: "Sanctified",
    fractured_item: "Fractured Item",
    implicit: "Implicit Modifier",
    enchantment: "Enhancement",
    corruption_enchantment: "Corruption Enhancement",
    unique: "Unique Modifier",
    vaal_unique: "Vaal Unique Modifier",
    prefix: ("Prefix Modifier \"", "\""),
    suffix: ("Suffix Modifier \"", "\""),
    crafted: "Crafted ",
    fractured: "Fractured ",
    desecrated: "Desecrated ",
    tier: (" (Tier: ", ")"),
};

pub(crate) static RU: Texts = Texts {
    item_class: "Класс предмета: ",
    rarity: "Редкость: ",
    frames: [
        "Обычный",
        "Волшебный",
        "Редкий",
        "Уникальный",
        "Камень",
        "Валюта",
        "Гадальная карта",
    ],
    requires: "Требуется: ",
    sockets: "Гнезда: ",
    item_level: "Уровень предмета: ",
    note: "Примечание: ",
    unidentified: "Неопознано",
    tier_word: "Ранг",
    // The link's own text is `Осквернено ` with a trailing space, which a line of its own doesn't
    // keep (a player's copy, `ru_report_rukopolozhennye.txt`).
    corrupted: "Осквернено",
    twice_corrupted: "Дважды осквернено",
    mirrored: "Отражено",
    sanctified: "Освящено",
    fractured_item: "Расколотый предмет",
    implicit: "Собственное свойство",
    enchantment: "Улучшение",
    corruption_enchantment: "Улучшение от осквернения",
    unique: "Уникальное свойство",
    vaal_unique: "Уникальное свойство ваал",
    prefix: ("Префикс \"", "\""),
    suffix: ("Суффикс \"", "\""),
    crafted: "Ремесленное ",
    fractured: "Расколотое ",
    desecrated: "Очернённое ",
    tier: (" (Уровень: ", ")"),
};

pub(crate) fn of(language: ItemLanguage) -> &'static Texts {
    match language {
        ItemLanguage::English => &EN,
        ItemLanguage::Russian => &RU,
    }
}
