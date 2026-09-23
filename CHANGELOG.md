# Changelog

**English** · [Русский](CHANGELOG.ru.md)

All notable changes to PoE2 Oracle are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - Unreleased

The first public release.

### Added

- **Price check.** Hover an item in the game and press `Ctrl+E` (rebindable): the app copies the
  item with the game's advanced copy (`Ctrl+Alt+C`), puts your clipboard back and opens a panel
  next to the inventory or the stash. Works with English and Russian game clients; searches go to
  www.pathofexile.com or ru.pathofexile.com, matching the item's language. Drag the panel sideways
  by its title bar: the next check on that side opens it there again, even after a restart, and a
  double-click on the title bar puts it back next to the inventory or the stash.
- **The price panel:** the item's name in its rarity colour, its art and links to poe2db and the
  wiki, and for an item you can craft, to Craft of Exile, which opens it with its base, item
  level, rarity and mods (in the site's Russian interface when the app's interface is Russian);
  one filter row per stat with min/max values, mod tier and source, pseudo totals, weapon DPS
  and defences; the cheapest listings, one page of 10 per search, with price, item level, seller
  and listing age; a price estimate with its range and confidence, and a warning when the listings
  look price-fixed. Prices are shown with currency icons. Hovering a listing shows it the way the
  game's item tooltip does: properties, requirements, sockets, item level and every mod with its
  tier and roll range, the searched ones marked. When nothing matches every selected stat, the
  search is relaxed to most of them and says so.
- **Search chips:** base type or the whole item class; rarity (a magic item is compared with magic
  items, a rare with all non-unique items); corrupted listings in or out; every stat or none;
  which sellers to include; the currency of the price. A magic item's prefix and suffix are both
  selected from the start, whatever their tier. As in Exiled Exchange 2, an uncorrupted item is
  compared with uncorrupted listings, gear leaves out mirrored and sanctified items, and an
  unidentified unique is compared with unidentified uniques of its base.
- **Whispers:** a click on a listing whose seller trades in person copies the trade site's whisper
  for the game chat.
- **Currency Exchange items** priced from poe.ninja in a market card: value in divine, exalted and
  chaos orbs, the week's chart and change, hourly volume, the most traded pair and the value of the
  copied stack. An item poe.ninja doesn't track in the league (common in small leagues such as
  Standard) shows poe2scout's price with a note saying so, and its trade listings are searched only
  on **Trade site listings**; without a poe2scout price either, the trade site is searched right
  away.
- **Unique prices** from poe2scout, shown beside the trade search.
- **Trade site limits kept:** after the trade site refuses a request (HTTP 429), nothing is sent to
  it until the lockout ends, and the panel says when to try again. The limit is per IP address and
  shared with the trade site open in a browser.
- **Waystone marks:** modifiers can be marked as dangerous, doubtful or wanted; the marks are kept
  and highlighted on every waystone checked later.
- **Vendor gamble offers** ("Random Helmet") are recognised and explained instead of searched.
- **Trade requests:** buyers' whispers from the trade site (English or Russian) appear as cards at
  the top of the screen with the item, price, stash tab and position, the buyer's note, repeats
  and arrival. Buttons answer in the game chat (invite, trade, one moment, sold, thanks, kick) or
  search the open stash for the item; clicking them leaves the keyboard with the game. An optional
  sound announces a new request.
- **XP overlay** above the experience bar: the levelling rate (percent of a level per hour,
  averaged over 5 to 30 minutes) and the time to the next level; optionally the level percentage
  and a map timer with the map's experience and the session's average map time.
- **Quick actions:** up to 12 hotkeys that type a chat command (`/hideout` is preset, `@last`
  answers whoever whispered last) or paste a stash search string.
- **Settings window:** league, client language, hotkeys, quick actions, value tolerance (±10 % by
  default, up to ±50 %), default sellers, seller column, interface scale (80 to 150 %), XP overlay,
  trade requests, start with Windows, update checks, bug reporting and the diagnostics report. It
  warns when the game runs in exclusive fullscreen or another program holds the item-copy
  combination.
- **Guided tour** at launch, until finished or skipped, and again from Settings → Help →
  **Tutorial**: the screen dims around one thing at a time, with a card beside it — the league in
  the settings, a first price check in the game, the price panel's filters, Search, listings and
  league, and the XP overlay. Next, Back and Skip tour on every card; a step that waits for the
  player (the first check, a search) moves on by itself when it happens.
- **English or Russian interface:** Settings → General → **Interface language**: **Auto** (the
  game client's language; before the game's first run, Windows' display language), **Русский** or
  **English**, applied at once, the tray menu included. Only the app's own words change: item
  names and mods stay in the language the game copied them in. Numbers follow the interface
  (`1.72` and `15%` in English, `1,72` and `15 %` in Russian), and so do the league names (as the
  trade site in that language names them), the pathofexile.com sign-in page, Craft of Exile's
  language and the GitHub forms the app fills in.
- **Tray icon** with Settings, the update check, Report a bug and Quit. One copy runs per Windows
  session; starting it again opens the settings.
- **Bug reports from the app:** **Report a bug**, in the tray menu and the settings, writes the
  diagnostics report to the desktop, shows it in Explorer and opens GitHub's bug form with the
  version, the client language and the report's file name filled in. The panel's **report a
  problem ↗** link under the item name, and **Report to the developer** under an item the app
  couldn't read, open the item form with the item text filled in. The player submits the form
  from their own GitHub account; the app sends nothing.
- **Updates** from GitHub Releases: the installer is downloaded, verified against the release's
  `SHA256SUMS`, installed silently, and the app restarts.
- **Installer** for the current user, without administrator rights: Start menu shortcut, optional
  start with Windows, the license texts and third-party notices next to the app. Uninstalling keeps
  settings and caches unless asked to remove them.
- **Diagnostics report:** a zip on the desktop with the logs, settings, unread item texts and a
  summary of the system, with the Windows user name masked.

[0.1.0]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.0
