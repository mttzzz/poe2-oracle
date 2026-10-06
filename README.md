# PoE2 Oracle

**English** · [Русский](README.ru.md)

A free price checker and XP tracker for Path of Exile 2 on Windows. Point at an item in the game
and press `Ctrl+E`: a panel next to your inventory shows what similar items sell for on the
official trade site. An XP overlay right on the game's HUD shows how fast you level, how long until
the next level and how long the current map has run.

**[Download for Windows](https://oracle.pushka.biz/download/latest)** ·
[Website](https://oracle.pushka.biz/) · [Guide](https://oracle.pushka.biz/guide/en/introduction.html) ·
[Watch the demo on YouTube](https://youtu.be/7KKJsgSsi7o)

<img src="docs/guide/src/images/en/hero.webp" width="941" alt="The PoE2 Oracle price panel, a currency's market card and both XP overlay plates">

Free, with no ads and no account needed. PoE2 Oracle is a fan-made tool: this product isn't
affiliated with or endorsed by Grinding Gear Games in any way.

## What it does

- **Price check.** Point at an item in your inventory, your stash or a vendor's window and press
  `Ctrl+E`. The panel shows the cheapest similar items on the official trade site, with their
  price, seller and age. The item's modifiers become search filters: pick how strictly to search,
  from a quick price to an exact match, and adjust the numbers. `Esc` closes the panel.
- **Currency priced from real trades.** Currency, omens, runes, essences and everything else
  traded on the game's Currency Exchange are priced from the trades made on the exchange, which GGG
  publishes every hour, with a seven-day chart from poe2scout. The price shows at once, with no
  wait for the trade site. Unique items also show poe2scout's price.
- **Craft of Exile in one click.** A link on the panel opens the item in Craft of Exile's crafting
  simulator with its base, item level and modifiers already set, from the Russian client too.
- **Waystone marks.** Mark waystone modifiers as danger, caution or wanted, and every waystone you
  check shows them.
- **XP overlay.** Two small plates on the game's HUD, in its own frames and colours: how fast you
  level and when the next level comes, and a timer for the current map. They step aside when a game
  tooltip covers them, and the settings can turn them off.
- **Quick actions.** Keys of your choice that send a chat command such as `/hideout` or type a
  stash search: one press, one command.
- **English and Russian.** The app speaks both and reads items from both game clients.
- **Updates itself.** New versions and new item data install by themselves, and only checked ones:
  the app makes sure the developer made them and they arrived whole. An update waits while any of
  the app's windows is open. The settings can turn updates off.
- **Light.** One process of about 40 MB in Task Manager. In a test during real play on one PC, it
  used 4 to 7 times less CPU than POE2 Currency Overlay, PoE Overlay II and Exiled Exchange 2
  running next to it ([how it was measured](https://oracle.pushka.biz/guide/en/performance.html)).

Signing in to pathofexile.com is optional. It happens on the site's own page and adds private
leagues and the trade site's **To hideout** and **Whisper** buttons on listings. The app never sees
your password.

## What you need

- Windows 10 or 11, 64-bit. Nothing else to install first.
- Path of Exile 2 with the English or Russian client.
- The game in **Windowed Fullscreen** or **Windowed** mode. Over exclusive Fullscreen, Windows
  doesn't show other programs, so the panel would stay hidden.
- An internet connection.

## Install

1. [Download the installer](https://oracle.pushka.biz/download/latest) and run it. It installs for
   your Windows user only and needs no administrator rights.
2. The installer isn't digitally signed yet, so Windows may say "Windows protected your PC": click
   **More info**, then **Run anyway**. The
   [install guide](https://oracle.pushka.biz/guide/en/install.html#download) shows how to check
   that you have the published file.
   How releases are built and approved: [Code signing policy](https://oracle.pushka.biz/code-signing.html).
3. Keep **Run PoE2 Oracle** ticked on the installer's last page. A short tour then shows the
   basics: in the game, point at an item and press `Ctrl+E`.

The app runs in the background: you find it by its icon next to the clock. To uninstall it, open
Windows Settings → Apps → Installed apps → PoE2 Oracle → Uninstall.

**Official downloads.** PoE2 Oracle is only published at
https://github.com/mttzzz/poe2-oracle/releases, https://oracle.pushka.biz/ and, once merged, WinGet
(`winget install mttzzz.PoE2Oracle`). Copies elsewhere aren't mine.

## How it's made

English isn't my first language, so I write and code with AI help (Claude). I design it, test every
release in my own play, and fix what you report.

## Privacy

No account of its own, and nothing collected about you or your play: the app talks only to the Path
of Exile trade site, GGG's server of pictures and exchange data, poe2scout and oracle.pushka.biz,
for updates and the reports you send. oracle.pushka.biz counts installations, starts and updates as
anonymous numbers and keeps no address or id
([what goes where](https://oracle.pushka.biz/guide/en/privacy.html),
[what it counts](https://oracle.pushka.biz/guide/en/privacy.html#what-the-service-counts)). Untick
**Update automatically** on the installer's first page, or switch it off later in the settings, and
the app doesn't connect there at all, save for the reports you send.

## Found a problem?

- **In the app:** Settings → **Help** → **Write to the developer**. For an item, click **report a
  problem** under its name on the price panel, and the item's text goes along. After a crash, the
  report window opens by itself on the next start. No account is needed, the report goes to the
  developer alone, and a contact (Telegram, Discord or email) gets you an answer.
- **Without the app:** the form at
  [oracle.pushka.biz/report.html](https://oracle.pushka.biz/report.html).
- **On GitHub:** [open an issue](https://github.com/mttzzz/poe2-oracle/issues/new/choose). Issues
  are public, so leave out your contacts, passwords and session cookies.
- **A security problem:** see [SECURITY.md](SECURITY.md).

Common problems and their fixes are in the guide's
[troubleshooting](https://oracle.pushka.biz/guide/en/troubleshooting.html) page.

## For developers

PoE2 Oracle is written in Rust, with [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)
drawing its windows through Direct3D 11. [CONTRIBUTING.md](CONTRIBUTING.md) tells how the code is
laid out, how to build and test it and how to send a change; [CHANGELOG.md](CHANGELOG.md) lists
what changed in each version.

## License

PoE2 Oracle is licensed under either of the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option. It includes item data derived from
[Exiled Exchange 2](https://github.com/Kvan7/Exiled-Exchange-2) (MIT) and from the game's own mod
and base item tables as exported by [RePoE](https://repoe-fork.github.io/poe2/) (MIT; the data
belongs to Grinding Gear Games), and the Philosopher and Alegreya SC fonts (SIL Open Font License
1.1). The installer puts the full third-party notices next to the app, in
`THIRD-PARTY-NOTICES.html`.

PoE2 Oracle is a fan-made tool. This product isn't affiliated with or endorsed by Grinding Gear
Games in any way. Path of Exile is a trademark of Grinding Gear Games.
