# Privacy

PoE2 Oracle has no account and no sign-in, and sends no telemetry, analytics or crash reports. It
talks only to the services below, to get prices, pictures and updates. The source code is open on
[GitHub](https://github.com/mttzzz/poe2-oracle), so all of this can be checked.

## What is sent where

| Service | When | What is sent |
|---|---|---|
| Path of Exile trade site: `www.pathofexile.com` and `ru.pathofexile.com` | The list of leagues and both sites' catalogs (modifiers, exchange items, item bases): at start and while running, when the saved copy is older than an hour (leagues) or six hours (catalogs). On every price check: the search, then the found listings. | For catalogs, nothing but the request. For a check, the search itself: league, item class or base, the selected filter rows with their bounds, the sellers and price choices. Items from a Russian client are searched on `ru.pathofexile.com`, from an English one on `www.pathofexile.com`. |
| `poe.ninja` | At start, when you change the league, and after a check, when the saved prices are older than 30 minutes. | The league, to get Currency Exchange prices. |
| `api.poe2scout.com` | At start, when you change the league, and when you check a unique item or an exchange item poe.ninja does not track in your league, when the saved prices are older than 30 minutes. | The league, to get the prices of unique items and exchange items. |
| `web.poecdn.com` (the game's picture server) | When the panel or a trade card shows an item picture or a currency icon. | Requests for those pictures. |
| `api.github.com`, `github.com` | 30 seconds after start, if «Проверять обновления» (check for updates) is on; when you click the update entry; when you install an update. | A request for the latest release; downloads of the installer and its `SHA256SUMS`. |

No request carries your account, a cookie or anything else that identifies you. Each one carries a
fixed User-Agent string that is the same for every player: a common web browser's for the Path of
Exile sites, poe.ninja, poe2scout and the picture server, and `PoE2-Oracle/<version>` for GitHub.
As with any request on the internet, each service sees your IP address.

The links on the panel («poe2db ↗», «вики ↗», «poe.ninja ↗», «…/trade ↗») open in your browser:
poe2db.tw, www.poe2wiki.net, poe.ninja and the trade site. PoE2 Oracle itself never contacts
poe2db or the wiki.

«Сообщить об ошибке» (report a bug), «сообщить об ошибке ↗» and «Сообщить разработчику» open a
GitHub issue form in your browser with the app's version, your client's language and, for an item,
its text filled in. Nothing reaches GitHub unless you submit that form yourself, from your own
account.

## What it reads on your computer

- **The clipboard**, only during a price check, a quick action or a trade reply. A check reads
  the item text the game copied and then puts back what you had copied before. A quick action or
  a reply puts its text on the clipboard for a moment and then restores yours.
- **The game's settings file**, `Documents\My Games\Path of Exile 2\poe2_production_Config.ini`:
  the display mode, the client language and the key for advanced item descriptions. Read only.
- **The game's chat log**, `Client.txt` in the game's `logs` folder: trade whispers and party
  join and leave lines for [trade requests](trade-requests.md); level-ups, area changes and returns
  to character selection for the [XP overlay](xp-overlay.md). Read only.
- **The screen**: every two seconds, the strip of pixels where the experience bar is, for the XP
  overlay. Nothing of it is saved or sent.

## What it types into the game

Only when you press one of its hotkeys or click one of its buttons: the game's item-copy shortcut
for a price check, the text of a [quick action](quick-actions.md), or a reply or stash search from
a trade request card.

## What it keeps on your computer

| What | Where |
|---|---|
| Settings, with waystone marks and quick actions | `%APPDATA%\poe2-oracle\config\settings.json` |
| Logs of the current and the previous run | `%LOCALAPPDATA%\poe2-oracle\data\logs` |
| Item texts the app could not fully read, the last 100 | `%LOCALAPPDATA%\poe2-oracle\data\unparsed` |
| Trade site catalogs, poe.ninja and poe2scout prices, downloaded updates | `%LOCALAPPDATA%\poe2-oracle\cache` |
| The start-with-Windows entry, when turned on | Registry: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `PoE2 Oracle` |

The logs note what the app did, such as the name and base of each checked item and how many
listings were found; they do not keep the full item text.

The [diagnostics report](troubleshooting.md#collecting-a-diagnostics-report) is written only when
you click «Собрать отчёт» (collect report) or «Сообщить об ошибке» (report a bug). It stays on your desktop, with the path of your
Windows user folder replaced by `%USERPROFILE%`, and goes wherever you decide to send it.

Uninstalling with **Settings and cache** ticked removes all of the above; see
[Updates and uninstall](updates.md#uninstall).
