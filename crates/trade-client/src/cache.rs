//! Disk cache for slow-changing trade-API data (the stat/currency catalogs `catalog.rs` fetches).
//! [`load_or_fetch`] reads a cached JSON file when it's still fresh, otherwise awaits the
//! caller-supplied `fetch` future and persists its result -- UI/runtime-agnostic per this crate's
//! module doc comment: it takes an already-constructed future rather than owning an
//! `HttpClient`/executor itself.
//!
//! Cache directory convention (not a public API here -- path construction is the app layer's job,
//! since it alone knows the real cache root): `directories::ProjectDirs::from("", "",
//! "poe2-oracle").cache_dir()`, joined with a per-catalog file name (e.g. `stat-catalog.json`,
//! `static-items.json`) -- matching the convention already established for `poe-dat`'s own
//! schema cache (`crates/poe-dat/src/schema.rs`'s `schema_cache_dir`).

use std::path::Path;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Reads and deserializes `cache_path` if it exists and its mtime is within `max_age` of now;
/// otherwise awaits `fetch`, best-effort writes the result to `cache_path` as JSON (a write
/// failure is logged and does not fail the call -- a working value beats a working cache), and
/// returns it.
///
/// If `fetch` itself errors, falls back to whatever is in `cache_path` even if stale/expired,
/// logging a warning instead of propagating the fetch error -- the app should keep running on old
/// data through a trade-API outage rather than go blank. The fetch error is only propagated when
/// there is truly no usable cache (missing, or itself fails to parse).
pub async fn load_or_fetch<T: Serialize + DeserializeOwned>(
    cache_path: &Path,
    max_age: Duration,
    fetch: impl Future<Output = Result<T>>,
) -> Result<T> {
    if let Some(value) = read_fresh_cache(cache_path, max_age) {
        return Ok(value);
    }

    match fetch.await {
        Ok(value) => {
            if let Err(err) = write_cache(cache_path, &value) {
                log::warn!("failed to write cache {}: {err:#}", cache_path.display());
            }
            Ok(value)
        }
        Err(fetch_err) => match read_stale_cache(cache_path) {
            Some(value) => {
                log::warn!(
                    "fetch failed ({fetch_err:#}), falling back to stale cache {}",
                    cache_path.display()
                );
                Ok(value)
            }
            None => Err(fetch_err),
        },
    }
}

/// `{prefix}-{league}.json` for a league's own cache file. League ids are free text (`"HC Forbidden
/// Rites"`); keeping `[A-Za-z0-9-]` means no id can reach outside the cache directory or trip over
/// a character Windows forbids in file names.
pub(crate) fn league_file_name(prefix: &str, league: &str) -> String {
    let league: String = league
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{prefix}-{league}.json")
}

/// Reads and deserializes `cache_path` if present and its mtime is within `max_age` of now.
/// `None` on any failure (missing file, unreadable mtime, unparseable content, or simply too
/// old) -- every failure mode is treated as a uniform cache miss, falling through to a real
/// fetch.
fn read_fresh_cache<T: DeserializeOwned>(cache_path: &Path, max_age: Duration) -> Option<T> {
    let modified = std::fs::metadata(cache_path).ok()?.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age > max_age {
        return None;
    }
    read_stale_cache(cache_path)
}

/// Reads and deserializes `cache_path` regardless of age. `None` if missing or unparseable.
fn read_stale_cache<T: DeserializeOwned>(cache_path: &Path) -> Option<T> {
    let body = std::fs::read_to_string(cache_path).ok()?;
    serde_json::from_str(&body).ok()
}

fn write_cache<T: Serialize>(cache_path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating cache directory {}", parent.display()))?;
    }
    let body = serde_json::to_string(value).context("serializing cache value")?;
    std::fs::write(cache_path, body)
        .with_context(|| format!("writing cache file {}", cache_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU32, Ordering};

    use serde::{Deserialize, Serialize};

    use super::*;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Payload {
        value: u32,
    }

    /// A unique, per-call scratch file path under the system temp dir -- this crate has no
    /// `tempfile` dependency (see this module's doc comment), so tests hand-roll a unique path
    /// and remove it themselves via [`CleanupGuard`].
    fn unique_cache_path() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "poe2-oracle-test-cache-{}-{n}.json",
            std::process::id()
        ))
    }

    struct CleanupGuard(std::path::PathBuf);
    impl Drop for CleanupGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn missing_cache_calls_fetch_and_writes_it() {
        let path = unique_cache_path();
        let _guard = CleanupGuard(path.clone());
        let called = Cell::new(false);

        let value =
            futures::executor::block_on(load_or_fetch(&path, Duration::from_secs(3600), async {
                called.set(true);
                Ok(Payload { value: 42 })
            }))
            .expect("load_or_fetch should succeed");

        assert!(
            called.get(),
            "fetch should have been called with no cache present"
        );
        assert_eq!(value, Payload { value: 42 });
        let cached: Payload =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            cached,
            Payload { value: 42 },
            "the fetched value should have been written back to disk"
        );
    }

    #[test]
    fn fresh_cache_skips_fetch() {
        let path = unique_cache_path();
        let _guard = CleanupGuard(path.clone());
        std::fs::write(&path, serde_json::to_string(&Payload { value: 7 }).unwrap()).unwrap();
        let called = Cell::new(false);

        let value =
            futures::executor::block_on(load_or_fetch(&path, Duration::from_secs(3600), async {
                called.set(true);
                Ok(Payload { value: 999 })
            }))
            .expect("load_or_fetch should succeed");

        assert!(
            !called.get(),
            "fetch should not run while the cache is still fresh"
        );
        assert_eq!(value, Payload { value: 7 });
    }

    #[test]
    fn fetch_failure_falls_back_to_a_stale_cache() {
        let path = unique_cache_path();
        let _guard = CleanupGuard(path.clone());
        std::fs::write(
            &path,
            serde_json::to_string(&Payload { value: 13 }).unwrap(),
        )
        .unwrap();
        // Force the file to read as expired regardless of real elapsed time.
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(60 * 60 * 48))
            .expect("setting mtime should be supported on this platform");
        drop(file);

        let value =
            futures::executor::block_on(load_or_fetch(&path, Duration::from_secs(60), async {
                Err::<Payload, _>(anyhow::anyhow!("simulated network failure"))
            }))
            .expect("load_or_fetch should fall back to the stale cache instead of erroring");

        assert_eq!(value, Payload { value: 13 });
    }

    #[test]
    fn fetch_failure_with_no_cache_at_all_propagates_the_error() {
        let path = unique_cache_path();
        // No CleanupGuard needed: load_or_fetch never writes a file on this path.

        let result =
            futures::executor::block_on(load_or_fetch(&path, Duration::from_secs(60), async {
                Err::<Payload, _>(anyhow::anyhow!("simulated network failure"))
            }));

        assert!(
            result.is_err(),
            "with no usable cache, the fetch error should propagate"
        );
    }
}
