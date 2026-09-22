//! Dynamically loads a real, RAD-authored Oodle compression DLL (`OodleLZ_Decompress`) and wraps
//! it behind a safe API. Deliberately does NOT vendor or reimplement Oodle's algorithm (e.g.
//! `powzix/ooz`, seven years stale with no formal license) -- this crate only ever calls a
//! genuine binary the caller supplies, sourced from any RAD-licensed game that ships one loose
//! (PoE2 itself statically links Oodle into its own exe and ships no loose `oo2core*.dll` --
//! confirmed both by this project's own search of a real PoE2 install and independently, by two
//! unrelated PoE2-modding projects' own notes: `EsintisiYeter/poe2-turkce-yama` and
//! `rocky6777/poe2-polish-patch`).
//!
//! The DLL path is a runtime parameter, never bundled or hardcoded here -- see
//! `crates/data-pipeline` for the one caller that supplies it today. This is the bottom of the
//! workspace's local game-data dependency chain: `poe-bundle` depends on this crate for
//! decompression, never the reverse.
//!
//! ## `OodleLZ_Decompress` signature
//!
//! The exact 14-parameter C signature below was confirmed against RAD/Epic's real `oodle2.h`
//! (Oodle SDK 2.9.11, mirrored at `WorkingRobot/OodleUE`) and independently cross-checked
//! against bindgen-generated Rust bindings (`sehnryr/oodle-sys`, SDK 2.9.10) and a third, current
//! (v2.9.12) C# P/Invoke declaration (`aianlinb/LibGGPK3`'s `LibBundle3/Oodle.cs`) -- three
//! independently-produced artifacts of the same vendor source agree field-for-field. On x64
//! (the only width this crate targets), every `OO_SINTa` parameter is a pointer-width (8-byte)
//! signed integer (`isize` in Rust), not a 32-bit `int` -- several stale community references
//! (e.g. an older `quickbms` binding, explicitly labeled "Oodle 2.3.0") use the wrong, older
//! 32-bit-sized variant; do not copy that shape.
//!
//! The vendor header declares `__stdcall` linkage on Windows (`OOEXPLINK`); Rust's
//! `extern "system"` is the portable spelling for that (maps to `__stdcall` on `i686-pc-windows-*`
//! and to the one unified x64 ABI on `x86_64-pc-windows-*`), and is the idiomatic choice the
//! `windows`/`winapi` crates themselves use for `__stdcall`-declared Win32-style APIs.
//!
//! Exported symbol name is the plain, undecorated C string `"OodleLZ_Decompress"` -- the header's
//! `extern "C"` linkage defeats C++ name mangling, and x64 MSVC applies no `@N` stdcall
//! decoration (that is a 32-bit-only artifact). Every real-world consuming binding surveyed
//! (C#, Go, Rust, and this crate) resolves this exact plain name against the modern win64 DLL.
use std::ffi::c_void;
use std::path::Path;

use anyhow::{Context, Result, bail};
use libloading::{Library, Symbol};

/// Raw FFI shape of `OodleLZ_Decompress`. Field names/order/types match `oodle2.h` exactly (see
/// module doc comment for the cross-checked sources); only the C-only enum/pointer types are
/// narrowed to their Rust FFI equivalents (`i32` for the plain-`enum` parameters -- C's default
/// enum underlying width -- `*const c_void`/`*mut c_void` for the untyped buffer/callback
/// pointers).
type OodleLzDecompressFn = unsafe extern "system" fn(
    comp_buf: *const c_void,
    comp_buf_size: isize,
    raw_buf: *mut c_void,
    raw_len: isize,
    fuzz_safe: i32,
    check_crc: i32,
    verbosity: i32,
    dec_buf_base: *mut c_void,
    dec_buf_size: isize,
    fp_callback: *const c_void,
    callback_user_data: *const c_void,
    decoder_memory: *mut c_void,
    decoder_memory_size: isize,
    thread_phase: i32,
) -> isize;

/// `OodleLZ_FuzzSafe_Yes`. The vendor header: "should always be ...Yes as of Oodle 2.9.0 ...
/// Use of OodleLZ_FuzzSafe_No is deprecated."
const FUZZ_SAFE_YES: i32 = 1;
/// `OodleLZ_CheckCRC_No`. `Yes` only works if the source data was compressed with
/// `sendQuantumCRCs`, not guaranteed for arbitrary bundle data; every surveyed binding uses `No`.
const CHECK_CRC_NO: i32 = 0;
/// `OodleLZ_Verbosity_None`: silences Oodle's internal logging.
const VERBOSITY_NONE: i32 = 0;
/// `OodleLZ_Decode_Unthreaded` (numerically identical to `OodleLZ_Decode_ThreadPhaseAll`, both
/// `3`): a single synchronous call, no hand-rolled 2-phase threaded decode pipeline.
const THREAD_PHASE_UNTHREADED: i32 = 3;
/// `OODLELZ_FAILED`: the documented sentinel return value on total failure/corruption.
const OODLELZ_FAILED: isize = 0;

/// A loaded Oodle DLL, resolved and ready to decompress buffers.
///
/// `_library` must outlive `decompress_fn` -- both are dropped together when this value is
/// dropped, so `decompress_fn` (a plain, `Copy` function pointer, not a borrow) is never called
/// after the DLL that owns it has been unloaded.
pub struct OodleDecompressor {
    _library: Library,
    decompress_fn: OodleLzDecompressFn,
}

/// Loads `dll_path` and resolves its `OodleLZ_Decompress` export.
///
/// Fails with a clear error (never panics) if the file can't be loaded as a native library, or
/// if it loads but has no `OodleLZ_Decompress` export -- e.g. the wrong DLL, wrong architecture,
/// or (on a non-Windows host) any Windows PE DLL at all, since dynamic loading is host-OS-native.
pub fn load(dll_path: &Path) -> Result<OodleDecompressor> {
    // SAFETY: loading an arbitrary native library can run its DllMain/constructors; this is the
    // inherent, unavoidable contract of dynamic loading (`libloading::Library::new`'s own safety
    // note). The caller supplies `dll_path`; this crate never picks or downloads one itself.
    let library = unsafe { Library::new(dll_path) }
        .with_context(|| format!("failed to load Oodle library at {}", dll_path.display()))?;
    // SAFETY: `library.get` requires the resolved symbol to actually have the signature we
    // declare. `OodleLzDecompressFn` is the vendor-header-matched shape documented above; a
    // mismatched real-world DLL would surface as a bad decompress result or a crash inside the
    // DLL's own code, not memory-unsafety introduced by this binding itself.
    let decompress_fn = unsafe {
        let symbol: Symbol<OodleLzDecompressFn> = library
            .get(b"OodleLZ_Decompress\0")
            .context("OodleLZ_Decompress export not found in the supplied DLL")?;
        *symbol
    };
    Ok(OodleDecompressor {
        _library: library,
        decompress_fn,
    })
}

impl OodleDecompressor {
    /// Decompresses `compressed` into exactly `decompressed_size` bytes.
    ///
    /// Calls `OodleLZ_Decompress` with the vendor-recommended "simple one-shot" arguments (see
    /// module doc comment): fuzz-safe decoding, no CRC check, silent, no dictionary priming, no
    /// callback, unthreaded. Fails if the DLL reports corruption (`OODLELZ_FAILED`/`0`) or writes
    /// a different byte count than `decompressed_size` -- the caller is expected to already know
    /// the exact decompressed size (from a bundle/table-of-contents header), so a mismatch means
    /// something upstream is wrong, not that this function should guess and retry.
    pub fn decompress(&self, compressed: &[u8], decompressed_size: usize) -> Result<Vec<u8>> {
        let mut out = vec![0u8; decompressed_size];
        // SAFETY: `compressed`/`out` are valid, correctly-sized slices for the duration of this
        // call; their raw pointers and lengths are passed through exactly as `OodleLZ_Decompress`
        // documents. No pointer is retained past this call.
        let written = unsafe {
            (self.decompress_fn)(
                compressed.as_ptr().cast::<c_void>(),
                compressed.len() as isize,
                out.as_mut_ptr().cast::<c_void>(),
                out.len() as isize,
                FUZZ_SAFE_YES,
                CHECK_CRC_NO,
                VERBOSITY_NONE,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
                THREAD_PHASE_UNTHREADED,
            )
        };
        if written == OODLELZ_FAILED {
            bail!(
                "OodleLZ_Decompress reported failure/corruption (returned 0) for a {}-byte \
                 input targeting {decompressed_size} decompressed bytes",
                compressed.len()
            );
        }
        if written as usize != decompressed_size {
            bail!("OodleLZ_Decompress wrote {written} bytes, expected exactly {decompressed_size}");
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decompresses a real Oodle-Kraken-compressed file (`powzix/ooz`'s own public `xml.kraken`
    /// test fixture, from the Silesia compression corpus -- public-domain-ish benchmark text,
    /// not PoE data, so no PoE-specific licensing question attaches to it) and asserts the
    /// output matches the corresponding uncompressed reference byte-for-byte.
    ///
    /// Needs a real `oo2core_*_win64.dll` to run, supplied via `ODLE_TEST_DLL_PATH` -- loading a
    /// real Windows PE DLL only succeeds on an actual Windows host regardless of this env var
    /// (dynamic loading is host-OS-native), so this test is a no-op skip everywhere else,
    /// including this workspace's own Linux CI lane. Per the architecture plan: do not fail CI
    /// on machines without a copy of the DLL.
    #[test]
    fn decompresses_real_kraken_fixture_against_a_real_dll() {
        let Ok(dll_path) = std::env::var("ODLE_TEST_DLL_PATH") else {
            eprintln!(
                "skipping decompresses_real_kraken_fixture_against_a_real_dll: set \
                 ODLE_TEST_DLL_PATH to a real oo2core_*_win64.dll to run this test"
            );
            return;
        };
        let decompressor =
            load(Path::new(&dll_path)).expect("failed to load the DLL at ODLE_TEST_DLL_PATH");

        let compressed: &[u8] = include_bytes!("../tests/fixtures/xml.kraken");
        let expected: &[u8] = include_bytes!("../tests/fixtures/xml");

        let actual = decompressor
            .decompress(compressed, expected.len())
            .expect("OodleLZ_Decompress failed against the real DLL");
        assert_eq!(
            actual, expected,
            "decompressed bytes did not match the reference xml file"
        );
    }
}
