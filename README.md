# Oracle

Oracle is a from-scratch, no-Electron PoE2 price checker, built around [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's Rust GUI framework) instead of a web-view stack.

**Current status: architecture foundation complete, not a working price checker yet.** Two phases are done:

1. A GPUI feasibility spike answering whether GPUI is viable as Oracle's UI framework at all (frameless/transparent/always-on-top/click-through window chrome, a system-wide hotkey, clipboard reads, correct Cyrillic + Latin text rendering, an async call to the real PoE2 trade API). See [`POC_FINDINGS.md`](./POC_FINDINGS.md) for the verdict and evidence: **GO**.
2. A 10-crate Cargo workspace (see `crates/`) with a from-scratch local game-data pipeline -- reads PoE2's own `Bundles2` bundle format and `.datc64` tables directly, through a real, dynamically-loaded Oodle DLL, with no dependency on any third-party PoE-specific tooling. Proven end to end against a real PoE2 install (16,784 real `Mods.datc64` rows correctly parsed) and a real Oodle DLL; see [`crates/data-pipeline/SPIKE_FINDINGS.md`](./crates/data-pipeline/SPIKE_FINDINGS.md) for the full evidence trail, including the localization question's answer (mod/stat display text comes from the trade API, never from local extraction -- confirmed empirically, not assumed).

## Workspace layout

```
crates/
  oodle-ffi/           dynamically loads a real Oodle DLL (OodleLZ_Decompress)
  poe-bundle/           Bundles2/_.index.bin + *.bundle.bin parser
  poe-dat/               .datc64 table parser, schema from poe-tool-dev/dat-schema
  poe2-domain/         Item/Mod/StatFilter/Currency/ItemCategory -- pure data, no I/O
  item-parser/          clipboard item-text parser (scaffolded, not yet implemented)
  trade-client/         PoE2 trade API client (leagues -> search -> fetch)
  data-pipeline/         binary: bundles+dat -> JSON for poe2-oracle (validation spike only so far)
  auto-update/           update-check/download logic (scaffolded, not yet implemented)
  auto-update-helper/    swap-running-exe-on-quit helper (scaffolded, not yet implemented)
  poe2-oracle/          the shipped app: GPUI UI + Win32 overlay + examples
```

Dependency direction is enforced via `Cargo.toml`, never inverted: `oodle-ffi <- poe-bundle <-
poe-dat <- data-pipeline`; `poe2-domain` (zero deps) `<- item-parser, trade-client`; `poe2-oracle`
depends on `poe2-domain`/`item-parser`/`trade-client`/`auto-update`, never on
`poe-bundle`/`poe-dat`/`data-pipeline` directly -- the shipped app only ever reads
`data-pipeline`'s JSON output, never touches bundles or Oodle itself.

Scope for the eventual product:

- **Windows-only** supported platform for now. The codebase keeps an explicit platform boundary (see `crates/poe2-oracle/src/platform/`) so other platforms aren't architecturally locked out later, but only Windows is being built and tested today. The local game-data pipeline (`oodle-ffi` and everything built on it) is Windows-only for a second, independent reason: Oodle ships as a Windows PE DLL, which cannot be dynamically loaded on any other OS.
- **English + Russian** game-client support only. Other languages are out of scope.

CI (`.github/workflows/ci.yml`) builds the project's own `lanes/runner.Dockerfile` image and runs
`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, and a full
`cargo build --workspace --target x86_64-pc-windows-gnu` on every push.

The actual Price Check feature -- item parsing, search UI, results overlay -- is the next phase,
built on top of this foundation, not started yet.
