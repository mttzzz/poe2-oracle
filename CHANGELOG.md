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
  by its title bar: the next check on that side opens it there again, even after a restart, as far
  from the inventory or the stash as you left it at any interface scale. Dragged back near the
  inventory or the stash, the panel sticks to it, and a double-click on the title bar puts it back
  there.
- **The price panel:** the item's name in its rarity colour, its art and links to poe2db and the
  wiki, and for an item you can craft, to Craft of Exile, which opens it with its base, item
  level, rarity and mods (in the site's Russian interface when the app's interface is Russian);
  one filter row per stat with min/max values, mod tier and source, pseudo totals, weapon DPS
  and defences; the cheapest listings, one page of 10 per search, with price, item level, seller
  and listing age, one seller's listings at one price in one row. Prices are shown with currency
  icons. Hovering a listing shows it the way the game's item tooltip does: properties,
  requirements, sockets, item level and every mod with its tier and roll range, the searched ones
  marked. When nothing is found, the panel offers broader searches (**Broad −10%**, **Match N of
  M**) rather than spending the trade site's limit on its own.
- **Each price check timed in the log:** one line per check once its panel settles, with the
  times from the key press to the panel shown, its first frame and the results, and each step's
  wall time and UI-thread CPU (copy, parse, placing and showing the panel, the trade search and
  fetch, frames drawn, idle paints, resizes), the UI thread's and the whole process's CPU, and
  the dedicated GPU memory. The copy's log line also says how long sending the combo and giving
  the clipboard back took.
- **GPU memory given back after hiding:** half a second after the price panel or the tour's
  window is hidden and taken down to a pixel, or a window closes, the app flushes and trims its
  Direct3D device and logs its dedicated GPU memory before and after.
- **Search profiles** in the row above the filters: **Quick price** (up to 4 most valuable stats,
  your rolls as minimums), **Exact match**, **Broad −10%** and **Crafting base** (implicit and
  fractured stats on the same base). **Tier minimum** sets each checked stat's minimum to the
  bottom of its tier.
- **The league on the panel:** the top bar names the league being searched, and a click switches
  it there, saved to the settings. Beside it, the Divine Orb's price in Exalted Orbs.
- **Search chips:** base type or the whole item class; rarity (by default a magic item is compared
  with magic items and a rare with rare items; a click switches to all non-unique items); corrupted
  listings in or out; every stat or none; which sellers to include; the currency of the price. As
  in Exiled Exchange 2, an uncorrupted item is compared with uncorrupted listings, gear leaves out
  mirrored and sanctified items, and an unidentified unique is compared with unidentified uniques
  of its base.
- **Whispers:** a click on a listing whose seller trades in person copies the trade site's whisper
  for the game chat.
- **The trade site's buttons on listings**, while you're signed in to pathofexile.com: **To
  hideout** on an instant buyout (the site's Travel to Hideout: it takes your character in the
  game to the seller's hideout; **Anyway** when the site says the item is in demand) and
  **Whisper** for a seller online or away (the site's Direct Whisper, sent from your character).
  One press is one request, never repeated on its own, and the row says how it went: done, the
  listing or the seller gone, the buttons expired, sign in again, the request limit, or no answer.
  The app presses nothing in the game.
- **Currency Exchange items** priced from GGG's own hourly record of the exchange's trades, in a
  market card: value in divine, exalted and chaos orbs, the week's chart and change from
  poe2scout, hourly volume, the most traded pair and the value of the copied stack. An item nobody
  traded on the exchange in the league lately (common in small leagues such as Standard) shows
  poe2scout's price with a note saying so, and its trade listings are searched only on **Trade site
  listings**; without a poe2scout price either, the trade site is searched right away.
- **Unique prices** from poe2scout, shown beside the trade search.
- **Trade site limits kept:** after the trade site refuses a request (HTTP 429), nothing is sent to
  it until the lockout ends, and the panel says when to try again. The limit is per IP address and
  shared with the trade site open in a browser.
- **Waystone marks**, right on the panel: each click on the ◇ at the end of a waystone's modifier
  row steps its mark through danger, caution, wanted and none; the marks are kept and highlighted
  on every waystone checked later.
- **Vendor gamble offers** ("Random Helmet") are recognised and explained instead of searched.
- **Sign-in to pathofexile.com**, optional: Settings → Account → **Sign in** opens the site's own
  sign-in page in a window of the app (Microsoft Edge WebView2), Steam included. The app doesn't
  read or keep the password; the site's session is kept in Windows Credential Manager, sent only
  to pathofexile.com, and removed by **Sign out** or uninstalling. Signed in, the app searches
  private leagues and the trade site's “sum” rows: the league menus offer the account's own
  private leagues, as the site's **Private Leagues** page lists them; nothing is typed in.
  Settings → Account lists them under the sign-in, each with the public league it's made from;
  **Refresh** looks them up again at once, and while you're signed in they're looked up by
  themselves every hour by default (15 minutes, 1 hour, 6 hours or off), without popups. A
  private league trades too little on the Currency Exchange to price by, so its exchange prices,
  Divine Orb rate and poe2scout prices come from the public league it's made from (the one its
  page names), and the market card says so.
- **XP overlay** on top of the game's HUD, on the rails along the top of its flask and skill
  panels -- never over them: the rails are the game's rage and stun gauges -- each plate running on
  to its life or mana globe's frame, the gap there filled to the pixel, and curling down onto the
  tip of the game's scrollwork at its other end. Above the flask panel the levelling rate (percent
  of a level per hour, averaged over 5 to 30 minutes), the time to the next level, optionally the
  level percentage, and a ⚙ that opens the settings; in a pause (a town or hideout, or five minutes
  without a gain) that plate dims to the level percentage alone. Above the skill panel a map timer
  with the map's experience and the session's average map time. The plates are drawn pixel by
  pixel in the HUD's own molding and colours, take the HUD's size, let clicks through to the game
  but for the ⚙, step aside the moment a game tooltip covers their rail while the game is in
  front (within a few seconds while it's behind another window), and say as much as they have
  room for.
- **Quick actions:** up to 12 hotkeys that type a chat command (`/hideout` is preset and waits for
  its key; `@last thanks` answers whoever whispered last) or paste a stash search string. Opened,
  the settings' **Quick actions** section warns if another program holds a combination the actions
  press, such as `Ctrl+F` for a stash search.
- **Settings window:** league, client language, hotkeys, quick actions, default sellers, seller
  column, interface scale (80 to 150 %, the price panel and the settings window), XP overlay, start
  with Windows, automatic updates, reports to the developer and the diagnostics report. It warns
  when the game runs as administrator (and the app doesn't) or in exclusive fullscreen, or another
  program holds the item-copy combination.
- **A game run as administrator** takes no keys from an app that isn't: a press of the price-check
  hotkey or a quick action's key then opens the panel with what's wrong and the fix (start the game
  normally, or the app as administrator too) instead of doing nothing.
- **Title-bar buttons** (⚙ and × on the panel, × on the settings window) light up gold under the
  pointer, with a soft glow behind the glyph.
- **Guided tour** at launch, until finished or skipped, and again from Settings → Help →
  **Tutorial**: the screen dims around one thing at a time, with a card beside it — the league in
  the settings, a first price check in the game, the price panel's filters, Search, listings and
  league, and the XP overlay. Skip tour on every card, Back on all but the first, and Next (Done on
  the last) on all but the first check's; a step that waits for the player (the first check, a
  search) moves on by itself when it happens.
- **English or Russian interface:** Settings → General → **Interface language**: **Auto** (the
  game client's language; before the game's first run, Windows' display language), **Русский** or
  **English**, applied at once, the tray menu included. Only the app's own words change: item
  names and mods stay in the language the game copied them in. Numbers follow the interface
  (`1.72` and `15%` in English, `1,72` and `15 %` in Russian), and so do the league names (as the
  trade site in that language names them), the pathofexile.com sign-in page, Craft of Exile's
  language and the page **Website ↗** opens.
- **Tray icon, taskbar button or both**, as Settings → General → **Where to show the app** picks
  (the tray icon by default; a change applies at once): a click on either opens the settings, or
  brings them to the front. The tray icon's right-click menu has Settings and Quit. The taskbar
  button stays while the app runs: its **Close window** quits, the settings and report windows add
  no button of their own while it shows, and hovering it shows the app's icon. **Quit the app** in
  Settings → Help quits as well. One copy runs per Windows session; starting it again opens the
  settings.
- **Reports to the developer**, from a window of the app, no account needed: **Write to the
  developer** in Settings → Help for a problem or an idea; **report a problem** under the item
  name on the panel, or **Report a problem** under an item the app couldn't read, for that item,
  with its text attached. The kind is **Problem**, **Idea** or **Item**; the text takes up to 8000
  characters, and a contact (Telegram, Discord or email) is optional, for an answer. **Attach
  diagnostics** adds the diagnostics report (on by default for a problem or an item, off for an
  idea), and **What's inside** saves the same zip to the desktop. The window sends the report to
  oracle.pushka.biz, which passes it on to the developer, and then says "Sent — thank you! Report
  #{id}", or "Couldn't send: {reason}" with **Try again**, **Save to desktop** (the whole report as
  one zip) and **Close**.
- **Crash reports:** after a crash, the next launch (within 7 days) opens the report window by
  itself, once, as a **Crash** report with what the app reported attached (the version, the time,
  the panic message, where in the code and the backtrace, with the Windows user name and folders
  masked); writing something is optional. Closing the window drops the report; quitting the app
  while it's open keeps it for the next launch.
- **A report form on the site**, https://oracle.pushka.biz/report.html (Russian: /ru/report.html),
  for a problem or an idea without the app.
- **Automatic updates** from oracle.pushka.biz while **Update automatically** is on (Settings →
  General → Updates; on by default). The app stays connected to the service's event stream, from
  about 10 seconds after start, and hears of a new app version or new game data within about two
  minutes of its publication. A new version's installer, or a game data pack (the tables the app
  reads items with), is downloaded at once and used only after the Ed25519 signature on its
  release's `SHA256SUMS` checks out against the public key built into the app and the file matches
  its SHA-256 there. It goes in once none of the app's windows is open (price panel, settings,
  report window, sign-in window, tour) and no price check or quick action is putting the clipboard
  back: the installer runs silently and restarts the app, or the app restarts itself to load the
  pack. After the restart a plate at the bottom right of the game's screen says "PoE2 Oracle
  updated to X.Y.Z" or "Game data updated" for 8 seconds, without taking the keyboard; × closes it,
  a click elsewhere on it opens the settings. The XP overlay carries on through the restart with
  its rate, time to level and map timer. A dropped connection comes back by itself, and at once
  when Windows says the internet is back (Windows 10 2004 and later); a failed download is retried
  later, quietly. A downloaded update that can't be started (an antivirus blocking the installer)
  isn't downloaded again: only its start is retried, a minute and then two minutes later, and
  after the third failure that version waits for the next launch. The settings show the version,
  the game data's version and what the updater is doing, with **Check now**. Turned off, the app
  doesn't connect to oracle.pushka.biz for updates; only the reports you send go there.
  Pre-releases are never offered.
- **Game data between app releases:** the tables the app reads items with -- the ways the client
  prints a stat that the trade catalog lacks (English and Russian), the mod tiers, the Currency
  Exchange items, and items' English names, art and bases -- are built into the app and also come
  as game data packs. The app takes a pack at its next start only when the pack is newer than the
  built-in tables and needs no newer version of the app, and each of its tables matches the
  SHA-256 the pack lists for it and reads without an error; otherwise the built-in tables stay.
  After an update, a pack no newer than the app's own tables is deleted; a damaged one is set
  aside.
- **Installer** for the current user, without administrator rights: Start menu shortcut, optional
  start with Windows, the license texts and third-party notices next to the app. Uninstalling
  removes the saved sign-in and keeps settings and caches unless asked to remove them. Nothing
  else to install: the C runtime is built into the app, so no Visual C++ Redistributable.
- **Welcome after installing:** the installer's last page starts the app with its settings open at
  General and a welcome over them: PoE2 Oracle is installed and running; where it shows from now
  on (tray icon, taskbar button or both) and that a click there opens the settings; that Windows
  first hides a new tray icon under the ^ arrow, and how to keep it in sight; how to check a price
  with the current hotkey, and where the XP overlay shows; and whether the app starts with
  Windows. **Got it**, Enter or Esc closes it; the guided tour, until done, starts after it. The
  installer's silent updates don't show it.
- **Diagnostics report:** **Collect report** (Settings → Help) saves a zip on the desktop with the
  logs, settings, unread item texts and a summary of the system, with the user's folder paths
  masked, and the Windows user name too if it has three characters or more, and shows it in
  Explorer; a report sent with **Attach diagnostics** on carries the same zip.
