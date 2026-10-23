//! Computer-use evaluation runner.
//!
//! An isolated `GROK_HOME` is not an environment boundary. Real model runs
//! belong in a one-shot OS or desktop container with no user directory mount
//! and no credentials. [`DesktopProvider`] reports `not_run` until that
//! container exists. [`LocalTempProvider`] is for unit tests and the fake
//! driver only.
//!
//! The harness does not open the operator's config homes to copy credentials.
//! It refuses seeds and temp roots whose canonical path falls under those
//! homes, including the OS account home, `GROK_HOME`, `CODEX_HOME`, absolute
//! `GEMINI_*` and `CLAUDE_*` values, and `~/.grok-app` (`agent-home`,
//! `agent-home-official`). A pathname check is not proof of an evaluation-only
//! identity. A Unix process group is not an OS sandbox. Real mode stays
//! unavailable until an external isolation boundary (ADR 0017) exists.
//! Client version pinning and an offline tool-policy check are not implemented.

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
mod fake_agent;
mod fake_gcu;
mod ffi;
mod launch;
mod metrics;
mod oracle;
mod prep;
mod proc;
mod release;
mod report;
mod runner;
pub mod scope;
pub mod soak;
mod tasks;
mod yaml;

pub use access::GCU_ACCEPTED_APPROVAL;
pub use access::{config_dirs_from, path_within};
pub use boundary::{
    ATTEST_REQUIRED, EXTERNAL_GCU_REFUSED, INTERNAL_FAKE_AGENT, INTERNAL_FAKE_GCU,
    WINDOWS_REAL_NOT_RUN,
};
pub use client_tools::{PreflightCache, CLIENT_TOOL_PROBE_PROMPT, EXPECTED_PREFLIGHT_TOOLS};
pub use fake_agent::{builtin_fake_agent_entry, builtin_fake_agent_exit};
pub use fake_gcu::{builtin_fake_gcu_entry, builtin_fake_gcu_exit};
pub use metrics::{compare_trials, percentile, summarize, CategoryRate, PairRow, Summary, Trial};
pub use oracle::{
    judge, prove_oracle, EffectCheck, Judgment, OracleSpec, PathCheck, Verdict, World,
};
#[cfg(debug_assertions)]
pub use prep::{testing_fail_next_populate, testing_fail_next_removal};
pub use release::{
    absent_report, judge_release, parse_release_evidence, write_release_report, EvidenceSource,
    LayerStatus, PlatformRates, ReleaseChannel, ReleaseDecision, ReleaseEvidence, ReleaseTier,
    COMPATIBILITY_FLOOR, NATIVE_PREVIEW_BP, OVERALL_MAC_BP, OVERALL_OTHER_BP, SECURITY_ITEMS,
    SECURITY_PREVIEW,
};
pub use report::{escape_html, render_html, trials_from_summary, write_reports, ReportPaths};
pub use runner::{
    block_on, bundled_tasks_dir, child_env_keys, host_platform, parse_approval, path_under_configs,
    run_tasks, user_config_dirs, ApprovalMode, Assurance, DesktopProvider, EnvProvider,
    Environment, LocalTempProvider, Provision, RoundsSource, RunConfig, RunStatus, SuiteReport,
    TaskMetrics, TaskOutcome, TelemetryState, DESKTOP_NOT_RUN,
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
