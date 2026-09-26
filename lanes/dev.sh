#!/usr/bin/env bash
# The lane's dev server: oracle-web, the service behind oracle.pushka.biz, run from the checkout.
#
# `lane dev up` runs this in the dev pod (lanes/lane.toml, [dev]); http://<laneHost> then opens from
# any machine on the LAN, laid out as in production:
#
#     /                 site/: English, Russian under /ru/, the report form at /report.html
#     /guide/en/, /guide/ru/
#                       the guide, a book per language, built by docs/guide/build.sh
#     /guide/images/    docs/guide/src/images, the books' pictures (/images/ too, for site/)
#     /api/v1/...       reports and the latest release;  /download/...  the release proxy
#
# / and /guide/ pick the reader's language: the `lang` cookie that the language links on the site
# and in the guide set, or else the browser's languages. / answers in English or redirects to /ru/,
# /guide/ redirects to /guide/en/ or /guide/ru/.
#
# site/ and the images are read from the checkout, so an edit shows on the next reload. The guide
# and the server are built once, at start: after editing docs/guide or the server's crates, run
# `lane dev restart`. The lane holds no GitHub or Telegram token, so the server runs dry: it logs a
# report and answers it, and nothing leaves the pod.
set -euo pipefail

work=.tmp/lane-dev

# mdBook as the Dockerfile's guide stage installs it: the same release, the same sha256 check.
pin() { sed -n "s/^ARG $1=//p" Dockerfile; }
version=$(pin MDBOOK_VERSION)
sha256=$(pin MDBOOK_SHA256)
if [[ -z $version || -z $sha256 ]]; then
  echo "Dockerfile: no ARG MDBOOK_VERSION or ARG MDBOOK_SHA256" >&2
  exit 1
fi
mdbook=$work/mdbook-$version/mdbook
if [[ ! -x $mdbook ]]; then
  archive=mdbook-$version-x86_64-unknown-linux-gnu.tar.gz
  mkdir -p "$work"
  echo "fetching mdBook $version"
  curl -sSfL -o "$work/$archive" "https://github.com/rust-lang/mdBook/releases/download/$version/$archive"
  echo "$sha256  $work/$archive" | sha256sum -c -
  rm -rf "$work/mdbook-$version" "$work/mdbook-$version.partial"
  mkdir "$work/mdbook-$version.partial"
  tar -xzf "$work/$archive" -C "$work/mdbook-$version.partial"
  rm "$work/$archive"
  mv "$work/mdbook-$version.partial" "$work/mdbook-$version"
fi
rm -rf "$work/guide"
MDBOOK=$mdbook docs/guide/build.sh "$PWD/$work/guide"

# Dry, whatever the lane's Secret may hold. No REDIS_URL either: counters and rate limits live in
# memory, and `lane dev restart` clears them. Behind the lane's proxies every visitor has the same
# address, so a day of testing would otherwise use up one client's report limits.
unset GITHUB_TOKEN TELEGRAM_TOKEN TELEGRAM_CHAT_ID REDIS_URL
# The lane's dev Service, Ingress and readiness probe all point at port 3000.
export PORT=3000
export PUBLIC_URL=http://$LANE_HOST
export SITE_DIR=site IMAGES_DIR=docs/guide/src/images GUIDE_DIR=$work/guide
# Cargo's home and target dir under .tmp/, where they outlive the pod: resolving the workspace
# clones the app's GPUI git dependency, which then happens on the first start only, and the build
# never waits for the runner's lock on target/. The first start takes minutes; the dev pod has only
# a readiness probe, which doesn't restart it.
export CARGO_HOME=$PWD/$work/cargo-home CARGO_TARGET_DIR=$PWD/$work/target
exec cargo run --release -p oracle-web
