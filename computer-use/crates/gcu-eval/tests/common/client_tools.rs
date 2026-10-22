//! Shared real-mode preflight harness. Each integration-test binary is its own
//! process, so each one gets a fresh `OnceLock`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::{
    block_on, bundled_tasks_dir, host_platform, load_tasks, run_tasks, RunConfig, TaskOutcome,
    CLIENT_TOOL_PROBE_PROMPT,
};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Drive {
    pub rows: Vec<TaskOutcome>,
    pub marker: PathBuf,
    pub log: PathBuf,
    pub dir: PathBuf,
}

pub fn drive(label: &str, listing: &str) -> Drive {
    let dir = std::env::temp_dir().join(format!("gcu-ev-tools-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("dispatched");
    let log = dir.join("invocations.log");
    let response = dir.join("preflight.json");
    let mut body = serde_json::to_vec(&serde_json::json!({ "text": listing })).unwrap();
    body.push(b'\n');
    std::fs::write(&response, body).unwrap();
    let seed = dir.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    std::fs::write(seed.join("auth.json"), b"opaque").unwrap();
    let script = write_grok(&dir, &log, &marker, &response);
    let task = load_tasks(&bundled_tasks_dir().unwrap())
        .unwrap()
        .into_iter()
        .find(|task| task.id == "native-form-01")
        .unwrap();
    let cfg = RunConfig {
        grok_bin: Some(script),
        gcu_bin: Some(bin("eval-fake-gcu")),
        platform: host_platform().into(),
        include_desktop: false,
        real_driver: true,
        attest_disposable_desktop: true,
        fixture_bin: Some(write_fixture(&dir)),
        auth_seed: Some(seed),
        timeout: Some(Duration::from_secs(15)),
        auto_approve: true,
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&[task.clone(), task], &cfg)).unwrap();
    Drive {
        rows: report.tasks,
        marker,
        log,
        dir,
    }
}

pub fn log_text(drive: &Drive) -> String {
    std::fs::read_to_string(&drive.log).unwrap_or_default()
}

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

fn write_grok(dir: &Path, log: &Path, marker: &Path, response: &Path) -> PathBuf {
    let script = dir.join("grok.sh");
    let body = r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> @@LOG@@
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
    .replace("@@PROMPT@@", &format!("\"{CLIENT_TOOL_PROBE_PROMPT}\""))
    .replace("@@RESPONSE@@", &shell_quote(response))
    .replace("@@MARKER@@", &shell_quote(marker));
    std::fs::write(&script, body).unwrap();
    executable(&script);
    script
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display())
}

fn executable(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
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
