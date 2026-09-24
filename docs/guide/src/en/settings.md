# Settings

![The settings window](../images/en/settings.webp)

Open the settings in any of three ways:

- the tray icon's menu → **Settings**;
- the gear **⚙** on the price panel;
- start PoE2 Oracle again while it is already running: the running copy opens its settings.

If the settings window is already open, any of these brings it to the front.

The window opens in the middle of the monitor the game is on and stays on top of the game. Drag it
by its title bar; resize it by its edges. The sections are listed on the left, the chosen section's
settings on the right.

Every change applies and is saved at once: there is no Save or Cancel button. Text in a box applies
when you press <kbd>Enter</kbd> or <kbd>Tab</kbd>, click elsewhere or close the window;
<kbd>Esc</kbd> in a box puts the old text back. The **×** in the title bar or <kbd>Esc</kbd> closes
the window; with the league list or a dialog open, <kbd>Esc</kbd> closes that first.

While the settings window is open, the price panel is hidden and the price-check hotkey is off, so
that you can record a new one.

A gold-framed box at the top of **General** shows warnings marked **⚠** about your setup, such as
exclusive Fullscreen or the copy shortcut held by another program. See
[Troubleshooting](troubleshooting.md).

This page names the settings as the English interface shows them; the
[interface language](#interface-language) switches them to Russian.

## General

### League

Row **League**: a list of where prices are searched.

- **Auto · *league*** (the default): the trade site's current league, the first one it lists.
- Any league the trade site lists, named the way the site names it in the interface language
  (www.pathofexile.com's names in English, ru.pathofexile.com's in Russian), as on the price panel's
  league chip.
- **Private league · *name***: each of your account's private leagues, as pathofexile.com's
  **Private Leagues** page lists them, while you are signed in ([Account](#account)).

Notes under the row's name: "Loading the league list from the trade site" or "The league list from
the trade site didn't load"; "The trade site no longer lists this league — searches go to the
current one" means the league you picked has ended and searches go to the current league instead.
Signed out, the row adds that your private leagues show up here once you sign in. With a private
league picked, it names the public league its exchange prices come from (see
[In a private league](price-check.md#in-a-private-league)), and signed out it warns: "Without a
sign-in the site won't answer searches in a private league — sign in, in “Account”".

### Game client language

Row **Game client language**: **Auto** (the default), **Русский** (Russian) or **English**. On auto,
the language of each item is recognised from its copied text. The language decides how the item text
is read and which trade site is searched: ru.pathofexile.com for a Russian client,
www.pathofexile.com for an English one.

### Interface language

Row **Interface language**: **Auto · *language*** (the default), **Русский** (Russian) or
**English**, each language named in its own words. It is the language of PoE2 Oracle's own words:
the settings, the price panel, the overlays, the tray menu, the messages and the way numbers are
written (`15.1%` in English, `15,1 %` in Russian). What comes from the game and the trade site keeps
its language: item names and mod lines as the client copied them, league names as the trade site
gives them. A change applies at once, in every open window.

**Auto** follows the game client's language, which the game's own settings file names; before the
game has ever run, it follows the Windows display language. Its choice names the language it stands
for now, for example **Auto · English**.

The interface language also picks the pathofexile.com sign-in page (see [Account](#account)), the
language Craft of Exile opens in and the trade site whose league names the league lists show.

### Interface scale

Row **Interface scale**: 100% by default, from 80 to 150% in steps of 5, with the − and + buttons. It
sizes the text and controls of the price panel (its width too) and of the settings window, which
grows or shrinks around the pointer, so the − and + buttons stay under it. The price panel keeps its
place beside the inventory or the stash and grows away from it. The XP overlay is part of the game's
HUD and takes its size.

### System

Group **System**.

- **Start with Windows**, off by default. It adds PoE2 Oracle to the apps Windows starts when you
  sign in; the installer's last page has the same option. Turning it on here also re-enables it if it
  was disabled in Task Manager's startup apps.
- **Check for updates automatically**, on by default: look for a new version 30 seconds after start.
  The tray menu's update entry works either way. See [Updates and uninstall](updates.md).

## Price check

Section **Price check**.

### Hotkey

Row **Price check**. Click the key box and press the new combination: it works at once.
<kbd>Esc</kbd> or a second click on the box keeps the old one.

A combination can be <kbd>Ctrl</kbd> or <kbd>Alt</kbd> (with or without <kbd>Shift</kbd>) plus a
letter A–Z or a digit 0–9, or one of <kbd>F1</kbd>–<kbd>F12</kbd>, alone or with modifiers. Letters
are physical keys: <kbd>Ctrl</kbd>+<kbd>E</kbd> stays the same key when the Russian keyboard layout
is on. The box refuses a combination and says why:

| Message | Reason |
|---|---|
| "Letters and digits need Ctrl or Alt — otherwise the key would stop typing" | Without Ctrl or Alt the letter or digit would stop typing everywhere |
| "Ctrl+C is taken: the app copies items with it" | Ctrl+C is how the item gets copied |
| "Alt+F4 closes windows, the game included" | Alt+F4 closes windows, the game included |
| "Combinations with the Windows key aren't supported" | Combinations with the Windows key are not supported |
| "Only letters A–Z, digits 0–9 and F1–F12 work" | Other keys can't be used |
| "Another quick action already uses this combination" | A quick action already uses it |

If another program already holds the combination, the box says so, for example "F7 is taken by
another program — Ctrl+E stays", and your previous hotkey keeps working. The tray icon's tooltip
always names the hotkey in use.

### Search

Group **Search**.

- **Default sellers**: which sellers every new check searches. **Instant Buyout** (the default: you
  buy in the game and the seller need not be online), **Buyout or In Person** (instant buyout and
  sellers online), **In Person** (sellers online) or **Any** (everyone, offline too). The panel's
  **Sellers:** chip still switches it for one item. Settings saved by an older version with its
  default, buyout or in person, move to Instant Buyout once; after that, your choice stays.
- **Seller column**, on by default: the seller's account name in the results table.

## Quick actions

Section **Quick actions**: hotkeys that type chat commands or stash searches into the game. Each
action has its kind (**Chat** or **Stash**), its text, its key and a **×**; **+ Add action** is
below them. See [Quick actions](quick-actions.md).

## XP overlay

Section **XP overlay**, group **XP line**. See [XP overlay](xp-overlay.md) for what it shows.

| Setting | Default | Meaning |
|---|---|---|
| **Show the XP overlay** | on | Show the experience rate and the time to the next level above the flask panel |
| **Level percentage** | off | Also show how much of the current level is done |
| **Map timer** | on | Show the time in the current map, the experience it gave and the session's average map time above the skill panel |
| **Rate smoothing** | 10m | 5, 10, 20 or 30 minutes. Shorter shows a change of farming sooner, longer reads steadier |

## Account

Section **Account**. Signing in to pathofexile.com is needed to search private leagues and for the
**sum** rows among the [filters](price-check.md#filters).

- **pathofexile.com.** The row tells what is known about your sign-in: "Sign in through the window
  that opened" while the sign-in window is open, "Not signed in", "Checking the sign-in…", "Signed
  in as *account*" (or "Signed in" when the site's page doesn't name the account), "Session
  expired" (the site no longer accepts it) or "Couldn't check the sign-in" (the site did not
  answer; the session is used as it is). **Sign in** opens pathofexile.com's sign-in page in the
  app's own window, in the interface language (www.pathofexile.com in English, ru.pathofexile.com
  in Russian; both keep the same session): sign in as usual, through Steam too, and the window
  closes by itself. The session is kept in Windows' Credential Manager and sent to pathofexile.com
  only. **Sign out** makes PoE2 Oracle forget the session; you stay signed in on the site. The
  sign-in window needs the Microsoft Edge WebView2 Runtime; without it, the row shows a link
  **Download from Microsoft**.
- **Private leagues.** Signed in, nothing needs typing: the league menus, in **General** and on
  the price panel, list your account's private leagues as pathofexile.com shows them on its
  **Private Leagues** page, and the row names them: "Your leagues on pathofexile.com are in the
  league menus: *names*". The list is read again each time the settings open, so a league you
  joined since shows up. Exchange prices come from the public league yours is made from: see
  [In a private league](price-check.md#in-a-private-league).

## Help

Section **Help**.

- **Tutorial** → **Replay** runs the short guided tour again: the settings and the league, a price
  check, the price panel and the XP overlay, one at a time.
- **Report a bug**:
  - **Report ↗** does what the tray menu entry **Report a bug** does: it writes the diagnostics
    report to your desktop, shows it in File Explorer and opens GitHub's bug report form in your
    browser. See [Reporting a bug](troubleshooting.md#reporting-a-bug).
  - **Collect report** only writes the report: a zip with the logs, the settings and the item texts
    the app could not read, on your desktop, shown in File Explorer. Your Windows user name, if it
    has three characters or more, is hidden in it. See
    [Troubleshooting](troubleshooting.md#collecting-a-diagnostics-report).
- **Logs folder** → **Open** opens the folder with the logs of this run and the one before.
- **About**: PoE2 Oracle's version and license. **Licenses** opens `THIRD-PARTY-NOTICES.html`, which
  the installer puts next to the app: the licenses of everything it includes. **GitHub ↗** opens the
  project's page.

## Where the settings are kept

`%APPDATA%\poe2-oracle\config\settings.json`, together with your waystone marks and quick actions.
If that file ever cannot be read, PoE2 Oracle renames it to `settings.json.bak` and starts from the
defaults.
