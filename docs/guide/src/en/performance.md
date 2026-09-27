# Performance

PoE2 Oracle is a native Windows app written in Rust: one process, with no Electron and no browser
inside, whose interface the graphics card draws. This page holds the measurements behind the
numbers on the [site](../../): PoE2 Oracle next to other price checkers in the game and with the
game in the background, its own speed and size, how they were measured and on which PC, and what
the numbers don't tell.

## How it draws

- [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the UI framework of the Zed
  code editor, draws every window of the app with the graphics card, through Direct3D 11, all of
  them with one device: at feature level 11.1 on the PC below, with 11.0 and 10.1 to fall back on.
  Its shaders are compiled when the app is built, not when it starts.
- Each window shows its frames through a DXGI flip swap chain of its own, and DirectComposition
  puts the windows together: that is how the overlays are see-through pixel by pixel.
- DirectWrite renders the text: its glyphs go into an atlas on the graphics card, which draws them
  from there, with ClearType in opaque windows and in grayscale in transparent ones. The
  interface's two fonts are built into the exe.
- It is one process, and there is nothing else to install: one exe with its C runtime built in, no
  .NET, no Visual C++ Redistributable, no Chromium.
- Two things are drawn by others. The menu of the icon by the clock is Windows' own. The optional
  window for [signing in to pathofexile.com](privacy.md#signing-in-to-pathofexilecom) shows the
  site in Windows' own browser component, Microsoft Edge WebView2, which starts only when you
  click **Sign in**; its processes end with the window.

## In the game

Measured on 27 September 2026 on the PC described under [How it was measured](#how-it-was-measured),
with Path of Exile 2 in front. PoE2 Oracle, POE2 Currency Overlay, Exiled Exchange 2 and PoE Overlay
II ran at once, and every minute of measuring read all four. Sidekick wasn't part of this session.

| App | Version | Built on |
|---|---|---|
| PoE2 Oracle | 0.1.0: commit 786f69b in real play, 5fc9b59 in the controlled test, 93296e4 for the price checks | Rust, GPUI on Direct3D 11 |
| POE2 Currency Overlay | 3.0.7 | Electron |
| Exiled Exchange 2 | 0.16.7 | Electron |
| PoE Overlay II | 1.67.0 | Overwolf 0.310.1.1 |

### Real play

Measured from 10:32 to 10:37 (UTC+3), with PoE2 Oracle's build of commit 786f69b, while the PC's
owner played as he usually does: three minutes, read with no input of ours. Each minute read all
four apps, so it was the same play for all of them.

CPU in each minute, in % of one core, and the median of the three:

| App | 10:32 | 10:35 | 10:36 | Median |
|---|--:|--:|--:|--:|
| **PoE2 Oracle, commit 786f69b** | **0.672** | **0.686** | **0.668** | **0.672** |
| POE2 Currency Overlay | 2.832 | 3.225 | 3.624 | 3.225 |
| PoE Overlay II + Overwolf, main window open | 3.301 | 3.700 | 4.104 | 3.700 |
| Exiled Exchange 2 | 3.717 | 4.228 | 4.772 | 4.228 |

- In real play, PoE2 Oracle used the least CPU of the four: 0.672% of a core at the median, against
  3.225% for POE2 Currency Overlay, 3.700% for PoE Overlay II and 4.228% for Exiled Exchange 2. In
  every minute it used 4 to 7 times less CPU than any of the others: 401–412 ms a minute, against
  1.7–2.9 s.
- The other three used more CPU in each minute than in the one before, by 0.39–0.54 percentage
  points; PoE2 Oracle stayed at 0.668–0.686%.
- After the game was restarted, PoE Overlay II had its main window open again, on the second
  monitor.
- Where PoE2 Oracle's CPU went, by thread, in a minute: the XP overlay's screen watcher 177–182 ms
  (64–70 wake-ups a second); its UI thread 115–126 ms (44–58), as the XP and map plates update;
  GPUI's vsync thread 38–41 ms (21–24); its keyboard hook, woken by each key pressed in the game,
  10–16 ms (26–36); and the NVIDIA driver's 31 threads about 40 ms (about 155).

The other counters, from the same readings, the median of the three minutes:

| App | Processes / threads | Memory in Task Manager, MB | GPU memory, dedicated, MB |
|---|--:|--:|--:|
| **PoE2 Oracle, commit 786f69b** | **1 / 44** | **50.5** | **68.1** |
| POE2 Currency Overlay | 4 / 152 | 41.0 | 72.3 |
| PoE Overlay II + Overwolf, main window open | 9 / 327 | 372.5 | 38.8 |
| Exiled Exchange 2 | 4 / 103 | 71.0 | 0.0 |

- The memory column isn't a comparison. Windows trims the memory of programs that sit idle, so over
  a session the numbers in Task Manager's Memory column change as you play. Exiled Exchange 2 and
  POE2 Currency Overlay had been running since 08:09–08:10, and since the morning Windows had
  trimmed Exiled Exchange 2 from 90 to 71 MB and POE2 Currency Overlay from 107 to 41 MB. PoE2
  Oracle had been running since 10:20, and by its log the owner checked prices in it six times from
  10:20 to 10:34, none of them in a measured minute. The site compares memory a few minutes after
  each app's launch: see [With the game in the background](#with-the-game-in-the-background).

### Controlled test

Measured from 09:40 to 09:46 (UTC+3), with PoE2 Oracle's build of commit 5fc9b59, the character in
the hideout and the inventory open. **At rest**: a minute with no input, begun after 100 seconds
without any. **Mouse moving**: a minute of slow circles over the inventory grid, with real input and
no clicks.

| App | Mouse at rest, % of a core (ms a minute) | Mouse moving, % of a core (ms a minute) | Memory in Task Manager, MB | Threads |
|---|--:|--:|--:|--:|
| **PoE2 Oracle, commit 5fc9b59** | **0.088 (53)** | **0.348 (209)** | **34.0** | **43** |
| POE2 Currency Overlay | 0.099 (59) | 0.71 (427) | 107.0 | 152 |
| Exiled Exchange 2 | 0.081 (49) | 1.00 (599) | 88.0 | 103 |
| PoE Overlay II + Overwolf | 1.49 (894) | 1.97 (1181) | 327.7–331.5 | 325 |
| PoE2 Oracle, commit 93296e4, the build before | 0.69–0.72 (412–430) | 5.57 (3343) | 35–48 | 43 |

- With the mouse moving, PoE2 Oracle used the least CPU of the four: 0.348% of a core, against
  0.71% for POE2 Currency Overlay, 1.00% for Exiled Exchange 2 and 1.97% for PoE Overlay II.
- At rest, PoE2 Oracle (0.088%), Exiled Exchange 2 (0.081%) and POE2 Currency Overlay (0.099%) were
  about even, Exiled Exchange 2 a little the lowest. In the earlier minutes of the same session, the
  other three measured about the same: Exiled Exchange 2 0.075–0.090% at rest and 1.00% with the
  mouse moving, POE2 Currency Overlay 0.10% and 0.73%, PoE Overlay II 1.45–1.47% and 2.03%.
- The memory column isn't a comparison: PoE2 Oracle's build of 5fc9b59 had been started at 09:39:57
  and hadn't checked a price yet, while the other three had each done six checks earlier in the
  session. [Price checks](#price-checks) compares memory before and after the same six checks.
- The build of 5fc9b59 changed the XP overlay's screen watcher: it reads a frame asynchronously, raw
  input wakes it, and it looks at the screen at most every 2 seconds while the mouse is at rest and
  every 50 ms while it moves. Against the build before, PoE2 Oracle used about 8 times less CPU at
  rest and 16 times less with the mouse moving.

Where PoE2 Oracle's CPU went, by thread, in a minute:

| State | Whole process, ms | Screen watcher, ms (wake-ups a second) | NVIDIA driver: threads / ms / wake-ups a second |
|---|--:|--:|--:|
| At rest, commit 5fc9b59 | 53 | 6.8 (1.4) | 31 / 36 / 150 |
| At rest, the build before | 394–446 | 347–399 (15–24) | 31 / 36–37 / 148–149 |
| Mouse moving, commit 5fc9b59 | 205 | 158 (51) | 31 / 35 / 149 |
| Mouse moving, the build before | 3249 | 3199 (66) | 31 / 36 / 150 |

- At rest, most of what is left is two of the NVIDIA driver's threads: 36 ms a minute and about 150
  wake-ups a second between them.
- The plates still step aside for a tooltip: with the pointer on the life flask, whose tooltip
  covers the XP bar, the XP plate and its ⚙ were gone within 129 ms, and back within 94 ms after
  the pointer moved to an empty inventory cell.

### Price checks

Measured from 08:37 to 09:25 (UTC+3), with PoE2 Oracle's build of commit 93296e4, the build before
5fc9b59. Six items were checked, each in all four apps.

Times are from the key press, read off a recording of the screen at 60 frames a second, so to a
frame, ±17 ms. **Window**: the first frame that shows the app's window. **Result**: the last change
in the window's area, after which nothing changed for 500 ms.

| Item | PoE2 Oracle: window / result, ms | The fastest other app to a result | Its window / result, ms |
|---|--:|---|--:|
| Currency: a stack of 27 Orbs of Augmentation | 92 / **231** | POE2 Currency Overlay | 47 / 342 |
| A rare helmet | 109 / **731** | POE2 Currency Overlay, with a search | 46 / 792 |
| A rare ring | 93 / 715 | POE2 Currency Overlay, with a search | 46 / **669** |
| A rare Waystone (Tier 15) | 108 / 715 | POE2 Currency Overlay, with a search | 48 / **686** |
| An Uncut Skill Gem (Level 18) | 108 / **217** | POE2 Currency Overlay | 46 / 280 |
| A magic jewel | 93 / 683 | POE2 Currency Overlay, with a search | 47 / **638** |

- PoE2 Oracle priced the currency and the gem without a trade search, and was the first of the four
  apps to show their result: at 231 and 217 ms.
- The four items that need a trade search took PoE2 Oracle 683–731 ms and POE2 Currency Overlay
  638–792 ms; POE2 Currency Overlay was sooner with three of them. Most of the time goes to the trade
  site: in PoE2 Oracle's log, about 0.61 s of a 0.64 s search is spent waiting for pathofexile.com's
  two answers (see [Speed and size](#speed-and-size)).
- POE2 Currency Overlay's window came first, at 46–48 ms, but with the previous result in it until
  the new one arrived.
- PoE Overlay II didn't search the trade site. At 451–659 ms, its window having opened at
  435–529 ms, it showed its own estimate of the value, with a confidence of "very low" or "low",
  and none for the jewel.
- Exiled Exchange 2 opened its window at 93–114 ms and priced the currency and the gem at 917 and
  980 ms. With its Ctrl+Y check, the one used here, it doesn't search the trade site by itself: for
  the other four items it showed the filters and waited for Search.
- PoE2 Oracle's panel fades in: its first frame came at 92–109 ms, and the filters were all drawn
  by about 200 ms.

The CPU a check cost is what the app's processes used in the 13.3 seconds from the key press, less
what they use at rest in that time. Memory is Task Manager's Memory before the first check and after
the last.

| App | Processes / threads | CPU per check, ms: median (range) | Memory in Task Manager, MB: before → after | GPU memory, dedicated, MB |
|---|--:|--:|--:|--:|
| **PoE2 Oracle, commit 93296e4** | **1 / 43** | **164** (89–223) | **35.1 → 47.7** | **84 → 130** |
| Exiled Exchange 2 | 4 / 100–103 | 154 (144–331) | 66.9 → 89.5 | 0 |
| PoE Overlay II + Overwolf | 9 / 318–326 | 1050 (602–1285) | 285.2 → 320.7 | 39 |
| POE2 Currency Overlay | 4 / 145–153 | 212 (148–385) | 41.8 → 106.3 | 71–72 |

- Over the six checks, PoE2 Oracle's memory grew the least: by 12.6 MB, against 22.6 MB for Exiled
  Exchange 2, 35.5 MB for PoE Overlay II and 64.5 MB for POE2 Currency Overlay.
- Its dedicated GPU memory grew from 84 to 130 MB: once the panel has been shown, its window stays.

PoE2 Oracle wasn't the best at everything in the game. In the controlled test at rest, Exiled
Exchange 2 used a little less CPU: 0.081% of a core against 0.088%. Per check, Exiled Exchange 2 used
a little less CPU too, 154 ms against 164 at the median, with no trade search for four of the six
items. POE2 Currency Overlay showed its window first, with the previous result, and finished three of
the four trade searches sooner. And around the price checks PoE2 Oracle held the most dedicated GPU
memory of the four, 84 MB before them and 130 MB after, with the build before 5fc9b59; in real play,
with the build of 786f69b, its 68.1 MB was less than POE2 Currency Overlay's 72.3 MB and more than
Exiled Exchange 2's and PoE Overlay II's. See [Caveats](#caveats).

## With the game in the background

Measured on 26 September 2026, in three sessions, on the PC described under
[How it was measured](#how-it-was-measured). Path of Exile 2 was open in the background and in
view, and nobody played or checked a price. The other apps ran one at a time, each next to the game
and a build of PoE2 Oracle; the build the site shows was measured on its own, in the same state.
The site's comparison shows this memory as memory after launch: each app was measured a few minutes
after it started, waiting for a price check.

| App | Version | Built on | Measured at |
|---|---|---|---|
| PoE2 Oracle | 0.1.0, build of 26 September, 18:30 | Rust, GPUI on Direct3D 11 | 19:00, 19:01 and 19:03 |
| POE2 Currency Overlay | 3.0.7 | Electron | 17:32 |
| Exiled Exchange 2 | 0.16.7 | Electron | 14:18 |
| Sidekick | 2026.9.2 | .NET and WebView2 | 14:22 and 14:32 |
| PoE Overlay II | 1.67.0 | Overwolf 0.310.1.1 | 14:37, 14:39 and 14:43 |

Each time, in UTC+3, starts a minute of measuring. The four measurements the site shows:

| App, state | Processes | Memory in Task Manager, MB | Idle CPU, % of a core | GPU memory, dedicated, MB |
|---|--:|--:|--:|--:|
| **PoE2 Oracle** | **1** | **39.6** | **0.12** | **40.1** |
| POE2 Currency Overlay | 4 | 132.9 | 0.12 | 61.7 |
| Exiled Exchange 2 | 4 | 138.3 | 0.09 | 0.0 |
| Sidekick, ready to work | 7 | 267.0 | 0.40 | 75.3 |
| Sidekick, on its setup page | 7 | 157.8 | 0.15 | 82.5 |
| PoE Overlay II, main window open, 2–3 min | 9 | 663.0 | 109.69 | 38.0 |
| PoE Overlay II, main window open, 4–5 min | 9 | 654.1 | 112.45 | 38.0 |
| PoE Overlay II, main window minimized, 8 min | 8 | 349.7 | 1.87 | 38.0 |

The other counters, from the same readings:

| App, state | Threads | Working set, MB | Commit, MB | GPU memory, shared, MB |
|---|--:|--:|--:|--:|
| **PoE2 Oracle** | **56** | **94.2** | **162.6** | **1.0** |
| POE2 Currency Overlay | 161 | 376.8 | 292.8 | 4.5 |
| Exiled Exchange 2 | 107 | 389.7 | 584.3 | 0.0 |
| Sidekick, ready to work | 300 | 704.5 | 449.0 | 4.6 |
| Sidekick, on its setup page | 304 | 596.9 | 341.4 | 5.6 |
| PoE Overlay II, main window open, 2–3 min | 366 | 1,197.9 | 1,193.1 | 5.9 |
| PoE Overlay II, main window open, 4–5 min | 346 | 1,183.9 | 1,187.5 | 2.7 |
| PoE Overlay II, main window minimized, 8 min | 313 | 657.4 | 788.4 | 2.6 |

- Memory is in MB of 1024 × 1024 bytes, as in Task Manager. PoE Overlay II was measured 2–3 and
  4–5 minutes after it started, with its main window open, and 8 minutes after, with it minimized.
- **Memory in Task Manager** is the column Task Manager shows by default, **Memory**: the private
  working set, the RAM that belongs to a process alone. The site compares this one. For an app of
  several processes it is their sum; see [Caveats](#caveats) for why the working set isn't summed
  instead.
- **Commit** is Task Manager's **Commit size**, the process's private bytes: memory promised to it,
  in RAM or in the page file, whether it has used it or not.
- **Dedicated GPU memory** is on the graphics card; **shared GPU memory** is system RAM the graphics
  card uses for the process.
- **Idle CPU** is in percent of one logical core, so 100% is one core fully busy. This PC has 24,
  so Task Manager's CPU column shows a 24th of these numbers: 110% there reads about 4.6%.
- PoE2 Oracle's row is the median of its three windows, at 19:00, 19:01 and 19:03, on its own
  next to the game, 3.7 to 8.2 minutes after it started. They gave 39.6, 39.5 and 39.6 MB, and
  0.128, 0.122 and 0.116% of a core; its dedicated GPU memory was 71.8 MB in the first and 40.1 MB
  in the other two (see [Caveats](#caveats)).
- In those minutes PoE2 Oracle showed three windows over the game: the XP plate, its ⚙ and the map
  plate, which shows once a map has been played since logging in. Its earlier builds were measured
  with the first two.

PoE2 Oracle isn't the lowest in three columns. Idle CPU: Exiled Exchange 2 used less, 0.094%
against PoE2 Oracle's 0.122%; POE2 Currency Overlay's 0.119% is within the spread of PoE2 Oracle's
own three minutes, 0.116–0.128%; Sidekick (0.145% and 0.398%) and PoE Overlay II used more.
Dedicated GPU memory: Exiled Exchange 2 and PoE Overlay II held less, POE2 Currency Overlay and
Sidekick more. Shared GPU memory: Exiled Exchange 2, which draws without the graphics card, used
none. See [Caveats](#caveats).

> **The previous build.** POE2 Currency Overlay was measured next to the build of 26 September,
> 15:32, in the same state, with two windows over the game: the XP plate and its ⚙. The median of
> its two windows, on its own at 17:28 and beside POE2 Currency Overlay at 17:32: 1 process and 55
> threads, 37.5 MB in Task Manager, a working set of 88.6 MB, 224.1 MB of commit, 70.5 MB of
> dedicated and 0.8 MB of shared GPU memory, and 0.20% of a core. On its own at 17:19, while a
> maximized browser window covered the whole game and it hid its plates, it had 37.4 MB, a working
> set of 88.6 MB, 277.4 MB of commit, 70.5 and 0.2 MB of GPU memory, and 0.14% of a core; the other
> apps weren't measured in that state.

> **The earlier build.** Exiled Exchange 2, Sidekick and PoE Overlay II were measured next to an
> earlier build of PoE2 Oracle 0.1.0, from before its idle work was cut down. In the same state it
> had 1 process and 71 threads, 49.9 MB in Task Manager, a working set of 133.1 MB, 277.2 MB of
> commit, 159.6 MB of dedicated and 1.0 MB of shared GPU memory, and used 0.71% of a core: the
> median of seven windows, with 0.70–0.76% in each.

## Speed and size

PoE2 Oracle's timings come from the log of its earlier build on the same PC, over 18 price checks
on 26 September 2026: 5 in the log of one run and 13 in the next.

| From, to | Median | Range | Checks |
|---|--:|--:|--:|
| From the copy the app sends the game to the item's text: the game copying it, and the app looking at the clipboard every 5 ms | 12 ms | 6–19 ms | 18 |
| Reading the item, setting up its filters, showing the panel, starting the search | 6 ms | 0–31 ms | 18 |
| The search on pathofexile.com | 328 ms | 274–451 ms | 12 |
| Reading its answer, and fetching the listings from pathofexile.com | 285 ms | 275–517 ms | 12 |
| Reading the listings | 2 ms | 1–9 ms | 12 |
| From the item's text to the listings, all of it | 640 ms | 570–905 ms | 10 |
| From the item's text to the price of an exchange item, or of a search repeated from memory | 6 ms | 0–16 ms | 4 |

- Of the 640 ms, about 610 go to the two requests to pathofexile.com. PoE2 Oracle's own part is
  the copy (about 12 ms, most of it the game's), reading the item (about 6 ms) and reading the
  answers (about 2 ms).
- Two searches waited 3.4 and 2.8 seconds before they went out, and the rows for the search and for
  all of it leave them out: the app holds back a third search within five seconds while GGG's rate
  limit is one request away.
- Two searches found nothing and so fetched no listings: 274 and 341 ms from the item's text.
- The log doesn't note the time from the hotkey to the copy the app sends, nor the moment the panel
  shows on screen. Those, from the key press to the panel and the result on screen, were recorded
  on 27 September: see [Price checks](#price-checks).
- The build of 786f69b times each check in its log by its own clock, from the key press to the
  moment it shows the panel and draws the listings, so the screen shows each a frame or so later.
  Over the six checks the PC's owner made while playing on 27 September (see
  [Real play](#real-play)), the panel came 36–66 ms after the key press and the listings 628–788 ms
  after it, 591–743 ms of that on the search and the fetch from pathofexile.com. The site gives
  these times.

**Start-up.** The app was ready to price 0.27 s after its process started: its first log line came
36 ms after the process, and "ready to price items" 233 ms after that. The start before, right after
installing and with detailed logging on, took 0.38 s from its first log line, 0.14 s of it creating
the Direct3D 11 device. By the files' times, both starts found the trade site's catalogs saved on
disk; a very first start downloads them before it is ready, and neither log holds one.

**Size.**

- The installer, `PoE2-Oracle-Setup-0.1.0.exe`: 7,268,475 bytes (6.9 MB).
- The app it installs, `poe2-oracle.exe`: 28,299,776 bytes (27.0 MB). Installed with its
  uninstaller, the licenses and the third-party notices, it takes about 27.5 MB. The build of 18:30,
  whose memory and CPU with the game in the background are measured above, is an exe of
  28,505,600 bytes, from an installer of 7,321,898 bytes; the build of 5fc9b59 an exe of
  28,535,808 bytes; and the build of 786f69b an exe of 28,691,968 bytes, from an installer of
  7,383,172 bytes.
- Its cache, `%LOCALAPPDATA%\poe2-oracle\cache`, holds about 5.4 MB for the current league: the
  trade site's catalogs in English and Russian (3.0 MB), three hours of GGG's record of the Currency
  Exchange (2.3 MB) and poe2scout's prices (0.14 MB).

## How it was measured

**The PC.** Windows 11 Pro 64-bit (build 26200), in Russian. Intel Core i7-13700KF (16 cores, 24
threads), 32 GB of RAM with 15.2 GB free at the start, NVIDIA GeForce RTX 4090 with driver
32.0.16.1714. Two 3840 × 2160 monitors at 200% scaling, the main one at 60 Hz.

**The sessions with the game in the background, on 26 September.** From 14:14 to 14:45 (UTC+3),
Exiled Exchange 2, Sidekick and PoE Overlay II were measured next to the earlier build of PoE2
Oracle, which had been running since 12:07, with 13 price checks and a sign-in behind it. From 17:19
to 17:33, POE2 Currency Overlay was measured next to the build of 15:32, running since 17:03, and
that build on its own at 17:19 and 17:28. From 19:00 to 19:05, the build of 18:30 was measured on
its own three times; its installer had started it at 18:56.

**The game on 26 September.** Path of Exile 2 (Steam) ran through all three sessions on the main
monitor, in view but not the window in front: in front was a browser on the second monitor. PoE2
Oracle's plates were on screen over the game in every window but the one at 17:19: the XP plate and
its ⚙ in the first two sessions, and in the third the map plate too, as a map had been played since
logging in. In the first session the character stood in the hideout in away mode. The game was
started anew before the second session, with the owner at the PC: from 17:18 to 17:28 a maximized
browser window covered the whole game, which is when the 17:19 window was taken; in the rest of the
second session, from 17:28:41, and in all of the third, the game was in view and not covered at
all. The game's log got no new line during any of the measuring minutes.

**The session in the game, on 27 September.** From 08:37 to 09:25 (UTC+3), PoE2 Oracle's build of
commit 93296e4, POE2 Currency Overlay, Exiled Exchange 2 and PoE Overlay II ran at once, for
minutes at rest and with the mouse moving, and for the price checks. The game was in front for every
minute measured, the character in the hideout with the inventory open. From 09:40 to 09:46, the
minutes at rest and with the mouse moving were measured again with the build of 5fc9b59. Its
installer, run silently in the user's session with ordinary rights, closed the running PoE2 Oracle
and put in the new exe in 2.0 seconds; the app was then started as usual, at 09:39:57, and its
minute at rest began 2 min 53 s later. The owner took the mouse a few times during the session;
those minutes were thrown out and measured again.

**Real play, on 27 September.** Then the build of commit 786f69b was installed silently too, its
installer taking 2.0 seconds, and started at 10:20:09 (UTC+3). From 10:20 the PC's owner played, and
checked prices in PoE2 Oracle himself; from 10:32 to 10:37, three minutes were read with no input of
ours, beginning at 10:32:47, 10:35:00 and 10:36:13.

**The method, on 26 September.**

1. The other apps were started one at a time, as a player starts them: Exiled Exchange 2, PoE
   Overlay II and POE2 Currency Overlay from their desktop shortcuts (PoE Overlay II's has
   Overwolf's launcher start it), Sidekick from the Start menu shortcut its installer made. Each ran
   in the user's session with ordinary rights, and none took the focus.
2. Each was given 90 seconds to start, PoE Overlay II 120, since it starts in steps: Overwolf's
   launcher, its client, then its browser processes. Then came a minute of measuring: three readings
   20 seconds apart, with CPU counted over the whole minute. The tables give the median of the three
   readings. Memory moved less than 1.5% between them, but for PoE Overlay II with its window open:
   620–689 MB in the first window and 649–675 MB in the second.
3. In the second and third sessions, the state of the desktop was checked from inside the user's
   session before the measuring started, before every reading and at the end of each minute:
   whether the game ran, whether it was in front or minimized, how much of it other windows covered
   (which window is on top at 144 points across the game's window), which window was in front, and
   which windows PoE2 Oracle showed. A minute in which the state had changed would have been
   measured again; from 17:28:41 on it held in every reading. The 17:19 window came before the
   check could see other windows over the game: a screenshot at 17:20 and a check at 17:23 found the
   game covered by the browser, so that window is kept as a state of its own.
4. Each process was read the same way:
   - memory in Task Manager: the performance counter
     `Win32_PerfFormattedData_PerfProc_Process.WorkingSetPrivate`;
   - working set and commit: the process's `WorkingSet64` and `PrivateMemorySize64`;
   - GPU memory: the GPU Process Memory counters, `DedicatedUsage` and `SharedUsage`;
   - CPU: the process's cycle count (`QueryProcessCycleTime`) at the start and at the end of the
     minute, checked against its CPU times (`GetProcessTimes`), which agreed within 0.2 percentage
     points, 1.2 at PoE Overlay II's 110%;
   - threads: `Get-Process`.
5. An app's processes are every process whose exe is in its folder (for POE2 Currency Overlay, in
   its folder or its updater's), and every process those started: this is how Sidekick's WebView2
   processes, which run from Windows' own WebView2 folder, count. For PoE Overlay II, all of
   Overwolf's processes count, as it doesn't run without Overwolf. Each process was held open for
   the whole minute, so one that ended or started meanwhile counts too.
6. PoE2 Oracle was read the same way in every window, and on its own: at 14:14 the earlier build,
   at 17:19 and 17:28 the build of 15:32, and at 19:00, 19:01 and 19:03 the build of 18:30.

**The method, on 27 September.**

1. Memory and CPU were read from inside the owner's session, by hidden tasks that open no window,
   over each app's process tree: Task Manager's Memory (`WorkingSetPrivate`), the working set and
   private bytes, the GPU Process Memory counters, and the CPU cycle count
   (`QueryProcessCycleTime`) over 60 seconds, with three readings 20 seconds apart. PoE2 Oracle's
   threads were read over the same 60 seconds.
2. The price checks: six items, each in all four apps, in an order that shifted from item to item,
   PoE2 Oracle first. The pointer was put on the item with `SetCursorPos` and then a small real
   movement of the mouse through the Interception driver; the hotkey went through Interception too:
   Ctrl, the letter, the letter released after 60 ms and Ctrl after 200 ms. Before each key press, a
   check made sure the game was in front and the pointer in place.
3. ffmpeg recorded the screen at 60 frames a second (`ddagrab` with NVENC), each frame stamped with
   the wall-clock time (`setpts=RTCTIME-C`); the key press's time was taken on the same clock, so
   the times are counted in frames.
4. CPU and memory per check: the process tree's CPU cycles and private working set just before the
   key press and 13 seconds after.
5. Each app's window was closed its own way: PoE2 Oracle's and PoE Overlay II's with Esc, which each
   takes while its window is open; Exiled Exchange 2's with Esc while its window had the focus;
   POE2 Currency Overlay's with a click on its close button, once a check had made sure its window
   was under the pointer.
6. With the mouse moving: 60 seconds of slow circles over the inventory grid, with real input and no
   clicks.
7. Real play: the owner played as he usually does, and nothing of ours touched the PC's input. The
   three minutes were read as in step 1, all four apps in each, and every reading noted whether the
   game was in front: it was at every one. His play can't be repeated, so this is one session.

## Caveats

- **Task Manager's Memory and the working set.** Memory, the private working set, is a process's
  pages in RAM that belong to it alone: the fairest single number. The working set adds the pages it
  shares with others: DLLs, mapped files, and the memory Chromium's processes share among
  themselves. Summed over several processes, it counts the same pages once for each, and so
  overstates an app of many processes; PoE2 Oracle, with one, has nothing to overstate. Windows'
  own DLLs and the graphics driver, shared by every program, are in each working set and in no
  one's Memory. Commit is memory promised rather than used: PoE2 Oracle's 162.6 MB of it came with
  39.6 MB in RAM.
- **The game in front and in the background aren't compared with each other.** The numbers in the
  game were taken on 27 September with the game in front, those with the game in the background on
  26 September, and none are carried over from one to the other. Memory especially: POE2 Currency
  Overlay had 132.9 MB with the game in the background, and 41.8 MB in the game before its first
  check.
- **Three builds in the game.** The price checks, with the memory and GPU memory around them, were
  measured with commit 93296e4; the controlled test with 5fc9b59, which changed the XP overlay's
  screen watcher; real play with 786f69b, the commit after it, which left the screen watcher as it
  was.
- **Real play is one session.** The owner's play can't be repeated, and another session would give
  other numbers. All four apps were read in the same minutes, so they had the same play. The other
  three used more CPU in each minute than in the one before; three minutes don't tell whether that
  would have gone on.
- **Times to a frame: ±17 ms.** "Window" is the first frame that changed: for PoE2 Oracle, the start
  of its panel's fade-in. Exiled Exchange 2's "result" for the items that need a search is only its
  filters, as it didn't search.
- **CPU per check** covers 13 seconds of rest too, taken off by the measurement at rest; for
  PoE Overlay II that is about 195 ms, which widens its range.
- **PoE2 Oracle's memory before the checks** is that of the instance started at 08:43: 35.1 MB
  before its first check. An instance started at 08:12 read 14.4 MB at 08:38, as Windows had trimmed
  its working set, and isn't compared.
- **GPU memory** is Windows' own count per process. Around the price checks, PoE2 Oracle held the
  most dedicated GPU memory of the four, 84 MB before them and 130 MB after, with commit 93296e4. In
  real play, with commit 786f69b and its panel hidden, it held 68.1 MB, less than POE2 Currency
  Overlay's 72.3 MB; its own log put it at 83.7 MB while the panel was open.
  With the game in the background, PoE2 Oracle held more than Exiled Exchange 2 and PoE Overlay II,
  and less than POE2 Currency Overlay and Sidekick; the build of 15:32 held 70.5 MB, the earlier
  build 159.6 MB, the most of all. The build of 18:30 read 71.8 MB in the first of its three
  minutes: between 19:01:03 and 19:01:33, about 4.5 minutes after it started, its GPU memory dropped
  to 40.1 MB and stayed there. Its log noted nothing at the time, and why it dropped wasn't looked
  into; the table gives the median of the three minutes. The memory wasn't broken down by
  measurement; by GPUI's code, each window holds GPU memory in proportion to its size in pixels, so
  the number grows with the screen, and this PC runs 4K at 200%. Exiled Exchange 2 holds none: it
  turns off hardware acceleration (`app.disableHardwareAcceleration()`) and draws its overlay with
  the CPU. Overwolf's overlay in the game runs inside the game's own process and isn't counted for
  PoE Overlay II.
- **Idle CPU with the game in the background.** PoE2 Oracle used 0.122% of one core, the median of
  its three minutes, which ranged from 0.116 to 0.128%: more than Exiled Exchange 2 (0.094%), about
  as much as POE2 Currency Overlay, whose 0.119% is within that spread, and less than Sidekick
  (0.145% on its setup page, 0.398% ready to work) and PoE Overlay II. On this 24-thread CPU that is
  0.005% of the whole, which Task Manager's CPU column shows as 0. Its plates stay on screen over the
  game, while none of the other apps showed anything over it at the time. Split by thread in two of
  its minutes, about a third of it went to the graphics driver's threads, a third to Windows' thread
  pool, where GPUI runs its timers and background work, a quarter to its UI thread and a tenth to
  GPUI's vsync thread. The build of 15:32 used 0.20%, and 0.14% with the game covered and its plates
  hidden; the earlier build used 0.70–0.76%.
- **What was measured with the game in the background: idle.** Nobody opened an overlay or checked
  a price in those sessions; the price checks were measured in the game. POE2 Currency Overlay had
  its Price check tab open, as its settings kept it; on its Currency tab it asks for live rates
  every 6 seconds, which wasn't measured.
- **The states.** Exiled Exchange 2 opened no window: it waits in the tray and shows its overlay on
  <kbd>Shift</kbd>+<kbd>Space</kbd>. Sidekick ran on the settings of an earlier install, whose league
  had ended. At its first start it opened its setup window, asking for a league, and waited there:
  the "setup page" row, which anyone whose saved league is last season's will see. With the current
  league in its settings, its next start went through its whole start-up in 12 seconds with no
  window (quiet start was on in those settings): the "ready to work" row, the one to compare.
  PoE Overlay II, started from its shortcut, opened its main window by itself on the second monitor,
  without taking the focus: the character, a session summary, the stash, and beside them two
  animated banner ads, whose goods changed between screenshots 45 seconds apart, and two offers to
  go Premium to remove them. While that window is open, Overwolf's browser keeps repainting it: its
  GPU process used 92–99% of a core and the page's process another 10–13%. The window was then
  minimized with its own Minimize command and measured again. Closing it without quitting the app
  wasn't tried. POE2 Currency Overlay showed a splash at start, gone by the 30th second, and no
  first-run window, as its tutorial had been done before. It keeps one window over the game, always
  on top and letting clicks through, at zero opacity: it can't be seen until its hotkey makes it
  opaque. None of the others showed ads; POE2 Currency Overlay's window has a link to a Ko-fi
  donation page.
- **PoE Overlay II's Session Recap, in the game.** When the game lost the focus, PoE Overlay II
  opened its Session Recap window in the middle of the screen, 3158 × 1628, with banner ads and an
  offer of Premium: twice in 30 minutes. It covered the inventory, so one check of PoE2 Oracle and
  one of POE2 Currency Overlay went for nothing: the item was under that window. Both times the
  window was closed as its close button closes it.
- **Three builds with the game in the background.** The site sets the build of 18:30 against apps
  measured next to earlier builds: POE2 Currency Overlay next to the build of 15:32, the other three
  next to the earlier build, earlier the same day, with the game started anew between the first and
  second sessions. The state was the same: the game in view and not in front, PoE2 Oracle's plates
  on screen, every app idle.
- **The game's state and the plates matter.** When another window covers the game, PoE2 Oracle
  hides its plates and uses less CPU: the build of 15:32 used 0.14% so, against 0.20% with the game
  in view. The map plate shows only once a map has been played since logging in, and the build of
  18:30 was measured with it: three windows over the game, where the builds before had two. If
  anything, that counts against the build of 18:30. A minimized game is another state again, which
  wasn't measured.
- **The first minutes against hours.** With the game in the background, the other apps were measured
  1.5 to 8.5 minutes after they started; PoE2 Oracle's earlier build after more than two hours, the
  build of 15:32 after 16 to 30 minutes and the build of 18:30 after 3.7 to 8.2 minutes. Apps on
  Electron, CEF and .NET usually grow as their caches, heaps and history of checks fill, so for them
  these numbers are likely a lower bound. PoE Overlay II's were the same at 2–3 and at 4–5 minutes:
  not a start-up spike.
- **Exiled Exchange 2 0.16.7** is a build of the fork
  [mttzzz/Exiled-Exchange-2](https://github.com/mttzzz/Exiled-Exchange-2): Kvan7's
  [Exiled Exchange 2](https://github.com/Kvan7/Exiled-Exchange-2) 0.16.3 with an update channel and
  Russian translations added. The engine is the same, so its memory and CPU shouldn't differ.
- **POE2 Currency Overlay 3.0.7**, by POE2 VibeTools
  ([POE2-VibeTools/poe2-currency-overlay](https://github.com/POE2-VibeTools/poe2-currency-overlay)),
  was its latest release, and the installed app's code matches that release's source.
- **PoE Overlay II Standalone**, the version without Overwolf, wasn't installed on the PC and wasn't
  measured.
- **One PC, one run of each app.** Within a measurement the numbers held steady, but another day,
  league or cache size would give somewhat different ones.
