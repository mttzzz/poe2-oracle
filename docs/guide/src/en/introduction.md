# Introduction

![The PoE2 Oracle price panel next to the game's inventory](../images/hero.png)

PoE2 Oracle is a free, open-source overlay for Path of Exile 2 on Windows. Point at an item in the
game, press <kbd>Ctrl</kbd>+<kbd>E</kbd>, and a panel next to your inventory shows what similar
items sell for on the official trade site, with the item's modifiers as filters you can adjust.

## What it does

- **[Price check](price-check.md).** Listings from the Path of Exile trade site with an estimated
  price. Currency and other Currency Exchange items are priced from poe.ninja, with a week's
  chart; unique items also show poe2scout's price. A click copies the whisper to a seller.
- **Waystone marks.** Mark waystone modifiers as dangerous, doubtful or wanted, and every waystone
  you check shows them.
- **[Trade requests](trade-requests.md).** A buyer's whisper becomes a card at the top of the game
  with the item, the price and where it lies in your stash, and buttons that answer in the chat.
- **[Quick actions](quick-actions.md).** Hotkeys that send chat commands such as `/hideout` or paste
  a stash search.
- **[XP overlay](xp-overlay.md).** Experience per hour and the time to the next level, above the
  experience bar, with a map timer.
- **[Updates](updates.md)** from the project's GitHub releases, checked against their SHA-256
  checksums.

## Before you start

- **The interface is in Russian.** This guide quotes every label as it appears in the app, in
  «angle quotes», followed by its English meaning, for example «Поиск» (Search).
- You need Windows 10 or 11 (64-bit), the English or Russian game client, and the game in Windowed
  or Windowed Fullscreen mode. See [Install and first run](install.md).
- PoE2 Oracle needs no account and sends no telemetry. See [Privacy](privacy.md).

## About

PoE2 Oracle is a fan-made tool. This product isn\'t affiliated with or endorsed by Grinding Gear
Games in any way. Path of Exile is a trademark of Grinding Gear Games.

The source code is on [GitHub](https://github.com/mttzzz/poe2-oracle) under the MIT or Apache 2.0
license, at your choice. Its item and modifier tables build on
[Exiled Exchange 2](https://github.com/Kvan7/Exiled-Exchange-2) (MIT).
