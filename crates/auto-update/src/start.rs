//! What the app tells the service when it starts: the query of the first connection to the event
//! stream ([`EVENTS_PATH`](oracle_protocol::EVENTS_PATH)) in a run, which [`Start`] builds. The
//! service counts starts, new installations, updates from one version to another and starts by
//! interface language from it, as numbers: the query holds no id, no name and no path, and what
//! it says is all it holds.
//!
//! It is `start=1`, then, as they apply and in this order, `first=1` (this installation's first
//! start), `from=<version>` (the version its last start ran, when that was another one),
//! `lang=en|ru` (the interface language) and `dev=1` (a build made for testing, which the service
//! counts as nothing else). oracle-protocol names them ([`START_PARAM`] and its neighbours).
//!
//! What a run can't know of the ones before it is kept in two small files in the app's data
//! folder:
//!
//! - `last-run-version`, the version of the last start the service was told about, as text.
//!   [`Start::delivered`] writes it once a connection has opened, and not before: a start that
//!   never got through -- offline, or refused -- is told the next time. Another version in it
//!   makes `from=<that version>`. Without the file the install is new (`first=1`), unless the
//!   caller knows better: the app ran from the folder before it kept the file (`earlier_runs`, an
//!   install upgrading to the first version that keeps it), or the updater's own marker says the
//!   update to this version took (`updated_from`, and then that is the version it came from).
//!   What the file holds when it is no plain `major.minor.patch` -- a pre-release, a half-written
//!   file, anything else -- says nothing, neither new nor from.
//! - `dev`, an empty file, which the developer's test builds have: their start says `dev=1`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, Result};
use oracle_protocol::{DEV_PARAM, FIRST_PARAM, FROM_PARAM, LANG_PARAM, START_PARAM};

use crate::Version;

/// The version of the last start the service was told about, in the data folder.
const LAST_RUN_FILE: &str = "last-run-version";
/// An empty file in the data folder marks a build made for testing.
const DEV_FILE: &str = "dev";

/// The start of this run, as the service is told of it: one per process, shared by every follower
/// of the event stream it makes ([`follow_events`](crate::events::follow_events)), so that one
/// made again -- updates turned off and on -- doesn't tell the service a second time.
pub struct Start {
    /// `start=1&...`, which every attempt carries until [`Start::delivered`].
    query: String,
    /// `last-run-version`, which [`Start::delivered`] writes.
    marker: PathBuf,
    /// The running version: what the marker says once the service has been told.
    version: String,
    delivered: AtomicBool,
}

impl Start {
    /// The start of the run of `running`, from the files in `dir`, the app's data folder, and
    /// what the caller knows of the rest: `lang`, the interface language, `en` or `ru` (anything
    /// else is left out); whether the app has `earlier_runs` -- has run from `dir` before,
    /// whether or not it kept `last-run-version` then; and, when the auto-updater's own marker
    /// says the update to `running` took, the version it was `updated_from`.
    pub fn new(
        dir: &Path,
        running: &Version,
        lang: &str,
        earlier_runs: bool,
        updated_from: Option<&str>,
    ) -> Arc<Start> {
        let marker = dir.join(LAST_RUN_FILE);
        let (first, from) = match fs::read_to_string(&marker) {
            Ok(text) => (false, plain_version(&text).filter(|last| last != running)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let updated_from = updated_from
                    .and_then(plain_version)
                    .filter(|from| from != running);
                match updated_from {
                    Some(from) => (false, Some(from)),
                    None => (!earlier_runs, None),
                }
            }
            // A file that is there but can't be read is no new install either.
            Err(err) => {
                log::warn!("reading {} failed: {err}", marker.display());
                (false, None)
            }
        };
        let mut params = vec![format!("{START_PARAM}=1")];
        if first {
            params.push(format!("{FIRST_PARAM}=1"));
        }
        if let Some(from) = from {
            params.push(format!("{FROM_PARAM}={from}"));
        }
        if matches!(lang, "en" | "ru") {
            params.push(format!("{LANG_PARAM}={lang}"));
        }
        if dir.join(DEV_FILE).exists() {
            params.push(format!("{DEV_PARAM}=1"));
        }
        Arc::new(Start {
            query: params.join("&"),
            marker,
            version: running.to_string(),
            delivered: AtomicBool::new(false),
        })
    }

    /// What the connection's request carries, its query without the `?`: the same for every
    /// attempt until the service has been told, and `None` after.
    pub(crate) fn query(&self) -> Option<&str> {
        (!self.delivered.load(Ordering::Relaxed)).then_some(self.query.as_str())
    }

    /// A connection has opened: the service has been told. `last-run-version` says this version
    /// from now on, and no later attempt carries the query. A file that can't be written is
    /// logged and no more: the service is not told twice in this run for it. The write is a few
    /// bytes, blocking.
    pub(crate) fn delivered(&self) {
        if self.delivered.swap(true, Ordering::Relaxed) {
            return;
        }
        if let Err(err) = self.write_marker() {
            log::warn!("keeping the version of this start failed: {err:#}");
        }
    }

    fn write_marker(&self) -> Result<()> {
        if let Some(dir) = self.marker.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        fs::write(&self.marker, &self.version)
            .with_context(|| format!("writing {}", self.marker.display()))
    }
}

/// `text` as a version, when it is a plain `major.minor.patch`, around whitespace: a
/// pre-release, build metadata or anything else is no version the service counts.
fn plain_version(text: &str) -> Option<Version> {
    Version::parse(text.trim())
        .ok()
        .filter(|version| version.pre.is_empty() && version.build.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    /// The start of `running` for an app that speaks Russian and hasn't run from `dir` before.
    fn start(dir: &Path, running: &str) -> Arc<Start> {
        Start::new(dir, &version(running), "ru", false, None)
    }

    /// What `last-run-version` holds, if it is there.
    fn marker(dir: &Path) -> Option<String> {
        fs::read_to_string(dir.join("last-run-version")).ok()
    }

    #[test]
    fn a_new_install_says_so_until_the_service_has_been_told() {
        // The data folder isn't there yet: the marker's write makes it.
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let start = start(&data, "0.1.3");
        // Every attempt says it, and the file that would say it was told is not there...
        assert_eq!(start.query(), Some("start=1&first=1&lang=ru"));
        assert_eq!(start.query(), Some("start=1&first=1&lang=ru"));
        assert_eq!(marker(&data), None);

        // ...until a connection has opened: then it is, and none says it again.
        start.delivered();
        assert_eq!(marker(&data).as_deref(), Some("0.1.3"));
        assert_eq!(start.query(), None);
    }

    #[test]
    fn once_told_the_next_start_of_the_same_version_says_only_that_it_started() {
        let dir = tempfile::tempdir().unwrap();
        start(dir.path(), "0.1.3").delivered();
        // Not new any more, and nothing to come from: no `first`, no `from`.
        let again = start(dir.path(), "0.1.3");
        assert_eq!(again.query(), Some("start=1&lang=ru"));
    }

    #[test]
    fn a_new_version_says_which_one_the_last_start_ran_and_takes_its_place_once_told() {
        let dir = tempfile::tempdir().unwrap();
        start(dir.path(), "0.1.2").delivered();
        let updated = start(dir.path(), "0.1.3");
        assert_eq!(updated.query(), Some("start=1&from=0.1.2&lang=ru"));
        // A start that doesn't get through leaves the marker: the next one says the same.
        assert_eq!(marker(dir.path()).as_deref(), Some("0.1.2"));
        updated.delivered();
        assert_eq!(marker(dir.path()).as_deref(), Some("0.1.3"));
    }

    #[test]
    fn an_install_from_before_the_marker_is_not_new_and_the_updater_knows_where_it_came_from() {
        let dir = tempfile::tempdir().unwrap();
        let new = |earlier_runs, updated_from| {
            Start::new(
                dir.path(),
                &version("0.1.3"),
                "ru",
                earlier_runs,
                updated_from,
            )
        };
        // It has run from the folder, only never kept the marker: not new, and its last version
        // is not known...
        assert_eq!(new(true, None).query(), Some("start=1&lang=ru"));
        // ...unless the updater's marker says which update took.
        let updated = new(true, Some("0.1.2"));
        assert_eq!(updated.query(), Some("start=1&from=0.1.2&lang=ru"));
        // That hint is no `from` when it names no other plain version.
        for hint in ["0.1.3", "0.1.2-rc.1", "junk", ""] {
            assert_eq!(
                new(true, Some(hint)).query(),
                Some("start=1&lang=ru"),
                "{hint:?}"
            );
        }
        // With the marker there is no need of it, and it is the later word.
        start(dir.path(), "0.1.3").delivered();
        assert_eq!(new(true, Some("0.1.1")).query(), Some("start=1&lang=ru"));
    }

    #[test]
    fn a_dev_file_makes_the_start_a_developers() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            start(dir.path(), "0.1.3").query(),
            Some("start=1&first=1&lang=ru")
        );
        fs::write(dir.path().join("dev"), b"").unwrap();
        assert_eq!(
            start(dir.path(), "0.1.3").query(),
            Some("start=1&first=1&lang=ru&dev=1")
        );
    }

    #[test]
    fn a_marker_that_is_no_plain_version_says_nothing() {
        let junk: [&[u8]; 13] = [
            b"",
            b"junk",
            b"0.1",
            b"0.1.2.3",
            b"v0.1.2",
            b"0.1.2-rc.1",
            b"0.1.2+7",
            b"-1.0.0",
            b"0.1.2 0.1.3",
            b"0.1.2&dev=1",
            b"99999999999999999999.0.0",
            // Not even text; and what a power cut leaves of a file, zeros.
            b"\xff\xfe0.1.2",
            &[0; 4096],
        ];
        for bytes in junk {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("last-run-version"), bytes).unwrap();
            // Neither `from` nor `first`: that the file is there says the install ran before.
            let start = start(dir.path(), "0.1.3");
            let text = String::from_utf8_lossy(&bytes[..bytes.len().min(32)]);
            assert_eq!(start.query(), Some("start=1&lang=ru"), "{text:?}");
        }
        // A line break around a version is no harm.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("last-run-version"), "0.1.2\r\n").unwrap();
        let start = start(dir.path(), "0.1.3");
        assert_eq!(start.query(), Some("start=1&from=0.1.2&lang=ru"));
    }

    #[test]
    fn the_language_is_english_or_russian_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let queries = [
            ("en", "start=1&first=1&lang=en"),
            ("ru", "start=1&first=1&lang=ru"),
            ("de", "start=1&first=1"),
            ("", "start=1&first=1"),
            ("ru&dev=1", "start=1&first=1"),
        ];
        for (lang, query) in queries {
            let start = Start::new(dir.path(), &version("0.1.3"), lang, false, None);
            assert_eq!(start.query(), Some(query), "{lang:?}");
        }
    }

    #[test]
    fn a_marker_that_cannot_be_written_is_no_reason_to_tell_the_service_again() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should go: neither read nor written.
        fs::create_dir(dir.path().join("last-run-version")).unwrap();
        let start = start(dir.path(), "0.1.3");
        assert_eq!(start.query(), Some("start=1&lang=ru"));
        start.delivered();
        assert_eq!(start.query(), None);
    }
}
