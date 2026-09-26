#!/usr/bin/env python3
"""Publishes a test release to the lane's stand-in for GitHub (lanes/fake-github.py), laid out the
way .github/workflows/release.yml and data-release.yml lay out a real one: the files, their
SHA256SUMS and SHA256SUMS.sig, the release key's signature of it, checked against the public key
the app carries. It runs in the lane's runner, which builds release-sign and the data pack; the
key's seed comes on stdin (or from a file outside the checkout) and is never written anywhere:

  lane exec -- lanes/publish-release.py v0.1.1 .tmp/PoE2-Oracle-Setup-0.1.1.exe \\
      < ~/.local/share/poe2-oracle-release/release-signing-key.txt
  lane exec -- lanes/publish-release.py data-2026092601 < <seed file>   # builds the pack
  lane exec -- lanes/publish-release.py --draft v0.1.1 <installer> < <seed file>
  lane exec -- lanes/publish-release.py --publish v0.1.1                # the draft goes public

A v<version> tag needs the installer, PoE2-Oracle-Setup-<version>.exe, among its files; a
pre-release version (0.2.0-rc.1) is marked a pre-release, as release.yml marks it. A data-<N> tag
without files gets this checkout's pack, built by `cargo run -p oracle-data -- build`, which
refuses an <N> other than crates/oracle-data/data-version.txt's; with files, it needs
PoE2-Oracle-Data-<N>.zip among them. oracle-web offers neither a draft nor a pre-release.
Publishing a tag again replaces its release: new files, new asset ids, as on GitHub.

The releases live in .tmp/lane-dev/fake-github/releases/<tag>/. lanes/dev.sh starts the stand-in
and points oracle-web at it only when that folder exists as the dev server starts: after the
first release, `lane dev restart`. oracle-web then lists the stand-in every 10 s, and a release
published or changed shows on /api/v1/events within that. To put the lane back to its dry run,
delete .tmp/lane-dev/fake-github and `lane dev restart`.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RELEASES = ROOT / ".tmp/lane-dev/fake-github/releases"
PUBLIC_KEY = ROOT / "crates/auto-update/release-signing-key.pub"
SEED_FILE = "~/.local/share/poe2-oracle-release/release-signing-key.txt"
# The files the script writes itself.
SUMS, SIGNATURE, META = "SHA256SUMS", "SHA256SUMS.sig", "release.json"
# semver.org's grammar, after the tag's "v".
APP_TAG = re.compile(
    r"^v((0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(-((0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(\.(0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?"
    r"(\+[0-9a-zA-Z-]+(\.[0-9a-zA-Z-]+)*)?)$"
)
# A data version as oracle-web reads a tag: ten digits, written plainly.
DATA_TAG = re.compile(r"^data-([1-9]\d{9})$")
# What a published release does next, as lanes/dev.sh runs the stand-in.
LISTED = (
    f"oracle-web lists the stand-in every 10 s: http://{os.environ.get('LANE_HOST', '<laneHost>')}"
    "/api/v1/events names the latest release of each kind within that."
)


def fail(message):
    sys.exit(f"publish-release: {message}")


def now():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def run(*command, env=None, quiet=False):
    """Runs `command` from the checkout's root; errors and, unless `quiet`, output go to the
    terminal."""
    done = subprocess.run(
        command, cwd=ROOT, env=env, stdout=subprocess.DEVNULL if quiet else None
    )
    if done.returncode != 0:
        fail(f"{' '.join(command[:5])} ... exited with {done.returncode}")


def seed(key_file):
    """The release key's seed: from `key_file`, which must lie outside the checkout, or stdin."""
    if key_file:
        path = Path(key_file).expanduser().resolve()
        if path.is_relative_to(ROOT):
            fail(f"{key_file} is inside the checkout: keep the release key out of it")
        try:
            text = path.read_text()
        except OSError as error:
            fail(f"reading {key_file}: {error.strerror}")
    elif sys.stdin.isatty():
        fail(f"no key: pipe the release key's seed in, e.g. < {SEED_FILE}")
    else:
        text = sys.stdin.read()
    if not text.strip():
        fail("no key: the seed came empty")
    return text.strip()


def write_meta(folder, meta):
    """Writes `folder`'s release.json in one step: the stand-in never reads half of it."""
    temporary = folder / f".{META}.tmp"
    temporary.write_text(json.dumps(meta, indent=2) + "\n")
    temporary.replace(folder / META)


def publish_draft(tag):
    folder = RELEASES / tag
    try:
        meta = json.loads((folder / META).read_text())
    except FileNotFoundError:
        fail(f"no release {tag} in {RELEASES.relative_to(ROOT)}")
    if not meta.get("draft"):
        fail(f"{tag} is published already")
    meta["draft"] = False
    meta["published_at"] = now()
    write_meta(folder, meta)
    print(f"{tag}: published")
    print("A pre-release: oracle-web offers none." if meta.get("prerelease") else LISTED)


def payload_of(tag):
    """The file the release is for: the installer or the data pack, as oracle-protocol names it."""
    if app := APP_TAG.match(tag):
        return f"PoE2-Oracle-Setup-{app[1]}.exe"
    if data := DATA_TAG.match(tag):
        return f"PoE2-Oracle-Data-{data[1]}.zip"
    fail(f"{tag} is neither v<semver> nor data-<YYYYMMDDNN>")


def check_files(tag, files):
    """Refuses files the release can't have, before the key is asked for."""
    payload = payload_of(tag)
    if not files and not tag.startswith("data-"):
        fail(f"{tag} needs its files, {payload} among them")
    names = [file.name for file in files]
    for file in files:
        if not file.is_file():
            fail(f"{file}: no such file")
        if file.name in (SUMS, SIGNATURE, META) or file.name.startswith("."):
            fail(f"{file}: the script writes {file.name} itself, or the stand-in hides it")
        if names.count(file.name) > 1:
            fail(f"{file.name} is given twice")
    if files and payload not in names:
        fail(f"{tag} needs {payload} among its files")


def lay_out(tag, files, staging):
    """The release's files and SHA256SUMS, into `staging`: the files given, or else the data pack
    of this checkout."""
    if not files:
        # Quiet: the tool lists the files it wrote, under the staging folder's passing name.
        run(
            "cargo", "run", "--quiet", "--locked", "-p", "oracle-data", "--",
            "build", str(staging), "--version", tag.removeprefix("data-"),
            quiet=True,
        )
        return
    sums = []
    for file in files:
        shutil.copyfile(file, staging / file.name)
        # sha256sum's text format with an LF line ending: what crates/auto-update parses.
        digest = hashlib.sha256((staging / file.name).read_bytes()).hexdigest()
        sums.append(f"{digest}  {file.name}\n")
    (staging / SUMS).write_text("".join(sums))


def sign(staging, key):
    """Signs SHA256SUMS with the key and checks it against the one the app carries. release-sign
    is built first, without the key: only the signing itself gets it, in its environment."""
    run("cargo", "build", "--quiet", "--locked", "-p", "release-sign")
    metadata = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    )
    release_sign = Path(json.loads(metadata.stdout)["target_directory"]) / "debug/release-sign"
    sums, signature = staging / SUMS, staging / SIGNATURE
    signed = subprocess.run(
        [str(release_sign), "sign", str(sums), str(signature)],
        cwd=ROOT, capture_output=True, text=True,
        env={**os.environ, "RELEASE_SIGNING_KEY": key},
    )
    if signed.returncode != 0:
        fail(f"release-sign: {signed.stderr.strip()}")
    checked = subprocess.run(
        [str(release_sign), "verify", str(sums), str(signature), PUBLIC_KEY.read_text().strip()],
        cwd=ROOT, capture_output=True, text=True,
    )
    if checked.returncode != 0:
        fail(
            f"the key isn't the release key: its public half isn't "
            f"{PUBLIC_KEY.relative_to(ROOT)}, so the app would refuse this release"
        )


def main():
    parser = argparse.ArgumentParser(
        description="Publishes a test release to the lane's stand-in for GitHub.",
        epilog=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("tag", help="v<semver> or data-<YYYYMMDDNN>")
    parser.add_argument("files", nargs="*", type=Path, help="the release's files")
    parser.add_argument("--draft", action="store_true", help="a draft: listed, never offered")
    parser.add_argument("--publish", action="store_true", help="publish the draft <tag>")
    parser.add_argument("--key-file", help="the seed from this file (outside the checkout)")
    args = parser.parse_args()
    if args.publish:
        if args.files or args.draft or args.key_file:
            fail("--publish takes the tag alone")
        return publish_draft(args.tag)

    check_files(args.tag, args.files)
    key = seed(args.key_file)
    first = not RELEASES.is_dir()
    RELEASES.mkdir(parents=True, exist_ok=True)
    staging = RELEASES / f".staging-{args.tag}-{os.getpid()}"
    staging.mkdir()
    try:
        lay_out(args.tag, [file.resolve() for file in args.files], staging)
        sign(staging, key)
        created = now()
        prerelease = args.tag.startswith("v") and "-" in args.tag
        write_meta(
            staging,
            {
                "name": args.tag,
                "draft": args.draft,
                "prerelease": prerelease,
                "created_at": created,
                **({} if args.draft else {"published_at": created}),
            },
        )
        target = RELEASES / args.tag
        replaced = target.exists()
        if replaced:
            old = RELEASES / f".old-{args.tag}-{os.getpid()}"
            target.rename(old)
            staging.rename(target)
            shutil.rmtree(old)
        else:
            staging.rename(target)
    finally:
        shutil.rmtree(staging, ignore_errors=True)

    state = "a draft" if args.draft else "a pre-release" if prerelease else "published"
    print(f"{args.tag}: {state}{', replacing the one before' if replaced else ''}, in "
          f"{target.relative_to(ROOT)}")
    for file in sorted(target.iterdir()):
        if not file.name.startswith(".") and file.name != META:
            print(f"  {file.name}  {file.stat().st_size} bytes")
    if first:
        print("The stand-in's first release: `lane dev restart` starts the stand-in.")
    if args.draft:
        print(f"oracle-web offers nothing of a draft: `{sys.argv[0]} --publish {args.tag}`.")
    elif prerelease:
        print("oracle-web offers no pre-release.")
    else:
        print(LISTED)


if __name__ == "__main__":
    main()
