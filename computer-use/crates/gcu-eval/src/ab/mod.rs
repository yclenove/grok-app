//! Core-suite efficiency A/B.
//!
//! Group A is the existing `gcu mcp --profile minimal` surface: nine single-step
//! tools, no batch and no script. Its client policy passes `include_screenshot:
//! true` (the schema default, ADR 0011) and ends the turn between actions, so
//! each delivery is full. Group B passes `--profile full`, `include_screenshot:
//! "auto"`, and stays inside one turn so later deliveries use the existing diff.
//! Batch and script are requested only through that profile. Script stays
//! unregistered unless the existing Unix script feature and runner are present.
//!
//! This module does not add Host or ACP methods, does not relax a safety check,
//! and does not change a product default. [`decide`] only reports a conclusion.
//!
//! Round trips prefer the client transcript `turns` field. If it is absent, the
//! count is the runner's tool-log tally of computer-use invocations. Input and
//! output tokens are transcript fields; a missing field is unavailable and is
//! never replaced with tool-result bytes.

mod decide;
mod report;
mod stats;

pub use decide::{decide, ArmTotals, Conclusion, Decision, Gate};
pub use report::{format_utc_date, parse_report, render_markdown, write_reports, ReportPaths};
pub use stats::{change_milli_pct, median_milli, paired_metric, sign_flip_p, PairStats, AB_SEED};

use crate::{RoundsSource, RunConfig, RunStatus, Task, TaskOutcome};
use serde::{Deserialize, Serialize};

/// Repeats per task per arm. Quality gates section 5.4.
pub const REPEATS: u32 = 3;
/// Explanation stored on every report. Tokens are never inferred from bytes.
pub const ROUNDS_SOURCE_NOTE: &str = "Round trips use the client transcript turns field when it is present. Otherwise the count is the runner tool-log tally of computer-use invocations. Missing token fields stay unavailable and are never filled from tool-result bytes.";

/// One arm of the fixed experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArmId {
    /// Single-step tools, full state, screenshot true.
    #[serde(rename = "A")]
    A,
    /// Diff, auto screenshot, batch, and script requested.
    #[serde(rename = "B")]
    B,
}

impl ArmId {
    /// Stable label. Pure; no IO, timeout, or cancellation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "A" => Some(Self::A),
            "B" => Some(Self::B),
            _ => None,
        }
    }
}

/// Switches for one arm. Only existing profile and client policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmConfig {
    /// `gcu mcp --profile` value: `minimal` or `full`.
    pub profile: String,
    /// Client `include_screenshot` policy: `true` or `auto`.
    pub screenshot: String,
    /// `full_each_step` ends the turn between actions. `within_turn` keeps diffs.
    pub diff: String,
    /// Profile exposes `computer_batch`.
    pub batch: bool,
    /// Profile requests `computer_script`. Registration still follows the product gates.
    pub script_requested: bool,
}

impl ArmConfig {
    /// Group A. Does not disable redaction, Stop, or approvals.
    #[must_use]
    pub fn baseline() -> Self {
        Self {
            profile: "minimal".into(),
            screenshot: "true".into(),
            diff: "full_each_step".into(),
            batch: false,
            script_requested: false,
        }
    }

    /// Group B. Does not change the screenshot schema default.
    #[must_use]
    pub fn candidate() -> Self {
        Self {
            profile: "full".into(),
            screenshot: "auto".into(),
            diff: "within_turn".into(),
            batch: true,
            script_requested: true,
        }
    }
}

/// Both fixed arms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmPair {
    /// Group A.
    #[serde(rename = "A")]
    pub a: ArmConfig,
    /// Group B.
    #[serde(rename = "B")]
    pub b: ArmConfig,
}

impl ArmPair {
    /// The only arm pair this experiment accepts.
    #[must_use]
    pub fn fixed() -> Self {
        Self {
            a: ArmConfig::baseline(),
            b: ArmConfig::candidate(),
        }
    }
}

/// How the trials were produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// No client or no desktop. Every trial is `not_run`.
    NotRun,
    /// Local runner, including the scripted fake client. Not a model API call.
    Runner,
    /// A file passed to `--ab-report`.
    Supplied,
    /// Checked-in or constructed example. Cannot change defaults.
    Synthetic,
}

/// One repeat. Token fields are null when the transcript omitted them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbTrial {
    /// Task id.
    pub task_id: String,
    /// Task category. `red_team` feeds the violation gate.
    pub category: String,
    /// Arm.
    pub arm: ArmId,
    /// Zero-based repeat, 0 through 2.
    pub repeat: u32,
    /// Oracle status. Client completion text is not a pass.
    pub status: RunStatus,
    /// Model round trips. Null when the row was not scored or the source is unset.
    pub rounds: Option<u64>,
    /// Where `rounds` came from.
    pub rounds_source: RoundsSource,
    /// Transcript input tokens. Null is unavailable, not zero.
    pub tokens_in: Option<u64>,
    /// Transcript output tokens. Null is unavailable, not zero.
    pub tokens_out: Option<u64>,
    /// Images the client reported.
    pub images: u32,
    /// Image bytes the client reported.
    pub image_bytes: u64,
    /// Unknown tool results.
    pub unknown_calls: u64,
    /// Client claimed success and the oracle did not pass.
    pub false_success: bool,
    /// Extra approval prompts the client reported.
    pub extra_approvals: u64,
    /// Tool-result bytes from the tool log. Not a token count.
    pub tool_result_bytes: u64,
    /// Oracle and side-effect labels. No payloads.
    pub violations: Vec<String>,
    /// Why a row did not run. Empty when the oracle ran.
    #[serde(default)]
    pub reason: String,
}

impl AbTrial {
    fn passed(&self) -> bool {
        self.status == RunStatus::Pass
    }

    fn scored(&self) -> bool {
        !matches!(self.status, RunStatus::NotRun)
    }

    fn safety_hit(&self) -> bool {
        self.false_success || self.status == RunStatus::SideEffect || !self.violations.is_empty()
    }
}

/// Identity shared by both arms. No host name, cache total, or home path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportId {
    /// `linux`, `macos`, or `windows`.
    pub platform: String,
    /// Model id. `fake` does not call a network API.
    pub model: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// Effort label. Both arms share it. This card does not vary effort.
    pub effort: String,
    /// Nonpersonal machine label. `unspecified` when none was supplied.
    pub machine: String,
}

/// Versioned core A/B trials. The conclusion is not an input field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbReport {
    /// Schema version. Only 1 is accepted.
    pub schema_version: u32,
    /// Always `core`.
    pub suite: String,
    /// True on the checked-in example and any other synthetic world.
    pub synthetic: bool,
    /// Where the trials came from.
    pub provenance: Provenance,
    /// Platform label.
    pub platform: String,
    /// Model id.
    pub model: String,
    /// Report date.
    pub date: String,
    /// Shared effort.
    pub effort: String,
    /// Shared machine label.
    pub machine: String,
    /// [`AB_SEED`].
    pub seed: u64,
    /// Human-readable round-trip and token source note.
    pub rounds_source_note: String,
    /// Fixed arm switches.
    pub arms: ArmPair,
    /// Task repeats.
    pub trials: Vec<AbTrial>,
}

/// MCP argv for one arm. Only flags `gcu mcp` already accepts.
///
/// Pure; no IO, timeout, or cancellation.
#[must_use]
pub fn mcp_args(scene: &str, arm: &ArmConfig) -> Vec<String> {
    vec![
        "mcp".into(),
        "--driver".into(),
        "fake".into(),
        "--scene".into(),
        scene.into(),
        "--profile".into(),
        arm.profile.clone(),
    ]
}

/// Copy an arm onto a [`RunConfig`] using the existing profile flag and eval notes.
///
/// Does not set approval, and does not add an environment variable the server reads.
pub fn apply_arm(cfg: &mut RunConfig, arm: &ArmConfig, id: ArmId) {
    cfg.group = id.as_str().into();
    cfg.mcp_profile = Some(arm.profile.clone());
    cfg.arm_label = Some(id.as_str().into());
    cfg.arm_screenshot = Some(arm.screenshot.clone());
    cfg.arm_diff = Some(arm.diff.clone());
    cfg.arm_batch = Some(arm.batch);
    cfg.arm_script = Some(arm.script_requested);
}

/// Empty shell with the fixed arms and the source note.
#[must_use]
pub fn blank_report(id: &ReportId, provenance: Provenance, synthetic: bool) -> AbReport {
    AbReport {
        schema_version: 1,
        suite: "core".into(),
        synthetic,
        provenance,
        platform: id.platform.clone(),
        model: id.model.clone(),
        date: id.date.clone(),
        effort: id.effort.clone(),
        machine: id.machine.clone(),
        seed: AB_SEED,
        rounds_source_note: ROUNDS_SOURCE_NOTE.into(),
        arms: ArmPair::fixed(),
        trials: Vec::new(),
    }
}

/// Every core task, both arms, three repeats, all `not_run`.
///
/// Pure aside from reading the task list the caller already loaded.
#[must_use]
pub fn not_run_report(tasks: &[Task], id: &ReportId) -> AbReport {
    let mut report = blank_report(id, Provenance::NotRun, false);
    for task in tasks {
        for arm in [ArmId::A, ArmId::B] {
            for repeat in 0..REPEATS {
                report.trials.push(AbTrial {
                    task_id: task.id.clone(),
                    category: task.category.clone(),
                    arm,
                    repeat,
                    status: RunStatus::NotRun,
                    rounds: None,
                    rounds_source: RoundsSource::Unset,
                    tokens_in: None,
                    tokens_out: None,
                    images: 0,
                    image_bytes: 0,
                    unknown_calls: 0,
                    false_success: false,
                    extra_approvals: 0,
                    tool_result_bytes: 0,
                    violations: Vec::new(),
                    reason: "no client binary and no model credentials".into(),
                });
            }
        }
    }
    report
}

/// Map a runner row. Oracle status is kept; a missing token field stays null.
///
/// Pure; no IO, timeout, or cancellation.
#[must_use]
pub fn trial_from_outcome(row: &TaskOutcome) -> Option<AbTrial> {
    let arm = ArmId::parse(&row.group)?;
    let scored = row.scored() && row.metrics.rounds_source != RoundsSource::Unset;
    Some(AbTrial {
        task_id: row.id.clone(),
        category: row.category.clone(),
        arm,
        repeat: row.repeat,
        status: row.status,
        rounds: scored.then_some(row.metrics.steps),
        rounds_source: if scored {
            row.metrics.rounds_source
        } else {
            RoundsSource::Unset
        },
        tokens_in: row
            .metrics
            .tokens_in_reported
            .then_some(row.metrics.tokens_in),
        tokens_out: row
            .metrics
            .tokens_out_reported
            .then_some(row.metrics.tokens_out),
        images: row.metrics.images,
        image_bytes: row.metrics.image_bytes,
        unknown_calls: row.metrics.unknown_calls,
        false_success: row.metrics.false_success,
        extra_approvals: row.metrics.extra_approvals,
        tool_result_bytes: row.metrics.tool_result_bytes,
        violations: row.violations.clone(),
        reason: row.reason.clone(),
    })
}

/// Run both arms for three repeats. Does not call a model API.
///
/// Blocking: each client budget. Cancellation: the runner kills the client on
/// timeout. Without a client, every task is `not_run`. After any cleanup
/// failure, later arms and repeats are `not_run` and are not started.
pub async fn run_ab(tasks: &[Task], base: &RunConfig, id: &ReportId) -> AbReport {
    if (!base.builtin_fake && base.grok_bin.is_none()) || !base.run_local {
        return not_run_report(tasks, id);
    }
    let mut report = blank_report(id, Provenance::Runner, false);
    let mut halted: Option<String> = None;
    for (arm_id, arm) in [
        (ArmId::A, ArmConfig::baseline()),
        (ArmId::B, ArmConfig::candidate()),
    ] {
        for repeat in 0..REPEATS {
            if let Some(reason) = &halted {
                push_halted(&mut report, tasks, arm_id, repeat, reason);
                continue;
            }
            let mut cfg = base.clone();
            apply_arm(&mut cfg, &arm, arm_id);
            cfg.repeat = repeat;
            cfg.include_desktop = false;
            let suite = crate::run_tasks(tasks, &cfg).await;
            for row in suite.tasks {
                if row.id == "desktop-oneshot" {
                    continue;
                }
                if row.assurance.cleanup_failed {
                    halted = Some(format!(
                        "cleanup_failed: remaining tasks were not started ({})",
                        row.reason
                    ));
                }
                if let Some(trial) = trial_from_outcome(&row) {
                    report.trials.push(trial);
                }
            }
        }
    }
    report
}

fn push_halted(report: &mut AbReport, tasks: &[Task], arm: ArmId, repeat: u32, reason: &str) {
    for task in tasks {
        report.trials.push(AbTrial {
            task_id: task.id.clone(),
            category: task.category.clone(),
            arm,
            repeat,
            status: RunStatus::NotRun,
            rounds: None,
            rounds_source: RoundsSource::Unset,
            tokens_in: None,
            tokens_out: None,
            images: 0,
            image_bytes: 0,
            unknown_calls: 0,
            false_success: false,
            extra_approvals: 0,
            tool_result_bytes: 0,
            violations: Vec::new(),
            reason: reason.into(),
        });
    }
}
