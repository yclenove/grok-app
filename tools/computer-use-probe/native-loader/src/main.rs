//! Executes the actual product module. Not a full App, portal or desktop test.
#[path = "../../../../src-tauri/src/linux_native_runtime.rs"]
mod linux_native_runtime;

fn main() {
    match linux_native_runtime::prepare() {
        Ok(()) => {
            let expected = std::path::PathBuf::from(
                std::env::var_os("GROK_CU_LOADER_EXPECTED_RESOURCE").expect(
                    "Probe needs expected resource path; product does not use this variable",
                ),
            );
            for (key, relative) in [
                ("SPA_PLUGIN_DIR", "runtime/lib/spa-0.2"),
                ("PIPEWIRE_MODULE_DIR", "runtime/lib/pipewire-0.3"),
                ("PIPEWIRE_CONFIG_DIR", "runtime/share/pipewire"),
            ] {
                assert_eq!(
                    std::env::var_os(key),
                    Some(expected.join(relative).into_os_string()),
                    "{key}"
                );
            }
            assert_eq!(
                std::env::var("PIPEWIRE_CONFIG_NAME").unwrap(),
                "client.conf"
            );
            assert_eq!(std::env::var("PIPEWIRE_CONFIG_PREFIX").unwrap(), "");
            assert_eq!(
                std::env::split_paths(&std::env::var_os("LD_LIBRARY_PATH").unwrap()).next(),
                Some(expected.join("runtime/lib"))
            );
            println!("Product loader identity/resources/environment passed; App/portal/input not accepted");
        }
        Err(error) => {
            eprintln!("Product loader refused: {error}");
            std::process::exit(2);
        }
    }
}
