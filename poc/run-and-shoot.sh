#!/usr/bin/env bash
# POC screenshot/proof harness. Runs entirely inside the lane pod (`lane exec -- bash
# poc/run-and-shoot.sh`) -- never on the host, per this workstation's standing rule.
#
# IMPORTANT environment finding (see POC_FINDINGS.md for full writeup): this Xvfb build has no
# DRI3 extension, and Mesa's Vulkan X11 WSI (lavapipe included) needs DRI3+Present -- or a
# working MIT-SHM fallback -- to actually blit rendered frames onto the visible X11 framebuffer.
# GPUI/wgpu select the GPU adapter and configure a swapchain successfully (confirmed via
# `RUST_LOG=debug`), but the composited pixels never reach anything `XGetImage`-based
# (`import`, ImageMagick) can read back: every window, and even the full root screen, capture as
# a single uniform color regardless of actual rendered content. This is a well-documented,
# externally-corroborated Xvfb-only limitation (Mesa/lavapipe issue trackers, CI discussions),
# not a bug in this POC's code or in GPUI itself -- a real X.org session or a DRI3-capable setup
# (e.g. Weston+Xwayland) would not hit it. Screenshots are still captured below as supplementary,
# honestly-labeled artifacts; the primary proof for each capability instead uses X11
# protocol-level state (`xwininfo`, `xev` event delivery) or the example's own stdout, which are
# unaffected by this limitation.
set -uo pipefail
cd "$(dirname "$0")/.."

export XDG_RUNTIME_DIR=/tmp/xdg-runtime
mkdir -p -m 0700 "$XDG_RUNTIME_DIR"
export DISPLAY=:99
mkdir -p poc/screenshots poc/logs

# IMPORTANT: this writes to stderr, not stdout. launch() below is called via command
# substitution (`WIN=$(launch ...)`), which captures ONLY stdout -- any diagnostic writing to
# stdout inside/after that call silently corrupts the captured window id with log text.
log() { echo "[harness] $*" >&2; }

# --- X server + compositor bring-up (idempotent) ---
if ! xdpyinfo >/dev/null 2>&1; then
  log "starting Xvfb on $DISPLAY"
  setsid Xvfb "$DISPLAY" -screen 0 1280x800x24 >poc/logs/xvfb.log 2>&1 </dev/null &
  for _ in $(seq 1 30); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
fi
xdpyinfo >/dev/null 2>&1 || { log "FATAL: Xvfb never came up"; exit 1; }

if ! ps -eo comm | grep -qx xcompmgr; then
  log "starting xcompmgr (does not fix the DRI3 limitation above, kept for completeness)"
  setsid xcompmgr >poc/logs/xcompmgr.log 2>&1 </dev/null &
  sleep 0.3
fi

# every blocking X11 call below is timeout-guarded: several of these tools (xwininfo/xdotool
# selectwindow-style modes in particular) fall back to an interactive "click a window" prompt
# when given no/an invalid target, which blocks forever with no one there to click.
launch() {
  # launch <name> -- starts target/debug/examples/<name>, prints its window id on stdout (empty
  # on failure -- callers MUST check before using it). Polls plain `search` rather than
  # `search --sync`: `--sync` blocks for a *future* window-related X event, and if the window was
  # already created+named before this function started listening, that notification already
  # fired and is never seen again -- `--sync` then hangs the full 10s even though the window
  # already exists (reproduced empirically).
  local name="$1"
  setsid "./target/debug/examples/$name" >"poc/logs/$name.log" 2>&1 </dev/null &
  echo $! >"poc/logs/$name.pid"
  local win=""
  for _ in $(seq 1 40); do
    # Plain "$name", not the full "Oracle POC — $name": xdotool's --name regex match runs
    # under this container's C/POSIX locale (confirmed via `locale`), which mishandles the
    # multi-byte UTF-8 em-dash in the real title -- the window is real and correctly named
    # (visible via plain `getwindowname` enumeration) but the regex never matches it. Each
    # example's title is still unique enough to match on just its own name.
    win=$(xdotool search --name "$name" 2>/dev/null | tail -1)
    [ -n "$win" ] && break
    sleep 0.25
  done
  if [ -z "$win" ]; then
    log "ERROR: no window matched '$name' within 10s. All windows currently open:"
    xdotool search --name "" getwindowname %@ >&2 2>&1
  fi
  echo "$win"
}

stop() {
  local name="$1"
  [ -f "poc/logs/$name.pid" ] && kill -9 "$(cat "poc/logs/$name.pid")" 2>/dev/null
  rm -f "poc/logs/$name.pid"
}

safe_xwininfo() {
  # safe_xwininfo <window-id-or-empty> <outfile>
  if [ -z "$1" ]; then
    echo "SKIPPED: no window id" >"$2"
    return 1
  fi
  timeout 5 xwininfo -id "$1" >"$2" 2>&1
}

safe_shoot() {
  # safe_shoot <window-id-or-empty> <out.png>
  if [ -z "$1" ]; then
    log "SKIPPED screenshot $2: no window id"
    return 1
  fi
  timeout 5 import -window "$1" "poc/screenshots/$2" 2>>"poc/logs/import.err"
}

# ============================================================ capability 1: window_chrome
log "=== window_chrome (capability 1) ==="
# Dummy target window underneath, at window_chrome's own on-screen origin (100,100 -- see
# WINDOW_BOUNDS in examples/window_chrome.rs). xev's `-geometry` only places its *outer* frame;
# the actual event-catching area is a small fixed-size inner window xev creates at a (10,10)
# offset with a 50x50 size (confirmed empirically from xev's own CreateNotify log line) -- click
# tests below target that inner area's on-screen center, not the outer geometry's center.
setsid xev -geometry 420x320+100+100 >poc/logs/xev.log 2>&1 </dev/null &
echo $! >poc/logs/xev.pid
sleep 0.5
log "windows after starting xev: $(xdotool search --name '' getwindowname %@ 2>&1 | tr '\n' ' ')"

WIN=$(launch window_chrome)
log "window_chrome window id: [$WIN]"
safe_xwininfo "$WIN" poc/logs/window_chrome.xwininfo.txt
safe_shoot "$WIN" window_chrome_off.png

click_at() {
  # click_at X Y -- plain `mousemove --sync X Y click 1` hangs indefinitely in this environment
  # (reproduced: exit 124 under `timeout 3`) even though the position ends up correct; a plain
  # mousemove (no --sync) followed by a separate click reliably lands instead (reproduced: both
  # exit 0 and the click is observed by the target window).
  timeout 3 xdotool mousemove "$1" "$2" >/dev/null 2>&1
  sleep 0.1
  timeout 3 xdotool click 1 >/dev/null 2>&1
}

if [ -n "$WIN" ]; then
  CENTER_X=135
  CENTER_Y=135
  BEFORE=$(grep -c ButtonPress poc/logs/xev.log 2>/dev/null || echo 0)
  click_at "$CENTER_X" "$CENTER_Y"
  sleep 0.3
  AFTER_OFF=$(grep -c ButtonPress poc/logs/xev.log 2>/dev/null || echo 0)
  log "click while click-through OFF: xev ButtonPress count $BEFORE -> $AFTER_OFF (want: unchanged)"

  timeout 3 xdotool key --clearmodifiers t
  sleep 0.2
  safe_shoot "$WIN" window_chrome_on.png

  CLICKS_BEFORE_ON=$(grep -c WINDOW_CHROME_GOT_CLICK poc/logs/window_chrome.log 2>/dev/null || echo 0)
  click_at "$CENTER_X" "$CENTER_Y"
  sleep 0.3
  AFTER_ON=$(grep -c ButtonPress poc/logs/xev.log 2>/dev/null || echo 0)
  CLICKS_AFTER_ON=$(grep -c WINDOW_CHROME_GOT_CLICK poc/logs/window_chrome.log 2>/dev/null || echo 0)
  log "click while click-through ON: xev ButtonPress count $AFTER_OFF -> $AFTER_ON (want: incremented)"
  log "click while click-through ON: window_chrome's own click count $CLICKS_BEFORE_ON -> $CLICKS_AFTER_ON (want: unchanged -- window must NOT see it once click-through is on)"
  grep CLICK_THROUGH_STATE poc/logs/window_chrome.log || log "WARNING: no CLICK_THROUGH_STATE line seen"
fi

stop window_chrome
kill -9 "$(cat poc/logs/xev.pid)" 2>/dev/null
rm -f poc/logs/xev.pid

# ============================================================ capabilities 2+3: hotkey_clipboard
log "=== hotkey_clipboard (capabilities 2+3) ==="
MARKER="PoE2-Oracle-Clipboard-Test-$$"
printf '%s' "$MARKER" | xclip -selection clipboard
WIN=$(launch hotkey_clipboard)
log "hotkey_clipboard window id: [$WIN]"
safe_xwininfo "$WIN" poc/logs/hotkey_clipboard.xwininfo.txt
safe_shoot "$WIN" hotkey_clipboard_before.png

# No --window: must be a real XTEST-injected key so it exercises global-hotkey's XGrabKey path,
# not a synthetic event targeted at one client.
timeout 3 xdotool key --clearmodifiers ctrl+alt+o
sleep 0.5
safe_shoot "$WIN" hotkey_clipboard_after.png
grep "HOTKEY_FIRED" "poc/logs/hotkey_clipboard.log" || log "WARNING: no HOTKEY_FIRED line seen"

stop hotkey_clipboard

# ============================================================ capabilities 4+6: text_rendering
log "=== text_rendering (capabilities 4+6) ==="
WIN=$(launch text_rendering)
log "text_rendering window id: [$WIN]"
safe_xwininfo "$WIN" poc/logs/text_rendering.xwininfo.txt
sleep 1
safe_shoot "$WIN" text_rendering.png
stop text_rendering

# ============================================================ capability 5: trade_api
log "=== trade_api (capability 5) ==="
WIN=$(launch trade_api)
log "trade_api window id: [$WIN]"
safe_xwininfo "$WIN" poc/logs/trade_api.xwininfo.txt
for _ in $(seq 1 20); do
  grep -qE "TRADE_RESOLVED|TRADE_ERROR" "poc/logs/trade_api.log" 2>/dev/null && break
  sleep 0.5
done
safe_shoot "$WIN" trade_api.png
grep -E "TRADE_RESOLVED|TRADE_ERROR" "poc/logs/trade_api.log" || log "WARNING: trade_api never resolved"
stop trade_api

# Independent cross-check: hit the same three real endpoints directly and compare by eye against
# poc/logs/trade_api.log's TRADE_RESOLVED line.
log "=== independent curl cross-check of the same endpoints ==="
python3 - >poc/logs/trade_api_crosscheck.json <<'PYEOF'
import json, urllib.request, urllib.parse

UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"

def req(url, data=None):
    r = urllib.request.Request(url, data=data, headers={
        "User-Agent": UA, "Accept": "application/json", "Content-Type": "application/json",
    })
    with urllib.request.urlopen(r, timeout=20) as resp:
        return json.load(resp)

leagues = req("https://www.pathofexile.com/api/trade2/data/leagues")
league = leagues["result"][0]["id"]

body = json.dumps({
    "query": {
        "status": {"option": "online"},
        "stats": [{"type": "and", "filters": []}],
        "filters": {"type_filters": {"filters": {"category": {"option": "weapon.crossbow"}}}},
    },
    "sort": {"price": "asc"},
}).encode()
search = req(f"https://www.pathofexile.com/api/trade2/search/{urllib.parse.quote(league)}", data=body)

ids = ",".join(search["result"][:5])
fetch = req(f"https://www.pathofexile.com/api/trade2/fetch/{ids}?query={search['id']}")

print(json.dumps({"league": league, "total": search["total"], "first_item": fetch["result"][0]}, indent=2))
PYEOF
cat poc/logs/trade_api_crosscheck.json

log "=== done. Screenshots: poc/screenshots/, raw stdout/xwininfo logs: poc/logs/ ==="
