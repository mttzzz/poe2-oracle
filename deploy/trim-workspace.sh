#!/usr/bin/env bash
# Turns a copy of this repository's workspace into the server's own: the members become
# oracle-protocol and oracle-web only, and Cargo.lock is pruned to the packages they use.
#
# Why: Cargo resolves the whole workspace before it builds any member, and the app's crates depend
# on GPUI by git, so even `cargo build -p oracle-web` first clones the zed repository, hundreds of
# megabytes the server never compiles. The Dockerfile runs this in a directory holding the root
# Cargo.toml, Cargo.lock and the two crates (their manifests and at least stub sources).
#
# The pruned lockfile may only lose packages: whatever it keeps must be in Cargo.lock at the same
# version, source and checksum, or the script fails. A package it would add or change means
# Cargo.lock is out of date for the server's crates -- what `cargo build --locked` refuses in the
# full workspace -- and the fix is a `cargo build -p oracle-web` in the lane, then a commit.
set -euo pipefail

members='members = ["crates/oracle-protocol", "crates/oracle-web"]'
awk -v members="$members" '
  /^members[[:space:]]*=/ { print members; skipping = ($0 !~ /\]/); next }
  skipping { if ($0 ~ /\]/) skipping = 0; next }
  { print }
' Cargo.toml > Cargo.toml.trimmed
if ! grep -qxF "$members" Cargo.toml.trimmed; then
  echo "trim-workspace: Cargo.toml has no members = [...] line to replace" >&2
  exit 1
fi
mv Cargo.toml.trimmed Cargo.toml

cp Cargo.lock Cargo.lock.full
# --workspace re-resolves the members only; every other package keeps its locked version.
cargo update --workspace --quiet

# One line per locked package: name, version, source, checksum.
packages() {
  awk '
    /^\[\[package\]\]/ { if (name != "") print name, version, source, checksum; name = version = source = checksum = "" }
    /^name = /     { name = $3 }
    /^version = /  { version = $3 }
    /^source = /   { source = $3 }
    /^checksum = / { checksum = $3 }
    END { if (name != "") print name, version, source, checksum }
  ' "$1" | sort
}
changed=$(comm -13 <(packages Cargo.lock.full) <(packages Cargo.lock))
rm Cargo.lock.full
if [[ -n $changed ]]; then
  echo "trim-workspace: Cargo.lock is out of date for oracle-protocol and oracle-web; resolving them" >&2
  echo "alone would lock:" >&2
  echo "$changed" >&2
  exit 1
fi
