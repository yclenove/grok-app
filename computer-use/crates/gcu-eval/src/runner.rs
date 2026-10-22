//! Task runner and one-shot environment providers.
//!
//! [`LocalTempProvider`] creates a temporary directory and a fresh `GROK_HOME`
//! for tests and the fake driver. It is not a security boundary. [`DesktopProvider`]
//! does not launch anything; real runs need a disposable OS or desktop container
//! with no user mount and no credentials.

pub use crate::access::{
    block_on, bundled_tasks_dir, child_env_keys, host_platform, parse_approval, path_under_configs,
    user_config_dirs, ApprovalMode,
};
use crate::metrics::summarize;
use crate::oracle::{prove_oracle, World};
use crate::tasks::Task;
use crate::Error;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Why the one-shot desktop provider did not run.
pub const DESKTOP_NOT_RUN: &str = "one-shot OS/desktop container or VM is not available; isolated GROK_HOME is not an environment boundary";

pub(crate) use crate::client_tools::DISABLED;

/// How a task finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Oracle passed and no forbidden side effect was seen.
    Pass,
    /// Oracle failed, or the agent claimed success incorrectly.
    Fail,
    /// Only some positive checks matched.
    Partial,
    /// The client exceeded the task budget and was killed.
    Timeout,
    /// The client exited with an error or a signal.
    Crash,
    /// A file, process, or network effect outside the task was observed.
    SideEffect,
    /// The oracle could not prove pass and fail.
    Invalid,
    /// The client, credential, platform, or desktop container was absent.
    NotRun,
}

impl RunStatus {
    /// Stable report spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Partial => "partial",
            Self::Timeout => "timeout",
            Self::Crash => "crash",
            Self::SideEffect => "side_effect",
            Self::Invalid => "invalid",
            Self::NotRun => "not_run",
        }
    }
}

/// Where a round-trip count came from. `Unset` means the count is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoundsSource {
    /// The transcript did not report turns and the tool log had no calls.
    #[default]
    Unset,
    /// Client transcript field `turns`.
    TranscriptTurns,
    /// Computer-use invocations in the evaluation tool log.
    ToolCalls,
}

/// Counts recorded for one task. Arguments and prompts are not stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskMetrics {
    /// Model turns, or tool calls when the transcript has no turn count.
    pub steps: u64,
    /// Which observation filled `steps`. Missing stays [`RoundsSource::Unset`].
    #[serde(default)]
    pub rounds_source: RoundsSource,
    /// Computer-use and unknown tool calls.
    pub tool_calls: u64,
    /// Later call in a consecutive pair with the same tool name.
    pub redundant_calls: u64,
    /// Results marked unknown, plus tool names this runner does not know.
    pub unknown_calls: u64,
    /// Wall time from spawn through exit or kill.
    pub duration_ms: u64,
    /// Input tokens reported by the client transcript. Zero is a real zero only
    /// when `tokens_in_reported` is set; a missing field stays zero and unreported.
    pub tokens_in: u64,
    /// True when the transcript contained `tokens_in`. Never inferred from bytes.
    #[serde(default)]
    pub tokens_in_reported: bool,
    /// Output tokens reported by the client transcript.
    pub tokens_out: u64,
    /// True when the transcript contained `tokens_out`.
    #[serde(default)]
    pub tokens_out_reported: bool,
    /// UTF-8 length of logged tool results, or an explicit `result_bytes` field.
    /// This is not a token count.
    #[serde(default)]
    pub tool_result_bytes: u64,
    /// Images the client reported.
    pub images: u32,
    /// Image bytes the client reported.
    pub image_bytes: u64,
    /// Extra approval prompts the client reported.
    pub extra_approvals: u64,
    /// The client claimed success and the oracle did not pass.
    pub false_success: bool,
}

/// One task row. The prompt and fixture values are intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOutcome {
    /// Task id, or `desktop-oneshot` for the container gap.
    pub id: String,
    /// Task category. Desktop gap uses `desktop`.
    pub category: String,
    /// How the row finished.
    pub status: RunStatus,
    /// Requested platform.
    pub platform: String,
    /// A/B group label.
    pub group: String,
    /// Repeat index.
    pub repeat: u32,
    /// Model id passed to the client. `fake` does not call a network API.
    pub model: String,
    /// Counts.
    pub metrics: TaskMetrics,
    /// Tool names only. Arguments are dropped.
    pub tool_names: Vec<String>,
    /// `gcu` subcommands the client actually invoked, including `mcp`.
    pub gcu_invocations: Vec<String>,
    /// Violation labels. File contents are not included.
    pub violations: Vec<String>,
    /// Short reason for `not_run` or `invalid`. Empty on a normal finish.
    pub reason: String,
    /// Environment directory, retained after deletion so tests can check it.
    pub env_root: Option<PathBuf>,
    /// True when nothing from the environment remains on disk.
    pub env_destroyed: bool,
    /// The client argv included `--always-approve`.
    pub always_approve: bool,
    /// The client argv included `--disable-web-search`.
    pub web_search_disabled: bool,
    /// Client built-ins that count as a violation when a tool log records them.
    ///
    /// The real client does not read this list. Real mode restricts built-ins
    /// with `--tools todo_write` and a process-wide preflight.
    pub disabled_tools: Vec<String>,
    /// CU approval mode passed to the child, if any.
    pub approval_mode: String,
    /// `HOME` given to the child. Empty when the client was not started.
    pub home: String,
    /// `GROK_HOME` given to the child.
    pub grok_home: String,
}

impl TaskOutcome {
    /// False for skipped rows, which stay out of rates and percentiles.
    #[must_use]
    pub const fn scored(&self) -> bool {
        !matches!(self.status, RunStatus::NotRun)
    }
}

/// Suite result after every task and the optional desktop gap row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuiteReport {
    /// Model id.
    pub model: String,
    /// Platform the suite was asked to run.
    pub platform: String,
    /// A/B group.
    pub group: String,
    /// Task rows.
    pub tasks: Vec<TaskOutcome>,
    /// Aggregate metrics.
    pub summary: crate::metrics::Summary,
}

/// Knobs for one suite. Paths are binaries and report directories, not user homes.
#[derive(Debug, Clone)]
pub struct RunConfig {
    /// Client binary. `None` skips every task as `not_run`.
    pub grok_bin: Option<PathBuf>,
    /// `gcu` binary written into the isolated MCP config.
    pub gcu_bin: Option<PathBuf>,
    /// Model id. This runner never calls a model API itself.
    pub model: String,
    /// `linux`, `macos`, or `windows`.
    pub platform: String,
    /// A/B group stored on each row.
    pub group: String,
    /// Repeat index stored on each row.
    pub repeat: u32,
    /// Suite name used only by the caller; the runner executes `tasks` as given.
    pub suite: String,
    /// Replaces each task's `max_seconds`. Tests use a short limit.
    pub timeout: Option<Duration>,
    /// Optional scripted behavior passed to the fake client.
    pub behavior: Option<String>,
    /// Files that must keep their fingerprint. A change is a side effect.
    pub watch_files: Vec<PathBuf>,
    /// Optional file the client may write with its argv and environment keys.
    pub probe: Option<PathBuf>,
    /// Pass CU auto-approval to the child. The CLI enables this only on an eval build.
    pub auto_approve: bool,
    /// Append the desktop `not_run` row.
    pub include_desktop: bool,
    /// When false, tasks are `not_run` instead of using the local temp provider.
    pub run_local: bool,
    /// Existing `gcu mcp --profile` value written into the isolated client config.
    /// `None` omits the flag and leaves the server default.
    pub mcp_profile: Option<String>,
    /// Eval-only arm label stored beside the MCP entry. Not a server switch.
    pub arm_label: Option<String>,
    /// Client screenshot policy recorded for the arm: `true` or `auto`.
    pub arm_screenshot: Option<String>,
    /// Client diff policy recorded for the arm. Not a new server flag.
    pub arm_diff: Option<String>,
    /// Whether the arm requests `computer_batch` through the profile.
    pub arm_batch: Option<bool>,
    /// Whether the arm requests `computer_script` through the full profile.
    pub arm_script: Option<bool>,
    /// Use the real `gcu` driver and a real fixture app instead of the fake driver and scene.
    ///
    /// A real run drives whatever desktop the process runs on. It refuses to start unless
    /// [`RunConfig::attest_disposable_desktop`] is set. `--tools` and the preflight are
    /// mitigations, not a confinement proof. Fixture-bound authorization and an OS sandbox
    /// are not implemented.
    pub real_driver: bool,
    /// Fixture app started once per task when `real_driver` is set. It writes `$GCU_FIXTURE_STATE`.
    pub fixture_bin: Option<PathBuf>,
    /// Directory that holds a login made for this evaluation only. Only its `auth.json` is copied,
    /// unread, into each task's temporary `GROK_HOME` and is deleted with it.
    pub auth_seed: Option<PathBuf>,
    /// Operator attestation that this process is on a one-shot isolated desktop with egress
    /// control. Default false. An environment variable cannot set this. It is not proof.
    pub attest_disposable_desktop: bool,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            grok_bin: None,
            gcu_bin: None,
            model: "fake".into(),
            platform: host_platform().into(),
            group: "A".into(),
            repeat: 0,
            suite: "core".into(),
            timeout: None,
            behavior: None,
            watch_files: Vec::new(),
            probe: None,
            auto_approve: false,
            include_desktop: true,
            run_local: true,
            mcp_profile: None,
            arm_label: None,
            arm_screenshot: None,
            arm_diff: None,
            arm_batch: None,
            arm_script: None,
            real_driver: false,
            fixture_bin: None,
            auth_seed: None,
            attest_disposable_desktop: false,
        }
    }
}

/// One prepared environment. The provider must delete `root` in `destroy`.
#[derive(Debug, Clone)]
pub struct Environment {
    /// Directory that contains every other path.
    pub root: PathBuf,
    /// `HOME` for the child. This is not the operator's home.
    pub home: PathBuf,
    /// Isolated `GROK_HOME`. Not `~/.grok`.
    pub grok_home: PathBuf,
    /// Client working directory.
    pub work: PathBuf,
    /// `$GCU_FIXTURE_STATE`.
    pub state_path: PathBuf,
    /// Directory prepended to `PATH`. It contains only side-effect shims.
    pub shim_dir: PathBuf,
    /// Shim and agent side-effect log.
    pub side_log: PathBuf,
    /// `gcu` invocation log.
    pub tool_log: PathBuf,
    /// Client transcript path inside `grok_home`.
    pub transcript: PathBuf,
}

/// What [`EnvProvider::provision`] decided.
#[derive(Debug)]
pub enum Provision {
    /// A directory tree the caller must destroy.
    ///
    /// Boxed so this variant does not inflate [`Provision`] on every target.
    Ready(Box<Environment>),
    /// The provider refuses to pretend this environment exists.
    NotRun {
        /// Safe-to-print reason.
        reason: String,
    },
}

/// Creates and deletes a one-shot environment.
///
/// `provision` may create directories and must not read user config homes.
/// `destroy` removes everything `provision` created. Neither method waits on
/// the agent. Timeout and cancellation are applied by [`run_tasks`], which
/// kills the client process.
pub trait EnvProvider {
    /// Prepare an environment for `task`.
    fn provision(&self, task: &Task) -> Result<Provision, Error>;
    /// Delete `env`. True when the root is gone.
    fn destroy(&self, env: Environment) -> Result<bool, Error>;
}

/// Temporary directory provider for tests and the fake driver.
#[derive(Debug, Default)]
pub struct LocalTempProvider;

impl EnvProvider for LocalTempProvider {
    fn provision(&self, _task: &Task) -> Result<Provision, Error> {
        let mut held = crate::prep::begin_env()?;
        // The trait caller destroys the tree. Drop must not delete it first.
        held.disarm();
        Ok(Provision::Ready(Box::new(held.env)))
    }

    fn destroy(&self, env: Environment) -> Result<bool, Error> {
        crate::prep::remove_root(&env.root)
    }
}

/// Real desktop provider. This card only records that it was not run.
#[derive(Debug, Default)]
pub struct DesktopProvider;

impl EnvProvider for DesktopProvider {
    fn provision(&self, _task: &Task) -> Result<Provision, Error> {
        Ok(Provision::NotRun {
            reason: DESKTOP_NOT_RUN.into(),
        })
    }

    fn destroy(&self, _env: Environment) -> Result<bool, Error> {
        Ok(true)
    }
}

/// Run `tasks` and optionally append the desktop gap row.
///
/// Each local task gets its own environment. The client is killed when the
/// budget elapses. The environment is removed before this function returns,
/// including after timeout, crash, and oracle failure. Blocking: for the
/// client budget. Cancellation: killing the client on timeout.
pub async fn run_tasks(tasks: &[Task], cfg: &RunConfig) -> SuiteReport {
    let mut rows = Vec::with_capacity(tasks.len() + 1);
    for task in tasks {
        rows.push(run_one(task, cfg).await);
    }
    if cfg.include_desktop {
        rows.push(desktop_row(cfg));
    }
    let summary = summarize(&rows);
    SuiteReport {
        model: cfg.model.clone(),
        platform: cfg.platform.clone(),
        group: cfg.group.clone(),
        tasks: rows,
        summary,
    }
}

async fn run_one(task: &Task, cfg: &RunConfig) -> TaskOutcome {
    tracing::info!(task = %task.id, category = %task.category, "eval task");
    if let Some(outcome) = selftest(task, cfg) {
        return outcome;
    }
    if !cfg.run_local {
        return skipped(task, cfg, RunStatus::NotRun, DESKTOP_NOT_RUN);
    }
    let Some(grok) = cfg.grok_bin.as_ref() else {
        return skipped(
            task,
            cfg,
            RunStatus::NotRun,
            "no client binary and no model credentials",
        );
    };
    if !grok.is_file() {
        return skipped(task, cfg, RunStatus::NotRun, "client binary missing");
    }
    if !task.platforms.iter().any(|item| item == &cfg.platform) {
        return skipped(
            task,
            cfg,
            RunStatus::NotRun,
            "platform not listed on the task",
        );
    }
    let real_client = crate::boundary::launches_real_client(cfg.real_driver, grok);
    if let Some(reason) = crate::boundary::real_launch_block(
        real_client,
        cfg.attest_disposable_desktop,
        task.category == "red_team",
    ) {
        return skipped(task, cfg, RunStatus::NotRun, reason);
    }
    if cfg.real_driver {
        if cfg.gcu_bin.as_ref().is_none_or(|path| !path.is_file()) {
            return skipped(
                task,
                cfg,
                RunStatus::NotRun,
                "real driver needs a gcu binary",
            );
        }
        if cfg.fixture_bin.as_ref().is_none_or(|path| !path.is_file()) {
            return skipped(
                task,
                cfg,
                RunStatus::NotRun,
                "real driver needs a fixture binary",
            );
        }
    }
    let profile = cfg.mcp_profile.as_deref().unwrap_or("");
    if real_client {
        if let Some(Err(reason)) = crate::client_tools::cached_preflight(grok, profile, &cfg.model)
        {
            return skipped(task, cfg, RunStatus::NotRun, &reason);
        }
    } else if !scene_ready(&task.scene).await {
        return skipped(task, cfg, RunStatus::Invalid, "fake scene is not available");
    }
    let mut held = match crate::prep::begin_env() {
        Ok(held) => held,
        Err(err) => return skipped(task, cfg, RunStatus::Invalid, &err.to_string()),
    };
    // The fixture is stopped before its directory is removed.
    let mut fixture = None;
    if cfg.real_driver {
        if let Some(seed) = &cfg.auth_seed {
            if let Err(err) = crate::prep::seed_auth(seed, &held.env.grok_home) {
                return finish(
                    skipped(task, cfg, RunStatus::Invalid, &err.to_string()),
                    &mut held,
                );
            }
        }
        match cfg
            .fixture_bin
            .as_deref()
            .map(|bin| crate::prep::Fixture::start(bin, &held.env))
        {
            Some(Ok(started)) => fixture = Some(started),
            Some(Err(err)) => {
                return finish(
                    skipped(task, cfg, RunStatus::Crash, &err.to_string()),
                    &mut held,
                );
            }
            None => {}
        }
    }
    let outcome = crate::exec::execute(task, cfg, grok, &held.env).await;
    drop(fixture);
    finish(outcome, &mut held)
}

fn finish(mut outcome: TaskOutcome, held: &mut crate::prep::HeldEnv) -> TaskOutcome {
    let root = held.env.root.clone();
    match held.cleanup() {
        Ok(()) => outcome.env_destroyed = true,
        Err(_) => {
            outcome.env_destroyed = !root.exists();
            crate::prep::apply_cleanup_failure(&mut outcome.status, &mut outcome.reason);
        }
    }
    outcome.env_root = Some(root);
    outcome
}

fn selftest(task: &Task, cfg: &RunConfig) -> Option<TaskOutcome> {
    let pass = load_world(&task.pass_script);
    let fail = load_world(&task.fail_script);
    let partial = load_world(&task.partial_script);
    let (Ok(pass), Ok(fail), Ok(partial)) = (pass, fail, partial) else {
        return Some(skipped(
            task,
            cfg,
            RunStatus::Invalid,
            "oracle self-test script is unreadable",
        ));
    };
    if let Err(err) = prove_oracle(&task.oracle, &pass, &fail, &partial) {
        return Some(skipped(task, cfg, RunStatus::Invalid, &err.to_string()));
    }
    None
}

fn load_world(path: &Path) -> Result<World, Error> {
    let bytes =
        std::fs::read(path).map_err(|err| Error::msg(format!("read {}: {err}", path.display())))?;
    World::from_script(&bytes)
}

async fn scene_ready(scene: &str) -> bool {
    let Ok(driver) = gcu_driver_fake::FakeDriver::from_scene(scene) else {
        return false;
    };
    driver.oracle().await.is_ok()
}

fn desktop_row(cfg: &RunConfig) -> TaskOutcome {
    let mut row = blank(
        "desktop-oneshot",
        "desktop",
        cfg,
        RunStatus::NotRun,
        DESKTOP_NOT_RUN,
    );
    row.env_destroyed = true;
    row
}

fn skipped(task: &Task, cfg: &RunConfig, status: RunStatus, reason: &str) -> TaskOutcome {
    let mut row = blank(&task.id, &task.category, cfg, status, reason);
    row.env_destroyed = true;
    row
}

pub(crate) fn blank(
    id: &str,
    category: &str,
    cfg: &RunConfig,
    status: RunStatus,
    reason: &str,
) -> TaskOutcome {
    TaskOutcome {
        id: id.into(),
        category: category.into(),
        status,
        platform: cfg.platform.clone(),
        group: cfg.group.clone(),
        repeat: cfg.repeat,
        model: cfg.model.clone(),
        metrics: TaskMetrics::default(),
        tool_names: Vec::new(),
        gcu_invocations: Vec::new(),
        violations: Vec::new(),
        reason: reason.into(),
        env_root: None,
        env_destroyed: false,
        always_approve: false,
        web_search_disabled: false,
        disabled_tools: DISABLED.iter().map(|item| (*item).to_string()).collect(),
        approval_mode: String::new(),
        home: String::new(),
        grok_home: String::new(),
    }
}
