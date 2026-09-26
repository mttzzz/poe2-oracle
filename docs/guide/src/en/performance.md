# Performance

PoE2 Oracle is a native Windows app written in Rust: one process, with no Electron and no browser
inside, whose interface the graphics card draws. This page holds the measurements behind the
numbers on the [site](../../): PoE2 Oracle next to other price checkers, its own speed and size,
how they were measured and on which PC, and what the numbers don't tell.

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

## Next to other price checkers

Measured on 26 September 2026, in two sessions, on the PC described under
[How it was measured](#how-it-was-measured). Path of Exile 2 was open in the background and in
view, but for the one row marked as covered, and nobody played or checked a price. The apps ran
one at a time, each next to the game and PoE2 Oracle.

| App | Version | Built on | Measured at |
|---|---|---|---|
| PoE2 Oracle | 0.1.0, build of 26 September, 15:32 | Rust, GPUI on Direct3D 11 | 17:19, 17:28 and 17:32 |
| POE2 Currency Overlay | 3.0.7 | Electron | 17:32 |
| Exiled Exchange 2 | 0.16.7 | Electron | 14:18 |
| Sidekick | 2026.9.2 | .NET and WebView2 | 14:22 and 14:32 |
| PoE Overlay II | 1.67.0 | Overwolf 0.310.1.1 | 14:37, 14:39 and 14:43 |

Each time, in UTC+3, starts a minute of measuring. The four measurements the site shows:

| App, state | Processes | Memory in Task Manager, MB | Idle CPU, % of a core | GPU memory, dedicated, MB |
|---|--:|--:|--:|--:|
| **PoE2 Oracle** | **1** | **37.5** | **0.20** | **70.5** |
| PoE2 Oracle, the game covered, XP plates hidden | 1 | 37.4 | 0.14 | 70.5 |
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
| **PoE2 Oracle** | **55** | **88.6** | **224.1** | **0.8** |
| PoE2 Oracle, the game covered, XP plates hidden | 55 | 88.6 | 277.4 | 0.2 |
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
- PoE2 Oracle's row is the median of its two windows with the game in view: on its own at 17:28,
  and beside POE2 Currency Overlay at 17:32. They gave 37.6 and 37.4 MB, and 0.20% of a core both
  times.
- The row with the game covered is PoE2 Oracle on its own at 17:19, while a maximized browser
  window covered the whole game: PoE2 Oracle then hides its XP plates. The other apps weren't
  measured in that state, so the row is there for reference.

PoE2 Oracle isn't the lowest in three columns: idle CPU, where Exiled Exchange 2, POE2 Currency
Overlay and Sidekick on its setup page used less, and Sidekick ready to work and PoE Overlay II
more; dedicated GPU memory, where Exiled Exchange 2, PoE Overlay II and POE2 Currency Overlay held
less, and Sidekick more; and shared GPU memory, where Exiled Exchange 2, which draws without the
graphics card, used none. See [Caveats](#caveats).

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
  shows on screen.

**Start-up.** The app was ready to price 0.27 s after its process started: its first log line came
36 ms after the process, and "ready to price items" 233 ms after that. The start before, right after
installing and with detailed logging on, took 0.38 s from its first log line, 0.14 s of it creating
the Direct3D 11 device. By the files' times, both starts found the trade site's catalogs saved on
disk; a very first start downloads them before it is ready, and neither log holds one.

**Size.**

- The installer, `PoE2-Oracle-Setup-0.1.0.exe`: 7,268,475 bytes (6.9 MB).
- The app it installs, `poe2-oracle.exe`: 28,299,776 bytes (27.0 MB). Installed with its
  uninstaller, the licenses and the third-party notices, it takes about 27.5 MB. The build of 15:32,
  whose memory and CPU are measured above, is an exe of 28,449,280 bytes.
- Its cache, `%LOCALAPPDATA%\poe2-oracle\cache`, holds about 5.4 MB for the current league: the
  trade site's catalogs in English and Russian (3.0 MB), three hours of GGG's record of the Currency
  Exchange (2.3 MB) and poe2scout's prices (0.14 MB).

## How it was measured

**The PC.** Windows 11 Pro 64-bit (build 26200), in Russian. Intel Core i7-13700KF (16 cores, 24
threads), 32 GB of RAM with 15.2 GB free at the start, NVIDIA GeForce RTX 4090 with driver
32.0.16.1714. Two 3840 × 2160 monitors at 200% scaling, the main one at 60 Hz.

**Two sessions.** From 14:14 to 14:45 (UTC+3), Exiled Exchange 2, Sidekick and PoE Overlay II were
measured next to the earlier build of PoE2 Oracle, which had been running since 12:07, with 13
price checks and a sign-in behind it. From 17:19 to 17:33, POE2 Currency Overlay was measured next
to the build of 15:32, running since 17:03, and that build on its own at 17:19 and 17:28.

**The game.** Path of Exile 2 (Steam) ran through both sessions on the main monitor, in view but
not the window in front: in front was a browser on the second monitor. PoE2 Oracle's XP plates
were on screen over the game in every window but the one at 17:19. In the first session the
character stood in the hideout in away mode. The game was started anew before the second session,
with the owner at the PC: from 17:18 to 17:28 a maximized browser window covered the whole game,
which is when the 17:19 window was taken; from 17:28:41 to the end the game was in view and not
covered at all. The game's log got no new line during any of the measuring minutes.

**The method.**

1. The other apps were started one at a time, as a player starts them: Exiled Exchange 2, PoE
   Overlay II and POE2 Currency Overlay from their desktop shortcuts (PoE Overlay II's has
   Overwolf's launcher start it), Sidekick from the Start menu shortcut its installer made. Each ran
   in the user's session with ordinary rights, and none took the focus.
2. Each was given 90 seconds to start, PoE Overlay II 120, since it starts in steps: Overwolf's
   launcher, its client, then its browser processes. Then came a minute of measuring: three readings
   20 seconds apart, with CPU counted over the whole minute. The tables give the median of the three
   readings. Memory moved less than 1.5% between them, but for PoE Overlay II with its window open:
   620–689 MB in the first window and 649–675 MB in the second.
3. In the second session, the state of the desktop was checked from inside the user's session
   before the measuring started, before every reading and at the end of each minute: whether the
   game ran, whether it was in front or minimized, how much of it other windows covered (which
   window is on top at 144 points across the game's window), which window was in front, and which
   windows PoE2 Oracle showed. A minute in which the state had changed would have been measured
   again; from 17:28:41 on it held in every reading. The 17:19 window came before the check could
   see other windows over the game: a screenshot at 17:20 and a check at 17:23 found the game
   covered by the browser, so that window is kept as a state of its own.
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
   at 17:19 and 17:28 the build of 15:32.

## Caveats

- **Task Manager's Memory and the working set.** Memory, the private working set, is a process's
  pages in RAM that belong to it alone: the fairest single number. The working set adds the pages it
  shares with others: DLLs, mapped files, and the memory Chromium's processes share among
  themselves. Summed over several processes, it counts the same pages once for each, and so
  overstates an app of many processes; PoE2 Oracle, with one, has nothing to overstate. Windows'
  own DLLs and the graphics driver, shared by every program, are in each working set and in no
  one's Memory. Commit is memory promised rather than used: PoE2 Oracle's 224 MB of it came with
  37.5 MB in RAM.
- **GPU memory** is Windows' own count per process. PoE2 Oracle holds more dedicated GPU memory than
  Exiled Exchange 2, PoE Overlay II and POE2 Currency Overlay, and less than Sidekick; its earlier
  build held 159.6 MB, the most of all. It wasn't broken down by measurement; by GPUI's code, each
  window holds GPU memory in proportion to its size in pixels, so the number grows with the screen,
  and this PC runs 4K at 200%. Exiled Exchange 2 holds none: it turns off hardware acceleration
  (`app.disableHardwareAcceleration()`) and draws its overlay with the CPU. Overwolf's overlay in
  the game runs inside the game's own process and isn't counted for PoE Overlay II.
- **Idle CPU.** PoE2 Oracle used 0.20% of one core in both of its windows with the game in view:
  more than Exiled Exchange 2 (0.09%), POE2 Currency Overlay (0.12%) and Sidekick on its setup page
  (0.15%), less than Sidekick ready to work (0.40%) and PoE Overlay II. On this 24-thread CPU that
  is under 0.01% of the whole, which Task Manager's CPU column shows as 0. Its XP plates stay on
  screen over the game, while none of the other apps showed anything over it at the time; with the
  game covered and the plates hidden, PoE2 Oracle used 0.14%. Its earlier build used 0.70–0.76%.
- **What was measured: idle.** The game ran in the background, and nobody opened an overlay or
  checked a price. What a price check costs in memory or CPU wasn't measured. POE2 Currency Overlay
  had its Price check tab open, as its settings kept it; on its Currency tab it asks for live rates
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
- **Two builds, two sessions.** The site sets the build of 15:32 against apps measured in two
  sessions: POE2 Currency Overlay next to it, the other three next to the earlier build, earlier
  the same day and with the game started anew in between. The state was the same: the game in view
  and not in front, PoE2 Oracle's plates on screen, every app idle.
- **The game's state matters.** When another window covers the game, PoE2 Oracle hides its XP
  plates, and it used less CPU: the 17:19 row. A minimized game is another state again, which
  wasn't measured.
- **The first minutes against hours.** The other apps were measured 1.5 to 8.5 minutes after they
  started; PoE2 Oracle's earlier build after more than two hours, the build of 15:32 after 16 to 30
  minutes. Apps on Electron, CEF and .NET usually grow as their caches, heaps and history of checks
  fill, so for them these numbers are likely a lower bound. PoE Overlay II's were the same at 2–3
  and at 4–5 minutes: not a start-up spike.
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
