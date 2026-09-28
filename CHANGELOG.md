# Changelog

**English** · [Русский](CHANGELOG.ru.md)

All notable changes to PoE2 Oracle are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-09-28

### Added

- **XP overlay diagnostics.** With `RUST_LOG=warn,poe2_oracle=info,poe2_oracle::xp_tracker=debug`
  the log shows every reading of the XP bar that changes something, every gain counted and, every
  30 s of play, what the rate stands on. With `POE2_ORACLE_XP_DEBUG=1` the bar's pixels behind each
  large jump are saved as PNG in `%LOCALAPPDATA%\poe2-oracle\data\xp-debug`, the newest 30 kept.
  See CONTRIBUTING.

### Fixed

- **Pointing at the game's XP bar no longer inflates the rate.** With the pointer resting on the bar
  for its tooltip, the bar read nearly full for as long as the pointer stayed, and the overlay
  counted that as experience: a player at level 93 saw +250-350%/h for half an hour. The overlay now
  leaves the bar unread while the pointer is on it, and for a second after.
- **Any other misread of the bar no longer counts as experience.** A jump of more than 5% of a
  level, or any drop, counts once the bar has shown it for 20 seconds; if the bar goes back before
  that, it was a misread and changes nothing. A level-up the game's log reports still counts at
  once.

## [0.1.0] - 2026-09-27

The first public release, on https://oracle.pushka.biz/. The installer isn't digitally signed yet,
so Windows may say "Windows protected your PC": click **More info**, then **Run anyway**.

### Added

- **Price check on `Ctrl+E`.** Point at an item in your inventory, your stash or a vendor's window
  and press the hotkey (you can change it). A panel opens next to the inventory with the cheapest
  similar listings on the official trade site: price, item level, seller and how long ago each was
  listed. Hover a listing to see it the way the game's tooltip shows it. Both the English and the
  Russian game client work: the search goes to the trade site in the item's language. Drag the
  panel aside, and it opens there next time.
- **Filters you can adjust.** Each of the item's modifiers is a row with its tier and the range to
  search. Search profiles pick the rows for you: **Quick price** (up to four of the most valuable
  modifiers), **Exact match**, **Broad −10%** and **Crafting base**. One click widens or narrows the
  search: the item's base or its whole class, its rarity, corrupted items, which sellers (instant
  buyout by default) and the currency of the price. When nothing is found, the panel offers a
  broader search.
- **Currency Exchange prices.** Currency, omens, runes, essences and everything else on the
  exchange get a market card priced from GGG's own hourly record of the exchange's trades: the
  value in divine, exalted and chaos orbs, a seven-day chart from poe2scout, the hourly volume and
  what your stack is worth. An item nobody has traded lately shows poe2scout's price instead.
- **Unique item prices** from poe2scout, next to the trade search.
- **The league on the panel,** with the Divine Orb's price in Exalted Orbs beside it; a click
  switches the league.
- **Links** to poe2db, the wiki and Craft of Exile. The Craft of Exile link opens the item in its
  crafting simulator with the base, item level, rarity and modifiers already set, from the Russian
  client too.
- **Whispers.** A click on the listing of a seller who trades in person copies the trade site's
  whisper for the game chat.
- **Waystone marks.** Mark a waystone's modifiers as danger, caution or wanted, and every waystone
  you check later shows them.
- **Vendor gamble offers** are recognised: instead of searching, the panel says the item is
  revealed only after you buy it.
- **Sign-in to pathofexile.com, optional,** on the site's own page, in a window of the app. It adds
  your private leagues to the league menus by themselves, the **sum** filter rows, and the trade
  site's **To hideout** and **Whisper** buttons on listings. The app never sees your password: it
  keeps only the site's session, in Windows Credential Manager.
- **The trade site's limits respected.** After the site refuses a search, the app sends it nothing
  until the lockout ends, and the panel says when to try again.
- **XP overlay** on the game's HUD, in its own frames and colours. Above the flask panel: how fast
  you level, in percent of a level per hour, and the time to the next level. Above the skill panel:
  a timer for the current map, with the map's experience and your average map time. Time in towns
  and hideouts stays out of the levelling speed: there its plate dims and shows only how much of
  the level you have. The plates step aside when a game tooltip covers them and let your clicks
  through to the game, except on the ⚙ that opens the settings.
- **Quick actions:** up to 12 keys of your choice that send a chat command (`/hideout`,
  `@last thanks`) or type a stash search. One press, one command.
- **Settings** for the league, the hotkeys, the quick actions, the default sellers, the interface
  scale (80 to 150%), the XP overlay, starting with Windows and updates. Warnings at the top say
  what keeps price checks from working: the game in exclusive Fullscreen or run as administrator,
  or another program holding the game's item-copy keys.
- **English and Russian interface,** in the game client's language by default.
- **A tray icon, a taskbar button or both,** as you choose; a click opens the settings.
- **A welcome after installing and a short tour** of the league setting, a first price check, the
  price panel and the XP overlay. Settings → Help → **Tutorial** shows it again.
- **Automatic updates** from oracle.pushka.biz. A new version or new item data installs by itself
  within minutes of its release, once none of the app's windows is open, and a plate over the game
  says so after the restart. Nothing runs unless it carries the developer's signature and arrived
  whole. **Update automatically** in the settings turns updates off.
- **Reports to the developer** from a window of the app, with no account: a problem, an idea, or
  an item read or priced wrong, with the item's text and a diagnostics report if you like. After a
  crash, the next start opens this window with what the app reported attached. Without the app, the
  form at https://oracle.pushka.biz/report.html takes a problem or an idea.
- **Installer** for your Windows user, with no administrator rights needed: a Start menu shortcut
  and, if you like, a start with Windows. Its first page says what the app sends over the internet
  and links the privacy policy. Uninstalling keeps your settings unless you tick **Settings and
  cache**.

[0.1.0]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.0
