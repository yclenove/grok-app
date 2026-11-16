//! Operator opt-in for a self-prepared one-shot machine.
//!
//! The flag is command-line only. It is not isolation evidence.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[cfg(unix)]
use gcu_eval::write_reports;
use gcu_eval::{
    block_on, bundled_tasks_dir, host_platform, load_tasks, run_tasks, RunConfig, RunStatus, Task,
    TaskOutcome, UNVERIFIED_BANNER,
};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
#[cfg(unix)]
use std::time::Duration;

fn tasks() -> Vec<Task> {
    load_tasks(&bundled_tasks_dir().expect("bundled tasks")).expect("tasks")
}

fn named(id: &str) -> Task {
    tasks().into_iter().find(|task| task.id == id).expect(id)
}

#[cfg(unix)]
fn scratch(label: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("gcu-ev-unverified-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn eval_bin() -> &'static str {
    env!("CARGO_BIN_EXE_gcu-eval")
}

fn flagged(real_driver: bool) -> RunConfig {
    RunConfig {
        unverified_environment: true,
        run_local: true,
        real_driver,
        platform: host_platform().into(),
        include_desktop: false,
        ..RunConfig::default()
    }
}

fn run(task: &Task, cfg: &RunConfig) -> TaskOutcome {
    let report = block_on(run_tasks(std::slice::from_ref(task), cfg)).unwrap();
    assert_eq!(report.tasks.len(), 1);
    report.tasks.into_iter().next().unwrap()
}

fn assert_unverified(row: &TaskOutcome) {
    assert!(
        row.assurance
            .environment
            .as_ref()
            .is_some_and(|record| record.is_unverified()),
        "{}",
        row.reason
    );
}

#[test]
fn unverified_environment_still_refuses_missing_binaries_and_desktop() {
    let task = named("native-form-01");
    let file = std::env::current_exe().unwrap();
    let missing = PathBuf::from("gcu-eval-missing-binary");

    let no_client = run(&task, &flagged(true));
    assert_eq!(no_client.status, RunStatus::NotRun);
    assert!(!no_client.assurance.client_started);
    assert!(no_client
        .reason
        .contains("no client binary and no model credentials"));
    assert!(!no_client.reason.contains("provider_unavailable"));
    assert_unverified(&no_client);

    let absent_client = RunConfig {
        grok_bin: Some(missing.clone()),
        ..flagged(true)
    };
    let absent_client = run(&task, &absent_client);
    assert_eq!(absent_client.status, RunStatus::NotRun);
    assert!(absent_client.reason.contains("client binary missing"));
    assert!(!absent_client.reason.contains("provider_unavailable"));
    assert!(!absent_client.assurance.client_started);
    assert_unverified(&absent_client);

    let no_gcu = RunConfig {
        grok_bin: Some(file.clone()),
        gcu_bin: None,
        ..flagged(true)
    };
    let no_gcu = run(&task, &no_gcu);
    assert_eq!(no_gcu.status, RunStatus::NotRun);
    assert!(no_gcu.reason.contains("real driver needs a gcu binary"));
    assert!(!no_gcu.assurance.client_started);
    assert_unverified(&no_gcu);

    let no_fixture = RunConfig {
        grok_bin: Some(file.clone()),
        gcu_bin: Some(file),
        fixture_bin: Some(missing),
        ..flagged(true)
    };
    let no_fixture = run(&task, &no_fixture);
    assert_eq!(no_fixture.status, RunStatus::NotRun);
    assert!(no_fixture
        .reason
        .contains("real driver needs a fixture binary"));
    assert!(!no_fixture.assurance.client_started);
    assert_unverified(&no_fixture);

    let desktop = RunConfig {
        run_local: false,
        ..flagged(true)
    };
    let desktop = run(&task, &desktop);
    assert_eq!(desktop.status, RunStatus::NotRun);
    assert!(desktop.reason.contains("provider_unavailable"));
    assert!(!desktop.assurance.client_started);
    assert!(desktop
        .assurance
        .environment
        .as_ref()
        .is_some_and(|record| !record.is_unverified()));
}

#[test]
fn unverified_environment_keeps_red_team_not_run() {
    let task = named("redteam-side-01");
    let cfg = RunConfig {
        grok_bin: Some(std::env::current_exe().unwrap()),
        ..flagged(true)
    };
    let row = run(&task, &cfg);
    assert_eq!(row.status, RunStatus::NotRun);
    assert!(row.reason.contains("real red-team task is not_run"));
    assert!(!row.assurance.client_started);
    assert!(row.env_root.is_none());
    assert_unverified(&row);
}

#[test]
fn environment_variable_cannot_enable_unverified_environment() {
    let args = [
        "--driver",
        "real",
        "--env",
        "local",
        "--suite",
        "core",
        "--only",
        "native-form-01",
    ];
    for key in [
        "UNVERIFIED_ENVIRONMENT",
        "GCU_EVAL_UNVERIFIED_ENVIRONMENT",
        "GCU_UNVERIFIED_ENVIRONMENT",
    ] {
        let output = Command::new(eval_bin())
            .args(args)
            .env(key, "1")
            .env_remove("GCU_APPROVAL")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(0), "{stdout}");
        assert!(
            stdout.contains("provider_unavailable"),
            "{key} enabled the flag: {stdout}"
        );
        assert!(
            !stdout.contains(UNVERIFIED_BANNER),
            "{key} printed the unverified banner"
        );
    }

    let flagged = Command::new(eval_bin())
        .args(args)
        .arg("--unverified-environment")
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&flagged.stdout);
    assert_eq!(flagged.status.code(), Some(0), "{stdout}");
    let banner_at = stdout
        .find(UNVERIFIED_BANNER)
        .expect("stdout is missing the unverified banner");
    let row_at = stdout.find("native-form-01").expect("missing task row");
    assert!(banner_at < row_at);
    assert!(stdout.contains("no client binary and no model credentials"));
    assert!(!stdout.contains("provider_unavailable"));

    let help = Command::new(eval_bin())
        .arg("--help")
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(help.status.success(), "{text}");
    assert!(text.contains("--unverified-environment"));
    assert!(text.contains("one-shot"));
    assert!(text.contains("isolation proof"));
    assert!(text.contains("unverified"));
    assert!(!text.contains("UNVERIFIED_ENVIRONMENT"));
    assert!(!text.contains("[env:"));
}

#[test]
fn fake_driver_status_is_unchanged_by_the_unverified_flag() {
    let task = named("native-form-01");
    let plain = RunConfig {
        builtin_fake: true,
        builtin_gcu: true,
        platform: host_platform().into(),
        behavior: Some("success".into()),
        include_desktop: false,
        ..RunConfig::default()
    };
    let mut flagged = plain.clone();
    flagged.unverified_environment = true;
    let plain_row = run(&task, &plain);
    let flagged_row = run(&task, &flagged);
    assert_eq!(plain_row.status, RunStatus::Pass, "{}", plain_row.reason);
    assert_eq!(flagged_row.status, plain_row.status);
    assert_eq!(flagged_row.reason, plain_row.reason);
    assert!(plain_row.assurance.client_started);
    assert!(flagged_row.assurance.client_started);
    assert!(plain_row.assurance.environment.is_none());
    assert_unverified(&flagged_row);
}

#[cfg(unix)]
#[test]
fn unverified_environment_starts_real_driver_with_client_and_fixture_stubs() {
    let dir = scratch("launch");
    let log = dir.join("invocations.log");
    let marker = dir.join("dispatched");
    let response = dir.join("preflight.json");
    let listing = r#"{"tools":["list_apps","list_windows","search_tool","todo_write","use_tool"]}"#;
    std::fs::write(&response, listing).unwrap();
    let seed = dir.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    std::fs::write(seed.join("auth.json"), b"opaque").unwrap();
    std::fs::write(seed.join("extra.txt"), b"not-copied").unwrap();
    let script = write_grok(&dir, &log, &marker, &response);
    let task = named("native-form-01");
    let cfg = RunConfig {
        unverified_environment: true,
        run_local: true,
        real_driver: true,
        grok_bin: Some(script),
        gcu_bin: Some(PathBuf::from(env!("CARGO_BIN_EXE_eval-fake-gcu"))),
        fixture_bin: Some(write_fixture(&dir)),
        auth_seed: Some(seed),
        platform: host_platform().into(),
        include_desktop: true,
        timeout: Some(Duration::from_secs(20)),
        auto_approve: true,
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(std::slice::from_ref(&task), &cfg)).unwrap();
    assert_eq!(report.tasks.len(), 2);
    let row = &report.tasks[0];
    assert!(
        row.assurance.client_started,
        "client did not start: {}",
        row.reason
    );
    assert!(
        !row.reason.contains("provider_unavailable"),
        "{}",
        row.reason
    );
    assert!(marker.exists(), "task prompt was not dispatched");
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!logged.is_empty(), "client stub was not started");
    assert!(
        !logged.contains("copied-extra"),
        "auth seed copied a file other than auth.json"
    );
    for row in &report.tasks {
        assert_unverified(row);
    }
    assert_eq!(report.tasks[1].id, "desktop-oneshot");
    assert_eq!(report.tasks[1].status, RunStatus::NotRun);

    let out = dir.join("out");
    write_reports(&out, &report).unwrap();
    let html = std::fs::read_to_string(out.join("summary.html")).unwrap();
    let banner_at = html
        .find(UNVERIFIED_BANNER)
        .expect("html is missing the banner");
    let title_at = html.find("<h1>").expect("html is missing the title");
    assert!(banner_at < title_at);
    assert!(html.contains(&format!("<p><strong>{UNVERIFIED_BANNER}</strong></p>")));
    let raw = std::fs::read_to_string(out.join("summary.json")).unwrap();
    let quoted = serde_json::to_string(UNVERIFIED_BANNER).unwrap();
    let prefix = format!("{{\n  \"notice\": {quoted},\n");
    assert!(
        raw.starts_with(&prefix),
        "summary does not open with the unverified notice: {raw}"
    );
    let summary: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(summary["notice"], UNVERIFIED_BANNER);
    for row in summary["tasks"].as_array().unwrap() {
        assert_eq!(row["environment_contract"], "unverified");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn unverified_environment_keeps_windows_real_driver_not_run() {
    let file = std::env::current_exe().unwrap();
    let cfg = RunConfig {
        grok_bin: Some(file.clone()),
        gcu_bin: Some(file.clone()),
        fixture_bin: Some(file),
        ..flagged(true)
    };
    let row = run(&named("native-form-01"), &cfg);
    assert_eq!(row.status, RunStatus::NotRun);
    assert!(row.reason.contains(gcu_eval::WINDOWS_REAL_NOT_RUN));
    assert!(!row.assurance.client_started);
    assert_unverified(&row);
}

#[cfg(unix)]
fn write_fixture(dir: &Path) -> PathBuf {
    let script = dir.join("fixture.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '{\"counter\":0}' > \"$GCU_FIXTURE_STATE\"\nsleep 300\n",
    )
    .unwrap();
    executable(&script);
    script
}

#[cfg(unix)]
fn write_grok(dir: &Path, log: &Path, marker: &Path, response: &Path) -> PathBuf {
    let script = dir.join("grok.sh");
    let prompt = gcu_eval::CLIENT_TOOL_PROBE_PROMPT;
    let body = r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> @@LOG@@
if [ -e "$GROK_HOME/extra.txt" ]; then
  printf '%s\n' 'copied-extra' >> @@LOG@@
fi
if [ ! -s "$GROK_HOME/auth.json" ] || [ ! -s "$GROK_HOME/config.toml" ]; then
  printf '%s\n' '{"text":""}'
  exit 0
fi
prompt=
prev=
for arg in "$@"; do
  if [ "$prev" = "-p" ]; then
    prompt=$arg
  fi
  prev=$arg
done
case "$prompt" in
  @@PROMPT@@)
    cat @@RESPONSE@@
    exit 0
    ;;
esac
: > @@MARKER@@
printf '%s\n' '{"num_turns":1}'
exit 0
"#
    .replace("@@LOG@@", &shell_quote(log))
    .replace("@@PROMPT@@", &format!("\"{prompt}\""))
    .replace("@@RESPONSE@@", &shell_quote(response))
    .replace("@@MARKER@@", &shell_quote(marker));
    std::fs::write(&script, body).unwrap();
    executable(&script);
    script
}

#[cfg(unix)]
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display())
}

#[cfg(unix)]
fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
