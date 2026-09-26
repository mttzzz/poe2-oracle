//! The game data pack: the game tables the app carries built in (`oracle_protocol::DATA_FILES`
//! -- the stat matchers, the mod tiers, the exchange items and the item references), replaced
//! between app releases by newer ones the service hands out, signed like the app's releases. The
//! updater (`updates`) downloads a pack and [`install`]s it for the next start; at start,
//! [`activate`] puts the installed pack's tables in place of the built-in ones before anything
//! reads them. `crates/oracle-data` holds the pack's format; `data-version.txt` there, the
//! built-in tables' data version.
//!
//! On disk, in the local data folder (`paths::data_pack_dir`,
//! `%LOCALAPPDATA%\poe2-oracle\data\game-data`):
//! - `current`: the installed pack's data version, one line. Replacing this file installs a
//!   pack, in one step: a start reads the old pack or the new one, never a mix;
//! - `<version>\`: that pack, unpacked: `manifest.json` and the tables;
//! - `rejected\`: the last pack [`activate`] found damaged, moved aside as it was.
//!
//! Anything else there -- another version's folder, `current.tmp` -- is left from a replaced pack
//! or an interrupted install, and goes at the next start.

use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context as _, Result};
use item_parser::stat_forms::{self, StatFormsOf};
use oracle_data::{Pack, Refusal, TABLES, Version};
use oracle_protocol::{DATA_FILES, DataVersion};
use poe2_domain::pack_table::TableInUse;
use stat_filters::ModTiers;
use trade_client::cx::{self, ExchangeItems};

use crate::item_refs::{self, ItemRefs};
use crate::paths;

/// The file naming the installed pack.
const CURRENT: &str = "current";
/// `current`'s next text, before it replaces `current`.
const CURRENT_NEXT: &str = "current.tmp";
/// Where a damaged pack is moved aside.
const REJECTED: &str = "rejected";

/// The data version of the tables in use, once [`activate`] has run.
static ACTIVE: OnceLock<DataVersion> = OnceLock::new();

/// Puts the installed pack's tables in place of the built-in ones when the pack is sound and this
/// app's to use: of its format, newer than its built-in tables, naming no newer app, each table
/// the one its manifest names, and each parsing. Runs once, first thing at start: a table read
/// before it would keep its built-in rows for the run. A pack the app can't use is deleted -- an
/// older one after an update brought newer tables, one for a newer app after going back to an
/// older one -- and a damaged one moved aside; the built-in tables stay either way.
pub fn activate() {
    let store = Store::here();
    // The pack's text lives as long as the tables pointing into it: the rest of the run (and a
    // table that doesn't parse leaves its text behind, once, for a pack set aside).
    let loaded = store.load(|pack| read_tables(pack.tables.map(|table| &*table.leak())));
    let version = match loaded {
        Some((version, tables)) => match tables.put_in_place() {
            Ok(()) => {
                log::info!("game data {version}, from the installed pack");
                version
            }
            Err(err) => {
                log::error!("couldn't put game data {version} in place: {err}");
                store.built_in
            }
        },
        None => {
            log::info!("game data {}, built in", store.built_in);
            store.built_in
        }
    };
    let _ = ACTIVE.set(version);
}

/// The data version of the tables in use: the installed pack's [`activate`] put in place, else
/// the built-in tables'. It stays the same for the whole run.
pub fn active_version() -> DataVersion {
    ACTIVE
        .get()
        .copied()
        .unwrap_or_else(|| oracle_data::built_in().version)
}

/// Installs the pack zip at `zip`, a signature-checked download, for the next start, which
/// activates it. Checked the way [`activate`] checks the installed pack, on the files as written:
/// an error -- a pack no newer than the built-in tables, for a newer app, damaged, or with a
/// table that doesn't parse -- leaves the installed pack as it was. Installing the pack already
/// installed changes nothing.
pub fn install(zip: &Path) -> Result<DataVersion> {
    Store::here().install(zip)
}

/// A pack's tables, read.
struct Tables<'a> {
    stat_matchers_en: StatFormsOf<'a>,
    stat_matchers_ru: StatFormsOf<'a>,
    mod_tiers: ModTiers<'a>,
    exchange_items: ExchangeItems<'a>,
    item_refs: ItemRefs<'a>,
}

/// Reads a pack's tables, given in `DATA_FILES`' order. An error names the first table that
/// doesn't read.
fn read_tables<'a>(tables: [&'a str; TABLES]) -> Result<Tables<'a>, String> {
    let [
        stat_matchers_en,
        stat_matchers_ru,
        mod_tiers,
        exchange_items,
        refs,
    ] = tables;
    let named = |index: usize| move |err: String| format!("{}: {err}", DATA_FILES[index]);
    Ok(Tables {
        stat_matchers_en: stat_forms::read_table(stat_matchers_en).map_err(named(0))?,
        stat_matchers_ru: stat_forms::read_table(stat_matchers_ru).map_err(named(1))?,
        mod_tiers: stat_filters::read_mod_tiers(mod_tiers).map_err(named(2))?,
        exchange_items: cx::read_exchange_items(exchange_items).map_err(named(3))?,
        item_refs: item_refs::read_table(refs).map_err(named(4))?,
    })
}

impl Tables<'static> {
    /// Puts the tables in place of the built-in ones: refused for any table read already.
    fn put_in_place(self) -> Result<(), TableInUse> {
        stat_forms::use_tables(self.stat_matchers_en, self.stat_matchers_ru)?;
        stat_filters::use_mod_tiers(self.mod_tiers)?;
        cx::use_exchange_items(self.exchange_items)?;
        item_refs::use_table(self.item_refs)
    }
}

/// Where packs are installed, and which ones the app takes: [`Store::here`] for the app.
struct Store {
    root: PathBuf,
    /// The built-in tables' data version.
    built_in: DataVersion,
    /// The app's version.
    app: Version,
}

impl Store {
    fn here() -> Store {
        Store {
            root: paths::data_pack_dir(),
            built_in: oracle_data::built_in().version,
            app: Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo takes semver only"),
        }
    }

    fn pack_dir(&self, version: DataVersion) -> PathBuf {
        self.root.join(version.to_string())
    }

    /// The installed pack's version, as `current` names it.
    fn installed(&self) -> Option<DataVersion> {
        fs::read_to_string(self.root.join(CURRENT))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// The pack in `version`'s folder, checked as the next start takes it, up to parsing its
    /// tables.
    fn check(&self, version: DataVersion) -> Result<Pack, Refusal> {
        let pack = oracle_data::read_dir(&self.pack_dir(version))?;
        if pack.manifest.version != version {
            return Err(Refusal::Damaged(format!(
                "its manifest names data {}",
                pack.manifest.version
            )));
        }
        oracle_data::check_usable(&pack.manifest, self.built_in, &self.app)?;
        Ok(pack)
    }

    /// At start: the installed pack's version and its tables as `read` reads them, when the pack
    /// is sound and the app's to use. A pack it can't use is deleted, a damaged one moved aside.
    fn load<T>(&self, read: impl FnOnce(Pack) -> Result<T, String>) -> Option<(DataVersion, T)> {
        let Some(version) = self.installed() else {
            self.tidy(None);
            return None;
        };
        let checked = self.check(version);
        match checked.and_then(|pack| read(pack).map_err(Refusal::Damaged)) {
            Ok(tables) => {
                self.tidy(Some(version));
                Some((version, tables))
            }
            Err(Refusal::NotForThisApp(why)) => {
                log::info!("deleting the installed game data pack {version}: {why}");
                self.tidy(None);
                None
            }
            Err(Refusal::Damaged(why)) => {
                log::warn!(
                    "setting the installed game data pack {version} aside in {}: {why}",
                    self.root.join(REJECTED).display()
                );
                self.move_aside(version);
                self.tidy(None);
                None
            }
        }
    }

    fn install(&self, zip: &Path) -> Result<DataVersion> {
        let bytes = fs::read(zip).with_context(|| format!("reading {}", zip.display()))?;
        let pack = oracle_data::unzip(&bytes)?;
        let version = pack.manifest.version;
        oracle_data::check_usable(&pack.manifest, self.built_in, &self.app)?;
        if self.installed() == Some(version) {
            return Ok(version);
        }
        let dir = self.pack_dir(version);
        // A folder of this version no `current` names is left from an interrupted install.
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| format!("deleting {}", dir.display()))?;
        }
        pack.write_to(&dir)
            .with_context(|| format!("writing {}", dir.display()))?;
        // Read back from the disk and parsed, as the next start will.
        let written = self.check(version).and_then(|pack| {
            read_tables(pack.tables.each_ref().map(String::as_str))
                .map(drop)
                .map_err(Refusal::Damaged)
        });
        if let Err(refusal) = written {
            let _ = fs::remove_dir_all(&dir);
            return Err(refusal).with_context(|| format!("game data pack {version}"));
        }
        self.point_to(version)
            .with_context(|| format!("installing game data pack {version}"))?;
        self.tidy(Some(version));
        log::info!("installed game data pack {version} for the next start");
        Ok(version)
    }

    /// Makes `version` the installed pack: `current` replaced in one step.
    fn point_to(&self, version: DataVersion) -> io::Result<()> {
        let next = self.root.join(CURRENT_NEXT);
        let mut file = File::create(&next)?;
        writeln!(file, "{version}")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&next, self.root.join(CURRENT))
    }

    /// Moves `version`'s folder to `rejected`, in place of the pack set aside before.
    fn move_aside(&self, version: DataVersion) {
        let dir = self.pack_dir(version);
        if !dir.is_dir() {
            return;
        }
        let rejected = self.root.join(REJECTED);
        let _ = fs::remove_dir_all(&rejected);
        if let Err(err) = fs::rename(&dir, &rejected) {
            log::warn!("couldn't set {} aside: {err}", dir.display());
        }
    }

    /// Deletes what no start will read: every pack but `keep` -- and `current` too without one --
    /// and whatever an interrupted install left. The pack set aside stays.
    fn tidy(&self, keep: Option<DataVersion>) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let keep = keep.map(|version| version.to_string());
        for entry in entries.flatten() {
            let name = entry.file_name();
            let kept = name == REJECTED
                || keep
                    .as_deref()
                    .is_some_and(|keep| name == CURRENT || name == keep);
            if kept {
                continue;
            }
            let path = entry.path();
            let deleted = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            if let Err(err) = deleted {
                log::warn!("couldn't delete {}: {err}", path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// This checkout's tables, in `DATA_FILES`' order.
    const CHECKOUT: [&str; TABLES] = [
        include_str!("../../item-parser/data/stat-matchers-en.tsv"),
        include_str!("../../item-parser/data/stat-matchers-ru.tsv"),
        include_str!("../../stat-filters/data/mod-tiers.tsv"),
        include_str!("../../trade-client/data/cx-items.tsv"),
        include_str!("../assets/data/item-refs.tsv"),
    ];

    /// Tables of a row each, which parse.
    const SMALL: [&str; TABLES] = [
        "#% increased Life\t\t\texplicit.stat_1\n",
        "#% повышение здоровья\t\t\texplicit.stat_1\n",
        "stat_1\tp\tjewel\ta\t10\tIncreasedLife1\t\t1:2\t0\n",
        "Metadata/Items/Currency/CurrencyModValues\tdivine\tCurrency\n",
        "gem\tHerald of Ice\tВестник льда\t\t\n",
    ];

    const BUILT_IN: DataVersion = 2026092601;
    const NEWER: DataVersion = 2026092701;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A test's pack folder, `game-data` in a fresh folder under the system temp dir that holds
    /// the zips too, removed on drop: the same hand-rolled scheme as `settings`' tests.
    struct Setup(PathBuf);

    impl Setup {
        fn new() -> Setup {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "poe2-oracle-data-pack-test-{}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            Setup(dir)
        }

        fn root(&self) -> PathBuf {
            self.0.join("game-data")
        }

        /// The store of an app `app` with built-in tables `built_in`.
        fn app(&self, built_in: DataVersion, app: &str) -> Store {
            Store {
                root: self.root(),
                built_in,
                app: Version::parse(app).unwrap(),
            }
        }

        /// The zip of a pack of `tables`, data `version` for apps from `min_app`.
        fn zip(&self, version: DataVersion, min_app: &str, tables: &[&str; TABLES]) -> PathBuf {
            let path = self.0.join(format!("{version}-{min_app}.zip"));
            fs::write(&path, oracle_data::build(version, min_app, tables).unwrap()).unwrap();
            path
        }

        fn has(&self, name: &str) -> bool {
            self.root().join(name).exists()
        }
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// What a start takes: the installed pack's version once each of its tables parsed.
    fn start(store: &Store) -> Option<DataVersion> {
        let loaded =
            store.load(|pack| read_tables(pack.tables.each_ref().map(String::as_str)).map(drop));
        loaded.map(|(version, ())| version)
    }

    #[test]
    fn a_pack_installed_is_the_one_the_next_starts_take() {
        let setup = Setup::new();
        let store = setup.app(BUILT_IN, "0.1.0");
        assert_eq!(
            start(&store),
            None,
            "nothing installed: the built-in tables"
        );

        let zip = setup.zip(NEWER, "0.1.0", &CHECKOUT);
        assert_eq!(store.install(&zip).unwrap(), NEWER);
        assert_eq!(store.install(&zip).unwrap(), NEWER, "installed already");
        let loaded = store.load(|pack| {
            read_tables(pack.tables.each_ref().map(String::as_str)).map(drop)?;
            Ok(pack.tables)
        });
        let (version, tables) = loaded.expect("the installed pack");
        assert_eq!(version, NEWER);
        assert_eq!(tables, CHECKOUT);
        assert_eq!(start(&store), Some(NEWER), "and at every start after");

        // A newer pack takes its place, and the old one's folder goes.
        store
            .install(&setup.zip(NEWER + 1, "0.1.0", &SMALL))
            .unwrap();
        assert_eq!(start(&store), Some(NEWER + 1));
        assert!(!setup.has(&NEWER.to_string()));
    }

    #[test]
    fn an_install_cut_short_leaves_the_installed_pack() {
        let setup = Setup::new();
        let store = setup.app(BUILT_IN, "0.1.0");
        store.install(&setup.zip(NEWER, "0.1.0", &SMALL)).unwrap();
        // The next pack half written, `current` not yet replaced.
        let next = setup.root().join((NEWER + 1).to_string());
        fs::create_dir_all(&next).unwrap();
        fs::write(next.join("manifest.json"), "{").unwrap();
        fs::write(setup.root().join(CURRENT_NEXT), "20260927").unwrap();

        assert_eq!(start(&store), Some(NEWER));
        assert!(!next.exists() && !setup.has(CURRENT_NEXT));
    }

    #[test]
    fn a_damaged_pack_is_set_aside_for_the_built_in_tables() {
        let setup = Setup::new();
        let store = setup.app(BUILT_IN, "0.1.0");
        store.install(&setup.zip(NEWER, "0.1.0", &SMALL)).unwrap();
        // A table changed since it was installed: its SHA-256 isn't the manifest's any more.
        let table = setup.root().join(NEWER.to_string()).join("mod-tiers.tsv");
        fs::write(
            &table,
            "stat_1\tp\tjewel\ta\t10\tIncreasedLife1\t\t1:3\t0\n",
        )
        .unwrap();

        assert_eq!(start(&store), None);
        assert!(setup.has("rejected/mod-tiers.tsv"), "kept as it was found");
        assert!(!setup.has(CURRENT) && !setup.has(&NEWER.to_string()));
        assert_eq!(start(&store), None);
    }

    #[test]
    fn a_pack_with_a_table_that_does_not_parse_is_refused() {
        let setup = Setup::new();
        let store = setup.app(BUILT_IN, "0.1.0");
        let mut tables = SMALL;
        tables[2] = "stat_1\tp\tjewel\ta\t10\tIncreasedLife1\t\t1:2\n";
        let zip = setup.zip(NEWER, "0.1.0", &tables);
        let err = store.install(&zip).unwrap_err();
        assert!(format!("{err:#}").contains("mod-tiers.tsv"), "{err:#}");
        assert!(!setup.has(CURRENT) && !setup.has(&NEWER.to_string()));

        // Installed by an app whose parser took it, at the start of one whose parser doesn't.
        let pack = oracle_data::unzip(&fs::read(&zip).unwrap()).unwrap();
        pack.write_to(&store.pack_dir(NEWER)).unwrap();
        store.point_to(NEWER).unwrap();
        assert_eq!(start(&store), None);
        assert!(setup.has("rejected/manifest.json"));
        assert!(!setup.has(CURRENT));
    }

    #[test]
    fn a_pack_no_newer_than_the_built_in_tables_is_refused_and_deleted_after_an_update() {
        let setup = Setup::new();
        let store = setup.app(BUILT_IN, "0.1.0");
        for version in [BUILT_IN, BUILT_IN - 1] {
            assert!(store.install(&setup.zip(version, "0.1.0", &SMALL)).is_err());
        }
        assert!(!setup.has(CURRENT));

        // Installed, and then an app update brings tables as new as the pack's.
        store.install(&setup.zip(NEWER, "0.1.0", &SMALL)).unwrap();
        let updated = setup.app(NEWER, "0.2.0");
        assert_eq!(start(&updated), None);
        assert!(!setup.has(CURRENT) && !setup.has(&NEWER.to_string()));
        assert!(!setup.has(REJECTED), "deleted, not set aside");
    }

    #[test]
    fn a_pack_for_a_newer_app_is_refused_and_deleted() {
        let setup = Setup::new();
        let store = setup.app(BUILT_IN, "0.1.0");
        let zip = setup.zip(NEWER, "0.2.0", &SMALL);
        assert!(store.install(&zip).is_err());
        assert!(!setup.has(CURRENT));

        // Installed by 0.2.0, found by 0.1.0 after going back to it.
        setup.app(BUILT_IN, "0.2.0").install(&zip).unwrap();
        assert_eq!(start(&store), None);
        assert!(!setup.has(CURRENT) && !setup.has(&NEWER.to_string()));
        assert!(!setup.has(REJECTED), "deleted, not set aside");
    }
}
