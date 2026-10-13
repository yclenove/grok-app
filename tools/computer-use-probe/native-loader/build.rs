#[path = "../../../src-tauri/build/linux_native.rs"]
mod linux_native;

fn main() {
    linux_native::configure(std::path::Path::new(
        &std::env::var_os("CARGO_MANIFEST_DIR").unwrap(),
    ));
    println!("cargo:rerun-if-env-changed=GROK_CU_LOADER_LINK_SDK");
    let sdk = std::env::var_os("GROK_CU_LOADER_LINK_SDK")
        .expect("Explicit private SDK required for probe linking");
    let path = std::path::Path::new(&sdk).join("lib");
    assert!(path.is_absolute() && path.join("libpipewire-0.3.so").is_file());
    println!("cargo:rustc-link-search=native={}", path.display());
}
