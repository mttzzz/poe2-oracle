//! `_.index.bin` parsing and path-hash file lookup.
//!
//! `_.index.bin`'s decompressed body (after unwrapping the outer Bundle container, see
//! [`crate::container`]) is, in order: a bundle-name table, a 20-byte-per-record file table
//! (`path_hash -> bundle_index/offset/size`), a 20-byte-per-record directory table, and finally
//! a second, independently-nested Bundle container holding a compact path-string encoding used
//! to reconstruct every file's human-readable path from its hash.
//!
//! This crate only ever needs the reverse direction -- given an already-known virtual path,
//! resolve it to bytes -- so [`BundleIndex::open`] stops parsing right after the file table.
//! The directory table and the nested path-reconstruction bundle exist to support the opposite
//! direction (enumerating every path a bundle contains) and are out of scope for `read_file`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use oodle_ffi::OodleDecompressor;

use crate::container;
use crate::hash::hash_path;

const INDEX_FILE_NAME: &str = "_.index.bin";
/// Bytes per file-table record: `path_hash: u64, bundle_index: u32, file_offset: u32,
/// file_size: u32`.
const FILE_RECORD_LEN: usize = 20;

struct FileRecord {
    bundle_index: u32,
    offset: u32,
    size: u32,
}

/// A parsed `_.index.bin`, ready to resolve virtual paths to bytes from the `*.bundle.bin` files
/// alongside it.
pub struct BundleIndex {
    bundles2_root: PathBuf,
    oodle: OodleDecompressor,
    /// `bundle_index` from a file record is a position into this table; each entry is the
    /// bundle's name *without* the `.bundle.bin` suffix or `Bundles2/` prefix (e.g.
    /// `"Data/Balance"`, not `"Bundles2/Data/Balance.bundle.bin"`).
    bundle_names: Vec<String>,
    files: HashMap<u64, FileRecord>,
}

impl BundleIndex {
    /// Reads and parses `<bundles2_root>/_.index.bin`. `oodle` is consumed and kept for the
    /// lifetime of the returned `BundleIndex`, since every subsequent [`read_file`](Self::read_file)
    /// call needs it too.
    pub fn open(bundles2_root: &Path, oodle: OodleDecompressor) -> Result<Self> {
        let index_path = bundles2_root.join(INDEX_FILE_NAME);
        let raw = fs::read(&index_path)
            .with_context(|| format!("failed to read {}", index_path.display()))?;
        let body = container::decompress(&raw, &oodle)
            .with_context(|| format!("failed to decompress {}", index_path.display()))?;

        let mut cursor = 0usize;
        let bundle_count = read_u32(&body, &mut cursor)? as usize;
        let mut bundle_names = Vec::with_capacity(bundle_count);
        for i in 0..bundle_count {
            let name_len = read_u32(&body, &mut cursor)? as usize;
            ensure!(
                body.len() >= cursor + name_len,
                "bundle name record {i} runs past the end of the index body"
            );
            let name = std::str::from_utf8(&body[cursor..cursor + name_len])
                .with_context(|| format!("bundle name record {i} is not valid UTF-8"))?
                .to_owned();
            cursor += name_len;
            let _uncompressed_size = read_u32(&body, &mut cursor)?;
            bundle_names.push(name);
        }

        let file_count = read_u32(&body, &mut cursor)? as usize;
        ensure!(
            body.len() >= cursor + file_count * FILE_RECORD_LEN,
            "index body too short for its declared {file_count} file records"
        );
        let mut files = HashMap::with_capacity(file_count);
        for _ in 0..file_count {
            let path_hash = read_u64(&body, &mut cursor)?;
            let bundle_index = read_u32(&body, &mut cursor)?;
            let offset = read_u32(&body, &mut cursor)?;
            let size = read_u32(&body, &mut cursor)?;
            files.insert(
                path_hash,
                FileRecord {
                    bundle_index,
                    offset,
                    size,
                },
            );
        }
        // Directory table + the nested path-reconstruction bundle follow here; intentionally
        // not parsed -- see module doc comment.

        Ok(Self {
            bundles2_root: bundles2_root.to_owned(),
            oodle,
            bundle_names,
            files,
        })
    }

    /// Resolves `virtual_path` (e.g. `"Data/Balance/Mods.datc64"`) to its decompressed bytes.
    pub fn read_file(&self, virtual_path: &str) -> Result<Vec<u8>> {
        let hash = hash_path(virtual_path);
        let record = self.files.get(&hash).ok_or_else(|| {
            anyhow::anyhow!("no bundle file entry for {virtual_path:?} (hash {hash:#018x})")
        })?;

        let bundle_name = self.bundle_names.get(record.bundle_index as usize).ok_or_else(|| {
            anyhow::anyhow!(
                "file record for {virtual_path:?} references bundle_index {} but only {} bundles exist",
                record.bundle_index,
                self.bundle_names.len()
            )
        })?;
        let bundle_path = self.bundles2_root.join(format!("{bundle_name}.bundle.bin"));
        let raw = fs::read(&bundle_path).with_context(|| {
            format!(
                "failed to read {} (bundle for {virtual_path:?})",
                bundle_path.display()
            )
        })?;
        let decompressed = container::decompress(&raw, &self.oodle)
            .with_context(|| format!("failed to decompress {}", bundle_path.display()))?;

        let start = record.offset as usize;
        let end = start + record.size as usize;
        ensure!(
            decompressed.len() >= end,
            "{virtual_path:?} claims byte range {start}..{end} but {} only decompressed to {} bytes",
            bundle_path.display(),
            decompressed.len()
        );
        Ok(decompressed[start..end].to_vec())
    }
}

fn read_u32(body: &[u8], cursor: &mut usize) -> Result<u32> {
    if body.len() < *cursor + 4 {
        bail!("unexpected end of index body reading a u32 at offset {cursor}");
    }
    let value = u32::from_le_bytes(body[*cursor..*cursor + 4].try_into().expect("4-byte slice"));
    *cursor += 4;
    Ok(value)
}

fn read_u64(body: &[u8], cursor: &mut usize) -> Result<u64> {
    if body.len() < *cursor + 8 {
        bail!("unexpected end of index body reading a u64 at offset {cursor}");
    }
    let value = u64::from_le_bytes(body[*cursor..*cursor + 8].try_into().expect("8-byte slice"));
    *cursor += 8;
    Ok(value)
}
