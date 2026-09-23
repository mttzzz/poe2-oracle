//! Bracket vs flat modifier-line grammar, ported from `advanced-mod-desc.ts` +
//! `Parser.ts:parseModifiers` (`.tmp/research/ItemTextFormat.md` section 4). Every regex-vs-real-
//! text behavior below (notably the `MODIFIER_LINE` capture boundaries) was empirically verified
//! against the real V8 regex engine this session, not just read from source -- see the worked
//! examples in this module's tests.
//!
//! One deliberate fidelity note: the flat-form suffix markers below (`" (rune)"` etc) are
//! hardcoded English literals in the real reference too (`advanced-mod-desc.ts`'s own
//! module-level consts, e.g. `export const AUGMENT_LINE = " (rune)"`) -- NOT sourced from its
//! per-locale `client_strings.js`, confirmed by reading that file directly. The PoE2 client
//! appears to always emit these specific suffix tags in English regardless of display language;
//! they are therefore plain constants here, not part of `client_strings.rs`'s per-language
//! tables.
//!
//! Known, deliberate scope trim (no fixture exercises it): Eldritch-rank implicit brackets
//! (`"Eater of Worlds Implicit Modifier (Lesser)"`, a PoE1 mechanic) parse as a modifier (their
//! stats still resolve normally) but fall through to whatever the block's flat-suffix-derived
//! default type is (usually `Explicit`) rather than being forced to `Implicit` with a numeric
//! rank -- the upstream regex's own capture-boundary behavior (verified empirically, see tests)
//! means an un-quoted bracket with no `"name"` lets `[^"]+` swallow any trailing `(...)` text
//! whole, so this is the *reference's own* real behavior for an un-quoted Eldritch bracket too,
//! not a gap introduced by skipping `EATER_IMPLICIT`/`EXARCH_IMPLICIT`.

use poe2_domain::{ModGeneration, ModifierInfo, ModifierType, ParsedItem, ParsedModifier};

use crate::catalog_match::{self, CatalogIndex};
use crate::client_strings::ClientStrings;

const SCOURGE_SUFFIX: &str = " (scourge)";
const AUGMENT_SUFFIX: &str = " (rune)";
const ADDED_AUGMENT_SUFFIX: &str = " (added rune)";
const ENCHANT_SUFFIX: &str = " (enchant)";
const IMPLICIT_SUFFIX: &str = " (implicit)";
const FRACTURED_SUFFIX: &str = " (fractured)";
const CRAFTED_SUFFIX: &str = " (crafted)";
const DESECRATED_SUFFIX: &str = " (desecrated)";

/// Priority-ordered suffix table -- matches `parseModType`'s if-else-if chain exactly (scourge,
/// enchant, implicit, fractured, crafted, rune, added-rune, desecrated; `Explicit` is the
/// fallback when nothing matches).
const SUFFIX_TABLE: &[(&str, ModifierType)] = &[
    (SCOURGE_SUFFIX, ModifierType::Scourge),
    (ENCHANT_SUFFIX, ModifierType::Enchant),
    (IMPLICIT_SUFFIX, ModifierType::Implicit),
    (FRACTURED_SUFFIX, ModifierType::Fractured),
    (CRAFTED_SUFFIX, ModifierType::Crafted),
    (AUGMENT_SUFFIX, ModifierType::Augment),
    (ADDED_AUGMENT_SUFFIX, ModifierType::AddedAugment),
    (DESECRATED_SUFFIX, ModifierType::Desecrated),
];

fn is_mod_info_line(line: &str) -> bool {
    line.len() >= 2 && line.starts_with('{') && line.ends_with('}')
}

/// True if `section` contains at least one line this module claims (a bracket line, or a line
/// ending in one of the flat suffixes above). Used by `lib.rs`'s catch-all pass over whatever
/// sections `properties.rs`'s own ordered parsers left unclaimed.
pub fn section_is_modifier_block(section: &[String]) -> bool {
    section.iter().any(|line| {
        is_mod_info_line(line)
            || SUFFIX_TABLE
                .iter()
                .any(|(suffix, _)| line.ends_with(suffix))
    })
}

/// Strips the winning suffix (if any) from every line that has it, matching `removeLinesEnding`
/// (which strips from *every* matching line in the block, not just the one that triggered
/// detection -- confirmed real behavior, e.g. `ItemAllTheModifierTypes`' two independently
/// `(rune)`-suffixed lines both get stripped for the one resulting `Augment` mod).
fn strip_flat_suffix(lines: &[&str]) -> (ModifierType, Vec<String>) {
    for (suffix, modifier_type) in SUFFIX_TABLE {
        if lines.iter().any(|l| l.ends_with(suffix)) {
            let stripped = lines
                .iter()
                .map(|l| l.strip_suffix(suffix).unwrap_or(l).to_string())
                .collect();
            return (*modifier_type, stripped);
        }
    }
    (
        ModifierType::Explicit,
        lines.iter().map(|s| s.to_string()).collect(),
    )
}

fn is_sanctum_relic(item: &ParsedItem) -> bool {
    item.category
        .as_ref()
        .is_some_and(|c| c.id == "sanctum.relic")
}

fn resolve_and_push(
    item: &mut ParsedItem,
    info: ModifierInfo,
    stat_lines: &[String],
    cs: &ClientStrings,
    index: &CatalogIndex,
) {
    // An unrevealed desecrated affix prints a placeholder line in place of its stats (EE2's
    // `parseModType` `VEILED_PREFIX`/`VEILED_SUFFIX`): nothing to resolve, and nothing unknown.
    if stat_lines
        .first()
        .is_some_and(|line| line == cs.veiled_prefix || line == cs.veiled_suffix)
    {
        item.is_veiled = true;
        item.mods.push(ParsedModifier {
            info: ModifierInfo {
                modifier_type: ModifierType::Veiled,
                ..info
            },
            stats: Vec::new(),
        });
        return;
    }
    let lines: Vec<&str> = stat_lines.iter().map(String::as_str).collect();
    let category = item.category.as_ref().map(|category| category.id.as_str());
    let stats = catalog_match::resolve_stat_lines(&lines, info.modifier_type, category, cs, index);
    if !stats.is_empty() && stats.iter().all(|s| s.stat_id.is_none()) {
        item.unknown_mods
            .push((stat_lines.join(" / "), info.modifier_type));
    }
    item.mods.push(ParsedModifier { info, stats });
}

fn parse_bracket_form(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    index: &CatalogIndex,
) {
    // Group lines into (bracket_line, stat_lines) blocks; blank lines between blocks are
    // tolerated and dropped entirely (never treated as stat lines) -- confirmed real behavior,
    // `FracturedItem`/`FracturedItemNoModMarked` fixtures have a literal blank line between
    // every bracket block.
    let mut blocks: Vec<(&str, Vec<&str>)> = Vec::new();
    for line in section {
        let line = line.as_str();
        if line.is_empty() {
            continue;
        }
        if is_mod_info_line(line) {
            blocks.push((line, Vec::new()));
        } else if let Some(last) = blocks.last_mut() {
            last.1.push(line);
        }
    }

    for (mod_line, stat_lines) in blocks {
        let (default_type, stripped_lines) = strip_flat_suffix(&stat_lines);

        let inner = mod_line[1..mod_line.len() - 1].trim();
        let mut parts = inner.split('\u{2014}').map(str::trim);
        let mod_text = parts.next().unwrap_or("");
        let tags_text = parts.next();

        let (info, resolved_lines) = match cs.modifier_line.captures(mod_text) {
            Some(caps) => {
                let mut type_str = caps
                    .name("type")
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default();
                let name = caps
                    .name("name")
                    .map(|m| m.as_str().to_string())
                    .filter(|s| !s.is_empty());
                let tier = caps.name("tier").and_then(|m| m.as_str().parse().ok());
                let rank = caps.name("rank").and_then(|m| m.as_str().parse().ok());

                let mut modifier_type = default_type;
                if let Some(rest) = type_str.strip_prefix(cs.fractured_modifier) {
                    type_str = rest.trim().to_string();
                    modifier_type = ModifierType::Fractured;
                }
                if let Some(rest) = type_str.strip_prefix(cs.desecrated_modifier) {
                    type_str = rest.trim().to_string();
                    if modifier_type != ModifierType::Fractured {
                        modifier_type = ModifierType::Desecrated;
                    }
                } else if let Some(rest) = type_str.strip_prefix(cs.crafted_modifier) {
                    type_str = rest.trim().to_string();
                    if modifier_type != ModifierType::Fractured {
                        modifier_type = ModifierType::Crafted;
                    }
                }

                // Prefix/Suffix bracket types set only the affix slot ("generation", EE2's
                // `ModifierInfo.generation`), never `type` itself -- `modifier_type` stays whatever
                // the flat-suffix-derived default (or the fractured/desecrated/crafted override
                // above) already determined. Compared case-insensitively: after a stripped
                // "Fractured"/"Desecrated" word the Russian client continues in lower case.
                let generation = if type_str.to_lowercase() == cs.prefix_modifier.to_lowercase() {
                    Some(ModGeneration::Prefix)
                } else if type_str.to_lowercase() == cs.suffix_modifier.to_lowercase() {
                    Some(ModGeneration::Suffix)
                } else {
                    None
                };
                if type_str == cs.implicit_modifier {
                    modifier_type = ModifierType::Implicit;
                } else if type_str == cs.enchant_modifier {
                    modifier_type = ModifierType::Enchant;
                } else if type_str == cs.corrupted_modifier {
                    // Bracket-form "Corruption Enhancement" maps to Enchant, NOT Scourge --
                    // matches the reference exactly (`generation: "corrupted"` is a flavor of
                    // Enchant there; `Scourge` is only ever reached via the flat `" (scourge)"`
                    // suffix path).
                    modifier_type = ModifierType::Enchant;
                }

                if is_sanctum_relic(item) && modifier_type == ModifierType::Explicit {
                    modifier_type = ModifierType::Sanctum;
                }

                let tags = tags_text
                    .map(|t| t.split(", ").map(str::to_string).collect())
                    .unwrap_or_default();

                (
                    ModifierInfo {
                        modifier_type,
                        generation,
                        name,
                        tier,
                        rank,
                        tags,
                    },
                    stripped_lines,
                )
            }
            None => (
                // Malformed bracket text never occurs in real clipboard output (the regex's
                // `type` group matches any non-quote text, which every real bracket satisfies);
                // this fallback exists only so a genuinely unexpected input still surfaces its
                // stats as an unresolved modifier instead of silently disappearing.
                ModifierInfo {
                    modifier_type: default_type,
                    generation: None,
                    name: None,
                    tier: None,
                    rank: None,
                    tags: Vec::new(),
                },
                stripped_lines,
            ),
        };

        resolve_and_push(item, info, &resolved_lines, cs, index);
    }
}

fn parse_flat_form(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    index: &CatalogIndex,
) {
    let refs: Vec<&str> = section
        .iter()
        .map(String::as_str)
        .filter(|l| !l.is_empty())
        .collect();
    let (modifier_type, stripped_lines) = strip_flat_suffix(&refs);
    let info = ModifierInfo {
        modifier_type,
        generation: None,
        name: None,
        tier: None,
        rank: None,
        tags: Vec::new(),
    };
    resolve_and_push(item, info, &stripped_lines, cs, index);
}

/// Parses one modifier-bearing section (bracket or flat form) into 1+ `ParsedModifier`s appended
/// to `item.mods`.
pub fn parse_modifier_section(
    section: &[String],
    item: &mut ParsedItem,
    cs: &ClientStrings,
    index: &CatalogIndex,
) {
    if section.iter().any(|line| is_mod_info_line(line)) {
        parse_bracket_form(section, item, cs, index);
    } else {
        parse_flat_form(section, item, cs, index);
    }
}

#[cfg(test)]
mod tests {

    /// Pins the `MODIFIER_LINE` capture-boundary behavior this module's bracket parsing depends
    /// on: `[^"]+` is bounded by a real `"` character when a quoted name is present (tier/name
    /// extract correctly), but swallows any trailing `(...)` whole when no quote is present at
    /// all (confirmed empirically against the real V8 engine this session; Rust's `regex` crate
    /// is documented to match the same leftmost-first semantics for this class of pattern).
    #[test]
    fn modifier_line_capture_boundaries() {
        let cs = &crate::client_strings::EN;
        let caps = cs
            .modifier_line
            .captures(r#"Prefix Modifier "Crackling" (Tier: 7)"#)
            .unwrap();
        assert_eq!(&caps["type"], "Prefix Modifier");
        assert_eq!(&caps["name"], "Crackling");
        assert_eq!(&caps["tier"], "7");

        let caps = cs.modifier_line.captures("Implicit Modifier").unwrap();
        assert_eq!(&caps["type"], "Implicit Modifier");
        assert!(caps.name("tier").is_none());

        // No quoted name: the tier text gets swallowed into `type` whole (real, verified
        // upstream behavior -- not a bug in this port).
        let caps = cs
            .modifier_line
            .captures("Suffix Modifier (Tier: 3)")
            .unwrap();
        assert_eq!(&caps["type"], "Suffix Modifier (Tier: 3)");
        assert!(caps.name("tier").is_none());
    }
}
