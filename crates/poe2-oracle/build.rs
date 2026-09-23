//! Windows resources for the shipped exe: the app icon and a VERSIONINFO block, so Explorer, the
//! taskbar, Task Manager and the file's Properties show the PoE2 Oracle mark, name and version
//! instead of a generic executable.
//!
//! Only those two: gpui's own build script already links the application manifest (RT_MANIFEST 1,
//! `gpui/resources/windows/gpui.manifest.xml` at the pinned rev), and a second one would be a
//! duplicate-resource link error. Zed splits it the same way -- its app exe, which links gpui,
//! embeds icon + version without a manifest (`windows_resources::compile(false)`).

use std::env;
use std::fs;
use std::path::PathBuf;

const ICON: &str = "assets/icon/poe2-oracle.ico";

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    // Build scripts run on the host, so `cfg!(windows)` would describe the Linux lane rather than
    // the exe being built: the target decides. The lane's windows-gnu cross builds embed through
    // mingw's windres; native Linux builds (CI's test/clippy passes) have nothing to embed.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo")).join(ICON);
    // A string literal in the script: backslashes escaped, and UTF-8 declared so a build path
    // with non-ASCII characters survives rc.exe (which otherwise reads the ANSI code page).
    let icon = icon.to_str().expect("UTF-8 path").replace('\\', "\\\\");
    let version = env!("CARGO_PKG_VERSION");
    let numeric = format!(
        "{},{},{},0",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH"),
    );
    // Icon id 1 is a contract, not a choice: gpui_windows loads exactly resource 1 as its window
    // class icon (`load_icon` in platform.rs at the pinned rev), so every GPUI window's taskbar
    // button and Alt+Tab entry show the mark too; being the only group, it is also the exe's icon
    // in Explorer. String values end in an explicit NUL, as in Zed's own resource script.
    let script = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "mttzzz\0"
            VALUE "FileDescription", "PoE2 Oracle\0"
            VALUE "FileVersion", "{version}\0"
            VALUE "InternalName", "poe2-oracle\0"
            VALUE "LegalCopyright", "Copyright (c) 2026 mttzzz\0"
            VALUE "OriginalFilename", "poe2-oracle.exe\0"
            VALUE "ProductName", "PoE2 Oracle\0"
            VALUE "ProductVersion", "{version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );

    let script_path =
        PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo")).join("poe2-oracle.rc");
    fs::write(&script_path, script).expect("writing the resource script");
    // `manifest_required` although this is no manifest: it is the variant that fails the build
    // when no resource compiler is found, instead of silently shipping an exe without its icon.
    embed_resource::compile(&script_path, embed_resource::NONE)
        .manifest_required()
        .expect("compiling the Windows resources");
}
