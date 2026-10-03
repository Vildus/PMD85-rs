//! Windows .exe resources: embeds the app icon (and version info)
//! into the binary so Explorer, shortcuts and file dialogs show it.
//! This is separate from the runtime window icon winit sets.
//!
//! Everything else is a no-op: the script runs on every host but
//! only compiles resources when the *target* is Windows. Building on
//! Windows with the MSVC toolchain works out of the box (`rc.exe`);
//! the GNU toolchain needs `windres` on PATH.

use std::env;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }
    // The icon lives at the repository root, next to its PNG source.
    println!("cargo:rerun-if-changed=../../assets/icon.ico");

    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/icon.ico");
    res.set("FileDescription", "PMD 85 emulator");
    res.set("ProductName", "slop-PMD");
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    // The numeric FILEVERSION/PRODUCTVERSION words (four 16-bit
    // parts packed into a u64, `major.minor.patch` + a trailing 0).
    let mut parts = [0u16; 4];
    for (slot, field) in version.split('.').take(4).enumerate() {
        parts[slot] = field.parse().unwrap_or(0);
    }
    let packed = (u64::from(parts[0]) << 48)
        | (u64::from(parts[1]) << 32)
        | (u64::from(parts[2]) << 16)
        | u64::from(parts[3]);
    res.set_version_info(winresource::VersionInfo::FILEVERSION, packed);
    res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);

    if let Err(e) = res.compile() {
        panic!("cannot embed the Windows resources: {e}");
    }
}
