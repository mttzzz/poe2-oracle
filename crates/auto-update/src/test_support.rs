//! For the app's own tests of its updater, which fetch a release through a real HTTP client from a
//! server of their own: the release's files, signed with a key of the tests' own, and the download
//! that trusts that key in place of the release key. Only the app's dev-dependency turns the
//! `test-support` feature on, so nothing the app ships has it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer as _, SigningKey};
use http_client::HttpClient;
use oracle_protocol::{
    DOWNLOAD_PATH, LATEST_RELEASE_PATH, Release, ReleaseAsset, SUMS_ASSET, SUMS_SIGNATURE_ASSET,
    installer_asset,
};
use sha2::{Digest as _, Sha256};

use crate::{UpdateInfo, Version, download_signed};

/// The key [`release_files`] signs with.
fn key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

/// What the service serves for release `version` carrying `installer`, laid out as the release
/// workflow makes it but signed with [`key`]: the latest release's answer, `SHA256SUMS`,
/// `SHA256SUMS.sig` and the installer, each as its path on the service and its body.
pub fn release_files(version: &Version, installer: &[u8]) -> Vec<(String, Vec<u8>)> {
    let tag = format!("v{version}");
    let name = installer_asset(&version.to_string());
    let sums = format!("{:x}  {name}\n", Sha256::digest(installer));
    let signature = format!(
        "{}\n",
        STANDARD.encode(key().sign(sums.as_bytes()).to_bytes())
    );
    let files = [
        (SUMS_ASSET.to_owned(), sums.into_bytes()),
        (SUMS_SIGNATURE_ASSET.to_owned(), signature.into_bytes()),
        (name, installer.to_vec()),
    ];
    let path = |name: &str| format!("{DOWNLOAD_PATH}/{tag}/{name}");
    let release = Release {
        tag_name: tag.clone(),
        html_url: format!("https://github.com/mttzzz/poe2-oracle/releases/tag/{tag}"),
        assets: files
            .iter()
            .map(|(name, body)| ReleaseAsset {
                name: name.clone(),
                browser_download_url: oracle_protocol::url(&path(name)),
                size: body.len() as u64,
            })
            .collect(),
    };
    let answer = serde_json::to_vec(&release).expect("a release is JSON");
    std::iter::once((LATEST_RELEASE_PATH.to_owned(), answer))
        .chain(files.into_iter().map(|(name, body)| (path(&name), body)))
        .collect()
}

/// [`download_update`](crate::download_update), trusting [`key`] -- the one [`release_files`]
/// signs with -- in place of the release key.
pub async fn download_update(
    client: &Arc<dyn HttpClient>,
    update: &UpdateInfo,
    dir: &Path,
) -> Result<PathBuf> {
    download_signed(client, &update.installer, dir, &key().verifying_key()).await
}
