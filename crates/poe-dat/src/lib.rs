//! Parses PoE2's `.dat64` table format using column schemas fetched from
//! `poe-tool-dev/dat-schema` (`schema.min.json`), rather than hand-maintained per-table struct
//! definitions -- the schema tracks upstream game-data changes without a code change here.
//!
//! Depends on `poe-bundle` for the raw table bytes; depended on by `crates/data-pipeline`.
