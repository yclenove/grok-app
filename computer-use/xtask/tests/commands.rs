//! Process-level assertions for development gate exit statuses.

use std::process::Command;

#[test]
fn schema_gates_pending_p0_03_skip_successfully() -> Result<(), Box<dyn std::error::Error>> {
    for args in [vec!["golden"], vec!["golden", "--check"], vec!["budget"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(args)
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout)?,
            "SKIP: not implemented yet: P0-03\n"
        );
        assert!(output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn invalid_command_or_arguments_returns_failure() -> Result<(), Box<dyn std::error::Error>> {
    for args in [vec![], vec!["unknown"], vec!["check-deps", "unexpected"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(args)
            .output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr)?.contains("usage:"));
    }
    Ok(())
}
