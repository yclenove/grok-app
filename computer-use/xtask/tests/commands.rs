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
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(args)
            .output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr)?.contains("usage:"));
    }
    Ok(())
}
