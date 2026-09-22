//! Integration test against the real, mounted Bundles2 SMB share (see the workspace root
//! architecture plan's step 1) and a real Oodle DLL. Gated behind the `live-share` feature so
//! `cargo test --workspace` (this crate's default, CI-safe test run) never touches either.
#![cfg(feature = "live-share")]

use std::path::Path;

use poe_bundle::BundleIndex;

#[test]
fn reads_a_real_file_from_the_mounted_share() {
    let bundles2_root =
        std::env::var("POE2_BUNDLES2_ROOT").unwrap_or_else(|_| "/mnt/poe2-bundles".to_owned());
    let dll_path = std::env::var("ODLE_TEST_DLL_PATH")
        .expect("live-share test needs ODLE_TEST_DLL_PATH set to a real oo2core_*_win64.dll");

    let oodle = oodle_ffi::load(Path::new(&dll_path)).expect("failed to load the Oodle DLL");
    let index = BundleIndex::open(Path::new(&bundles2_root), oodle)
        .expect("failed to open the real _.index.bin");

    let bytes = index
        .read_file("Data/Balance/Mods.datc64")
        .expect("failed to read a known real file from the live share");
    assert!(!bytes.is_empty(), "Mods.datc64 decompressed to zero bytes");
}
