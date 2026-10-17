//! Process-level assertions for development gate exit statuses.

use std::process::Command;

#[test]
fn schema_gates_generated_surface_passes() -> Result<(), Box<dyn std::error::Error>> {
    for args in [vec!["golden", "--check"], vec!["budget"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(args)
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8(output.stdout)?.contains("PASS:"));
        assert!(output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn invalid_command_or_arguments_returns_failure() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec![],
        vec!["unknown"],
        vec!["check-deps", "unexpected"],
        vec!["golden"],
        vec!["golden", "--invalid"],
        vec!["golden", "--check", "--update"],
        vec!["budget", "unexpected"],
        vec!["sbom", "unexpected"],
        vec!["ui"],
        vec!["ui", "nope"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(args)
            .output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr)?.contains("usage:"));
    }
    Ok(())
}

#[test]
fn release_dry_run_empty_path_never_executes_or_writes() -> Result<(), Box<dyn std::error::Error>> {
    let scratch = std::env::temp_dir().join(format!("gcu-release-dry-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["release", "--dry-run", "--tag", "cu-v1.2.3", "--output"])
        .arg(scratch.join("must-not-exist"))
        .current_dir(&scratch)
        .env("PATH", "")
        // Invalid configured credentials must not be inspected in an offline dry run.
        .env("APPLE_CERTIFICATE", "invalid-not-a-certificate")
        .env("WINDOWS_CERTIFICATE", "invalid-not-a-certificate")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(plan["dry_run"], true);
    assert_eq!(plan["publish"], false);
    assert_eq!(plan["version"], "1.2.3");
    assert_eq!(plan["targets"].as_array().map(Vec::len), Some(5));
    assert_eq!(std::fs::read_dir(&scratch)?.count(), 0);
    std::fs::remove_dir(&scratch)?;
    Ok(())
}

#[test]
fn sbom_command_emits_parseable_locked_inventory() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("sbom")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let bom: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(bom["bomFormat"], "CycloneDX");
    assert!(!bom["components"]
        .as_array()
        .ok_or("missing components")?
        .is_empty());
    Ok(())
}
