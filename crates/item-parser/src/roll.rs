//! Numeric roll extraction from a single stat line, e.g. `"173(170-179)% increased Physical
//! Damage"` or `"Adds 1(1-4) to 50(46-66) Lightning Damage"`. Ported from the shape of
//! `stat-translations.ts`'s `_statPlaceholderGenerator`/`parseRoll`
//! (`.tmp/research/ItemTextFormat.md` section 4, `StatFilterBuilding.md`), but simplified: this
//! project's `StatCatalog` is trade-API-sourced only (no local `stats.ndjson` with per-matcher
//! `negate`/`legacy` annotations). Only some stats carry both an `increased` and a `reduced`
//! catalog text; the sign flip for the rest happens where a line resolves
//! (`catalog_match.rs`), not here.

/// One numeric run found in a stat line: `value(min-max)` or a bare `value` (a Unique-item fixed
/// stat with no roll range prints just the number, no parens -- treated as `min == max == value`).
/// `span` is the byte range in the source line this run occupied, for template substitution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumericRun {
    pub span: (usize, usize),
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
        regex::Regex::new(r"([+-]?\d+(?:\.\d+)?)(?:\(([+-]?\d+(?:\.\d+)?)-([+-]?\d+(?:\.\d+)?)\))?")
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
        let (min, max, dp) = match (caps.get(2), caps.get(3)) {
            (Some(min_m), Some(max_m)) => {
                let min: f64 = min_m.as_str().parse().expect("regex guarantees numeric");
                let max: f64 = max_m.as_str().parse().expect("regex guarantees numeric");
                let dp = dp || min_m.as_str().contains('.') || max_m.as_str().contains('.');
                if min > max {
                    // "reduced"-phrased stats sometimes print their bound in descending order,
                    // e.g. `14(15-10)%` -- swap, do not treat as an error.
                    (max, min, dp)
                } else {
                    (min, max, dp)
                }
            }
            _ => (value, value, dp),
        };
        runs.push(NumericRun {
            span: (whole.start(), whole.end()),
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
/// templates occasionally need when one number is baked in and the rest are roll placeholders.
/// `runs.len()` is almost always 0-2 in practice, so the `2^N` combinations this enumerates are
/// always cheap.
pub fn template_candidates(line: &str, runs: &[NumericRun]) -> Vec<String> {
    if runs.is_empty() {
        return vec![line.to_string()];
    }
    let n = runs.len();
    let mut candidates = Vec::with_capacity(1 << n);
    for mask in 0..(1u32 << n) {
        let mut out = line.to_string();
        for (i, run) in runs.iter().enumerate().rev() {
            if mask & (1 << i) != 0 {
                // keep literal: leave the source text at this span untouched.
                continue;
            }
            out.replace_range(run.span.0..run.span.1, "#");
        }
        candidates.push(out);
    }
    // Fully-templated (mask 0) is already first; de-duplicate in case a line has no digits
    // besides `#`-adjacent ones that coincide across masks (harmless either way, just avoids
    // redundant catalog lookups).
    candidates.dedup();
    candidates
}
