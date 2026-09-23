# Updates and uninstall

## Updates

PoE2 Oracle updates from the [GitHub releases](https://github.com/mttzzz/poe2-oracle/releases) of
the project. Nothing is installed without your click.

- With «Проверять обновления» (check for updates) on, the default, the app asks GitHub for the
  latest release 30 seconds after it starts. There is no pop-up: when a newer version exists, the
  update entry in the tray menu changes to «Установить версию X.Y.Z» (install version X.Y.Z).
- To check yourself, click «Проверить обновления» (check for updates) in the tray menu. It shows
  «Проверка обновлений…» (checking), then «Установлена последняя версия» (you have the latest
  version) or «Не удалось проверить обновления» (the check failed; click to try again).
- Click «Установить версию X.Y.Z» to update. The entry shows «Загрузка версии X.Y.Z…»
  (downloading) while PoE2 Oracle downloads the installer and checks it against the `SHA256SUMS`
  file of the release. Then the app closes, the installer runs silently, and the new version starts
  by itself a few seconds later. Your settings and your start-with-Windows choice stay as they
  were.
- If the download or the check fails, the entry reads «Ошибка обновления — повторить (X.Y.Z)»
  (update error, retry); click it to try again. A download that does not match its checksum is
  deleted and never run.

You can also update by hand: download the new installer from the
[latest release](https://github.com/mttzzz/poe2-oracle/releases/latest) and run it. It installs
over the old version, into the same folder, and keeps your settings.

Downloaded installers are kept in `%LOCALAPPDATA%\poe2-oracle\cache\updates`; the next update
removes the old one.

## Uninstall

1. Open Windows **Settings → Apps → Installed apps** (on Windows 10: **Apps & features**).
2. Find **PoE2 Oracle** (publisher mttzzz) and choose **Uninstall**.
3. If PoE2 Oracle is running, the uninstaller closes it.

The uninstaller removes the program, its Start menu shortcut and its start-with-Windows entry.
Your settings and the downloaded data stay, ready for a reinstall, unless you tick **Settings and
cache**: "Also delete your settings and the downloaded price data. Leave unticked to keep them for
a reinstall." Ticked, it deletes the folders `%APPDATA%\poe2-oracle` and
`%LOCALAPPDATA%\poe2-oracle`, logs and saved item texts included.

To remove them later by hand, delete those two folders.
