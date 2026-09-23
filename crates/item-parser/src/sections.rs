//! Splits clipboard text into `--------`-delimited sections and folds in the "Cannot use this
//! item" special case. Port of `itemTextToSections` + the `CANNOT_USE_ITEM` merge at the top of
//! `parseClipboard` (`Parser.ts:189-249`, `.tmp/research/ItemTextFormat.md` section 1 points
//! 1-5), verified against the real code (not just the research doc's summary) this session.

use crate::client_strings::ClientStrings;

/// Splits `text` on the exact literal line `"--------"` (not a regex, not a prefix match --
/// equality after nothing more than the line-ending split itself), tolerating both `\n` and
/// `\r\n`, dropping one trailing empty line if present, and dropping any section left with zero
/// lines (guards against adjacent delimiters). Then, if the first section's 3rd line is the
/// language's `cannot_use_item` marker, pops that line, and merges the first section's remaining
/// lines onto the FRONT of the second section, dropping the first section entirely -- real PoE2
/// clipboard text for an item your current character can't use prepends this warning line
/// directly after the `Item Class:`/`Rarity:` lines, splitting what would otherwise be one
/// nameplate section into two.
pub fn split_sections(text: &str, cs: &ClientStrings) -> Vec<Vec<String>> {
    let mut lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }

    let mut sections: Vec<Vec<String>> = vec![Vec::new()];
    for line in lines {
        if line == "--------" {
            sections.push(Vec::new());
        } else {
            sections
                .last_mut()
                .expect("always at least one section")
                .push(line.to_string());
        }
    }
    sections.retain(|section| !section.is_empty());

    if sections.len() >= 2 && sections[0].get(2).is_some_and(|l| l == cs.cannot_use_item) {
        sections[0].pop();
        let mut merged = sections.remove(0);
        merged.append(&mut sections[0]);
        sections[0] = merged;
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_exact_delimiter_and_drops_trailing_blank() {
        let text = "Item Class: Helmets\nRarity: Normal\nSuperior Divine Crown\n--------\nQuality: +9% (augmented)\n--------\nItem Level: 81\n";
        let sections = split_sections(text, &crate::client_strings::EN);
        assert_eq!(sections.len(), 3);
        assert_eq!(
            sections[0],
            vec![
                "Item Class: Helmets",
                "Rarity: Normal",
                "Superior Divine Crown"
            ]
        );
        assert_eq!(sections[2], vec!["Item Level: 81"]);
    }

    #[test]
    fn merges_cannot_use_item_section_into_nameplate() {
        let text = "Item Class: Two Hand Maces\nRarity: Rare\nYou cannot use this item. Its stats will be ignored\n--------\nCrackling Temple Maul of the Brute\n--------\nItem Level: 32\n";
        let sections = split_sections(text, &crate::client_strings::EN);
        assert_eq!(sections.len(), 2);
        assert_eq!(
            sections[0],
            vec![
                "Item Class: Two Hand Maces",
                "Rarity: Rare",
                "Crackling Temple Maul of the Brute"
            ]
        );
        assert_eq!(sections[1], vec!["Item Level: 32"]);
    }
}
