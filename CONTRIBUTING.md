# Contributing to PoE2 Oracle

**English** · [Русский](CONTRIBUTING.ru.md)

Thank you for helping. The most useful contributions are precise bug reports and the texts of items
the app reads wrong; code is welcome too. Everyone taking part follows the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting a problem

Players report from the app or from the site; this repository's issue forms are for the
maintainer's own notes, since outsiders can't reach them while the repository is private.

1. **One problem per report.** Two unrelated bugs in one report tend to get only one of them fixed.
2. **Report from the app**, which adds what is needed: **Write to the developer** in the **Help**
   section of the settings opens the report window. Along with your text it sends the app version,
   the interface and client languages, the Windows version, the league and the interface scale. You
   can write in English or Russian; leave a contact (Telegram, Discord or email) if you'd like an
   answer.
3. **Always say** what you did, what you expected and what happened instead.
4. **For an item problem, report it from the item.** The link **report a problem** under the item
   name on the price panel, or the button **Report a problem** in the message about an item the app
   couldn't read, opens the window with the item's text attached (not recognised, wrong filters,
   wrong price). By hand: in the game, hover the item and press `Ctrl+Alt+C` (the game's advanced
   copy, the same one the app uses: it includes the mod tiers), then paste it into your report.
   Texts the app couldn't read are also kept in `%LOCALAPPDATA%\poe2-oracle\data\unparsed`.
5. **Leave Attach diagnostics on** when the problem isn't about one item (it is on by default for a
   problem, an item or a crash). The report then carries the diagnostics report: the logs of the
   current and the previous run, your settings, the item texts the app couldn't read and a summary
   of the system (Windows version, monitors, the game window's size, the game's display mode,
   language and copy key, the cached files). Your Windows user name, if it has three characters or
   more, is replaced with `%USERNAME%` in every file, and the paths of your user, Desktop, Documents
   and AppData folders with `%USERPROFILE%`, `%DESKTOP%`, `%DOCUMENTS%`, `%APPDATA%` and
   `%LOCALAPPDATA%`. To check it first, **What's inside** in the report window saves the same zip
   to your desktop and shows it in Explorer; so does **Collect report** in the **Help** section of
   the settings, as `PoE2-Oracle-report-<date>_<time>.zip`.
6. **After a crash**, the next launch opens the report window by itself, with what the app reported
   attached; say what you were doing when it closed.
7. **Without the app**, the form at https://oracle.pushka.biz/report.html (Russian:
   https://oracle.pushka.biz/ru/report.html) takes a problem or an idea, text only: mention the app
   version (Windows Settings → Apps → Installed apps shows it next to PoE2 Oracle) and your game
   client language.

Reports reach only the maintainer: oracle.pushka.biz files each one as an issue in this repository
and sends a Telegram message, the diagnostics zip to Telegram only.

**Never post** passwords, your pathofexile.com session cookie (`POESESSID`) or any other token in
reports, logs or screenshots: whoever has them can act as you on the site.

For security vulnerabilities, see [SECURITY.md](SECURITY.md).

## The code

```text
crates/
  poe2-oracle/     the app: price panel, settings, XP overlay, report window, tray icon and taskbar
                   button, updates, Win32 overlay windows, hotkeys and the game's input
  item-parser/     clipboard item text -> ParsedItem (English and Russian clients)
  stat-filters/    ParsedItem -> the trade search's filter rows (built as in Exiled Exchange 2;
                   which ones a search starts with, and their bounds, as in PoE Overlay II)
  trade-client/    trade API (leagues, catalogs, search, fetch, rate limits), the Currency Exchange
                   market from GGG's hourly exchange data, poe2scout prices
  poe2-domain/     shared item and stat types, no I/O
  auto-update/     the live link to oracle.pushka.biz's event stream (reconnecting with backoff),
                   release and data pack checks, Ed25519-verified SHA256SUMS and the
                   SHA-256-checked download of the installer or the data pack
  oracle-protocol/ the app <-> service contract: where the service is, what a report carries, the
                   release answers and the event stream the updater reads
  oracle-data/     the game data pack: the built-in tables' data version (data-version.txt), the
                   pack's zip format, and the tool that builds a pack in CI
  oracle-web/      the oracle.pushka.biz service: site, guide, reports, update proxy and event
                   stream, daily digest
  release-sign/    signs a release's SHA256SUMS in CI
packaging/         release script, NSIS installer, data table generators (packaging/data)
docs/guide/        the user guide: English and Russian mdBooks, served by oracle-web under /guide/
site/              the landing pages and the report form, served by oracle-web
deploy/            the service's Helm values and the Docker build's workspace trim
lanes/             the Linux build container used by CI
```

The app is Windows-only. Crates without Windows code build and test on Linux too; the app crate's
Windows parts (`platform/`, `ui/`, the price check) compile only for a Windows target.

## Building on Windows

A binary that runs is built natively on Windows. You need:

- **Rust** (current stable) from [rustup](https://rustup.rs), with the MSVC toolchain
  (`stable-x86_64-pc-windows-msvc`, the default on Windows).
- **Visual Studio Build Tools** (2022 or later) with the **Desktop development with C++** workload:
  the MSVC compiler and linker, and the **Windows SDK**, which it installs by default. The SDK
  provides `fxc.exe`, with which GPUI compiles its shaders in release builds, and `rc.exe`, with
  which `crates/poe2-oracle/build.rs` embeds the icon and version into the exe.
- For the installer only: **NSIS 3** and **cargo-about**. `packaging/build-release.ps1` finds
  `makensis` on `PATH` or in Program Files and otherwise downloads the pinned NSIS 3.12 zip (checked
  against its SHA-256); it installs the pinned cargo-about itself.

```powershell
# Run from source (a debug build)
cargo run -p poe2-oracle

# Release exe: target\release\poe2-oracle.exe
cargo build -p poe2-oracle --release

# Installer, SHA256SUMS and third-party notices in target\dist
powershell -NoProfile -ExecutionPolicy Bypass -File packaging\build-release.ps1
```

Things that save time:

- Only one copy of the app runs per Windows session. If an installed copy is running, `cargo run`
  just opens that copy's settings and exits: quit the installed one first (**Quit** in its tray
  icon's menu, or **Quit the app** in its settings' **Help**).
- A debug build compiles GPUI's shaders at run time from the source checkout, so a debug exe runs
  only on the machine that built it. Give testers a release build.
- The log is `%LOCALAPPDATA%\poe2-oracle\data\logs\poe2-oracle.log`. `RUST_LOG` replaces its
  default filters (for example `RUST_LOG=debug`); with a console attached, the lines also go to
  stderr.
- `POE2_ORACLE_KEEP_ITEM_TEXTS=1` keeps the text of every checked item in `data\unparsed`, not only
  the troubled ones: handy for collecting parser fixtures. The folder keeps the newest 100 texts,
  so copy them out before checking more.
- `POE2_ORACLE_CLIENT_LOG=<file>` makes the app read that file instead of the game's `Client.txt`,
  so the XP overlay can be tested by appending lines to it.
- `POE2_ORACLE_PAINT_CENSUS=1` logs, every 10 seconds, how often GPUI's vsync thread waited for a
  refresh of the display or slept instead, and for how much of the time it slept; then what woke
  the UI thread, window by window: the display refreshes' asks for a paint and what the redraw
  filter did with them, the paints a gated window let through or swallowed, what opened its paints,
  and the messages taken off the thread's queue or sent from other threads, per second. For
  measuring idle cost; off, it only reads a flag.
- `POE2_ORACLE_API_BASE` is read at build time: the app then sends its reports and update checks to
  that address instead of `https://oracle.pushka.biz`, for testing against your own `oracle-web`
  (see [The web service](#the-web-service)). It must be https, without a trailing slash; a plain
  `http://` address also needs `--features oracle-protocol/dev-endpoints`.
  `packaging/build-release.ps1` refuses both, so a release always talks to oracle.pushka.biz.

## Checks (what CI runs)

CI (`.github/workflows/ci.yml`) runs on Linux, in the container built from
`lanes/runner.Dockerfile` (Rust with the `x86_64-pc-windows-gnu` cross toolchain, rustfmt and
clippy). The windows-gnu cross build is for type-checking only; it is not a binary to ship, and it
leaves out `oracle-web`, a Linux server. To run exactly what CI runs (Linux, macOS or WSL with
Docker):

```sh
docker build -f lanes/runner.Dockerfile -t poe2-oracle-runner .
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/workspace -w /workspace \
  -e CARGO_HOME=/workspace/.ci-cargo-home -e HOME=/workspace/.ci-cargo-home \
  poe2-oracle-runner bash -c '
    set -euo pipefail
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --exclude poe2-oracle -- -D warnings
    cargo clippy -p poe2-oracle --all-targets --target x86_64-pc-windows-gnu -- -D warnings
    cargo test --workspace --exclude poe2-oracle
    cargo test -p poe2-oracle --lib
    cargo build --workspace --exclude oracle-web --target x86_64-pc-windows-gnu
  '
```

`.ci-cargo-home` and `target` then hold the container's cargo cache and build output; delete them
whenever you like. The maintainer runs the same container as a local "lane" (`lane exec -- cargo
...`); you don't need that setup.

## Code style

- `cargo fmt --all` with the default rustfmt settings, and clippy clean with `-D warnings` as CI
  runs it.
- Match the code around your change. Comments say why, not what, and name the source of a fact
  (an Exiled Exchange 2 file, a live API answer and its date) when the code depends on it.
- The interface speaks English and Russian. Write new interface text in English inside `tr!("…")`
  (`tr_n!` for a count) and put its Russian in the `crates/poe2-oracle/assets/i18n/ru/*.json` file
  for that part of the app, in the wording the app already uses (a new file also goes into
  `RUSSIAN_FILES` in `i18n.rs`). `cargo test -p poe2-oracle --lib` fails on a text without its
  Russian, a Russian entry no code uses, a text in two files, or a lost `{placeholder}` or plural
  form.
- Tests go where a plausible bug would fail them. For the parser, that is a real item text copied
  from the game (`Ctrl+Alt+C`) saved under `crates/item-parser/tests/fixtures/`, plus what it must
  parse to.

## Regenerating the data tables

Some tables come from [Exiled Exchange 2](https://github.com/Kvan7/Exiled-Exchange-2) (MIT), at the
commit named in `crates/poe2-oracle/assets/data/NOTICE`, `crates/item-parser/data/NOTICE` and
`crates/stat-filters/data/NOTICE`. After a game patch or at a new league, from the repository root:

```sh
# The ways the client prints stats: crates/item-parser/data/stat-matchers-{en,ru}.tsv
python3 packaging/data/generate_stat_matchers.py <EE2>/renderer/public/data

# The item reference table (names, art, bases): crates/poe2-oracle/assets/data/item-refs.tsv
python3 packaging/data/generate_item_refs.py <EE2>/renderer/public/data <RePoE folder>
```

`crates/stat-filters/src/better.rs` (stats where a lower roll is better) is kept by hand from the
same stat database; its module comment says how. Don't edit the generated `.tsv` files by hand. When
you move to another EE2 commit, update the commit in those three `NOTICE` files, in `better.rs`'s
module comment and in `about.hbs`, which copies them into the third-party notices.

To check the result against real items, sweep item texts from the game (a vendor, the stash, your
inventory) into one file, separated by lines starting with `####`, copy the app's cached catalogs
(`stat-catalog[-ru].json`, `static-items[-ru].json` and `item-types[-ru].json` from
`%LOCALAPPDATA%\poe2-oracle\cache`) into a folder, and run:

```sh
cargo run -p item-parser --example sweep -- <catalog folder> <texts file>
```

It lists every item or line it can't handle and exits non-zero if there is one.

Three tables come from the game's own data as [RePoE](https://repoe-fork.github.io/poe2/) exports it
(MIT; the data belongs to Grinding Gear Games), as named in `crates/stat-filters/data/NOTICE`,
`crates/trade-client/data/NOTICE` and `crates/poe2-oracle/assets/data/NOTICE`: the mod tiers, the
exchange item ids and the item reference table's bases, for Craft of Exile's link (its command is
above). After a game patch, download RePoE's `mods.json` and `base_items.json` into a folder, save
the English trade site's list of exchange items
(`https://www.pathofexile.com/api/trade2/data/static`) as `static.json`, and run:

```sh
# Mod tiers and the item level each needs: crates/stat-filters/data/mod-tiers.tsv
python3 packaging/data/generate_mod_tiers.py <RePoE folder> <EE2>/renderer/public/data

# The trade id of each item GGG's exchange data names: crates/trade-client/data/cx-items.tsv
python3 packaging/data/generate_cx_ids.py <RePoE folder>/base_items.json static.json
```

Given a saved hour of the exchange data
(`https://web.poecdn.com/api/currency-exchange/poe2/<unix hour>`) as a third argument,
`generate_cx_ids.py` also lists the items that hour trades without a trade id.

## Game data packs

The five tables above -- `stat-matchers-en.tsv`, `stat-matchers-ru.tsv`, `mod-tiers.tsv`,
`cx-items.tsv` and `item-refs.tsv` -- are built into the app and are also published on their own,
as a signed game data pack, so that installed apps get newer ones without a new installer. An app
takes a pack at its next start when the pack is newer than the app's built-in tables and names no
newer app than it. A pack it can't use is deleted; a damaged one (a table changed on the disk, or
one that doesn't parse) is set aside in `%LOCALAPPDATA%\poe2-oracle\data\game-data\rejected`, and
the built-in tables stay in use.

`crates/oracle-data/data-version.txt` records the built-in tables:

- `version`: their data version, `YYYYMMDDNN`, the day they were made and that day's number from
  `01` (`2026100101`, then `2026100102` the same day). A pack replaces an app's tables only when
  its version is higher.
- `min_app`: the oldest app version whose code reads them. When a table changes in a way older
  apps can't read (a new column, a tag or value their parser doesn't know), raise it to the
  version about to be released: older apps then refuse the pack (they download it once per run,
  say why in their settings and keep their built-in tables).
- `tables`: the SHA-256 of the tables' `sha256sum` listing.

**Whenever a table changes, give it a new version.** `cargo test -p oracle-data` fails while the
tables aren't the ones recorded, and prints the digest to write; set `version` above the old one in
the same commit. A data version names one set of tables: apps compare versions only, and a tag
can't be pushed twice.

To publish a pack, for the maintainer:

1. Commit the regenerated tables with the new `data-version.txt`, push, and wait for CI to pass on
   that commit: its tests parse every table.
2. Push a tag `data-<version>` with the version `data-version.txt` names
   (`git tag data-2026100101 && git push origin data-2026100101`).
   `.github/workflows/data-release.yml` builds `PoE2-Oracle-Data-<version>.zip` and `SHA256SUMS`
   with `cargo run -p oracle-data -- build` (a job without secrets), signs `SHA256SUMS` with
   `crates/release-sign` and the `RELEASE_SIGNING_KEY` secret as `release.yml` does, and creates a
   **draft** GitHub release with the three files. The build refuses a tag whose version isn't
   `data-version.txt`'s, and tables that changed since that version was recorded.
3. Check the draft, then publish it, never as the latest release:
   `gh release edit data-<version> --draft=false --latest=false`, or leave **Set as the latest
   release** unticked on GitHub. oracle.pushka.biz serves published packs only; from then on,
   running apps with automatic updates on hear of it within about two minutes, download it (after
   an app update, if one is out too) and restart to load it once none of their windows is open.

`cargo run -p oracle-data -- build <folder>` makes the same pack by hand: the same tables give the
same bytes (entries in name order, fixed dates, one deflate backend), so a published pack can be
rebuilt and compared. An app release carries its commit's tables, so after an update the app
deletes an installed pack that isn't newer than its own tables.

## Pictures of the app

The landing pages draw the app's interface from data instead of showing screenshots. `site/ui/`
holds a component per part of the app: `panel`, `market`, `xp`, `settings`, and `hero`, which puts
three of them together. Each is drawn in HTML and CSS after the app's own widgets
(`crates/poe2-oracle/src/ui/style.rs`) and palette (`ui/theme.rs`), from
`site/ui/data/<part>.<en|ru>.json`. To show another item or other numbers, edit the JSON; when the
app's look changes, change its drawing in the same pull request.

The README and the guide show the same drawings as images. After changing a drawing or its data,
save them again (Node 22 or newer, and Chrome or Chromium; `CHROME` points to it if it's not on
`PATH`):

```sh
node packaging/render-ui.mjs
```

Flags after it go to Chrome: on Ubuntu 23.10 or newer, which restricts the sandbox Chrome needs,
add `--no-sandbox`. It writes `docs/guide/src/images/<en|ru>/*.webp` and the social previews
`og.jpg` from `site/ui/shots.html`. The app's text face is Segoe UI, which comes with Windows and
may not be shared: render on Windows or with Segoe UI installed, or the script warns and the
pictures' text comes out in another face. The XP overlay's pictures lie on two crops of the game's
HUD, `site/ui/img/hud-flask.webp` and `hud-skill.webp` (840 px), with copies halved to 420 px,
`hud-*-420.webp`, that the site gives 1x screens and phones; replace a crop and halve its copy
too. The plates on them are the app's own
pixels, `site/ui/img/plate-flask.png` and `plate-skill.png`: after changing the plates' look, draw
them again from the app's code before running `render-ui.mjs`:

```sh
cargo run -p poe2-oracle --example plate_art -- site/ui/img
```

## The web service

`crates/oracle-web` is the service at oracle.pushka.biz. It serves the landing pages (`site/`), the
guide (`docs/guide`, a book per language) and its images; takes reports on `POST /api/v1/reports`
and passes each one on to the maintainer, as an issue in this repository and a Telegram message;
serves the latest app release and the latest data pack, with their files, from this private
repository (`/api/v1/releases/latest`, `/api/v1/data/latest`, `/download/<tag>/<file>`,
`/download/latest`); tells the running apps about both over an event stream (`/api/v1/events`);
and posts a daily digest of downloads, stream connections, update checks and reports. What the app
and the service share is in `crates/oracle-protocol`: the service's address, the report, release
and event types, and the report limits (`Report::check`, which both sides run).

The service lists the repository's releases when it starts and every two minutes after, sending
GitHub the last list's ETag, so an unchanged list costs no rate limit. Of the published releases
(drafts and prereleases never count) it offers two: the app's, the `v<semver>` tag highest by
semver precedence, and the data pack, the `data-<N>` tag with the highest N; a release of one kind
never stands for the other. `GET /api/v1/events` is a Server-Sent Events stream: a `versions` event
with both versions (`{"app":"0.1.1","data":2026092601}`) as soon as the app connects and again
whenever either changes, and a `: ping` comment after 25 s of silence. One address may hold 64
streams at once and the service 4000 in all (about 23 KiB of memory each). A stream past either
gets no place, only the `versions` event with `retry: 60000`, and ends at once; the app then asks
again on its backoff, which grows to 5 min. When the service shuts down, the streams end, and the
apps reconnect.

The site's root and `/guide/` open in the reader's language: the one in the `lang` cookie, which the
language links on the site and in the guide set when clicked, or else the browser's
`Accept-Language`. `/` answers in English or redirects to `/ru/`, and `/guide/` redirects to
`/guide/en/` or `/guide/ru/`; a page's own address, such as `/ru/` or `/guide/en/install.html`,
never redirects.

It takes its settings from the environment, all optional:

| Variable | Default | What for |
|---|---|---|
| `PORT` | `8080` | Listens on `0.0.0.0:PORT` |
| `PUBLIC_URL` | `https://oracle.pushka.biz` | The public address, no trailing slash; the release answer's download links start with it |
| `SITE_DIR` | `/app/site` | The landing pages (`site/`); a `404.html` there is the 404 page |
| `GUIDE_DIR` | `/app/guide` | The built guide (`docs/guide/build.sh`): a book per language in `en/` and `ru/`, served under `/guide/en/` and `/guide/ru/`; each answers a missing page with its own `404.html` |
| `IMAGES_DIR` | `/app/images` | `docs/guide/src/images`, served under `/images/` and `/guide/images/` |
| `GITHUB_TOKEN` | — | A fine-grained token for this repository (Issues read and write, Contents read): files the report issues, lists the releases and downloads their files. Unset: no issues are filed, `/api/v1/releases/latest`, `/api/v1/data/latest` and `/download` answer 503, and the event stream announces no versions |
| `GITHUB_REPO` | `mttzzz/poe2-oracle` | The repository the issues and releases belong to |
| `GITHUB_API` | `https://api.github.com` | GitHub's REST API, trailing slash trimmed: every GitHub call, issues included, goes there with `GITHUB_TOKEN`. Only for a stand-in: the lane can point it at `lanes/fake-github.py` |
| `LIST_RELEASES_EVERY` | `120` | How often the releases are listed, in whole seconds; under 5 or not a whole number stops the service at start with a configuration error |
| `TELEGRAM_TOKEN`, `TELEGRAM_CHAT_ID` | — | The bot and the chat the reports and the digest go to; with either unset, nothing goes to Telegram |
| `REDIS_URL` | — | Redis for the daily counters (kept 120 days), the report rate limits and the digest lock; unset, they're kept in memory and lost on a restart |
| `RUST_LOG` | `info` | The log filter; the log goes to stdout |

The digest goes out at 09:00 Moscow time: the day before, against the same weekday a week earlier.

To run the service without sending anything, leave `GITHUB_TOKEN` and `TELEGRAM_TOKEN` unset: every
report is still checked, rate-limited and counted, then written to the log instead of sent, and
answered with id 0. Build the guide's two books with `docs/guide/build.sh` and mdBook, the version
the root `Dockerfile` pins as `MDBOOK_VERSION` (`MDBOOK` names the binary if it isn't the `mdbook`
on `PATH`); the script takes an absolute output directory. Then run the service and open
http://localhost:8080/:

```sh
docs/guide/build.sh "$PWD/target/guide"
SITE_DIR=site GUIDE_DIR=target/guide IMAGES_DIR=docs/guide/src/images cargo run -p oracle-web
```

`curl -N http://localhost:8080/api/v1/events` shows the event stream; without a token, its event
announces no versions.

A test build of the app pointed at it (`POE2_ORACLE_API_BASE`) takes release files only from under
that address's `/download/`, so set the service's `PUBLIC_URL` to the same address; a test
release's `SHA256SUMS` still has to be signed with the real release key. The maintainer's lane runs
the service the same way, dry, with `lane dev up` (`lanes/dev.sh`; `lane dev restart` after
editing the guide or the server).

The lane can also offer test releases, with no GitHub. `lanes/publish-release.py` lays a release
out as the release workflows do (its files, `SHA256SUMS` and `SHA256SUMS.sig`, checked against the
app's public key) in `.tmp/lane-dev/fake-github/releases/<tag>/`. The key's seed comes on stdin,
or with `--key-file` from a file outside the checkout, and is never written anywhere:

```sh
lane exec -- lanes/publish-release.py v0.1.1 .tmp/PoE2-Oracle-Setup-0.1.1.exe < <seed file>
lane exec -- lanes/publish-release.py data-2026092601 < <seed file>  # builds this checkout's pack
lane exec -- lanes/publish-release.py --draft v0.1.1 <installer> < <seed file>
lane exec -- lanes/publish-release.py --publish v0.1.1               # the draft goes public
```

A `v<version>` tag needs `PoE2-Oracle-Setup-<version>.exe` among its files, and a pre-release
version is marked a pre-release. A `data-<N>` tag without files gets this checkout's pack, and its
`<N>` must be `data-version.txt`'s. Publishing a tag again replaces its release. When
`.tmp/lane-dev/fake-github/releases` exists as the dev server starts (so `lane dev restart` after
the first release), `lanes/dev.sh` runs `lanes/fake-github.py` in the dev pod as GitHub's stand-in,
with a dummy token, and the service lists it every 10 s (`GITHUB_API`, `LIST_RELEASES_EVERY`).
Reports' issues then land in `.tmp/lane-dev/fake-github/issues`, Telegram stays dry, and nothing
leaves the pod. To go back to the dry run, delete `.tmp/lane-dev/fake-github` and `lane dev
restart`.

Each guide page has the same file name in both languages, `docs/guide/src/en/<page>.md` and
`src/ru/<page>.md`, and each book lists its pages in its own `SUMMARY.md`: the language switch in
the guide's header leads to the page of the same name in the other book. The pictures stay outside
both books, in `docs/guide/src/images/<en|ru>/`; pages show them as `../images/<lang>/<name>`, which
the service serves from `IMAGES_DIR` under `/guide/images/`.

### Deploying the service

`.github/workflows/deploy.yml` deploys on a push to `main` that touches the service's crates,
`site/`, `docs/guide/`, the `Dockerfile` or `.dockerignore`, `deploy/`, `Cargo.lock` or the
workflow itself, and by hand (workflow_dispatch). It builds the root `Dockerfile` (`oracle-web` in
a workspace trimmed to the two crates it needs by `deploy/trim-workspace.sh`, so the app's GPUI
dependency isn't fetched; the guide with the pinned mdBook; `site/` and the guide's images; on a
distroless image) and rolls it out through the shared deploy workflow as the Helm release
`oracle-pushka-biz` on the DigitalOcean cluster (`deploy/values.yaml`: one replica, port 8080,
probe `/healthz`). The service's secrets (`GITHUB_TOKEN`, `TELEGRAM_TOKEN`, `TELEGRAM_CHAT_ID`,
`REDIS_URL`) live in Infisical, project `oracle-pushka-biz`, environment `prod`, which syncs them
into the Kubernetes Secret `oracle-pushka-biz-env-secret`; the workflow's own secrets (the
registry, the Helm repository, DigitalOcean, the Telegram note) are this repository's Actions
secrets.

## Dependencies and licenses

Every dependency must be under a license listed in `about.toml`: cargo-about fails, and with it the
release build, on anything else. Check a new dependency with cargo-about (its binary needs the
`cli` feature):

```sh
cargo install cargo-about --version 0.9.2 --locked --features cli
cargo about generate -m crates/poe2-oracle/Cargo.toml about.hbs -o target/THIRD-PARTY-NOTICES.html
```

## Pull requests

Write to the maintainer first (**Write to the developer** in the **Help** section of the app's
settings) for anything bigger than a small fix, so the approach can be agreed before you spend time
on it. Then:

- [ ] One change per pull request.
- [ ] `cargo fmt`, clippy and the tests pass as in [Checks](#checks-what-ci-runs).
- [ ] New behaviour is covered by a test where a plausible bug would fail it.
- [ ] Changes to the panel or the overlays were tried in the game on Windows; visible changes come
      with a screenshot, and the drawings in `site/ui` follow them.
- [ ] Data tables were regenerated with the scripts, not edited by hand, and
      `crates/oracle-data/data-version.txt` gives them a new version.
- [ ] A new dependency's license is accepted by `about.toml`.
- [ ] `CHANGELOG.md` and `CHANGELOG.ru.md` have a line under `Unreleased` for a change players will
      notice.

## Releases

For the maintainer:

1. Set the new version in the root `Cargo.toml` (`[workspace.package] version`) and commit it
   together with the `Cargo.lock` the next cargo command updates: the release build runs with
   `--locked`.
2. Date the version's heading in `CHANGELOG.md` and `CHANGELOG.ru.md`, keeping its form,
   `## [<version>] - <date>`: `release.yml` takes the release notes from that section.
3. Push a tag `v<version>`. `.github/workflows/release.yml` builds the installer on Windows with
   `packaging/build-release.ps1` (a job without secrets), signs `SHA256SUMS` on Ubuntu with
   `crates/release-sign` and the `RELEASE_SIGNING_KEY` secret, checks that signature against the
   app's public key, and creates a **draft** GitHub release with the installer, `SHA256SUMS` and
   `SHA256SUMS.sig`.
4. Test the draft's installer, then publish the release by hand. oracle.pushka.biz serves only
   published releases, so from then on the app offers it as the latest version and the site's
   download button gives its installer.

A tag with a pre-release suffix (`v0.2.0-rc.1`) becomes a pre-release, which the app never offers.
Game data packs are released on their own, with a `data-<version>` tag: see
[Game data packs](#game-data-packs).

`RELEASE_SIGNING_KEY` is the standard base64 of the 32-byte Ed25519 seed: one line, 44 characters,
kept as a GitHub Actions secret. Its public key, `crates/auto-update/release-signing-key.pub`, is
compiled into the app, which installs an update only if `SHA256SUMS.sig` checks out against it.
`packaging/build-release.ps1` run by hand signs too when `RELEASE_SIGNING_KEY` is set, and
otherwise warns that the release is unsigned. `cargo run -p release-sign -- keygen <secret file>`
makes a new key pair: it writes the seed to a new file and prints only the public key.
`sign <SHA256SUMS> [<out.sig>]` (the key from `RELEASE_SIGNING_KEY`, or `--key-file <file>`) and
`verify <SHA256SUMS> <sig> <public key>` work by hand too.

## License

PoE2 Oracle is licensed under either of the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
