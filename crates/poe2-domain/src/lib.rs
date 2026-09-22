//! Plain data types shared across the price-check pipeline: `Item`, `Mod`, `StatFilter`,
//! `Currency`, `ItemCategory`. Zero crate-local dependencies and no I/O -- every other crate in
//! this workspace that needs these shapes (`item-parser`, `trade-client`, `crates/poe2-oracle`)
//! depends on this one, never the reverse.
//!
//! Field shapes are informed by the real request/response structs already proven against the
//! live trade API in `crates/poe2-oracle/examples/trade_api.rs`'s POC, plus whatever the
//! `crates/data-pipeline` localization spike (`SPIKE_FINDINGS.md`) resolves for how `Mod` text
//! is stored -- filled in once that spike lands, not guessed ahead of it.
