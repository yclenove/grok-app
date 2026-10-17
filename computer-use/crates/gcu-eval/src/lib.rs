//! Computer-use evaluation runner.
//!
//! An isolated `GROK_HOME` is not an environment boundary. Real model runs
//! belong in a one-shot OS or desktop container with no user directory mount
//! and no credentials. [`DesktopProvider`] reports `not_run` until that
//! container exists. [`LocalTempProvider`] is for unit tests and the fake
//! driver only.
//!
//! This crate never reads or writes `~/.grok`, `~/.codex`, `~/.gemini`, or
//! `~/.claude`, and it never logs into an account, IM, or mailbox.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

mod access;
#[path = "../../../bench/mod.rs"]
pub mod budget;
mod exec;
mod metrics;
mod oracle;
mod prep;
mod report;
mod runner;
mod tasks;
mod yaml;

pub use metrics::{compare_trials, percentile, summarize, CategoryRate, PairRow, Summary, Trial};
pub use oracle::{
    judge, prove_oracle, EffectCheck, Judgment, OracleSpec, PathCheck, Verdict, World,
};
pub use report::{escape_html, render_html, trials_from_summary, write_reports, ReportPaths};
pub use runner::{
    block_on, bundled_tasks_dir, child_env_keys, host_platform, parse_approval, path_under_configs,
    run_tasks, user_config_dirs, ApprovalMode, DesktopProvider, EnvProvider, LocalTempProvider,
    Provision, RunConfig, RunStatus, SuiteReport, TaskMetrics, TaskOutcome, DESKTOP_NOT_RUN,
};
pub use tasks::{load_tasks, Task};

/// Recoverable runner failure. The text never includes a secret payload.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Parse or harness failure.
    #[error("{0}")]
    Message(String),
    /// `GCU_APPROVAL` is set on a build that must not auto-approve.
    #[error("{0}")]
    Approval(String),
}

impl Error {
    /// Harness error from text that is safe to print.
    #[must_use]
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}
