# PoE2 Oracle

A native Path of Exile 2 price checker for Windows, built on
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's Rust UI framework) --
no Electron, no web view, no Overwolf. The interface is Russian; the game client may be Russian
or English.

## What it does

- **Price check.** Hover an item in the game and press `Ctrl+E` (rebindable). A panel opens
  beside the inventory (or the stash) with the item's name and art, poe2db/wiki links, its stat
  filters -- tiers, pseudo totals, weapon DPS, defences, flag mods -- and the trade site's
  cheapest listings with an estimate. A rare is searched among its whole class; the class chip
  switches the search to its base type and back. As in EE2, an uncorrupted item is compared with
  uncorrupted listings only (a chip lets corrupted ones in), and gear with neither mirrored
  copies nor sanctified items; an unidentified unique is compared with unidentified uniques of
  its base. The stats-count chip checks every stat or none, and the "Цена" chip asks for prices
  in one currency (exalted or divine, exalted, divine, chaos) -- also offered when the listings
  look price-fixed. When nothing matches every selected stat, the search is relaxed to most of
  them and says so, with a low-confidence estimate. Hovering a listing shows its mods -- the
  searched ones ticked or marked short of the bounds, the ones it lacks listed, each mod's item
  level -- and after a relaxed search every row says how many searched stats it meets. A unique
  also shows poe2scout's price. Currency Exchange items (currency, omens, runes, essences, uncut
  gems, ...) are priced from poe.ninja with its weekly chart; what poe.ninja doesn't price, or
  everything while it can't be reached, falls back to the trade site's listings. A click on a
  listing whose seller trades in person (✉) copies the trade site's whisper for the game's chat.
  Chips and price markers say what they mean on hover. Clicks on the panel leave the keyboard
  with the game; only the number boxes take it. Esc or × closes the panel.
- **Waystones.** Mark modifiers as dangerous / warning / wanted; the marks are remembered.
- **XP overlay.** Above the experience bar: the experience rate over a chosen window (5-30 min)
  and the time to the next level, optionally the level percent and the current map's timer and
  gain with the session's average map time -- read from the bar itself and `Client.txt`.
- **Quick actions.** Hotkeys that type into the game: chat commands (`/hideout`, `@last спасибо`
  answers the last whisper, `/invite @last`) and stash searches (a poe2.re string), pasted the
  way EE2 does it, with the player's clipboard put back right after.
- **Trade requests.** A buyer's whisper from the trade site (English or Russian) shows as a card
  at the top of the game: item, price, stash tab and cell, the buyer's own note, whether they
  asked again or already joined the party. Its buttons answer in the game's chat -- invite,
  trade, one moment, sold, thanks, kick -- one message per click, and "Найти" searches the open
  stash for the item; clicking never takes the keyboard from the game. An optional sound
  announces a new request.
- **Settings** (tray menu, the panel's gear, or launching the app again while it runs -- one copy
  runs at a time): league, client language, hotkey, quick actions, value tolerance, default
  sellers, seller column, interface scale (80-150 %, the whole panel scales), XP overlay, trade
  requests, start with Windows, update checks, and a diagnostics report.
- **Setup checks.** The settings window warns about what keeps checks from working: exclusive
  fullscreen (use "Windowed Fullscreen") and another program holding the item-copy combo
  (`Ctrl+Alt+C`). A check swallowed by such a program says so in the panel instead of doing
  nothing. The first launch opens the settings with a short welcome.
- **Installer and updates.** A per-user NSIS installer; the tray's update entry installs newer
  GitHub releases after verifying their SHA-256. Installing over a running copy or uninstalling
  asks it to quit and closes it by force only if it doesn't.

The hotkeys are held only while the game is in front (the price check's also while the panel
is), so `Ctrl+E` and the rest stay usable in other programs. Idle, the app uses about half a
percent of one CPU core and no GPU time.

## Files

| What | Where |
|---|---|
| Settings | `%APPDATA%\poe2-oracle\config\settings.json` |
| Log (this run, and the previous run's) | `%LOCALAPPDATA%\poe2-oracle\data\logs\` |
| Item texts the parser couldn't read | `%LOCALAPPDATA%\poe2-oracle\data\unparsed\` |
| Trade catalogs, poe.ninja market, downloaded updates | `%LOCALAPPDATA%\poe2-oracle\cache\` |
| Diagnostics report ("Собрать отчёт" in the settings) | a zip on the desktop, user name masked |

## Workspace

```
crates/
  poe2-oracle/     the app: GPUI panel, settings, XP overlay, tray, Win32 overlay and hooks
  item-parser/     clipboard item text -> ParsedItem (English and Russian clients)
  stat-filters/    ParsedItem -> the trade search's filter rows (EE2's defaults)
  trade-client/    trade API (leagues, catalogs, search, fetch, rate limits), poe.ninja market
  poe2-domain/     shared item/stat types, no I/O
  auto-update/     GitHub release check and SHA-256-verified installer download
  oodle-ffi/, poe-bundle/, poe-dat/, data-pipeline/
                   local game-data extraction (Bundles2 + .datc64 through a real Oodle DLL);
                   the app never depends on them
```

## Building

Everything project-related runs in the checkout's lane (`lane exec -- cargo ...`). CI
(`.github/workflows/ci.yml`) runs `cargo fmt --check`, clippy with `-D warnings` (the app itself
for `x86_64-pc-windows-gnu`), the tests, and a windows-gnu build.

A binary meant to run is built natively on Windows (MSVC toolchain and Windows SDK: GPUI compiles
its shaders with the SDK's `fxc.exe`); a windows-gnu cross build is for type-checking only.
`packaging/build-release.ps1` builds the release exe, the installer
`target\dist\PoE2-Oracle-Setup-<version>.exe` and `SHA256SUMS`; pushing a tag
`v<workspace version>` runs it in `.github/workflows/release.yml` and publishes the release the
updater reads. The updater asks this repository's GitHub releases; a build with
`POE2_ORACLE_RELEASES_URL` set asks another URL of the same shape instead (a public releases
repository's `https://api.github.com/repos/<owner>/<repo>/releases/latest`, or a local stand-in
for testing an update end to end).

After a patch or at a new league: `packaging/data/generate_stat_matchers.py <EE2's
renderer/public/data>` refreshes the ways the client prints stats that the parser falls back on
(`generate_item_refs.py` the item database), and `cargo run -p item-parser --example sweep --
<cache dir> <texts>` runs item texts swept from the game (a vendor, the stash, the inventory)
against the app's cached catalogs and lists every item or line it can't handle.
