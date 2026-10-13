//! Shared by the App and the native loader probe, so its link policy cannot drift.
pub fn configure(manifest_dir: &std::path::Path) {
    println!("cargo:rustc-check-cfg=cfg(grok_private_pipewire)");
    for name in ["GROK_CU_BUNDLE_TARGET", "TAURI_ENV_TARGET_TRIPLE"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let manifest = manifest_dir.join("resources/computer-use/native-pipewire/manifest.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    if manifest.is_file() {
        assert_eq!(
            std::env::var("CARGO_CFG_TARGET_ARCH").as_deref(),
            Ok("x86_64"),
            "Private PipeWire resources support only Linux x64"
        );
        println!("cargo:rustc-cfg=grok_private_pipewire");
        // DT_RPATH, not RUNPATH: private PW must precede a host-WebKit
        // LD_LIBRARY_PATH, before Rust can inspect the loaded identity.
        println!("cargo:rustc-link-arg=-Wl,--disable-new-dtags,-rpath,$ORIGIN/../lib/grok-app/resources/computer-use/native-pipewire/runtime/lib:$ORIGIN/resources/computer-use/native-pipewire/runtime/lib");
    } else if std::env::var("PROFILE").as_deref() == Ok("release")
        || std::env::var("GROK_CU_BUNDLE_TARGET").as_deref() == Ok("x86_64-unknown-linux-gnu")
        || std::env::var("TAURI_ENV_TARGET_TRIPLE").as_deref() == Ok("x86_64-unknown-linux-gnu")
    {
        panic!("Linux bundle requires staged private PipeWire; see docs/BUILD.md");
    }
}
