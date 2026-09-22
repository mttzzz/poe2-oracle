# GPUI Feasibility POC — Findings

Linux-only spike (X11). Pinned GPUI commit `b54cc1d0acc8fe3f7581721ee1195516e7581f9d`
(`zed-industries/zed`, `main`, 2026-09-22T03:48:24Z). All commands run through this project's
lane (`lane exec`); nothing built or run on the host. Harness: `poc/run-and-shoot.sh`. Raw
evidence: `poc/screenshots/*.png`, `poc/logs/*.log`, `poc/logs/*.xwininfo.txt`,
`poc/logs/trade_api_crosscheck.json`.

## Verdict: **GO**

All six capabilities either pass outright or are blocked *only* by a well-documented, narrowly
scoped headless-CI environment limitation (below) that does not apply to this tool's actual
target environment (a real user's desktop, with a real GPU and a real compositor/window
manager). No genuine GPUI/crate-level blocker was found for any capability. **Windows is
completely unverified** — everything below is Linux/X11 only. Real Win32 click-through styles,
DirectWrite Cyrillic shaping, and `global-hotkey`'s and GPUI's Windows backends were never
exercised and carry none of this spike's confidence.

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

## Files

- `examples/window_chrome.rs`, `examples/hotkey_clipboard.rs`, `examples/text_rendering.rs`,
  `examples/trade_api.rs`
- `src/platform/x11.rs`, `src/platform/mod.rs`, `src/lib.rs`
- `poc/run-and-shoot.sh`
- `poc/screenshots/*.png`, `poc/logs/*.log`, `poc/logs/*.xwininfo.txt`,
  `poc/logs/trade_api_crosscheck.json`
