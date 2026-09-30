# Privacy

PoE2 Oracle has no account of its own and no analytics of you: no id, no profile, nothing about
your play. Its own service counts installations, in numbers only (see
[What the service counts](#what-the-service-counts)). The app talks only to the services below: for
prices and pictures, for updates and, only when you send one yourself, for a report to the
developer. Signing in to pathofexile.com is optional; signed in, the app searches the trade site,
and presses its buttons on listings when you do, with your session (see
[Signing in to pathofexile.com](#signing-in-to-pathofexilecom)).

## What is sent where

| Service | When | What is sent |
|---|---|---|
| Path of Exile trade site: `www.pathofexile.com` and `ru.pathofexile.com` | The list of leagues and both sites' catalogs (modifiers, exchange items, item bases): at start and while running, when the saved copy is older than an hour (leagues) or six hours (catalogs). On a price check, unless the Currency Exchange prices the item: the search, then the found listings; the same search again within two minutes reuses them. An exchange item is searched only when neither GGG's record nor poe2scout prices it, or when you ask for its listings. Signed in, also the site's account page, to check the session: at start, when you sign in and when the trade site refuses a **sum** row; and, on `www.pathofexile.com`, your Private Leagues page and the page of each of your PoE 2 private leagues: once the site accepts the session (or doesn't answer its check), each time you open the settings, when you click **Refresh** in **Account**, and by themselves every hour by default (**Refresh automatically**: 15 minutes, 1 hour, 6 hours or off). Signed in, also each time you press **To hideout** or **Whisper** on a listing: one request per press. | For catalogs and pages, nothing but the request. For a check, the search itself: league, item class or base, the selected filter rows with their bounds, the sellers and price choices. For a press of **To hideout** or **Whisper**, the token the site gave with that listing, and after **In demand** that you go anyway. Items from a Russian client are searched on `ru.pathofexile.com`, from an English one on `www.pathofexile.com`. |
| `api.poe2scout.com` | At start, when you change the league, and after a check, when the saved prices are older than 30 minutes. | The league, to get the prices of unique items, the Currency Exchange's last seven days and item pages, and the prices of exchange items not traded lately. |
| `web.poecdn.com` (the game's server for pictures and exchange data) | When the panel shows an item picture or a currency icon. At start, when you change the league, and during checks at most every 10 minutes: each complete hour of GGG's record of the Currency Exchange's trades not downloaded yet. | Requests for those pictures, and for the hours of exchange data, which are the same for every league and player: nothing about you or your league. |
| `oracle.pushka.biz`, PoE2 Oracle's own service | Updates, while **Update automatically** is on (the default): a connection opened about 10 seconds after start and kept open for as long as the app runs, opened again when it drops; downloads when a newer version or newer game data is out. Reports: only when you click **Send** in the [report window](report.md). The site: when you open its pages in a browser. | Updates: the connection's request, which carries no cookie, account or id, only the app's User-Agent with its version. The first connection after each start also carries a few flags in its address, which the service counts (see [What the service counts](#what-the-service-counts)): `start=1` and, as they apply, `first=1` (this installation's first start), `from=<version>` (the version that ran last time, if it was another one), `lang=en` or `lang=ru` (the interface language) and `dev=1` (a build the developer made for testing); the reconnections after it carry none. Over the connection the service sends the latest app and game data versions, and a keep-alive line every 25 seconds. When something newer is out: a request for the latest release or game data pack, then the installer or the pack, its `SHA256SUMS` and that file's signature, which the service passes on from the project's private GitHub repository. Reports: see [Reports](#reports). The site: your browser's request for a page, as for any site; the link to the installer carries `?from=<tag>` when you came by a link tagged with where it was published (see [What the service counts](#what-the-service-counts)). |

Signed out, no request for prices, pictures or updates carries your account, a cookie or anything
else that identifies you; a report carries what [Reports](#reports) lists, and only when you send
it. Signed in, the requests to the trade site carry your pathofexile.com session, the cookie
`POESESSID`; no other request does. Each request carries the same fixed User-Agent string for every
player, `PoE2-Oracle/<version> (+https://oracle.pushka.biz)`: it names the app, as GGG asks of
tools that call its sites, and says nothing about you. As with any
request on the internet, each service sees your IP address.

While **Update automatically** is on, the connection to oracle.pushka.biz stays open, so the
service sees your IP address, and from the User-Agent the app's version, for as long as the app
runs; the first connection after each start adds the flags described in
[What the service counts](#what-the-service-counts). Turn it off in the settings, section
**General**, group **Updates**, or untick it on the installer's first page before the first start,
and PoE2 Oracle doesn't connect to oracle.pushka.biz at all, save for a report you send.

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

- as an issue in a private GitHub repository, kept apart from the app's source code, with
  everything above but the diagnostics report;
- as a Telegram message to the developer, with the diagnostics report and the item's or the
  crash's text as files.

Only the developer reads them. You need no account, on GitHub or anywhere else.

Apart from the counts in [What the service counts](#what-the-service-counts), your IP address is
used only for limits: how many reports come from one address, and how many update connections one
address holds at once (8). It is not logged. A report count is forgotten when its time window ends,
at the latest at the end of the day, Moscow time; a connection's, when the connection closes. The
service's logs note each report's kind, whether it came from the app or the site, the app's version,
its issue number and the size of its diagnostics report, never its text or the contact.

## What the service counts

oracle.pushka.biz counts, per day (Moscow time), what the app and the site's visitors do. It keeps
numbers only, for 120 days: no address, no id, no name, and no number can be traced back to a
person. The counts, and what they are made of:

- **The app's connections and update checks**, in total and by the version in its User-Agent, only
  while **Update automatically** is on (with it off the app doesn't connect at all). Only the
  versions of the newest published releases, sixteen at most, get a count of their own; any other
  text there counts as `other`.
- **How many different installations were active** in a day and in a week (Monday to Sunday), in
  total and by version. The service can't tell installations apart and doesn't try to; it tells
  *connections* apart. For each one it makes a hash (SHA-256) of the connection's address and the
  User-Agent's product name (`PoE2-Oracle`, without the version, so that an update doesn't make one
  installation two), mixed with a random salt. The salt is made anew for each day and for each week,
  is kept in the service's database only until that period is over (plus a couple of hours) and is
  never written to a log. The hash goes into a HyperLogLog sketch: a table of small numbers of a
  fixed size, which holds no hashes and no addresses and from which only the approximate number of
  different ones can be read. Players who share an address, such as a shared line or a mobile
  network, count as one, so the number is a floor. An installation that stays connected for days
  counts for each day it is connected.
- **Starts**, from the app's first connection after it starts, and never from the reconnections
  after it: the app adds a few flags to that one request. That it is a start; that it is this
  installation's first start; the version it ran the last time, if that was another one; and the
  interface language, `en` or `ru`. The service turns them into counts: starts, new installations,
  updates from one version to another (`0.1.2` → `0.1.3`, both among those releases, else `other`) and
  starts by language. For this the app keeps small marker files in
  `%LOCALAPPDATA%\poe2-oracle\data`: `last-run-version`, the version of its last start the service
  was told about, and, in builds the developer makes for testing, an empty file `dev`: the service
  counts such a start as a developer's start and as nothing else.
- **Installer downloads**, by who fetched them: the site's button (or any link to the file) or the
  app's updater, told apart by the User-Agent; the updater's also by the version it fetches.
  Downloads of game data packs and of the other release files (`SHA256SUMS` and its signature), and
  reports by kind, are counted as before.
- **The site**: how many pages were loaded (pages only: no pictures, styles or scripts) and how many
  different visitors loaded one in a day, made in the same way, with the browser's User-Agent. For a
  visit that arrived by a link tagged with where it was published, `?from=` and one of a fixed list
  of tags (`reddit`, `forum`, `discord`, `youtube`, `steam`, `wiki`, `lists`, `creators`,
  `article`), the same two counts, and how many times the installer was downloaded after such a
  visit. To tell that, the page's script keeps the tag in your browser tab's session storage, which
  the browser drops when the tab closes, and adds it to the download button's link. No cookie is
  set for this, and a tag that isn't on the list counts nothing. The landing pages ask the service
  for the latest version's number, which they show under their download buttons; that request
  counts nothing.

Your IP address is used for these counts only in the hash described above and, as before, for the
limits on reports and connections (see [Reports](#reports)). It is not logged.

## Signing in to pathofexile.com

Signing in is needed only for private leagues, the **sum** rows among the filters (see
[Account](settings.md#account)) and the trade site's buttons on listings (see
[The trade site's buttons](price-check.md#the-trade-sites-buttons)). **Sign in** opens
pathofexile.com's own sign-in page in a window of the app, a Microsoft Edge WebView2 browser, where
you sign in as on the site: with your password, or through Steam, PlayStation or Xbox. PoE2 Oracle
never sees your password. It takes only the session the site gives that browser, the cookie
`POESESSID`, and checks it on the site's account page.

- **What is kept.** The session alone, in Windows' Credential Manager, as the generic credential
  `PoE2 Oracle/pathofexile.com` (user name `POESESSID`). Windows keeps it encrypted for your Windows
  user on this computer, and it never goes to another one. It is not in the settings file, the logs
  or the diagnostics report.
- **Where it goes.** In the `Cookie` of the requests to `https://www.pathofexile.com` and
  `https://ru.pathofexile.com` (searches, listings, the buttons on listings, leagues, catalogs, the
  account page, your private leagues' pages). Never to poe2scout, the game's picture and exchange
  server or oracle.pushka.biz, and never in a report.
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
- **The keyboard**, through a keyboard hook that is on only while the price panel is open and, if
  a [quick action](quick-actions.md) has a key, while the game is in front. Meanwhile the app notes
  when each key goes down and up, so that holding a quick action's key sends it once, and while
  the panel is open it takes <kbd>Esc</kbd>, which closes the panel and doesn't reach the game.
  Nothing of it is saved or sent. The rest of the time the hook is off, and the app learns only of
  its own hotkeys and, for the XP overlay, that a key was pressed (see **The screen** below).
- **The game's settings file**, `Documents\My Games\Path of Exile 2\poe2_production_Config.ini`:
  the display mode, the client language (which the **Auto** interface language follows) and the key
  for advanced item descriptions. Read only.
- **The Windows display language**, only to pick the interface language on **Auto** before the game
  has ever run.
- **The game's log**, `Client.txt` in the game's `logs` folder: level-ups, area changes and returns
  to character selection for the [XP overlay](xp-overlay.md). Read only. The first time, it also
  reads back up to 64 MB of the log's end, for each character's latest level-up, to record
  [your characters' levels](#what-it-keeps-on-your-computer) for the overlay.
- **The screen**, for the XP overlay: the strip of pixels where the experience bar is, and a few
  rows along the top of the flask and skill panels' rails, where the plates stand. While the game
  is in front, and for two seconds after you switch away, the rails are looked at twenty times a
  second while you move the mouse or press keys, so that a plate steps aside the moment a tooltip
  covers its rail, and less and less often once you leave them alone, down to every two seconds;
  the bar at most twice a second. For that it notes when you use the mouse or the keyboard, never
  which key or where. While the game is behind another window, the rails are looked at every two
  seconds and the bar every ten, or two seconds after a look that finds it changed or can't read
  it; while it is minimised, not at all. Nothing of it is sent and no pixel is saved; only where the
  bar stood is kept, for each of your characters (see
  [What it keeps on your computer](#what-it-keeps-on-your-computer)).

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
| The version of the last start the service was told about, for the next start to say whether it is a first start or an update, and from which version | `%LOCALAPPDATA%\poe2-oracle\data\last-run-version` |
| In a build the developer makes for testing, an empty marker: the service counts such a start as a developer's and as nothing else | `%LOCALAPPDATA%\poe2-oracle\data\dev` |
| The installed game data pack, and the last damaged one set aside | `%LOCALAPPDATA%\poe2-oracle\data\game-data` |
| For the XP overlay: the names of your 20 most recent characters, their levels and where their experience bars stood; it never leaves your computer | `%LOCALAPPDATA%\poe2-oracle\data\xp-levels.json` |
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
