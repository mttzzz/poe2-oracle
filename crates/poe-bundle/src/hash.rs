//! Virtual-path hashing: MurmurHash64A, seed `0x1337B33F` -- the current, and PoE2's only-ever,
//! scheme (retired the older FNV1a64-plus-`"++"`-suffix scheme in patch 3.21.2, a year before
//! PoE2 launched). Confirmed identical, field-for-field, across four independent
//! implementations: `aianlinb/LibGGPK3` (C#), `juddisjudd/ggpk-explorer` (Rust),
//! `Project-Path-of-Exile-Wiki/PyPoE`'s `murmur2.py` (the exact file the community wiki itself
//! links as its reference), and the canonical public-domain `MurmurHash64A.c` by Austin Appleby.

/// Hashes a virtual bundle path the way PoE2's own index does: lowercase, UTF-8, at most one
/// trailing `/` stripped, no `"++"` suffix. An empty path (after stripping) hashes to a
/// hard-coded sentinel constant that real bundle indices also special-case, rather than running
/// the general algorithm on zero bytes.
pub fn hash_path(path: &str) -> u64 {
    /// `LibBundle3/Index.cs`'s `NameHash()` fast path for an empty name; doubles as the
    /// hash-scheme-detection sentinel other tooling uses (irrelevant here -- PoE2 has only ever
    /// used this one scheme).
    const EMPTY_PATH_HASH: u64 = 0xF42A_94E6_9CFF_42FE;
    const SEED: u64 = 0x1337_B33F;

    // Real Rust prior art (`ggpk-explorer`) lowercases with `to_ascii_lowercase`, not a
    // Unicode-aware lowercase -- every real bundle path observed across all sources is plain
    // ASCII, so this matches the one real Rust PoE2 tool found rather than over-generalizing.
    let lower = path.to_ascii_lowercase();
    let trimmed = lower.strip_suffix('/').unwrap_or(&lower);
    if trimmed.is_empty() {
        return EMPTY_PATH_HASH;
    }
    murmur_hash64a(trimmed.as_bytes(), SEED)
}

/// The public-domain MurmurHash64A algorithm (Austin Appleby), x64 variant. Reads each 8-byte
/// block as little-endian explicitly (portable on any host, unlike the canonical C reference's
/// native-endian pointer cast, which is only correct on a little-endian machine) and folds the
/// trailing `len % 8` bytes exactly as the reference's byte-at-a-time switch-fallthrough does:
/// zero-padded into the high bytes of one little-endian word, XORed in once, then multiplied
/// once -- not once per tail byte.
fn murmur_hash64a(data: &[u8], seed: u64) -> u64 {
    const M: u64 = 0xC6A4_A793_5BD1_E995;
    const R: u32 = 47;

    let mut h = seed ^ (data.len() as u64).wrapping_mul(M);

    let (chunks, remainder) = data.as_chunks::<8>();
    for chunk in chunks {
        let mut k = u64::from_le_bytes(*chunk);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h ^= k;
        h = h.wrapping_mul(M);
    }

    if !remainder.is_empty() {
        let mut tail = [0u8; 8];
        tail[..remainder.len()].copy_from_slice(remainder);
        h ^= u64::from_le_bytes(tail);
        h = h.wrapping_mul(M);
    }

    h ^= h >> R;
    h = h.wrapping_mul(M);
    h ^= h >> R;
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented empty-path sentinel, independent of the general algorithm.
    #[test]
    fn empty_path_hashes_to_the_documented_sentinel() {
        assert_eq!(hash_path(""), 0xF42A_94E6_9CFF_42FE);
        assert_eq!(
            hash_path("/"),
            0xF42A_94E6_9CFF_42FE,
            "a lone '/' strips to empty"
        );
    }

    /// Case and one trailing slash must not change the hash -- this is what lets `poe-bundle`
    /// look up a path regardless of how a caller happened to capitalize or terminate it.
    #[test]
    fn hash_is_case_and_trailing_slash_insensitive() {
        let base = hash_path("Art/2DArt/Foo.dds");
        assert_eq!(hash_path("art/2dart/foo.dds"), base);
        assert_eq!(hash_path("art/2dart/foo.dds/"), base);
    }

    /// Cross-checked against an independent Python reimplementation of the same documented
    /// algorithm (seed `0x1337B33F`, MurmurHash64A), not derived from this Rust code.
    #[test]
    fn matches_independently_computed_reference_values() {
        let cases: &[(&str, u64)] = &[
            ("Art/2DArt/Foo.dds", 0xEA60_093B_1CBB_65B0),
            ("Data/Balance/Mods.datc64", 0xA1E0_359F_98D9_412B),
            (
                "Metadata/Items/Weapons/OneHandWeapons/Claws/ClawA.it",
                0x2198_E9E3_F943_D9B4,
            ),
        ];
        for (path, expected) in cases {
            assert_eq!(hash_path(path), *expected, "mismatch for {path:?}");
        }
    }
}
