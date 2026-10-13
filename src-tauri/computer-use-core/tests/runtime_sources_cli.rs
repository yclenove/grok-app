//! CLI argument errors must not accidentally prepare/publish a seed.
use std::process::Command;

fn cli(args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cu-prepare-runtime"));
    command.args(args).env_clear();
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.output().unwrap()
}

#[test]
fn conflicting_modes_are_not_last_flag_wins() {
    for args in [
        vec!["--check", "--verify-sources"],
        vec!["--verify-sources", "--prepare"],
        vec!["--verify-sources", "--verify-sources"],
    ] {
        let result = cli(&args);
        assert_eq!(result.status.code(), Some(2));
        assert!(String::from_utf8(result.stderr)
            .unwrap()
            .contains("exactly one mode"));
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn missing_values_do_not_fall_back_to_environment_or_defaults() {
    for flag in ["--target", "--repo", "--seed", "--cache", "--lock"] {
        for args in [
            vec!["--verify-sources", flag],
            vec!["--verify-sources", flag, "--check"],
        ] {
            let result = cli(&args);
            assert_eq!(result.status.code(), Some(2));
            assert!(String::from_utf8(result.stderr)
                .unwrap()
                .contains(&format!("{flag} requires a value")));
            assert!(result.stdout.is_empty());
        }
    }
}

#[test]
fn source_audit_is_described_in_help_and_requires_explicit_target() {
    let help = cli(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout)
        .unwrap()
        .contains("--verify-sources"));
    let result = cli(&["--verify-sources"]);
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("--target is required"));
}
