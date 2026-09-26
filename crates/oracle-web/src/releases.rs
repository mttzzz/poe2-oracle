//! The latest release, for the app's update check and the site's download button.
//!
//! The repository is private, so GitHub shows its releases to the owner's token only. The service
//! asks with it (at most every five minutes), answers the update check with the part of the
//! release the updater reads ([`oracle_protocol::Release`]), and serves the release's files itself
//! under `/download`, keeping them in memory once fetched. When GitHub fails, the last answer
//! stands.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use oracle_protocol::{DOWNLOAD_PATH, Release, ReleaseAsset, installer_asset};
use parking_lot::Mutex;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use sha2::{Digest as _, Sha256};
use tokio::sync::OnceCell;
use tracing::warn;

use crate::App;
use crate::github::{GhAsset, GhRelease, GitHub};
use crate::stats::{self, Stat};
use crate::upstream::describe;

/// How long an answer from GitHub stands before the next request asks again.
const FRESH_FOR: Duration = Duration::from_secs(5 * 60);
/// After GitHub fails, how long the last answer (or none) stands before asking again.
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// The release files kept in memory, together: the installer (a few MB), `SHA256SUMS` and its
/// signature fit many times over. A file over what's left streams from GitHub on every download.
const KEEP_BYTES: u64 = 40 * 1024 * 1024;
/// A file fetched to be kept: the whole of it from GitHub's storage.
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);
/// Path segments keep the URL-safe characters as they are and encode everything else.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub struct Releases {
    public_url: String,
    cache: Mutex<Cache>,
    /// Held while asking GitHub: one request asks, the rest take the last answer meanwhile.
    asking: tokio::sync::Mutex<()>,
    files: Mutex<Files>,
}

struct Cache {
    latest: Option<Arc<Latest>>,
    ask_after: Instant,
}

/// The latest release as the service last heard it.
pub struct Latest {
    tag: String,
    /// Its files, those finished uploading.
    assets: Vec<GhAsset>,
    /// The update check's answer, and its strong ETag.
    json: Bytes,
    etag: HeaderValue,
}

impl Latest {
    fn new(release: GhRelease, public_url: &str) -> Latest {
        let assets: Vec<GhAsset> = release
            .assets
            .into_iter()
            .filter(|asset| asset.state == "uploaded")
            .collect();
        let json = serde_json::to_vec(&answer(&release.tag_name, &assets, public_url))
            .expect("a release serializes");
        let etag = strong_etag(&json);
        Latest {
            tag: release.tag_name,
            assets,
            json: Bytes::from(json),
            etag,
        }
    }

    /// The version its tag names: `v0.1.0` is `0.1.0`.
    fn version(&self) -> &str {
        self.tag.strip_prefix('v').unwrap_or(&self.tag)
    }

    fn asset(&self, name: &str) -> Option<&GhAsset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

impl Releases {
    pub fn new(public_url: String) -> Releases {
        Releases {
            public_url,
            cache: Mutex::new(Cache {
                latest: None,
                ask_after: Instant::now(),
            }),
            asking: tokio::sync::Mutex::new(()),
            files: Mutex::default(),
        }
    }

    /// The latest release: the last answer while it's fresh, else GitHub's; `None` when there is
    /// no answer to give.
    pub async fn current(&self, github: &GitHub) -> Option<Arc<Latest>> {
        if !github.configured() {
            return None;
        }
        if let Some(standing) = self.standing() {
            return standing;
        }
        let _asking = match self.asking.try_lock() {
            Ok(asking) => asking,
            Err(_) => match self.last() {
                Some(last) => return Some(last),
                None => self.asking.lock().await,
            },
        };
        // Whoever asked while this request waited may have got the answer.
        if let Some(standing) = self.standing() {
            return standing;
        }
        let asked = github.latest_release().await;
        let mut cache = self.cache.lock();
        match asked {
            Ok(release) => {
                let latest = Arc::new(Latest::new(release, &self.public_url));
                self.files.lock().switch(&latest.assets);
                cache.latest = Some(latest);
                cache.ask_after = Instant::now() + FRESH_FOR;
            }
            Err(problem) => {
                warn!(%problem, "GitHub didn't give the latest release");
                cache.ask_after = Instant::now() + RETRY_AFTER;
            }
        }
        cache.latest.clone()
    }

    /// The last answer while it stands, which may be none; `None` when it's time to ask.
    fn standing(&self) -> Option<Option<Arc<Latest>>> {
        let cache = self.cache.lock();
        (Instant::now() < cache.ask_after).then(|| cache.latest.clone())
    }

    fn last(&self) -> Option<Arc<Latest>> {
        self.cache.lock().latest.clone()
    }

    /// Where `asset` of the release made of `assets` is kept once fetched; `None` when it doesn't
    /// fit.
    fn kept(&self, assets: &[GhAsset], asset: &GhAsset) -> Option<Arc<OnceCell<Bytes>>> {
        let mut files = self.files.lock();
        files.switch(assets);
        if let Some(cell) = files.cells.get(&asset.id) {
            return Some(cell.clone());
        }
        if files.promised + asset.size > KEEP_BYTES {
            return None;
        }
        files.promised += asset.size;
        let cell = Arc::new(OnceCell::new());
        files.cells.insert(asset.id, cell.clone());
        Some(cell)
    }
}

/// The latest release's files, each fetched once by whichever download comes first. Known by
/// their asset ids: GitHub gives every upload a new one, so a file replaced under its name --
/// `gh release upload --clobber`, or the release deleted and published again under its tag -- is
/// another file.
#[derive(Default)]
struct Files {
    /// The release's asset ids, sorted.
    release: Vec<u64>,
    cells: HashMap<u64, Arc<OnceCell<Bytes>>>,
    /// The sizes of the files given a cell, fetched or not yet.
    promised: u64,
}

impl Files {
    /// Keeps only the files of the release made of `assets`: another release, or a file of this
    /// one replaced, lets all the kept ones go.
    fn switch(&mut self, assets: &[GhAsset]) {
        let mut release: Vec<u64> = assets.iter().map(|asset| asset.id).collect();
        release.sort_unstable();
        if self.release != release {
            *self = Files {
                release,
                ..Files::default()
            };
        }
    }
}

/// The update check's answer for release `tag`: every file downloads from this service, and the
/// release's page is the site.
fn answer(tag: &str, assets: &[GhAsset], public_url: &str) -> Release {
    Release {
        tag_name: tag.to_owned(),
        html_url: format!("{public_url}/"),
        assets: assets
            .iter()
            .map(|asset| ReleaseAsset {
                name: asset.name.clone(),
                browser_download_url: format!("{public_url}{}", download_path(tag, &asset.name)),
                size: asset.size,
            })
            .collect(),
    }
}

/// `/download/<tag>/<name>`, each a percent-encoded path segment.
fn download_path(tag: &str, name: &str) -> String {
    format!(
        "{DOWNLOAD_PATH}/{}/{}",
        utf8_percent_encode(tag, SEGMENT),
        utf8_percent_encode(name, SEGMENT)
    )
}

/// A strong validator of the exact answer: equal answers get equal tags on every replica.
fn strong_etag(json: &[u8]) -> HeaderValue {
    let digest = Sha256::digest(json);
    let hex: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    HeaderValue::from_str(&format!("\"{hex}\"")).expect("hex in quotes is a header value")
}

/// Whether `If-None-Match` names `etag`. RFC 9110 compares weakly here: `W/"x"` names `"x"`.
fn names(if_none_match: &HeaderValue, etag: &HeaderValue) -> bool {
    let (Ok(tags), Ok(etag)) = (if_none_match.to_str(), etag.to_str()) else {
        return false;
    };
    let etag = etag.trim_start_matches("W/");
    tags.split(',')
        .map(str::trim)
        .any(|tag| tag == "*" || tag.trim_start_matches("W/") == etag)
}

/// `GET /api/v1/releases/latest`: the updater's check, `304` when it already has this answer.
pub async fn latest(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    stats::count(&app, Stat::UpdateCheck);
    let Some(latest) = app.releases.current(&app.github).await else {
        return no_release();
    };
    let unchanged = headers
        .get(header::IF_NONE_MATCH)
        .is_some_and(|tags| names(tags, &latest.etag));
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        (
            [(header::CONTENT_TYPE, "application/json")],
            latest.json.clone(),
        )
            .into_response()
    };
    let headers = response.headers_mut();
    headers.insert(header::ETAG, latest.etag.clone());
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

/// `GET /download/latest`: the site's download button, sent on to the current installer.
pub async fn latest_installer(State(app): State<Arc<App>>) -> Response {
    let Some(latest) = app.releases.current(&app.github).await else {
        return no_release();
    };
    let name = installer_asset(latest.version());
    if latest.asset(&name).is_none() {
        return (StatusCode::NOT_FOUND, "the latest release has no installer").into_response();
    }
    (
        StatusCode::FOUND,
        [
            (header::LOCATION, download_path(&latest.tag, &name)),
            (header::CACHE_CONTROL, "no-cache".to_owned()),
        ],
    )
        .into_response()
}

/// `GET /download/<tag>/<asset>`: a file of the latest release. Kept in memory after the first
/// download, which fetches it for everyone asking meanwhile.
pub async fn asset(
    State(app): State<Arc<App>>,
    Path((tag, name)): Path<(String, String)>,
    method: Method,
) -> Response {
    let Some(latest) = app.releases.current(&app.github).await else {
        return no_release();
    };
    let Some(asset) = latest.asset(&name).filter(|_| latest.tag == tag) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
        .unwrap_or(HeaderValue::from_static("attachment"));
    let headers = [
        (
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        ),
        (header::CONTENT_LENGTH, HeaderValue::from(asset.size)),
        (header::CONTENT_DISPOSITION, disposition),
    ];
    if method == Method::HEAD {
        return (headers, Body::empty()).into_response();
    }
    let body = match app.releases.kept(&latest.assets, asset) {
        Some(cell) => match cell.get_or_try_init(|| fetch(&app.github, asset)).await {
            Ok(bytes) => Body::from(bytes.clone()),
            Err(problem) => return unreachable_file(&name, &problem),
        },
        None => match app.github.download(asset.id).await {
            Ok(response) => Body::from_stream(response.bytes_stream()),
            Err(problem) => return unreachable_file(&name, &problem),
        },
    };
    let installer = name == installer_asset(latest.version());
    stats::count(
        &app,
        if installer {
            Stat::Download
        } else {
            Stat::UpdateDownload
        },
    );
    (headers, body).into_response()
}

/// The whole of `asset`, exactly as long as the release says.
async fn fetch(github: &GitHub, asset: &GhAsset) -> Result<Bytes, String> {
    let whole = async {
        let response = github.download(asset.id).await?;
        response.bytes().await.map_err(describe)
    };
    let bytes = tokio::time::timeout(FETCH_TIMEOUT, whole)
        .await
        .map_err(|_| format!("not fetched in {} s", FETCH_TIMEOUT.as_secs()))??;
    if bytes.len() as u64 != asset.size {
        return Err(format!(
            "{} bytes where the release lists {}",
            bytes.len(),
            asset.size
        ));
    }
    Ok(bytes)
}

fn unreachable_file(name: &str, problem: &str) -> Response {
    warn!(name, %problem, "couldn't get a release file from GitHub");
    (
        StatusCode::BAD_GATEWAY,
        "GitHub didn't give the file; try again later",
    )
        .into_response()
}

fn no_release() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, "no release to offer").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: u64, name: &str, size: u64, state: &str) -> GhAsset {
        GhAsset {
            id,
            name: name.to_owned(),
            size,
            state: state.to_owned(),
        }
    }

    #[test]
    fn the_answer_downloads_everything_from_this_service() {
        let release = GhRelease {
            tag_name: "v0.1.0".to_owned(),
            assets: vec![
                asset(1, "PoE2-Oracle-Setup-0.1.0.exe", 9_000_000, "uploaded"),
                asset(2, "SHA256SUMS", 96, "uploaded"),
                asset(3, "odd name+ü.txt", 5, "uploaded"),
                asset(4, "SHA256SUMS.sig", 88, "open"),
            ],
        };
        let latest = Latest::new(release, "https://oracle.pushka.biz");
        let answer: Release = serde_json::from_slice(&latest.json).unwrap();
        assert_eq!(
            answer,
            Release {
                tag_name: "v0.1.0".to_owned(),
                html_url: "https://oracle.pushka.biz/".to_owned(),
                assets: vec![
                    ReleaseAsset {
                        name: "PoE2-Oracle-Setup-0.1.0.exe".to_owned(),
                        browser_download_url:
                            "https://oracle.pushka.biz/download/v0.1.0/PoE2-Oracle-Setup-0.1.0.exe"
                                .to_owned(),
                        size: 9_000_000,
                    },
                    ReleaseAsset {
                        name: "SHA256SUMS".to_owned(),
                        browser_download_url:
                            "https://oracle.pushka.biz/download/v0.1.0/SHA256SUMS".to_owned(),
                        size: 96,
                    },
                    ReleaseAsset {
                        name: "odd name+ü.txt".to_owned(),
                        browser_download_url:
                            "https://oracle.pushka.biz/download/v0.1.0/odd%20name%2B%C3%BC.txt"
                                .to_owned(),
                        size: 5,
                    },
                ],
            },
            "a file still uploading isn't offered"
        );
        assert_eq!(latest.version(), "0.1.0");
    }

    #[test]
    fn the_etag_is_strong_and_follows_the_answer() {
        let tag = |json: &[u8]| strong_etag(json).to_str().unwrap().to_owned();
        let etag = tag(b"{\"tag_name\":\"v0.1.0\"}");
        assert!(
            etag.starts_with('"') && etag.ends_with('"') && etag.len() == 34,
            "{etag}"
        );
        assert_eq!(etag, tag(b"{\"tag_name\":\"v0.1.0\"}"));
        assert_ne!(etag, tag(b"{\"tag_name\":\"v0.1.1\"}"));
    }

    #[test]
    fn if_none_match_names_the_etag_weakly() {
        let etag = HeaderValue::from_static("\"abc\"");
        let names = |header: &'static str| names(&HeaderValue::from_static(header), &etag);
        assert!(names("\"abc\""));
        assert!(names("W/\"abc\""));
        assert!(names("\"old\", \"abc\""));
        assert!(names("*"));
        assert!(!names("\"abcd\""));
        assert!(!names("\"old\""));
        assert!(!names("abc"));
    }

    #[test]
    fn a_file_replaced_under_its_name_is_kept_anew() {
        let releases = Releases::new("https://oracle.example".to_owned());
        let published = [
            asset(1, "PoE2-Oracle-Setup-0.1.0.exe", 9_000_000, "uploaded"),
            asset(2, "SHA256SUMS", 96, "uploaded"),
        ];
        let sums = releases.kept(&published, &published[1]).unwrap();
        sums.set(Bytes::from_static(b"the first sums")).unwrap();
        // The same release, its files listed in another order: the same file, kept.
        let reordered = [published[1].clone(), published[0].clone()];
        let again = releases.kept(&reordered, &reordered[0]).unwrap();
        assert!(Arc::ptr_eq(&sums, &again));

        // `gh release upload --clobber`: the same tag and name, a new upload with a new id.
        let replaced = [published[0].clone(), asset(3, "SHA256SUMS", 97, "uploaded")];
        let fresh = releases.kept(&replaced, &replaced[1]).unwrap();
        assert!(fresh.get().is_none(), "the first file's bytes are let go");
    }
}
