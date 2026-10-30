//! Round-4 fail-closed checks for library launch slots, telemetry, and A/B halt.
//! These tests use local scripts or the built-in fake agent. They do not call a model.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::ab::{run_ab, ArmId, ReportId};
use gcu_eval::{
    block_on, bundled_tasks_dir, host_platform, load_tasks, run_tasks, write_reports, RunConfig,
    RunStatus, Task, TelemetryState,
};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "gcu-ev-r4-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn bin(name: &str) -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.file_name().and_then(|part| part.to_str()) == Some("deps") {
        path.pop();
    }
    path.push(name);
    path
}

fn agent() -> PathBuf {
    bin("eval-fake-agent")
}

fn gcu() -> PathBuf {
    bin("eval-fake-gcu")
}

fn named(id: &str) -> Task {
    load_tasks(&bundled_tasks_dir().unwrap())
        .unwrap()
        .into_iter()
        .find(|task| task.id == id)
        .unwrap()
}

fn executable(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn touch_script(path: &Path, marker: &Path) {
    let body = format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display());
    std::fs::write(path, body).unwrap();
    executable(path);
}

fn preflight_listing() -> &'static str {
    r#"{"tools":["list_apps","list_windows","search_tool","todo_write","use_tool"]}"#
}

/// External client that answers preflight and then exits 0.
/// The task path touches `marker` and writes a passing fixture state.
/// It does not create the tool log or the side log.
fn silent_success_client(dir: &Path, marker: &Path) -> PathBuf {
    let script = dir.join("grok.sh");
    let body = format!(
        r#"#!/bin/sh
prompt=
prev=
for arg in "$@"; do
  if [ "$prev" = "-p" ]; then
    prompt=$arg
  fi
  prev=$arg
done
case "$prompt" in
  *"List the exact names"*)
    printf '%s\n' '{listing}'
    exit 0
    ;;
esac
touch '{marker}'
printf '%s\n' '{{"form":{{"name":"\u5f20\u4e09","agree":true,"submitted_count":1}}}}' > "$GCU_FIXTURE_STATE"
exit 0
"#,
        listing = preflight_listing(),
        marker = marker.display(),
    );
    std::fs::write(&script, body).unwrap();
    executable(&script);
    script
}

fn remove_marked_roots(lines: &[String]) {
    for line in lines {
        let path = Path::new(line);
        let owned = path
            .components()
            .any(|part| part.as_os_str().to_string_lossy().starts_with("gcu-ev-"));
        if !owned {
            continue;
        }
        if let Some(root) = path.parent() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

#[test]
fn builtin_fake_bin_wrapper_without_attestation_is_not_started() {
    let dir = scratch("fake-slot");
    let marker = dir.join("started");
    let wrapper = dir.join("eval-fake-agent");
    touch_script(&wrapper, &marker);
    let cfg = RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(wrapper),
        builtin_gcu: false,
        platform: host_platform().into(),
        include_desktop: false,
        timeout: Some(Duration::from_secs(3)),
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&[named("native-form-01")], &cfg)).unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
    assert!(
        row.reason.contains("attest-disposable-desktop"),
        "{}",
        row.reason
    );
    assert!(
        !marker.exists(),
        "builtin_fake_bin wrapper was started without attestation"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn builtin_gcu_wrapper_without_attestation_is_not_started() {
    let dir = scratch("gcu-slot");
    let marker = dir.join("gcu-started");
    let wrapper = dir.join("eval-fake-gcu");
    touch_script(&wrapper, &marker);
    let cfg = RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(agent()),
        builtin_gcu: true,
        gcu_bin: Some(wrapper),
        platform: host_platform().into(),
        behavior: Some("scripted".into()),
        include_desktop: false,
        timeout: Some(Duration::from_secs(3)),
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&[named("native-form-01")], &cfg)).unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
    assert!(
        row.reason.contains("external gcu") || row.reason.contains("attest-disposable-desktop"),
        "{}",
        row.reason
    );
    assert!(
        !marker.exists(),
        "builtin_gcu wrapper was started without attestation"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn blocked_external_client_reports_unknown_not_zero() {
    let dir = scratch("silent");
    let marker = dir.join("client-started");
    let script = silent_success_client(&dir, &marker);
    let cfg = RunConfig {
        grok_bin: Some(script),
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        include_desktop: false,
        attest_disposable_desktop: true,
        timeout: Some(Duration::from_secs(5)),
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&[named("native-form-01")], &cfg)).unwrap();
    let row = &report.tasks[0];
    assert!(
        !marker.exists(),
        "client/preflight ran without provider evidence"
    );
    assert_eq!(
        row.assurance.tool_telemetry,
        TelemetryState::Unknown,
        "tool log without a producer was measured: {} {:?}",
        row.reason,
        row.metrics.tool_calls
    );
    assert_eq!(
        row.assurance.side_telemetry,
        TelemetryState::Unknown,
        "side log without a producer was measured: {}",
        row.reason
    );
    assert_ne!(
        row.status,
        RunStatus::Pass,
        "missing logs were scored as a measured pass: {}",
        row.reason
    );
    assert!(
        row.reason.contains("provider_unavailable"),
        "{}",
        row.reason
    );
    let out = dir.join("out");
    write_reports(&out, &report).unwrap();
    let task_json = std::fs::read_to_string(out.join("native-form-01.json")).unwrap();
    assert!(
        task_json.contains("\"tool_telemetry\": \"unknown\""),
        "{task_json}"
    );
    assert!(
        task_json.contains("\"side_telemetry\": \"unknown\""),
        "{task_json}"
    );
    assert!(task_json.contains("\"tool_calls\": null"), "{task_json}");
    assert!(
        !task_json.contains("\"tool_calls\": 0"),
        "report stored a measured zero: {task_json}"
    );
    assert!(task_json.contains("\"unknown_calls\": null"), "{task_json}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(debug_assertions)]
#[test]
fn ab_cleanup_failure_on_desktop_oneshot_stops_later_arms() {
    let dir = scratch("ab-halt");
    let _cleanup = AbCleanup { dir: dir.clone() };
    gcu_eval::testing_fail_next_removal();
    let mut task = named("text-note-01");
    task.id = "desktop-oneshot".into();
    let cfg = RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(agent()),
        builtin_gcu: true,
        gcu_bin: Some(gcu()),
        probe: Some(dir.join("probe.json")),
        behavior: Some("scripted".into()),
        platform: host_platform().into(),
        include_desktop: true,
        timeout: Some(Duration::from_secs(5)),
        ..RunConfig::default()
    };
    let id = ReportId {
        platform: host_platform().into(),
        model: "fake".into(),
        date: "1970-01-01".into(),
        effort: "same".into(),
        machine: "unspecified".into(),
    };
    let report = block_on(run_ab(std::slice::from_ref(&task), &cfg, &id)).unwrap();
    assert_eq!(report.provenance, gcu_eval::ab::Provenance::Runner);
    let probe: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("probe.json")).unwrap()).unwrap();
    let leftover = Path::new(probe["grok_home"].as_str().unwrap())
        .parent()
        .unwrap();
    assert!(leftover.exists(), "failed cleanup did not retain the tree");
    std::fs::remove_dir_all(leftover).unwrap();
    assert!(
        report.trials.iter().any(|trial| {
            trial.arm == ArmId::A && trial.repeat == 0 && trial.reason.contains("cleanup_failed")
        }),
        "the cleanup failure was discarded: {:?}",
        report.trials
    );
    let later: Vec<_> = report
        .trials
        .iter()
        .filter(|trial| !(trial.arm == ArmId::A && trial.repeat == 0))
        .collect();
    assert_eq!(later.len(), 5, "{:?}", report.trials);
    for trial in later {
        assert_eq!(
            trial.status,
            RunStatus::NotRun,
            "arm {:?} repeat {} ran: {}",
            trial.arm,
            trial.repeat,
            trial.reason
        );
        assert!(trial.reason.contains("remaining tasks"), "{}", trial.reason);
    }
}

struct AbCleanup {
    dir: PathBuf,
}

impl Drop for AbCleanup {
    fn drop(&mut self) {
        let homes: Vec<String> = std::fs::read(self.dir.join("probe.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|probe| probe["grok_home"].as_str().map(str::to_owned))
            .into_iter()
            .collect();
        remove_marked_roots(&homes);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
