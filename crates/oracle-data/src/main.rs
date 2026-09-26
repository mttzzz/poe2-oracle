//! Makes the game data pack of this checkout's tables, for `.github/workflows/data-release.yml`:
//!
//! ```text
//! oracle-data build <out-dir> [--version <YYYYMMDDNN>]
//! ```
//!
//! writes `<out-dir>/PoE2-Oracle-Data-<version>.zip` and `<out-dir>/SHA256SUMS`, which
//! `release-sign sign` then signs. The version and `min_app` are `data-version.txt`'s. It refuses
//! tables that changed since their digest was recorded there (a data version names one set of
//! tables), and with `--version`, the release tag's version, one that isn't `data-version.txt`'s.
//! The tables are compiled in, so the pack is exactly the checkout the tool was built from.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use oracle_data::{BuiltIn, TABLES};
use oracle_protocol::{DATA_FILES, DataVersion, SUMS_ASSET, data_pack_asset};

const USAGE: &str = "usage: oracle-data build <out-dir> [--version <YYYYMMDDNN>]";

/// This checkout's tables, by the names a pack gives them.
const CHECKOUT_TABLES: [(&str, &str); TABLES] = [
    (
        "stat-matchers-en.tsv",
        include_str!("../../item-parser/data/stat-matchers-en.tsv"),
    ),
    (
        "stat-matchers-ru.tsv",
        include_str!("../../item-parser/data/stat-matchers-ru.tsv"),
    ),
    (
        "mod-tiers.tsv",
        include_str!("../../stat-filters/data/mod-tiers.tsv"),
    ),
    (
        "cx-items.tsv",
        include_str!("../../trade-client/data/cx-items.tsv"),
    ),
    (
        "item-refs.tsv",
        include_str!("../../poe2-oracle/assets/data/item-refs.tsv"),
    ),
];

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let (out, version) = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["build", out] => (out, None),
        ["build", out, "--version", version] | ["build", "--version", version, out] => {
            (out, Some(oracle_data::parse_data_version(version)?))
        }
        _ => bail!(USAGE),
    };
    let (pack, sums) = build(oracle_data::built_in(), version, Path::new(out))?;
    println!("{}\n{}", pack.display(), sums.display());
    Ok(())
}

/// Writes the pack of this checkout's tables and its `SHA256SUMS` into `out`, and returns their
/// paths. `tag` is the version the release is tagged with, when there is one.
fn build(built_in: &BuiltIn, tag: Option<DataVersion>, out: &Path) -> Result<(PathBuf, PathBuf)> {
    let names = CHECKOUT_TABLES.map(|(name, _)| name);
    ensure!(
        names == DATA_FILES,
        "the tool packs {names:?}, but a pack's tables are {DATA_FILES:?}"
    );
    let tables = CHECKOUT_TABLES.map(|(_, table)| table);
    check_recorded(built_in, &tables)?;
    if let Some(tag) = tag {
        ensure!(
            tag == built_in.version,
            "the tag names data {tag}, but data-version.txt names {}: tag the version it names",
            built_in.version
        );
    }
    let zip = oracle_data::build(built_in.version, &built_in.min_app, &tables)?;
    let name = data_pack_asset(built_in.version);
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let pack = out.join(&name);
    fs::write(&pack, &zip).with_context(|| format!("writing {}", pack.display()))?;
    let sums = out.join(SUMS_ASSET);
    fs::write(&sums, oracle_data::sums_line(&name, &zip))
        .with_context(|| format!("writing {}", sums.display()))?;
    Ok((pack, sums))
}

/// Whether `tables` are the ones `data-version.txt` recorded: a data version names one set of
/// tables, so changed ones need a version of their own.
fn check_recorded(built_in: &BuiltIn, tables: &[&str; TABLES]) -> Result<()> {
    let digest = oracle_data::digest(tables);
    ensure!(
        digest == built_in.digest,
        "the game tables changed since data {version} was recorded: in \
         crates/oracle-data/data-version.txt, set `version` to a new data version above {version} \
         (YYYYMMDDNN: today's date and that day's number, 01 first) and `tables` to {digest}",
        version = built_in.version,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_the_ones_their_data_version_was_recorded_for() {
        let tables = CHECKOUT_TABLES.map(|(_, table)| table);
        if let Err(err) = check_recorded(oracle_data::built_in(), &tables) {
            panic!("{err}");
        }
    }

    #[test]
    fn a_release_of_the_recorded_version_gets_its_pack_and_sums_and_another_gets_nothing() {
        let built_in = oracle_data::built_in();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("dist");

        let other = built_in.version + 1;
        assert!(build(built_in, Some(other), &out).is_err());
        assert!(!out.exists(), "a refused tag writes nothing");
        let changed = BuiltIn {
            digest: "0".repeat(64),
            ..built_in.clone()
        };
        assert!(build(&changed, None, &out).is_err(), "tables changed since");
        assert!(!out.exists());

        let (pack, sums) = build(built_in, Some(built_in.version), &out).unwrap();
        assert_eq!(pack, out.join(data_pack_asset(built_in.version)));
        let zip = fs::read(&pack).unwrap();
        let listed = fs::read_to_string(&sums).unwrap();
        let name = data_pack_asset(built_in.version);
        assert_eq!(listed, oracle_data::sums_line(&name, &zip));
        let read = oracle_data::unzip(&zip).unwrap();
        assert_eq!(read.manifest.version, built_in.version);
        assert_eq!(read.manifest.min_app, built_in.min_app);
        assert_eq!(read.tables, CHECKOUT_TABLES.map(|(_, table)| table));
    }
}
