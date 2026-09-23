# Introduction

![The PoE2 Oracle price panel next to the game's inventory](../images/hero.png)

PoE2 Oracle is a free, open-source overlay for Path of Exile 2 on Windows. Point at an item in the
game, press <kbd>Ctrl</kbd>+<kbd>E</kbd>, and a panel next to your inventory shows what similar
items sell for on the official trade site, with the item's modifiers as filters you can adjust.

## What it does

- **[Price check](price-check.md).** Listings from the Path of Exile trade site, cheapest first.
  Currency and other Currency Exchange items are priced from GGG's own record of the
  exchange's trades, with poe2scout's chart of the week; unique items also show poe2scout's price.
  A click copies the whisper to a seller.
- **Waystone marks.** Mark Waystone modifiers as danger, caution or wanted, and every Waystone you
  check shows them.
- **[Quick actions](quick-actions.md).** Hotkeys that send chat commands such as `/hideout` or paste
  a stash search.
- **[XP overlay](xp-overlay.md).** Experience per hour and the time to the next level, built into
  the game's flask panel, and a map timer on its skill panel.
- **[Updates](updates.md)** from the project's GitHub releases, checked against their SHA-256
  checksums.

## Before you start

- **The interface is in English or Russian.** By default it follows the game client's language,
  or Windows' before the game has ever run; **Interface language** in the
  [settings](settings.md#interface-language) changes it. This guide names the labels as the English
  interface shows them, in **bold**. Item names, mod lines and league names keep the language the
  game and the trade site give them.
- You need Windows 10 or 11 (64-bit), the English or Russian game client, and the game in Windowed
  or Windowed Fullscreen mode. See [Install and first run](install.md).
- PoE2 Oracle needs no account and sends no telemetry. See [Privacy](privacy.md).

## About

PoE2 Oracle is a fan-made tool. This product isn\'t affiliated with or endorsed by Grinding Gear
Games in any way. Path of Exile is a trademark of Grinding Gear Games.

The source code is on [GitHub](https://github.com/mttzzz/poe2-oracle) under the MIT or Apache 2.0
license, at your choice. Its item and modifier tables build on
[Exiled Exchange 2](https://github.com/Kvan7/Exiled-Exchange-2) (MIT).
