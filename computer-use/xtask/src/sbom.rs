//! Offline CycloneDX inventory of the complete locked workspace (all targets).
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub(crate) fn run() -> Result<()> {
    let lock = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock"),
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&generate(&lock, env!("CARGO_PKG_VERSION"))?)?
    );
    Ok(())
}

pub(crate) fn generate(lock: &str, version: &str) -> Result<Value> {
    let lock: toml::Value = toml::from_str(lock)?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .context("lockfile packages missing")?;
    let mut components = Vec::new();
    for package in packages {
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .context("package name missing")?;
        let locked = package
            .get("version")
            .and_then(toml::Value::as_str)
            .context("package version missing")?;
        let local = package.get("source").is_none();
        let release_version = if local { version } else { locked };
        let purl = format!("pkg:cargo/{name}@{release_version}");
        let mut component = json!({"type":"library", "bom-ref":purl, "name":name, "version":release_version, "purl":purl});
        if let Some(hash) = package.get("checksum").and_then(toml::Value::as_str) {
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("invalid locked checksum");
            }
            // Cargo's checksum describes the registry crate archive, not the executable.
            component["hashes"] = json!([{"alg":"SHA-256", "content":hash}]);
        }
        components.push(component);
    }
    components.sort_by_key(|c| c["bom-ref"].as_str().unwrap_or_default().to_owned());
    Ok(json!({
        "bomFormat":"CycloneDX", "specVersion":"1.5", "version":1,
        "metadata": {"component":{"type":"application", "name":"gcu", "version":version},
            "properties":[{"name":"gcu:inventory:scope", "value":"Cargo.lock; all workspace members and targets; includes build/dev dependencies"}]},
        "components":components
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    #[test]
    fn sbom_locked_inventory_versions_hashes_and_scope_are_explicit() {
        let lock = format!("[[package]]\nname='local'\nversion='0.0.0'\n[[package]]\nname='dep'\nversion='2.0.0'\nsource='registry+https://github.com/rust-lang/crates.io-index'\nchecksum='{}'", "a".repeat(64));
        let bom = generate(&lock, "1.2.3").unwrap();
        assert_eq!(bom["bomFormat"], "CycloneDX");
        assert_eq!(bom["components"][0]["hashes"][0]["content"], "a".repeat(64));
        assert_eq!(bom["components"][1]["version"], "1.2.3");
        assert!(bom["metadata"]["properties"][0]["value"]
            .as_str()
            .unwrap()
            .contains("all workspace"));
        assert!(generate("[[package]]\nname='bad'", "1.0.0").is_err());
    }
}
