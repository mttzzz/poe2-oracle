//! Reads PoE2's `Bundles2` asset container format from scratch: `_.index.bin`'s bundle-name
//! table, per-file path-hash lookup, and the individual Oodle-compressed `*.bundle.bin` files it
//! indexes into. Written against the on-disk layout observed on the real game install (see
//! `crates/data-pipeline/SPIKE_FINDINGS.md` once the validation spike lands), using the format
//! knowledge documented across `ggpk`/`ggpklib`/`poe_bundle`/`pathofexile-dat` as a reference
//! spec to read for understanding, not as a code dependency.
//!
//! Depends on `oodle-ffi` for decompression; depended on by `poe-dat`. Never depended on by
//! `crates/poe2-oracle` -- the shipped app only ever reads `data-pipeline`'s JSON output, never
//! touches bundles or Oodle directly.
