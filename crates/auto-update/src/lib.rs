//! Keeps an installed PoE2 Oracle current from its GitHub releases.
//!
//! The app drives three steps: [`check_for_update`] asks GitHub for the latest release,
//! [`download_update`] fetches that release's installer and checks it against the release's
//! `SHA256SUMS`, and [`apply_update`] starts the installer silently. The app then quits and the
//! installer finishes the job: it waits for the app to exit (closing it if it lingers), replaces
//! the files, keeps the user's autostart choice and starts the new version.
//!
//! There is no separate swap-on-quit helper exe: the NSIS installer (`packaging/installer.nsi`)
//! has to close a running copy anyway for manual installs, so updates go through exactly that
//! path, and there is no second binary that would itself need updating.
//!
//! The release layout relied on here is what `packaging/build-release.ps1` and
//! `.github/workflows/release.yml` publish: tag `vX.Y.Z`, assets `PoE2-Oracle-Setup-X.Y.Z.exe`
//! and `SHA256SUMS` (`sha256sum` format), attached before the release becomes visible.

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, bail, ensure};
use futures::AsyncReadExt as _;
use http_client::http::header::{ETAG, IF_NONE_MATCH};
use http_client::{
    AsyncBody, HttpClient, HttpRequestExt as _, RedirectPolicy, Request, StatusCode, Url,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub use semver::Version;

/// Where the latest release is asked for: this repository's GitHub releases, unless the build sets
/// `POE2_ORACLE_RELEASES_URL` to another URL answering the same way -- the releases of a separate
/// public repository while this one stays private. It must be https (checked below, at build
/// time): the answer names the installer the app runs, and a plain-http one could come from
/// anyone along the way.
///
/// Testing an update end to end without a public release takes a local stand-in serving a test
/// release over plain http on 127.0.0.1, which only a build with the `local-release-server`
/// feature accepts -- for this URL and for the downloads the release lists. The release script
/// refuses that feature:
///
/// ```text
/// $env:POE2_ORACLE_RELEASES_URL = 'http://127.0.0.1:8765/releases/latest'
/// cargo build -p poe2-oracle --release --features auto-update/local-release-server
/// ```
const LATEST_RELEASE_URL: &str = match option_env!("POE2_ORACLE_RELEASES_URL") {
    Some(url) => url,
    None => "https://api.github.com/repos/mttzzz/poe2-oracle/releases/latest",
};
const _: () = assert!(
    is_allowed_release_url(LATEST_RELEASE_URL),
    "POE2_ORACLE_RELEASES_URL must be https; http://127.0.0.1 needs the local-release-server feature"
);
/// GitHub rejects API requests without a User-Agent; its docs ask for the app's name. Every crate
/// shares the workspace version, so this is the running app's version too.
const USER_AGENT: &str = concat!("PoE2-Oracle/", env!("CARGO_PKG_VERSION"));
const INSTALLER_PREFIX: &str = "PoE2-Oracle-Setup-";
const CHECKSUMS_ASSET: &str = "SHA256SUMS";
/// `packaging/installer.nsi`'s switches: no UI, then start the app again once done.
const INSTALLER_ARGS: [&str; 2] = ["/S", "/relaunch"];
const METADATA_TIMEOUT: Duration = Duration::from_secs(20);
/// The whole installer download (a few MB): generous for a slow line, yet a stalled connection
/// cannot keep "downloading" forever.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// The release JSON and SHA256SUMS are a few KB; a body far larger is not what was asked for.
const MAX_METADATA_BYTES: u64 = 1 << 20;
/// GitHub's last latest-release answer, kept in the updates folder as [`KeptRelease`].
const KEPT_RELEASE_FILE: &str = "latest-release.json";

/// A published release newer than the running app, with what [`download_update`] needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    /// The release's version: its tag without the `v`.
    pub version: Version,
    /// The release's GitHub page, where its notes are -- for a "what's new" link.
    pub release_url: String,
    installer: Asset,
    checksums: Asset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Asset {
    name: String,
    url: String,
    size: u64,
}

/// The fields used from GitHub's release object (`GET /repos/{owner}/{repo}/releases/latest`).
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// GitHub's error body: `{"message": "...", "documentation_url": "..."}`.
#[derive(Deserialize)]
struct GitHubError {
    message: String,
}

/// GitHub's last latest-release answer with its `ETag`, kept on disk: the next check sends the tag
/// as `If-None-Match`, and GitHub's `304 Not Modified` -- which it doesn't count against the
/// anonymous API's 60 requests an hour per IP, shared by every device behind one address -- stands
/// for the kept body. Live 2026-09-23 the owner's IP ran out of those 60 while the app was
/// relaunched a few dozen times, and the update check failed with a 403.
#[derive(Serialize, Deserialize)]
struct KeptRelease {
    etag: String,
    body: String,
}

/// Returns the latest published release when it is newer than `current` by semver precedence --
/// `0.2.0` updates `0.2.0-rc.1`, and nothing ever downgrades. `cache_dir` keeps GitHub's last
/// answer ([`KeptRelease`]), so an unchanged release costs none of the anonymous API's hourly
/// allowance.
///
/// Every failure is an error, never a silent "up to date": network trouble, a GitHub refusal (its
/// anonymous API allows 60 requests an hour per IP), a 404 (no release published yet, or the
/// repository is private -- both look the same without credentials), and a latest release lacking
/// its installer or `SHA256SUMS` (the release workflow attaches both before publishing, so that is
/// a broken release, not one still uploading).
pub async fn check_for_update(
    client: &Arc<dyn HttpClient>,
    current: &Version,
    cache_dir: &Path,
) -> Result<Option<UpdateInfo>> {
    let kept_path = cache_dir.join(KEPT_RELEASE_FILE);
    let kept: Option<KeptRelease> = fs::read(&kept_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let mut request = Request::get(LATEST_RELEASE_URL)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(kept) = &kept {
        request = request.header(IF_NONE_MATCH, kept.etag.as_str());
    }
    let request = request
        .follow_redirects(RedirectPolicy::FollowAll)
        .timeout(METADATA_TIMEOUT)
        .body(AsyncBody::default())?;
    let mut response = client
        .send(request)
        .await
        .context("asking GitHub for the latest release")?;
    let status = response.status();
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = read_capped(response.body_mut(), MAX_METADATA_BYTES)
        .await
        .context("reading GitHub's latest-release response")?;
    let body = match (status, kept) {
        (StatusCode::NOT_MODIFIED, Some(kept)) => kept.body.into_bytes(),
        (StatusCode::NOT_FOUND, _) => bail!(
            "GitHub lists no release of PoE2 Oracle (404): none is published yet, \
             or the repository is private"
        ),
        (status, _) if !status.is_success() => {
            let message = serde_json::from_slice::<GitHubError>(&body)
                .map(|error| error.message)
                .unwrap_or_else(|_| String::from_utf8_lossy(&body).into_owned());
            bail!("GitHub answered {status} for the latest release: {message}");
        }
        _ => {
            // Best effort: without a kept copy the next check just asks in full.
            if let Some(etag) = etag {
                let kept = KeptRelease {
                    etag,
                    body: String::from_utf8_lossy(&body).into_owned(),
                };
                if let Ok(json) = serde_json::to_vec(&kept) {
                    let _ =
                        fs::create_dir_all(cache_dir).and_then(|()| fs::write(&kept_path, json));
                }
            }
            body
        }
    };
    let release: Release =
        serde_json::from_slice(&body).context("parsing GitHub's latest-release JSON")?;

    let tag = release.tag_name.as_str();
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .with_context(|| format!("release tag {tag:?} is not vX.Y.Z"))?;
    if version.cmp_precedence(current).is_le() {
        return Ok(None);
    }
    let asset = |name: &str| -> Result<Asset> {
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .with_context(|| format!("release {tag} has no {name} asset"))?;
        ensure!(
            is_release_download(&asset.browser_download_url),
            "release {tag}'s {name} isn't on GitHub: {}",
            asset.browser_download_url
        );
        Ok(Asset {
            name: asset.name.clone(),
            url: asset.browser_download_url.clone(),
            size: asset.size,
        })
    };
    Ok(Some(UpdateInfo {
        installer: asset(&format!("{INSTALLER_PREFIX}{version}.exe"))?,
        checksums: asset(CHECKSUMS_ASSET)?,
        release_url: release.html_url,
        version,
    }))
}

/// Downloads `update`'s installer into `dir` and returns its path, but only once its SHA-256
/// matches the release's `SHA256SUMS` and its size the one GitHub lists; a mismatching or
/// interrupted download is deleted, never returned.
///
/// `dir` is the app's own updates folder (e.g. `<cache dir>/updates`, created if missing).
/// Installers left there by earlier updates are deleted first -- only files named like ours. The
/// body streams into `<installer>.part`, renamed once verified, so an interrupted download never
/// leaves a runnable-looking file. File writes block: run this on a background executor.
pub async fn download_update(
    client: &Arc<dyn HttpClient>,
    update: &UpdateInfo,
    dir: &Path,
) -> Result<PathBuf> {
    let sums = get_capped(client, &update.checksums.url)
        .await
        .context("downloading SHA256SUMS")?;
    let sums = String::from_utf8(sums).context("SHA256SUMS is not text")?;
    let expected = expected_sha256(&sums, &update.installer.name)?;

    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    remove_old_installers(dir);
    let partial = dir.join(format!("{}.part", update.installer.name));
    if let Err(error) = download_verified(client, &update.installer, &expected, &partial).await {
        let _ = fs::remove_file(&partial);
        return Err(error.context(format!("downloading {}", update.installer.name)));
    }
    let installer = dir.join(&update.installer.name);
    fs::rename(&partial, &installer)
        .with_context(|| format!("moving the verified installer to {}", installer.display()))?;
    Ok(installer)
}

/// Starts the installer [`download_update`] verified, silently: it waits for this process to
/// exit, replaces the files, keeps the autostart choice and starts the new version
/// (`packaging/installer.nsi`).
///
/// Quit the app as soon as this returns `Ok` -- the normal way (`cx.quit()`), so the tray icon is
/// removed: the installer gives a quitting app 10 s, then force-closes it, and a force-closed
/// process leaves its tray icon behind until the mouse passes over it.
pub fn apply_update(installer: &Path) -> Result<()> {
    Command::new(installer)
        .args(INSTALLER_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("starting {}", installer.display()))?;
    Ok(())
}

fn asset_request(url: &str, timeout: Duration) -> Result<Request<AsyncBody>> {
    // Release assets redirect from github.com to GitHub's storage host.
    Ok(Request::get(url)
        .header("User-Agent", USER_AGENT)
        .follow_redirects(RedirectPolicy::FollowAll)
        .timeout(timeout)
        .body(AsyncBody::default())?)
}

async fn get_capped(client: &Arc<dyn HttpClient>, url: &str) -> Result<Vec<u8>> {
    let mut response = client.send(asset_request(url, METADATA_TIMEOUT)?).await?;
    ensure!(
        response.status().is_success(),
        "{url} answered {}",
        response.status()
    );
    read_capped(response.body_mut(), MAX_METADATA_BYTES).await
}

async fn read_capped(body: &mut AsyncBody, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    body.take(limit + 1).read_to_end(&mut bytes).await?;
    ensure!(
        bytes.len() as u64 <= limit,
        "the response is larger than {limit} bytes"
    );
    Ok(bytes)
}

async fn download_verified(
    client: &Arc<dyn HttpClient>,
    asset: &Asset,
    expected: &[u8; 32],
    path: &Path,
) -> Result<()> {
    let mut response = client
        .send(asset_request(&asset.url, DOWNLOAD_TIMEOUT)?)
        .await?;
    ensure!(
        response.status().is_success(),
        "{} answered {}",
        asset.url,
        response.status()
    );
    let mut file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = response.body_mut().read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        received += read as u64;
        ensure!(
            received <= asset.size,
            "more than the {} bytes GitHub lists arrived",
            asset.size
        );
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])?;
    }
    ensure!(
        received == asset.size,
        "only {received} of {} bytes arrived",
        asset.size
    );
    ensure!(
        hasher.finalize().as_slice() == expected,
        "its SHA-256 does not match SHA256SUMS"
    );
    file.sync_all()?;
    Ok(())
}

/// Best effort: a leftover only costs disk space, and never shadows the download (that has a
/// `.part` name until verified, then replaces any same-named file).
fn remove_old_installers(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(INSTALLER_PREFIX)
            && (name.ends_with(".exe") || name.ends_with(".exe.part"))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The digest `sums` lists for `file_name`, in `sha256sum`'s format: `<hex>  <name>`, or
/// `<hex> *<name>` in binary mode; either line ending.
fn expected_sha256(sums: &str, file_name: &str) -> Result<[u8; 32]> {
    sums.lines()
        .find_map(|line| {
            let (hex, name) = line.split_at_checked(64)?;
            let name = name.strip_prefix(' ')?;
            let name = name.strip_prefix([' ', '*']).unwrap_or(name);
            if name.trim_end() != file_name {
                return None;
            }
            let mut digest = [0u8; 32];
            let (pairs, _) = hex.as_bytes().as_chunks::<2>();
            for (byte, &[high, low]) in digest.iter_mut().zip(pairs) {
                let high = char::from(high).to_digit(16)?;
                let low = char::from(low).to_digit(16)?;
                *byte = (high << 4 | low) as u8;
            }
            Some(digest)
        })
        .with_context(|| format!("SHA256SUMS lists no SHA-256 for {file_name}"))
}

/// Whether the update check may ask `url`: https -- or, in a `local-release-server` build, plain
/// http to 127.0.0.1. A `const fn`, so that a build baking in any other URL fails
/// (`LATEST_RELEASE_URL`).
const fn is_allowed_release_url(url: &str) -> bool {
    starts_with(url, "https://")
        || (cfg!(feature = "local-release-server")
            && (starts_with(url, "http://127.0.0.1:") || starts_with(url, "http://127.0.0.1/")))
}

/// `str::starts_with`, which isn't a `const fn`.
const fn starts_with(text: &str, prefix: &str) -> bool {
    let (text, prefix) = (text.as_bytes(), prefix.as_bytes());
    if text.len() < prefix.len() {
        return false;
    }
    let mut index = 0;
    while index < prefix.len() {
        if text[index] != prefix[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// Whether a release's asset may be downloaded from `url`: https on GitHub's own hosts -- a
/// release's download links are on github.com, which redirects to its storage -- or, in a
/// `local-release-server` build, plain http on 127.0.0.1. The installer it names is run.
fn is_release_download(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    match (url.scheme(), url.host_str()) {
        ("https", Some("github.com" | "objects.githubusercontent.com")) => url.port().is_none(),
        ("http", Some("127.0.0.1")) => cfg!(feature = "local-release-server"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use futures::executor::block_on;
    use futures::future::BoxFuture;
    use http_client::http::HeaderValue;
    use http_client::{Response, Url};
    use parking_lot::Mutex;
    use serde_json::json;

    use super::*;

    const INSTALLER: &[u8] = b"MZ\x90\x00 stand-in for an NSIS installer";

    /// Serves canned bodies by exact URL (anything else: 404) and records each request's URL,
    /// User-Agent and `If-None-Match`. With an `etag`, the latest release carries it, and a request
    /// asking `If-None-Match` it gets GitHub's empty `304`.
    struct CannedClient {
        responses: HashMap<String, (u16, Vec<u8>)>,
        requests: Mutex<Vec<(String, Option<String>)>>,
        etag: Option<&'static str>,
        conditions: Mutex<Vec<Option<String>>>,
    }

    impl CannedClient {
        fn new(responses: impl IntoIterator<Item = (String, u16, Vec<u8>)>) -> Arc<Self> {
            Arc::new(Self {
                responses: responses
                    .into_iter()
                    .map(|(url, status, body)| (url, (status, body)))
                    .collect(),
                requests: Mutex::new(Vec::new()),
                etag: None,
                conditions: Mutex::new(Vec::new()),
            })
        }
    }

    impl HttpClient for CannedClient {
        fn user_agent(&self) -> Option<&HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&Url> {
            None
        }

        fn send(
            &self,
            request: Request<AsyncBody>,
        ) -> BoxFuture<'static, Result<Response<AsyncBody>>> {
            let url = request.uri().to_string();
            let header = |name: &str| {
                request
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let condition = header("if-none-match");
            self.requests
                .lock()
                .push((url.clone(), header("user-agent")));
            self.conditions.lock().push(condition.clone());
            let etag = self.etag.filter(|_| url == LATEST_RELEASE_URL);
            let (status, body) = if etag.is_some() && condition.as_deref() == etag {
                (304, Vec::new())
            } else {
                self.responses
                    .get(&url)
                    .cloned()
                    .unwrap_or((404, Vec::new()))
            };
            let mut response = Response::builder().status(status);
            if let Some(etag) = etag {
                response = response.header("etag", etag);
            }
            let response = response
                .body(AsyncBody::from(body))
                .map_err(anyhow::Error::from);
            Box::pin(async move { response })
        }
    }

    fn download_url(tag: &str, name: &str) -> String {
        format!("https://github.com/mttzzz/poe2-oracle/releases/download/{tag}/{name}")
    }

    /// GitHub's latest-release answer (trimmed, same shape) for `tag` with `assets` as
    /// `(name, size)`, plus the given asset downloads.
    fn github(
        tag: &str,
        assets: &[(&str, usize)],
        downloads: Vec<(&str, Vec<u8>)>,
    ) -> Arc<CannedClient> {
        let release = json!({
            "url": "https://api.github.com/repos/mttzzz/poe2-oracle/releases/250000001",
            "html_url": format!("https://github.com/mttzzz/poe2-oracle/releases/tag/{tag}"),
            "id": 250000001,
            "tag_name": tag,
            "name": format!("PoE2 Oracle {tag}"),
            "draft": false,
            "prerelease": false,
            "published_at": "2026-09-22T18:00:00Z",
            "body": "Release notes",
            "assets": assets.iter().map(|(name, size)| json!({
                "name": name,
                "size": size,
                "content_type": "application/octet-stream",
                "browser_download_url": download_url(tag, name),
            })).collect::<Vec<_>>(),
        });
        let mut responses = vec![(
            LATEST_RELEASE_URL.to_owned(),
            200,
            release.to_string().into_bytes(),
        )];
        responses.extend(
            downloads
                .into_iter()
                .map(|(name, body)| (download_url(tag, name), 200, body)),
        );
        CannedClient::new(responses)
    }

    /// A v0.2.0 release whose SHA256SUMS lists `listed_digest_of`'s hash for the installer, which
    /// serves [`INSTALLER`].
    fn release_0_2_0(listed_digest_of: &[u8]) -> Arc<CannedClient> {
        let installer = "PoE2-Oracle-Setup-0.2.0.exe";
        let sums = format!(
            "{:x}  poe2-oracle-0.2.0-symbols.zip\n{:x}  {installer}\n",
            Sha256::digest(b"other asset"),
            Sha256::digest(listed_digest_of),
        );
        github(
            "v0.2.0",
            &[(CHECKSUMS_ASSET, sums.len()), (installer, INSTALLER.len())],
            vec![
                (CHECKSUMS_ASSET, sums.into_bytes()),
                (installer, INSTALLER.to_vec()),
            ],
        )
    }

    /// A check with nothing kept from an earlier one.
    fn check(client: &Arc<CannedClient>, current: &str) -> Result<Option<UpdateInfo>> {
        let dir = tempfile::tempdir().unwrap();
        check_in(client, current, dir.path())
    }

    fn check_in(
        client: &Arc<CannedClient>,
        current: &str,
        cache_dir: &Path,
    ) -> Result<Option<UpdateInfo>> {
        let client: Arc<dyn HttpClient> = client.clone();
        block_on(check_for_update(
            &client,
            &Version::parse(current).unwrap(),
            cache_dir,
        ))
    }

    #[test]
    fn an_unchanged_release_is_read_from_the_kept_answer() {
        let mut github = Arc::into_inner(release_0_2_0(INSTALLER)).expect("sole owner");
        github.etag = Some("W/\"release-v0.2.0\"");
        let github = Arc::new(github);
        let dir = tempfile::tempdir().unwrap();

        let first = check_in(&github, "0.1.0", dir.path()).unwrap();
        // The second check asks conditionally, gets GitHub's empty 304 -- which doesn't count
        // against the hourly limit -- and reads the same release from what the first one kept.
        let second = check_in(&github, "0.1.0", dir.path()).unwrap();
        assert_eq!(
            *github.conditions.lock(),
            [None, Some("W/\"release-v0.2.0\"".to_owned())]
        );
        assert_eq!(second, first);
        assert_eq!(
            second.map(|update| update.version),
            Some(Version::new(0, 2, 0))
        );
    }

    #[test]
    fn newer_release_downloads_as_its_verified_installer() {
        let github = release_0_2_0(INSTALLER);
        let update = check(&github, "0.1.0")
            .unwrap()
            .expect("0.2.0 is newer than 0.1.0");
        assert_eq!(update.version, Version::new(0, 2, 0));
        assert_eq!(
            update.release_url,
            "https://github.com/mttzzz/poe2-oracle/releases/tag/v0.2.0"
        );
        let (api_url, user_agent) = github.requests.lock()[0].clone();
        assert_eq!(api_url, LATEST_RELEASE_URL);
        assert!(
            user_agent.is_some(),
            "GitHub refuses API calls without a User-Agent"
        );

        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("PoE2-Oracle-Setup-0.1.0.exe"),
            b"previous update",
        )
        .unwrap();
        fs::write(dir.path().join("settings.json"), b"{}").unwrap();
        let client: Arc<dyn HttpClient> = github;
        let installer = block_on(download_update(&client, &update, dir.path())).unwrap();

        assert_eq!(installer, dir.path().join("PoE2-Oracle-Setup-0.2.0.exe"));
        assert_eq!(fs::read(&installer).unwrap(), INSTALLER);
        let mut left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["PoE2-Oracle-Setup-0.2.0.exe", "settings.json"]);
    }

    #[test]
    fn tampered_installer_is_rejected_and_deleted() {
        let github = release_0_2_0(b"the installer the release was built with");
        let update = check(&github, "0.1.0").unwrap().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let client: Arc<dyn HttpClient> = github;

        let error = block_on(download_update(&client, &update, dir.path())).unwrap_err();

        assert!(format!("{error:#}").contains("SHA-256"), "{error:#}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn only_a_newer_version_is_an_update() {
        let github = release_0_2_0(INSTALLER);
        assert_eq!(check(&github, "0.2.0").unwrap(), None);
        assert_eq!(check(&github, "0.3.0").unwrap(), None);
        let update = check(&github, "0.2.0-rc.1").unwrap();
        assert_eq!(
            update.map(|update| update.version),
            Some(Version::new(0, 2, 0))
        );
    }

    #[test]
    fn release_missing_its_installer_is_an_error() {
        // The installer of another version must not be taken for this one's.
        let github = github(
            "v0.2.0",
            &[(CHECKSUMS_ASSET, 100), ("PoE2-Oracle-Setup-0.1.9.exe", 100)],
            Vec::new(),
        );
        let error = check(&github, "0.1.0").unwrap_err();
        assert!(
            error.to_string().contains("PoE2-Oracle-Setup-0.2.0.exe"),
            "{error:#}"
        );
    }

    #[test]
    fn a_release_whose_installer_is_off_github_is_an_error() {
        // Refused at the check: the tray never offers what it couldn't install.
        let release = json!({
            "html_url": "https://github.com/mttzzz/poe2-oracle/releases/tag/v0.2.0",
            "tag_name": "v0.2.0",
            "assets": [
                {
                    "name": CHECKSUMS_ASSET,
                    "size": 100,
                    "browser_download_url": download_url("v0.2.0", CHECKSUMS_ASSET),
                },
                {
                    "name": "PoE2-Oracle-Setup-0.2.0.exe",
                    "size": 100,
                    "browser_download_url": "http://downloads.example.org/PoE2-Oracle-Setup-0.2.0.exe",
                },
            ],
        });
        let github = CannedClient::new([(
            LATEST_RELEASE_URL.to_owned(),
            200,
            release.to_string().into_bytes(),
        )]);
        let error = check(&github, "0.1.0").unwrap_err();
        assert!(
            error.to_string().contains("downloads.example.org"),
            "{error:#}"
        );
    }

    #[test]
    fn assets_come_from_github_over_https_only() {
        for url in [
            "https://github.com/mttzzz/poe2-oracle/releases/download/v0.2.0/PoE2-Oracle-Setup-0.2.0.exe",
            "https://github.com:443/mttzzz/poe2-oracle/releases/download/v0.2.0/SHA256SUMS",
            "https://objects.githubusercontent.com/github-production-release-asset-2e65be/1/2",
        ] {
            assert!(is_release_download(url), "{url}");
        }
        for url in [
            "http://github.com/mttzzz/poe2-oracle/releases/download/v0.2.0/SHA256SUMS",
            "https://github.com.example.org/mttzzz/poe2-oracle/releases/download/v0.2.0/SHA256SUMS",
            "https://github.com@example.org/SHA256SUMS",
            "https://example.org/github.com/SHA256SUMS",
            "https://github.com:8443/mttzzz/poe2-oracle/releases/download/v0.2.0/SHA256SUMS",
            "https://api.github.com/repos/mttzzz/poe2-oracle/releases/assets/1",
            "file:///C:/Users/Public/PoE2-Oracle-Setup-0.2.0.exe",
            "PoE2-Oracle-Setup-0.2.0.exe",
        ] {
            assert!(!is_release_download(url), "{url}");
        }
        // A local stand-in's, only in a build made for one.
        assert_eq!(
            is_release_download("http://127.0.0.1:8765/PoE2-Oracle-Setup-0.2.0.exe"),
            cfg!(feature = "local-release-server")
        );
    }

    #[test]
    fn the_update_check_asks_over_https_only() {
        assert!(is_allowed_release_url(
            "https://api.github.com/repos/mttzzz/poe2-oracle/releases/latest"
        ));
        for url in [
            "http://api.github.com/repos/mttzzz/poe2-oracle/releases/latest",
            "http://127.0.0.1.example.org/releases/latest",
            "",
        ] {
            assert!(!is_allowed_release_url(url), "{url}");
        }
        assert_eq!(
            is_allowed_release_url("http://127.0.0.1:8765/releases/latest"),
            cfg!(feature = "local-release-server")
        );
    }

    #[test]
    fn no_published_release_is_an_error_not_up_to_date() {
        // What an anonymous client sees for a private repository or one without releases.
        let github = CannedClient::new([(
            LATEST_RELEASE_URL.to_owned(),
            404,
            br#"{"message":"Not Found","documentation_url":"https://docs.github.com/rest/releases/releases#get-the-latest-release","status":"404"}"#.to_vec(),
        )]);
        assert!(check(&github, "0.1.0").is_err());
    }

    #[test]
    fn sha256sums_in_text_binary_and_crlf_forms() {
        let digest = |bytes: &[u8]| -> [u8; 32] { Sha256::digest(bytes).into() };
        let sums = format!(
            "{:x}  a.exe\r\n{:X} *b.exe\r\n{:x}  c.exe",
            Sha256::digest(b"a"),
            Sha256::digest(b"b"),
            Sha256::digest(b"c"),
        );
        assert_eq!(expected_sha256(&sums, "a.exe").unwrap(), digest(b"a"));
        assert_eq!(expected_sha256(&sums, "b.exe").unwrap(), digest(b"b"));
        assert_eq!(expected_sha256(&sums, "c.exe").unwrap(), digest(b"c"));
        assert!(expected_sha256(&sums, "d.exe").is_err());
    }
}
