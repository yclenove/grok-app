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

pub mod ab;
mod access;
mod boundary;
pub mod browser;
#[path = "../../../bench/mod.rs"]
pub mod budget;
mod client_tools;
mod exec;
mod ffi;
mod launch;
mod metrics;
mod oracle;
mod prep;
mod proc;
mod release;
mod report;
mod runner;
pub mod soak;
mod tasks;
mod yaml;

pub use access::GCU_ACCEPTED_APPROVAL;
pub use boundary::{is_eval_fake_agent, ATTEST_REQUIRED};
pub use client_tools::{CLIENT_TOOL_PROBE_PROMPT, EXPECTED_PREFLIGHT_TOOLS};
pub use metrics::{compare_trials, percentile, summarize, CategoryRate, PairRow, Summary, Trial};
pub use oracle::{
    judge, prove_oracle, EffectCheck, Judgment, OracleSpec, PathCheck, Verdict, World,
};
pub use release::{
    absent_report, judge_release, parse_release_evidence, write_release_report, EvidenceSource,
    LayerStatus, PlatformRates, ReleaseChannel, ReleaseDecision, ReleaseEvidence, ReleaseTier,
    COMPATIBILITY_FLOOR, NATIVE_PREVIEW_BP, OVERALL_MAC_BP, OVERALL_OTHER_BP, SECURITY_ITEMS,
    SECURITY_PREVIEW,
};
pub use report::{escape_html, render_html, trials_from_summary, write_reports, ReportPaths};
pub use runner::{
    block_on, bundled_tasks_dir, child_env_keys, host_platform, parse_approval, path_under_configs,
    run_tasks, user_config_dirs, ApprovalMode, DesktopProvider, EnvProvider, LocalTempProvider,
    Provision, RoundsSource, RunConfig, RunStatus, SuiteReport, TaskMetrics, TaskOutcome,
    DESKTOP_NOT_RUN,
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
