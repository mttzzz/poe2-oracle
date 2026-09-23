# Troubleshooting and FAQ

## The hotkey does nothing

Go through these in order.

1. **Is PoE2 Oracle running?** Its icon must be in the notification area by the clock (maybe under
   the **^** arrow). If not, start it from the Start menu.
2. **Is the game the window in front?** The hotkey works only while the game or the price panel is
   active. Click into the game once and try again. If you changed the hotkey, hover the tray icon
   to see the current one.
3. **Does another program hold <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd>?** PoE2 Oracle checks
   prices by pressing the game's own copy shortcut. If a graphics card overlay, a screen recorder
   or Discord has taken that combination, the game never receives it. The settings window then
   shows at the top: "Ctrl+Alt+C, which the game copies items with, is taken by another program…",
   and after a failed check the panel says "The game doesn't copy the item: another program takes
   Ctrl+Alt+C…". Open that program's hotkey settings and change or switch off
   <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd>.
4. **Does another program hold your hotkey?** If some program already uses
   <kbd>Ctrl</kbd>+<kbd>E</kbd>, PoE2 Oracle cannot take it. Pick another hotkey in the
   [settings](settings.md#hotkey).
5. **Does the game run as administrator?** Windows does not let a program without administrator
   rights send keys to one that runs as administrator, and PoE2 Oracle cannot detect this. Start
   the game normally, without "Run as administrator", or run PoE2 Oracle as administrator too.
6. **Is the game in exclusive Fullscreen?** Then the check may run, but Windows shows nothing over
   the game. Set the game's display mode to Windowed Fullscreen. The settings window warns about
   this: "The game runs in “Fullscreen” mode: the panel can't show over it. …".

With no item under the cursor, pressing the hotkey does nothing at all; that is expected.

## The panel opens but shows no prices

| The panel says | What to do |
|---|---|
| "Loading trade site data…" | The first start downloads the trade site's data. Wait a few seconds. |
| "No data from the trade site — no internet connection, or the site is down. Retrying automatically." | Check that pathofexile.com opens in your browser and that your firewall or antivirus lets PoE2 Oracle connect. The app tries again by itself, and again every time you press the hotkey. |
| "Nothing found" | No listing matches. The buttons under the message search more broadly, one trade search each: **Broad −10%** searches the same ticked rows with each **min** 10% below your roll, **Match N of M** finds listings with all the ticked rows but one. You can also untick some filter rows, lower some **min** values, switch **Base: …** to **Class: …** or let other sellers in with the **Sellers:** chip. See [When nothing matches exactly](price-check.md#when-nothing-matches-exactly). |

## "No connection" message

"Can't reach the trade site — check your internet connection and try again." means the trade site
could not be reached at all. Check your internet connection, whether the Path of Exile website is
up (it may be down for maintenance) and whether a firewall, VPN or proxy blocks PoE2 Oracle. Then
search again: click **Search** or press <kbd>Enter</kbd> in a filter box.

## Request limit messages

- "Trade API request limit — waiting 5s…" is normal: the trade site's limit is close, and the app
  waits a few seconds before searching.
- "The trade site has limited searches for a while — try again in 9m 50s." means the trade site
  has locked searches from your IP address for a while, often for minutes. PoE2 Oracle sends no
  trade request until then. Wait that long, then search again; Currency Exchange prices keep
  working.

The trade site counts every request from your IP address: the site in your browser and other trade
tools use the same limits. Many checks in a row, or several tools at once, run into them sooner.
See [Request limits](price-check.md#request-limits).

## An item is not recognised

When the item text cannot be read, the panel says "Couldn't read the item" with the reason, "The
item's text was saved to …" with the file's path, and a button **Report a problem**.

- "…only the English and Russian game clients are supported": the item text is in another
  language, or not in the one chosen as **Game client language** in the
  [settings](settings.md#game-client-language).
- "…unknown item class “…”": the app does not know this item class yet.

Texts are saved to `%LOCALAPPDATA%\poe2-oracle\data\unparsed`; the last 100 are kept. The app also
saves there, without a message, items it read only partly: a modifier line it could not match
simply has no filter row.

Please report such items with the button **Report a problem** under the message, or, when an item
was read but got wrong filters or a price that looks wrong, with the link **report a problem ↗**
under the item's name. Both open
the [item problem form](https://github.com/mttzzz/poe2-oracle/issues/new?template=item_problem.yml)
in your browser with the item's text, the app's version and your client's language filled in.
Describe what went wrong and submit it; you need a GitHub account. You can also fill the form by
hand: copy the item in the game with <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd> and paste it.

## Collecting a diagnostics report

1. Open the [settings](settings.md#help), section **Help**.
2. In the row **Report a bug**, click **Collect report**.
3. A file `PoE2-Oracle-report-<date>_<time>.zip` appears on your desktop and Explorer opens with it
   selected. Under the row, the settings window says "Saved: … — attach it to your bug report".

The zip holds plain text files:

- `summary.txt`: the app's version, league and state (catalogs, prices, hotkey, the last item
  checked), your Windows version, monitors, the game window's size, and the display mode, language
  and copy key from the game's settings file, plus the names and sizes of the cached files;
- `logs/`: the log of this run and of the one before;
- `settings.json`: your settings;
- `unparsed/`: the saved item texts.

Everywhere in it, your Windows user name, if it has three characters or more, is replaced with
`%USERNAME%`, and the paths of your user folder and of your Desktop, Documents and AppData folders,
wherever Windows keeps them, with `%USERPROFILE%`, `%DESKTOP%`, `%DOCUMENTS%`, `%APPDATA%` and
`%LOCALAPPDATA%`. Nothing is sent anywhere: you attach the file yourself, and you can open it and
read it first.

**Logs folder** → **Open** in the same section opens `%LOCALAPPDATA%\poe2-oracle\data\logs`, with
`poe2-oracle.log` for the current run and `poe2-oracle.previous.log` for the one before.

## Reporting a bug

Click **Report a bug** in the tray icon's menu, or **Report ↗** in the settings, section **Help**.
PoE2 Oracle then:

1. writes the diagnostics report to your desktop and shows it in Explorer;
2. opens GitHub's [bug report form](https://github.com/mttzzz/poe2-oracle/issues/new?template=bug_report.yml)
   in your browser, with the app's version, your client's language (when known) and a note naming
   the report already filled in.

Describe what happened and how to repeat it, drag the zip from Explorer into the form and submit
it from your GitHub account. PoE2 Oracle itself sends nothing: you see the whole form before it
goes anywhere.

Windows **Settings → Apps → Installed apps** shows the installed version under PoE2 Oracle; the
first line of `summary.txt` in the report has it too.

## Questions

**Does it work in exclusive Fullscreen?**
No. Windows does not show other programs over exclusive fullscreen. Use Windowed Fullscreen.

**Which game languages are supported?**
The English and the Russian client. Other client languages are not read.

**Which language is the interface in?**
English or Russian, as set in the [settings](settings.md#interface-language), section **General**,
row **Interface language**. On **Auto**, the default, it follows the game client's language, or
the Windows display language before the game has ever run. Only PoE2 Oracle's own words change:
item names, modifiers and the trade site's texts keep their own language.

**Does it need my account, password or session cookie?**
No. PoE2 Oracle prices items like a visitor who is not signed in. Signing in is optional: it opens
private leagues, live search and the "sum" rows. You sign in on pathofexile.com's own page in a
window of the app; PoE2 Oracle doesn't read or keep your password, only the site's session, in
Windows Credential Manager. See [Account](settings.md#account) and [Privacy](privacy.md).

**Does it play for me?**
No. Each hotkey press does one thing: it copies one item, sends one chat message or pastes one
search. Whispers to sellers are only copied; you send them yourself.

**Where are my settings? How do I start over?**
In `%APPDATA%\poe2-oracle\config\settings.json`. Quit PoE2 Oracle from the tray, delete the file,
and the next start begins with the defaults and the guided tour.
