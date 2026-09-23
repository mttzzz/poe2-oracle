//! Port of `parseNamePlate` (`Parser.ts:539-629`, `.tmp/research/ItemTextFormat.md` section 1
//! point 6). Consumes the nameplate section (already merged with the `CANNOT_USE_ITEM` special
//! case by `sections.rs` if applicable) into a `ParsedItem` skeleton: an optional `Item Class:`
//! line, a `Rarity:` line, a name line (always present), and an optional base-type line.
//!
//! Category resolution is this crate's own departure from the reference (see this crate's own
//! module doc comment in `categories.rs`): `Item Class:` text is looked up directly against
//! `categories::ITEM_CLASSES` for equipment-rarity items, rather than discarded in favor of a
//! database lookup by name.

use std::collections::VecDeque;

use poe2_domain::{ItemCategory, ItemRarity, ParsedItem};

use crate::client_strings::ClientStrings;
use crate::{ItemLanguage, ParseError, categories};

/// The English name of the class a vendor's gamble offers carry ("Random Helmet").
const HIDDEN_ITEMS: &str = "Hidden Items";

/// Port of `isItemMissingItemClass` (`Parser.ts:2245-2248`): a 2-3 line section whose first line
/// is a `Rarity:` line -- Meta Skill Gems omit `Item Class:` entirely.
fn is_missing_item_class(lines: &VecDeque<&str>, cs: &ClientStrings) -> bool {
    (2..=3).contains(&lines.len()) && lines.front().is_some_and(|l| l.starts_with(cs.rarity))
}

/// Strips the quality-tier wording the client adds to a quality item's name (`"Superior "`/EN,
/// `" высокого качества"`/RU-suffix; `"Exceptional "`/EN or its 4-way-gendered RU form) -- only
/// where the client adds it, EE2's `parseSuperior`/`parseExceptional`: the name of a Normal item
/// or of an unidentified one, which is its bare base type. Every other name is kept whole: a
/// currency's own name can open with the same word (`Exceptional Verisium` is an exchange item
/// of its own, not a quality Verisium). Runs after the property pass, which is what marks an item
/// unidentified.
pub(crate) fn normalize_name(item: &mut ParsedItem, cs: &ClientStrings) {
    let names_its_base =
        item.rarity == Some(ItemRarity::Normal) || (item.is_unidentified && item.rarity.is_some());
    if !names_its_base {
        return;
    }
    if let Some(caps) = cs.item_superior.captures(&item.name) {
        item.name = caps["rest"].to_string();
    } else if let Some(caps) = cs.item_exceptional.captures(&item.name) {
        item.name = caps["rest"].to_string();
    }
}

pub fn parse_nameplate(
    section: &[String],
    cs: &ClientStrings,
    language: ItemLanguage,
) -> Result<ParsedItem, ParseError> {
    let mut lines: VecDeque<&str> = section.iter().map(String::as_str).collect();

    let mut missing_item_class = false;
    let mut item_class_text: Option<&str> = None;

    match lines.pop_front() {
        Some(l) if l.starts_with(cs.item_class) => {
            item_class_text = Some(&l[cs.item_class.len()..]);
        }
        first => {
            if let Some(l) = first {
                lines.push_front(l);
            }
            if is_missing_item_class(&lines, cs) {
                missing_item_class = true;
            } else {
                return Err(ParseError::MissingNameplate);
            }
        }
    }

    if item_class_text.is_some_and(|text| {
        categories::find(language, text).is_some_and(|class| class.en == HIDDEN_ITEMS)
    }) {
        return Err(ParseError::Unrevealed);
    }

    let mut rarity_text: Option<&str> = None;
    let mut line = lines.pop_front();
    if let Some(l) = line
        && l.starts_with(cs.rarity)
    {
        rarity_text = Some(&l[cs.rarity.len()..]);
        line = lines.pop_front();
    }
    let Some(name) = line else {
        return Err(ParseError::MissingNameplate);
    };
    let base_type = lines.pop_front().map(str::to_string);

    let mut item = ParsedItem {
        name: name.to_string(),
        base_type,
        ..Default::default()
    };

    if let Some(t) = rarity_text {
        if t == cs.rarity_currency {
            item.category = Some(ItemCategory {
                id: "currency".to_string(),
                display_name: t.to_string(),
            });
        } else if t == cs.rarity_divcard {
            item.category = Some(ItemCategory {
                id: "card".to_string(),
                display_name: t.to_string(),
            });
        } else if t == cs.rarity_gem {
            item.category = Some(ItemCategory {
                id: "gem".to_string(),
                display_name: t.to_string(),
            });
        } else if t == cs.rarity_normal || t == cs.rarity_quest {
            item.rarity = Some(ItemRarity::Normal);
        } else if t == cs.rarity_magic {
            item.rarity = Some(ItemRarity::Magic);
        } else if t == cs.rarity_rare {
            item.rarity = Some(ItemRarity::Rare);
        } else if t == cs.rarity_unique {
            item.rarity = Some(ItemRarity::Unique);
        }
    }
    // Unconditional override, matching the reference's own post-switch `if (missingItemClass)`
    // (real-world: rarity_text is always "Gem" here anyway, so this never actually disagrees
    // with the switch above, but the override order is replicated faithfully regardless).
    if missing_item_class {
        item.category = Some(ItemCategory {
            id: "gem".to_string(),
            display_name: "Gem".to_string(),
        });
    }

    // Item Class -> trade category resolution: only ever consulted for equipment-rarity items and
    // quest items (Currency/DivinationCard/Gem already got `category` set directly above from the
    // `Rarity:` value itself, mirroring the reference's own `parseNamePlate` switch -- their `Item
    // Class:` text, even when present, is never looked up here, matching the Uncut-Gem fixtures'
    // `Item Class: Uncut Skill Gems` / `Rarity: Currency` -> `category = Currency`). A class the
    // trade site has no category for (a quest item, a Wombgift) parses with no category at all.
    if item.rarity.is_some()
        && let Some(class_text) = item_class_text
    {
        let class = categories::find(language, class_text)
            .ok_or_else(|| ParseError::UnrecognizedItemClass(class_text.to_string()))?;
        item.category = class.trade_id.map(|trade_id| ItemCategory {
            id: trade_id.to_string(),
            display_name: class_text.to_string(),
        });
    }

    Ok(item)
}
