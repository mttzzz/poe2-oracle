# Updates and uninstall

## Updates

PoE2 Oracle updates from [oracle.pushka.biz](https://oracle.pushka.biz/), the app's own site.
Nothing is installed without your click.

- With **Check for updates automatically** on in the settings, the default, the app asks
  oracle.pushka.biz for the latest release 30 seconds after it starts. There is no pop-up: when a
  newer version exists, the update entry in the tray menu changes to **Install version X.Y.Z**.
- To check yourself, click **Check for updates** in the tray menu. It shows "Checking for
  updates…", then "You have the latest version" or "Couldn't check for updates" (click it to try
  again).
- Click **Install version X.Y.Z** to update. The entry shows "Downloading version X.Y.Z…" while
  PoE2 Oracle downloads the release's `SHA256SUMS` file, checks its Ed25519 signature with the key
  built into the app, then downloads the installer and checks its SHA-256 checksum against
  `SHA256SUMS`. The signing key stays where releases are built, so neither oracle.pushka.biz nor
  anyone on the way can hand you an installer of their own. Then the app closes, the
  installer runs silently, and the new version starts by itself a few seconds later. Your settings
  and your start-with-Windows choice stay as they were.
- If the download or a check fails, the entry reads **Update failed — retry (X.Y.Z)**; click it
  to try again. Nothing runs then: with a bad signature the installer isn't even downloaded, and a
  download that doesn't match its checksum is deleted.

You can also update by hand: download the new installer from
[oracle.pushka.biz](https://oracle.pushka.biz/) and run it. It installs over the old version, into
the same folder, and keeps your settings.

Downloaded installers are kept in `%LOCALAPPDATA%\poe2-oracle\cache\updates`; the next update
removes the old one.

## Uninstall

1. Open Windows **Settings → Apps → Installed apps** (on Windows 10: **Apps & features**).
2. Find **PoE2 Oracle** (publisher mttzzz) and choose **Uninstall**.
3. If PoE2 Oracle is running, the uninstaller closes it.

The uninstaller removes the program, its Start menu shortcut, its start-with-Windows entry and the
saved pathofexile.com sign-in. Your settings and the downloaded data stay, ready for a reinstall,
unless you tick **Settings and cache**: "Also delete your settings and the downloaded price data.
Leave unticked to keep them for a reinstall." Ticked, it deletes the folders
`%APPDATA%\poe2-oracle` and `%LOCALAPPDATA%\poe2-oracle`, logs and saved item texts included.

To remove them later by hand, delete those two folders.
