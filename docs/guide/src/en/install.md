# Install and first run

## Requirements

- Windows 10 or Windows 11, 64-bit. Nothing else to install first: the app needs no .NET and no
  Visual C++ Redistributable. Only the optional sign-in to pathofexile.com needs the Microsoft Edge
  WebView2 Runtime; if Windows lacks it, the settings say so and offer **Download from Microsoft**.
- Path of Exile 2 with the English or Russian client.
- The game in **Windowed** or **Windowed Fullscreen** display mode. Over exclusive Fullscreen,
  Windows cannot show other programs' windows, so the panel would stay invisible.
- An internet connection: prices come from the Path of Exile trade site, GGG's Currency Exchange
  data (`web.poecdn.com`) and poe2scout.

## Download

On [oracle.pushka.biz](https://oracle.pushka.biz/), click **Download for Windows**: the button
always downloads the latest installer, `PoE2-Oracle-Setup-<version>.exe`.

Each release also has a `SHA256SUMS` file on the site, at
`https://oracle.pushka.biz/download/v<version>/SHA256SUMS`: for version 0.1.0,
<https://oracle.pushka.biz/download/v0.1.0/SHA256SUMS>. To check the download, run this in
PowerShell in the folder with the installer and compare the result with the installer's line in
`SHA256SUMS` (letter case doesn't matter):

```powershell
Get-FileHash .\PoE2-Oracle-Setup-*.exe -Algorithm SHA256
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
  is ticked if Windows has a startup entry for PoE2 Oracle, even one turned off in Task Manager;
  finishing with it ticked turns autostart on, leaving it unticked turns it off. With **Run PoE2
  Oracle** ticked, the app starts with a welcome; see [First start](#first-start).
- If PoE2 Oracle is running, the installer says "PoE2 Oracle is running and will be closed to
  continue." and closes it first.

## Set the game's display mode

In the game's graphics options, set the display mode to **Windowed Fullscreen** (or Windowed).
If the game is in exclusive Fullscreen, the settings window of PoE2 Oracle warns about it at the
top.

## First start

Finish the installer with **Run PoE2 Oracle** ticked, and PoE2 Oracle starts with its settings
window open at **General** and a welcome over it. The welcome says that PoE2 Oracle is installed
and running; where you will find it from now on (its icon by the clock, its button on the taskbar,
or both) and that a click there opens the settings; for the icon by the clock, that Windows first
hides a new app's icon under the **^** arrow and that dragging it from there onto the taskbar keeps
it in sight; how to check a price with your hotkey, and that the XP overlay shows above the flask
panel (or that it is off); and whether PoE2 Oracle starts with Windows. **Got it** closes the
welcome, and so do <kbd>Enter</kbd> and <kbd>Esc</kbd>. If a copy of PoE2 Oracle was already
running, that copy shows the welcome.

PoE2 Oracle has no main window. It runs in the background, and you find it by its icon in the
notification area next to the clock, by its button on the taskbar, or by both: **Where to show the
app** in the [settings](settings.md#system) picks which, the icon by the clock being the default. If
you do not see the icon, it may be under the **^** arrow (show hidden icons); you can drag it out
onto the taskbar. Hover the icon to see the price-check hotkey: "PoE2 Oracle — price check: Ctrl+E".

On the very first start the app downloads the trade site's data, which takes a few seconds, and
then, after the welcome if it is showing, walks you through a short tour. It dims the screen around
one thing at a time and explains it on a card: the league in the settings window; then, in the
game, pointing at an item and pressing <kbd>Ctrl</kbd>+<kbd>E</kbd>; then the filters, **Search**,
the listings and the league on the price panel that opens; and last the XP overlay. **Next** and
**Back** move between the steps, and a step that waits for you, like the first price check, moves
on by itself once you do it. **Skip tour** ends it. The tour starts with every launch until you
finish or skip it; after that, **Help** → **Tutorial** → **Replay** in the settings runs it again.

PoE2 Oracle speaks English or Russian. At first it follows the game client's language, or
Windows' before the game has ever run; **Interface language** in the settings changes it. See
[Interface language](settings.md#interface-language).

Look through the [settings](settings.md): the league, the client and interface languages and the
hotkey are the main ones. Changes apply and are saved at once. Warnings marked **⚠** at the top mean
something in your setup keeps price checks from working; see [Troubleshooting](troubleshooting.md).

Then, in the game, point at an item and press <kbd>Ctrl</kbd>+<kbd>E</kbd>. See
[Price check](price-check.md).

## The tray icon and the taskbar button

A click on the tray icon opens the [settings](settings.md), or brings them to the front. A right
click opens its menu:

- **Settings** opens the settings;
- **Quit** closes PoE2 Oracle.

With **On the taskbar** or **Both** picked in **Where to show the app**, a **PoE2 Oracle** button
stays on the taskbar for as long as the app runs, with the app's icon:

- a click on it opens the settings, or brings them to the front. The settings and report windows
  have no taskbar button of their own then: this one stands for them;
- **Close window** in its right-click menu closes PoE2 Oracle, as **Quit** does;
- hovering it shows the app's icon instead of a preview;
- <kbd>Alt</kbd>+<kbd>Tab</kbd> lists it as PoE2 Oracle while neither the settings nor the report
  window is open, and picking it there opens the settings.

**Quit the app** in the settings, section **Help**, closes PoE2 Oracle too, whichever of the two
shows.

## One copy at a time

Only one copy of PoE2 Oracle runs at a time. Starting it again, for example from the Start menu,
opens the settings of the copy that is already running. A copy started with Windows while another
one runs quietly exits.

## Start with Windows

Turn on **Start with Windows** in the settings, section **General**, or tick the box on the
installer's last page. Started with Windows, PoE2 Oracle waits in the background until you play,
with two exceptions: until you finish or skip it, the tour opens at every start, and the first start
after a crash opens the [report window](report.md#after-a-crash).
