//! Executable admission checks for both fake-agent entries, without a model.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

fn scratch() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "gcu-fake-admission-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn entry(hidden: bool) -> Command {
    let mut command = Command::new(if hidden {
        env!("CARGO_BIN_EXE_gcu-eval")
    } else {
        env!("CARGO_BIN_EXE_eval-fake-agent")
    });
    if hidden {
        command.arg(gcu_eval::INTERNAL_FAKE_AGENT);
    }
    command.env_clear().env("GCU_EVAL_BEHAVIOR", "crash");
    command
}

#[test]
fn genuine_fake_artifact_and_verified_reentry_work_without_trust_markers() {
    let root = scratch();
    for hidden in [false, true] {
        for reexec in [false, true] {
            let log = root.join(format!("tools-{hidden}-{reexec}.jsonl"));
            let output = entry(hidden)
                .env(
                    "GCU_BIN",
                    if reexec {
                        env!("CARGO_BIN_EXE_gcu-eval")
                    } else {
                        env!("CARGO_BIN_EXE_eval-fake-gcu")
                    },
                )
                .env("GCU_EVAL_GCU_REEXEC", if reexec { "1" } else { "0" })
                .env("GCU_EVAL_TOOL_LOG", &log)
                .output()
                .unwrap();
            // The backend must succeed before the scripted crash returns 101.
            assert_eq!(
                output.status.code(),
                Some(101),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(std::fs::read_to_string(log).unwrap().contains("list_apps"));
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn same_named_external_copy_never_starts_on_any_platform() {
    let root = scratch();
    let external = root.join(if cfg!(windows) {
        "eval-fake-gcu.exe"
    } else {
        "eval-fake-gcu"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_eval-fake-gcu"), &external).unwrap();
    for hidden in [false, true] {
        for flags in 0..8 {
            let log = root.join(format!("sentinel-{hidden}-{flags}.jsonl"));
            let output = entry(hidden)
                .env("GCU_BIN", &external)
                .env("GCU_EVAL_ATTESTED", if flags & 1 != 0 { "1" } else { "0" })
                .env(
                    "GCU_EVAL_BUILTIN_GCU",
                    if flags & 2 != 0 { "1" } else { "0" },
                )
                .env(
                    "GCU_EVAL_GCU_REEXEC",
                    if flags & 4 != 0 { "1" } else { "0" },
                )
                .env("GCU_EVAL_TOOL_LOG", &log)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert!(String::from_utf8_lossy(&output.stderr).contains("provider_unavailable"));
            // A started copy would append list_apps: this is its harmless start sentinel.
            assert!(!std::fs::read_to_string(log).unwrap().contains("list_apps"));
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
