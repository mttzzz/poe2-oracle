//! Throwaway validation spike (architecture plan step 6): loads Oodle, opens the real
//! `Bundles2` index, fetches the PoE2 dat-schema, locates and parses a table, and prints the
//! first 20 rows -- the one piece of proof this whole plan's foundation claim rests on: does the
//! from-scratch local game-data pipeline actually work against real PoE2 files end to end.
//!
//! Not a real CLI (hand-rolled flag parsing, no config file, no output writing yet) -- that's
//! this binary's own follow-up work once the pipeline is proven. See `SPIKE_FINDINGS.md`
//! alongside this file for what running it against the real share actually found.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

struct Args {
    dll_path: PathBuf,
    bundles2_root: PathBuf,
    table_name: String,
    virtual_path: Option<String>,
    refresh_schema: bool,
    row_limit: usize,
}

fn parse_args() -> Result<Args> {
    let mut dll_path = None;
    let mut bundles2_root = None;
    let mut table_name = "Mods".to_string();
    let mut virtual_path = None;
    let mut refresh_schema = false;
    let mut row_limit = 20usize;

    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--dll" => {
                dll_path = Some(PathBuf::from(raw.get(i + 1).context("--dll needs a path")?));
                i += 2;
            }
            "--bundles2-root" => {
                bundles2_root = Some(PathBuf::from(
                    raw.get(i + 1).context("--bundles2-root needs a path")?,
                ));
                i += 2;
            }
            "--table" => {
                table_name = raw.get(i + 1).context("--table needs a name")?.clone();
                i += 2;
            }
            "--path" => {
                virtual_path = Some(raw.get(i + 1).context("--path needs a value")?.clone());
                i += 2;
            }
            "--refresh-schema" => {
                refresh_schema = true;
                i += 1;
            }
            "--rows" => {
                row_limit = raw.get(i + 1).context("--rows needs a number")?.parse()?;
                i += 2;
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    Ok(Args {
        dll_path: dll_path.context("--dll <path-to-oo2core_9_win64.dll> is required")?,
        bundles2_root: bundles2_root
            .context("--bundles2-root <path-to-Bundles2-folder> is required")?,
        table_name,
        virtual_path,
        refresh_schema,
        row_limit,
    })
}

fn main() -> Result<()> {
    let args = parse_args()?;

    println!("=== data-pipeline validation spike ===");
    println!("Loading Oodle from {}", args.dll_path.display());
    let oodle = oodle_ffi::load(&args.dll_path)?;
    println!("Oodle loaded OK.");

    println!("Opening bundle index at {}", args.bundles2_root.display());
    let index = poe_bundle::BundleIndex::open(&args.bundles2_root, oodle)?;
    println!("Bundle index opened OK.");

    println!(
        "Fetching PoE2 dat-schema (force_refresh={})...",
        args.refresh_schema
    );
    let schema = poe_dat::fetch_schema(args.refresh_schema)?;
    println!(
        "Schema version {} loaded: {} tables, {} enums.",
        schema.version,
        schema.tables.len(),
        schema.enumerations.len()
    );

    let table = poe_dat::find_table(&schema, &args.table_name)
        .with_context(|| format!("table {:?} not found in schema", args.table_name))?;
    println!(
        "Found table {:?} (validFor={}): {} columns, tags={:?}",
        table.name,
        table.valid_for,
        table.columns.len(),
        table.tags
    );
    for column in &table.columns {
        println!(
            "  - {:?}: {:?}{} {}",
            column.name,
            column.kind,
            if column.array { "[]" } else { "" },
            if column.localized { "LOCALIZED" } else { "" }
        );
    }

    let virtual_path = args
        .virtual_path
        .unwrap_or_else(|| format!("Data/Balance/{}.datc64", args.table_name));
    println!("\nReading {virtual_path:?} from the bundle index...");
    let rows = poe_dat::read_table(&index, &virtual_path, table)
        .with_context(|| format!("failed to read+parse {virtual_path:?}"))?;
    println!("Parsed {} rows.", rows.len());

    println!("\n--- first {} rows ---", args.row_limit.min(rows.len()));
    for (row_index, row) in rows.iter().take(args.row_limit).enumerate() {
        println!("row {row_index}:");
        for (column, value) in table.columns.iter().zip(&row.values) {
            println!("  {:?} = {:?}", column.name, value);
        }
    }

    Ok(())
}
