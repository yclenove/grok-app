//! One-shot environment providers.
//!
//! [`LocalTempProvider`] is a private temporary directory for tests and the
//! fake driver. It is not an OS sandbox. [`DesktopProvider`] does not launch
//! anything.

use super::{RunConfig, RunStatus, TaskOutcome};
use crate::tasks::Task;
use crate::Error;
use std::path::PathBuf;

/// Why the one-shot desktop provider did not run.
pub const DESKTOP_NOT_RUN: &str = "one-shot OS/desktop container or VM is not available; isolated GROK_HOME is not an environment boundary";

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
/// the agent. Timeout and cancellation are applied by [`super::run_tasks`],
/// which kills the client process.
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

/// Fields a report records about how the launch was constrained.
///
/// Missing telemetry is [`TelemetryState::Unknown`], never a measured zero.
/// A tool or side log is available only after the backend writes `hello`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct Assurance {
    /// The task client was spawned, even if scoring or cleanup later failed.
    /// False for setup failures and rows skipped after an earlier failure.
    #[serde(default)]
    pub client_started: bool,
    /// ADR 0017 §4 gate and cleanup evidence. None for local fake-only tasks.
    #[serde(default)]
    pub environment: Option<crate::disposable::EnvironmentReport>,
    /// `--attest-disposable-desktop` was set. This is not a confinement proof.
    #[serde(default)]
    pub attestation_provided: bool,
    /// `fake` or `real`.
    #[serde(default)]
    pub driver_mode: String,
    /// Tool-log read.
    #[serde(default)]
    pub tool_telemetry: TelemetryState,
    /// Side-effect log read.
    #[serde(default)]
    pub side_telemetry: TelemetryState,
    /// Fixture state read.
    #[serde(default)]
    pub state_telemetry: TelemetryState,
    /// Transcript read.
    #[serde(default)]
    pub transcript_telemetry: TelemetryState,
    /// Deleting the task tree failed. Later tasks in the run were not started.
    #[serde(default)]
    pub cleanup_failed: bool,
    /// A pipe reader failed or was aborted before EOF.
    #[serde(default)]
    pub capture_incomplete: bool,
}

/// Whether a log was actually read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryState {
    /// The file was missing, empty, handshake-less, unreadable, or over the size cap.
    #[default]
    Unknown,
    /// The producer wrote `hello` and the rest was read within the cap.
    Available,
}

impl TelemetryState {
    /// Report spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
        }
    }
}

/// Driver label stored on every row, including rows that never started.
pub(crate) fn driver_mode(cfg: &RunConfig) -> String {
    if cfg.real_driver {
        "real".into()
    } else {
        "fake".into()
    }
}

/// Assurance for a row that did not read logs.
pub(crate) fn assurance_for(cfg: &RunConfig) -> Assurance {
    Assurance {
        attestation_provided: cfg.attest_disposable_desktop,
        driver_mode: driver_mode(cfg),
        ..Assurance::default()
    }
}

pub(super) fn cleanup_failed(outcome: &mut TaskOutcome) {
    crate::prep::apply_cleanup_failure(&mut outcome.status, &mut outcome.reason);
    outcome.status = RunStatus::NotRun;
}

impl TaskOutcome {
    /// False for skipped rows, which stay out of rates and percentiles.
    #[must_use]
    pub fn scored(&self) -> bool {
        !matches!(self.status_for_report(), RunStatus::NotRun)
    }

    pub(crate) fn status_for_report(&self) -> RunStatus {
        if self.assurance.cleanup_failed
            || self
                .assurance
                .environment
                .as_ref()
                .is_some_and(|environment| !environment.complete())
        {
            RunStatus::NotRun
        } else {
            self.status
        }
    }
}
