FROM rust:1-bookworm

# This project ships to Windows exclusively for now (see POC_FINDINGS.md's "Windows" section) --
# the runner cross-compiles from this Linux lane via mingw-w64, no MSVC/Windows SDK involved.
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

RUN useradd -m -u 1000 -s /bin/bash runner
USER runner
ENV HOME=/home/runner
