//! Numeric roll extraction from a single stat line, e.g. `"173(170-179)% increased Physical
//! Damage"` or `"Adds 1(1-4) to 50(46-66) Lightning Damage"`. Ported from the shape of
//! `stat-translations.ts`'s `_statPlaceholderGenerator`/`parseRoll`
//! (`.tmp/research/ItemTextFormat.md` section 4, `StatFilterBuilding.md`), but simplified: this
//! project's `StatCatalog` is trade-API-sourced only (no local `stats.ndjson` with per-matcher
//! `negate`/`legacy` annotations). Only some stats carry both an `increased` and a `reduced`
//! catalog text; the sign flip for the rest happens where a line resolves
//! (`catalog_match.rs`), not here.

/// One numeric run found in a stat line: `value(min-max)`, `value(fixed)` -- a corrupted unique's
/// fixed stat its corruption moved, `+2(3) to Level of all Spell Skills`, the fixed value in
/// brackets -- or a bare `value` (a Unique-item fixed stat with no roll range prints just the
/// number, no parens -- treated as `min == max == value`). `span` is the byte range in the source
/// line this run occupied, for template substitution; `bare` the part of it a text without the
/// advanced copy's roll range prints: the `1` of `1(1-2)`, all of a bare `value` or of a
/// `value(fixed)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumericRun {
    pub span: (usize, usize),
    pub bare: (usize, usize),
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub dp: bool,
}

fn numeric_run_regex() -> &'static regex::Regex {
    // `[+-]?` (not just `-?`): a leading `+` must be consumed as PART of the matched span so it
    // gets templated away with the digits -- real trade-catalog stat text never carries a sign
    // (confirmed live: `{"text":"# to Strength", ...}`) even though real clipboard text shows
    // `"+8(5-8) to Strength"`; leaving the `+` as literal text in the template would make every
    // positive additive stat fail to match the catalog.
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"([+-]?\d+(?:\.\d+)?)(?:\(([+-]?\d+(?:\.\d+)?)(?:-([+-]?\d+(?:\.\d+)?))?\))?",
        )
        .expect("numeric run regex")
    });
    &RE
}

/// Finds every numeric run in `line`. Real fixtures never show thousands-separator commas
/// inside a mod's own stat line (only in *property* lines, e.g. `"Physical Damage:
/// 414-1,043"` -- `properties.rs` strips those itself before calling this), so no comma
/// handling is needed here.
pub fn find_numeric_runs(line: &str) -> Vec<NumericRun> {
    let mut runs = Vec::new();
    for caps in numeric_run_regex().captures_iter(line) {
        let whole = caps.get(0).expect("group 0 always present");
        let value: f64 = caps[1].parse().expect("regex guarantees numeric");
        let dp = caps[1].contains('.');
        let (min, max, dp, bare) = match (caps.get(2), caps.get(3)) {
            (Some(min_m), Some(max_m)) => {
                let bare = caps.get(1).expect("group 1 always present");
                let min: f64 = min_m.as_str().parse().expect("regex guarantees numeric");
                let max: f64 = max_m.as_str().parse().expect("regex guarantees numeric");
                let dp = dp || min_m.as_str().contains('.') || max_m.as_str().contains('.');
                if min > max {
                    // "reduced"-phrased stats sometimes print their bound in descending order,
                    // e.g. `14(15-10)%` -- swap, do not treat as an error.
                    (max, min, dp, (bare.start(), bare.end()))
                } else {
                    (min, max, dp, (bare.start(), bare.end()))
                }
            }
            // `+2(3)`: the stat's fixed value, which corruption moved to the one before it.
            (Some(fixed_m), None) => {
                let fixed: f64 = fixed_m.as_str().parse().expect("regex guarantees numeric");
                (
                    fixed,
                    fixed,
                    dp || fixed_m.as_str().contains('.'),
                    (whole.start(), whole.end()),
                )
            }
            _ => (value, value, dp, (whole.start(), whole.end())),
        };
        runs.push(NumericRun {
            span: (whole.start(), whole.end()),
            bare,
            value,
            min,
            max,
            dp,
        });
    }
    runs
}

/// Every candidate templating of `line` against `runs`, most-templated first (all `#`, the
/// common case) down to fully literal (no `#` at all) -- the partial-substitution trick real
/// templates need when a number is baked into the stat's text (`#% increased Spirit Reservation
/// Efficiency of Buff Skills per 100 Maximum Life`). Each comes with the runs it keeps literal, bit
/// `i` for `runs[i]`: a stat matched by it rolls at the other runs only, as EE2's
/// `_statPlaceholderGenerator` passes on only the `values` it templated. A run kept literal is
/// written bare, without its roll range: no catalog text prints one, and the live EN catalog words
/// an Expedition tablet's buried Strongboxes, rolled 1 to 2, only as `Expeditions contain 1 buried
/// Strongbox in Map` (2026-10-31), which the copy prints `1(1-2)`. `runs.len()` is almost always
/// 0-2 in practice, so the `2^N` combinations this enumerates are always cheap.
pub fn template_candidates(line: &str, runs: &[NumericRun]) -> Vec<(String, u32)> {
    if runs.is_empty() {
        return vec![(line.to_string(), 0)];
    }
    let n = runs.len();
    let mut candidates = Vec::with_capacity(1 << n);
    for literal in 0..(1u32 << n) {
        let mut out = line.to_string();
        for (i, run) in runs.iter().enumerate().rev() {
            let written = if literal & (1 << i) != 0 {
                &line[run.bare.0..run.bare.1]
            } else {
                "#"
            };
            out.replace_range(run.span.0..run.span.1, written);
        }
        candidates.push((out, literal));
    }
    // Fully-templated (literal 0) is already first; de-duplicate in case a line has no digits
    // besides `#`-adjacent ones that coincide across masks (harmless either way, just avoids
    // redundant catalog lookups).
    candidates.dedup_by(|a, b| a.0 == b.0);
    candidates
}
