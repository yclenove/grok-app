//! Versioned browser evidence records. No model claims are trusted summaries.

use crate::{PairRow, RunStatus, TaskMetrics};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Browser observation/action route selected by the experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Native browser accessibility, no DOM share used for delivery.
    L0,
    /// User-shared tab DOM enhancement through the existing ActionRouter.
    L2,
}

impl Level {
    /// Stable spelling. Pure; no IO, timeout or cancellation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::L0 => "l0",
            Self::L2 => "l2",
        }
    }
}

/// Provenance recorded by the trusted collector, never inferred from a score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    /// Matching disposable desktop/browser with native external side-effect scans.
    Native,
    /// Scripted/fake driver or oracle self-test; never qualifies for A/B.
    Fake,
    /// No matching collector was available.
    Unavailable,
}

/// Conditions that must be identical in both arms and all raw trial artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    /// `macos`, `windows`, or `linux-x11`; not a build host substitute.
    pub platform: String,
    /// Actual model revision/id, excluding the offline `fake` model.
    pub model: String,
    /// Actual effort setting.
    pub effort: String,
    /// Nonpersonal machine/CPU/memory identity.
    pub machine: String,
    /// OS and desktop version.
    pub os_version: String,
    /// System browser build and experiment profile/reset version.
    pub browser_version: String,
    /// Engine build/commit identity.
    pub engine_revision: String,
    /// Static fixture build/content revision.
    pub fixture_revision: String,
    /// Fixed task/oracle catalog content revision.
    pub taskset_revision: String,
    /// No real credentials or user-home mounts; isolated GROK_HOME is insufficient.
    pub disposable_desktop: bool,
}

/// Complete external scan coverage, including tools other than computer-use.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    /// File baseline/final scans and attempted writes were collected.
    pub files: bool,
    /// Process creation/attempts were independently collected.
    pub processes: bool,
    /// Network attempts outside the allowed local fixture channel were collected.
    pub network: bool,
}

/// One native raw external observation; not an agent's completion statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTrial {
    /// Exactly the supported browser contract version.
    pub schema_version: u32,
    /// Driver, clock and all external scans must be native for this label.
    pub kind: EvidenceKind,
    /// Selected experiment arm.
    pub level: Level,
    /// Actual collection conditions, matched to the index.
    pub metadata: Metadata,
    /// Fixed task id, matched to the index.
    pub task_id: String,
    /// Zero-based repeat index, exactly 0, 1 or 2.
    pub repeat: u32,
    /// Pre-invocation reset digest, matched across the two arms for this pair.
    pub initial_state: String,
    /// Runner termination status; `pass` still requires the external oracle.
    pub status: RunStatus,
    /// Existing external-world format: state, file fingerprints, processes, network.
    pub world: serde_json::Value,
    /// Independent scan completeness; absence is never treated as a clean scan.
    pub coverage: Coverage,
    /// One-shot desktop/profile and sessions were destroyed after collection.
    pub env_destroyed: bool,
    /// Existing duration/turn/tool/token/image/approval metrics, without payloads.
    pub metrics: TaskMetrics,
    /// Additional external collector violations, including disabled tools.
    pub violations: Vec<String>,
}

/// Reference to one external observation; no inline scores are accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialRef {
    /// Fixed task id.
    pub task_id: String,
    /// Exactly 0, 1 or 2.
    pub repeat: u32,
    /// Actual pre-run reset digest, without an arm-dependent share identifier.
    pub initial_state: String,
    /// Bundle-relative `artifacts/...json` path, never a self-test path.
    pub artifact: PathBuf,
}

/// One of exactly two arms, with its own native raw artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arm {
    /// Unique L0 or L2 route.
    pub level: Level,
    /// Actual conditions; null when the matching desktop was not run.
    pub metadata: Option<Metadata>,
    /// Complete fixed task set, three repeats each, no duplicates.
    pub trials: Vec<TrialRef>,
}

/// Versioned report index. Recommendations are derived, never input fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserReport {
    /// Exactly the supported contract version.
    pub schema_version: u32,
    /// Exactly `browser`.
    pub suite: String,
    /// Exactly one L0 and one L2 arm; an empty list represents not_run.
    pub arms: Vec<Arm>,
}

/// Whether the evidence is sufficient for a measured A/B decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    /// Complete native paired experiment, including a measured negative result.
    Complete,
    /// Missing samples, incompatible conditions, or missing native evidence.
    NotRun,
}

impl ValidationStatus {
    /// CLI spelling. Pure; no IO, timeout or cancellation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::NotRun => "not_run",
        }
    }
}

/// Recommended default, without changing any product setting.
pub type DefaultLevel = Level;

/// Recomputed arm totals. Missing evidence has no published success rate.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ArmSummary {
    /// Complete externally judged samples.
    pub scored: usize,
    /// Runner and external oracle both passed, without violations.
    pub passed: usize,
    /// Absent unless both native arms qualify completely.
    pub success_rate: Option<f64>,
    /// Nearest-rank median measured latency.
    pub p50_ms: Option<u64>,
    /// Nearest-rank 95th percentile measured latency.
    pub p95_ms: Option<u64>,
}

/// Derived decision and paired significance, separate from the raw input index.
#[derive(Debug, Clone, Serialize)]
pub struct Validation {
    /// Evidence qualification, never a claim that L2 is necessarily better.
    pub status: ValidationStatus,
    /// True only for improved L2 success and zero safety violations.
    pub recommend_l2: bool,
    /// L0 unless the complete experiment meets the recommendation rule.
    pub default_level: DefaultLevel,
    /// Recomputed L0 totals.
    pub l0: ArmSummary,
    /// Recomputed L2 totals.
    pub l2: ArmSummary,
    /// Violations across red-team tasks in both arms.
    pub red_team_violations: usize,
    /// All observed violations, including CU-external effects on normal tasks.
    pub violations: Vec<String>,
    /// Task-level paired sign-test results; no new significance threshold.
    pub pairs: Vec<PairRow>,
    /// Explicit missing-evidence or keep-L0 reasons.
    pub reasons: Vec<String>,
}
