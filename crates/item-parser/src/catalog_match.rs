//! Resolves a mod's suffix-stripped stat lines against a `poe2_domain::StatCatalog` -- the Rust
//! equivalent of `stat-translations.ts`'s `linesToStatStrings` + `tryParseTranslation`
//! (`.tmp/research/ItemTextFormat.md` section 5, `.tmp/research/StatIdMapping.md`) for a
//! trade-API-only catalog.
//!
//! The trade catalog carries ONE text per stat, while the client prints a stat in several ways;
//! EE2 keeps every printed variant as its own matcher in its `stats.ndjson`. Run against the live
//! EN catalog (`GET /api/trade2/data/stats`, 2026-09-22), the EE2 fixture suite missed on exactly
//! these differences, each bridged here:
//!
//! - a stat printed over two lines is one catalog text joined by a line break (a tablet's
//!   `Adds a Mirror of Delirium to a Map \n# use remaining`, with or without a space before the
//!   break): consecutive lines are tried joined, shortest span first, as `linesToStatStrings`
//!   does;
//! - a negative roll flips a word (`48% reduced Duration of Bleeding on You` is the catalog's
//!   `#% increased ...` at -48): `ClientStrings::negations` holds the word pairs, the value is
//!   negated;
//! - the catalog prints grammatical number for one value (`Has # Charm Slot`, `# use remaining`,
//!   `Loads an additional bolt`) where the item prints another (`Has 2 Charm Slots`, `17 uses
//!   remaining`, `Loads 2 additional bolts`): words may differ in their ending, and a
//!   `ClientStrings::number_words` word ("an") may stand for `#` (value 1);
//! - a weapon's accuracy and attack speed and an armour piece's defences and block are the
//!   `(Local)` catalog stats, which print exactly like their global counterparts
//!   (`LOCAL_STATS`).
//! - a catalog text may print a number's sign (`+# к уровню всех умений Вихрь стрел`), which a
//!   templated line never keeps (`roll.rs` templates `+1` to `#`): texts are indexed unsigned;
//! - past all of these, EE2's own printed forms (`stat_forms`, from its `stats.ndjson`): a
//!   negative roll in words no pair covers, a 100% chance printed as a plain effect, a count of
//!   one as a word.
//!
//! A catalog entry's mod type is its id's prefix -- the namespace `ModifierType::trade_key()`
//! names -- not its `type` field: rune stats are `rune.stat_*` ids typed `"augment"`.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::HashMap;

use poe2_domain::{ModifierType, ParsedStat, TradeStat};

use crate::client_strings::ClientStrings;
use crate::roll::{NumericRun, find_numeric_runs, template_candidates};

/// The catalog, indexed for one `parse_clipboard` call.
pub struct CatalogIndex<'a> {
    stats: &'a [TradeStat],
    /// Single-line texts by `(mod type, text)`, `unsigned`.
    single_line: HashMap<(&'a str, Cow<'a, str>), &'a TradeStat>,
    /// Multi-line texts by `(mod type, text without the spaces around its line breaks)`,
    /// `unsigned`.
    multi_line: HashMap<(&'a str, String), &'a TradeStat>,
    /// The most lines any catalog text spans.
    max_lines: usize,
    /// Every stat and its words by `(mod type, word count)`, for the grammatical-number
    /// fallback; built by the first line that falls through to it, which most items never have.
    by_word_count: OnceCell<WordCountIndex<'a>>,
}

type WordCountIndex<'a> = HashMap<(&'a str, usize), Vec<(&'a TradeStat, Vec<&'a str>)>>;

/// A catalog entry's mod type: its id's `rune`/`explicit`/... prefix.
fn mod_type_of(stat: &TradeStat) -> &str {
    stat.id.split_once('.').map_or("", |(mod_type, _)| mod_type)
}

/// `text` with the whitespace around each line break dropped: the live catalog writes both
/// `"Map \n# use"` and `"found\nYour"`, and joined clipboard lines have none.
pub(crate) fn without_break_spaces(text: &str) -> String {
    text.split('\n')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
}

/// `text` without the sign before its `#`s: a templated line has none (`roll.rs`), while 269 of the
/// live RU catalog's texts print one (`+# к уровню всех умений Вихрь стрел`, 2026-09-23).
fn unsigned(text: &str) -> Cow<'_, str> {
    if text.contains("+#") {
        Cow::Owned(text.replace("+#", "#"))
    } else {
        Cow::Borrowed(text)
    }
}

/// `text`'s words, each line break a word of its own, so line structure takes part in
/// comparisons.
fn words(text: &str) -> Vec<&str> {
    let mut words = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            words.push("\n");
        }
        words.extend(line.split(' ').filter(|word| !word.is_empty()));
    }
    words
}

impl<'a> CatalogIndex<'a> {
    pub fn build(stats: &'a [TradeStat]) -> Self {
        let mut single_line = HashMap::with_capacity(stats.len());
        let mut multi_line = HashMap::new();
        let mut max_lines = 1;
        for stat in stats {
            let mod_type = mod_type_of(stat);
            if stat.text.contains('\n') {
                max_lines = max_lines.max(stat.text.split('\n').count());
                multi_line.insert(
                    (
                        mod_type,
                        unsigned(&without_break_spaces(&stat.text)).into_owned(),
                    ),
                    stat,
                );
            } else {
                let key = (mod_type, unsigned(&stat.text));
                // A text that is both a stat's own and another stat's option (`Blood Magic`, a
                // keystone and an option of `#(Ancestral Bond-Zealot's Oath)` in the live EN
                // catalog, 2026-09-23) means the stat itself, as EE2 reads it.
                let option_over_stat = stat.id.contains('|')
                    && single_line
                        .get(&key)
                        .is_some_and(|kept: &&TradeStat| !kept.id.contains('|'));
                if !option_over_stat {
                    single_line.insert(key, stat);
                }
            }
        }
        Self {
            stats,
            single_line,
            multi_line,
            max_lines,
            by_word_count: OnceCell::new(),
        }
    }

    /// The stat printed exactly as `text` (line-break spacing aside) under `mod_type`.
    fn exact(&self, mod_type: &str, text: &str) -> Option<&'a TradeStat> {
        if text.contains('\n') {
            self.multi_line
                .get(&(mod_type, without_break_spaces(text)))
                .copied()
        } else {
            self.single_line
                .get(&(mod_type, Cow::Borrowed(text)))
                .copied()
        }
    }

    fn by_word_count(&self) -> &WordCountIndex<'a> {
        self.by_word_count.get_or_init(|| {
            let mut buckets: WordCountIndex = HashMap::new();
            for stat in self.stats {
                let words = words(&stat.text);
                buckets
                    .entry((mod_type_of(stat), words.len()))
                    .or_default()
                    .push((stat, words));
            }
            buckets
        })
    }

    /// The one stat under `mod_type` that `text` prints in another grammatical number, and the
    /// value its number words imply (`Some(1.0)` when `text` has `an` where the catalog has `#`).
    /// `None` when no stat or several different ones fit.
    fn by_grammatical_number(
        &self,
        mod_type: &str,
        text: &str,
        cs: &ClientStrings,
    ) -> Option<(&'a TradeStat, Option<f64>)> {
        let text_words = words(text);
        let bucket = self.by_word_count().get(&(mod_type, text_words.len()))?;
        let mut found: Option<(&TradeStat, Option<f64>)> = None;
        for (stat, stat_words) in bucket {
            let Some(implied) = same_but_number(&text_words, stat_words, cs) else {
                continue;
            };
            match found {
                Some((other, _)) if other.id != stat.id => return None,
                Some(_) => {}
                None => found = Some((stat, implied)),
            }
        }
        found
    }

    fn by_id(&self, id: &str) -> Option<&'a TradeStat> {
        self.stats.iter().find(|stat| stat.id == id)
    }
}

/// Whether `printed` and `catalog` are one text in different grammatical number: word for word
/// equal, save words differing only in a short ending (`Slots`/`Slot`, `ячейки`/`ячейку`) and a
/// number word facing `#`. Returns the value such a number word implies, if one stands in for
/// the catalog's `#`.
fn same_but_number(printed: &[&str], catalog: &[&str], cs: &ClientStrings) -> Option<Option<f64>> {
    let number_word = |word: &str| cs.number_words.contains(&word);
    let mut implied = None;
    for (&p, &c) in printed.iter().zip(catalog) {
        if p == c || (p == "#" && number_word(c)) || same_stem(p, c) {
            continue;
        }
        if c == "#" && number_word(p) {
            implied = Some(1.0);
            continue;
        }
        return None;
    }
    Some(implied)
}

/// Two words sharing a stem of at least three characters and at least half the longer word,
/// each ending past it in at most three characters.
fn same_stem(a: &str, b: &str) -> bool {
    let common = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let (a_len, b_len) = (a.chars().count(), b.chars().count());
    common >= 3 && common * 2 >= a_len.max(b_len) && a_len - common <= 3 && b_len - common <= 3
}

/// `text` with its first whole word `from` replaced by `to`; `None` when `text` has no such
/// word.
fn replace_word(text: &str, from: &str, to: &str) -> Option<String> {
    let mut offset = 0;
    for word in text.split([' ', '\n']) {
        if word == from {
            let mut replaced = String::with_capacity(text.len() + to.len());
            replaced.push_str(&text[..offset]);
            replaced.push_str(to);
            replaced.push_str(&text[offset + word.len()..]);
            return Some(replaced);
        }
        offset += word.len() + 1;
    }
    None
}

/// Which items print a stat's `(Local)` form.
#[derive(Clone, Copy)]
enum LocalOn {
    Weapons,
    /// Armour pieces: every `armour.*` category but quivers, which hold no defences of their own.
    Armour,
}

/// `(global hash, local hash, where the local one applies)`: every `(Local)` stat of the live
/// catalog (2026-09-22), whose text is its global counterpart's plus ` (Local)`/` (На
/// предмете)`. EE2 names both ids for these stats (`stats.ndjson`); the item's own kind decides.
const LOCAL_STATS: [(&str, &str, LocalOn); 8] = [
    // # to Accuracy Rating
    ("stat_803737631", "stat_691932474", LocalOn::Weapons),
    // #% increased Attack Speed
    ("stat_681332047", "stat_210067635", LocalOn::Weapons),
    // # to maximum Energy Shield
    ("stat_3489782002", "stat_4052037485", LocalOn::Armour),
    // # to Evasion Rating
    ("stat_2144192055", "stat_53045048", LocalOn::Armour),
    // # to Armour
    ("stat_809229260", "stat_3484657501", LocalOn::Armour),
    // #% increased Evasion Rating
    ("stat_2106365538", "stat_124859000", LocalOn::Armour),
    // #% increased Armour
    ("stat_2866361420", "stat_1062208444", LocalOn::Armour),
    // #% increased Block chance
    ("stat_4147897060", "stat_2481353198", LocalOn::Armour),
];

/// `stat`, or its `(Local)` form where `category` is an item that prints it locally.
fn localized<'a>(
    stat: &'a TradeStat,
    category: Option<&str>,
    index: &CatalogIndex<'a>,
) -> &'a TradeStat {
    let Some(category) = category else {
        return stat;
    };
    let Some((mod_type, hash)) = stat.id.split_once('.') else {
        return stat;
    };
    let Some(&(_, local, on)) = LOCAL_STATS.iter().find(|(global, ..)| *global == hash) else {
        return stat;
    };
    let applies = match on {
        LocalOn::Weapons => category.starts_with("weapon."),
        LocalOn::Armour => category.starts_with("armour.") && category != "armour.quiver",
    };
    if !applies {
        return stat;
    }
    index.by_id(&format!("{mod_type}.{local}")).unwrap_or(stat)
}

/// How one printed text resolved.
struct Resolved<'a> {
    stat: &'a TradeStat,
    /// The catalog stat counts the other way (`reduced` for `increased`): the roll is negated,
    /// and this is the printed text that says so.
    negated: Option<String>,
    /// A number word stood in for the catalog's `#`, with this value.
    implied: Option<f64>,
    /// An EE2 printed form (`stat_forms`) gave the stat, worded another way than the catalog's
    /// text but not the other way round: how the item says it.
    printed: Option<String>,
}

/// Resolves one printed text, numbers already templated to `#`: exactly, then with a word of a
/// `negations` pair flipped, then in another grammatical number.
fn resolve_text<'a>(
    text: &str,
    mod_type: &str,
    cs: &ClientStrings,
    index: &CatalogIndex<'a>,
) -> Option<Resolved<'a>> {
    if let Some(stat) = index.exact(mod_type, text) {
        return Some(Resolved {
            stat,
            negated: None,
            implied: None,
            printed: None,
        });
    }
    for &(positive, negative) in cs.negations {
        for (from, to) in [(negative, positive), (positive, negative)] {
            if let Some(flipped) = replace_word(text, from, to)
                && let Some(stat) = index.exact(mod_type, &flipped)
            {
                return Some(Resolved {
                    stat,
                    negated: Some(text.to_owned()),
                    implied: None,
                    printed: None,
                });
            }
        }
    }
    index
        .by_grammatical_number(mod_type, text, cs)
        .map(|(stat, implied)| Resolved {
            stat,
            negated: None,
            implied,
            printed: None,
        })
        .or_else(|| by_printed_form(text, mod_type, cs, index))
}

/// The stat EE2 knows `text` as a printed form of (`stat_forms`), under `mod_type`: the first of
/// its trade ids the catalog lists for that mod type.
fn by_printed_form<'a>(
    text: &str,
    mod_type: &str,
    cs: &ClientStrings,
    index: &CatalogIndex<'a>,
) -> Option<Resolved<'a>> {
    let forms = if text.contains('\n') {
        cs.stat_forms.get(&without_break_spaces(text))
    } else {
        cs.stat_forms.get(text)
    }?;
    forms.iter().find_map(|form| {
        let stat = form
            .ids
            .iter()
            .filter(|id| id.split_once('.').is_some_and(|(kind, _)| kind == mod_type))
            .find_map(|id| index.by_id(id))?;
        Some(Resolved {
            stat,
            negated: form.negated.then(|| text.to_owned()),
            implied: form.value,
            printed: (!form.negated).then(|| text.to_owned()),
        })
    })
}

/// Whether `line` opens a parenthesised reminder text (EE2's `LOCALIZED_PAREN_LEFT`).
fn opens_reminder(line: &str) -> bool {
    line.trim_start().starts_with(['(', '（'])
}

/// Whether `line` closes a parenthesised reminder text (EE2's `LOCALIZED_PAREN_RIGHT`).
fn closes_reminder(line: &str) -> bool {
    line.trim_end().ends_with([')', '）'])
}

/// Resolves one mod's suffix-stripped stat `lines` (e.g. `["173(170-179)% increased Physical
/// Damage"]`) against the catalog for `modifier_type`, into one `ParsedStat` per stat: a stat
/// printed over several lines becomes one, parenthesised reminder text none. A line nothing in
/// the catalog matches still becomes a stat, with `stat_id: None`. `category` is the item's trade
/// category, which picks a stat's local or global form.
pub fn resolve_stat_lines(
    lines: &[&str],
    modifier_type: ModifierType,
    category: Option<&str>,
    cs: &ClientStrings,
    index: &CatalogIndex,
) -> Vec<ParsedStat> {
    let mut stats = Vec::new();
    let mut start = 0;
    let mut in_reminder = false;
    while start < lines.len() {
        let line = lines[start];
        if !in_reminder && opens_reminder(line) {
            in_reminder = true;
        }
        if in_reminder {
            in_reminder = !closes_reminder(line);
            start += 1;
            continue;
        }
        // Shortest span first, as `linesToStatStrings` tries them: a line joins the next ones
        // only when it resolves on no catalog stat by itself.
        let longest = (lines.len() - start).min(index.max_lines);
        let resolved = (1..=longest).find_map(|span| {
            resolve_span(
                &lines[start..start + span],
                modifier_type,
                category,
                cs,
                index,
            )
            .map(|stat| (stat, span))
        });
        match resolved {
            Some((stat, span)) => {
                stats.push(stat);
                start += span;
            }
            None => {
                stats.push(unresolved(line, cs));
                start += 1;
            }
        }
    }
    stats
}

/// The catalog stat `lines` print, joined by line breaks, if the catalog has one.
fn resolve_span(
    lines: &[&str],
    modifier_type: ModifierType,
    category: Option<&str>,
    cs: &ClientStrings,
    index: &CatalogIndex,
) -> Option<ParsedStat> {
    // The client prints `()` where a stat lacks an advanced description (a Megalomaniac's
    // `Allocates Incendiary()`); EE2's `_statPlaceholderGenerator` drops it before matching.
    let joined = lines.join("\n").replace("()", "");
    let (text, unscalable) = match joined.strip_suffix(cs.unscalable_value) {
        Some(rest) => (rest, true),
        None => (joined.as_str(), false),
    };
    let runs = find_numeric_runs(text);
    let mod_type = modifier_type.trade_key();
    let resolved = template_candidates(text, &runs)
        .iter()
        .find_map(|candidate| resolve_text(candidate, mod_type, cs, index))?;
    Some(stat_from(&resolved, category, &runs, unscalable, index))
}

/// An unresolved line's stat.
fn unresolved(line: &str, cs: &ClientStrings) -> ParsedStat {
    let (text, unscalable) = match line.strip_suffix(cs.unscalable_value) {
        Some(rest) => (rest, true),
        None => (line, false),
    };
    let runs = find_numeric_runs(text);
    ParsedStat {
        unscalable: unscalable || runs.is_empty(),
        ..rolled(None, text.to_string(), &runs)
    }
}

fn stat_from<'a>(
    resolved: &Resolved<'a>,
    category: Option<&str>,
    runs: &[NumericRun],
    unscalable: bool,
    index: &CatalogIndex<'a>,
) -> ParsedStat {
    let stat = localized(resolved.stat, category, index);
    let mut parsed = rolled(Some(stat.id.clone()), stat.text.clone(), runs);
    if let Some(value) = resolved.implied.filter(|_| runs.is_empty()) {
        parsed.value = value;
        parsed.min = value;
        parsed.max = value;
    }
    if let Some(printed) = &resolved.negated {
        (parsed.value, parsed.min, parsed.max) = (-parsed.value, -parsed.max, -parsed.min);
        parsed.negated_text = Some(printed.clone());
    }
    parsed.printed_text = resolved.printed.clone();
    parsed.unscalable = unscalable || (runs.is_empty() && resolved.implied.is_none());
    parsed
}

/// A stat rolled from `runs`: several numbers on one stat are still one stat ("Adds 12(10-15) to
/// 220(200-240) Lightning Damage" is `Adds # to # Lightning Damage`), and the trade API filters it
/// by their average -- EE2's `getRollOrMinmaxAvg` (`stat-translations.ts`) rolls two or four
/// numbers at their mean and anything else at the first. Summing them instead (one stat per
/// number, grouped downstream) doubled every such filter's bounds and emptied its searches. No
/// runs at all is a flag stat (an `" — Unscalable Value"` line, a named allocation), rolled at 0.
fn rolled(stat_id: Option<String>, text: String, runs: &[NumericRun]) -> ParsedStat {
    let rolled = match runs.len() {
        0 => &[][..],
        2 | 4 => runs,
        _ => &runs[..1],
    };
    // `sum` of no `f64`s is -0.0; a flag stat rolls at a plain 0.
    let mean = |field: fn(&NumericRun) -> f64| {
        rolled.iter().map(field).fold(0.0, |sum, value| sum + value) / rolled.len().max(1) as f64
    };
    ParsedStat {
        stat_id,
        text,
        value: mean(|run| run.value),
        min: mean(|run| run.min),
        max: mean(|run| run.max),
        dp: rolled.iter().any(|run| run.dp),
        unscalable: false,
        negated_text: None,
        printed_text: None,
    }
}
