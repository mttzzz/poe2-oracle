# GPUI Feasibility POC — Findings

Pinned GPUI commit `b54cc1d0acc8fe3f7581721ee1195516e7581f9d` (`zed-industries/zed`, `main`,
2026-09-22T03:48:24Z). All commands run through this project's lane (`lane exec`); nothing built
or run on the host. Two parts: a Linux/X11 spike (harness: `poc/run-and-shoot.sh`, evidence in
`poc/screenshots/`, `poc/logs/`) run interactively end-to-end, and a same-day Windows
cross-compile follow-up (evidence: build logs in this document, binaries shipped to
`/mnt/poe2/oracle-poc-windows/`) that builds and ships real `.exe`s but has **not** been run on
an actual Windows machine by this agent — no Windows execution access, only the SMB file share.

## Verdict: **GO** (Linux proven; Windows builds clean and ships, execution pending the user's
own hands-on test)

All six capabilities either pass outright on Linux or are blocked *only* by a well-documented,
narrowly scoped headless-CI environment limitation (below) that does not apply to this tool's
actual target environment (a real user's desktop, with a real GPU and a real compositor/window
manager). No genuine GPUI/crate-level blocker was found for any capability on Linux. The Windows
backend (`gpui_windows`) cross-compiles cleanly from this Linux lane via mingw-w64 with no MSVC
needed, and all four examples build and link into real, dependency-light Windows executables now
sitting in `/mnt/poe2/oracle-poc-windows/` — but **whether they actually behave correctly when
run is unverified**: this agent has file-share access to the Windows box, not execution access.
See "Windows" below for exactly what cross-compiling did and did not establish.

> **Update, same day, after this document was first written:** the Linux platform module
> (`src/platform/x11.rs`), the Xvfb harness (`poc/run-and-shoot.sh`), and its screenshots/logs
> have since been **removed** from the repo. Decision: this project ships to Windows
> exclusively for the foreseeable future, the Xvfb harness could never do more than
> behavioral/protocol-level proof anyway (see the DRI3 limitation below — it never delivered
> real pixel verification, which is exactly what capability 4/6 needed most), and dual-
> maintaining a platform backend + test harness for a target nothing ships to is pure cost. The
> Linux findings below are kept as-written, unedited, as the historical record of *why* the
> underlying GPUI approach was judged sound before committing to it -- the specific files they
> cite (`poc/logs/...`, `X11Overlay`) no longer exist in the tree. See "Windows" further down for
> the current, maintained state.

## Environment limitation (affects capabilities 1's pixel proof, 4, and 6)

This runner's Xvfb has no `DRI3` X extension (confirmed: `xdpyinfo -queryExtensions` lists
`Present`, `Composite`, `GLX`, `MIT-SHM` — no `DRI3`/`DRI2`). Mesa's Vulkan X11 WSI (including
lavapipe, the software Vulkan driver this sandboxed pod uses) needs DRI3+Present, or a working
MIT-SHM fallback, to blit rendered frames onto the real, screenshot-able X11 framebuffer. GPUI
and wgpu do their side correctly:

- GPU adapter selection succeeds: `Selected GPU adapter: "llvmpipe (LLVM 15.0.6, 256 bits)" (Vulkan)`.
- Swapchain configuration succeeds at the right size: `configuring surface with SurfaceConfiguration { format: Bgra8Unorm, width: 400, height: 480, present_mode: Fifo, ... }`.
- No `wgpu`/`gpui_wgpu` error or panic ever appears, with `RUST_LOG=debug` and an `env_logger`
  backend installed in every example specifically to surface `gpui_wgpu`'s `log::error!`/`warn!`
  calls (none fired).

But the composited pixels never reach anything `XGetImage`-based: **every** window, and even a
full-screen `import -window root` capture, comes back a single uniform color regardless of what
was actually drawn — confirmed with `identify -verbose` (`Colors: 1`) on multiple captures.
Installing `xcompmgr` (added to `lanes/runner.Dockerfile`) does not fix it: GPUI's own compositor
detection (`gpui_linux/.../client.rs::check_compositor_present`) also reports
`compositor present: false` even with `xcompmgr` alive, because that function interns
`_NET_WM_CM_S{root-window-id}` instead of the EWMH-correct `_NET_WM_CM_S{screen-number}` — a real
upstream bug, but a decorations-path detail (`x11/window.rs:1910-1913`), not the cause of the
black screenshots. This exact failure mode (Xvfb + lavapipe/Mesa X11 WSI without DRI3 → apps
render correctly internally but screenshot as blank) is independently corroborated by multiple
external sources (Mesa/lavapipe issue discussions, CI-infrastructure write-ups on `wsi_common_x11.c`'s
DRI3-or-Xshm branch, and general "Vulkan headless Xvfb black window" reports) — this is a known,
external, Xvfb-only artifact, not something introduced by this POC's code.

Two things follow from this and are folded into the per-capability verdicts below rather than
repeated: **screenshots in `poc/screenshots/` show uniform color for every example** (documented
per-capability, not hidden), and wherever a capability's actual proof does not require reading
pixels back through this specific pipeline (key/mouse/clipboard/network state, all provable via
the app's own stdout or raw X11 protocol state), that alternate proof is used instead and is
unaffected by this limitation.

## Capability 1 — Frameless + transparent-background + popup window, with click-through toggle

**PASS.**

Declarative half (`WindowOptions { titlebar: None, window_background: Transparent, kind: PopUp,
is_movable: false, .. }`, `examples/window_chrome.rs`): confirmed via `xwininfo`
(`poc/logs/window_chrome.xwininfo.txt`) — `Depth: 32`, `Visual Class: TrueColor`,
`Override Redirect State: yes`, correct `420x320` size at the requested position (off by the
constant +2px GPUI itself applies as a documented workaround for a Gnome/Ubuntu placement bug —
`gpui_linux/.../x11/window.rs:570-582` — not a bug in this POC).

Click-through half (raw X11 SHAPE extension via `poe2_oracle::platform::x11::X11Overlay`, since
`Window::set_input_region` is a Wayland-only no-op on GPUI's X11 backend): proven with a real,
two-sided test against a dummy `xev` target window sitting directly behind `window_chrome` at the
same screen coordinates, using real XTEST-synthesized clicks (`xdotool mousemove`+`click`, no
`--window` targeting so hit-testing is genuine, not a synthetic event aimed at one client):

```
click while click-through OFF: xev ButtonPress count 0 -> 0   (window intercepts; want unchanged — PASS)
click while click-through ON:  xev ButtonPress count 0 -> 1   (click passes through — want incremented — PASS)
click while click-through ON:  window_chrome's own click count 1 -> 1  (window stops seeing clicks — PASS)
```

(`poc/logs/xev.log`, `poc/logs/window_chrome.log`.) This event-delivery proof is a raw X11
protocol-level fact, entirely independent of the rendering/screenshot limitation above.

Not provable here, per the plan's own scope: **always-on-top** (`_NET_WM_STATE_ABOVE`) — Xvfb
runs with no window manager at all, so there is nothing to honor the hint or to stack against.
**Pixel-level transparency blending** — needs a real compositor; Xvfb has none. Both are the
plan's pre-named allowed exceptions, not new gaps.

Screenshots (`window_chrome_off.png`, `window_chrome_on.png`) show no legible content (see
environment limitation above); kept as supplementary artifacts only.

## Capabilities 2+3 — Global hotkey + native clipboard read

**PASS.**

`examples/hotkey_clipboard.rs` registers Ctrl+Alt+O via `global-hotkey` 0.8
(`GlobalHotKeyManager::register`, X11 backend = `XGrabKey` on the root window — focus-independent
by construction) and reads the X11 clipboard via GPUI's native
`App::read_from_clipboard()`/`ClipboardItem::text()` (no `arboard`).

Harness set a unique marker string via `xclip -selection clipboard`, then sent the combo with
`xdotool key --clearmodifiers ctrl+alt+o` — deliberately **without** `--window`, so it is a real
system-wide synthetic key event exercised through the X server's own grab dispatch, not a
targeted `SendEvent` aimed at one client:

```
HOTKEY_FIRED trigger_count=1 clipboard=Some("PoE2-Oracle-Clipboard-Test-3978")
```

(`poc/logs/hotkey_clipboard.log`.) The clipboard text matches the marker exactly, and the event
fired exactly once (an earlier version double-fired because `global-hotkey` queues both `Pressed`
and `Released` states on the same channel; fixed by filtering to `HotKeyState::Pressed`).

## Capabilities 4+6 — Cyrillic+Latin text rendering, price-check card layout

**Rendering pipeline verified operational; pixel-level visual confirmation blocked by the
Xvfb/DRI3 environment limitation above, not a code or GPUI defect.**

`examples/text_rendering.rs` renders a static price-check card (rarity-colored header, item
name, four Latin+Cyrillic-mixed modifier lines, price footer) with `.font_family("DejaVu Sans")`
set explicitly on the root div — required because the default `.SystemUIFont` resolves to Zed's
own bundled `"IBM Plex Sans"` on Linux, which is not installed in this runner image (confirmed
by reading `gpui_linux/.../platform.rs:171` and this project's own `fonts-dejavu-core`-only
Dockerfile). The window is created at the correct `400x480` size (`xwininfo`,
`poc/logs/text_rendering.xwininfo.txt`), and the same `RUST_LOG=debug` pass used for the
environment-limitation investigation shows the GPU/text pipeline running with no error through
adapter selection and swapchain configuration.

What is **not** established: actually eyeballing the Cyrillic glyphs, which the plan calls out
as the one output needing real visual inspection, not just "did it crash." `text_rendering.png`
was read back with the `read` tool's image support per the plan's instruction and shows a single
uniform color, not glyphs — an honest negative result, reported as such rather than asserted as
a pass. DejaVu Sans has complete, mature Cyrillic coverage and cosmic-text/rustybuzz (GPUI's
shaping stack) are general-purpose, script-agnostic shapers with no Latin-only restriction
anywhere in the code read for this spike, so there is no specific reason to expect a failure on
real hardware — but that is an inference, not a verified result, and is reported as one.

**Post-hoc correction (Windows pass):** the Linux example forced `.font_family("DejaVu Sans")`,
chosen for that runner image's installed fonts. DejaVu Sans is not a standard Windows font and
almost certainly is not installed on a real Windows machine — shipping that override as-is would
have actively risked breaking the one thing this capability exists to test. Fixed before shipping
to `/mnt/poe2`: `examples/text_rendering.rs` no longer overrides the font family at all. On
Windows, `.SystemUIFont` resolves via `SystemParametersInfoW`
(`gpui_windows/src/direct_write.rs::get_system_ui_font_name`) to the user's actual configured UI
font, falling back to `"Segoe UI"` — Microsoft's own flagship UI font, with complete Cyrillic
coverage, installed on every Windows machine since Vista. The default is the correct choice here,
not an override.

## Capability 5 — Real async trade API call, loading → resolved re-render

**PASS**, with a rigorous independent cross-check.

`examples/trade_api.rs` performs the real, three-endpoint flow used by Exiled Exchange 2's own
trade integration (`renderer/src/web/price-check/trade/pathofexile-trade.ts`,
`.../background/Leagues.ts`, read from the canonical checkout at
`~/projects/exiled-exchange-2/renderer/src/web/...`): `GET /api/trade2/data/leagues`, `POST
/api/trade2/search/{league}` (crossbow category, sorted by price), `GET
/api/trade2/fetch/{ids}?query={id}` — through `http_client`/`reqwest_client` (Zed's own wrapper;
no direct `reqwest`, no `gpui_tokio`), with a `Loading → Resolved` state machine re-rendering on
completion via `cx.spawn` + `Entity::update`.

The harness's own run and an independent, separately-executed Python `urllib` call to the exact
same three endpoints, moments apart, were compared directly:

```
app (stdout):  TRADE_RESOLVED league="Forbidden Rites" total=3490 rows=5
               first=("Mind Core (Alloy Crossbow)", "1 transmute", "nivon#0926")
cross-check:   league "Forbidden Rites", total 3490,
               first_item name "Mind Core" / typeLine "Alloy Crossbow" / price "1 transmute" / account "nivon#0926"
```

(`poc/logs/trade_api.log`, `poc/logs/trade_api_crosscheck.json`.) League, total listing count,
and the full identity/price/account of the top-priced result match exactly. This capability's
proof does not depend on the rendering/screenshot pipeline at all — the network round trip and
JSON parsing are the proof.

# Windows (cross-compiled from Linux, same day — not yet run on real hardware)

No plan text ever covered Windows in detail (it was explicitly out of scope for the Linux
spike); this section documents what was actually done and verified on this pass, and draws the
line precisely at what remains unverified.

## Toolchain: mingw-w64 cross-compilation, no MSVC

`gpui_platform`'s Windows backend is a real, separate crate (`gpui_windows`, parallel to
`gpui_linux`), auto-selected via `[target.'cfg(target_os = "windows")'.dependencies]` — no
feature flag needed on our side, unlike Linux's x11/wayland choice. Added to
`lanes/runner.Dockerfile`: `gcc-mingw-w64-x86-64`/`g++-mingw-w64-x86-64` +
`rustup target add x86_64-pc-windows-gnu`, plus `.cargo/config.toml` pointing that target's
linker at `x86_64-w64-mingw32-gcc`. With just that, the **entire** `gpui_windows` stack —
DirectComposition, DirectWrite, Direct3D 11, `accesskit_windows`, the official `windows` crate
(0.62.2) — cross-compiles cleanly from this Linux lane. No MSVC, no Windows SDK, no `cargo-xwin`
needed for compiling. (One thing does need a real Windows SDK at *build* time — release-mode
shader precompilation — see below.)

## Making the crate cross-platform (as of the mingw-w64 pass; later simplified to Windows-only)

At the point this was written, both `x11.rs` and `win32.rs` existed side by side, `Cargo.toml`
had matching `[target.'cfg(target_os = "linux")']`/`[target.'cfg(target_os = "windows")']`
dependency sections, and `window_chrome.rs` picked between `X11Overlay`/`Win32Overlay` via a
`#[cfg]`-aliased `PlatformOverlay` name. After the decision to drop Linux (see the note at the
top of this document), `x11.rs`, `x11rb`, and the alias were all removed; `window_chrome.rs` now
names `Win32Overlay` directly. One thing survives from that pass regardless of platform count:
a real, reproducible cross-target bug found and fixed. `raw_window_handle::HandleError`'s
`impl std::error::Error` is gated behind that crate's own `std` feature (its `src/lib.rs`).
Something in the (now-removed) Linux dependency graph happened to unify that feature on; nothing
on Windows did. Result: `.context()` on `Result<WindowHandle, HandleError>` compiled fine on
Linux and failed with an unsatisfied-trait-bounds error on Windows, for byte-identical code.
Fixed by requesting `raw-window-handle = { version = "0.6", features = ["std"] }` explicitly --
harmless and correct regardless of target count.

## `src/platform/win32.rs`

Every constant, struct layout, and function signature (`HWND(pub *mut c_void)`, `GWL_EXSTYLE`,
`WS_EX_LAYERED`/`WS_EX_TRANSPARENT`, `HWND_TOPMOST`/`HWND_NOTOPMOST`,
`SetWindowLongPtrW`/`SetWindowPos`) was checked against the real downloaded `windows` 0.62.2
crate source in this lane's cargo registry cache before writing code against it. Two things fall
out of reading `gpui_windows/src/window.rs` itself, not just the `windows` crate:

- **Always-on-top is free on Windows.** `WindowKind::PopUp` already sets
  `WS_EX_TOOLWINDOW | WS_EX_TOPMOST` at `CreateWindowExW` time (`window.rs:490`) — genuinely
  native always-on-top, unlike X11 where override-redirect popups aren't WM-stacked at all and
  the EWMH hint was only ever a best-effort ask nothing in Xvfb could even honor.
  `set_always_on_top` exists in `win32.rs` as a runtime-toggle escape hatch, not because the
  static case needs it.
- **Click-through genuinely needs raw code.** `gpui_windows`'s `CreateWindowExW` call never sets
  `WS_EX_LAYERED`/`WS_EX_TRANSPARENT` for any `WindowKind` — confirmed by reading its
  `dwexstyle` construction directly, not inferred. `win32.rs` toggles both via
  `SetWindowLongPtrW(GWL_EXSTYLE, ...)`, the standard recipe.

**What this establishes and what it doesn't**: the code compiles against real, version-matched
API signatures and follows the standard, widely-documented Win32 click-through recipe — but it
has never actually run. Whether `WS_EX_TRANSPARENT` alone is sufficient on a DirectComposition
window (`gpui_windows` sets `WS_EX_NOREDIRECTIONBITMAP` for its own GPU-composited rendering
path, a different, newer mechanism than the classic GDI layered-window path `WS_EX_LAYERED`
historically implies) is exactly the kind of thing that looks right on paper and needs a real
click, on a real desktop, to actually confirm. That is squarely what `window_chrome.exe`'s manual
test (see `/mnt/poe2/oracle-poc-windows/README.txt`) is for.

## Release-mode limitation: shader precompilation needs a real Windows SDK

`gpui_windows`'s build script only precompiles HLSL shaders (`fxc.exe`, located via
`GPUI_FXC_PATH`, `where.exe`, or the Windows registry's installed-SDK key) when
`debug_assertions` is off, and only inside a block additionally gated by
`#[cfg(target_os = "windows")]` — which build scripts evaluate against the *build host*, not
`--target`, so cross-compiling from Linux means neither the fxc lookup nor a real Windows
registry/SDK is ever reachable regardless of profile. A genuine `cargo build --release` for this
target fails outright:
```
error: couldn't read ".../release/build/gpui_windows-.../out/shaders_bytes.rs": No such file or directory
```
`cargo build` (dev/debug) works precisely because `debug_assertions = true` skips shader
precompilation entirely and falls back to compiling shaders **at runtime** via
`D3DCompileFromFile` (confirmed as a real runtime dependency of the built `.exe` via
`objdump -p` — `d3dcompiler_47.dll`, part of every Windows install with DirectX, i.e. any machine
that already runs PoE2). Debug binaries are enormous, though (~400-470MB per example,
unoptimized + full debug info) — too large to comfortably ship. Fix: a custom
`[profile.dist]` in `Cargo.toml` that `inherits = "dev"` (keeping `debug_assertions = true`, so
the fxc path stays inert and shaders still compile at runtime) but adds `opt-level = 2` and
`strip = true`. Result: 23-31MB per example, a ~16x reduction, with identical behavior to debug
as far as this limitation goes. This is a real, load-bearing constraint for anyone doing CI/CD
Windows builds of a GPUI app from Linux — a true release build needs either a Windows build
machine, or a Wine-hosted Windows SDK/fxc.exe, or an actual DXC-based shim; none of that was
attempted here as out of scope for a feasibility spike.

## Shipped artifacts

`/mnt/poe2/oracle-poc-windows/`: `window_chrome.exe`, `hotkey_clipboard.exe`,
`text_rendering.exe`, `trade_api.exe` (all `[profile.dist]`, `x86_64-pc-windows-gnu`, PE32+
console subsystem — each opens a console alongside its window showing `println!` diagnostics
(`CLICK_THROUGH_STATE`, `HOTKEY_FIRED`, `TRADE_RESOLVED` etc., the same markers the now-removed
Linux harness used to grep for), plus `README.txt` (Russian, matching how this project's
owner communicates) with per-exe manual test steps. No mingw runtime DLLs are needed
(`libgcc`/`libstdc++`/`libwinpthread` are statically linked by this Rust target by default,
confirmed absent from `objdump -p`'s import table) — only standard Windows system DLLs
(`user32`, `d3d11`, `dwrite`, `dcomp`, `dwmapi`, `d3dcompiler_47`, `ws2_32`, etc.), all present on
any Windows 10/11 machine capable of running PoE2 itself.

## What is still genuinely unverified

Everything that requires actually running the binaries: whether the window opens and looks
right, whether `T` really toggles click-through and a click really passes through
(`WS_EX_TRANSPARENT` interacting correctly with `gpui_windows`'s DirectComposition-based
rendering is inferred from reading both codebases, not observed), whether `global-hotkey`'s
Windows backend (`RegisterHotKey`) actually delivers Ctrl+Alt+O system-wide, whether
`App::read_from_clipboard()` round-trips real clipboard content on Windows, whether DirectWrite
actually shapes and rasterizes the Cyrillic text correctly (this is the one capability that
*needed* Windows to test at all, since it's the platform PoE2 Oracle would actually ship on), and
whether the real trade-API flow behaves identically (it should — that code path has zero
platform-specific branches — but "should" is not "observed"). This agent has SMB file-share
access to the Windows box (`/mnt/poe2`), not execution access; the user runs these by hand next
and reports back per `README.txt`.

## Deviations from the plan's literal text (carried over from the prior session's handoff, still accurate)

- `arboard` dropped: GPUI's native `App::read_from_clipboard()` is used instead.
- Direct `reqwest` dropped: all HTTP goes through `http_client`/`reqwest_client`.
- `gpui_tokio` never needed: `ReqwestClient` manages its own background runtime.
- `examples/window.rs`/`window_positioning.rs` do exist at the pinned commit (autodiscovered by
  Cargo, not listed in gpui's own `Cargo.toml`).

## New findings from this session, for whoever picks this up next

- **Runner image needed two additions**, both now in `lanes/runner.Dockerfile`:
  `libxkbcommon-dev`/`libxkbcommon-x11-dev` (missing `-l` link targets, only surfaced once an
  actual example — not just a lib build — reached the final link step), and `xcompmgr` (does not
  fix the DRI3 limitation, kept only because it was a reasonable thing to try and costs nothing).
- **`XDG_RUNTIME_DIR` must be set** to a real, `0700` directory before running any GPUI binary in
  this container, or startup silently stalls past window creation (`error: XDG_RUNTIME_DIR is
  invalid or not set`); the harness creates `/tmp/xdg-runtime` itself.
- **`xdotool search --sync`** is unreliable here: it blocks for a *future* window-related X
  event, and if the window was already created+named before the search call starts listening
  (common — GPUI opens windows fast), that notification already fired and is never seen again,
  so it hangs the full timeout even though the window already exists. The harness polls plain
  `search` in a retry loop instead.
- **`xdotool search --name` runs under this container's `C`/`POSIX` locale** (confirmed via
  `locale`) and cannot match the real window title's UTF-8 em dash ("Oracle POC — window_chrome")
  even though the title is set correctly (visible via `getwindowname`) — the harness searches on
  each example's plain ASCII name instead ("window_chrome", not "Oracle POC — window_chrome").
- **`xdotool mousemove --sync X Y click 1` hangs** (reproduced, `timeout 3` kills it with exit
  124) even though the pointer does end up at the right position; a plain `mousemove` (no
  `--sync`) followed by a separate `click` command reliably lands instead.
- **`xterm` is not installed** in the runner image (only `xvfb x11-utils x11-xserver-utils
  xdotool xclip imagemagick xcompmgr`). Not needed: `XGrabKey`-based global hotkeys are
  focus-independent by design, so proving "system-wide, not scoped to our own window" only
  requires *not* using `xdotool key --window`, not a second focused foreground app.
- **`xev`'s `-geometry` only places its outer frame**; the actual event-catching area is a small
  fixed `50x50` inner window at a `(10,10)` offset (confirmed via `xwininfo -tree`), not the full
  outer geometry — matters for anyone reusing `xev` as a click-delivery probe.

## Files (current tree; `poc/` and `src/platform/x11.rs` no longer exist -- see the archival note)

- `examples/window_chrome.rs`, `examples/hotkey_clipboard.rs`, `examples/text_rendering.rs`,
  `examples/trade_api.rs`
- `src/platform/win32.rs`, `src/platform/mod.rs`, `src/lib.rs`
- `Cargo.toml` (Windows-only deps, `[profile.dist]`), `.cargo/config.toml` (windows-gnu cross
  linker), `lanes/runner.Dockerfile` (mingw-w64 + rustup target)
- `/mnt/poe2/oracle-poc-windows/*.exe`, `/mnt/poe2/oracle-poc-windows/README.txt` (Windows
  binaries + manual test instructions, outside this repo, on the shared Windows drive)
