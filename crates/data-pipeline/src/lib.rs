//! Standalone binary (see `src/main.rs`, added once the `oodle-ffi`/`poe-bundle`/`poe-dat`
//! validation spike lands -- see `SPIKE_FINDINGS.md` alongside this file for its results):
//! reads PoE2's local game-data bundles end to end and writes JSON consumed by
//! `crates/poe2-oracle` at build/runtime.
//!
//! Depends on `poe-bundle` and `poe-dat`; never depended on by `crates/poe2-oracle` itself --
//! only this binary's JSON output is, since the shipped app never touches bundles or Oodle
//! directly (that machinery only runs where the Bundles2 SMB share is reachable).
