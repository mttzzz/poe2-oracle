//! The game data pack: the tables PoE2 Oracle carries built in ([`DATA_FILES`] -- the stat
//! matchers of `item-parser`, the mod tiers of `stat-filters`, the exchange items of
//! `trade-client` and the app's item references) as one zip, which the service hands out between
//! app releases, signed like them ([`oracle_protocol::LATEST_DATA_PATH`]). [`build`] makes one
//! -- the `oracle-data` tool builds this checkout's for `.github/workflows/data-release.yml` --
//! and [`unzip`] and [`read_dir`] read one back, refusing a pack whose files aren't exactly the
//! tables its manifest names. The app (`poe2_oracle::data_pack`) parses the tables and installs
//! and activates packs.
//!
//! `data-version.txt` records the built-in tables ([`built_in`]): their data version, the oldest
//! app that reads them and their [`digest`]. An app uses a pack only when it is newer than its
//! own tables and names no newer app than it ([`check_usable`]).

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Cursor, Read as _, Write as _};
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context as _, Result, bail, ensure};
use oracle_protocol::{DATA_FILES, DATA_FORMAT, DATA_MANIFEST, DataManifest, DataVersion};
pub use semver::Version;
use sha2::{Digest as _, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// How many tables a pack carries.
pub const TABLES: usize = DATA_FILES.len();

/// The most a pack's file may hold when read back from a zip: each table is under a megabyte.
const MAX_FILE_BYTES: u64 = 32 << 20;

/// The tables built into this build, as `data-version.txt` records them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltIn {
    /// Their data version: a pack must be newer to replace them.
    pub version: DataVersion,
    /// The oldest app version that reads them: what a pack of them names as its `min_app`.
    pub min_app: String,
    /// Their [`digest`].
    pub digest: String,
}

static BUILT_IN: LazyLock<BuiltIn> = LazyLock::new(|| {
    BuiltIn::parse(include_str!("../data-version.txt")).expect("data-version.txt is well-formed")
});

/// The tables this build carries.
pub fn built_in() -> &'static BuiltIn {
    &BUILT_IN
}

impl BuiltIn {
    /// Reads `data-version.txt`: `version <YYYYMMDDNN>`, `min_app <semver>` and
    /// `tables <digest>`, a line each; a line starting with `#` is a comment.
    pub fn parse(text: &str) -> Result<BuiltIn> {
        let mut version = None;
        let mut min_app = None;
        let mut digest = None;
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let number = number + 1;
            let (key, value) = line
                .split_once(' ')
                .map(|(key, value)| (key, value.trim()))
                .with_context(|| format!("line {number}: {line} has no value"))?;
            let first = match key {
                "version" => {
                    let value =
                        parse_data_version(value).with_context(|| format!("line {number}"))?;
                    version.replace(value).is_none()
                }
                "min_app" => {
                    Version::parse(value).with_context(|| {
                        format!("line {number}: min_app {value} isn't a version")
                    })?;
                    min_app.replace(value.to_owned()).is_none()
                }
                "tables" => {
                    ensure!(
                        is_sha256(value),
                        "line {number}: tables {value} isn't a SHA-256 in lowercase hex"
                    );
                    digest.replace(value.to_owned()).is_none()
                }
                _ => bail!("line {number}: no such key as {key}"),
            };
            ensure!(first, "line {number}: {key} a second time");
        }
        Ok(BuiltIn {
            version: version.context("no version")?,
            min_app: min_app.context("no min_app")?,
            digest: digest.context("no tables")?,
        })
    }
}

/// Reads a data version, `YYYYMMDDNN`: ten digits naming a month and a day that exist, and that
/// day's number from 01.
pub fn parse_data_version(text: &str) -> Result<DataVersion> {
    ensure!(
        text.len() == 10 && text.bytes().all(|byte| byte.is_ascii_digit()),
        "{text} isn't a data version, YYYYMMDDNN"
    );
    let version: DataVersion = text.parse()?;
    let (month, day, number) = (version / 10_000 % 100, version / 100 % 100, version % 100);
    ensure!(
        (1..=12).contains(&month) && (1..=31).contains(&day) && number >= 1,
        "{text} isn't a data version, YYYYMMDDNN: there's no such month, day or number"
    );
    Ok(version)
}

/// What `data-version.txt` records for `tables`, given in [`DATA_FILES`]' order: the SHA-256 of
/// their `sha256sum` listing, `<SHA-256>  <name>` a line each, in that order.
pub fn digest(tables: &[&str; TABLES]) -> String {
    let listing: String = DATA_FILES
        .iter()
        .zip(tables)
        .map(|(name, table)| sums_line(name, table.as_bytes()))
        .collect();
    sha256_hex(listing.as_bytes())
}

/// `bytes`' line in a `SHA256SUMS` (`sha256sum`'s format), under `name`.
pub fn sums_line(name: &str, bytes: &[u8]) -> String {
    format!("{}  {name}\n", sha256_hex(bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Makes a pack of `tables`, given in [`DATA_FILES`]' order, whose manifest names `version` and
/// `min_app`. The same arguments give the same bytes whenever and wherever it runs: the entries
/// in name order, each dated 1980-01-01 (the zip format's first day), with the same permissions,
/// deflated alike.
pub fn build(version: DataVersion, min_app: &str, tables: &[&str; TABLES]) -> Result<Vec<u8>> {
    Version::parse(min_app).with_context(|| format!("min_app {min_app} isn't a version"))?;
    let manifest = manifest_json(&DataManifest {
        format: DATA_FORMAT,
        version,
        min_app: min_app.to_owned(),
        files: DATA_FILES
            .iter()
            .zip(tables)
            .map(|(name, table)| ((*name).to_owned(), sha256_hex(table.as_bytes())))
            .collect(),
    });
    let mut entries: Vec<(&str, &[u8])> = DATA_FILES
        .iter()
        .zip(tables)
        .map(|(name, table)| (*name, table.as_bytes()))
        .collect();
    entries.push((DATA_MANIFEST, &manifest));
    entries.sort_unstable_by_key(|&(name, _)| name);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::DEFAULT)
        .unix_permissions(0o644);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn manifest_json(manifest: &DataManifest) -> Vec<u8> {
    let mut json = serde_json::to_vec_pretty(manifest).expect("a manifest is plain data");
    json.push(b'\n');
    json
}

/// A pack as read back ([`unzip`], [`read_dir`]).
#[derive(Debug)]
pub struct Pack {
    pub manifest: DataManifest,
    /// The tables in [`DATA_FILES`]' order, each the one the manifest names.
    pub tables: [String; TABLES],
}

impl Pack {
    /// Writes the pack into `dir` as [`read_dir`] reads it, the manifest and the tables, each
    /// file flushed to the disk.
    pub fn write_to(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let manifest = manifest_json(&self.manifest);
        let tables = DATA_FILES.iter().zip(&self.tables);
        let files = tables
            .map(|(name, table)| (*name, table.as_bytes()))
            .chain([(DATA_MANIFEST, manifest.as_slice())]);
        for (name, bytes) in files {
            let mut file = File::create(dir.join(name))?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Ok(())
    }
}

/// Why a pack was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Sound, but not for this app: of another format than [`DATA_FORMAT`], or not usable by it
    /// ([`check_usable`]).
    NotForThisApp(String),
    /// Unreadable, or not the tables its manifest names: one missing, extra, changed or not text.
    Damaged(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NotForThisApp(why) => write!(f, "not for this app: {why}"),
            Refusal::Damaged(why) => write!(f, "damaged: {why}"),
        }
    }
}

impl std::error::Error for Refusal {}

fn damaged(why: impl fmt::Display) -> Refusal {
    Refusal::Damaged(why.to_string())
}

/// Reads a pack zip. Refused: a manifest of another format, or listing other files than the
/// tables; a zip holding anything but the manifest and the tables; a table whose SHA-256 isn't
/// the manifest's, or that isn't UTF-8 text.
pub fn unzip(zip: &[u8]) -> Result<Pack, Refusal> {
    let mut archive =
        ZipArchive::new(Cursor::new(zip)).map_err(|err| damaged(format!("not a zip: {err}")))?;
    // The manifest first: another format's pack may hold other files.
    let manifest = read_manifest(&entry(&mut archive, DATA_MANIFEST)?)?;
    let names: BTreeSet<&str> = archive.file_names().collect();
    let expected: BTreeSet<&str> = DATA_FILES.into_iter().chain([DATA_MANIFEST]).collect();
    if names != expected {
        return Err(damaged(format!(
            "the zip holds {names:?}, not {expected:?}"
        )));
    }
    let tables = read_tables(&manifest, |name| entry(&mut archive, name))?;
    Ok(Pack { manifest, tables })
}

/// Reads a pack [`Pack::write_to`] wrote into `dir`, refused as [`unzip`] refuses one.
pub fn read_dir(dir: &Path) -> Result<Pack, Refusal> {
    let read =
        |name: &str| fs::read(dir.join(name)).map_err(|err| damaged(format!("{name}: {err}")));
    let manifest = read_manifest(&read(DATA_MANIFEST)?)?;
    let tables = read_tables(&manifest, read)?;
    Ok(Pack { manifest, tables })
}

fn entry(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, Refusal> {
    let file = archive
        .by_name(name)
        .map_err(|err| damaged(format!("{name}: {err}")))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| damaged(format!("{name}: {err}")))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(damaged(format!("{name} is over {MAX_FILE_BYTES} bytes")));
    }
    Ok(bytes)
}

/// The manifest in `bytes`: of this format, listing exactly the tables.
fn read_manifest(bytes: &[u8]) -> Result<DataManifest, Refusal> {
    let unreadable = |err: serde_json::Error| damaged(format!("{DATA_MANIFEST}: {err}"));
    let json: serde_json::Value = serde_json::from_slice(bytes).map_err(unreadable)?;
    // The format first: another format's manifest needn't have this one's fields.
    match json.get("format").and_then(serde_json::Value::as_u64) {
        Some(format) if format == u64::from(DATA_FORMAT) => {}
        Some(format) => {
            return Err(Refusal::NotForThisApp(format!(
                "the pack is of format {format}, and this app reads format {DATA_FORMAT}"
            )));
        }
        None => return Err(damaged(format!("{DATA_MANIFEST} names no format"))),
    }
    let manifest: DataManifest = serde_json::from_value(json).map_err(unreadable)?;
    let listed: BTreeSet<&str> = manifest.files.keys().map(String::as_str).collect();
    let tables: BTreeSet<&str> = DATA_FILES.into_iter().collect();
    if listed != tables {
        return Err(damaged(format!(
            "{DATA_MANIFEST} lists {listed:?}, not the tables {tables:?}"
        )));
    }
    Ok(manifest)
}

/// Each table `read` gives, in [`DATA_FILES`]' order: the one `manifest` names, and text.
fn read_tables(
    manifest: &DataManifest,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, Refusal>,
) -> Result<[String; TABLES], Refusal> {
    let mut tables: [String; TABLES] = Default::default();
    for (name, table) in DATA_FILES.into_iter().zip(&mut tables) {
        let bytes = read(name)?;
        if manifest.files[name] != sha256_hex(&bytes) {
            return Err(damaged(format!(
                "{name} isn't the table {DATA_MANIFEST} names: its SHA-256 differs"
            )));
        }
        *table = String::from_utf8(bytes).map_err(|_| damaged(format!("{name} isn't text")))?;
    }
    Ok(tables)
}

/// Whether an app of version `app`, whose built-in tables are data `built_in`, uses a pack of
/// this manifest: one newer than its own tables, and naming no newer app as its `min_app`.
pub fn check_usable(
    manifest: &DataManifest,
    built_in: DataVersion,
    app: &Version,
) -> Result<(), Refusal> {
    if manifest.version <= built_in {
        return Err(Refusal::NotForThisApp(format!(
            "data {} isn't newer than the built-in {built_in}",
            manifest.version
        )));
    }
    let min_app = Version::parse(&manifest.min_app).map_err(|_| {
        damaged(format!(
            "{DATA_MANIFEST}: min_app {:?} isn't a version",
            manifest.min_app
        ))
    })?;
    if min_app > *app {
        return Err(Refusal::NotForThisApp(format!(
            "data {} needs app {min_app} or later, and this is {app}",
            manifest.version
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXTS: [&str; TABLES] = ["en\n", "ru\n", "tiers\n", "cx\n", "refs\n"];

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    /// A zip of `entries` as given, whatever they are.
    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// A manifest of this format listing `files`' SHA-256s.
    fn manifest_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        manifest_json(&DataManifest {
            format: DATA_FORMAT,
            version: 2026092701,
            min_app: "0.1.0".to_owned(),
            files: files
                .iter()
                .map(|(name, bytes)| ((*name).to_owned(), sha256_hex(bytes)))
                .collect(),
        })
    }

    #[test]
    fn a_pack_reads_back_as_built_and_builds_the_same_every_time() {
        let zip = build(2026092701, "0.1.0", &TEXTS).unwrap();
        let pack = unzip(&zip).unwrap();
        assert_eq!(pack.tables, TEXTS);
        assert_eq!(pack.manifest.format, DATA_FORMAT);
        assert_eq!(pack.manifest.version, 2026092701);
        assert_eq!(pack.manifest.min_app, "0.1.0");

        // Nothing of the day or the machine in it: entries in name order, each dated the zip
        // format's first day with the same permissions.
        let mut archive = ZipArchive::new(Cursor::new(&zip[..])).unwrap();
        let names: Vec<&str> = archive.file_names().collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        for index in 0..archive.len() {
            let file = archive.by_index(index).unwrap();
            assert_eq!(
                file.last_modified(),
                Some(DateTime::DEFAULT),
                "{}",
                file.name()
            );
            assert_eq!(file.unix_mode().map(|mode| mode & 0o777), Some(0o644));
        }

        // Unpacked, it reads back the same.
        let dir = tempfile::tempdir().unwrap();
        pack.write_to(dir.path()).unwrap();
        let unpacked = read_dir(dir.path()).unwrap();
        assert_eq!(unpacked.tables, TEXTS);
        assert_eq!(unpacked.manifest, pack.manifest);
    }

    #[test]
    fn a_pack_whose_files_are_not_the_tables_its_manifest_names_is_damaged() {
        let is_damaged = |result: Result<Pack, Refusal>| matches!(result, Err(Refusal::Damaged(_)));
        let tables: Vec<(&str, &[u8])> = DATA_FILES
            .into_iter()
            .zip(TEXTS.map(str::as_bytes))
            .collect();
        let manifest = manifest_of(&tables);
        let mut sound = tables.clone();
        sound.push((DATA_MANIFEST, &manifest));
        assert!(unzip(&zip_of(&sound)).is_ok());

        // A file besides the tables.
        let mut extra = sound.clone();
        extra.push(("notes.txt", b"hi"));
        assert!(is_damaged(unzip(&zip_of(&extra))));
        // A table missing.
        let mut missing = sound.clone();
        missing.retain(|(name, _)| *name != "mod-tiers.tsv");
        assert!(is_damaged(unzip(&zip_of(&missing))));
        // A table changed since the manifest was made.
        let mut changed = sound.clone();
        changed[2].1 = b"tiers, edited\n";
        assert!(is_damaged(unzip(&zip_of(&changed))));
        // A table that isn't text, though the manifest names it.
        let mut binary = tables.clone();
        binary[3].1 = &[0xff, 0xfe, 0x00];
        let binary_manifest = manifest_of(&binary);
        binary.push((DATA_MANIFEST, &binary_manifest));
        assert!(is_damaged(unzip(&zip_of(&binary))));
        // No manifest, or not a zip at all.
        assert!(is_damaged(unzip(&zip_of(&tables))));
        assert!(is_damaged(unzip(b"PK, but no")));

        // Unpacked, a table changed on the disk.
        let dir = tempfile::tempdir().unwrap();
        unzip(&zip_of(&sound))
            .unwrap()
            .write_to(dir.path())
            .unwrap();
        fs::write(dir.path().join("cx-items.tsv"), "cx, edited\n").unwrap();
        assert!(is_damaged(read_dir(dir.path())));
    }

    #[test]
    fn a_pack_of_another_format_is_not_for_this_app_whatever_it_holds() {
        let mut manifest: serde_json::Value = serde_json::from_slice(&manifest_of(&[])).unwrap();
        manifest["format"] = (DATA_FORMAT + 1).into();
        manifest.as_object_mut().unwrap().remove("files");
        let manifest = manifest.to_string();
        let zip = zip_of(&[
            (DATA_MANIFEST, manifest.as_bytes()),
            ("new-table.tsv", b"new\n"),
        ]);
        assert!(matches!(unzip(&zip), Err(Refusal::NotForThisApp(_))));
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(DATA_MANIFEST), &manifest).unwrap();
        assert!(matches!(
            read_dir(dir.path()),
            Err(Refusal::NotForThisApp(_))
        ));
    }

    #[test]
    fn an_app_uses_only_a_newer_pack_that_names_no_newer_app() {
        let manifest = |data, min_app: &str| DataManifest {
            format: DATA_FORMAT,
            version: data,
            min_app: min_app.to_owned(),
            files: Default::default(),
        };
        let app = version("0.2.0");
        let usable = |manifest: &DataManifest| check_usable(manifest, 2026092601, &app);
        assert_eq!(usable(&manifest(2026092602, "0.2.0")), Ok(()));
        assert_eq!(usable(&manifest(2026100101, "0.1.0")), Ok(()));
        for (manifest, why) in [
            (
                manifest(2026092601, "0.1.0"),
                "the built-in tables' own version",
            ),
            (
                manifest(2026092501, "0.1.0"),
                "older than the built-in tables",
            ),
            (manifest(2026092602, "0.2.1"), "for a newer app"),
            (
                manifest(2026092602, "0.3.0-rc.1"),
                "for a newer app's pre-release",
            ),
        ] {
            assert!(
                matches!(usable(&manifest), Err(Refusal::NotForThisApp(_))),
                "{why}"
            );
        }
        assert!(matches!(
            usable(&manifest(2026092602, "next")),
            Err(Refusal::Damaged(_))
        ));
    }

    #[test]
    fn data_version_txt_is_read_only_when_well_formed() {
        // This build's own, which the app and the tool read.
        let built_in = built_in();
        assert!(parse_data_version(&built_in.version.to_string()).is_ok());

        let file = |version: &str| {
            format!(
                "# comment\nversion {version}\nmin_app 0.1.0\ntables {}\n",
                "0".repeat(64)
            )
        };
        assert_eq!(
            BuiltIn::parse(&file("2026092601")).unwrap().version,
            2026092601
        );
        for version in [
            "202609261",
            "20260926011",
            "2026132601",
            "2026093201",
            "2026092600",
        ] {
            assert!(BuiltIn::parse(&file(version)).is_err(), "{version}");
        }
        let good = file("2026092601");
        for bad in [
            good.replace("min_app 0.1.0", "min_app one"),
            good.replace(&"0".repeat(64), &"0".repeat(63)),
            good.replace("tables", "table"),
            good.replace("min_app 0.1.0\n", ""),
            format!("{good}version 2026092602\n"),
        ] {
            assert!(BuiltIn::parse(&bad).is_err(), "{bad}");
        }
    }
}
