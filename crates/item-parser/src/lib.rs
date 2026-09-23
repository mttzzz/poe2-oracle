//! Parses PoE2's Ctrl+C clipboard item-text format (the plain-text block the game writes to the
//! clipboard when a player copies an item) into `poe2_domain::ParsedItem`. Depends only on
//! `poe2-domain` for the output shape plus `regex` for the client's own line grammars.
//!
//! Architecture: `sections::split_sections` -> `nameplate::parse_nameplate` (consumes the first
//! section) -> an ordered pass of `properties::PARSERS` (each claims at most one remaining
//! section per call) -> a catch-all pass of `modifiers::parse_modifier_section` over whatever
//! sections still look like a modifier block. See each module's own doc comment for the real,
//! working reference behavior it ports (`.tmp/research/ItemTextFormat.md`,
//! `StatFilterBuilding.md`, `StatIdMapping.md`).

pub mod catalog_match;
pub mod categories;
pub mod client_strings;
pub mod modifiers;
pub mod nameplate;
pub mod properties;
pub mod roll;
pub mod sections;
pub mod stat_forms;

use poe2_domain::{ModifierType, ParsedItem, StatCatalog};

/// A display/parse language for clipboard item text: the two game client languages this project
/// supports. The trade API knows more, but nothing here needs them yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemLanguage {
    English,
    Russian,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// The clipboard text was empty (or whitespace-only).
    Empty,
    /// Text was present but matched neither language's `Item Class:`/`Rarity:` markers -- not
    /// PoE item text at all (or a language this crate doesn't support).
    UnknownLanguage,
    /// Text matched a *different* supported language's markers than the one requested.
    WrongLanguage {
        found: ItemLanguage,
        wanted: ItemLanguage,
    },
    /// Markers matched the requested language, but the nameplate section itself was malformed
    /// (no name line at all -- never seen in real client output, a defensive-only case).
    MissingNameplate,
    /// A real, well-formed `Item Class:` value with no entry in `categories.rs`'s table for this
    /// language -- see that module's own doc comment for why this is treated as a gap to fill,
    /// never guessed at.
    UnrecognizedItemClass(String),
    /// A vendor's gamble offer (`Item Class: Hidden Items`, "Random Helmet" at the gamblers): the
    /// item it turns into is revealed only once bought, so there is nothing to price, and the
    /// trade site lists none.
    Unrevealed,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Empty => write!(f, "clipboard text is empty"),
            ParseError::UnknownLanguage => write!(f, "text does not look like a PoE2 item"),
            ParseError::WrongLanguage { found, wanted } => {
                write!(f, "item text is in {found:?}, expected {wanted:?}")
            }
            ParseError::MissingNameplate => write!(f, "item text has no name line"),
            ParseError::UnrecognizedItemClass(text) => {
                write!(f, "unrecognized Item Class: {text:?}")
            }
            ParseError::Unrevealed => write!(f, "a gamble offer, revealed only once bought"),
        }
    }
}

impl std::error::Error for ParseError {}

fn client_strings_for(language: ItemLanguage) -> &'static client_strings::ClientStrings {
    match language {
        ItemLanguage::English => &client_strings::EN,
        ItemLanguage::Russian => &client_strings::RU,
    }
}

/// Cheap language sniff against the first line of clipboard text: does it start with either
/// language's `Item Class:` or `Rarity:` marker (the only two markers a nameplate's first line
/// can ever be, per `nameplate.rs`)?
fn detect_language(first_line: &str) -> Option<ItemLanguage> {
    let en = &client_strings::EN;
    let ru = &client_strings::RU;
    if first_line.starts_with(en.item_class) || first_line.starts_with(en.rarity) {
        Some(ItemLanguage::English)
    } else if first_line.starts_with(ru.item_class) || first_line.starts_with(ru.rarity) {
        Some(ItemLanguage::Russian)
    } else {
        None
    }
}

/// Which supported language `text` is in, by its first line's `Item Class:`/`Rarity:` marker --
/// `None` for non-item text and for game languages this crate can't parse. Callers use it to pick
/// the matching stat catalog and trade site before calling [`parse_clipboard`].
pub fn detect_item_language(text: &str) -> Option<ItemLanguage> {
    detect_language(text.lines().next().unwrap_or(""))
}

/// First-line markers of item text for every game client language, ported verbatim from EE2's
/// `LANGUAGE_DETECTOR` (`main/src/shortcuts/HostClipboard.ts`): `(Item Class line, Rarity line)`.
/// Wider than the two languages [`parse_clipboard`] supports on purpose -- an item copied from a
/// German client must still be recognized as an item, so the app reports "unsupported language"
/// instead of silently timing out on the clipboard poll.
const ITEM_TEXT_MARKERS: &[(&str, &str)] = &[
    ("Item Class: ", "Rarity: "),
    ("Класс предмета: ", "Редкость: "),
    ("Classe d'objet: ", "Rareté: "),
    ("Gegenstandsklasse: ", "Seltenheit: "),
    ("Classe do Item: ", "Raridade: "),
    ("Clase de objeto: ", "Rareza: "),
    ("ชนิดไอเทม: ", "Rarity: "),
    ("아이템 종류: ", "아이템 희귀도: "),
    ("物品種類: ", "稀有度: "),
    ("物品类别: ", "Rarity: "),
    ("アイテムクラス: ", "レアリティ: "),
];

/// Whether clipboard text is PoE item text in any game language (EE2's `isPoeItem`). This is the
/// clipboard poll's "the game answered the copy combo" signal, so it must not be narrower than the
/// languages players actually run the client in.
pub fn looks_like_item_text(text: &str) -> bool {
    ITEM_TEXT_MARKERS
        .iter()
        .any(|(item_class, rarity)| text.starts_with(item_class) || text.starts_with(rarity))
}

/// Parses real PoE2 Ctrl+C clipboard text into a `ParsedItem`. `catalog` is the live trade-API
/// stat catalog (`trade_client::catalog::fetch_stat_catalog`) -- every numeric mod stat is
/// resolved against it; a stat with no catalog match still parses (kept with `stat_id: None`,
/// surfaced via `ParsedItem::unknown_mods`), it just never fails the whole parse.
pub fn parse_clipboard(
    text: &str,
    language: ItemLanguage,
    catalog: &StatCatalog,
) -> Result<ParsedItem, ParseError> {
    if text.trim().is_empty() {
        return Err(ParseError::Empty);
    }

    let cs = client_strings_for(language);
    let mut sections = sections::split_sections(text, cs);
    if sections.is_empty() {
        return Err(ParseError::Empty);
    }

    let first_line = sections[0].first().map(String::as_str).unwrap_or("");
    match detect_language(first_line) {
        None => return Err(ParseError::UnknownLanguage),
        Some(found) if found != language => {
            return Err(ParseError::WrongLanguage {
                found,
                wanted: language,
            });
        }
        _ => {}
    }

    let nameplate_section = sections.remove(0);
    let mut item = nameplate::parse_nameplate(&nameplate_section, cs, language)?;
    item.raw_text = text.to_string();

    let index = catalog_match::CatalogIndex::build(&catalog.stats);

    // Ordered property parsers: each call claims at most one remaining section, in the order
    // given by `properties::PARSERS` -- later parsers only ever see whatever earlier ones left.
    for parser in properties::PARSERS {
        if let Some(pos) = sections.iter().position(|section| {
            parser(section, &mut item, cs, &index) == properties::SectionResult::Parsed
        }) {
            sections.remove(pos);
        }
    }
    nameplate::normalize_name(&mut item, cs);
    properties::derive_waystone_name_fields(&mut item, cs);

    // Catch-all: whatever sections remain that look like a modifier block (bracket or flat-
    // suffix form) get claimed here, one at a time; anything left after this (e.g. a Unique
    // item's flavor-text footer) is inert leftover, silently dropped.
    let mut i = 0;
    while i < sections.len() {
        if modifiers::section_is_modifier_block(&sections[i]) {
            let section = sections.remove(i);
            modifiers::parse_modifier_section(&section, &mut item, cs, &index);
        } else {
            i += 1;
        }
    }
    properties::count_empty_rune_sockets(&mut item);

    // `is_fractured` is derived from whether any MOD resolved to `Fractured`, never from the
    // item-level "Fractured Item" footer alone (the reference pins this exact distinction with
    // its `FracturedItem`/`FracturedItemNoModMarked` fixture pair).
    item.is_fractured = item
        .mods
        .iter()
        .any(|m| m.info.modifier_type == ModifierType::Fractured);

    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_client_item_text_is_recognized_and_detected_as_russian() {
        // Regression test: the clipboard poll used to accept English markers only, so every item
        // copied from a Russian client timed out silently -- Ctrl+E appeared to do nothing.
        let text = "Класс предмета: Обувь\nРедкость: Волшебный\nЗдоровые сапоги\n--------\n";
        assert!(looks_like_item_text(text));
        assert_eq!(detect_item_language(text), Some(ItemLanguage::Russian));
    }

    #[test]
    fn unsupported_client_language_is_still_item_text_but_has_no_parse_language() {
        let text = "Gegenstandsklasse: Stiefel\nSeltenheit: Magisch\n";
        assert!(looks_like_item_text(text));
        assert_eq!(detect_item_language(text), None);
    }

    #[test]
    fn arbitrary_clipboard_text_is_not_item_text() {
        assert!(!looks_like_item_text(""));
        assert!(!looks_like_item_text("hunter2"));
        assert!(!looks_like_item_text("see Item Class: Boots"));
    }
}
