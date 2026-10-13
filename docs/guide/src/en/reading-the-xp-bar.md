# Reading a game's XP bar without touching its memory: DXGI desktop duplication in Rust

I wrote this article with AI help. The code is in the repository, and every number below comes from the project's guide, its changelog or the source itself.

PoE2 Oracle is a free, open-source price checker and XP overlay for Path of Exile 2, for Windows, written in Rust. The overlay puts plates on the game's HUD that say how fast you level and when the next level comes. For that, the app has to know how full the game's experience bar is.

## Pixels, not memory

The app reads the bar from the screen. It never touches the game's memory. The bar is on the screen for the player, so it is there for the app too. The guide's [privacy page](privacy.md#what-it-reads-on-your-computer) lists what the app reads on your PC, including two files of the game (its settings file and its log, both read only) and the screen. The game's memory is not on the list.

## From a screen blit to desktop duplication

The plates stand on two rails of the HUD. When the game draws a tooltip, a panel or the chat over a rail, the plate has to step aside. To tell, a watcher thread takes a look at a few rows of each rail's top edge, its lip. The same look reads the bar.

The older way is a GDI blit of the screen. It waits for the desktop's next composition: on the test machine's 4K game, 11 ms for one lip, 27 ms for both, up to 145 ms. That is fine every two seconds and too slow for many looks a second.

With DXGI desktop duplication, Windows hands the app each frame it composes, as a Direct3D texture. The app copies a few rows out of it on the GPU and reads back only those. In `lip_watch.rs`, `Duplication::open` picks the output under the middle of the game's client area, makes a Direct3D 11 device on its adapter and calls `DuplicateOutput1`. It asks for 8-bit BGRA only. The code comment says that is what comes out whatever the desktop is, HDR included.

Three rectangles are copied: the two lips and the bar's fill band with its tick stems (1536 by 10 pixels on the 4K test machine). All are in physical pixels, like the monitor's own rectangle. The code assumes the HUD scales with the game's height, so each is worked out at 2160 rows and scaled. `CopySubresourceRegion` puts them into one small staging texture, the lips first. This is the core of one look, trimmed from `Duplication::look`:

```rust,noplayground
// …
match unsafe {
    self.duplication
        .AcquireNextFrame(wait, &mut info, &mut resource)
} {
    Err(err) if err.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(()),
    other => other.context("AcquireNextFrame")?,
}
self.framed = true;
// A present time of zero: only the pointer moved.
let copied = match resource {
    Some(frame) if info.LastPresentTime != 0 => {
        self.copy(slot, &frame, lips, bar, game, now).map(Some)
    }
    _ => Ok(None),
};
// Windows holds the next frame back until this one is released. The copies are queued
// on the GPU before the release, so they read this frame.
unsafe { self.duplication.ReleaseFrame() }.context("ReleaseFrame")?;
// …
```

`wait` is 100 ms until a new duplication has its first frame, and zero after that. The code doesn't use dirty or moved rectangles.

## Never waiting for the GPU

A staging texture must be mapped before the CPU can read it. An earlier build mapped it right after queueing the copy, and `Map` then waited inside the graphics driver, on the watcher's thread, for the GPU to get to the copy behind the game's own work. Measured on 27 September 2026 with the game in front and idle, that thread took 1.45 ms of CPU a look, at four looks a second. It also made four context switches a look: three for the input poll and one for that wait.

Now `Map` is tried 4 ms after the copy, with `D3D11_MAP_FLAG_DO_NOT_WAIT`:

```rust,noplayground
while let Some(slot) = self.oldest() {
    // …
    match unsafe {
        self.context.Map(
            &texture,
            0,
            D3D11_MAP_READ,
            D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
            Some(&mut mapped),
        )
    } {
        Err(err) if err.code() == DXGI_ERROR_WAS_STILL_DRAWING => break,
        other => other.context("Map")?,
    }
    // …
}
```

If the GPU isn't done, the thread tries again later, the wait doubling up to 50 ms. Three staging textures take turns, and a look that finds all three busy is dropped.

## When it looks, and when it doesn't

The watcher is a plain thread, and each report it sends over an `async_channel` wakes the UI thread, so it reports only when a rail is seen or missed where it wasn't before. The bar's reading changes as experience comes in, so it skips the channel: it sits in an `AtomicU64` as the bits of an `f64`, with `u64::MAX`, a NaN pattern no fill has, meaning none. The sampler takes it every two seconds.

The thread watches only while the game is in front and not minimised, and for two seconds after, so a quick click elsewhere keeps the duplication. It doesn't poll for the foreground: Windows reports each change. Otherwise it sleeps, and the duplication and its device are dropped with their video memory, 32 MiB for the monitor's frame at 4K. While the game is behind another window, the older blit takes over, every two seconds for the rails and every ten for the bar.

While it watches, the pace follows the player:

- A look every 50 ms while the mouse moves or keys are pressed, and until the player has kept still for 250 ms.
- Then each look waits as long as the player had kept still at the last one, up to 2 seconds.
- A covered rail is looked at every 250 ms for 10 seconds after the last input.

The bar is read in a look at most every 500 ms. The game presents far more often: a look at each of its frames cost 4% of a core, measured at 77 frames a second. At 50 ms a tooltip over a rail is read within about 55 ms of the frame that shows it.

Between looks the thread sleeps on a high-resolution waitable timer. While the player keeps still, raw input also ends the sleep. The thread registers the mouse and the keyboard with `RIDEV_INPUTSINK`, to a message-only window of its own. At the first report it unregisters with `RIDEV_REMOVE`, so a moving mouse's hundreds of reports a second never reach it. It learns that the player did something, never which key or where. The game's own input is untouched. A low-level mouse hook would put every mouse report through the app first. If raw input doesn't arrive, as may happen with a game run as administrator, the thread asks `GetLastInputInfo` every 100 ms instead.

## What it costs

The performance page compares the build with the asynchronous read-back and the raw-input wake-up (commit 5fc9b59) with the one before it. The PC: an NVIDIA RTX 4090, two 3840 × 2160 monitors at 200% scaling, the game in front. At rest the whole process used 53 ms of CPU a minute, and 6.8 ms of that was the watcher. With the mouse moving, the watcher took 158 ms a minute. The build before had the watcher alone at 347 to 399 ms a minute at rest and 3199 ms with the mouse moving. Against it, PoE2 Oracle used about 8 times less CPU at rest and 16 times less with the mouse moving. It is one PC and one session: the [performance page](performance.md#how-it-was-measured) has the method and the caveats.

## What goes wrong

- **Two monitors.** One duplication covers one output. If the lips and the bar aren't all on the monitor it duplicates, the watcher logs the reason once and leaves the plates to the blit.
- **A lost duplication.** A mode change, the secure desktop of a UAC prompt or another program's exclusive fullscreen ends it. The watcher opens a new one on the next round, and waits 3 seconds after a failure.
- **A window over the bar.** The duplicated desktop is the composed one, price panel included, and a cover that passes the fill checks would read as a wrong fill. So the app asks `WindowFromPoint` at nine points along the bar and wants the game's window at all of them.
- **Other layouts.** The rails' positions are confirmed only at 16:9. The game's other HUD layouts leave a plate off.

## Reading the fill

The bar has 20 segments with 19 ornamental ticks, whose dark stems hang below the fill. `read_fill` returns the fraction of the level, or `None` unless the bar is unmistakably what is on screen. At least 17 of the 19 stems must be in place. The track must show a bright, warm start at least twice as bright as the rest, with 3% of the columns allowed to stray, or no fill and a dark track. The fill is told from the track by relative brightness, not by colour or a fixed level. A tooltip, panel or loading screen over the bar reads `None`, never fuller than the bar is.

Pixels failed twice in ways players saw. With the pointer resting on the bar, its tooltip made the bar read as nearly full: 94.75% for 40 seconds where it showed 59.33%. A level 93 player saw +250 to 350%/h for half an hour, until 0.1.1. Now readings are set aside while the pointer is near the bar and for a second after. A warm light in the scene made the empty part read as filled: a level 93 player saw +186%/h where the real rate was about +40%/h, until 0.1.3.

## From a fill to XP per hour

The tracker takes the reading every two seconds and reads the game's log for level-ups and area changes. The rate is experience per hour of play, in percent of the current level. It weighs recent play more: with Rate smoothing at its default of 10 minutes, play from 10 minutes ago counts half as much as play now. Time in towns and hideouts doesn't count. The time to the next level is the playing time left at that rate. The [XP overlay guide](xp-overlay.md#how-it-works) has the rest.

## Source

The code is [lip_watch.rs](https://github.com/mttzzz/poe2-oracle/blob/main/crates/poe2-oracle/src/platform/lip_watch.rs) in the [repository](https://github.com/mttzzz/poe2-oracle). The app is at [oracle.pushka.biz](../../).

PoE2 Oracle is a fan-made tool. This product isn't affiliated with or endorsed by Grinding Gear Games in any way. Path of Exile is a trademark of Grinding Gear Games.

English isn't my first language, so I write and code with AI help (Claude). I design it, test every release in my own play, and fix what you report.
