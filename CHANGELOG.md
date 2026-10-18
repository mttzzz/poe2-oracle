# Changelog

**English** · [Русский](CHANGELOG.ru.md)

All notable changes to PoE2 Oracle are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
<!-- ANCHOR: releases. The guide's "What's new" page shows every section below, up to ANCHOR_END. -->

## [0.1.19] - 2026-10-18

### Fixed

- **Mageblood's legacies.** A Mageblood was searched among belts with its legacies among any
  others, and how many copies of a legacy it has didn't count: one with Legacy of Quicksilver three
  times and Legacy of Gold took 343 listings, most with other legacies. Now each legacy's row
  searches at least the belt's copies (**Legacy of Quicksilver ×3**), and a new checked row, **No
  other Mage's Legacies**, leaves out belts with a legacy yours lacks: that belt finds the 11
  listings with the same four legacies. Lower a row's copies or uncheck that row to compare more
  widely.

## [0.1.18] - 2026-10-18

### Fixed

- **Fractured listings.** The search of an unfractured item took fractured listings too, and on
  the trade site they are no rarity: for an unfractured Absent Amulet with +3 to Level of all Spell
  Skills and open affixes, 522 of the 562 listings it found were fractured, listed from 1 divine,
  while unfractured ones like it were listed from 175. A fractured modifier can't be changed, so
  fractured items sell at other prices. Now an unfractured item is searched among unfractured
  listings and a fractured one among fractured ones, and a fractured item's chip row has a list
  like corruption's: **Fractured only**, where it starts, **Unfractured only** or **Fractured or
  not**.

## [0.1.17] - 2026-10-17

### Fixed

- **Raised modifiers.** A jewel's increased Effect of Suffixes or Prefixes, or a catalyst's quality
  on a ring or amulet, raises the values of the modifiers it covers, and the trade site lists the
  raised values. The price check searched the rolls the game's copy prints before the raise: a
  jewel with 53% increased Effect of Suffixes was searched from 19% Critical Spell Damage Bonus
  where it has 29%, among 33 listings rather than 7. Raised modifiers are now searched at the values
  the item has, and their sliders, tier hints and **Tier minimum** are raised too. The crafted
  Effect of Suffixes row itself, which rolls in a range, now has a minimum as well, rather than only
  asking for the modifier.

## [0.1.16] - 2026-10-13

### Fixed

- **Mageblood's legacies.** The four legacies of a Mageblood (**Legacy of Diamond** and the like)
  were unread lines: they couldn't be checked, and the search took every Mageblood, 5130 listings
  where one with the same four legacies has 2. They are now read as the trade site lists them and
  checked like the other rows. The game copies them with the range of their options after them,
  `(Amethyst-Topaz)`; any modifier it copies that way is read the same.

## [0.1.15] - 2026-10-12

### Added

- **Sanctification is a list too.** A sanctified item was searched among sanctified listings only,
  and nothing in the panel changed that. Its chip row now has a list like corruption's: **Sanctified
  only**, where it starts, **Unsanctified only** or **Sanctified or not**. Picking one searches again
  at once.

## [0.1.14] - 2026-10-12

### Added

- **Which unique it is, by its art.** An unidentified unique's text names only its base. Where the
  base has several uniques, the price check now tells which one it is by its art under the cursor
  and searches that unique's unidentified listings at once; the **Unique:** chip names it. When the
  art doesn't settle it (two uniques look alike, or something covers the item), the panel asks,
  likeliest first, and searches nothing until you answer. The chip's list still picks another one;
  it no longer has **any**, which searched every unique of the base together.

### Fixed

- **Weapons with chaos damage.** A weapon's chaos damage line went unread, so its total DPS
  counted only its physical and elemental damage, and the search for it went among far weaker
  weapons: Widowhail's 82 DPS was searched from 19. Total DPS now counts chaos damage, as the trade
  site does.
- **Pictures of uniques.** Prism of Belief and Blackflame showed art the game no longer draws,
  Loreweave, Runeseeker's Call and The Master's Reach showed none, and Reverie and Splinter of
  Lorrata were unknown, so they had no poe2db and wiki links either. They now show the art the trade
  site shows.

## [0.1.13] - 2026-10-12

### Changed

- **Rarity and corruption are lists.** The rarity and corruption chips open a list of every
  option: **Normal**, **Magic**, **Rare** or **Any Non-Unique**, and **Uncorrupted only**,
  **Corrupted only** or **Corrupted or not**. A click used to switch only between two, so a
  corrupted item couldn't be priced among uncorrupted listings. Picking an option searches again
  at once.

### Fixed

- **Hints over open lists.** The hint of the league, profile and unique selects showed up over the
  list the select had just opened, covering its first options. It now stays away while the list is
  open.

## [0.1.12] - 2026-10-12

### Added

- **Which unique an unidentified one is.** An unidentified unique names only its base, so it was
  priced among every unique of that base: an unidentified The Adorned among Megalomaniacs, from 1
  exalted where it starts at 1000 divine. Where a base has several uniques, the new chip
  **Unique** lists them: pick the one your item is, as its art shows, and the search finds only
  unidentified listings of that unique.

## [0.1.11] - 2026-10-11

### Fixed

- **A charm's duration and a jewel's mana on kill.** A charm's "increased Duration" was searched
  as a flask's, which no charm has, so a unique charm such as Forsaken Bangle found nothing. A
  jewel's "Recover #% of maximum Mana on Kill" was searched as the one other items carry, which no
  jewel has. Each is now searched as the trade site files it for that kind of item.

## [0.1.10] - 2026-10-11

### Fixed

- **Fractured, desecrated and crafted modifiers.** Their rows found only listings with the same
  modifier fractured, desecrated or crafted too, which left most offers out. They now find the
  modifier whatever its kind, as the trade site's own explicit filter does. The **Crafting base**
  profile still searches a fractured modifier as fractured, which is what a base is bought for.
  Hover the row's label to see which.
- **A corrupted unique's changed value.** A unique modifier that corruption moved off its fixed
  value, such as Adonia's Ego's "+2(3) to Level of all Spell Skills", had no row; it has one now,
  searched from the item's own value.
- **Cultivated uniques.** A unique changed by a Vaal Cultivation Orb and one it didn't change were
  priced together, though their modifiers and prices differ: an uncultivated The Adorned was priced
  among cultivated ones, at a few divines instead of hundreds. Each is now searched among its own
  kind.

## [0.1.9] - 2026-10-11

### Fixed

- **Item level of high-level items.** An item above item level 82 (81 for wands and staves) had
  its item level row read 82, as an item level 83 sceptre's did. The row now shows the item's own
  level. The search still starts at 82, as no modifier on such items needs a higher item level, and
  the minimum's box says so on hover.

## [0.1.8] - 2026-10-06

### Added

- **Quality on every item.** A weapon, armour piece, ring or amulet with quality now lists its
  quality among the price panel's filters, unchecked: tick it to find items with at least as much.
  That includes a catalyst's quality on jewellery, such as "Quality (Attack Modifiers): +62%". The
  trade site's quality filter can't tell the kinds of quality apart, so it finds any kind.

### Fixed

- **Critical hit chance keeps its decimals.** A weapon with 12.52% critical hit chance showed and
  searched 12%; now it's 12.52%.
- **Physical DPS on weapons with elemental damage.** A weapon whose physical damage was under two
  thirds of its total, like a spear with added lightning damage, hid its **Physical DPS** filter.
  Now it's listed whenever physical damage is at least 15% of the total, as elemental DPS already
  was.
- **Links in What's new.** An address in a change no longer breaks in the middle across two lines:
  links read as words and open in your browser.

## [0.1.7] - 2026-10-06

### Added

- **A click outside closes the panel, if you want.** A new setting, **Close on a click outside** in
  the settings' **Price check** section, lets a click anywhere in the game outside the price panel
  close it, as Esc does; the click itself still reaches the game. It's off by default, so the panel
  still closes only with Esc or its ×.
- **What's new in the settings.** A new last section of the settings, **What's new**, lists every
  version's changes, newest first, in the app's language, and marks the version you have.
- **A scrollbar in the settings.** A section longer than the window shows a scrollbar on its right,
  so you can see there is more below and drag it.

## [0.1.6] - 2026-10-01

### Fixed

- **The XP plate knows your level again after a restart.** If a character had gained levels lately,
  the plate showed "next level in" instead of the level after you restarted the app or logged in.
  Now the level shows right away. The app also saves the XP bar more often, so even a sudden
  shutdown hardly throws it off. And if the plate still shows "next level in", the level appears as
  soon as you reach the next one.

## [0.1.5] - 2026-09-30

### Added

- **Update automatically, right on the installer's first page.** You can turn it off
  there, before the app ever starts, and it stays off until you turn it back on in the
  settings. With it off the app stays offline and never updates itself.

## [0.1.4] - 2026-09-30

### Changed

- **Starts, installs and updates are counted, as numbers only.** While **Update
  automatically** is on, the app tells oracle.pushka.biz when it starts, whether this is a
  fresh install, and your interface language, so these can be counted. No name, address or
  id is kept, and with **Update automatically** off nothing is sent. The installer's
  privacy page and the guide say what is counted.

### Fixed

- **The XP plate shows your level again after a restart or a login.** After a restart or a
  login the plate often didn't know your level and showed "next level in" until your next
  level-up, which at high levels takes days. Now it works out your level on its own and shows
  it right away. If it can't tell which character you're on, it still shows "next level in"
  until your next level-up.

## [0.1.3] - 2026-09-29

### Fixed

- **A light on the game's XP bar no longer inflates your levelling speed.** A warm light in
  the scene could make the overlay misread the empty part of the bar as filled and count the
  difference as experience: a player at level 93 saw +186%/h where the real rate was about
  +40%/h. The overlay now reads the bar correctly through such light, and leaves a reading it
  isn't sure of alone.

## [0.1.2] - 2026-09-29

### Fixed

- **Automatic updates no longer give up on a slow download.** If the installer was slow to
  start arriving, while an antivirus checked it, say, or on a slow connection, the app
  abandoned the update and kept retrying with the same result. Now it waits. Versions 0.1.0
  and 0.1.1 still give up: if yours keeps saying "Couldn't update to version 0.1.2" in its
  settings, download this version from [oracle.pushka.biz](https://oracle.pushka.biz/) and
  install it once by hand.

## [0.1.1] - 2026-09-28

### Added

- **Extra logging for the XP overlay,** to help track down a wrong levelling speed. How to
  turn it on is in CONTRIBUTING.

### Fixed

- **Resting the pointer on the game's XP bar no longer inflates your levelling speed.**
  Holding the pointer over the bar for its tooltip made the overlay read it as nearly full
  and count that as experience: a player at level 93 saw +250-350%/h for half an hour. The
  overlay now ignores the bar while the pointer is on it.
- **Other brief misreadings of the bar no longer count as experience.** A sudden jump or
  drop now has to hold for a moment before it counts, so a one-off misread that corrects
  itself changes nothing. A level-up the game reports still counts at once.

## [0.1.0] - 2026-09-27

The first public release, on [oracle.pushka.biz](https://oracle.pushka.biz/). The installer isn't
digitally signed yet, so Windows may say "Windows protected your PC": click **More info**, then
**Run anyway**.

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
  [form on the site](https://oracle.pushka.biz/report.html) takes a problem or an idea.
- **Installer** for your Windows user, with no administrator rights needed: a Start menu shortcut
  and, if you like, a start with Windows. Its first page says what the app sends over the internet
  and links the privacy policy. Uninstalling keeps your settings unless you tick **Settings and
  cache**.

<!-- ANCHOR_END: releases. The version links stay below it, off that page: mdBook can't draw a heading that is a link. -->
[0.1.19]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.19
[0.1.18]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.18
[0.1.17]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.17
[0.1.16]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.16
[0.1.15]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.15
[0.1.14]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.14
[0.1.13]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.13
[0.1.12]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.12
[0.1.11]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.11
[0.1.10]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.10
[0.1.9]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.9
[0.1.8]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.8
[0.1.7]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.7
[0.1.6]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.6
[0.1.5]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.5
[0.1.4]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.4
[0.1.3]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.3
[0.1.2]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.2
[0.1.1]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.1
[0.1.0]: https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.0
