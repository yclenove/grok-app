//! Broker and lease errors. Errors are never verification evidence.

use std::fmt;

pub const ERROR_CODES: &[&str] = &[
    "feature_disabled",
    "run_not_found",
    "target_unauthorized",
    "surface_unavailable",
    "identity_mismatch",
    "dead_target",
    "stop_requested",
    "lease_held",
    "duplicate_action",
    "schema",
    "adapter",
    "timeout",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerError {
    FeatureDisabled,
    RunNotFound,
    TargetUnauthorized,
    SurfaceUnavailable {
        surface: crate::adapter::SurfaceKind,
    },
    IdentityMismatch(&'static str),
    DeadTarget,
    StopRequested,
    LeaseHeld {
        run_id: String,
    },
    DuplicateAction,
    Schema(String),
    Adapter(String),
    BrowserWorker(crate::browser::WorkerError),
    Timeout,
}

impl BrokerError {
    pub fn code(&self) -> &'static str {
        match self {
            BrokerError::FeatureDisabled => "feature_disabled",
            BrokerError::RunNotFound => "run_not_found",
            BrokerError::TargetUnauthorized => "target_unauthorized",
            BrokerError::SurfaceUnavailable { .. } => "surface_unavailable",
            BrokerError::IdentityMismatch(_) => "identity_mismatch",
            BrokerError::DeadTarget => "dead_target",
            BrokerError::StopRequested => "stop_requested",
            BrokerError::LeaseHeld { .. } => "lease_held",
            BrokerError::DuplicateAction => "duplicate_action",
            BrokerError::Schema(_) => "schema",
            BrokerError::Adapter(_) => "adapter",
            BrokerError::BrowserWorker(_) => "adapter",
            BrokerError::Timeout => "timeout",
        }
    }
}

impl fmt::Display for BrokerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrokerError::FeatureDisabled => write!(f, "computer use is disabled"),
            BrokerError::RunNotFound => write!(f, "run not found"),
            BrokerError::TargetUnauthorized => write!(f, "target is not authorized"),
            BrokerError::SurfaceUnavailable { surface } => write!(
                f,
                "surface {} is unavailable; desktop fallback is forbidden",
                surface.as_wire()
            ),
            BrokerError::IdentityMismatch(field) => write!(f, "identity mismatch: {field}"),
            BrokerError::DeadTarget => {
                write!(f, "directed target is gone; desktop fallback is forbidden")
            }
            BrokerError::StopRequested => write!(f, "stop already requested; dispatch revoked"),
            BrokerError::LeaseHeld { run_id } => {
                write!(f, "desktop input lease held by run {run_id}")
            }
            BrokerError::DuplicateAction => write!(f, "actionId already used in this run"),
            BrokerError::Schema(s) => write!(f, "schema: {s}"),
            BrokerError::Adapter(s) => write!(f, "adapter: {s}"),
            BrokerError::BrowserWorker(error) => write!(f, "browser worker: {error}"),
            BrokerError::Timeout => write!(f, "adapter timed out; outcome unknown"),
        }
    }
}

impl std::error::Error for BrokerError {}

impl From<crate::browser::WorkerError> for BrokerError {
    fn from(error: crate::browser::WorkerError) -> Self {
        Self::BrowserWorker(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    Held { run_id: String, instance_id: String },
    Io(String),
}

impl fmt::Display for LeaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LeaseError::Held {
                run_id,
                instance_id,
            } => write!(
                f,
                "exclusive desktop lease held by run {run_id} instance {instance_id}"
            ),
            LeaseError::Io(s) => write!(f, "lease io: {s}"),
        }
    }
}

impl std::error::Error for LeaseError {}
