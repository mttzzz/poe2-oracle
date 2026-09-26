//! Keeps an installed PoE2 Oracle current: a new version shows up in the tray, from the releases
//! the app's web service hands out (`crates/oracle-web`, at oracle.pushka.biz for a release build).
//!
//! The app drives three steps: [`check_for_update`] asks the service for the latest release,
//! [`download_update`] fetches that release's installer and checks it against the release's
//! `SHA256SUMS`, itself trusted only with the release key's signature, and [`apply_update`] starts
//! the installer silently. The app then quits and the installer finishes the job: it waits for the
//! app to exit (closing it if it lingers), replaces the files, keeps the user's autostart choice
//! and starts the new version.
//!
//! There is no separate swap-on-quit helper exe: the NSIS installer (`packaging/installer.nsi`)
//! has to close a running copy anyway for manual installs, so updates go through exactly that
//! path, and there is no second binary that would itself need updating.
//!
//! The release layout relied on here is what `packaging/build-release.ps1` and
//! `.github/workflows/release.yml` make: tag `vX.Y.Z`, assets `PoE2-Oracle-Setup-X.Y.Z.exe`,
//! `SHA256SUMS` (`sha256sum` format) and `SHA256SUMS.sig`, all attached to a draft release that
//! the owner publishes once tested. The service hands out published releases only.
//!
//! Testing an update end to end takes a build pointed at a stand-in service that answers like
//! `crates/oracle-web` (the lane's own, say) over plain http, which needs oracle-protocol's
//! `dev-endpoints` feature; the release script refuses both. The test release still has to be
//! signed with the release key (`release-sign sign`): the app trusts no other.
//!
//! ```text
//! $env:POE2_ORACLE_API_BASE = 'http://poe2-oracle-main.lanes.internal'
//! cargo build -p poe2-oracle --release --features oracle-protocol/dev-endpoints
//! ```

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, VerifyingKey};
use futures::AsyncReadExt as _;
use http_client::http::header::{ETAG, IF_NONE_MATCH};
use http_client::{
    AsyncBody, HttpClient, HttpRequestExt as _, RedirectPolicy, Request, StatusCode,
};
use oracle_protocol::{
    API_BASE, DOWNLOAD_PATH, LATEST_RELEASE_PATH, Release, ReleaseAsset, SUMS_ASSET,
    SUMS_SIGNATURE_ASSET, installer_asset,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub use semver::Version;

/// The release key's public half, standard base64: a release's `SHA256SUMS.sig` must be this
/// key's Ed25519 signature of its `SHA256SUMS`, or nothing of that release runs. The private half
/// is the GitHub repository's `RELEASE_SIGNING_KEY` secret, which `.github/workflows/release.yml`
/// signs with, plus the owner's safe copy; `release-sign keygen` made the pair. Installed copies
/// accept no other key, so a new one reaches players only through an installer they run by hand.
const RELEASE_PUBLIC_KEY: &str = include_str!("../release-signing-key.pub").trim_ascii();
/// Tells the service which app and version asks. Every crate shares the workspace version, so this
/// is the running app's version too.
const USER_AGENT: &str = concat!("PoE2-Oracle/", env!("CARGO_PKG_VERSION"));
/// How [`installer_asset`]'s names start, whatever the version: what [`remove_old_installers`]
/// deletes.
const INSTALLER_PREFIX: &str = "PoE2-Oracle-Setup-";
/// `packaging/installer.nsi`'s switches: no UI, then start the app again once done.
const INSTALLER_ARGS: [&str; 2] = ["/S", "/relaunch"];
const METADATA_TIMEOUT: Duration = Duration::from_secs(20);
/// The whole installer download (a few MB): generous for a slow line, yet a stalled connection
/// cannot keep "downloading" forever.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// The release JSON, SHA256SUMS and its signature are a few KB at most; a body far larger is not
/// what was asked for.
const MAX_METADATA_BYTES: u64 = 1 << 20;
/// The most an installer may be, whatever the release lists: that size comes unsigned, and a
/// service that isn't what it should be -- or a proxy on the way -- could otherwise fill the disk
/// before the signed SHA-256 turns the download down. The real one is a few MB.
const MAX_INSTALLER_BYTES: u64 = 256 << 20;
/// The service's last latest-release answer, kept in the updates folder as [`KeptRelease`].
const KEPT_RELEASE_FILE: &str = "latest-release.json";

/// A published release newer than the running app, with what [`download_update`] needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    /// The release's version: its tag without the `v`.
    pub version: Version,
    installer: ReleaseAsset,
    /// Where the release's `SHA256SUMS` is.
    sums_url: String,
    /// Where its `SHA256SUMS.sig` is.
    signature_url: String,
}

/// The service's last latest-release answer, with its `ETag` and the URL it came from, kept on
/// disk: the next check asks that URL with the tag as `If-None-Match`, and the service's empty
/// `304 Not Modified` stands for the kept body. A build asking another URL ignores it.
#[derive(Serialize, Deserialize)]
struct KeptRelease {
    url: String,
    etag: String,
    body: String,
}

/// Returns the latest published release when it is newer than `current` by semver precedence --
/// `0.2.0` updates `0.2.0-rc.1`, and nothing ever downgrades. `cache_dir` keeps the service's last
/// answer ([`KeptRelease`]), so an unchanged release comes back as an empty `304`.
///
/// Every failure is an error, never a silent "up to date": network trouble, a refusal of the
/// service (a 503 while it knows no release), and a latest release lacking its installer,
/// `SHA256SUMS` or `SHA256SUMS.sig`, listing one that isn't downloaded from the service, or listing
/// an installer over [`MAX_INSTALLER_BYTES`]. The release workflow attaches all three while the
/// release is still a draft, so a published one without them is broken, not still uploading.
pub async fn check_for_update(
    client: &Arc<dyn HttpClient>,
    current: &Version,
    cache_dir: &Path,
) -> Result<Option<UpdateInfo>> {
    let url = oracle_protocol::url(LATEST_RELEASE_PATH);
    let kept_path = cache_dir.join(KEPT_RELEASE_FILE);
    let kept = fs::read(&kept_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<KeptRelease>(&bytes).ok())
        .filter(|kept| kept.url == url);
    let mut request = Request::get(url.as_str())
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json");
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
        .with_context(|| format!("asking {url} for the latest release"))?;
    let status = response.status();
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = read_capped(response.body_mut(), MAX_METADATA_BYTES)
        .await
        .with_context(|| format!("reading {url}'s answer"))?;
    let body = match (status, kept) {
        (StatusCode::NOT_MODIFIED, Some(kept)) => kept.body.into_bytes(),
        (status, _) if !status.is_success() => bail!(
            "{url} answered {status}: {}",
            String::from_utf8_lossy(&body).trim()
        ),
        _ => {
            // Best effort: without a kept copy the next check just asks in full.
            if let Some(etag) = etag {
                let kept = KeptRelease {
                    url: url.clone(),
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
        serde_json::from_slice(&body).with_context(|| format!("parsing {url}'s answer"))?;

    let tag = release.tag_name.as_str();
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .with_context(|| format!("release tag {tag:?} is not vX.Y.Z"))?;
    if version.cmp_precedence(current).is_le() {
        return Ok(None);
    }
    let mut assets = release.assets;
    let mut take = |name: &str| -> Result<ReleaseAsset> {
        let at = assets
            .iter()
            .position(|asset| asset.name == name)
            .with_context(|| format!("release {tag} has no {name}"))?;
        let asset = assets.swap_remove(at);
        ensure!(
            is_service_download(&asset.browser_download_url),
            "release {tag}'s {name} isn't downloaded from {API_BASE}{DOWNLOAD_PATH}/: {}",
            asset.browser_download_url
        );
        Ok(asset)
    };
    let installer = take(&installer_asset(&version.to_string()))?;
    ensure!(
        installer.size <= MAX_INSTALLER_BYTES,
        "release {tag}'s {} is {} bytes, more than the {MAX_INSTALLER_BYTES} an installer may be",
        installer.name,
        installer.size
    );
    Ok(Some(UpdateInfo {
        installer,
        sums_url: take(SUMS_ASSET)?.browser_download_url,
        signature_url: take(SUMS_SIGNATURE_ASSET)?.browser_download_url,
        version,
    }))
}

/// Downloads `update`'s installer into `dir` and returns its path, but only once the release's
/// `SHA256SUMS` carries the release key's signature ([`RELEASE_PUBLIC_KEY`]), and the installer's
/// SHA-256 matches the one listed there and its size the one the release lists. A missing or
/// wrong signature stops the update before the installer is fetched; a mismatching or interrupted
/// download is deleted, never returned.
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
    download_signed(client, update, dir, &release_key()?).await
}

/// [`download_update`], trusting the signatures of `key`.
async fn download_signed(
    client: &Arc<dyn HttpClient>,
    update: &UpdateInfo,
    dir: &Path,
    key: &VerifyingKey,
) -> Result<PathBuf> {
    let sums = get_capped(client, &update.sums_url)
        .await
        .with_context(|| format!("downloading {SUMS_ASSET}"))?;
    let signature = get_capped(client, &update.signature_url)
        .await
        .with_context(|| format!("downloading {SUMS_SIGNATURE_ASSET}"))?;
    check_signature(key, &sums, &signature)?;
    let sums = String::from_utf8(sums).with_context(|| format!("{SUMS_ASSET} is not text"))?;
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

/// [`RELEASE_PUBLIC_KEY`] as the key signatures are checked with.
fn release_key() -> Result<VerifyingKey> {
    let bytes: [u8; 32] = STANDARD
        .decode(RELEASE_PUBLIC_KEY)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .context("the release key isn't 32 bytes of standard base64")?;
    VerifyingKey::from_bytes(&bytes).context("the release key isn't an Ed25519 key")
}

/// Whether `signature`, the text of `SHA256SUMS.sig` (one line of standard base64), is `key`'s
/// signature of `sums`' exact bytes: `verify_strict`, as `release-sign verify` checks it too.
fn check_signature(key: &VerifyingKey, sums: &[u8], signature: &[u8]) -> Result<()> {
    let signature: [u8; 64] = STANDARD
        .decode(signature.trim_ascii())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .with_context(|| format!("{SUMS_SIGNATURE_ASSET} isn't 64 bytes of standard base64"))?;
    key.verify_strict(sums, &Signature::from_bytes(&signature))
        .map_err(|_| {
            anyhow!("{SUMS_SIGNATURE_ASSET} isn't the release key's signature of {SUMS_ASSET}")
        })
}

fn asset_request(url: &str, timeout: Duration) -> Result<Request<AsyncBody>> {
    // The service may send a file on with a redirect; whatever arrives is checked all the same.
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
    asset: &ReleaseAsset,
    expected: &[u8; 32],
    path: &Path,
) -> Result<()> {
    // The size the release lists bounds the stream, and the cap bounds that size.
    ensure!(
        asset.size <= MAX_INSTALLER_BYTES,
        "the release lists {} bytes, more than the {MAX_INSTALLER_BYTES} an installer may be",
        asset.size
    );
    let url = &asset.browser_download_url;
    let mut response = client.send(asset_request(url, DOWNLOAD_TIMEOUT)?).await?;
    ensure!(
        response.status().is_success(),
        "{url} answered {}",
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
            "more than the {} bytes the release lists arrived",
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
        "its SHA-256 does not match {SUMS_ASSET}"
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
        .with_context(|| format!("{SUMS_ASSET} lists no SHA-256 for {file_name}"))
}

/// Whether a release's asset may be downloaded from `url`: under [`DOWNLOAD_PATH`] on this build's
/// service, the one place updates come from.
fn is_service_download(url: &str) -> bool {
    url.strip_prefix(API_BASE)
        .and_then(|path| path.strip_prefix(DOWNLOAD_PATH))
        .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ed25519_dalek::{Signer as _, SigningKey};
    use futures::executor::block_on;
    use futures::future::BoxFuture;
    use http_client::http::HeaderValue;
    use http_client::{Response, Url};
    use parking_lot::Mutex;

    use super::*;

    const INSTALLER: &[u8] = b"MZ\x90\x00 stand-in for an NSIS installer";
    const INSTALLER_NAME: &str = "PoE2-Oracle-Setup-0.2.0.exe";
    const ETAG_0_2_0: &str = "\"release-v0.2.0\"";
    const NEXT_INSTALLER: &[u8] = b"MZ\x90\x00 stand-in for the next version's installer";
    const ETAG_0_2_1: &str = "\"release-v0.2.1\"";

    /// Serves canned bodies by exact URL (anything else: 404) and records each request's URL and
    /// `If-None-Match`. With an `etag`, the latest release carries it, and a request asking
    /// `If-None-Match` it gets the service's empty `304`.
    struct CannedClient {
        responses: HashMap<String, (u16, Vec<u8>)>,
        requests: Mutex<Vec<String>>,
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

        fn with_etag(self: Arc<Self>, etag: &'static str) -> Arc<Self> {
            let mut client = Arc::into_inner(self).expect("sole owner");
            client.etag = Some(etag);
            Arc::new(client)
        }

        fn fetched_an_installer(&self) -> bool {
            self.requests.lock().iter().any(|url| url.ends_with(".exe"))
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
            let condition = request
                .headers()
                .get(IF_NONE_MATCH)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            self.requests.lock().push(url.clone());
            self.conditions.lock().push(condition.clone());
            let etag = self.etag.filter(|_| url == latest_url());
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

    fn latest_url() -> String {
        oracle_protocol::url(LATEST_RELEASE_PATH)
    }

    fn download_url(tag: &str, name: &str) -> String {
        format!("{API_BASE}{DOWNLOAD_PATH}/{tag}/{name}")
    }

    /// The service's latest-release answer for `tag`, its assets `(name, size)` downloaded from
    /// the service, plus those downloads.
    fn service(
        tag: &str,
        assets: &[(&str, usize)],
        downloads: Vec<(&str, Vec<u8>)>,
    ) -> Arc<CannedClient> {
        let release = Release {
            tag_name: tag.to_owned(),
            html_url: format!("https://github.com/mttzzz/poe2-oracle/releases/tag/{tag}"),
            assets: assets
                .iter()
                .map(|&(name, size)| ReleaseAsset {
                    name: name.to_owned(),
                    browser_download_url: download_url(tag, name),
                    size: size as u64,
                })
                .collect(),
        };
        let mut responses = vec![(latest_url(), 200, serde_json::to_vec(&release).unwrap())];
        responses.extend(
            downloads
                .into_iter()
                .map(|(name, body)| (download_url(tag, name), 200, body)),
        );
        CannedClient::new(responses)
    }

    /// The key the test releases are signed with. The updater's own, [`RELEASE_PUBLIC_KEY`], is
    /// another: its private half isn't in this repository.
    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    /// SHA256SUMS listing `installer`'s digest for `version`'s installer, after another asset's.
    fn sums_listing(version: &str, installer: &[u8]) -> String {
        format!(
            "{:x}  poe2-oracle-{version}-symbols.zip\n{:x}  {}\n",
            Sha256::digest(b"other asset"),
            Sha256::digest(installer),
            installer_asset(version),
        )
    }

    /// SHA256SUMS.sig for `sums` by `key`, as `release-sign sign` writes it.
    fn signature_file(key: &SigningKey, sums: &str) -> Vec<u8> {
        format!(
            "{}\n",
            STANDARD.encode(key.sign(sums.as_bytes()).to_bytes())
        )
        .into_bytes()
    }

    /// A release of `version` serving `sums`, `signature` and `installer`.
    fn release(
        version: &str,
        sums: String,
        signature: Vec<u8>,
        installer: &[u8],
    ) -> Arc<CannedClient> {
        let name = installer_asset(version);
        service(
            &format!("v{version}"),
            &[
                (SUMS_ASSET, sums.len()),
                (SUMS_SIGNATURE_ASSET, signature.len()),
                (&name, installer.len()),
            ],
            vec![
                (SUMS_ASSET, sums.into_bytes()),
                (SUMS_SIGNATURE_ASSET, signature),
                (&name, installer.to_vec()),
            ],
        )
    }

    /// A v0.2.0 release serving `sums`, `signature` and [`INSTALLER`].
    fn release_0_2_0(sums: String, signature: Vec<u8>) -> Arc<CannedClient> {
        release("0.2.0", sums, signature, INSTALLER)
    }

    /// `version` as the release workflow makes it, with `installer`, signed with [`test_key`].
    fn signed(version: &str, installer: &[u8]) -> Arc<CannedClient> {
        let sums = sums_listing(version, installer);
        let signature = signature_file(&test_key(), &sums);
        release(version, sums, signature, installer)
    }

    /// v0.2.0 as the release workflow makes it, signed with [`test_key`].
    fn signed_release() -> Arc<CannedClient> {
        signed("0.2.0", INSTALLER)
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

    /// [`download_update`], trusting [`test_key`].
    fn download(client: &Arc<CannedClient>, update: &UpdateInfo, dir: &Path) -> Result<PathBuf> {
        let client: Arc<dyn HttpClient> = client.clone();
        block_on(download_signed(
            &client,
            update,
            dir,
            &test_key().verifying_key(),
        ))
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut files: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        files
    }

    #[test]
    fn an_unchanged_release_is_read_from_the_kept_answer() {
        let service = signed_release().with_etag(ETAG_0_2_0);
        let dir = tempfile::tempdir().unwrap();

        let first = check_in(&service, "0.1.0", dir.path()).unwrap();
        // The second check asks conditionally, gets the service's empty 304 and reads the same
        // release from what the first one kept.
        let second = check_in(&service, "0.1.0", dir.path()).unwrap();
        assert_eq!(
            *service.conditions.lock(),
            [None, Some(ETAG_0_2_0.to_owned())]
        );
        assert_eq!(second, first);
        assert_eq!(
            second.map(|update| update.version),
            Some(Version::new(0, 2, 0))
        );
    }

    #[test]
    fn asking_again_after_a_newer_release_installs_that_one() {
        // Offered at launch: v0.2.0, the service's answer kept.
        let dir = tempfile::tempdir().unwrap();
        let before = signed_release().with_etag(ETAG_0_2_0);
        let offered = check_in(&before, "0.1.0", dir.path()).unwrap().unwrap();
        assert_eq!(offered.version, Version::new(0, 2, 0));
        // Days later v0.2.1 is out, and the service hands out its files only. Installing asks
        // again first, with the kept tag, and the service answers the newer release in full.
        let after = signed("0.2.1", NEXT_INSTALLER).with_etag(ETAG_0_2_1);

        let latest = check_in(&after, "0.1.0", dir.path()).unwrap().unwrap();
        assert_eq!(*after.conditions.lock(), [Some(ETAG_0_2_0.to_owned())]);
        assert_eq!(latest.version, Version::new(0, 2, 1));

        let installer = download(&after, &latest, dir.path()).unwrap();
        assert_eq!(fs::read(installer).unwrap(), NEXT_INSTALLER);
    }

    #[test]
    fn an_installer_over_the_cap_is_never_offered() {
        // Only the size listed matters: the check downloads nothing.
        let listing = |size: u64| {
            let assets = [
                (SUMS_ASSET, 100),
                (SUMS_SIGNATURE_ASSET, 100),
                (INSTALLER_NAME, usize::try_from(size).unwrap()),
            ];
            service("v0.2.0", &assets, Vec::new())
        };

        let at_the_cap = check(&listing(MAX_INSTALLER_BYTES), "0.1.0").unwrap();
        assert_eq!(
            at_the_cap.map(|update| update.version),
            Some(Version::new(0, 2, 0))
        );
        assert!(check(&listing(MAX_INSTALLER_BYTES + 1), "0.1.0").is_err());
    }

    #[test]
    fn an_answer_kept_from_another_url_is_not_used() {
        // What a build asking another service kept in the same folder. Were its tag sent, the 304
        // would stand for this body.
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = KeptRelease {
            url: "https://elsewhere.example.org/api/v1/releases/latest".to_owned(),
            etag: ETAG_0_2_0.to_owned(),
            body: r#"{"tag_name":"v9.0.0","html_url":"","assets":[]}"#.to_owned(),
        };
        fs::write(
            dir.path().join(KEPT_RELEASE_FILE),
            serde_json::to_vec(&elsewhere).unwrap(),
        )
        .unwrap();
        let service = signed_release().with_etag(ETAG_0_2_0);

        let update = check_in(&service, "0.1.0", dir.path()).unwrap();

        assert_eq!(*service.conditions.lock(), [None]);
        assert_eq!(
            update.map(|update| update.version),
            Some(Version::new(0, 2, 0))
        );
    }

    #[test]
    fn newer_release_downloads_as_its_verified_installer() {
        let service = signed_release();
        let update = check(&service, "0.1.0")
            .unwrap()
            .expect("0.2.0 is newer than 0.1.0");
        assert_eq!(update.version, Version::new(0, 2, 0));

        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(installer_asset("0.1.0")),
            b"previous update",
        )
        .unwrap();
        fs::write(dir.path().join("settings.json"), b"{}").unwrap();
        let installer = download(&service, &update, dir.path()).unwrap();

        assert_eq!(installer, dir.path().join(INSTALLER_NAME));
        assert_eq!(fs::read(&installer).unwrap(), INSTALLER);
        assert_eq!(files_in(dir.path()), [INSTALLER_NAME, "settings.json"]);
    }

    #[test]
    fn tampered_installer_is_rejected_and_deleted() {
        // A signed SHA256SUMS, listing another installer than the one served.
        let sums = sums_listing("0.2.0", b"the installer the release was built with");
        let signature = signature_file(&test_key(), &sums);
        let service = release_0_2_0(sums, signature);
        let update = check(&service, "0.1.0").unwrap().unwrap();
        let dir = tempfile::tempdir().unwrap();

        let error = download(&service, &update, dir.path()).unwrap_err();

        assert!(format!("{error:#}").contains("SHA-256"), "{error:#}");
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn a_release_whose_signature_does_not_match_runs_nothing() {
        // SHA256SUMS swapped, after signing, for one listing the installer served.
        let signed = sums_listing("0.2.0", b"the installer the release was built with");
        let signature = signature_file(&test_key(), &signed);
        let service = release_0_2_0(sums_listing("0.2.0", INSTALLER), signature);
        let update = check(&service, "0.1.0").unwrap().unwrap();
        let dir = tempfile::tempdir().unwrap();

        let error = download(&service, &update, dir.path()).unwrap_err();

        assert!(
            format!("{error:#}").contains(SUMS_SIGNATURE_ASSET),
            "{error:#}"
        );
        assert!(!service.fetched_an_installer());
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn a_release_signed_with_another_key_runs_nothing() {
        // Signed as the release workflow signs -- with a key other than the release key.
        let service = signed_release();
        let update = check(&service, "0.1.0").unwrap().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let client: Arc<dyn HttpClient> = service.clone();

        let error = block_on(download_update(&client, &update, dir.path())).unwrap_err();

        assert!(
            format!("{error:#}").contains(SUMS_SIGNATURE_ASSET),
            "{error:#}"
        );
        assert!(!service.fetched_an_installer());
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn the_release_key_is_an_ed25519_public_key() {
        release_key().unwrap();
    }

    #[test]
    fn only_a_newer_version_is_an_update() {
        let service = signed_release();
        assert_eq!(check(&service, "0.2.0").unwrap(), None);
        assert_eq!(check(&service, "0.3.0").unwrap(), None);
        let update = check(&service, "0.2.0-rc.1").unwrap();
        assert_eq!(
            update.map(|update| update.version),
            Some(Version::new(0, 2, 0))
        );
    }

    #[test]
    fn a_release_missing_one_of_its_files_is_an_error() {
        let files = [INSTALLER_NAME, SUMS_ASSET, SUMS_SIGNATURE_ASSET];
        for missing in files {
            // The installer of another version must not be taken for this one's.
            let mut assets = vec![("PoE2-Oracle-Setup-0.1.9.exe", 100)];
            assets.extend(
                files
                    .iter()
                    .filter(|&&name| name != missing)
                    .map(|&name| (name, 100)),
            );
            let service = service("v0.2.0", &assets, Vec::new());
            let error = check(&service, "0.1.0").unwrap_err();
            assert!(
                error.to_string().ends_with(&format!("has no {missing}")),
                "{error:#}"
            );
        }
    }

    #[test]
    fn a_release_whose_installer_is_off_the_service_is_an_error() {
        // Refused at the check: the tray never offers what it couldn't install.
        let off_service = "https://github.com/mttzzz/poe2-oracle/releases/download/v0.2.0/\
            PoE2-Oracle-Setup-0.2.0.exe";
        let release = Release {
            tag_name: "v0.2.0".to_owned(),
            html_url: "https://github.com/mttzzz/poe2-oracle/releases/tag/v0.2.0".to_owned(),
            assets: [
                (SUMS_ASSET, download_url("v0.2.0", SUMS_ASSET)),
                (
                    SUMS_SIGNATURE_ASSET,
                    download_url("v0.2.0", SUMS_SIGNATURE_ASSET),
                ),
                (INSTALLER_NAME, off_service.to_owned()),
            ]
            .into_iter()
            .map(|(name, browser_download_url)| ReleaseAsset {
                name: name.to_owned(),
                browser_download_url,
                size: 100,
            })
            .collect(),
        };
        let service =
            CannedClient::new([(latest_url(), 200, serde_json::to_vec(&release).unwrap())]);

        let error = check(&service, "0.1.0").unwrap_err();

        assert!(error.to_string().contains(off_service), "{error:#}");
    }

    #[test]
    fn assets_come_from_the_service_only() {
        for url in [
            download_url("v0.2.0", INSTALLER_NAME),
            download_url("v0.2.0", SUMS_SIGNATURE_ASSET),
        ] {
            assert!(is_service_download(&url), "{url}");
        }
        let (scheme, host_and_path) = API_BASE.split_once("://").unwrap();
        // The service's host under another scheme: plain http for an https service.
        let other_scheme = if scheme == "https" { "http" } else { "https" };
        for url in [
            format!("{other_scheme}://{host_and_path}/download/v0.2.0/SHA256SUMS"),
            format!("{API_BASE}/downloads/v0.2.0/SHA256SUMS"),
            format!("{API_BASE}/download"),
            format!("{API_BASE}/SHA256SUMS"),
            format!("{API_BASE}.example.org/download/v0.2.0/SHA256SUMS"),
            format!("{API_BASE}@example.org/download/v0.2.0/SHA256SUMS"),
            format!("{API_BASE}:8443/download/v0.2.0/SHA256SUMS"),
            format!("https://example.org/{host_and_path}/download/v0.2.0/SHA256SUMS"),
            "https://github.com/mttzzz/poe2-oracle/releases/download/v0.2.0/SHA256SUMS".to_owned(),
            "file:///C:/Users/Public/PoE2-Oracle-Setup-0.2.0.exe".to_owned(),
            INSTALLER_NAME.to_owned(),
        ] {
            assert!(!is_service_download(&url), "{url}");
        }
    }

    #[test]
    fn no_known_release_is_an_error_not_up_to_date() {
        // What the service answers while it knows no release at all.
        let service = CannedClient::new([(
            latest_url(),
            503,
            br#"{"error":"unavailable","message":"no release is known yet"}"#.to_vec(),
        )]);
        assert!(check(&service, "0.1.0").is_err());
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
