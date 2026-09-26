//! Signs a release's `SHA256SUMS` with the release key. An installed PoE2 Oracle runs an update
//! only when the release's `SHA256SUMS.sig` is this key's Ed25519 signature of `SHA256SUMS`'s
//! exact bytes, checked against the public half crates/auto-update carries
//! (`crates/auto-update/release-signing-key.pub`).
//!
//! ```text
//! release-sign keygen <secret-file>
//! release-sign sign <SHA256SUMS> [<out.sig>] [--key-file <secret-file>]
//! release-sign verify <SHA256SUMS> <sig> <public-key>
//! ```
//!
//! `keygen` makes a key pair: the secret seed goes into a new file, never over an existing one,
//! and only the public key is printed. `sign` takes the seed from `--key-file`, or else from the
//! `RELEASE_SIGNING_KEY` environment variable -- the GitHub secret `.github/workflows/release.yml`
//! signs with -- and writes `<SHA256SUMS>.sig` unless given another path. `verify` checks a
//! signature the way the updater does. Seeds, public keys and signatures are one line of standard
//! base64 each; nothing derived from a seed but its public key is ever printed.

use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};

/// Where `sign` takes the seed from without `--key-file`.
const KEY_VARIABLE: &str = "RELEASE_SIGNING_KEY";

const USAGE: &str = "usage:
  release-sign keygen <secret-file>
  release-sign sign <SHA256SUMS> [<out.sig>] [--key-file <secret-file>]
  release-sign verify <SHA256SUMS> <sig> <public-key>";

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let command = args.next().unwrap_or_default();
    let args: Vec<OsString> = args.collect();
    match (command.to_str(), args.as_slice()) {
        (Some("keygen"), [secret_file]) => println!("{}", keygen(Path::new(secret_file))?),
        (Some("sign"), args) => {
            let (files, key_file) = sign_args(args)?;
            let (sums, out) = match files.as_slice() {
                [sums] => (sums, sums.with_added_extension("sig")),
                [sums, out] => (sums, out.clone()),
                _ => bail!(USAGE),
            };
            let key = signing_key(key_file.as_deref())?;
            let message = fs::read(sums).with_context(|| format!("reading {}", sums.display()))?;
            fs::write(&out, format!("{}\n", sign(&key, &message)))
                .with_context(|| format!("writing {}", out.display()))?;
            println!("{}: signed by {}", out.display(), public_key(&key));
        }
        (Some("verify"), [sums, signature, public_key]) => {
            let sums = Path::new(sums);
            let message = fs::read(sums).with_context(|| format!("reading {}", sums.display()))?;
            let signature_file = Path::new(signature);
            let signature = fs::read_to_string(signature_file)
                .with_context(|| format!("reading {}", signature_file.display()))?;
            let public_key = public_key.to_str().context("the public key isn't text")?;
            verify(&message, &signature, public_key)
                .with_context(|| format!("checking {}", signature_file.display()))?;
            println!("{}: the signature matches", sums.display());
        }
        _ => bail!(USAGE),
    }
    Ok(())
}

/// `sign`'s file arguments, and `--key-file`'s path wherever it stands among them.
fn sign_args(args: &[OsString]) -> Result<(Vec<PathBuf>, Option<PathBuf>)> {
    let mut files = Vec::new();
    let mut key_file = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--key-file" {
            key_file = Some(PathBuf::from(args.next().context(USAGE)?));
        } else {
            files.push(PathBuf::from(arg));
        }
    }
    Ok((files, key_file))
}

/// Makes a key pair: the seed goes into `secret_file`, which must not exist yet and which, on
/// Unix, only its owner may read. Returns the public key. An existing file is never replaced: the
/// key in it may be the only one installed copies accept.
fn keygen(secret_file: &Path) -> Result<String> {
    let mut seed = [0; 32];
    getrandom::fill(&mut seed).map_err(|error| anyhow!("drawing a random seed: {error}"))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = match options.open(secret_file) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => bail!(
            "{} already exists, and keygen never replaces a key",
            secret_file.display()
        ),
        Err(error) => {
            return Err(error).with_context(|| format!("creating {}", secret_file.display()));
        }
    };
    if let Err(error) = writeln!(file, "{}", STANDARD.encode(seed)).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(secret_file);
        return Err(error).with_context(|| format!("writing {}", secret_file.display()));
    }
    Ok(public_key(&SigningKey::from_bytes(&seed)))
}

/// The key to sign with: the seed in `key_file`, or else in [`KEY_VARIABLE`].
fn signing_key(key_file: Option<&Path>) -> Result<SigningKey> {
    let (seed, source) = match key_file {
        Some(path) => (
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
            path.display().to_string(),
        ),
        None => match env::var(KEY_VARIABLE) {
            Ok(seed) if !seed.trim().is_empty() => (seed, KEY_VARIABLE.to_owned()),
            _ => bail!("no signing key: set {KEY_VARIABLE} or pass --key-file"),
        },
    };
    let seed = decode(&seed)
        .with_context(|| format!("the key in {source} isn't 32 bytes of standard base64"))?;
    Ok(SigningKey::from_bytes(&seed))
}

/// `N` bytes from one line of standard base64, whitespace around it ignored. A failure says
/// nothing about the text: it may be a seed.
fn decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    STANDARD.decode(text.trim()).ok()?.try_into().ok()
}

fn sign(key: &SigningKey, message: &[u8]) -> String {
    STANDARD.encode(key.sign(message).to_bytes())
}

fn public_key(key: &SigningKey) -> String {
    STANDARD.encode(key.verifying_key().as_bytes())
}

/// Whether `signature` is `public_key`'s signature of `message`, both in standard base64: the
/// strict check crates/auto-update makes before it trusts a release's `SHA256SUMS`.
fn verify(message: &[u8], signature: &str, public_key: &str) -> Result<()> {
    let key_bytes =
        decode(public_key).context("the public key isn't 32 bytes of standard base64")?;
    let key =
        VerifyingKey::from_bytes(&key_bytes).context("the public key isn't an Ed25519 key")?;
    let signature = decode(signature).context("the signature isn't 64 bytes of standard base64")?;
    key.verify_strict(message, &Signature::from_bytes(&signature))
        .map_err(|_| {
            anyhow!("the signature doesn't match: another key made it, or the file changed since")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMS: &[u8] = b"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08  \
        PoE2-Oracle-Setup-0.1.0.exe\n";

    #[test]
    fn a_signature_matches_until_one_byte_changes() {
        let dir = tempfile::tempdir().unwrap();
        let secret_file = dir.path().join("release-signing-key.txt");
        let public_key = keygen(&secret_file).unwrap();
        let signature = sign(&signing_key(Some(&secret_file)).unwrap(), SUMS);
        verify(SUMS, &signature, &public_key).unwrap();
        // As `sign` writes the file: one line.
        verify(SUMS, &format!("{signature}\n"), &public_key).unwrap();

        let mut changed = SUMS.to_vec();
        changed[0] ^= 1;
        assert!(verify(&changed, &signature, &public_key).is_err());
    }

    #[test]
    fn keygen_never_replaces_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let secret_file = dir.path().join("release-signing-key.txt");
        fs::write(&secret_file, "the only copy").unwrap();

        assert!(keygen(&secret_file).is_err());
        assert_eq!(fs::read_to_string(&secret_file).unwrap(), "the only copy");
    }

    #[cfg(unix)]
    #[test]
    fn only_its_owner_may_read_a_new_seed() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let secret_file = dir.path().join("release-signing-key.txt");
        keygen(&secret_file).unwrap();
        let mode = fs::metadata(&secret_file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
