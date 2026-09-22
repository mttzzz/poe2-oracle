//! Version-check + download logic for keeping a shipped `crates/poe2-oracle` install current,
//! mirroring the shape of Zed's own `crates/auto_update`. Library only -- `auto-update-helper`
//! is the separate tiny binary that actually swaps the running exe on quit, since a running
//! Windows exe cannot overwrite itself directly.
//!
//! Scaffolded here as part of the architecture foundation; the real update-check/download logic
//! is out of scope for this plan and lands in a follow-up plan.
