FROM rust:1-bookworm

# xvfb: headless X11 server so GPUI windows can open/render without a real display.
# x11-utils / x11-xserver-utils: xdpyinfo, xrandr — sanity checks that the X server is up.
# xdotool: search/activate/focus windows and synthesize keys/clicks for the screenshot harness.
# xclip: set/read the X11 clipboard from the harness (the app itself uses GPUI's native
# App::read_from_clipboard(), backed internally by the `x11-clipboard` crate -- no arboard).
# imagemagick: `import -window <id>` captures a specific window to PNG.
# mesa-vulkan-drivers / libgl1-mesa-dri / libglx-mesa0 / libvulkan1: software Vulkan (lavapipe)
# so GPUI can render without a passed-through GPU device inside the pod.
# libxkbcommon-dev / libxkbcommon-x11-dev: gpui_linux's X11 backend links -lxkbcommon and
# -lxkbcommon-x11 for keymap/keysym translation; only discovered at the final `cc` link step of
# an actual binary/example (a lib-only `cargo build` never invokes that linker path).
# fonts-dejavu-core: DejaVu Sans covers both Latin and Cyrillic, needed for capability 4.
# xcompmgr: minimal X11 compositing manager. GPUI always creates depth-32 (ARGB) windows
# regardless of `WindowBackgroundAppearance` (confirmed via `xwininfo`); with no compositor
# registered, Xvfb's own implicit/automatic Composite-redirect path alpha-blends that ARGB
# buffer against the root window's black background, so *every* GPUI window screenshots as
# solid black via `import`/`XGetImage` regardless of actual rendered content (verified: GPU
# adapter selection and swapchain configuration both succeed per `RUST_LOG=debug` -- this is a
# compositing artifact, not a render failure). Running `xcompmgr` gives the ARGB window a real
# compositing manager that blends it correctly before the harness screenshots it.
# pkg-config / git / ca-certificates / curl: building crates and fetching the pinned GPUI git dep.
RUN apt-get update && apt-get install -y --no-install-recommends \
      xvfb x11-utils x11-xserver-utils xdotool xclip imagemagick xcompmgr \
      mesa-vulkan-drivers libgl1-mesa-dri libglx-mesa0 libvulkan1 \
      libxkbcommon-dev libxkbcommon-x11-dev \
      fonts-dejavu-core pkg-config git ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

# /tmp/.X11-unix normally gets created by root at boot (systemd-tmpfiles); this container has no
# boot sequence, and the non-root `runner` user can't create it under /tmp itself once dropped
# below root, so Xvfb falls back to a transport clients other than legacy Xlib may not accept.
# Pre-create it here as root with the standard 1777 (world-writable + sticky) permissions.
RUN mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix

RUN useradd -m -u 1000 -s /bin/bash runner
USER runner
ENV HOME=/home/runner
