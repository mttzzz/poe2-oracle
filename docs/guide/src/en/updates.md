# Updates and uninstall

## Updates

PoE2 Oracle keeps itself up to date from [oracle.pushka.biz](https://oracle.pushka.biz/), the
app's own site, while **Update automatically** is on in the settings (section **General**, group
**Updates**; on by default). Nothing needs a click.

- **Staying connected.** About 10 seconds after it starts, once the trade site's catalogs are in,
  the app connects to oracle.pushka.biz and stays connected. The service tells it the latest app
  version and the latest game data version as soon as it connects, and again when one of them
  changes: within about two minutes of a new release. The first connection after each start also
  tells the service, in a few flags, that the app started, whether it is a first start, which
  version ran before and the interface language; the service only counts them (see
  [What the service counts](privacy.md#what-the-service-counts)).
- **A new version** is downloaded at once: first the release's `SHA256SUMS` file, whose Ed25519
  signature PoE2 Oracle checks with the key built into the app, then the installer, whose SHA-256
  checksum must match `SHA256SUMS`. The signing key stays where releases are built, so neither
  oracle.pushka.biz nor anyone on the way can hand you an installer of their own. Then the app
  closes, the installer runs silently, and the new version starts by itself a few seconds later.
  Your settings and your start-with-Windows choice stay as they were.
- **New game data** — the tables PoE2 Oracle reads items with: the ways the game client prints a
  modifier that the trade site's catalog lacks (in both client languages), the modifier tiers, the
  Currency Exchange's items, and items' English names, pictures and bases — comes as a game data
  pack, checked the same way before it is used. The app unpacks it and restarts itself to load it;
  the program itself isn't reinstalled.
- **Not while you use it.** An update waits while any of the app's windows is open: the price
  panel, the settings, the report window, the pathofexile.com sign-in window or the tour; and
  while a price check or a quick action is still putting your clipboard back. It goes in the
  moment the last of them is over. It doesn't wait for the game: with none of those windows open,
  the app restarts even while you play.
- **After the restart** a plate at the bottom right of the screen the game is on (with no game
  running, the screen under the mouse pointer) says "PoE2 Oracle updated to X.Y.Z" or "Game data
  updated" for 8 seconds. It never takes the keyboard from the game: **×** closes it, and a click
  anywhere else on it opens the settings. The XP overlay comes back as it was, with its rate, time
  to the next level and map timer, rather than measuring afresh.
- **Without internet** the app connects again by itself: a few seconds after a failed attempt,
  then less often, up to about 5 minutes apart. On Windows 10 version 2004 and later it connects
  at once when Windows says the internet is back.
- **When something fails** — the download, a signature or checksum check — nothing runs, and the
  app quietly tries again later: after a minute, then after longer waits, at least once an hour.
  With a bad signature the installer isn't even downloaded, and a download that doesn't match its
  checksum is deleted. An update that is downloaded but can't be started (an antivirus blocking
  the installer, say) isn't downloaded again: the app tries only the start again, a minute later
  and then two minutes later, and after the third failure leaves that version until the next
  start. An update that didn't take after its restart is tried again only at the next start, and
  so is a game data pack this version of the app can't use (one made for a newer version, or one
  that fails its checks when unpacked); the settings say why.

In the settings, section **General**, group **Updates**, the row under the switch names the app's
version and its game data's, and says what is going on: connecting, connected, "No connection to
oracle.pushka.biz — will connect when the internet is back", "You have the latest version", a
download under way, an update that is ready and waits for the app's windows to close, or why the
last try failed. **Check now** connects at once instead of waiting for the next attempt, retries
a failed download right away, and a failed start as soon as the app's windows are closed.

With **Update automatically** off, PoE2 Oracle doesn't connect to oracle.pushka.biz for updates:
nothing is downloaded or installed, and only a report you send yourself goes there. You can update
by hand then: download the new installer from [oracle.pushka.biz](https://oracle.pushka.biz/) and
run it. It installs over the old version, into the same folder, and keeps your settings.

Downloaded installers are kept in `%LOCALAPPDATA%\poe2-oracle\cache\updates`; the next update
removes the old one. A game data pack is deleted from there once it is unpacked.

The unpacked game data is kept in `%LOCALAPPDATA%\poe2-oracle\data\game-data`. At each start the
app uses it only while it is newer than the tables built into the app and made for this version of
the app or an older one, and only if every table still matches its checksum and reads without an
error. An installed pack that the next app update has caught up with is deleted, and so is one
made for a newer version, found after going back to an older one; a damaged one is moved to
`game-data\rejected`, and the app uses its built-in tables.

## Uninstall

1. Open Windows **Settings → Apps → Installed apps** (on Windows 10: **Apps & features**).
2. Find **PoE2 Oracle** and choose **Uninstall**.
3. If PoE2 Oracle is running, the uninstaller says "PoE2 Oracle is running and will be closed to
   continue.": **OK** closes it, **Cancel** stops the uninstall.

The uninstaller removes the program, its Start menu shortcut, its start-with-Windows entry and the
saved pathofexile.com sign-in. Your settings and the downloaded data stay, ready for a reinstall,
unless you tick **Settings and cache**: "Also delete your settings and the downloaded price data.
Leave unticked to keep them for a reinstall." Ticked, it deletes the folders
`%APPDATA%\poe2-oracle` and `%LOCALAPPDATA%\poe2-oracle`, logs and saved item texts included.

To remove them later by hand, delete those two folders.
