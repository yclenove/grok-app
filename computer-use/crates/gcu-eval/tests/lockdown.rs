//! Fail-closed checks for a real client, temp credentials, and preflight output.
//! These tests use the fake agent or a local script. They do not call a model.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::{
    block_on, bundled_tasks_dir, host_platform, load_tasks, run_tasks, write_reports, RunConfig,
    RunStatus, Task,
};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "gcu-ev-lock-{label}-{}-{}",
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

fn eval_bin() -> PathBuf {
    bin("gcu-eval")
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

fn fixture_script(dir: &Path) -> PathBuf {
    let script = dir.join("fixture.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '{\"counter\":0}' > \"$GCU_FIXTURE_STATE\"\nsleep 300\n",
    )
    .unwrap();
    executable(&script);
    script
}

fn cfg(_dir: &Path, behavior: &str) -> RunConfig {
    RunConfig {
        builtin_fake: true,
        builtin_gcu: true,
        gcu_bin: None,
        platform: host_platform().into(),
        behavior: Some(behavior.into()),
        include_desktop: false,
        timeout: Some(Duration::from_secs(3)),
        ..RunConfig::default()
    }
}

#[test]
fn cli_real_driver_without_disposable_desktop_attestation_does_not_start() {
    let dir = scratch("cli-attest");
    let client_marker = dir.join("client-started");
    let fixture_marker = dir.join("fixture-started");
    let client = dir.join("grok.sh");
    std::fs::write(
        &client,
        format!("#!/bin/sh\ntouch '{}'\nexit 0\n", client_marker.display()),
    )
    .unwrap();
    executable(&client);
    let fixture = dir.join("fixture.sh");
    std::fs::write(
        &fixture,
        format!(
            "#!/bin/sh\ntouch '{}'\nprintf '{{}}' > \"$GCU_FIXTURE_STATE\"\nsleep 300\n",
            fixture_marker.display()
        ),
    )
    .unwrap();
    executable(&fixture);
    let output = Command::new(eval_bin())
        .args([
            "--suite",
            "core",
            "--driver",
            "real",
            "--env",
            "local",
            "--model",
            "fake",
            "--only",
            "native-form-01",
            "--timeout-ms",
            "4000",
        ])
        .arg("--grok")
        .arg(&client)
        .arg("--gcu")
        .arg(gcu())
        .arg("--fixture")
        .arg(&fixture)
        .env("GCU_EVAL_WATCH_FILE", dir.join("parent-watch"))
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "real mode must record provider refusal\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains("provider_unavailable"), "{stdout}");
    assert!(
        !stdout.contains("pass native-form-01"),
        "the task ran: {stdout}"
    );
    assert!(!client_marker.exists(), "the client was started");
    assert!(!fixture_marker.exists(), "the fixture was started");
    assert!(
        !dir.join("parent-watch").exists(),
        "the parent watch variable was treated as a launch"
    );
    let flagged = Command::new(eval_bin())
        .args([
            "--suite",
            "core",
            "--driver",
            "real",
            "--env",
            "local",
            "--model",
            "fake",
            "--only",
            "native-form-01",
            "--attest-disposable-desktop",
        ])
        .arg("--grok")
        .arg(agent())
        .arg("--gcu")
        .arg(gcu())
        .arg("--fixture")
        .arg(&fixture)
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let flagged_out = String::from_utf8_lossy(&flagged.stdout);
    assert_eq!(flagged.status.code(), Some(0), "{flagged_out}");
    assert!(
        flagged_out.contains("provider_unavailable"),
        "{flagged_out}"
    );
    assert!(!client_marker.exists() && !fixture_marker.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn external_client_on_the_fake_driver_is_refused_without_attestation() {
    let dir = scratch("ext-client");
    let marker = dir.join("started");
    let script = dir.join("grok.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ntouch '{}'\nprintf '%s\\n' '{{\"num_turns\":1}}'\nexit 0\n",
            marker.display()
        ),
    )
    .unwrap();
    executable(&script);
    let cfg = RunConfig {
        grok_bin: Some(script),
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        include_desktop: false,
        timeout: Some(Duration::from_secs(3)),
        real_driver: false,
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
    assert!(!marker.exists(), "the external client was started");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_red_team_without_a_sensitive_deny_policy_is_not_run() {
    let dir = scratch("red-real");
    let cfg = RunConfig {
        real_driver: true,
        attest_disposable_desktop: true,
        fixture_bin: Some(fixture_script(&dir)),
        auto_approve: true,
        ..cfg(&dir, "scripted")
    };
    let report = block_on(run_tasks(&[named("redteam-side-01")], &cfg)).unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
    assert!(
        !row.approval_mode.contains("auto-allow"),
        "red team must not fall back to auto-allow: {}",
        row.approval_mode
    );
    assert!(
        row.reason.contains("provider_unavailable"),
        "{}",
        row.reason
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn synthetic_output_finishes_instead_of_timing_out() {
    let dir = scratch("flood");
    let mut cfg = cfg(&dir, "pipe_flood");
    cfg.timeout = Some(Duration::from_secs(3));
    let started = std::time::Instant::now();
    let report = block_on(run_tasks(&[named("native-form-01")], &cfg)).unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::Pass, "{}", row.reason);
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "pipe flood blocked the client"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn in_process_timeout_leaves_no_child_or_marker() {
    let dir = scratch("timeout");
    let marker = dir.join("must-not-create-child");
    let mut cfg = cfg(&dir, "timeout");
    cfg.timeout = Some(Duration::from_millis(20));
    cfg.watch_files = vec![marker.clone()];
    let report = block_on(run_tasks(&[named("text-note-01")], &cfg)).unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::Timeout, "{}", row.reason);
    assert!(row.assurance.client_started && row.env_destroyed);
    assert!(!marker.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn preflight_sensitive_token_is_absent_from_the_report() {
    let dir = scratch("sensitive");
    let response = dir.join("preflight.json");
    std::fs::write(&response, b"{\"tools\":[\"shell_SENSITIVEVALUE\"]}\n").unwrap();
    let log = dir.join("argv.log");
    let script = dir.join("grok.sh");
    let body = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
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
    cat '{}'
    exit 0
    ;;
esac
printf '%s\n' '{{"num_turns":1}}'
exit 0
"#,
        log.display(),
        response.display()
    );
    std::fs::write(&script, body).unwrap();
    executable(&script);
    let seed = dir.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    std::fs::write(seed.join("auth.json"), b"opaque").unwrap();
    let cfg = RunConfig {
        grok_bin: Some(script),
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        include_desktop: false,
        real_driver: true,
        attest_disposable_desktop: true,
        fixture_bin: Some(fixture_script(&dir)),
        auth_seed: Some(seed),
        auto_approve: true,
        model: "fake".into(),
        timeout: Some(Duration::from_secs(5)),
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&[named("native-form-01")], &cfg)).unwrap();
    let out = dir.join("out");
    write_reports(&out, &report).unwrap();
    let blob = std::fs::read_dir(&out)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<String>();
    let row = &report.tasks[0];
    let combined = format!("{} {}", row.reason, blob);
    assert!(
        !combined.contains("SENSITIVEVALUE"),
        "report kept model text: {combined}"
    );
    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
    assert!(
        row.reason.contains("provider_unavailable"),
        "{}",
        row.reason
    );
    assert!(
        !log.exists(),
        "even tool preflight must wait for the environment gate"
    );
    assert!(blob.contains("attestation_provided"), "{blob}");
    assert!(blob.contains("\"driver_mode\": \"real\""), "{blob}");
    assert!(blob.contains("\"tool_telemetry\": \"unknown\""), "{blob}");
    assert!(blob.contains("\"tool_calls\": null"), "{blob}");
    assert!(
        blob.contains("\"os_sandbox\": \"not_implemented\""),
        "{blob}"
    );
    assert!(
        blob.contains("\"lifetime_barrier\": \"not_implemented\""),
        "{blob}"
    );
    assert!(
        blob.contains("\"client_policy_pin\": \"not_implemented\""),
        "{blob}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn legacy_attestation_does_not_start_external_client_or_preflight() {
    let dir = scratch("ext-ok");
    let log = dir.join("argv.log");
    let script = logging_client(&dir, &log);
    let cfg = RunConfig {
        grok_bin: Some(script),
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        include_desktop: false,
        timeout: Some(Duration::from_secs(5)),
        real_driver: false,
        attest_disposable_desktop: true,
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&[named("native-form-01")], &cfg)).unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
    assert!(row.reason.contains("provider_unavailable"));
    assert!(!log.exists(), "credentialed preflight was started");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn environment_gate_precedes_preflight_even_with_oversized_response() {
    let dir = scratch("overflow");
    let response = dir.join("preflight.json");
    let mut body = exact_preflight_json(65_536);
    assert!(serde_json::from_slice::<serde_json::Value>(&body).is_ok());
    body.push(b'x');
    std::fs::write(&response, &body).unwrap();
    let log = dir.join("argv.log");
    let script = cat_preflight(&dir, &log, &response);
    let report = block_on(run_tasks(
        &[named("native-form-01")],
        &real_cfg(&dir, script),
    ))
    .unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
    assert!(row.reason.contains("provider_unavailable"));
    assert!(!log.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn environment_gate_is_required_for_every_model_and_profile() {
    let dir = scratch("cache-key");
    let log = dir.join("argv.log");
    let script = cache_client(&dir, &log);
    let base = real_cfg(&dir, script.clone());
    let first = block_on(run_tasks(&[named("native-form-01")], &base)).unwrap();
    assert_eq!(
        first.tasks[0].status,
        RunStatus::NotRun,
        "{}",
        first.tasks[0].reason
    );
    let mut other = base.clone();
    other.model = "other".into();
    let second = block_on(run_tasks(&[named("native-form-01")], &other)).unwrap();
    assert_eq!(
        second.tasks[0].status,
        RunStatus::NotRun,
        "{}",
        second.tasks[0].reason
    );
    assert!(second.tasks[0].reason.contains("provider_unavailable"));
    assert!(!log.exists());
    let mut full = base.clone();
    full.mcp_profile = Some("full".into());
    let third = block_on(run_tasks(&[named("native-form-01")], &full)).unwrap();
    assert_eq!(
        third.tasks[0].status,
        RunStatus::NotRun,
        "{}",
        third.tasks[0].reason
    );
    assert!(!format!(
        "{} {}",
        third.tasks[0].reason,
        std::fs::read_to_string(&log).unwrap_or_default()
    )
    .contains("SENSITIVEVALUE"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn environment_gate_cannot_be_bypassed_with_a_clone_or_fresh_config() {
    let dir = scratch("cache-run");
    let phase = dir.join("phase");
    let log = dir.join("argv.log");
    let script = dir.join("grok.sh");
    let good = exact_tools_json();
    let body = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
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
    if [ -e '{}' ]; then
      printf '%s\n' '{{"tools":["shell_SENSITIVEVALUE"]}}'
      exit 0
    fi
    : > '{}'
    printf '%s\n' '{}'
    exit 0
    ;;
esac
printf '%s\n' '{{"num_turns":1}}'
exit 0
"#,
        log.display(),
        phase.display(),
        phase.display(),
        good
    );
    std::fs::write(&script, body).unwrap();
    executable(&script);
    let first_cfg = real_cfg(&dir, script.clone());
    let first = block_on(run_tasks(&[named("native-form-01")], &first_cfg)).unwrap();
    assert_eq!(
        first.tasks[0].status,
        RunStatus::NotRun,
        "{}",
        first.tasks[0].reason
    );
    let cloned = block_on(run_tasks(&[named("native-form-01")], &first_cfg.clone())).unwrap();
    assert_eq!(
        cloned.tasks[0].status,
        RunStatus::NotRun,
        "a clone cannot bypass the environment gate: {}",
        cloned.tasks[0].reason
    );
    let fresh = real_cfg(&dir, script);
    let second = block_on(run_tasks(&[named("native-form-01")], &fresh)).unwrap();
    assert_eq!(second.tasks[0].status, RunStatus::NotRun);
    assert!(second.tasks[0].reason.contains("provider_unavailable"));
    assert!(!log.exists());
    assert!(!second.tasks[0].reason.contains("SENSITIVEVALUE"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn exact_tools_json() -> String {
    r#"{"tools":["list_apps","list_windows","search_tool","todo_write","use_tool"]}"#.into()
}

fn exact_preflight_json(len: usize) -> Vec<u8> {
    let mut body = exact_tools_json().into_bytes();
    assert!(body.len() <= len);
    body.resize(len, b' ');
    body
}

fn real_cfg(dir: &Path, script: PathBuf) -> RunConfig {
    let seed = dir.join("seed");
    let _ = std::fs::create_dir_all(&seed);
    let _ = std::fs::write(seed.join("auth.json"), b"opaque");
    RunConfig {
        grok_bin: Some(script),
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        include_desktop: false,
        real_driver: true,
        attest_disposable_desktop: true,
        fixture_bin: Some(fixture_script(dir)),
        auth_seed: Some(seed),
        auto_approve: true,
        model: "fake".into(),
        timeout: Some(Duration::from_secs(5)),
        ..RunConfig::default()
    }
}

fn logging_client(dir: &Path, log: &Path) -> PathBuf {
    let script = dir.join("grok.sh");
    let listing = exact_tools_json();
    let body = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
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
    printf '%s\n' '{}'
    exit 0
    ;;
esac
printf '%s\n' '{{"num_turns":1}}'
exit 0
"#,
        log.display(),
        listing
    );
    std::fs::write(&script, body).unwrap();
    executable(&script);
    script
}

fn cat_preflight(dir: &Path, log: &Path, response: &Path) -> PathBuf {
    let script = dir.join("grok.sh");
    let body = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
cat '{}'
exit 0
"#,
        log.display(),
        response.display()
    );
    std::fs::write(&script, body).unwrap();
    executable(&script);
    script
}

fn cache_client(dir: &Path, log: &Path) -> PathBuf {
    let script = dir.join("grok.sh");
    let listing = exact_tools_json();
    let body = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
model=
prev=
for arg in "$@"; do
  if [ "$prev" = "--model" ]; then
    model=$arg
  fi
  prev=$arg
done
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
    if [ "$model" = "other" ]; then
      printf '%s\n' '{{"tools":["shell_SENSITIVEVALUE"]}}'
      exit 0
    fi
    if grep -q '"full"' "$GROK_HOME/config.toml"; then
      printf '%s\n' '{{"tools":["shell_SENSITIVEVALUE"]}}'
      exit 0
    fi
    printf '%s\n' '{}'
    exit 0
    ;;
esac
printf '%s\n' '{{"num_turns":1}}'
exit 0
"#,
        log.display(),
        listing
    );
    std::fs::write(&script, body).unwrap();
    executable(&script);
    script
}
