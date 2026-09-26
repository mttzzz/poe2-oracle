//! What the updater (`crate::updates`) goes by, apart from Windows and GPUI. The app's service
//! announces the latest app and game data versions ([`Versions`]); [`next_fetch`] says which
//! update to fetch -- the app's first -- and [`Showing::quiet`] whether a fetched one may restart
//! the app now: only while none of its windows is up, so a restart never closes one under the
//! player.
//!
//! Before the restart the updater leaves a [`Marker`] in the app's data folder. The next start
//! takes it ([`Marker::take`]) and reads what came of the update ([`Marker::outcome`]): a plate
//! says it was updated -- or, when the version didn't change after all (the installer failed, the
//! pack wasn't loaded), that version is left alone for the rest of the run, instead of restarting
//! into the same failure again and again. A failed fetch is tried again after [`retry_delay`].

use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, Result};
use auto_update::Version;
use oracle_protocol::{DataVersion, Versions};
use serde::{Deserialize, Serialize};

/// An update: to a new version of the app, or of the game data it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    App(Version),
    Data(DataVersion),
}

/// The app version the service announced, when it's newer than `running` by semver precedence --
/// as `auto_update::check_for_update` compares, so `0.2.0` updates `0.2.0-rc.1` and nothing ever
/// downgrades. An announcement that isn't a version is never newer.
pub fn newer_app(announced: &str, running: &Version) -> Option<Version> {
    Version::parse(announced)
        .ok()
        .filter(|announced| announced.cmp_precedence(running).is_gt())
}

/// Which update to fetch, from the service's `latest` word: the app when a newer one is out, else
/// the game data when newer than the tables loaded (`active_data`). The app comes first: a new
/// app carries tables of its own, and a newer pack may need it. Nothing is fetched twice: not the
/// update already fetched and waiting for the restart (`ready`) -- nor, while an app update waits,
/// any data -- and not a version the last run's update to didn't take (`left_alone`).
pub fn next_fetch(
    latest: &Versions,
    running_app: &Version,
    active_data: DataVersion,
    ready: Option<&Target>,
    left_alone: &[Target],
) -> Option<Target> {
    let app = latest
        .app
        .as_deref()
        .and_then(|announced| newer_app(announced, running_app))
        .map(Target::App)
        .filter(|app| !left_alone.contains(app));
    if let Some(app) = app {
        return (ready != Some(&app)).then_some(app);
    }
    let data = latest
        .data
        .filter(|&data| data > active_data)
        .map(Target::Data)
        .filter(|data| !left_alone.contains(data))?;
    match (ready, &data) {
        (Some(Target::App(_)), _) => None,
        (Some(Target::Data(held)), Target::Data(announced)) if held >= announced => None,
        _ => Some(data),
    }
}

/// Whether the update fetched and waiting for the restart is still the one to install, now that
/// the service said `latest`: an app release it no longer announces -- withdrawn, or a newer one
/// out -- is dropped. A data pack stays: it's installed for the next start already, and a newer
/// one comes after the restart.
pub fn still_wanted(ready: &Target, latest: &Versions) -> bool {
    match ready {
        Target::App(version) => latest
            .app
            .as_deref()
            .and_then(|announced| Version::parse(announced).ok())
            .is_some_and(|announced| announced.cmp_precedence(version).is_eq()),
        Target::Data(_) => true,
    }
}

/// The app's windows that are up, any of which an update's restart would close under the player.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Showing {
    /// The price panel.
    pub panel: bool,
    /// The settings window, and the welcome inside it.
    pub settings: bool,
    /// The report window.
    pub report: bool,
    /// The pathofexile.com sign-in window.
    pub sign_in: bool,
    /// The tour, under way: its cards over the game or the app's windows.
    pub tour: bool,
}

impl Showing {
    /// Whether a fetched update may restart the app now: none of its windows is up.
    pub fn quiet(&self) -> bool {
        let Showing {
            panel,
            settings,
            report,
            sign_in,
            tour,
        } = *self;
        !(panel || settings || report || sign_in || tour)
    }
}

/// How long after its `failures`-th failure in a row a fetch is tried again: a minute, doubling
/// up to an hour.
pub fn retry_delay(failures: u32) -> Duration {
    const FIRST: Duration = Duration::from_secs(60);
    const LONGEST: Duration = Duration::from_secs(60 * 60);
    let doublings = failures.saturating_sub(1);
    FIRST
        .saturating_mul(1u32.checked_shl(doublings).unwrap_or(u32::MAX))
        .min(LONGEST)
}

/// What an update leaves for the next start, in the app's data folder: what it replaced, and from
/// which version to which.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Marker {
    /// The installer replaced the app: semver versions, no `v`.
    App { from: String, to: String },
    /// The app restarted to load a new game data pack.
    Data { from: DataVersion, to: DataVersion },
}

/// What came of the update a [`Marker`] marks, as the next start finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It took: this is what runs now.
    Updated(Target),
    /// It didn't: an older version runs -- the installer failed, or the pack wasn't loaded.
    Failed(Target),
    /// Something newer runs already, installed by hand since; or the marker names no version.
    Passed,
}

impl Marker {
    /// The marker for an update to `to`, away from what runs now.
    pub fn new(to: &Target, running_app: &Version, active_data: DataVersion) -> Marker {
        match to {
            Target::App(to) => Marker::App {
                from: running_app.to_string(),
                to: to.to_string(),
            },
            Target::Data(to) => Marker::Data {
                from: active_data,
                to: *to,
            },
        }
    }

    /// What came of the update, now that `running_app` runs with the game data `active_data`.
    pub fn outcome(&self, running_app: &Version, active_data: DataVersion) -> Outcome {
        match self {
            Marker::App { to, .. } => match Version::parse(to) {
                Ok(to) => match to.cmp_precedence(running_app) {
                    std::cmp::Ordering::Equal => Outcome::Updated(Target::App(to)),
                    std::cmp::Ordering::Greater => Outcome::Failed(Target::App(to)),
                    std::cmp::Ordering::Less => Outcome::Passed,
                },
                Err(_) => Outcome::Passed,
            },
            Marker::Data { to, .. } => match to.cmp(&active_data) {
                std::cmp::Ordering::Equal => Outcome::Updated(Target::Data(*to)),
                std::cmp::Ordering::Greater => Outcome::Failed(Target::Data(*to)),
                std::cmp::Ordering::Less => Outcome::Passed,
            },
        }
    }

    /// Writes the marker to `path`, the folder included.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let json = serde_json::to_vec(self).context("writing the update marker")?;
        fs::write(path, json).with_context(|| format!("writing {}", path.display()))
    }

    /// The marker left at `path`, taken: read and deleted, so what it says is said once. `None`
    /// without one; one that can't be read is deleted too.
    pub fn take(path: &Path) -> Option<Marker> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
            Err(err) => {
                log::warn!("reading {} failed: {err}", path.display());
                Vec::new()
            }
        };
        if let Err(err) = fs::remove_file(path) {
            log::warn!("deleting {} failed: {err}", path.display());
        }
        serde_json::from_slice(&bytes)
            .inspect_err(|err| log::warn!("the update marker isn't readable: {err}"))
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A fresh directory under the system temp dir, removed on drop: this crate has no `tempfile`
    /// dependency, the same hand-rolled scheme as `settings`' tests.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> TempDir {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            TempDir(std::env::temp_dir().join(format!(
                "poe2-oracle-update-test-{}-{n}",
                std::process::id()
            )))
        }

        /// Nested, so saving also has to create the folder.
        fn marker_file(&self) -> PathBuf {
            self.0.join("data").join("last-update.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn latest(app: Option<&str>, data: Option<DataVersion>) -> Versions {
        Versions {
            app: app.map(str::to_owned),
            data,
        }
    }

    const DATA: DataVersion = 2026092601;

    #[test]
    fn only_a_newer_app_version_is_an_update() {
        let running = version("0.1.0");
        assert_eq!(newer_app("0.1.1", &running), Some(version("0.1.1")));
        assert_eq!(newer_app("0.1.0", &running), None, "the same version");
        assert_eq!(newer_app("0.0.9", &running), None, "never a downgrade");
        assert_eq!(
            newer_app("0.1.1-rc.1", &running),
            Some(version("0.1.1-rc.1"))
        );
        assert_eq!(newer_app("v0.1.1", &running), None, "not a version");
        assert_eq!(newer_app("", &running), None);
    }

    #[test]
    fn a_release_updates_its_own_prereleases_but_not_the_other_way() {
        assert_eq!(
            newer_app("0.2.0", &version("0.2.0-rc.1")),
            Some(version("0.2.0"))
        );
        assert_eq!(newer_app("0.2.0-rc.2", &version("0.2.0")), None);
        assert_eq!(
            newer_app("0.1.0+build.7", &version("0.1.0")),
            None,
            "build metadata doesn't count"
        );
    }

    #[test]
    fn the_app_is_fetched_before_the_data() {
        let running = version("0.1.0");
        let both = latest(Some("0.1.1"), Some(DATA + 1));
        assert_eq!(
            next_fetch(&both, &running, DATA, None, &[]),
            Some(Target::App(version("0.1.1")))
        );
        let data_only = latest(Some("0.1.0"), Some(DATA + 1));
        assert_eq!(
            next_fetch(&data_only, &running, DATA, None, &[]),
            Some(Target::Data(DATA + 1))
        );
        let none_newer = latest(Some("0.1.0"), Some(DATA));
        assert_eq!(next_fetch(&none_newer, &running, DATA, None, &[]), None);
        let nothing_published = latest(None, None);
        assert_eq!(
            next_fetch(&nothing_published, &running, DATA, None, &[]),
            None
        );
        let older_data = latest(None, Some(DATA - 1));
        assert_eq!(next_fetch(&older_data, &running, DATA, None, &[]), None);
    }

    #[test]
    fn a_broken_app_announcement_leaves_the_data_to_update() {
        let announced = latest(Some("latest"), Some(DATA + 1));
        assert_eq!(
            next_fetch(&announced, &version("0.1.0"), DATA, None, &[]),
            Some(Target::Data(DATA + 1))
        );
    }

    #[test]
    fn an_update_waiting_for_the_restart_isnt_fetched_again() {
        let running = version("0.1.0");
        let app = Target::App(version("0.1.1"));
        let announced = latest(Some("0.1.1"), Some(DATA + 1));
        assert_eq!(
            next_fetch(&announced, &running, DATA, Some(&app), &[]),
            None
        );
        let newer = latest(Some("0.1.2"), None);
        assert_eq!(
            next_fetch(&newer, &running, DATA, Some(&app), &[]),
            Some(Target::App(version("0.1.2"))),
            "a newer release is fetched in its place"
        );

        let data = Target::Data(DATA + 1);
        let same = latest(None, Some(DATA + 1));
        assert_eq!(next_fetch(&same, &running, DATA, Some(&data), &[]), None);
        let newer_data = latest(None, Some(DATA + 2));
        assert_eq!(
            next_fetch(&newer_data, &running, DATA, Some(&data), &[]),
            Some(Target::Data(DATA + 2))
        );
        assert_eq!(
            next_fetch(&newer_data, &running, DATA, Some(&app), &[]),
            None,
            "the app's restart comes first"
        );
    }

    #[test]
    fn a_version_that_didnt_take_is_left_alone_for_the_run() {
        let running = version("0.1.0");
        let failed_app = [Target::App(version("0.1.1"))];
        assert_eq!(
            next_fetch(
                &latest(Some("0.1.1"), Some(DATA + 1)),
                &running,
                DATA,
                None,
                &failed_app
            ),
            Some(Target::Data(DATA + 1)),
            "the data goes on without the app"
        );
        assert_eq!(
            next_fetch(
                &latest(Some("0.1.2"), None),
                &running,
                DATA,
                None,
                &failed_app
            ),
            Some(Target::App(version("0.1.2"))),
            "a newer release is tried"
        );
        let failed_data = [Target::Data(DATA + 1)];
        assert_eq!(
            next_fetch(
                &latest(None, Some(DATA + 1)),
                &running,
                DATA,
                None,
                &failed_data
            ),
            None
        );
        assert_eq!(
            next_fetch(
                &latest(None, Some(DATA + 2)),
                &running,
                DATA,
                None,
                &failed_data
            ),
            Some(Target::Data(DATA + 2))
        );
    }

    #[test]
    fn a_fetched_release_the_service_no_longer_announces_is_dropped() {
        let ready = Target::App(version("0.1.1"));
        assert!(still_wanted(&ready, &latest(Some("0.1.1"), None)));
        assert!(
            !still_wanted(&ready, &latest(Some("0.1.2"), None)),
            "superseded"
        );
        assert!(
            !still_wanted(&ready, &latest(Some("0.1.0"), None)),
            "withdrawn"
        );
        assert!(
            !still_wanted(&ready, &latest(None, Some(DATA))),
            "withdrawn"
        );
        assert!(
            still_wanted(&Target::Data(DATA), &latest(None, None)),
            "a pack is installed already"
        );
    }

    #[test]
    fn a_restart_waits_until_no_window_is_up() {
        assert!(Showing::default().quiet());
        let windows: [fn(&mut Showing); 5] = [
            |showing| showing.panel = true,
            |showing| showing.settings = true,
            |showing| showing.report = true,
            |showing| showing.sign_in = true,
            |showing| showing.tour = true,
        ];
        for (index, show) in windows.iter().enumerate() {
            let mut showing = Showing::default();
            show(&mut showing);
            assert!(!showing.quiet(), "window {index} alone holds the restart");
        }
    }

    #[test]
    fn a_failed_fetch_waits_a_minute_doubling_to_an_hour() {
        let minutes = |failures| retry_delay(failures).as_secs() / 60;
        assert_eq!(minutes(0), 1);
        assert_eq!(minutes(1), 1);
        assert_eq!(minutes(2), 2);
        assert_eq!(minutes(3), 4);
        assert_eq!(minutes(6), 32);
        assert_eq!(minutes(7), 60);
        assert_eq!(minutes(40), 60, "no overflow past 32 doublings");
        assert_eq!(minutes(u32::MAX), 60);
    }

    #[test]
    fn a_marker_is_read_back_once() {
        let dir = TempDir::new();
        let path = dir.marker_file();
        assert_eq!(Marker::take(&path), None, "no update yet");
        for marker in [
            Marker::new(&Target::App(version("0.1.1")), &version("0.1.0"), DATA),
            Marker::new(&Target::Data(DATA + 1), &version("0.1.0"), DATA),
        ] {
            marker.save(&path).unwrap();
            assert_eq!(Marker::take(&path), Some(marker));
            assert!(!path.exists(), "taken");
            assert_eq!(Marker::take(&path), None);
        }
    }

    #[test]
    fn a_marker_that_cant_be_read_is_dropped() {
        let dir = TempDir::new();
        let path = dir.marker_file();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"kind": "app", "fro"#).unwrap();
        assert_eq!(Marker::take(&path), None);
        assert!(!path.exists());
    }

    #[test]
    fn a_marker_says_whether_its_update_took() {
        let app = Marker::App {
            from: "0.1.0".to_owned(),
            to: "0.1.1".to_owned(),
        };
        assert_eq!(
            app.outcome(&version("0.1.1"), DATA),
            Outcome::Updated(Target::App(version("0.1.1")))
        );
        assert_eq!(
            app.outcome(&version("0.1.0"), DATA),
            Outcome::Failed(Target::App(version("0.1.1"))),
            "the old version was started again"
        );
        assert_eq!(app.outcome(&version("0.1.2"), DATA), Outcome::Passed);
        let unreadable = Marker::App {
            from: "0.1.0".to_owned(),
            to: "next".to_owned(),
        };
        assert_eq!(unreadable.outcome(&version("0.1.0"), DATA), Outcome::Passed);

        let data = Marker::Data {
            from: DATA,
            to: DATA + 1,
        };
        let running = version("0.1.0");
        assert_eq!(
            data.outcome(&running, DATA + 1),
            Outcome::Updated(Target::Data(DATA + 1))
        );
        assert_eq!(
            data.outcome(&running, DATA),
            Outcome::Failed(Target::Data(DATA + 1)),
            "the pack wasn't loaded"
        );
        assert_eq!(data.outcome(&running, DATA + 2), Outcome::Passed);
    }
}
