//! Tiny helper binary (see Zed's own `crates/auto_update_helper` for the pattern this mirrors):
//! swaps a shipped `crates/poe2-oracle` install's running exe for a freshly downloaded one on
//! quit, since a running Windows exe cannot overwrite itself while it is still executing.
//!
//! Scaffolded here as part of the architecture foundation; the real swap-on-quit logic is out of
//! scope for this plan and lands in a follow-up plan alongside `auto-update`.
