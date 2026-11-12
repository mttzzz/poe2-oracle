FROM rust:1-bookworm

# This project ships to Windows exclusively for now -- the runner cross-compiles from this Linux
# lane via mingw-w64, no MSVC/Windows SDK involved.
# gcc-mingw-w64-x86-64 / g++-mingw-w64-x86-64: cross gcc/binutils used as the linker for
# x86_64-pc-windows-gnu. gpui_platform auto-selects the `gpui_windows` backend via
# `cfg(target_os = "windows")` -- no feature flag needed on our side.
# pkg-config / git / ca-certificates / curl: building crates and fetching the pinned GPUI git dep.
RUN apt-get update && apt-get install -y --no-install-recommends \
      gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 \
      pkg-config git ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-pc-windows-gnu
# fmt/clippy are not part of the default `rust:1-bookworm` toolchain install; step 9 of the
# architecture plan's CI gate (`cargo fmt --check`, `cargo clippy --workspace -- -D warnings`)
# needs both.
RUN rustup component add rustfmt clippy

# sccache, the compiler wrapper of the lane's builds (lanes/lane.toml turns it on): it keeps the
# output of each rustc call, and of the C compiler calls of the crates' build scripts (the cc crate
# sends those through RUSTC_WRAPPER too), in the node's volume that every lane's runner mounts at
# /var/cache/lanes/sccache, so what one lane has built another does not build again. A release's
# static musl binary, checked against its sha256 (the release's .sha256 file says the same).
ARG SCCACHE_VERSION=v0.19.0
ARG SCCACHE_SHA256=aa73c7270c997894038c0d8f0d297865f57569e1687883322bc431713d4bb9dd
RUN archive="sccache-${SCCACHE_VERSION}-x86_64-unknown-linux-musl" \
 && curl -sSfL -o "/tmp/$archive.tar.gz" \
      "https://github.com/mozilla/sccache/releases/download/${SCCACHE_VERSION}/$archive.tar.gz" \
 && echo "${SCCACHE_SHA256}  /tmp/$archive.tar.gz" | sha256sum -c - \
 && tar -xzf "/tmp/$archive.tar.gz" -C /usr/local/bin --strip-components=1 "$archive/sccache" \
 && rm "/tmp/$archive.tar.gz"

RUN useradd -m -u 1000 -s /bin/bash runner
USER runner
ENV HOME=/home/runner
