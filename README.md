# Oracle

Oracle is a from-scratch, no-Electron PoE2 price checker, built around [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's Rust GUI framework) instead of a web-view stack.

**Current status: GPUI feasibility spike, not a working price checker yet.** This repository currently holds a proof-of-concept that answers one question — is GPUI viable as the UI framework for Oracle — by exercising the six capabilities a real price-check overlay needs: frameless/transparent/always-on-top/click-through window chrome, a system-wide hotkey, clipboard reads, correct Cyrillic + Latin text rendering, an async call to the real PoE2 trade API, and a hardcoded price-check card layout. See [`POC_FINDINGS.md`](./POC_FINDINGS.md) for the verdict and evidence once the spike is complete.

Scope for the eventual product:

- **Windows-only** supported platform for now. The codebase keeps an explicit platform boundary (see `src/platform/`) so other platforms aren't architecturally locked out later, but only Windows is being built and tested today.
- **English + Russian** game-client support only. Other languages are out of scope.

Architecture, documentation, and the real task backlog are deliberately not written yet — they depend on real GPUI patterns learned from this spike, not speculation ahead of it.
