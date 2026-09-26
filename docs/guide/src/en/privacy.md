# Privacy

PoE2 Oracle has no account of its own and sends no telemetry or analytics. It talks only to the
services below: for prices and pictures, for updates and, only when you send one yourself, for a
report to the developer. Signing in to pathofexile.com is optional; signed in, the app searches the
trade site with your session (see [Signing in to pathofexile.com](#signing-in-to-pathofexilecom)).

## What is sent where

| Service | When | What is sent |
|---|---|---|
| Path of Exile trade site: `www.pathofexile.com` and `ru.pathofexile.com` | The list of leagues and both sites' catalogs (modifiers, exchange items, item bases): at start and while running, when the saved copy is older than an hour (leagues) or six hours (catalogs). On a price check, unless the Currency Exchange prices the item: the search, then the found listings; the same search again within two minutes reuses them. An exchange item is searched only when neither GGG's record nor poe2scout prices it, or when you ask for its listings. Signed in, also the site's account page, to check the session: at start, when you sign in and when the trade site refuses a **sum** row; and, on `www.pathofexile.com`, your Private Leagues page and the page of each of your PoE 2 private leagues: once the site accepts the session (or doesn't answer its check), each time you open the settings, when you click **Refresh** in **Account**, and by themselves every hour by default (**Refresh automatically**: 15 minutes, 1 hour, 6 hours or off). | For catalogs and pages, nothing but the request. For a check, the search itself: league, item class or base, the selected filter rows with their bounds, the sellers and price choices. Items from a Russian client are searched on `ru.pathofexile.com`, from an English one on `www.pathofexile.com`. |
| `api.poe2scout.com` | At start, when you change the league, and after a check, when the saved prices are older than 30 minutes. | The league, to get the prices of unique items, the Currency Exchange's last seven days and item pages, and the prices of exchange items not traded lately. |
| `web.poecdn.com` (the game's server for pictures and exchange data) | When the panel shows an item picture or a currency icon. At start, when you change the league, and during checks at most every 10 minutes: each complete hour of GGG's record of the Currency Exchange's trades not downloaded yet. | Requests for those pictures, and for the hours of exchange data, which are the same for every league and player: nothing about you or your league. |
| `oracle.pushka.biz`, PoE2 Oracle's own service | Updates, while **Update automatically** is on (the default): a connection opened about 10 seconds after start and kept open for as long as the app runs, opened again when it drops; downloads when a newer version or newer game data is out. Reports: only when you click **Send** in the [report window](report.md). | Updates: the connection's request, which carries no cookie, account or id, only the app's User-Agent with its version; over it the service sends the latest app and game data versions, and a keep-alive line every 25 seconds. When something newer is out: a request for the latest release or game data pack, then the installer or the pack, its `SHA256SUMS` and that file's signature, which the service passes on from the project's private GitHub repository. Reports: see [Reports](#reports). |

Signed out, no request for prices, pictures or updates carries your account, a cookie or anything
else that identifies you; a report carries what [Reports](#reports) lists, and only when you send
it. Signed in, the requests to the trade site carry your pathofexile.com session, the cookie
`POESESSID`; no other request does. Each request carries a fixed User-Agent string that is the same
for every player: a common web browser's for the Path of Exile sites, poe2scout and the game's
picture and exchange server, and `PoE2-Oracle/<version>` for oracle.pushka.biz. As with any
request on the internet, each service sees your IP address.

While **Update automatically** is on, the connection to oracle.pushka.biz stays open, so the
service sees your IP address, and from the User-Agent the app's version, for as long as the app
runs. Turn it off in the settings, section **General**, group **Updates**, and PoE2 Oracle doesn't
connect to oracle.pushka.biz at all, save for a report you send.

The links on the panel (**poe2db ↗**, **wiki ↗**, **Craft of Exile ↗**, **poe2scout ↗**,
**…/trade ↗**) open in your browser when you click them: poe2db.tw, www.poe2wiki.net,
beta.craftofexile.com, poe2scout.com and the trade site. The Craft of Exile link carries the item
in its address: its base, item level, rarity and modifiers. PoE2 Oracle itself never contacts
poe2db, the wiki or Craft of Exile.

## Reports

Nothing goes to the developer until you click **Send** in the [report window](report.md) or on the
site's [report form](../../report.html).

A report from the app carries your text, the contact if you gave one, the app's version, the
interface and client languages, your Windows version, the league and the interface scale, and what
you attached: the item's name and text (**Item**), what PoE2 Oracle reported when it closed
(**Crash**), and the [diagnostics report](troubleshooting.md#collecting-a-diagnostics-report) while
**Attach diagnostics** is on. A report from the site's form carries only your text and the
contact.

oracle.pushka.biz passes each report on to the developer and keeps none of it itself:

- as an issue in the project's private GitHub repository, with everything above but the
  diagnostics report;
- as a Telegram message to the developer, with the diagnostics report and the item's or the
  crash's text as files.

Only the developer reads them. You need no account, on GitHub or anywhere else.

Your IP address is used only for limits: how many reports come from one address, and how many
update connections one address holds at once (8). It is not logged. A report count is forgotten
when its time window ends, at the latest at the end of the day, Moscow time; a connection's, when
the connection closes. The service's logs note each report's kind, whether it came from the app or
the site, the app's version, its issue number and the size of its diagnostics report, never its
text or the contact. It also counts, per day, the installers downloaded, the update
connections opened, the update checks it answered, the game data packs and other update files
downloaded, and the reports of each kind: numbers only, with no address, id or version, kept for
120 days.

## Signing in to pathofexile.com

Signing in is needed only for private leagues and the **sum** rows among the filters (see
[Account](settings.md#account)). **Sign in** opens pathofexile.com's own sign-in page in a
window of the app, a Microsoft Edge WebView2 browser, where you sign in as on the site: with your
password, or through Steam, PlayStation or Xbox. PoE2 Oracle never sees your password. It takes
only the session the site gives that browser, the cookie `POESESSID`, and checks it on the site's
account page.

- **What is kept.** The session alone, in Windows' Credential Manager, as the generic credential
  `PoE2 Oracle/pathofexile.com` (user name `POESESSID`). Windows keeps it encrypted for your Windows
  user on this computer, and it never goes to another one. It is not in the settings file, the logs
  or the diagnostics report.
- **Where it goes.** In the `Cookie` of the requests to `https://www.pathofexile.com` and
  `https://ru.pathofexile.com` (searches, listings, leagues, catalogs, the account page, your
  private leagues' pages). Never to poe2scout, the game's picture and exchange server or
  oracle.pushka.biz, and never in a report.
- **The sign-in window.** It opens https pages only, and its title names the site of the page it
  shows: signing in through Steam, PlayStation or Xbox goes through their pages. Its browser runs
  in private mode, in a folder of its own, `%LOCALAPPDATA%\poe2-oracle\data\login-browser`, which
  is deleted once the window has closed, or at the next sign-in or start if something interrupted
  that.
- **Removing it.** **Sign out** in the settings, section **Account**, deletes the session from
  Credential Manager, and PoE2 Oracle no longer sends it. Uninstalling deletes it too, whatever you
  tick. You can also delete it yourself: **Control Panel → Credential Manager → Windows
  Credentials**, entry `PoE2 Oracle/pathofexile.com`.

## What it reads on your computer

- **The clipboard**, only during a price check or a quick action. A check reads the item text the
  game copied and then puts back what you had copied before. A quick action puts its text on the
  clipboard for a moment, marked to stay out of Windows' clipboard history, and then restores
  yours. What was copied as private, such as a password from a password manager, is not put back:
  the clipboard is left empty instead, and you copy the password again when you need it.
- **The game's settings file**, `Documents\My Games\Path of Exile 2\poe2_production_Config.ini`:
  the display mode, the client language (which the **Auto** interface language follows) and the key
  for advanced item descriptions. Read only.
- **The Windows display language**, only to pick the interface language on **Auto** before the game
  has ever run.
- **The game's log**, `Client.txt` in the game's `logs` folder: level-ups, area changes and returns
  to character selection for the [XP overlay](xp-overlay.md). Read only.
- **The screen**, for the XP overlay: the strip of pixels where the experience bar is, and a few
  rows along the top of the flask and skill panels' rails, where the plates stand. While the game
  is in front, and for two seconds after you switch away, the rails are looked at many times a
  second, so that a plate steps aside the moment a tooltip covers its rail, and the bar twice a
  second; while the game is behind another window, both every two seconds; while it is minimised,
  not at all. Nothing of it is saved or sent.

## What it types into the game

Only when you press one of its hotkeys: the game's item-copy shortcut for a price check, or the text
of a [quick action](quick-actions.md).

## What it keeps on your computer

| What | Where |
|---|---|
| Settings, with waystone marks and quick actions | `%APPDATA%\poe2-oracle\config\settings.json` |
| Logs of the current and the previous run | `%LOCALAPPDATA%\poe2-oracle\data\logs` |
| Item texts the app could not fully read, the last 100 | `%LOCALAPPDATA%\poe2-oracle\data\unparsed` |
| Trade site catalogs, exchange and poe2scout prices, downloaded updates | `%LOCALAPPDATA%\poe2-oracle\cache` |
| After a crash, what PoE2 Oracle reported, until you send that report or close its window | `%LOCALAPPDATA%\poe2-oracle\data\crash\last-crash.txt` |
| After an update, which one it was (from which version to which), until the next start has said so | `%LOCALAPPDATA%\poe2-oracle\data\last-update.json` |
| The installed game data pack, and the last damaged one set aside | `%LOCALAPPDATA%\poe2-oracle\data\game-data` |
| Your pathofexile.com session, while you are signed in | Windows' Credential Manager: `PoE2 Oracle/pathofexile.com` |
| The sign-in window's browser, while the window is open | `%LOCALAPPDATA%\poe2-oracle\data\login-browser` |
| The start-with-Windows entry, when turned on | Registry: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `PoE2 Oracle` |

The logs note what the app did, such as the name and base of each checked item and how many
listings were found; they do not keep the full item text.

The [diagnostics report](troubleshooting.md#collecting-a-diagnostics-report) is written only when
you ask for it: **Collect report** in the settings and **What's inside** in the report window save
it on your desktop, for you to open and to send wherever you decide; a report you send with
**Attach diagnostics** on takes it to the developer (see [Reports](#reports)). In it, and in what a
crash left, the paths of your user folder and of your Desktop, Documents and AppData folders,
wherever Windows keeps them, are replaced by `%USERPROFILE%`, `%DESKTOP%`, `%DOCUMENTS%`,
`%APPDATA%` and `%LOCALAPPDATA%`, and your Windows user name, if it has three characters or more,
by `%USERNAME%` wherever else it stands as a word of its own.

Uninstalling always deletes the saved session and the start-with-Windows entry; with **Settings and
cache** ticked, it removes everything in the table above. Reports saved on your desktop stay until
you delete them. See [Updates and uninstall](updates.md#uninstall).
