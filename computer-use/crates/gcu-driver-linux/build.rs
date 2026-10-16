//! Build script for gcu-driver-linux.
//!
//! Ensures the linker can resolve `-lxkbcommon` when runtime `libxkbcommon.so.0`
//! is present even if the `libxkbcommon-dev` unversioned symlink is absent.

fn main() {
    #[cfg(unix)]
    {
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        if target_os != "linux" {
            return;
        }

        let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
        if out_dir.is_empty() {
            return;
        }

        // Search common library locations for versioned libxkbcommon.so.0
        let candidates = [
            "/home/build/cu2/lib/libxkbcommon.so.0",
            "/usr/lib/x86_64-linux-gnu/libxkbcommon.so.0",
            "/usr/lib/aarch64-linux-gnu/libxkbcommon.so.0",
            "/usr/lib64/libxkbcommon.so.0",
            "/usr/lib/libxkbcommon.so.0",
        ];

        let symlink_path = std::path::Path::new(&out_dir).join("libxkbcommon.so");
        if !symlink_path.exists() {
            for &candidate in &candidates {
                if std::path::Path::new(candidate).exists() {
                    let _ = std::os::unix::fs::symlink(candidate, &symlink_path);
                    break;
                }
            }
        }

        if symlink_path.exists() {
            println!("cargo:rustc-link-search=native={out_dir}");
        }
    }
}
