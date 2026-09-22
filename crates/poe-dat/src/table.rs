//! `.datc64` binary row/heap parsing.
//!
//! Layout, confirmed against `SnosMe/poe-dat-viewer`'s production TypeScript reference
//! (`lib/src/dat/dat-file.ts`, `lib/src/dat/reader.ts`, `lib/src/utils/findSequence.ts` -- read
//! directly, not paraphrased from prose) and cross-checked against an independent Rust parser
//! (`shadr/ggpkviewer`'s `ggpklib/src/dat.rs`):
//!
//! `[4-byte LE row count][fixed-width row region][8 bytes of 0xBB][heap region]`. The row count
//! is stored explicitly; row *width* is derived (`heap_marker_offset / row_count`), not read
//! from a header or the schema. The 8-byte `0xBB` marker can coincidentally occur inside real
//! row/heap bytes before the true boundary, so the search re-scans until it finds a match whose
//! offset (measured from byte 4) is an exact multiple of `row_count` -- the reference's own
//! `findAlignedSequence`; a naive first-match search (used by some other prior art) is wrong.
//!
//! **Heap offsets stored in row data are relative to the 8-byte marker's own start**, not to the
//! first byte after it -- confirmed directly from the reference's `dataVariable = file.subarray(
//! INT_ROWCOUNT + boundary)` (the marker bytes are `dataVariable[0..8]`) and `oneString`/
//! `readMany` indexing straight into that same `dataVariable` with the raw stored offset, no `-8`
//! adjustment anywhere. Getting this wrong silently misreads every string/array/row/foreignrow
//! by exactly 8 bytes.
//!
//! Row/string/foreignrow/array fields are declared as 8- or 16-byte slots, but the reference
//! implementation only actually reads their low 32 bits (a `DataView`/`BigInt`-avoidance
//! artifact of the host language, workable since real offsets/indices never exceed 2^32) -- this
//! implementation reads the full 8-byte little-endian value instead, the more faithful choice
//! for a new implementation (matches the independent Rust reference, which does the same).

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use crate::schema::{ColumnType, SchemaTable, TableColumn};

const ROW_COUNT_LEN: usize = 4;
const HEAP_MARKER: [u8; 8] = [0xBB; 8];
/// Widened from the reference's 32-bit `MEM32_NULL = 0xfefefefe`: real null slots pattern-fill
/// their upper 32 bits identically, so a full 8-byte compare is equivalent and more honest about
/// the field's real on-disk width.
const NULL_SENTINEL_64: u64 = 0xFEFE_FEFE_FEFE_FEFE;

/// One column's value. Array elements and interval bounds are always the column's own
/// (non-array, non-interval) scalar shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Bool(bool),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    F32(f32),
    String(String),
    /// Row index into this table's own rows, or `None` for a null reference.
    Row(Option<u64>),
    /// Row index into another table (which table is `TableColumn::references`' job to say), or
    /// `None` for a null reference.
    ForeignRow(Option<u64>),
    /// Index into the referenced enum's `enumerators[]`.
    EnumRow(i32),
    Array(Vec<Value>),
    Interval(Box<Value>, Box<Value>),
    /// A column whose element type the schema doesn't know (source `_`/`[_]`, see
    /// [`ColumnType::Array`]'s doc comment) -- only the element count is readable; the heap
    /// bytes are deliberately left uninterpreted.
    UnknownArray {
        count: u64,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub values: Vec<Value>,
}

/// Parses every row of `raw_bytes` (a `.datc64` file's full, already-Oodle-decompressed
/// contents) according to `table`'s column definitions, in schema column order.
pub fn parse_table(table: &SchemaTable, raw_bytes: &[u8]) -> Result<Vec<Row>> {
    ensure!(
        raw_bytes.len() >= ROW_COUNT_LEN,
        "table data too short ({} bytes) for a {ROW_COUNT_LEN}-byte row-count header",
        raw_bytes.len()
    );
    let row_count = u32::from_le_bytes(raw_bytes[0..4].try_into().unwrap()) as usize;

    let marker_pos = find_heap_marker(raw_bytes, row_count)?;
    let row_length = (marker_pos - ROW_COUNT_LEN)
        .checked_div(row_count)
        .unwrap_or(0);
    let fixed_region = &raw_bytes[ROW_COUNT_LEN..marker_pos];
    // The heap slice starts AT the marker, matching the reference's `dataVariable` exactly --
    // stored heap offsets are relative to this position, not to the first post-marker byte (see
    // module doc comment).
    let heap = &raw_bytes[marker_pos..];

    let column_offsets = compute_column_offsets(table)?;
    if let Some(&last_offset) = column_offsets.last() {
        let (last_column, last_width) = (
            table
                .columns
                .last()
                .expect("column_offsets non-empty implies columns non-empty"),
            column_byte_width(table.columns.last().unwrap()),
        );
        ensure!(
            last_offset + last_width <= row_length,
            "schema declares at least {} bytes/row for table {:?} (last column {:?}) but the \
             file's derived row length is only {row_length} bytes",
            last_offset + last_width,
            table.name,
            last_column.name
        );
    }

    let mut rows = Vec::with_capacity(row_count);
    for row_index in 0..row_count {
        let row_start = row_index * row_length;
        let row_bytes = &fixed_region[row_start..row_start + row_length];
        let mut values = Vec::with_capacity(table.columns.len());
        for (column, &offset) in table.columns.iter().zip(&column_offsets) {
            values.push(
                read_column_value(column, row_bytes, offset, heap).with_context(|| {
                    format!(
                        "reading column {:?} (row {row_index}) of table {:?}",
                        column.name, table.name
                    )
                })?,
            );
        }
        rows.push(Row { values });
    }
    Ok(rows)
}

/// Reads `<virtual_path>` via `index` and parses it as `table` in one call.
pub fn read_table(
    index: &poe_bundle::BundleIndex,
    virtual_path: &str,
    table: &SchemaTable,
) -> Result<Vec<Row>> {
    let raw_bytes = index
        .read_file(virtual_path)
        .with_context(|| format!("reading {virtual_path:?} for table {:?}", table.name))?;
    parse_table(table, &raw_bytes)
}

/// Finds the 8-byte `0xBB` marker, re-scanning past any coincidental match until the offset
/// (measured from byte 4) is an exact multiple of `row_count` -- see module doc comment.
fn find_heap_marker(raw: &[u8], row_count: usize) -> Result<usize> {
    let mut search_from = ROW_COUNT_LEN;
    loop {
        let found = raw
            .get(search_from..)
            .and_then(|s| s.windows(HEAP_MARKER.len()).position(|w| w == HEAP_MARKER))
            .map(|p| p + search_from);
        let Some(pos) = found else {
            bail!("no 0xBB heap marker found (searched from byte {search_from})");
        };
        // `is_multiple_of` is panic-safe for a zero divisor (`x.is_multiple_of(0) == (x == 0)`,
        // unlike raw `%`) and happens to be exactly the right check for row_count == 0 too: an
        // empty table has a zero-byte fixed region, so the marker must sit at relative offset 0
        // -- no separate `row_count == 0` branch needed.
        if (pos - ROW_COUNT_LEN).is_multiple_of(row_count) {
            return Ok(pos);
        }
        search_from = pos + 1;
    }
}

fn compute_column_offsets(table: &SchemaTable) -> Result<Vec<usize>> {
    let mut offsets = Vec::with_capacity(table.columns.len());
    let mut offset = 0usize;
    for column in &table.columns {
        offsets.push(offset);
        offset += column_byte_width(column);
    }
    Ok(offsets)
}

/// The fixed-row byte width this column consumes: 16 bytes for any array (a `{count, offset}`
/// descriptor, regardless of element type), 2x the scalar width for an interval (`[min, max]`
/// inline, never heap-referenced), otherwise the scalar width itself.
fn column_byte_width(column: &TableColumn) -> usize {
    if column.array {
        16
    } else if column.interval {
        scalar_width(column.kind) * 2
    } else {
        scalar_width(column.kind)
    }
}

fn scalar_width(kind: ColumnType) -> usize {
    match kind {
        ColumnType::Bool => 1,
        ColumnType::I16 | ColumnType::U16 => 2,
        ColumnType::I32 | ColumnType::U32 | ColumnType::F32 | ColumnType::EnumRow => 4,
        ColumnType::String | ColumnType::Row => 8,
        ColumnType::ForeignRow => 16,
        // Opaque: only ever read as a {count, offset} array descriptor (see Value::UnknownArray).
        ColumnType::Array => 16,
    }
}

fn read_column_value(
    column: &TableColumn,
    row_bytes: &[u8],
    offset: usize,
    heap: &[u8],
) -> Result<Value> {
    // Array/interval/plain are mutually exclusive in the reference reader's own dispatch
    // (`getFieldReader`'s if/else-if/else chain) -- mirrored here with the same precedence.
    if column.array {
        return read_array_value(column, row_bytes, offset, heap);
    }
    if column.interval {
        let width = scalar_width(column.kind);
        let lo = read_scalar(column.kind, &row_bytes[offset..offset + width], heap)?;
        let hi = read_scalar(
            column.kind,
            &row_bytes[offset + width..offset + 2 * width],
            heap,
        )?;
        return Ok(Value::Interval(Box::new(lo), Box::new(hi)));
    }
    read_scalar(
        column.kind,
        &row_bytes[offset..offset + scalar_width(column.kind)],
        heap,
    )
}

fn read_array_value(
    column: &TableColumn,
    row_bytes: &[u8],
    offset: usize,
    heap: &[u8],
) -> Result<Value> {
    let count = read_u64(&row_bytes[offset..offset + 8]);
    let heap_offset = read_u64(&row_bytes[offset + 8..offset + 16]) as usize;

    if column.kind == ColumnType::Array {
        // Element type unknown -- deliberately not walking into the heap; see Value::UnknownArray.
        return Ok(Value::UnknownArray { count });
    }

    let element_width = scalar_width(column.kind);
    let mut values = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let element_offset = heap_offset + i * element_width;
        ensure!(
            heap.len() >= element_offset + element_width,
            "array element {i} of column {:?} runs past the end of the heap",
            column.name
        );
        values.push(read_scalar(
            column.kind,
            &heap[element_offset..element_offset + element_width],
            heap,
        )?);
    }
    Ok(Value::Array(values))
}

fn read_scalar(kind: ColumnType, bytes: &[u8], heap: &[u8]) -> Result<Value> {
    Ok(match kind {
        ColumnType::Bool => Value::Bool(bytes[0] != 0),
        ColumnType::I16 => Value::I16(i16::from_le_bytes(bytes.try_into().unwrap())),
        ColumnType::U16 => Value::U16(u16::from_le_bytes(bytes.try_into().unwrap())),
        ColumnType::I32 => Value::I32(i32::from_le_bytes(bytes.try_into().unwrap())),
        ColumnType::U32 => Value::U32(u32::from_le_bytes(bytes.try_into().unwrap())),
        ColumnType::F32 => Value::F32(f32::from_le_bytes(bytes.try_into().unwrap())),
        ColumnType::EnumRow => Value::EnumRow(i32::from_le_bytes(bytes.try_into().unwrap())),
        ColumnType::String => {
            let heap_offset = read_u64(&bytes[0..8]) as usize;
            Value::String(read_heap_string(heap, heap_offset)?)
        }
        ColumnType::Row => {
            let raw = read_u64(&bytes[0..8]);
            Value::Row(if raw == NULL_SENTINEL_64 {
                None
            } else {
                Some(raw)
            })
        }
        ColumnType::ForeignRow => {
            // Second 8-byte slot is documented as always-null/reserved; the reference reader
            // never exposes it either (see module doc comment).
            let raw = read_u64(&bytes[0..8]);
            Value::ForeignRow(if raw == NULL_SENTINEL_64 {
                None
            } else {
                Some(raw)
            })
        }
        ColumnType::Array => bail!("read_scalar called with the opaque Array type directly"),
    })
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes.try_into().expect("8-byte slice"))
}

/// Reads a UTF-16LE string starting at `offset` in `heap`, terminated by 4 consecutive zero
/// bytes positioned at an even offset relative to the string's own start (not a plain 2-byte
/// UTF-16 null terminator) -- ported directly from the reference's `readStringAt`/
/// `findZeroSequence`, which retries the search one byte later whenever the first zero-run found
/// lands at an odd relative offset.
fn read_heap_string(heap: &[u8], offset: usize) -> Result<String> {
    ensure!(
        offset <= heap.len(),
        "string heap offset {offset} beyond heap length {}",
        heap.len()
    );

    let mut end = find_zero_run(heap, offset)
        .with_context(|| format!("no null terminator found for heap string at offset {offset}"))?;
    while !(end - offset).is_multiple_of(2) {
        end = find_zero_run(heap, end + 1).with_context(|| {
            format!("no aligned null terminator found for heap string at offset {offset}")
        })?;
    }

    let units: Vec<u16> = heap[offset..end]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    Ok(String::from_utf16_lossy(&units))
}

/// Finds the first position `>= from` where 4 consecutive bytes are all zero.
fn find_zero_run(data: &[u8], from: usize) -> Option<usize> {
    let mut from = from;
    'outer: loop {
        let mut idx = from;
        while idx < data.len() && data[idx] != 0 {
            idx += 1;
        }
        if idx + 4 > data.len() {
            return None;
        }
        if data[idx + 1..idx + 4].iter().any(|&b| b != 0) {
            from = idx + 1;
            continue 'outer;
        }
        return Some(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ColumnType, SchemaTable, TableColumn};

    fn column(name: &str, kind: ColumnType, array: bool, interval: bool) -> TableColumn {
        TableColumn {
            name: Some(name.to_owned()),
            description: None,
            array,
            kind,
            unique: false,
            localized: false,
            until: None,
            references: None,
            file: None,
            files: None,
            interval,
        }
    }

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// Hand-builds a minimal `.datc64`-format byte buffer (2 rows: i32, bool, string) and
    /// confirms `parse_table` reproduces the exact values encoded into it -- a synthetic,
    /// self-contained cross-check of the trickiest byte-layout details (heap-offset base is
    /// relative to the marker's own start, alignment-aware marker search, UTF-16LE string
    /// termination), independent of any real game file. A real trimmed fixture (per the
    /// architecture plan) is added once the step 6 validation spike has live bundle access.
    #[test]
    fn parses_a_hand_built_fixture_with_scalars_and_a_string() {
        let table = SchemaTable {
            valid_for: 2,
            name: "Synthetic".to_owned(),
            columns: vec![
                column("Count", ColumnType::I32, false, false),
                column("Active", ColumnType::Bool, false, false),
                column("Label", ColumnType::String, false, false),
            ],
            tags: vec![],
        };

        // Heap offsets are relative to the marker's own start (byte 0 of the heap slice), so
        // real string data begins at offset 8 -- see module doc comment.
        let mut post_marker_heap = Vec::new();
        let hello_offset = 8 + post_marker_heap.len();
        post_marker_heap.extend(utf16le("Hello"));
        post_marker_heap.extend([0u8; 4]);
        let world_offset = 8 + post_marker_heap.len();
        post_marker_heap.extend(utf16le("World"));
        post_marker_heap.extend([0u8; 4]);

        let mut fixed = Vec::new();
        fixed.extend(42i32.to_le_bytes());
        fixed.push(1u8);
        fixed.extend((hello_offset as u64).to_le_bytes());
        fixed.extend((-7i32).to_le_bytes());
        fixed.push(0u8);
        fixed.extend((world_offset as u64).to_le_bytes());

        let mut file = Vec::new();
        file.extend(2u32.to_le_bytes()); // row_count
        file.extend(&fixed);
        file.extend([0xBBu8; 8]); // heap marker
        file.extend(&post_marker_heap);

        let rows = parse_table(&table, &file).expect("parse_table should succeed");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].values,
            vec![
                Value::I32(42),
                Value::Bool(true),
                Value::String("Hello".into())
            ]
        );
        assert_eq!(
            rows[1].values,
            vec![
                Value::I32(-7),
                Value::Bool(false),
                Value::String("World".into())
            ]
        );
    }

    /// Covers null-reference sentinel handling (`Row`/`ForeignRow`) and array-of-scalar
    /// decoding, the other two byte-layout details most likely to silently misread real data.
    #[test]
    fn parses_a_hand_built_fixture_with_arrays_and_null_refs() {
        let table = SchemaTable {
            valid_for: 2,
            name: "SyntheticRefs".to_owned(),
            columns: vec![
                column("SelfRef", ColumnType::Row, false, false),
                column("ForeignRef", ColumnType::ForeignRow, false, false),
                column("Values", ColumnType::I32, true, false),
            ],
            tags: vec![],
        };

        let mut post_marker_heap = Vec::new();
        let array_offset = 8 + post_marker_heap.len();
        post_marker_heap.extend(10i32.to_le_bytes());
        post_marker_heap.extend(20i32.to_le_bytes());
        post_marker_heap.extend(30i32.to_le_bytes());

        let mut fixed = Vec::new();
        // Row 0: SelfRef=5, ForeignRef=null, Values=[10,20,30].
        fixed.extend(5u64.to_le_bytes());
        // ForeignRow is 16 bytes: [row_index, reserved-always-null]. Only the first slot is the
        // null sentinel the reader checks; the second slot's value is irrelevant/unread.
        fixed.extend(0xFEFE_FEFE_FEFE_FEFEu64.to_le_bytes());
        fixed.extend(0u64.to_le_bytes());
        fixed.extend(3u64.to_le_bytes());
        fixed.extend((array_offset as u64).to_le_bytes());

        let mut file = Vec::new();
        file.extend(1u32.to_le_bytes()); // row_count
        file.extend(&fixed);
        file.extend([0xBBu8; 8]);
        file.extend(&post_marker_heap);

        let rows = parse_table(&table, &file).expect("parse_table should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].values,
            vec![
                Value::Row(Some(5)),
                Value::ForeignRow(None),
                Value::Array(vec![Value::I32(10), Value::I32(20), Value::I32(30)]),
            ]
        );
    }

    /// A coincidental 8x0xBB run inside the fixed row region, at a relative offset that is
    /// *not* a multiple of `row_count`, must be rejected in favor of the true, aligned marker
    /// further along -- not mistaken for the boundary just because it matches first.
    ///
    /// Layout (row_count=3, row width=4, byte offsets relative to byte 4): row 0 =
    /// `[00,00,BB,BB]`, row 1 = `[BB,BB,BB,BB]`, row 2 = `[BB,BB,00,00]`. This plants an 8-byte
    /// run of `0xBB` spanning relative offsets 2..10 (`2 % 3 != 0` -- misaligned, must be
    /// skipped), bounded on both sides by `0x00` guards so it cannot chain into an adjacent
    /// match; the real marker follows immediately after the 12-byte fixed region, at relative
    /// offset 12 (`12 % 3 == 0` -- aligned, the correct boundary).
    #[test]
    fn heap_marker_search_skips_a_misaligned_coincidental_match() {
        let table = SchemaTable {
            valid_for: 2,
            name: "Coincidence".to_owned(),
            columns: vec![column("Filler", ColumnType::I32, false, false)],
            tags: vec![],
        };

        let row0 = [0x00u8, 0x00, 0xBB, 0xBB];
        let row1 = [0xBBu8, 0xBB, 0xBB, 0xBB];
        let row2 = [0xBBu8, 0xBB, 0x00, 0x00];

        let mut file = Vec::new();
        file.extend(3u32.to_le_bytes()); // row_count
        file.extend(row0);
        file.extend(row1);
        file.extend(row2);
        file.extend([0xBBu8; 8]); // the real, row-count-aligned marker

        // Sanity-check the fixture itself plants exactly the intended misaligned false match
        // before parsing, so a future edit to this test can't silently stop testing anything.
        let coincidental_run = &file[6..14];
        assert!(
            coincidental_run.iter().all(|&b| b == 0xBB),
            "fixture setup bug: no coincidental run"
        );

        let rows = parse_table(&table, &file).expect("parse_table should succeed");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].values, vec![Value::I32(i32::from_le_bytes(row0))]);
        assert_eq!(rows[1].values, vec![Value::I32(i32::from_le_bytes(row1))]);
        assert_eq!(rows[2].values, vec![Value::I32(i32::from_le_bytes(row2))]);
    }

    /// Parses a real, committed `.datc64` file: PoE2's actual `BetrayalRanks` table (4 rows, 4
    /// plain-string/i32 columns -- small enough to commit whole rather than needing to trim),
    /// extracted from a real game install during the architecture plan's step 6 validation spike
    /// (see `crates/data-pipeline/SPIKE_FINDINGS.md`). No live share or network needed to run
    /// this test -- the bytes and the schema slice needed to parse them are both committed.
    #[test]
    fn parses_a_real_committed_datc64_fixture() {
        let table = SchemaTable {
            valid_for: 3,
            name: "BetrayalRanks".to_owned(),
            columns: vec![
                column("Id", ColumnType::String, false, false),
                column("Text", ColumnType::String, false, false),
                column("Level", ColumnType::I32, false, false),
                column("RankImage", ColumnType::String, false, false),
            ],
            tags: vec![],
        };

        let raw: &[u8] = include_bytes!("../tests/fixtures/BetrayalRanks.datc64");
        let rows = parse_table(&table, raw).expect("parse_table should succeed on real game data");

        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[0].values,
            vec![
                Value::String("None".into()),
                Value::String("".into()),
                Value::I32(0),
                Value::String("".into()),
            ]
        );
        assert_eq!(
            rows[1].values,
            vec![
                Value::String("Rank1".into()),
                Value::String("Sergeant".into()),
                Value::I32(3),
                Value::String("Art/2DArt/UIImages/InGame/Betrayal/RankIcon1Stars".into()),
            ]
        );
        assert_eq!(
            rows[2].values,
            vec![
                Value::String("Rank2".into()),
                Value::String("Lieutenant".into()),
                Value::I32(2),
                Value::String("Art/2DArt/UIImages/InGame/Betrayal/RankIcon2Stars".into()),
            ]
        );
        assert_eq!(
            rows[3].values,
            vec![
                Value::String("Rank3".into()),
                Value::String("Captain".into()),
                Value::I32(1),
                Value::String("Art/2DArt/UIImages/InGame/Betrayal/RankIcon3Stars".into()),
            ]
        );
    }
}
