//! Parses PoE2's `.datc64` table format using column schemas fetched from
//! `poe-tool-dev/dat-schema`'s `schema-poe2.min.json` release asset, rather than hand-maintained
//! per-table struct definitions -- the schema tracks upstream game-data changes without a code
//! change here. (The extension is `.datc64`, not `.dat64`/`.dat` used by some older PoE1-era
//! tooling and community writing -- confirmed against the real reference parser; see
//! [`table`]'s doc comment for the full binary-layout citation trail.)
//!
//! Depends on `poe-bundle` for the raw table bytes ([`read_table`] wraps both in one call);
//! depended on by `crates/data-pipeline`.

mod schema;
mod table;

pub use schema::{
    ColumnReference, ColumnType, SchemaEnumeration, SchemaFile, SchemaTable, TableColumn,
    fetch_schema, find_table,
};
pub use table::{Row, Value, parse_table, read_table};
