# Privacy

PoE2 Oracle has no account of its own and sends no telemetry, analytics or crash reports. It talks
only to the services below, to get prices, pictures and updates. Signing in to pathofexile.com is
optional; signed in, the app searches the trade site with your session (see
[Signing in to pathofexile.com](#signing-in-to-pathofexilecom)). The source code is open on
[GitHub](https://github.com/mttzzz/poe2-oracle), so all of this can be checked.

## What is sent where

| Service | When | What is sent |
|---|---|---|
| Path of Exile trade site: `www.pathofexile.com` and `ru.pathofexile.com` | The list of leagues and both sites' catalogs (modifiers, exchange items, item bases): at start and while running, when the saved copy is older than an hour (leagues) or six hours (catalogs). On every price check: the search, then the found listings. Signed in, also the site's account page, to check the session: at start and when you sign in. | For catalogs, nothing but the request. For a check, the search itself: league, item class or base, the selected filter rows with their bounds, the sellers and price choices. Items from a Russian client are searched on `ru.pathofexile.com`, from an English one on `www.pathofexile.com`. |
| `api.poe2scout.com` | At start, when you change the league, and after a check, when the saved prices are older than 30 minutes. | The league, to get the prices of unique items, the Currency Exchange's last seven days and item pages, and the prices of exchange items not traded lately. |
| `web.poecdn.com` (the game's server for pictures and exchange data) | When the panel shows an item picture or a currency icon. At start, when you change the league, and during checks at most every 10 minutes: each complete hour of GGG's record of the Currency Exchange's trades not downloaded yet. | Requests for those pictures, and for the hours of exchange data, which are the same for every league and player: nothing about you or your league. |
| `api.github.com`, `github.com` and GitHub's file storage (`…githubusercontent.com`) | 30 seconds after start, if **Check for updates automatically** is on; when you click the update entry; when you install an update. | A request for the latest release; downloads of the installer and its `SHA256SUMS`, which github.com passes on to its file storage. |

Signed out, no request carries your account, a cookie or anything else that identifies you. Signed
in, the requests to the trade site carry your pathofexile.com session, the cookie `POESESSID`; no
other request does. Each request carries a fixed User-Agent string that is the same for every
player: a common web browser's for the Path of Exile sites, poe2scout and the game's picture and
exchange server, and `PoE2-Oracle/<version>` for GitHub. As with any request on the internet, each
service sees your IP address.

The links on the panel (**poe2db ↗**, **wiki ↗**, **Craft of Exile ↗**, **poe2scout ↗**,
**…/trade ↗**) open in your browser when you click them: poe2db.tw, www.poe2wiki.net,
beta.craftofexile.com, poe2scout.com and the trade site. The Craft of Exile link carries the item
in its address: its base, item level, rarity and modifiers. PoE2 Oracle itself never contacts
poe2db, the wiki or Craft of Exile.

**Report a bug**, the button **Report a problem** and the link **report a problem ↗** open a GitHub
issue form in your browser with the app's version, your client's language and, for an item, its
text filled in. Nothing reaches GitHub unless you submit that form yourself, from your own account.

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
  `https://ru.pathofexile.com` (searches, listings, leagues, catalogs, the account page). Never to
  poe2scout, the game's picture and exchange server or GitHub.
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
- **The screen**: every two seconds, the strip of pixels where the experience bar is, for the XP
  overlay. Nothing of it is saved or sent.

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
| Your pathofexile.com session, while you are signed in | Windows' Credential Manager: `PoE2 Oracle/pathofexile.com` |
| The sign-in window's browser, while the window is open | `%LOCALAPPDATA%\poe2-oracle\data\login-browser` |
| The start-with-Windows entry, when turned on | Registry: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `PoE2 Oracle` |

The logs note what the app did, such as the name and base of each checked item and how many
listings were found; they do not keep the full item text.

The [diagnostics report](troubleshooting.md#collecting-a-diagnostics-report) is written only when
you click **Collect report** or **Report a bug**. It stays on your desktop and goes wherever you
decide to send it. In it, the paths of your user folder and of your Desktop, Documents and AppData
folders, wherever Windows keeps them, are replaced by `%USERPROFILE%`, `%DESKTOP%`, `%DOCUMENTS%`,
`%APPDATA%` and `%LOCALAPPDATA%`, and your Windows user name, if it has three characters or more,
by `%USERNAME%` wherever else it appears.

Uninstalling always deletes the saved session and the start-with-Windows entry; with **Settings and
cache** ticked, it removes all of the above. See [Updates and uninstall](updates.md#uninstall).
