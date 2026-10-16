fn main() {
    // Windows test STATUS_ENTRYPOINT_NOT_FOUND fix lives in CI (post-link mt.exe
    // Common Controls v6 on the test harness). Do NOT add /MANIFESTINPUT here:
    // tauri_build already embeds a Windows app manifest; a second one fails
    // link with CVT1100 "duplicate resource" (v0.1.9 release).

    println!("cargo:rerun-if-env-changed=GROK_UPDATER_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=GROK_UPDATER_ENDPOINT");
    println!("cargo:rustc-check-cfg=cfg(grok_updater_enabled)");

    // Release CI injects both env vars so registration is allowed at runtime.
    // Local `tauri dev` leaves env unset → plugin crate is linked for ACL only,
    // never registered.
    let updater_public_key = std::env::var("GROK_UPDATER_PUBLIC_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let updater_endpoint = std::env::var("GROK_UPDATER_ENDPOINT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if let (Some(_key), Some(endpoint)) = (updater_public_key, updater_endpoint) {
        println!("cargo:rustc-cfg=grok_updater_enabled");
        // Expose non-secret endpoint URL to `option_env!` / status DTO.
        println!("cargo:rustc-env=GROK_UPDATER_ENDPOINT={endpoint}");
    }

    stage_engine();
    tauri_build::build()
}

/// Compile `gcu` into `binaries/gcu-<target>` before Tauri checks externalBin.
///
/// The nested cargo uses `computer-use/target/sidecar` so it does not take the
/// App target-dir lock. Clippy wrappers and rustflags from the outer build are
/// removed; they would compile the sidecar with the App's lint driver.
fn stage_engine() {
    let manifest_dir = std::path::PathBuf::from(env_or("CARGO_MANIFEST_DIR"));
    let target = env_or("TARGET");
    let profile = env_or("PROFILE");
    let computer_use = manifest_dir.join("../computer-use");
    let target_dir = computer_use.join("target").join("sidecar");
    let mut command = std::process::Command::new(cargo_bin());
    command
        .current_dir(&computer_use)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO_MAKEFLAGS")
        .env_remove("MAKEFLAGS")
        .env("CARGO_TARGET_DIR", &target_dir)
        .args([
            "build", "-p", "gcu-cli", "--bin", "gcu", "--target", &target, "--locked",
        ]);
    if profile == "release" {
        command.arg("--release");
    }
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to start gcu sidecar build: {error}"));
    if !status.success() {
        panic!("gcu sidecar build failed with {status}");
    }
    let exe = if target.contains("windows") {
        "gcu.exe"
    } else {
        "gcu"
    };
    let folder = if profile == "release" {
        "release"
    } else {
        "debug"
    };
    let built = target_dir.join(&target).join(folder).join(exe);
    let dest_dir = manifest_dir.join("binaries");
    std::fs::create_dir_all(&dest_dir)
        .unwrap_or_else(|error| panic!("create sidecar dir: {error}"));
    let dest_name = if target.contains("windows") {
        format!("gcu-{target}.exe")
    } else {
        format!("gcu-{target}")
    };
    let dest = dest_dir.join(&dest_name);
    std::fs::copy(&built, &dest).unwrap_or_else(|error| panic!("copy gcu sidecar: {error}"));
    println!("cargo:rustc-env=GCU_ENGINE_SHA256={}", sha256_file(&dest));
    println!("cargo:rustc-env=GCU_TARGET={target}");
    forward_identity("GCU_TEAM_ID");
    forward_identity("GCU_SIGNING_ID");
    forward_identity("GCU_CDHASH");
    println!("cargo:rerun-if-changed=../computer-use/Cargo.toml");
    println!("cargo:rerun-if-changed=../computer-use/Cargo.lock");
    println!("cargo:rerun-if-changed=../computer-use/crates");
    println!("cargo:rerun-if-env-changed=GCU_TEAM_ID");
    println!("cargo:rerun-if-env-changed=GCU_SIGNING_ID");
    println!("cargo:rerun-if-env-changed=GCU_CDHASH");
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn env_or(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("missing {key}"))
}

fn forward_identity(key: &str) {
    let Ok(value) = std::env::var(key) else {
        return;
    };
    let value = value.trim();
    if value.is_empty() || value.chars().any(|c| c.is_whitespace()) {
        return;
    }
    println!("cargo:rustc-env={key}={value}");
}

fn sha256_file(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).unwrap_or_else(|error| panic!("hash sidecar: {error}"));
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let read = file
            .read(&mut buffer)
            .unwrap_or_else(|error| panic!("read sidecar: {error}"));
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
