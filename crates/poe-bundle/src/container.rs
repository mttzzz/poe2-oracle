//! The generic 60-byte-header "Bundle" container format that both `_.index.bin` and every
//! `*.bundle.bin` file share. Confirmed field-for-field identical across three independently
//! authored parsers (C# `LibBundle3`, Rust `ggpk-explorer`, Go `pogo`) and the `poe-tool-dev`
//! community wiki's own 010-editor-style template.

use anyhow::{Result, bail, ensure};
use oodle_ffi::OodleDecompressor;

/// Byte length of the fixed header, before the per-chunk size table.
const HEADER_LEN: usize = 60;

/// Decompresses a whole "Bundle"-format byte slice (a `_.index.bin` or `*.bundle.bin` file's raw
/// contents) into its logical, uncompressed payload.
///
/// This decompresses every chunk unconditionally rather than seeking to only the chunk(s)
/// covering a requested byte range -- the simpler of the two approaches real prior art uses
/// (`LibBundle3`'s `ReadWithoutCache` does partial-range seeking; the Rust/Go references
/// decompress the whole bundle, same as here). Correct and adequate for this crate's current
/// `read_file`-by-path use case; partial-range reads are a possible future optimization, not a
/// correctness requirement.
pub(crate) fn decompress(raw: &[u8], oodle: &OodleDecompressor) -> Result<Vec<u8>> {
    ensure!(
        raw.len() >= HEADER_LEN,
        "bundle container too short for a {HEADER_LEN}-byte header: {} bytes",
        raw.len()
    );

    let uncompressed_size = read_u32(raw, 0) as usize;
    let chunk_count = read_u32(raw, 36) as usize;
    let chunk_size = read_u32(raw, 40) as usize;

    let block_table_start = HEADER_LEN;
    let block_table_len = chunk_count.checked_mul(4).ok_or_else(|| {
        anyhow::anyhow!("chunk_count {chunk_count} overflows the block-size table length")
    })?;
    let block_table_end = block_table_start + block_table_len;
    ensure!(
        raw.len() >= block_table_end,
        "bundle container too short for its {chunk_count}-entry block-size table: have {} bytes, need at least {block_table_end}",
        raw.len()
    );

    let mut out = Vec::with_capacity(uncompressed_size);
    let mut chunk_cursor = block_table_end;
    for chunk_index in 0..chunk_count {
        let block_size = read_u32(raw, block_table_start + chunk_index * 4) as usize;
        let block_end = chunk_cursor + block_size;
        ensure!(
            raw.len() >= block_end,
            "bundle chunk {chunk_index} runs past the end of the container: need {block_end} bytes, have {}",
            raw.len()
        );

        let this_chunk_uncompressed_size = if chunk_index + 1 == chunk_count {
            uncompressed_size
                .checked_sub(chunk_size * (chunk_count - 1))
                .ok_or_else(|| anyhow::anyhow!("last-chunk size computation underflowed: uncompressed_size={uncompressed_size} chunk_size={chunk_size} chunk_count={chunk_count}"))?
        } else {
            chunk_size
        };

        let decompressed =
            oodle.decompress(&raw[chunk_cursor..block_end], this_chunk_uncompressed_size)?;
        out.extend_from_slice(&decompressed);
        chunk_cursor = block_end;
    }

    if out.len() != uncompressed_size {
        bail!(
            "decompressed bundle payload is {} bytes, header declared uncompressed_size={uncompressed_size}",
            out.len()
        );
    }
    Ok(out)
}

fn read_u32(raw: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(raw[offset..offset + 4].try_into().expect("4-byte slice"))
}
