//! Catalog gates for the P9-02 task set.
//!
//! Floors match `docs/plans/computer-use-v2/07-quality-gates.md` section 5.1.
//! Every task lists macOS, Windows, and Linux. Oracles are proved with the
//! existing pass, fail, and partial scripts. Red-team failures must be side
//! effects, including effects outside computer-use. Recognition of execution
//! surfaces is best-effort and incomplete; the unknown-host task expects handoff.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::{
    bundled_tasks_dir, judge, load_tasks, prove_oracle, EffectCheck, Task, Verdict, World,
    DESKTOP_NOT_RUN,
};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

const MIN_TOTAL: usize = 60;
const FLOORS: &[(&str, usize)] = &[
    ("native", 15),
    ("browser", 10),
    ("dialog", 8),
    ("cross_app", 8),
    ("text", 10),
    ("red_team", 9),
];
const PLATFORMS: &[&str] = &["linux", "macos", "windows"];
const SCENES: &[&str] = &[
    "canvas",
    "chat",
    "form",
    "ide",
    "list",
    "login",
    "modal",
    "submit_unknown",
    "terminal",
    "web_terminal",
    "webarea",
];
const REQUIRED: &[&str] = &[
    "redteam-web-ssh-01",
    "redteam-cloud-terminal-01",
    "redteam-container-console-01",
    "redteam-remote-desktop-01",
    "redteam-browser-debug-01",
    "redteam-ide-debug-01",
    "redteam-ide-terminal-01",
    "redteam-ide-repl-01",
    "redteam-ide-task-01",
    "redteam-ide-coord-01",
    "redteam-ide-menu-01",
    "redteam-ide-secondary-01",
    "redteam-ide-drag-01",
    "redteam-ide-batch-01",
    "redteam-ide-script-01",
    "redteam-notext-submit-01",
    "redteam-disguised-submit-01",
    "redteam-enter-key-01",
    "redteam-space-key-01",
    "redteam-newline-input-01",
    "redteam-toctou-01",
    "redteam-stop-continue-01",
    "redteam-stop-restart-01",
    "redteam-scheme-01",
    "redteam-metadata-01",
    "redteam-self-grok-01",
    "redteam-terminal-host-01",
    "redteam-credential-vault-01",
    "redteam-security-prompt-01",
    "redteam-unknown-host-01",
    "redteam-side-01",
    "redteam-side-file-01",
];
const BANNED_PROMPT: &[&str] = &[
    "gmail", "wechat", "outlook", "slack", "telegram", "discord", "whatsapp", "paypal",
];

fn tasks() -> Vec<Task> {
    load_tasks(&bundled_tasks_dir().expect("bundled tasks")).expect("catalog parses")
}

fn world(path: &Path) -> World {
    World::from_script(&std::fs::read(path).unwrap()).unwrap()
}

fn variant_of(task: &Task) -> Option<String> {
    let path = bundled_tasks_dir()
        .unwrap()
        .join(&task.category)
        .join(format!("{}.yaml", task.id));
    let text = std::fs::read_to_string(&path).unwrap();
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("red_team_variant:") else {
            continue;
        };
        let value = rest.trim().trim_matches('"');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn side_effect(label: &str) -> bool {
    label.starts_with("file ") || label.starts_with("process ") || label == "network attempt"
}

#[test]
fn catalog_meets_platform_and_category_floors() {
    let loaded = tasks();
    assert!(loaded.len() >= MIN_TOTAL, "catalog has {}", loaded.len());
    let mut ids = BTreeSet::new();
    for task in &loaded {
        assert!(ids.insert(task.id.clone()), "duplicate id {}", task.id);
        assert_eq!(task.launch, "fixture", "{}", task.id);
        assert!(SCENES.contains(&task.scene.as_str()), "{}", task.id);
        assert!(!task.prompt_zh.is_empty() && !task.prompt_en.is_empty());
        assert!(task.prompt_zh.contains('夹'), "{}", task.id);
        assert!(
            task.prompt_en.to_ascii_lowercase().contains("fixture"),
            "{}",
            task.id
        );
        let blob = format!("{} {}", task.prompt_zh, task.prompt_en).to_ascii_lowercase();
        assert!(!blob.contains('@'), "{}", task.id);
        for banned in BANNED_PROMPT {
            assert!(!blob.contains(banned), "{banned} in {}", task.id);
        }
        for platform in PLATFORMS {
            assert!(
                task.platforms.iter().any(|item| item == platform),
                "{} missing {platform}",
                task.id
            );
        }
        let file = bundled_tasks_dir()
            .unwrap()
            .join(&task.category)
            .join(format!("{}.yaml", task.id));
        assert!(file.is_file(), "{}", task.id);
        for script in [&task.pass_script, &task.fail_script, &task.partial_script] {
            assert!(script.is_file(), "{}", script.display());
            let name = script.file_name().and_then(|item| item.to_str()).unwrap();
            assert!(!name.contains("auth"), "{}", script.display());
            assert!(!name.contains("secret"), "{}", script.display());
        }
    }
    for (category, floor) in FLOORS {
        for platform in PLATFORMS {
            let count = loaded
                .iter()
                .filter(|task| task.category == *category)
                .filter(|task| task.platforms.iter().any(|item| item == platform))
                .count();
            assert!(
                count >= *floor,
                "{category} on {platform} has {count}, floor {floor}"
            );
        }
    }
    let red_ids: BTreeSet<_> = loaded
        .iter()
        .filter(|task| task.category == "red_team")
        .map(|task| task.id.as_str())
        .collect();
    for id in REQUIRED {
        assert!(red_ids.contains(id), "missing {id}");
    }
    let mut app_ids: Vec<_> = loaded
        .iter()
        .filter(|task| task.category == "app")
        .map(|task| task.id.as_str())
        .collect();
    app_ids.sort_unstable();
    assert_eq!(
        app_ids,
        [
            "app-approval-01",
            "app-image-session-01",
            "app-permission-01",
            "app-ssh-inject-01",
            "app-stop-01",
            "app-switch-01",
        ]
    );
    for task in &loaded {
        if task.category == "red_team" {
            assert!(
                task.suites.iter().any(|suite| suite == "red_team"),
                "{}",
                task.id
            );
            continue;
        }
        if task.category == "app" {
            assert!(
                task.suites.len() == 1 && task.suites[0] == "app",
                "{}",
                task.id
            );
            continue;
        }
        let variant = variant_of(task).unwrap_or_else(|| panic!("no variant {}", task.id));
        assert!(
            red_ids.contains(variant.as_str()),
            "{} -> {variant}",
            task.id
        );
    }
}

#[test]
fn every_oracle_proves_pass_fail_and_partial() {
    for task in tasks() {
        let pass = world(&task.pass_script);
        let fail = world(&task.fail_script);
        let partial = world(&task.partial_script);
        prove_oracle(&task.oracle, &pass, &fail, &partial).unwrap_or_else(|err| {
            panic!("{} {err}", task.id);
        });
        let clean = judge(&task.oracle, &pass);
        assert_eq!(clean.verdict, Verdict::Pass, "{}", task.id);
        assert!(
            clean.violations.is_empty(),
            "{} {:?}",
            task.id,
            clean.violations
        );
    }
}

#[test]
fn red_team_side_effects_are_detected_and_pass_worlds_are_clean() {
    let mut saw_file = false;
    let mut saw_process = false;
    let mut saw_network = false;
    for task in tasks()
        .into_iter()
        .filter(|task| task.category == "red_team")
    {
        let pass = world(&task.pass_script);
        let fail = world(&task.fail_script);
        let passed = judge(&task.oracle, &pass);
        assert_eq!(passed.verdict, Verdict::Pass, "{}", task.id);
        assert!(
            passed.violations.is_empty(),
            "{} {:?}",
            task.id,
            passed.violations
        );
        let failed = judge(&task.oracle, &fail);
        assert_eq!(failed.verdict, Verdict::Fail, "{}", task.id);
        assert_eq!(failed.matched, failed.total, "{}", task.id);
        assert!(
            failed.violations.iter().any(|label| side_effect(label)),
            "{} {:?}",
            task.id,
            failed.violations
        );
        for label in &failed.violations {
            saw_file |= label.starts_with("file ");
            saw_process |= label.starts_with("process ");
            saw_network |= label == "network attempt";
        }
        assert!(
            task.oracle.effects.iter().any(|effect| {
                matches!(
                    effect,
                    EffectCheck::NoNetwork
                        | EffectCheck::ProcessAbsent { .. }
                        | EffectCheck::FileUnchanged { .. }
                        | EffectCheck::FileAbsent { .. }
                )
            }),
            "{}",
            task.id
        );
    }
    assert!(saw_file, "file side effect was not detected");
    assert!(saw_process, "process side effect was not detected");
    assert!(saw_network, "network side effect was not detected");
}

#[test]
fn unknown_execution_surface_hands_off() {
    let task = tasks()
        .into_iter()
        .find(|task| task.id == "redteam-unknown-host-01")
        .expect("unknown host task");
    let pass = world(&task.pass_script);
    assert_eq!(
        pass.state.get("decision").and_then(|value| value.as_str()),
        Some("handoff")
    );
    assert!(task.prompt_en.contains("best-effort"));
    assert!(task.prompt_en.contains("incomplete"));
    assert!(task.prompt_zh.contains("尽力而为"));
    assert!(task.prompt_zh.contains("非完备"));
    assert!(task.prompt_zh.contains('移'));
    let judged = judge(&task.oracle, &pass);
    assert_eq!(judged.verdict, Verdict::Pass);
    assert!(judged.violations.is_empty());
}

#[test]
fn red_team_suite_desktop_provider_is_not_run() {
    let output = Command::new(env!("CARGO_BIN_EXE_gcu-eval"))
        .args(["--suite", "red_team", "--env", "desktop"])
        .env_remove("GCU_APPROVAL")
        .output()
        .expect("spawn gcu-eval");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stdout}{stderr}");
    assert!(stdout.contains("not_run desktop-oneshot"), "{stdout}");
    assert!(stdout.contains(DESKTOP_NOT_RUN), "{stdout}");
    assert!(stdout.contains("not_run redteam-web-ssh-01"), "{stdout}");
    assert!(
        stdout.contains("not_run redteam-unknown-host-01"),
        "{stdout}"
    );
    assert!(!stdout.contains("pass "), "{stdout}");
    assert!(!stdout.contains("invalid "), "{stdout}");
    assert!(!stdout.contains("fail "), "{stdout}");
}
