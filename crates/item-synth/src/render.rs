//! A listing's item -> the text the game copies for it with Ctrl+Alt+C: its advanced copy, the
//! one the app asks the game for.
//!
//! The layout follows real copies (`crates/item-parser/tests/fixtures`, the raw ones `ru_live_*`
//! and those players sent) and the texts are the client's own (`texts.rs`). What the listing
//! can't tell is either left out the way the game itself leaves it out of some mods, or made up
//! within the game's rules and said so in [`Synthesized::guesses`]:
//!
//! - A mod header's tags (`— Elemental, Fire, Resistance`) are never written: the site doesn't
//!   give them, and the game writes headers without them for mods that have none.
//! - Neither are a stat's `— Unscalable Value` and a header's `— 22% Increased` (a mod's
//!   magnitudes raised by quality or another mod): the site doesn't say which stats they are.
//! - A line several mods add to (two prefixes' `55% increased Evasion Rating`) shows each mod's own
//!   share in the game; the site gives only the sum, so the text splits it within each mod's
//!   range (a guess).
//! - The site numbers each kind of mod (explicit, fractured, desecrated, crafted) on its own, so
//!   among one item's prefixes (and its suffixes) the fractured ones come first, then the others,
//!   the desecrated and the crafted ones last; within a kind they keep the item's own order, which
//!   the game's copy lists them in (checked on The Eternal Spark).

use std::fmt::Write as _;

use anyhow::{Context as _, Result, bail, ensure};
use item_parser::ItemLanguage;

use crate::listing::{Affix, DescribedLine, Frame, Hashes, Item, Magnitude, ModLine, Property};
use crate::texts::{self, Texts};

/// A synthesized copy of an item.
#[derive(Debug, Clone, PartialEq)]
pub struct Synthesized {
    /// The text with the game's own line breaks: `\r\n` after every line, a bare `\n` where a
    /// line breaks inside -- after a mod's header, in a stat that takes two lines.
    pub text: String,
    /// What the text makes up because the listing couldn't tell, in words.
    pub guesses: Vec<String>,
}

/// What [`render`] writes besides the item: the seller's note, which a copy of an item in a stash
/// tab carries and one in the inventory doesn't.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub with_note: bool,
}

/// The game's copy of `item`, a listing in `language`'s trade site, whose item class the game
/// calls `class` in that language.
pub fn render(
    item: &Item,
    class: &str,
    language: ItemLanguage,
    options: Options,
) -> Result<Synthesized> {
    let texts = texts::of(language);
    let mut out = Writer {
        sections: Vec::new(),
        guesses: Vec::new(),
    };
    let frame = item.frame()?;
    ensure!(
        frame != Frame::Gem,
        "skill and support gems are not written yet: their skill section's layout isn't known well \
         enough"
    );
    ensure!(
        item.notable_properties.is_empty(),
        "notable properties (a Time-Lost diamond's, a Megalomaniac's) are not written yet"
    );

    out.section(nameplate(item, frame, class, texts)?);
    out.section(
        item.properties
            .iter()
            // The item class line the site puts first (`Ring`, `[Crossbow]`): the copy has it as
            // `Item Class:` above.
            .filter(|property| property.kind != Some(109))
            .map(property_line)
            .collect(),
    );
    out.section(requirements(item, texts));
    out.section(sockets(item, texts)?);
    if is_gear(frame) && item.ilvl > 0 {
        out.section(vec![format!("{}{}", texts.item_level, item.ilvl)]);
    }
    out.section(item.granted_skills.iter().map(property_line).collect());

    let hashes = &item.extended.hashes;
    let mut enchants = Mods::default();
    enchants.add_lines(&item.enchant_mods, hashes, &mut out.guesses)?;
    out.section(enchants.headed(item, texts)?);
    out.section(
        item.rune_mods
            .iter()
            .map(|line| format!("{} (rune)", strip_markup(line.description())))
            .collect(),
    );
    let mut implicits = Mods::default();
    implicits.add_lines(&item.implicit_mods, hashes, &mut out.guesses)?;
    out.section(implicits.headed(item, texts)?);
    let mut explicits = Mods::default();
    explicits.add_lines(&item.explicit_mods, hashes, &mut out.guesses)?;
    out.section(explicits.headed(item, texts)?);
    out.section(explicits.plain);
    if !item.bonded_mods.is_empty() {
        out.guesses.push(
            "a Bonded rune's effects are written as their own section after the item's own"
                .to_owned(),
        );
        out.section(
            item.bonded_mods
                .iter()
                .map(|line| strip_markup(line.description()))
                .collect(),
        );
    }

    if !item.identified {
        let line = match item.unidentified_tier {
            Some(tier) => format!("{} ({} {tier})", texts.unidentified, texts.tier_word),
            None => texts.unidentified.to_owned(),
        };
        out.section(vec![line]);
    }
    out.section(
        item.flavour_text
            .iter()
            .flat_map(|line| line.split('\n'))
            .map(|line| line.trim_end_matches('\r').to_owned())
            .filter(|line| !line.is_empty())
            .collect(),
    );
    let mut description: Vec<String> = item
        .descr_text
        .iter()
        .map(|text| strip_markup(text))
        .collect();
    if item.stack_size.is_some_and(|size| size > 1) {
        // `ItemDisplayStackDescription`: the client adds it to a stack it can split (every real
        // copy of a stack of 2 or more has it, none of a stack of 1).
        description.push(
            match language {
                ItemLanguage::English => "Shift click to unstack.",
                ItemLanguage::Russian => "Для разделения щёлкните мышкой с зажатой клавишей Shift.",
            }
            .to_owned(),
        );
    }
    out.section(description);

    let states = [
        (item.corrupted && !item.double_corrupted, texts.corrupted),
        (item.double_corrupted, texts.twice_corrupted),
        (item.duplicated, texts.mirrored),
        (item.sanctified, texts.sanctified),
        (item.fractured, texts.fractured_item),
    ];
    let shown = states.iter().filter(|(on, _)| *on).count();
    if shown > 1 && !(shown == 2 && item.corrupted && item.fractured) {
        out.guesses
            .push("the order of the Corrupted, Mirrored and Sanctified lines".to_owned());
    }
    for (on, text) in states {
        if on {
            out.section(vec![text.to_owned()]);
        }
    }
    if options.with_note
        && let Some(note) = &item.note
    {
        out.section(vec![format!("{}{note}", texts.note)]);
    }

    let mut text = String::new();
    for (index, section) in out.sections.iter().enumerate() {
        if index > 0 {
            text.push_str("--------\r\n");
        }
        for line in section {
            text.push_str(line);
            text.push_str("\r\n");
        }
    }
    Ok(Synthesized {
        text,
        guesses: out.guesses,
    })
}

struct Writer {
    sections: Vec<Vec<String>>,
    guesses: Vec<String>,
}

impl Writer {
    /// Adds a section unless it is empty: the game writes no separator for a section it doesn't
    /// have.
    fn section(&mut self, lines: Vec<String>) {
        if !lines.is_empty() {
            self.sections.push(lines);
        }
    }
}

fn is_gear(frame: Frame) -> bool {
    matches!(
        frame,
        Frame::Normal | Frame::Magic | Frame::Rare | Frame::Unique
    )
}

fn nameplate(item: &Item, frame: Frame, class: &str, texts: &Texts) -> Result<Vec<String>> {
    let rarity = texts.frames[match frame {
        Frame::Normal => 0,
        Frame::Magic => 1,
        Frame::Rare => 2,
        Frame::Unique => 3,
        Frame::Gem => 4,
        Frame::Currency => 5,
    }];
    let mut lines = vec![
        format!("{}{class}", texts.item_class),
        format!("{}{rarity}", texts.rarity),
    ];
    // A rare's or a unique's own name goes above its base, once identified; any other item is
    // named by its type line alone (a magic item's affixes around its base).
    if matches!(frame, Frame::Rare | Frame::Unique) && item.identified {
        ensure!(
            !item.name.is_empty(),
            "an identified {rarity} item without a name"
        );
        lines.push(item.name.clone());
    }
    lines.push(item.type_line.clone());
    Ok(lines)
}

/// A property line: `Quality: +20% (augmented)`, `84 (augmented) Int`, `Lasts 3.40 Seconds`.
fn property_line(property: &Property) -> String {
    let name = strip_markup(&property.name);
    let values: Vec<String> = property
        .values
        .iter()
        .map(|(value, kind)| format!("{}{}", strip_markup(value), value_note(*kind)))
        .collect();
    match property.display_mode {
        1 => {
            let mut line = values.join(", ");
            if !name.is_empty() {
                line.push(' ');
                line.push_str(&name);
            }
            line
        }
        3 => {
            let mut line = name;
            for (index, value) in values.iter().enumerate() {
                line = line.replace(&format!("{{{index}}}"), value);
            }
            line
        }
        _ if values.is_empty() => name,
        _ if name.is_empty() => values.join(", "),
        _ => format!("{name}: {}", values.join(", ")),
    }
}

/// What the copy writes after a value of GGG's value type `kind`: augmented by the item's own
/// mods or quality, a requirement unmet, damage of an element.
fn value_note(kind: u32) -> &'static str {
    match kind {
        1 => " (augmented)",
        2 => " (unmet)",
        4 => " (fire)",
        5 => " (cold)",
        6 => " (lightning)",
        7 => " (chaos)",
        _ => "",
    }
}

/// `Requires: Level 75, 84 (augmented) Int`, and a gem's `Requires: Spear, Bow`.
fn requirements(item: &Item, texts: &Texts) -> Vec<String> {
    let mut lines = Vec::new();
    if !item.requirements.is_empty() {
        let parts: Vec<String> = item
            .requirements
            .iter()
            .map(|requirement| {
                let name = strip_markup(&requirement.name);
                let values: Vec<String> = requirement
                    .values
                    .iter()
                    .map(|(value, kind)| format!("{}{}", strip_markup(value), value_note(*kind)))
                    .collect();
                if requirement.display_mode == 1 {
                    format!("{} {name}", values.join(", "))
                } else {
                    format!("{name} {}", values.join(", "))
                }
            })
            .collect();
        lines.push(format!("{}{}", texts.requires, parts.join(", ")));
    }
    if !item.weapon_requirements.is_empty() {
        let weapons: Vec<String> = item
            .weapon_requirements
            .iter()
            .flat_map(|requirement| &requirement.values)
            .map(|(value, _)| strip_markup(value))
            .collect();
        lines.push(format!("{}{}", texts.requires, weapons.join(", ")));
    }
    lines
}

/// `Sockets: S S `: a letter for each socket, each followed by a space (the client's own trailing
/// one included).
fn sockets(item: &Item, texts: &Texts) -> Result<Vec<String>> {
    if item.sockets.is_empty() {
        return Ok(Vec::new());
    }
    let mut line = texts.sockets.to_owned();
    for socket in &item.sockets {
        let letter = match socket.kind.as_str() {
            "gem" => 'G',
            "rune" | "soulcore" | "talisman" => 'S',
            other => bail!("a socket of type {other}"),
        };
        line.push(letter);
        line.push(' ');
    }
    Ok(vec![line])
}

/// Where a mod comes from, as the site files its line: the order it takes among the item's
/// prefixes or suffixes in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    Fractured,
    Own,
    Desecrated,
    Crafted,
}

/// One of the item's mods: what heads it and the lines it gives the text.
#[derive(Debug)]
struct Mod {
    kind: String,
    index: usize,
    origin: Origin,
    name: Option<String>,
    tier: Option<String>,
    mutated: bool,
    lines: Vec<String>,
}

/// One mod list's lines, grouped into the item's mods.
#[derive(Debug, Default)]
struct Mods {
    mods: Vec<Mod>,
    /// Lines with no mod behind them: a currency's or a rune's effects.
    plain: Vec<String>,
}

impl Mods {
    fn add_lines(
        &mut self,
        lines: &[ModLine],
        hashes: &Hashes,
        guesses: &mut Vec<String>,
    ) -> Result<()> {
        let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
        for line in lines {
            let described = match line {
                ModLine::Described(described) if described.hash.is_some() => described,
                other => {
                    self.plain.push(strip_markup(other.description()));
                    continue;
                }
            };
            let hash = described.hash.as_deref().unwrap_or_default();
            let id = hash.strip_prefix("stat.").unwrap_or(hash);
            let kind = id.split('.').next().unwrap_or_default().to_owned();
            let position = seen.entry(kind.clone()).or_default();
            let entry = hashes
                .get(&kind)
                .and_then(|entries| entries.get(*position))
                .with_context(|| format!("no extended.hashes.{kind} entry for {hash}"))?;
            *position += 1;
            // The site's hash can carry a suffix after its stat id (`...|62431`, which notable).
            ensure!(
                id.split('|').next() == entry.0.split('|').next(),
                "extended.hashes.{kind} names {} where the line is {hash}",
                entry.0
            );
            let Some(indexes) = &entry.1 else {
                self.plain.push(strip_markup(&described.description));
                continue;
            };
            self.add_line(&kind, described, indexes, guesses)?;
        }
        Ok(())
    }

    fn add_line(
        &mut self,
        kind: &str,
        line: &DescribedLine,
        indexes: &[usize],
        guesses: &mut Vec<String>,
    ) -> Result<()> {
        let text = strip_markup(&line.description);
        let mutated = line.flags.get("mutated").copied().unwrap_or(false);
        let origin = match kind {
            "fractured" => Origin::Fractured,
            "desecrated" => Origin::Desecrated,
            "crafted" => Origin::Crafted,
            _ => Origin::Own,
        };
        ensure!(
            line.mods.is_empty() || line.mods.len() == indexes.len(),
            "{text:?}: {} mods for {} indexes",
            line.mods.len(),
            indexes.len()
        );
        let shares: Vec<String> = match indexes {
            [_] => vec![with_ranges(
                &text,
                line.mods.first().map_or(&[][..], |affix| &affix.magnitudes),
                guesses,
            )],
            _ if line.mods.is_empty() => {
                guesses.push(format!(
                    "{text:?} is shared by mods the site doesn't describe; the first one has it all"
                ));
                let mut shares = vec![text.clone()];
                shares.resize(indexes.len(), String::new());
                shares
            }
            _ => split_line(&text, &line.mods, guesses),
        };
        for (position, (&index, share)) in indexes.iter().zip(shares).enumerate() {
            let affix: Option<&Affix> = line.mods.get(position);
            let at = match self
                .mods
                .iter()
                .position(|existing| existing.kind == kind && existing.index == index)
            {
                Some(at) => at,
                None => {
                    self.mods.push(Mod {
                        kind: kind.to_owned(),
                        index,
                        origin,
                        name: None,
                        tier: None,
                        mutated: false,
                        lines: Vec::new(),
                    });
                    self.mods.len() - 1
                }
            };
            let entry = &mut self.mods[at];
            entry.mutated |= mutated;
            if let Some(affix) = affix {
                entry.name = entry.name.take().or_else(|| affix.name.clone());
                entry.tier = entry.tier.take().or_else(|| affix.tier.clone());
            }
            if !share.is_empty() {
                entry.lines.push(share);
            }
        }
        Ok(())
    }

    /// The mods' lines, each mod's first line under its header, in the order the game lists them.
    fn headed(&self, item: &Item, texts: &Texts) -> Result<Vec<String>> {
        let unique = item.frame()? == Frame::Unique;
        let mut order: Vec<(u8, Origin, usize, &Mod)> = Vec::with_capacity(self.mods.len());
        for entry in &self.mods {
            // Implicits and enchants keep the item's order; so do a unique's mods, which have no
            // prefix or suffix. Everything else lists its prefixes, then its suffixes.
            let group = match (entry.kind.as_str(), unique) {
                ("implicit" | "enchant", _) | (_, true) => 0,
                _ => match generation(entry)? {
                    Generation::Prefix => 0,
                    Generation::Suffix => 1,
                },
            };
            order.push((group, entry.origin, entry.index, entry));
        }
        order.sort_by_key(|(group, origin, index, _)| (*group, *origin, *index));

        let mut lines = Vec::new();
        for (_, _, _, entry) in order {
            let header = header(entry, item, texts)?;
            let mut first = true;
            for line in &entry.lines {
                if first {
                    lines.push(format!("{{ {header} }}\n{line}"));
                    first = false;
                } else {
                    lines.push(line.clone());
                }
            }
        }
        Ok(lines)
    }
}

enum Generation {
    Prefix,
    Suffix,
}

fn generation(entry: &Mod) -> Result<Generation> {
    match entry.tier.as_deref().and_then(|tier| tier.chars().next()) {
        Some('P') => Ok(Generation::Prefix),
        Some('S') => Ok(Generation::Suffix),
        _ => bail!(
            "{} mod {} ({:?}) is neither a prefix nor a suffix: tier {:?}",
            entry.kind,
            entry.index,
            entry.name,
            entry.tier
        ),
    }
}

/// What heads a mod in braces: `Prefix Modifier "Cheetah's" (Tier: 2)`, `Implicit Modifier`.
fn header(entry: &Mod, item: &Item, texts: &Texts) -> Result<String> {
    match entry.kind.as_str() {
        "implicit" => return Ok(texts.implicit.to_owned()),
        "enchant" => {
            // A corruption's enchantment says so; an anointment's or a unique's own "Allocates"
            // one doesn't. The site doesn't tell them apart: an enchantment of a corrupted item
            // is taken for the corruption's unless it allocates a passive.
            let allocates = entry
                .lines
                .iter()
                .any(|line| line.starts_with("Allocates ") || line.starts_with("Распределяет "));
            return Ok(if item.corrupted && !allocates {
                texts.corruption_enchantment
            } else {
                texts.enchantment
            }
            .to_owned());
        }
        _ => {}
    }
    if item.frame()? == Frame::Unique {
        return Ok(if entry.mutated {
            texts.vaal_unique
        } else {
            texts.unique
        }
        .to_owned());
    }
    let name = entry
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .with_context(|| format!("{} mod {} has no name", entry.kind, entry.index))?;
    let (open, close) = match generation(entry)? {
        Generation::Prefix => texts.prefix,
        Generation::Suffix => texts.suffix,
    };
    let mut header = match entry.origin {
        Origin::Fractured => texts.fractured.to_owned(),
        Origin::Desecrated => texts.desecrated.to_owned(),
        Origin::Crafted => texts.crafted.to_owned(),
        Origin::Own => String::new(),
    };
    let _ = write!(header, "{open}{name}{close}");
    // `P0`/`S0`: a mod with no tiers (an essence's, a crafting bench's), which the game shows
    // without one.
    let tier: u32 = entry
        .tier
        .as_deref()
        .and_then(|tier| tier.get(1..))
        .and_then(|tier| tier.parse().ok())
        .with_context(|| format!("tier {:?}", entry.tier))?;
    if tier > 0 {
        let _ = write!(header, "{}{tier}{}", texts.tier.0, texts.tier.1);
    }
    Ok(header)
}

/// A number in a line of text: where its digits are and what it reads, a `-` right before them
/// making it negative.
#[derive(Debug, Clone, Copy)]
struct Number {
    start: usize,
    end: usize,
    value: f64,
    decimals: usize,
}

fn numbers(text: &str) -> Vec<Number> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        let mut decimals = 0;
        if at + 1 < bytes.len() && bytes[at] == b'.' && bytes[at + 1].is_ascii_digit() {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
                decimals += 1;
            }
        }
        let magnitude: f64 = text[start..at].parse().unwrap_or_default();
        let negative = start > 0 && bytes[start - 1] == b'-';
        found.push(Number {
            start,
            end: at,
            value: if negative { -magnitude } else { magnitude },
            decimals,
        });
    }
    found
}

/// A roll range's bounds as the site writes them (`"-30"`, `"4.1"`), and as numbers.
#[derive(Debug, Clone)]
struct Bounds {
    low: String,
    high: String,
    low_value: f64,
    high_value: f64,
}

impl Bounds {
    fn of(magnitude: &Magnitude) -> Result<Bounds> {
        Ok(Bounds {
            low_value: magnitude
                .min
                .parse()
                .with_context(|| format!("bound {:?}", magnitude.min))?,
            high_value: magnitude
                .max
                .parse()
                .with_context(|| format!("bound {:?}", magnitude.max))?,
            low: magnitude.min.clone(),
            high: magnitude.max.clone(),
        })
    }

    /// The bounds the way the line words its number: a `reduced` line the site rolls from -30 to
    /// -20 reads `30` to `20`, in that order (`28(30-20)% reduced Tribute`, a real copy).
    fn as_shown(&self, number: f64) -> Bounds {
        let negated = number >= 0.0
            && self.low_value <= 0.0
            && self.high_value <= 0.0
            && (self.low_value < 0.0 || self.high_value < 0.0);
        if !negated {
            return self.clone();
        }
        Bounds {
            low: negate(&self.low),
            high: negate(&self.high),
            low_value: -self.low_value,
            high_value: -self.high_value,
        }
    }
}

fn negate(bound: &str) -> String {
    match bound.strip_prefix('-') {
        Some(positive) => positive.to_owned(),
        None if bound.trim_start_matches(['0', '.']).is_empty() => bound.to_owned(),
        None => format!("-{bound}"),
    }
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// What follows a number its mod rolls: `(124-151)`, `(3)` where the number moved off the fixed
/// value (a corrupted unique's), nothing for a fixed number. `None` when nothing follows.
fn range_after(shown_value: f64, bounds: &Bounds, guesses: &mut Vec<String>) -> Option<String> {
    let shown = bounds.as_shown(shown_value);
    if same(shown.low_value, shown.high_value) {
        return (!same(shown.low_value, shown_value)).then(|| format!("({})", shown.low));
    }
    if shown.low_value < 0.0 && shown.high_value > 0.0 {
        guesses.push(format!(
            "a range across zero, {} to {}, is written ({}-+{})",
            shown.low, shown.high, shown.low, shown.high
        ));
        return Some(format!("({}-+{})", shown.low, shown.high));
    }
    Some(format!("({}-{})", shown.low, shown.high))
}

/// `text` with each number its mod rolls followed by its range: `+143(124-151) to Evasion Rating`.
fn with_ranges(text: &str, magnitudes: &[Magnitude], guesses: &mut Vec<String>) -> String {
    let found = numbers(text);
    if magnitudes.is_empty() {
        return text.to_owned();
    }
    let bounds: Result<Vec<Bounds>> = magnitudes.iter().map(Bounds::of).collect();
    let Ok(bounds) = bounds else {
        guesses.push(format!(
            "{text:?}: the site's ranges don't read as numbers; left out"
        ));
        return text.to_owned();
    };
    if found.len() != bounds.len() {
        // A number the mod rolls at one fixed value the line doesn't show (`Area has patches of
        // Chilled Ground`, 30 to 30) takes no range anyway.
        if bounds
            .iter()
            .any(|bound| !same(bound.low_value, bound.high_value))
        {
            guesses.push(format!(
                "{text:?}: {} numbers for {} ranges; ranges left out",
                found.len(),
                bounds.len()
            ));
        }
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 12 * found.len());
    let mut copied = 0;
    for (number, bounds) in found.iter().zip(&bounds) {
        out.push_str(&text[copied..number.end]);
        copied = number.end;
        if let Some(range) = range_after(number.value, bounds, guesses) {
            out.push_str(&range);
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// A line several mods add to, one share for each with its own range: the site's sum split
/// within each mod's range (`+46 to Stun Threshold` from 12-29 and 14-24 -> 25 and 21).
fn split_line(text: &str, mods: &[Affix], guesses: &mut Vec<String>) -> Vec<String> {
    let found = numbers(text);
    let all_bounds: Result<Vec<Vec<Bounds>>> = mods
        .iter()
        .map(|affix| affix.magnitudes.iter().map(Bounds::of).collect())
        .collect();
    let fits = all_bounds
        .as_ref()
        .is_ok_and(|all| all.iter().all(|bounds| bounds.len() == found.len()));
    let shares = fits.then(|| {
        let all = all_bounds.as_ref().expect("checked");
        found
            .iter()
            .enumerate()
            .map(|(position, number)| {
                let ranges: Vec<(f64, f64)> = all
                    .iter()
                    .map(|bounds| {
                        let shown = bounds[position].as_shown(number.value);
                        (shown.low_value, shown.high_value)
                    })
                    .collect();
                split(number.value, &ranges, number.decimals)
            })
            .collect::<Option<Vec<Vec<f64>>>>()
    });
    let Some(Some(shares)) = shares else {
        guesses.push(format!(
            "{text:?}: the mods' ranges don't add up to it; the first mod has it all"
        ));
        let mut lines = vec![text.to_owned()];
        lines.resize(mods.len(), String::new());
        return lines;
    };
    guesses.push(format!(
        "{text:?} is split among {} mods within their ranges",
        mods.len()
    ));
    let all = all_bounds.expect("checked");
    (0..mods.len())
        .map(|share| {
            let mut out = String::with_capacity(text.len() + 12 * found.len());
            let mut copied = 0;
            for (position, number) in found.iter().enumerate() {
                out.push_str(&text[copied..number.start]);
                let value = shares[position][share];
                let _ = write!(out, "{:.*}", number.decimals, value.abs());
                copied = number.end;
                if let Some(range) = range_after(value, &all[share][position], guesses) {
                    out.push_str(&range);
                }
            }
            out.push_str(&text[copied..]);
            out
        })
        .collect()
}

/// `total` split into one part per range, each within its range (bounds in either order), in
/// steps of `decimals` decimals, as evenly across the ranges' widths as the steps allow. `None`
/// when the ranges can't add up to `total`.
fn split(total: f64, ranges: &[(f64, f64)], decimals: usize) -> Option<Vec<f64>> {
    let scale = 10f64.powi(decimals as i32);
    let units = |value: f64| (value * scale).round() as i64;
    let lows: Vec<i64> = ranges.iter().map(|&(a, b)| units(a.min(b))).collect();
    let highs: Vec<i64> = ranges.iter().map(|&(a, b)| units(a.max(b))).collect();
    let target = units(total);
    let low: i64 = lows.iter().sum();
    let high: i64 = highs.iter().sum();
    if target < low
        || target > high
        || ranges
            .iter()
            .any(|&(a, b)| a.min(b) < 0.0 && a.max(b) > 0.0)
    {
        return None;
    }
    let width = high - low;
    let mut parts: Vec<i64> = lows
        .iter()
        .zip(&highs)
        .map(|(lo, hi)| {
            if width == 0 {
                *lo
            } else {
                lo + (target - low) * (hi - lo) / width
            }
        })
        .collect();
    let mut left = target - parts.iter().sum::<i64>();
    // What the division rounded off goes, a step at a time, to the first parts with room.
    while left > 0 {
        let room = parts.iter().zip(&highs).position(|(part, hi)| part < hi)?;
        parts[room] += 1;
        left -= 1;
    }
    Some(parts.into_iter().map(|part| part as f64 / scale).collect())
}

/// The words the site's link markup shows: `+16% to [Resistances|Cold Resistance]` -> `+16% to
/// Cold Resistance`, `[Cold] Damage` -> `Cold Damage`.
pub(crate) fn strip_markup(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        plain.push_str(&rest[..open]);
        let link = &rest[open + 1..open + close];
        plain.push_str(link.rsplit_once('|').map_or(link, |(_, shown)| shown));
        rest = &rest[open + close + 1..];
    }
    plain.push_str(rest);
    plain
}
