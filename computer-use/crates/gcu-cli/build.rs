//! Windows gives the main thread a 1 MiB stack, and the CLI drives the whole action
//! pipeline from `block_on` on that thread. The unified settle engine made the debug
//! build's async frames deep enough to overflow it (`gcu.exe call get_app_state`
//! died with a stack overflow in CI). Linux and macOS main threads already have 8 MiB.
//! Give the Windows binaries the same 8 MiB. Nothing else changes.

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:8388608"),
        Ok("gnu") => println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608"),
        _ => {}
    }
}
