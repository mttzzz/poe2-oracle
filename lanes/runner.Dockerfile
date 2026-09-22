FROM rust:1-bookworm

# xvfb: headless X11 server so GPUI windows can open/render without a real display.
# x11-utils / x11-xserver-utils: xdpyinfo, xrandr — sanity checks that the X server is up.
# xdotool: search/activate/focus windows and synthesize keys/clicks for the screenshot harness.
# xclip: set/read the X11 clipboard from the harness (the app itself uses `arboard`).
# imagemagick: `import -window <id>` captures a specific window to PNG.
# mesa-vulkan-drivers / libgl1-mesa-dri / libglx-mesa0 / libvulkan1: software Vulkan (lavapipe)
# so GPUI can render without a passed-through GPU device inside the pod.
# fonts-dejavu-core: DejaVu Sans covers both Latin and Cyrillic, needed for capability 4.
# pkg-config / git / ca-certificates / curl: building crates and fetching the pinned GPUI git dep.
RUN apt-get update && apt-get install -y --no-install-recommends \
      xvfb x11-utils x11-xserver-utils xdotool xclip imagemagick \
      mesa-vulkan-drivers libgl1-mesa-dri libglx-mesa0 libvulkan1 \
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
