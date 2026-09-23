# PoE2 Oracle

**English** · [Русский](README.ru.md)

PoE2 Oracle is a price checker for Path of Exile 2 on Windows. Hover an item in the game and
press `Ctrl+E`: a panel opens next to your inventory with the item's stats as search filters and
the cheapest current listings from the official trade site. Currency and other Currency Exchange
items are priced from GGG's own hourly record of the trades made on the exchange; poe2scout adds a
chart of the week and prices uniques and exchange items that haven't traded lately. It is a native
Windows program written in Rust with
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), without Electron or Overwolf;
only signing in to pathofexile.com opens a page in Windows' own Edge component (WebView2).

The game has had a price check of its own since patch 0.5 (<kbd>Shift</kbd>+<kbd>Alt</kbd>+click):
it opens the in-game market, in town or your hideout, with every modifier of the item as a filter.
PoE2 Oracle works over the game anywhere, maps included, picks the modifiers that decide the price
for you, prices currency from the exchange's own trade record, and adds an XP rate line and a few
quick chat actions.

The app's interface is in English and Russian: by default it follows the game client's language,
and the settings can switch it. The game client can be English or Russian. A short tour shows the
basics on the first launch.

![The price panel open next to the game inventory](docs/guide/src/images/hero.png)

## Features

- **Price check.** The panel shows the item's name and art with links to poe2db, the wiki and a
  problem report for this item, one row per stat with its min/max values and mod tier, and the
  cheapest listings on the trade site (one page of 10 per search): price, item level, seller and
  how long ago it was listed. Searches go to www.pathofexile.com or ru.pathofexile.com, matching
  the language of the item. `Esc` closes the panel.
- **Search profiles** set up which stats are searched and how far below your rolls, as PoE Overlay
  II's do: **Quick price** (up to four of the most valuable stats, picked by PoE Overlay II's
  scoring of tier, tags and roll), **Exact match**, **Broad −10%** and **Crafting base**. A
  modifier row's slider spans its rolls across all tiers, and its tier badge tells the item level
  the tier needs and the best tier the item's level allows. When nothing is found, the panel offers
  a broader search instead of spending the trade site's limit on its own.
- **Search chips** narrow or widen the search with one click: the item's base type or its whole
  class, its rarity (a magic item is compared with magic items, a rare with every non-unique item),
  corrupted listings in or out, every stat or none, which sellers to include (instant buyout only by
  default), and the currency of the price.
- **Whispers.** Click a listing whose seller trades in person and the trade site's whisper is copied;
  paste it into the game chat.
- **Currency Exchange items** (currency, omens, runes, essences and the like) get a market card from
  GGG's own record of the trades made on the exchange: the value in divine, exalted and chaos orbs
  for the last complete hour (up to three for a rarely traded item; the card names the hours),
  poe2scout's chart of the week, the hourly volume and what the copied stack is worth. When an item
  hasn't traded in your league in the last hours (common in small leagues such as Standard), the
  panel shows poe2scout's price instead and searches the trade site only when you press
  **Trade site listings**, since every search counts against the trade site's limit. Without a
  poe2scout price either, the trade site is searched right away.
- **Waystones.** Click the ◇ at the end of a modifier to mark it as danger, caution or wanted;
  the marks are remembered and highlighted on every waystone you check.
- **Vendor gamble offers** are recognised: the panel says the item is only revealed after buying
  instead of searching for it.
- **XP overlay** above the experience bar: how fast you level (percent of a level per hour) and the
  time to the next level, optionally the level percentage and a timer for the current map.
- **Quick actions:** your own hotkeys that type a chat command (`/hideout`, `@last thanks`) or a
  stash search string (for example one made with poe2.re).
- **Updates** from the tray menu: the new installer is downloaded from GitHub Releases, checked
  against its published SHA-256 sum and installed; the app restarts by itself.

## Requirements

- Windows 10 or 11, 64-bit.
- Path of Exile 2 in **Windowed Fullscreen** or **Windowed** mode. The panel can't be shown over
  exclusive Fullscreen; the app's settings window warns you about it.
- An English or Russian game client.
- Internet access to pathofexile.com, web.poecdn.com and poe2scout.

## Install

1. Download `PoE2-Oracle-Setup-<version>.exe` from the
   [latest release](https://github.com/mttzzz/poe2-oracle/releases/latest).
2. Run it. It installs for your Windows user only, without administrator rights, into
   `%LOCALAPPDATA%\Programs\PoE2 Oracle` and adds a Start menu shortcut. The last page offers to
   start the app and to start it with Windows.
3. The installer isn't code-signed yet, so Windows SmartScreen may show "Windows protected your
   PC". Click **More info**, then **Run anyway**. To check that you have the published file, run
   `Get-FileHash .\PoE2-Oracle-Setup-<version>.exe` in PowerShell and compare the result with the
   `SHA256SUMS` file of the same release.

To uninstall, open Windows Settings → Apps → Installed apps (Apps & features on Windows 10) →
PoE2 Oracle → Uninstall. Your settings and downloaded price data are kept unless you tick
**Settings and cache**.

## First run

1. A short guided tour starts in the settings window, at the league, and walks you through a first
   price check, the price panel and the XP overlay; skip it any time, and replay it from Settings →
   Help → **Tutorial**. The app runs in the background: its icon is in the notification area next
   to the clock, sometimes behind the "Show hidden icons" arrow. Its menu has **Settings**,
   **Check for updates**, **Report a bug** and **Quit**.
2. In the game's graphics options, set the display mode to Windowed Fullscreen.
3. Hover an item in the game and press `Ctrl+E`. Right after the first launch the app spends a few
   seconds downloading the trade site's catalogs; until then the panel says "Loading trade site
   data…".

Good to know:

- The hotkeys work only while the game (or the price panel) is in front, so `Ctrl+E` stays free in
  other programs. You can change it in the settings.
- To read an item, the app presses the game's own advanced copy, `Ctrl+Alt+C`, and puts your
  clipboard back afterwards. If another program (a graphics card overlay, a screen recorder,
  Discord) has taken that combination, price checks can't work; the settings window tells you.
- One copy runs at a time. Starting it again opens the settings.

## User guide

The full guide, with every setting explained, is at
**[mttzzz.github.io/poe2-oracle/guide](https://mttzzz.github.io/poe2-oracle/guide/)**:
[introduction](https://mttzzz.github.io/poe2-oracle/guide/en/introduction.html),
[troubleshooting](https://mttzzz.github.io/poe2-oracle/guide/en/troubleshooting.html) and
[privacy](https://mttzzz.github.io/poe2-oracle/guide/en/privacy.html). The project site is
[mttzzz.github.io/poe2-oracle](https://mttzzz.github.io/poe2-oracle/).

## Reporting a problem

The quickest way is from the app itself. It fills in the form for you, and you submit it from your
own GitHub account; the app sends nothing by itself.

- **Report a bug** in the tray menu, or **Report ↗** in the **Help** section of the settings, saves
  a diagnostics report to your desktop (a zip with the logs and settings, your Windows user name
  masked), shows it in Explorer and opens the bug form with the version, the client language and
  the report's file name filled in. Drag the zip into the form.
- **report a problem ↗** under the item name on the price panel, or **Report to the developer** in
  the message about an item the app couldn't read, opens the item form with the item text filled
  in.

You can also open a form yourself:

- [Report a bug](https://github.com/mttzzz/poe2-oracle/issues/new?template=bug_report.yml)
- [An item isn't recognised, or gets wrong filters or a wrong price](https://github.com/mttzzz/poe2-oracle/issues/new?template=item_problem.yml)
- [Suggest a feature](https://github.com/mttzzz/poe2-oracle/issues/new?template=feature_request.yml)

Please search the [existing issues](https://github.com/mttzzz/poe2-oracle/issues) first. Mention
the app version (Windows Settings → Apps → Installed apps shows it) and your game client language.
For an item problem, paste the item text: hover the item in the game, press `Ctrl+Alt+C`, then
paste into the form. Never post passwords or session cookies. Security problems go through private
reporting, see [SECURITY.md](SECURITY.md).

If searches stop with "The trade site has limited searches for a while", you have hit the trade
site's limit on requests from one IP address, which is shared with the trade site open in your
browser. After the site refuses a request, the app sends it nothing until the lockout ends: wait
for the time the panel shows and try again. Currency Exchange prices keep working meanwhile.

## Privacy

PoE2 Oracle has no telemetry, no analytics and no accounts of its own. It connects to:

| Where | What for |
|---|---|
| www.pathofexile.com, ru.pathofexile.com | The trade site's API: leagues, stat and item catalogs, your searches (the item's stats) and the listings they find |
| api.poe2scout.com | Prices of uniques; the week's prices and pages of Currency Exchange items, and prices of those that haven't traded in your league lately |
| web.poecdn.com | Item images, and GGG's hourly record of the trades made on the Currency Exchange (one file per hour for all leagues, the same for everyone) |
| api.github.com, github.com | The update check (30 seconds after start while update checks are on, or from the tray menu) and the installer download |

Everything else stays on your computer. The app reads the item text the game copies, the game's
own log (`Client.txt`: level-ups, area changes), the game's settings file and the screen
pixels of the experience bar itself. It keeps its files here:

| What | Where |
|---|---|
| Settings | `%APPDATA%\poe2-oracle\config\settings.json` |
| Logs of this run and the previous one | `%LOCALAPPDATA%\poe2-oracle\data\logs` |
| Item texts it couldn't read | `%LOCALAPPDATA%\poe2-oracle\data\unparsed` |
| Downloaded catalogs, prices and updates | `%LOCALAPPDATA%\poe2-oracle\cache` |

The diagnostics report is made only when you ask for it, and the app never sends it anywhere:
reporting a bug only opens GitHub's form in your browser, and you decide what to submit. Links on
the panel (poe2db, the wiki, poe2scout, the trade site) open in your browser too.

## Contributing

Bug reports, item texts that the app gets wrong and pull requests are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) for the rules and how to build the app, and
[CHANGELOG.md](CHANGELOG.md) for what changed between versions.

## License and disclaimer

PoE2 Oracle is licensed under either of the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option. It includes item and stat data derived from
[Exiled Exchange 2](https://github.com/Kvan7/Exiled-Exchange-2) (MIT) and the Philosopher and
Alegreya SC fonts (SIL Open Font License 1.1); the installer puts the full third-party notices
next to the app as `THIRD-PARTY-NOTICES.html`.

PoE2 Oracle is a fan-made tool. This product isn't affiliated with or endorsed by Grinding Gear
Games in any way. Path of Exile is a trademark of Grinding Gear Games.
