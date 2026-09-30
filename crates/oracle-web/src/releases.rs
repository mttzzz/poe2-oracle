//! The releases, for the app's updater and the site's download button.
//!
//! The repository publishes two kinds: the app's releases, tagged `v<semver>`, whose installer
//! replaces the exe, and data packs, tagged `data-<N>` ([`DATA_TAG_PREFIX`]), the game tables the
//! app takes in without a new exe. The repository is private, so GitHub shows them to the owner's
//! token only. The service lists them itself ([`refresh`]: at start, then every two minutes, each
//! page asked with its last validator, so that an unchanged list costs no rate limit) and keeps the
//! latest of each kind: the published `v` release with the highest version by semver precedence,
//! and the published `data-` release with the highest number. Drafts and prereleases never count,
//! and a release of one kind never stands for the other. The service answers the updater with the
//! part of either release it reads ([`oracle_protocol::Release`]), serves both releases' files
//! itself under `/download`, keeping them in memory once fetched, and tells the event streams
//! ([`crate::events`]) their versions. When GitHub fails, the last listing stands.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Path, RawQuery, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use oracle_protocol::{
    DATA_TAG_PREFIX, DOWNLOAD_PATH, DataVersion, Release, ReleaseAsset, Versions, data_pack_asset,
    installer_asset,
};
use parking_lot::Mutex;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use semver::Version;
use sha2::{Digest as _, Sha256};
use tokio::sync::{OnceCell, watch};
use tracing::{info, warn};

use crate::App;
use crate::agent::AppAgent;
use crate::distinct::Who;
use crate::github::{GhAsset, GhRelease, GitHub, Listed, Page, RELEASES_PER_PAGE};
use crate::stats::{self, Source, Stat};
use crate::upstream::describe;
use crate::usage;

/// How often the releases are listed, unless the configuration says otherwise
/// ([`crate::Config::list_releases_every`]: `LIST_RELEASES_EVERY`, or a test's own).
pub const LIST_EVERY: Duration = Duration::from_secs(2 * 60);
/// The most pages a listing reads, [`RELEASES_PER_PAGE`] releases each.
const MAX_PAGES: usize = 10;
/// The release files kept in memory, together: the installer (a few MB), the data pack, and each
/// release's `SHA256SUMS` and its signature fit many times over. A file over what's left streams
/// from GitHub on every download.
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
    /// What the service knows of the releases, for the updater's answers and the downloads.
    known: watch::Sender<Known>,
    /// The latest versions, for the event streams; `None` while the service doesn't know them.
    versions: watch::Sender<Option<Versions>>,
    files: Mutex<Files>,
}

/// What the service knows of the releases.
enum Known {
    /// The first listing is on its way.
    Awaited,
    /// No listing: no token to list with, or GitHub has failed every time so far.
    Unlisted,
    Listed(Arc<Listing>),
}

/// The latest release of each kind, as a listing found them.
pub struct Listing {
    app: Option<Latest>,
    data: Option<Latest>,
    versions: Versions,
    /// The plain versions of the newest published app releases, [`KNOWN_VERSIONS`] at most, newest
    /// first: the only ones a counter is named for.
    known_versions: Vec<String>,
}

/// A release the service offers.
pub struct Latest {
    tag: String,
    /// The file it's for: the app's installer, or the data pack.
    payload: String,
    /// Its files, those finished uploading.
    assets: Vec<GhAsset>,
    /// The updater's answer, and its strong ETag.
    json: Bytes,
    etag: HeaderValue,
}

/// What a release's tag makes it.
#[derive(Debug, PartialEq)]
enum Kind {
    /// `v` and a semver version.
    App(Version),
    /// [`DATA_TAG_PREFIX`] and the pack's version, written plainly: `data-007` isn't the tag of
    /// pack 7, whose file is named `…-7.zip`.
    Data(DataVersion),
}

impl Kind {
    fn of(tag: &str) -> Option<Kind> {
        if let Some(number) = tag.strip_prefix(DATA_TAG_PREFIX) {
            let version: DataVersion = number.parse().ok()?;
            return (version.to_string() == number).then_some(Kind::Data(version));
        }
        Version::parse(tag.strip_prefix('v')?).ok().map(Kind::App)
    }
}

/// The latest release of each kind among some.
#[derive(Default)]
struct Picked<'a> {
    app: Option<(&'a GhRelease, Version)>,
    data: Option<(&'a GhRelease, DataVersion)>,
}

/// The latest release of each kind among `releases`, listed newest first: the published app
/// release with the highest version by semver precedence, and the published data pack with the
/// highest version. Of two releases with equal versions, the first listed stands.
fn pick<'a>(releases: impl IntoIterator<Item = &'a GhRelease>) -> Picked<'a> {
    let mut picked = Picked::default();
    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        match Kind::of(&release.tag_name) {
            Some(Kind::App(version))
                if picked
                    .app
                    .as_ref()
                    .is_none_or(|(_, latest)| version.cmp_precedence(latest).is_gt()) =>
            {
                picked.app = Some((release, version));
            }
            Some(Kind::Data(version)) if picked.data.is_none_or(|(_, latest)| version > latest) => {
                picked.data = Some((release, version));
            }
            _ => {}
        }
    }
    picked
}

/// How many of the newest app releases a counter is named for. Older ones, and anything that
/// isn't a release, count as `other`.
const KNOWN_VERSIONS: usize = 16;

/// The plain versions (`0.1.3`, no pre-release or build part) of the published app releases among
/// `releases`, newest by semver precedence first, at most [`KNOWN_VERSIONS`].
fn known_versions(releases: &[&GhRelease]) -> Vec<String> {
    let mut versions: Vec<Version> = releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| Version::parse(release.tag_name.strip_prefix('v')?).ok())
        .filter(|version| version.pre.is_empty() && version.build.is_empty())
        .collect();
    versions.sort_by(|a, b| b.cmp_precedence(a));
    versions.dedup();
    versions
        .iter()
        .take(KNOWN_VERSIONS)
        .map(Version::to_string)
        .collect()
}

impl Listing {
    /// The latest releases among `releases`, listed newest first, as the service at `public_url`
    /// offers them.
    fn new<'a>(releases: impl IntoIterator<Item = &'a GhRelease>, public_url: &str) -> Listing {
        let releases: Vec<&GhRelease> = releases.into_iter().collect();
        let picked = pick(releases.iter().copied());
        let app = picked.app.map(|(release, _)| {
            let version = release.tag_name.strip_prefix('v').unwrap_or_default();
            (
                version,
                Latest::new(release, installer_asset(version), public_url),
            )
        });
        let data = picked.data.map(|(release, version)| {
            (
                version,
                Latest::new(release, data_pack_asset(version), public_url),
            )
        });
        Listing {
            versions: Versions {
                app: app.as_ref().map(|(version, _)| (*version).to_owned()),
                data: data.as_ref().map(|(version, _)| *version),
            },
            known_versions: known_versions(&releases),
            app: app.map(|(_, latest)| latest),
            data: data.map(|(_, latest)| latest),
        }
    }

    fn releases(&self) -> impl Iterator<Item = &Latest> {
        self.app.iter().chain(&self.data)
    }

    /// The offered release tagged `tag`, and what a download of its payload counts as.
    fn release(&self, tag: &str) -> Option<(&Latest, Stat)> {
        [
            (&self.app, Stat::Download),
            (&self.data, Stat::DataDownload),
        ]
        .into_iter()
        .find_map(|(latest, payload)| {
            latest
                .as_ref()
                .filter(|latest| latest.tag == tag)
                .map(|latest| (latest, payload))
        })
    }
}

impl Latest {
    fn new(release: &GhRelease, payload: String, public_url: &str) -> Latest {
        let assets: Vec<GhAsset> = release
            .assets
            .iter()
            .filter(|asset| asset.state == "uploaded")
            .cloned()
            .collect();
        let json = serde_json::to_vec(&answer(&release.tag_name, &assets, public_url))
            .expect("a release serializes");
        let etag = strong_etag(&json);
        Latest {
            tag: release.tag_name.clone(),
            payload,
            assets,
            json: Bytes::from(json),
            etag,
        }
    }

    fn asset(&self, name: &str) -> Option<&GhAsset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

impl Releases {
    /// The releases as the service at `public_url` offers them. `listed`: whether there is a token
    /// to list them with; without one there is no release to offer, and the event streams say so.
    pub fn new(public_url: String, listed: bool) -> Releases {
        let (known, versions) = if listed {
            (Known::Awaited, None)
        } else {
            (Known::Unlisted, Some(Versions::default()))
        };
        Releases {
            public_url,
            known: watch::Sender::new(known),
            versions: watch::Sender::new(versions),
            files: Mutex::default(),
        }
    }

    /// The last listing, once the first is in; `None` when there is none.
    pub async fn listing(&self) -> Option<Arc<Listing>> {
        let mut known = self.known.subscribe();
        let known = known
            .wait_for(|known| !matches!(known, Known::Awaited))
            .await
            .ok()?;
        match &*known {
            Known::Listed(listing) => Some(listing.clone()),
            Known::Awaited | Known::Unlisted => None,
        }
    }

    /// The versions counters are named for ([`KNOWN_VERSIONS`] of the newest published releases,
    /// newest first). Waits for the first listing; none when there is none.
    pub async fn known_versions(&self) -> Vec<String> {
        self.listing()
            .await
            .map(|listing| listing.known_versions.clone())
            .unwrap_or_default()
    }

    /// The latest versions, and each change of them.
    pub fn versions(&self) -> watch::Receiver<Option<Versions>> {
        self.versions.subscribe()
    }

    /// Takes a listing of `releases`, newest first: their latest are offered from now on.
    pub(crate) fn listed<'a>(&self, releases: impl IntoIterator<Item = &'a GhRelease>) {
        let listing = Listing::new(releases, &self.public_url);
        let mut offered: Vec<u64> = listing
            .releases()
            .flat_map(|latest| latest.assets.iter().map(|asset| asset.id))
            .collect();
        offered.sort_unstable();
        self.files.lock().switch(offered);
        let versions = listing.versions.clone();
        self.known.send_replace(Known::Listed(Arc::new(listing)));
        self.versions.send_if_modified(|known| {
            if known.as_ref() == Some(&versions) {
                return false;
            }
            info!(app = ?versions.app, data = ?versions.data, "the latest versions");
            *known = Some(versions);
            true
        });
    }

    /// A listing failed: the last one stands, and without one there is nothing to offer.
    fn unlisted(&self) {
        self.known.send_if_modified(|known| {
            let first = matches!(known, Known::Awaited);
            if first {
                *known = Known::Unlisted;
            }
            first
        });
    }

    /// Where `asset` is kept once fetched; `None` when it doesn't fit, or its release is no
    /// longer offered.
    fn kept(&self, asset: &GhAsset) -> Option<Arc<OnceCell<Bytes>>> {
        let mut files = self.files.lock();
        if files.offered.binary_search(&asset.id).is_err() {
            return None;
        }
        if let Some(kept) = files.kept.get(&asset.id) {
            return Some(kept.cell.clone());
        }
        if files.promised + asset.size > KEEP_BYTES {
            return None;
        }
        files.promised += asset.size;
        let cell = Arc::new(OnceCell::new());
        files.kept.insert(
            asset.id,
            Kept {
                size: asset.size,
                cell: cell.clone(),
            },
        );
        Some(cell)
    }
}

/// The offered releases' files, each fetched once by whichever download comes first. Known by
/// their asset ids: GitHub gives every upload a new one, so a file replaced under its name --
/// `gh release upload --clobber`, or the release deleted and published again under its tag -- is
/// another file.
#[derive(Default)]
struct Files {
    /// The asset ids of the offered releases, sorted: the files that may be kept.
    offered: Vec<u64>,
    kept: HashMap<u64, Kept>,
    /// The sizes of the files given a cell, fetched or not yet.
    promised: u64,
}

struct Kept {
    size: u64,
    cell: Arc<OnceCell<Bytes>>,
}

impl Files {
    /// Offers the files `offered` (sorted asset ids) from now on: the kept files not among them
    /// are let go, the rest stay.
    fn switch(&mut self, offered: Vec<u64>) {
        if self.offered == offered {
            return;
        }
        self.kept.retain(|id, _| offered.binary_search(id).is_ok());
        self.promised = self.kept.values().map(|kept| kept.size).sum();
        self.offered = offered;
    }
}

/// Lists the releases at once and then every `every`, for as long as the service runs. Without a
/// GitHub token there is nothing to list.
pub async fn refresh(app: Arc<App>, every: Duration) {
    if !app.github.configured() {
        return;
    }
    let mut pages = Vec::new();
    loop {
        match list(&app.github, &mut pages).await {
            Ok(()) => app
                .releases
                .listed(pages.iter().flat_map(|page| &page.releases)),
            Err(problem) => {
                warn!(%problem, "GitHub didn't list the releases");
                app.releases.unlisted();
            }
        }
        tokio::time::sleep(every).await;
    }
}

/// Every release GitHub lists, into `pages`: page after page, until one isn't full or
/// [`MAX_PAGES`] are read. Each page is asked with the validator of its copy in `pages`, so a page
/// unchanged since costs no rate limit and stays as it is.
async fn list(github: &GitHub, pages: &mut Vec<Page>) -> Result<(), String> {
    for index in 0..MAX_PAGES {
        let etag = pages.get(index).and_then(|page| page.etag.as_deref());
        match github.releases(index + 1, etag).await? {
            Listed::Changed(page) if index < pages.len() => pages[index] = page,
            Listed::Changed(page) => pages.push(page),
            Listed::Unchanged if index < pages.len() => {}
            Listed::Unchanged => return Err("an unchanged page that was never listed".to_owned()),
        }
        if pages[index].releases.len() < RELEASES_PER_PAGE {
            pages.truncate(index + 1);
            break;
        }
    }
    Ok(())
}

/// The updater's answer for release `tag`: every file downloads from this service, and the
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

/// `GET /api/v1/releases/latest`: the app's latest release, `304` when the updater already has
/// this answer.
pub async fn latest(State(app): State<Arc<App>>, request: Request) -> Response {
    checked(&app, &request);
    let listing = app.releases.listing().await;
    offer(
        listing.as_deref().and_then(|listing| listing.app.as_ref()),
        request.headers(),
    )
}

/// `GET /api/v1/data/latest`: the latest data pack's release, answered the same way.
pub async fn latest_data(State(app): State<Arc<App>>, request: Request) -> Response {
    checked(&app, &request);
    let listing = app.releases.listing().await;
    offer(
        listing.as_deref().and_then(|listing| listing.data.as_ref()),
        request.headers(),
    )
}

/// An update check: counted as one, and as an active install, when the app made it. Anything else
/// that asks -- the site's own pages, which show the latest version -- counts nothing.
fn checked(app: &Arc<App>, request: &Request) {
    if let Some(agent) = AppAgent::of(request.headers()) {
        stats::count(app, Stat::UpdateCheck);
        usage::app_active(app, Who::of(request), agent);
    }
}

/// The updater's answer offering `latest`, `304` when `headers` name its ETag.
fn offer(latest: Option<&Latest>, headers: &HeaderMap) -> Response {
    let Some(latest) = latest else {
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

/// `GET /download/latest`: the site's download button, sent on to the current installer. The
/// landing tag the site's script adds to the link (`?from=reddit`, one of a [`Source`]'s) goes on
/// to the file, which counts the download by it; any other tag is dropped here.
pub async fn latest_installer(State(app): State<Arc<App>>, RawQuery(query): RawQuery) -> Response {
    let listing = app.releases.listing().await;
    let Some(latest) = listing.as_deref().and_then(|listing| listing.app.as_ref()) else {
        return no_release();
    };
    if latest.asset(&latest.payload).is_none() {
        return (StatusCode::NOT_FOUND, "the latest release has no installer").into_response();
    }
    let mut location = download_path(&latest.tag, &latest.payload);
    if let Some(source) = query.as_deref().and_then(Source::in_query) {
        location.push_str("?from=");
        location.push_str(source.tag());
    }
    (
        StatusCode::FOUND,
        [
            (header::LOCATION, location),
            (header::CACHE_CONTROL, "no-cache".to_owned()),
        ],
    )
        .into_response()
}

/// `GET /download/<tag>/<asset>`: a file of an offered release, the app's or the data pack's.
/// Kept in memory after the first download, which fetches it for everyone asking meanwhile.
pub async fn asset(
    State(app): State<Arc<App>>,
    Path((tag, name)): Path<(String, String)>,
    method: Method,
    sent: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let Some(listing) = app.releases.listing().await else {
        return no_release();
    };
    let Some((release, payload)) = listing.release(&tag) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(asset) = release.asset(&name) else {
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
    let body = match app.releases.kept(asset) {
        Some(cell) => match cell.get_or_try_init(|| fetch(&app.github, asset)).await {
            Ok(bytes) => Body::from(bytes.clone()),
            Err(problem) => return unreachable_file(&name, &problem),
        },
        None => match app.github.download(asset.id).await {
            Ok(response) => Body::from_stream(response.bytes_stream()),
            Err(problem) => return unreachable_file(&name, &problem),
        },
    };
    let counted = if name == release.payload {
        payload
    } else {
        Stat::UpdateDownload
    };
    stats::count(&app, counted);
    // The installer, told apart by who took it: the updater, or a person's browser.
    if counted == Stat::Download {
        let version = tag.strip_prefix('v').unwrap_or(&tag);
        usage::installer_downloaded(&app, &sent, version, query.as_deref());
    }
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

/// Published releases tagged `tags`, with no files: what the tests of other modules list.
#[cfg(test)]
pub(crate) fn published(tags: &[&str]) -> Vec<GhRelease> {
    tags.iter()
        .map(|tag| GhRelease {
            tag_name: (*tag).to_owned(),
            draft: false,
            prerelease: false,
            assets: Vec::new(),
        })
        .collect()
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

    fn release(tag: &str, assets: Vec<GhAsset>) -> GhRelease {
        GhRelease {
            tag_name: tag.to_owned(),
            draft: false,
            prerelease: false,
            assets,
        }
    }

    fn tags(picked: &Picked) -> (Option<String>, Option<String>) {
        (
            picked
                .app
                .as_ref()
                .map(|(release, _)| release.tag_name.clone()),
            picked
                .data
                .as_ref()
                .map(|(release, _)| release.tag_name.clone()),
        )
    }

    #[test]
    fn the_answer_downloads_everything_from_this_service() {
        let release = release(
            "v0.1.0",
            vec![
                asset(1, "PoE2-Oracle-Setup-0.1.0.exe", 9_000_000, "uploaded"),
                asset(2, "SHA256SUMS", 96, "uploaded"),
                asset(3, "odd name+ü.txt", 5, "uploaded"),
                asset(4, "SHA256SUMS.sig", 88, "open"),
            ],
        );
        let listing = Listing::new([&release], "https://oracle.pushka.biz");
        let latest = listing.app.as_ref().unwrap();
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
        assert_eq!(latest.payload, "PoE2-Oracle-Setup-0.1.0.exe");
        assert_eq!(
            listing.versions,
            Versions {
                app: Some("0.1.0".to_owned()),
                data: None
            }
        );
    }

    #[test]
    fn each_kind_has_its_own_latest_release() {
        let published = |tag: &str| release(tag, Vec::new());
        let draft = |tag: &str| GhRelease {
            draft: true,
            ..published(tag)
        };
        let prerelease = |tag: &str| GhRelease {
            prerelease: true,
            ..published(tag)
        };
        // Newest first, as GitHub lists them: data packs made after the app's latest release.
        let releases = [
            published("data-2026092602"),
            draft("data-2026092701"),
            prerelease("data-2026092700"),
            draft("v0.11.0"),
            prerelease("v0.12.0"),
            published("v0.9.1"),
            published("v0.10.0"),
            published("data-2026092601"),
            published("v0.9.0"),
            published("data-2026092503"),
        ];
        assert_eq!(
            tags(&pick(&releases)),
            (
                Some("v0.10.0".to_owned()),
                Some("data-2026092602".to_owned())
            ),
            "by version, not by name or date; drafts and prereleases never"
        );
        let listing = Listing::new(&releases, "https://oracle.example");
        assert_eq!(
            listing.versions,
            Versions {
                app: Some("0.10.0".to_owned()),
                data: Some(2026092602),
            }
        );
        assert_eq!(
            listing.data.unwrap().payload,
            "PoE2-Oracle-Data-2026092602.zip"
        );

        // A newer data pack leaves the app's latest release as it was, and the other way round.
        assert_eq!(
            tags(&pick(&releases[1..])),
            (
                Some("v0.10.0".to_owned()),
                Some("data-2026092601".to_owned())
            )
        );
        assert_eq!(
            tags(&pick([&published("v0.10.1")].into_iter().chain(&releases))),
            (
                Some("v0.10.1".to_owned()),
                Some("data-2026092602".to_owned())
            )
        );
    }

    #[test]
    fn versions_go_by_semver_precedence_and_ties_by_the_newer_release() {
        let published = |tag: &str| release(tag, Vec::new());
        let app = |names: &[&str]| {
            let releases: Vec<GhRelease> = names.iter().map(|name| published(name)).collect();
            tags(&pick(&releases)).0
        };
        // A release candidate published as a release ranks below its final version.
        assert_eq!(app(&["v1.0.0-rc.1", "v1.0.0"]).as_deref(), Some("v1.0.0"));
        assert_eq!(
            app(&["v1.0.0-rc.2", "v1.0.0-rc.10"]).as_deref(),
            Some("v1.0.0-rc.10")
        );
        // Build metadata doesn't rank: of equal versions, the one GitHub lists first -- the newer.
        assert_eq!(
            app(&["v1.0.0+build.2", "v1.0.0+build.10"]).as_deref(),
            Some("v1.0.0+build.2")
        );
    }

    #[test]
    fn a_tag_of_neither_kind_is_never_offered() {
        for tag in [
            "v0.10",
            "0.13.0",
            "V0.14.0",
            "v01.0.0",
            "latest",
            "data-",
            "data-abc",
            "data-007",
            "data-+7",
            "data--7",
            "data-7.0",
            "Data-7",
            "data-18446744073709551616",
        ] {
            assert_eq!(Kind::of(tag), None, "{tag}");
        }
        assert_eq!(Kind::of("data-7"), Some(Kind::Data(7)));
        assert_eq!(
            Kind::of("v0.1.0-rc.1"),
            Some(Kind::App(Version::parse("0.1.0-rc.1").unwrap()))
        );
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
    fn kept_files_follow_their_asset_ids() {
        let releases = Releases::new("https://oracle.example".to_owned(), true);
        let app = release(
            "v0.1.0",
            vec![
                asset(1, "PoE2-Oracle-Setup-0.1.0.exe", 9_000_000, "uploaded"),
                asset(2, "SHA256SUMS", 96, "uploaded"),
            ],
        );
        let data = |id: u64| {
            release(
                &format!("data-{id}"),
                vec![asset(id, &data_pack_asset(id), 400_000, "uploaded")],
            )
        };
        releases.listed([&data(10), &app]);
        let sums = releases.kept(&app.assets[1]).unwrap();
        sums.set(Bytes::from_static(b"the first sums")).unwrap();
        let pack = releases.kept(&data(10).assets[0]).unwrap();
        pack.set(Bytes::from_static(b"the first pack")).unwrap();

        // A new data pack: the old one is let go, the app's files stay.
        releases.listed([&data(11), &data(10), &app]);
        assert!(Arc::ptr_eq(&sums, &releases.kept(&app.assets[1]).unwrap()));
        assert!(
            releases.kept(&data(10).assets[0]).is_none(),
            "a pack no longer offered isn't kept"
        );

        // `gh release upload --clobber`: the same tag and name, a new upload with a new id.
        let replaced = GhRelease {
            assets: vec![
                app.assets[0].clone(),
                asset(3, "SHA256SUMS", 97, "uploaded"),
            ],
            ..app.clone()
        };
        releases.listed([&data(11), &replaced]);
        let fresh = releases.kept(&replaced.assets[1]).unwrap();
        assert!(fresh.get().is_none(), "the first file's bytes are let go");
        assert!(releases.kept(&app.assets[1]).is_none());
    }

    /// An app release with its installer (`installer_id`) and its `SHA256SUMS` (the next id).
    fn app_release(version: &str, installer_id: u64, installer: &[u8]) -> GhRelease {
        release(
            &format!("v{version}"),
            vec![
                asset(
                    installer_id,
                    &installer_asset(version),
                    installer.len() as u64,
                    "uploaded",
                ),
                asset(
                    installer_id + 1,
                    "SHA256SUMS",
                    installer.len() as u64,
                    "uploaded",
                ),
            ],
        )
    }

    #[test]
    fn counters_are_named_for_the_newest_sixteen_published_releases_only() {
        let mut releases: Vec<GhRelease> = (0..20)
            .map(|patch| release(&format!("v0.1.{patch}"), Vec::new()))
            .collect();
        // Only a published, plain release counts: not a draft, a prerelease, a pre-release or
        // build part, a data pack, or a tag of neither kind.
        releases.push(GhRelease {
            draft: true,
            ..release("v0.9.0", Vec::new())
        });
        releases.push(GhRelease {
            prerelease: true,
            ..release("v0.9.1", Vec::new())
        });
        releases.extend(
            [
                "v0.9.2-rc.1",
                "v0.9.3+build.5",
                "data-2026092601",
                "nightly",
                "0.9.4",
                "v1",
            ]
            .map(|tag| release(tag, Vec::new())),
        );
        let releases: Vec<&GhRelease> = releases.iter().collect();
        // Sixteen, the newest first by number, not by text: 0.1.19 down to 0.1.4.
        let expected: Vec<String> = (4..20).rev().map(|patch| format!("0.1.{patch}")).collect();
        assert_eq!(known_versions(&releases), expected);
        assert_eq!(known_versions(&[]), Vec::<String>::new());
    }

    /// A GitHub that hands any file out as `bytes`.
    async fn github_serving(bytes: &'static [u8]) -> String {
        let router = axum::Router::new().fallback(move || async move { bytes });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{address}")
    }

    /// `name` of release 0.1.3 downloaded as a client with `agent` asks, from `query`.
    async fn download(app: &Arc<App>, name: &str, agent: &str, query: Option<&str>) -> Response {
        let mut sent = HeaderMap::new();
        sent.insert(header::USER_AGENT, agent.parse().unwrap());
        let file = Path(("v0.1.3".to_owned(), name.to_owned()));
        super::asset(
            State(app.clone()),
            file,
            Method::GET,
            sent,
            RawQuery(query.map(str::to_owned)),
        )
        .await
    }

    /// Today's counts, once the background tasks that made them are in.
    async fn counted_today(app: &App) -> crate::stats::Snapshot {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let now = crate::moscow::now();
        crate::stats::snapshots(&app.store, &[crate::moscow::Day::of(now)], now)
            .await
            .unwrap()
            .remove(0)
    }

    #[tokio::test]
    async fn an_installer_download_counts_by_who_took_it_and_by_the_tag_it_came_with() {
        let installer: &'static [u8] = b"MZ the installer";
        let app = App::new(crate::Config {
            github_api: github_serving(installer).await,
            github_token: Some("token".to_owned()),
            ..crate::Config::default()
        })
        .unwrap();
        app.releases.listed([
            &app_release("0.1.3", 1, installer),
            &app_release("0.1.2", 3, installer),
        ]);
        let name = installer_asset("0.1.3");
        for (agent, query) in [
            // The site's button, with the landing tag its script adds, and without.
            ("Mozilla/5.0 Firefox/130.0", Some("from=reddit")),
            ("Mozilla/5.0 Firefox/130.0", None),
            // A script, with a tag nobody published.
            ("curl/8.5.0", Some("from=evil")),
            // The updater of 0.1.2, taking the release after it; a tag means nothing to it.
            ("PoE2-Oracle/0.1.2", None),
            ("PoE2-Oracle/0.1.2", Some("from=reddit")),
        ] {
            let file = download(&app, &name, agent, query).await;
            assert_eq!(file.status(), StatusCode::OK, "{agent} {query:?}");
        }
        // The updater's other file is no installer; and a HEAD, which only checks the link, is
        // no download.
        let sums = download(&app, "SHA256SUMS", "PoE2-Oracle/0.1.2", None).await;
        assert_eq!(sums.status(), StatusCode::OK);
        let mut sent = HeaderMap::new();
        sent.insert(header::USER_AGENT, "Mozilla/5.0".parse().unwrap());
        let file = Path(("v0.1.3".to_owned(), name.clone()));
        let head = super::asset(State(app.clone()), file, Method::HEAD, sent, RawQuery(None));
        assert_eq!(head.await.status(), StatusCode::OK);

        let today = counted_today(&app).await;
        // The old counter reads all the installers, as it always did.
        assert_eq!(today.get("download"), 5);
        assert_eq!(today.get("download_site"), 3);
        assert_eq!(today.get("download_site_from_reddit"), 1);
        assert_eq!(today.sum("download_site_from_"), 1);
        assert_eq!(today.get("download_update"), 2);
        assert_eq!(today.get("download_update_v_0.1.3"), 2);
        assert_eq!(today.get("update_download"), 1);
    }

    #[tokio::test]
    async fn an_update_check_counts_the_app_as_active_and_a_browser_as_nothing() {
        let app = App::new(crate::Config::default()).unwrap();
        app.releases.listed(&published(&["v0.1.3"]));
        // The app asks for the release and for the data pack; another app asks; a browser asks.
        for (path, client, agent) in [
            (
                "/api/v1/releases/latest",
                "203.0.113.1",
                "PoE2-Oracle/0.1.3",
            ),
            ("/api/v1/data/latest", "203.0.113.1", "PoE2-Oracle/0.1.3"),
            (
                "/api/v1/releases/latest",
                "203.0.113.2",
                "PoE2-Oracle/0.1.3",
            ),
            ("/api/v1/releases/latest", "203.0.113.3", "Mozilla/5.0"),
        ] {
            let request = Request::builder()
                .uri(path)
                .header("x-forwarded-for", client)
                .header("user-agent", agent)
                .body(Body::empty())
                .unwrap();
            if path.contains("data") {
                latest_data(State(app.clone()), request).await;
            } else {
                latest(State(app.clone()), request).await;
            }
        }
        let today = counted_today(&app).await;
        // Three from apps; the browser's (the site showing the latest version) counts nothing.
        assert_eq!(today.get("update_check"), 3);
        assert_eq!(today.get("uniq_app_day"), 2);
        assert_eq!(today.get("uniq_app_day_v_0.1.3"), 2);
        // Asking is not connecting.
        assert_eq!(today.get("app_conn"), 0);
    }
}
