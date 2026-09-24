# Contributing to PoE2 Oracle

**English** · [Русский](CONTRIBUTING.ru.md)

Thank you for helping. The most useful contributions are precise bug reports and the texts of items
the app reads wrong; code is welcome too. Everyone taking part follows the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Filing an issue

1. **Search first**, among open and closed issues. If your problem is already there, add what is
   new to it (a different item, another client language) instead of opening a second issue.
2. **One problem per issue.** Two unrelated bugs in one issue tend to get only one of them fixed.
3. **Use the forms**, they ask for what is needed:
   [bug report](https://github.com/mttzzz/poe2-oracle/issues/new?template=bug_report.yml),
   [item problem](https://github.com/mttzzz/poe2-oracle/issues/new?template=item_problem.yml)
   (not recognised, wrong filters, wrong price),
   [feature request](https://github.com/mttzzz/poe2-oracle/issues/new?template=feature_request.yml).
   You can write in English or Russian.
4. **Always include:**
   - the app version: Windows Settings → Apps → Installed apps shows it next to PoE2 Oracle;
   - the game client language (English or Russian);
   - what you did, what you expected and what happened instead.
5. **For an item problem, include the item text.** The quickest way is the link
   **report a problem ↗** under the item name on the price panel, or the button **Report a problem**
   in the message about an item the app couldn't read: either opens the item form with the text
   filled in. By hand: in the game, hover the item and press `Ctrl+Alt+C` (the game's advanced
   copy, the same one the app uses: it includes the mod tiers), then paste it into the form. Texts
   the app couldn't read are also kept in `%LOCALAPPDATA%\poe2-oracle\data\unparsed`.
6. **Attach the diagnostics report** when the problem isn't about one item. **Report a bug** in the
   tray menu, or **Report ↗** in the **Help** section of the settings, writes
   `PoE2-Oracle-report-<date>_<time>.zip` to your desktop, shows it in Explorer and opens the bug
   form with the version, the client language and the zip's name filled in; drag the zip into the
   form. (**Collect report** only writes the zip.) It holds the logs of the current and the previous
   run, your settings, the item texts the app couldn't read and a summary of the system (Windows
   version, monitors, the game window's size, the game's display mode, language and copy key, the
   cached files). Your Windows user name, if it has three characters or more, is replaced with
   `%USERNAME%` in every file, and the paths of your user, Desktop, Documents and AppData folders
   with `%USERPROFILE%`, `%DESKTOP%`, `%DOCUMENTS%`, `%APPDATA%` and `%LOCALAPPDATA%`. You can open
   the zip and check it before attaching it. The app sends nothing itself: you submit the form from
   your own GitHub account.

**Never post** passwords, your pathofexile.com session cookie (`POESESSID`) or any other token in
issues, logs or screenshots: whoever has them can act as you on the site.

Security vulnerabilities are not reported in public issues; see [SECURITY.md](SECURITY.md).

## The code

```text
crates/
  poe2-oracle/     the app: price panel, settings, XP overlay, live search, tray, updates,
                   Win32 overlay windows, hotkeys and the game's input
  item-parser/     clipboard item text -> ParsedItem (English and Russian clients)
  stat-filters/    ParsedItem -> the trade search's filter rows (built as in Exiled Exchange 2;
                   which ones a search starts with, and their bounds, as in PoE Overlay II)
  trade-client/    trade API (leagues, catalogs, search, fetch, rate limits), the Currency Exchange
                   market from GGG's hourly exchange data, poe2scout prices
  poe2-domain/     shared item and stat types, no I/O
  auto-update/     GitHub release check and SHA-256-verified installer download
packaging/         release script, NSIS installer, data table generators (packaging/data)
docs/guide/        the user guide (mdBook), published with the site
site/              the project site's landing pages
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
  just opens that copy's settings and exits: quit the installed one from its tray icon first.
- A debug build compiles GPUI's shaders at run time from the source checkout, so a debug exe runs
  only on the machine that built it. Give testers a release build.
- The log is `%LOCALAPPDATA%\poe2-oracle\data\logs\poe2-oracle.log`. `RUST_LOG` replaces its
  default filters (for example `RUST_LOG=debug`); with a console attached, the lines also go to
  stderr.
- `POE2_ORACLE_KEEP_ITEM_TEXTS=1` keeps the text of every checked item in `data\unparsed`, not only
  the troubled ones: handy for collecting parser fixtures.
- `POE2_ORACLE_CLIENT_LOG=<file>` makes the app read that file instead of the game's `Client.txt`,
  so the XP overlay can be tested by appending lines to it.
- `POE2_ORACLE_RELEASES_URL` is read at build time: the updater then asks that URL instead of this
  repository's latest GitHub release, for testing an update end to end against a local stand-in.

## Checks (what CI runs)

CI (`.github/workflows/ci.yml`) runs on Linux, in the container built from
`lanes/runner.Dockerfile` (Rust with the `x86_64-pc-windows-gnu` cross toolchain, rustfmt and
clippy). The windows-gnu cross build is for type-checking only; it is not a binary to ship. To run
exactly what CI runs (Linux, macOS or WSL with Docker):

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
    cargo build --workspace --target x86_64-pc-windows-gnu
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
- The app's interface is Russian; new interface text is Russian too, in the wording the panel
  already uses.
- Tests go where a plausible bug would fail them. For the parser, that is a real item text copied
  from the game (`Ctrl+Alt+C`) saved under `crates/item-parser/tests/fixtures/`, plus what it must
  parse to.

## Regenerating the data tables

Some tables come from [Exiled Exchange 2](https://github.com/Kvan7/Exiled-Exchange-2) (MIT), at the
commit named in `crates/poe2-oracle/assets/data/NOTICE` and `crates/item-parser/data/NOTICE`. After
a game patch or at a new league, from the repository root:

```sh
# The ways the client prints stats: crates/item-parser/data/stat-matchers-{en,ru}.tsv
python3 packaging/data/generate_stat_matchers.py <EE2>/renderer/public/data

# The item reference table (names, art, bases): crates/poe2-oracle/assets/data/item-refs.tsv
python3 packaging/data/generate_item_refs.py <EE2>/renderer/public/data <RePoE folder>
```

`crates/stat-filters/src/better.rs` (stats where a lower roll is better) is kept by hand from the
same stat database; its module comment says how. Don't edit the generated `.tsv` files by hand. When
you move to another EE2 commit, update the commit in both `NOTICE` files and in `about.hbs`, which
copies them into the third-party notices.

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
HUD, `site/ui/img/hud-flask.webp` and `hud-skill.webp`.

## Dependencies and licenses

Every dependency must be under a license listed in `about.toml`: cargo-about fails, and with it the
release build, on anything else. Check a new dependency with cargo-about (its binary needs the
`cli` feature):

```sh
cargo install cargo-about --version 0.9.2 --locked --features cli
cargo about generate -m crates/poe2-oracle/Cargo.toml about.hbs -o target/THIRD-PARTY-NOTICES.html
```

## Pull requests

Open an issue first for anything bigger than a small fix, so the approach can be agreed before you
spend time on it. Then:

- [ ] One change per pull request, linked to its issue.
- [ ] `cargo fmt`, clippy and the tests pass as in [Checks](#checks-what-ci-runs).
- [ ] New behaviour is covered by a test where a plausible bug would fail it.
- [ ] Changes to the panel or the overlays were tried in the game on Windows; visible changes come
      with a screenshot, and the drawings in `site/ui` follow them.
- [ ] Data tables were regenerated with the scripts, not edited by hand.
- [ ] A new dependency's license is accepted by `about.toml`.
- [ ] `CHANGELOG.md` and `CHANGELOG.ru.md` have a line under `Unreleased` for a change players will
      notice.

## Releases

For the maintainer: set the new version in the root `Cargo.toml` (`[workspace.package] version`),
move the `Unreleased` changelog entries under it, and push a tag `v<version>`.
`.github/workflows/release.yml` builds the installer with `packaging/build-release.ps1` and
publishes it with `SHA256SUMS` as the GitHub release the in-app updater reads. A tag with a
pre-release suffix (`v0.2.0-rc.1`) becomes a pre-release, which the updater never offers.

## License

PoE2 Oracle is licensed under either of the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
