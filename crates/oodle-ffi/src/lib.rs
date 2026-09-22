//! Dynamically loads a real, RAD-authored Oodle compression DLL (`OodleLZ_Decompress`) and wraps
//! it behind a safe API. Deliberately does NOT vendor or reimplement Oodle's algorithm (e.g.
//! `powzix/ooz`, seven years stale with no formal license) -- this crate only ever calls a
//! genuine binary the caller supplies, sourced from any RAD-licensed game that ships one loose
//! (PoE2 itself statically links Oodle into its own exe and ships no loose `oo2core*.dll`, so it
//! cannot be sourced from PoE2's own install).
//!
//! The DLL path is a runtime parameter, never bundled or hardcoded here -- see
//! `crates/data-pipeline` for the one caller that supplies it today. This is the bottom of the
//! workspace's local game-data dependency chain: `poe-bundle` depends on this crate for
//! decompression, never the reverse.
