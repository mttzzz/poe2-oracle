//! `poe-tool-dev/dat-schema` types and fetch/cache logic.
//!
//! Schema shape confirmed against that repo's own `src/types.ts` and a live fetch of its
//! compiled JSON output (`SCHEMA_VERSION = 7` as of this writing). Always fetches
//! `schema-poe2.min.json` specifically -- the pre-filtered, PoE2-only endpoint -- never the
//! combined `schema.min.json`: the combined file legitimately contains **the same table name
//! twice** with different, incompatible column layouts when a table exists in both games with
//! different shapes (e.g. `RogueExiles`: 3 columns at `validFor:1`, 16 columns at `validFor:2`).
//! Using the pre-filtered endpoint makes a plain by-name lookup safe without this crate having
//! to reimplement `validFor`-bitmask disambiguation itself.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// `poe-tool-dev/dat-schema`'s own `SCHEMA_VERSION` constant (`src/types.ts`), bumped only on
/// breaking shape changes. Checked against every fetch; a mismatch is a loud error rather than a
/// silent misparse, since this crate's byte-offset math depends on the shape matching exactly.
const EXPECTED_SCHEMA_VERSION: u32 = 7;

const SCHEMA_URL: &str =
    "https://github.com/poe-tool-dev/dat-schema/releases/download/latest/schema-poe2.min.json";

/// The compiled schema: every PoE2-relevant table and enum definition.
#[derive(Debug, Deserialize)]
pub struct SchemaFile {
    pub version: u32,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    pub tables: Vec<SchemaTable>,
    pub enumerations: Vec<SchemaEnumeration>,
}

#[derive(Debug, Deserialize)]
pub struct SchemaTable {
    #[serde(rename = "validFor")]
    pub valid_for: u32,
    pub name: String,
    pub columns: Vec<TableColumn>,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TableColumn {
    pub name: Option<String>,
    pub description: Option<String>,
    pub array: bool,
    #[serde(rename = "type")]
    pub kind: ColumnType,
    pub unique: bool,
    pub localized: bool,
    pub until: Option<String>,
    pub references: Option<ColumnReference>,
    pub file: Option<String>,
    pub files: Option<Vec<String>>,
    pub interval: bool,
}

/// A column's `type`. Matches `poe-tool-dev/dat-schema`'s `ColumnType` union exactly
/// (`src/types.ts`); `Array` is the literal `"array"` type string used for a column whose
/// *element* type is unknown (source `_`/`[_]`, still-unmigrated schema entries) -- distinct
/// from the separate `array: bool` flag on every column, which marks *any* column (known or
/// unknown element type) as holding zero-or-more elements rather than one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnType {
    Bool,
    I16,
    U16,
    I32,
    U32,
    F32,
    String,
    Row,
    ForeignRow,
    EnumRow,
    Array,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ColumnReference {
    /// A plain `Foo: OtherTable` field with no `@ref(column: ...)`: references a *row index* in
    /// `table`.
    RowIndex { table: String },
    /// `@ref(column: "Id")`-style: references a named column in `table`.
    Column { table: String, column: String },
}

#[derive(Debug, Deserialize)]
pub struct SchemaEnumeration {
    #[serde(rename = "validFor")]
    pub valid_for: u32,
    pub name: String,
    pub indexing: u8,
    pub enumerators: Vec<Option<String>>,
}

#[derive(Serialize, Deserialize)]
struct CacheManifest {
    fetched_at_unix: u64,
    schema_version: u32,
}

/// Fetches the PoE2 schema, using a disk cache unless `force_refresh` is set. On a cache hit,
/// makes no network request at all.
pub fn fetch_schema(force_refresh: bool) -> Result<SchemaFile> {
    let cache_dir = schema_cache_dir()?;
    let schema_path = cache_dir.join("dat-schema-poe2.json");
    let manifest_path = cache_dir.join("dat-schema-poe2.manifest.json");

    if !force_refresh && let Some(schema) = try_read_cache(&schema_path, &manifest_path) {
        return Ok(schema);
    }

    let body = ureq::get(SCHEMA_URL)
        .call()
        .with_context(|| format!("failed to fetch {SCHEMA_URL}"))?
        .into_string()
        .context("schema response body was not valid UTF-8")?;
    let schema = parse_and_validate(&body)?;

    fs::create_dir_all(&cache_dir)
        .with_context(|| format!("failed to create cache directory {}", cache_dir.display()))?;
    fs::write(&schema_path, &body)
        .with_context(|| format!("failed to write {}", schema_path.display()))?;
    let manifest = CacheManifest {
        fetched_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        schema_version: schema.version,
    };
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    Ok(schema)
}

/// Looks up a table by name. Assumes `schema` came from [`fetch_schema`] (the PoE2-filtered
/// endpoint), so a plain by-name match is safe -- see module doc comment.
pub fn find_table<'a>(schema: &'a SchemaFile, table_name: &str) -> Result<&'a SchemaTable> {
    schema
        .tables
        .iter()
        .find(|t| t.name == table_name)
        .ok_or_else(|| anyhow::anyhow!("no table named {table_name:?} in the fetched schema"))
}

fn try_read_cache(
    schema_path: &std::path::Path,
    manifest_path: &std::path::Path,
) -> Option<SchemaFile> {
    let manifest_bytes = fs::read(manifest_path).ok()?;
    let manifest: CacheManifest = serde_json::from_slice(&manifest_bytes).ok()?;
    if manifest.schema_version != EXPECTED_SCHEMA_VERSION {
        return None;
    }
    let body = fs::read_to_string(schema_path).ok()?;
    parse_and_validate(&body).ok()
}

fn parse_and_validate(body: &str) -> Result<SchemaFile> {
    let schema: SchemaFile =
        serde_json::from_str(body).context("failed to parse dat-schema JSON")?;
    if schema.version != EXPECTED_SCHEMA_VERSION {
        bail!(
            "fetched dat-schema version {} does not match the version this crate was written \
             against ({EXPECTED_SCHEMA_VERSION}) -- the schema shape may have changed; re-verify \
             poe-tool-dev/dat-schema's src/types.ts before trusting this crate's parsing",
            schema.version
        );
    }
    Ok(schema)
}

fn schema_cache_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "poe2-oracle")
        .context("could not determine a cache directory for this platform")?;
    Ok(dirs.cache_dir().to_owned())
}
