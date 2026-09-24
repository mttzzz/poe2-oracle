# Install and first run

## Requirements

- Windows 10 or Windows 11, 64-bit. Nothing else to install first: the app needs no .NET and no
  Visual C++ Redistributable.
- Path of Exile 2 with the English or Russian client.
- The game in **Windowed** or **Windowed Fullscreen** display mode. Over exclusive Fullscreen,
  Windows cannot show other programs' windows, so the panel would stay invisible.
- An internet connection: prices come from the Path of Exile trade site, GGG's Currency Exchange
  data (`web.poecdn.com`) and poe2scout.

## Download

Download `PoE2-Oracle-Setup-<version>.exe` from the
[latest release](https://github.com/mttzzz/poe2-oracle/releases/latest).

The release also has a `SHA256SUMS` file. To check the download, run this in PowerShell in the
folder with the installer and compare the result with the line for the installer in `SHA256SUMS`:

```powershell
Get-FileHash .\PoE2-Oracle-Setup-<version>.exe -Algorithm SHA256
```

## Windows SmartScreen

The installer is not code-signed yet, so Windows may show "Windows protected your PC" when you
start it. Click **More info**, then **Run anyway**. Your browser may also warn that the file is
not commonly downloaded; keep it.

## Install

- The installer is in English or Russian, following your Windows display language.
- It installs for your Windows user only and needs no administrator rights. The default folder is
  `%LOCALAPPDATA%\Programs\PoE2 Oracle`; you can pick another one.
- It adds a **PoE2 Oracle** shortcut to the Start menu. There is no desktop shortcut.
- The last page offers to run PoE2 Oracle and to start it with Windows. The start-with-Windows box
  is ticked only if PoE2 Oracle already starts with Windows; leaving it unticked turns that off.
- If PoE2 Oracle is running, the installer says "PoE2 Oracle is running and will be closed to
  continue." and closes it first.

## Set the game's display mode

In the game's graphics options, set the display mode to **Windowed Fullscreen** (or Windowed).
If the game is in exclusive Fullscreen, the settings window of PoE2 Oracle warns about it at the
top.

## First start

PoE2 Oracle has no main window. It runs in the background, and its only sign is an icon in the
notification area next to the clock. If you do not see it, it may be under the **^** arrow (show
hidden icons); you can drag it out onto the taskbar. Hover the icon to see the price-check
hotkey: "PoE2 Oracle — price check: Ctrl+E".

On the very first start the app downloads the trade site's data, which takes a few seconds, and
then walks you through a short tour. It dims the screen around one thing at a time and explains it
on a card: the league in the settings window; then, in the game, pointing at an item and pressing
<kbd>Ctrl</kbd>+<kbd>E</kbd>; then the filters, **Search**, the listings and the league on the
price panel that opens; and last the XP overlay. **Next** and **Back** move between the steps, and a
step that waits for you, like the first price check, moves on by itself once you do it. **Skip
tour** ends it. The tour starts with every launch until you finish or skip it; after that,
**Help** → **Tutorial** → **Replay** in the settings runs it again.

PoE2 Oracle speaks English or Russian. At first it follows the game client's language, or
Windows' before the game has ever run; **Interface language** in the settings changes it. See
[Interface language](settings.md#interface-language).

Look through the [settings](settings.md): the league, the client and interface languages and the
hotkey are the main ones. Changes apply and are saved at once. Warnings marked **⚠** at the top mean
something in your setup keeps price checks from working; see [Troubleshooting](troubleshooting.md).

Then, in the game, point at an item and press <kbd>Ctrl</kbd>+<kbd>E</kbd>. See
[Price check](price-check.md).

## The tray icon

Click the icon (a right-click works too) to open its menu:

- **Settings** opens the [settings](settings.md);
- **Check for updates** looks for a new version; see [Updates and uninstall](updates.md);
- **Report a bug** saves a diagnostics report to your desktop and opens GitHub's bug report form
  in your browser; see [Reporting a bug](troubleshooting.md#reporting-a-bug);
- **Quit** closes PoE2 Oracle.

## One copy at a time

Only one copy of PoE2 Oracle runs at a time. Starting it again, for example from the Start menu,
opens the settings of the copy that is already running. A copy started with Windows while another
one runs quietly exits.

## Start with Windows

Turn on **Start with Windows** in the settings, section **General**, or tick the box on the
installer's last page. Started with Windows, PoE2 Oracle waits in the tray until you play.
