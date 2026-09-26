# Introduction

![The PoE2 Oracle price panel, a currency's market card and the XP overlay's line](../images/en/hero.webp)

PoE2 Oracle is a free overlay for Path of Exile 2 on Windows. Point at an item in the
game, press <kbd>Ctrl</kbd>+<kbd>E</kbd>, and a panel next to your inventory shows what similar
items sell for on the official trade site, with the item's modifiers as filters you can adjust.

**[Download for Windows](../../download/latest)** · [Project site](../../)

## What it does

- **[Price check](price-check.md).** Listings from the Path of Exile trade site, cheapest first.
  Currency and other Currency Exchange items are priced from GGG's own record of the
  exchange's trades, with poe2scout's chart of the week; unique items also show poe2scout's price.
  Listings of sellers who trade in person carry a ✉: a click copies the whisper to that seller.
  **Instant Buyout** listings, which a check searches by default, need none.
- **Waystone marks.** Mark Waystone modifiers as danger, caution or wanted, and every Waystone you
  check shows them.
- **[Quick actions](quick-actions.md).** Hotkeys that send chat commands such as `/hideout` or paste
  a stash search.
- **[XP overlay](xp-overlay.md).** Your levelling rate, in percent of a level per hour, and the
  time to the next level, above the game's flask panel, and a map timer above its skill panel.
- **[Updates](updates.md).** A new version from oracle.pushka.biz shows up in the tray icon's menu;
  a click installs it, and only once the release's signature and the installer's SHA-256 checksum
  check out.
- **[Reporting a problem](report.md).** Write to the developer from the app's report window, with
  the item's text or a diagnostics report attached if you like; no account needed. After a crash,
  the window opens by itself on the next start.

## Before you start

- **The interface is in English or Russian.** By default it follows the game client's language,
  or Windows' before the game has ever run; **Interface language** in the
  [settings](settings.md#interface-language) changes it. This guide names the labels as the English
  interface shows them, in **bold**. Item names and mod lines keep the language the game gives
  them; league names are the ones the trade site uses in the interface language.
- You need Windows 10 or 11 (64-bit), the English or Russian game client, and the game in Windowed
  or Windowed Fullscreen mode. See [Install and first run](install.md).
- PoE2 Oracle needs no account for price checks and sends no telemetry. Signing in to
  pathofexile.com is optional: it opens private leagues and **sum** filter rows. See
  [Privacy](privacy.md).

## About

PoE2 Oracle is a fan-made tool. This product isn\'t affiliated with or endorsed by Grinding Gear
Games in any way. Path of Exile is a trademark of Grinding Gear Games.

PoE2 Oracle is licensed under the [MIT](../../LICENSE-MIT.txt) or
[Apache 2.0](../../LICENSE-APACHE.txt) license, at your choice; both license texts are installed
next to the program. Its item and modifier tables build on
[Exiled Exchange 2](https://github.com/Kvan7/Exiled-Exchange-2) (MIT).
