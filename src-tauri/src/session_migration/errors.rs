//! Stable import error codes. UI maps these through 15 locales.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    SourceNotFound,
    SourcePermissionDenied,
    SourceBusy,
    SourceChanged,
    SnapshotUnavailable,
    FormatUnsupported,
    SchemaUnsupported,
    PartialHistory,
    WorkspaceUnresolved,
    MappingConflict,
    ResourceLimit,
    DiskSpaceLow,
    PlanStale,
    JobStateChanged,
    JobActionConflict,
    ImportedHistoryReadOnly,
    HistoryAccessUnavailable,
    HandoffChanged,
    DispatchUnknown,
    ImportSchemaTooNew,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceNotFound => "SOURCE_NOT_FOUND",
            Self::SourcePermissionDenied => "SOURCE_PERMISSION_DENIED",
            Self::SourceBusy => "SOURCE_BUSY",
            Self::SourceChanged => "SOURCE_CHANGED",
            Self::SnapshotUnavailable => "SNAPSHOT_UNAVAILABLE",
            Self::FormatUnsupported => "FORMAT_UNSUPPORTED",
            Self::SchemaUnsupported => "SCHEMA_UNSUPPORTED",
            Self::PartialHistory => "PARTIAL_HISTORY",
            Self::WorkspaceUnresolved => "WORKSPACE_UNRESOLVED",
            Self::MappingConflict => "MAPPING_CONFLICT",
            Self::ResourceLimit => "RESOURCE_LIMIT",
            Self::DiskSpaceLow => "DISK_SPACE_LOW",
            Self::PlanStale => "PLAN_STALE",
            Self::JobStateChanged => "JOB_STATE_CHANGED",
            Self::JobActionConflict => "JOB_ACTION_CONFLICT",
            Self::ImportedHistoryReadOnly => "IMPORTED_HISTORY_READ_ONLY",
            Self::HistoryAccessUnavailable => "HISTORY_ACCESS_UNAVAILABLE",
            Self::HandoffChanged => "HANDOFF_CHANGED",
            Self::DispatchUnknown => "DISPATCH_UNKNOWN",
            Self::ImportSchemaTooNew => "IMPORT_SCHEMA_TOO_NEW",
        }
    }

    pub fn retry_kind(self) -> &'static str {
        match self {
            Self::SourceBusy
            | Self::DiskSpaceLow
            | Self::HistoryAccessUnavailable
            | Self::HandoffChanged => "retry",
            Self::SourceChanged | Self::PlanStale | Self::PartialHistory => "rescan",
            Self::DispatchUnknown | Self::JobStateChanged | Self::JobActionConflict => "manual",
            _ => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum FailureOrigin {
    #[default]
    Shared,
    Job,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportError {
    pub code: ErrorCode,
    pub message: String,
    pub retry: String,
    pub scope: String,
    // Internal scheduling provenance, NEVER a caller/wire permission flag.
    #[serde(skip)]
    failure_origin: FailureOrigin,
}

impl PartialEq for ImportError {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
            && self.message == other.message
            && self.retry == other.retry
            && self.scope == other.scope
    }
}

impl ImportError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            retry: code.retry_kind().to_string(),
            code,
            message: message.into(),
            scope: "item".into(),
            failure_origin: FailureOrigin::Shared,
        }
    }

    pub fn into_command_err(self) -> String {
        format!("{}: {}", self.code.as_str(), self.message)
    }

    pub(in crate::session_migration) fn job_local(mut self) -> Self {
        self.failure_origin = FailureOrigin::Job;
        self
    }

    pub(in crate::session_migration) fn catalog_wide(mut self) -> Self {
        self.failure_origin = FailureOrigin::Shared;
        self
    }

    pub(in crate::session_migration) fn is_job_local(&self) -> bool {
        self.failure_origin == FailureOrigin::Job
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ImportError {}

#[cfg(test)]
#[path = "storage_failure_tests.rs"]
mod storage_failure_tests;

/// Preserve machine-readable SQLite failures before callers erase their type.
/// Error text can contain paths or custom messages; it is not a result code.
pub(super) fn sqlite_storage_error(cause: rusqlite::Error) -> ImportError {
    let code = match cause.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DiskFull) => ErrorCode::DiskSpaceLow,
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            ErrorCode::SourceBusy
        }
        _ => ErrorCode::SnapshotUnavailable,
    };
    ImportError::new(code, cause.to_string())
}

/// A capacity check cannot reserve free space. Preserve write/sync/rename
/// failures by type, not OS-language-dependent text, so an explicit retry works.
pub(super) fn io_storage_error(cause: std::io::Error) -> ImportError {
    let code = if cause.kind() == std::io::ErrorKind::StorageFull {
        ErrorCode::DiskSpaceLow
    } else if cfg!(windows) && matches!(cause.raw_os_error(), Some(32 | 33)) {
        // ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION are transient. Other
        // permission failures must not be mislabeled as source permissions.
        ErrorCode::SourceBusy
    } else {
        ErrorCode::SnapshotUnavailable
    };
    ImportError::new(code, cause.to_string())
}

/// SQLite writes made while holding another store lock must fail without a
/// wait. Preserve a retryable result through wrappers that carry display text.
pub(super) fn storage_error(cause: impl std::fmt::Display) -> ImportError {
    let message = cause.to_string();
    let code =
        if message.contains("database is locked") || message.contains("database table is locked") {
            ErrorCode::SourceBusy
        } else {
            ErrorCode::SnapshotUnavailable
        };
    ImportError::new(code, message)
}

/// Parse a host command error that used [`ImportError::into_command_err`].
#[cfg(test)]
pub fn parse_command_err(raw: &str) -> Option<ErrorCode> {
    let code = raw.split(':').next()?.trim();
    match code {
        "SOURCE_NOT_FOUND" => Some(ErrorCode::SourceNotFound),
        "SOURCE_PERMISSION_DENIED" => Some(ErrorCode::SourcePermissionDenied),
        "SOURCE_BUSY" => Some(ErrorCode::SourceBusy),
        "SOURCE_CHANGED" => Some(ErrorCode::SourceChanged),
        "SNAPSHOT_UNAVAILABLE" => Some(ErrorCode::SnapshotUnavailable),
        "FORMAT_UNSUPPORTED" => Some(ErrorCode::FormatUnsupported),
        "SCHEMA_UNSUPPORTED" => Some(ErrorCode::SchemaUnsupported),
        "PARTIAL_HISTORY" => Some(ErrorCode::PartialHistory),
        "WORKSPACE_UNRESOLVED" => Some(ErrorCode::WorkspaceUnresolved),
        "MAPPING_CONFLICT" => Some(ErrorCode::MappingConflict),
        "RESOURCE_LIMIT" => Some(ErrorCode::ResourceLimit),
        "DISK_SPACE_LOW" => Some(ErrorCode::DiskSpaceLow),
        "PLAN_STALE" => Some(ErrorCode::PlanStale),
        "JOB_STATE_CHANGED" => Some(ErrorCode::JobStateChanged),
        "JOB_ACTION_CONFLICT" => Some(ErrorCode::JobActionConflict),
        "IMPORTED_HISTORY_READ_ONLY" => Some(ErrorCode::ImportedHistoryReadOnly),
        "HISTORY_ACCESS_UNAVAILABLE" => Some(ErrorCode::HistoryAccessUnavailable),
        "HANDOFF_CHANGED" => Some(ErrorCode::HandoffChanged),
        "DISPATCH_UNKNOWN" => Some(ErrorCode::DispatchUnknown),
        "IMPORT_SCHEMA_TOO_NEW" => Some(ErrorCode::ImportSchemaTooNew),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_err_roundtrip() {
        let e = ImportError::new(ErrorCode::SourceBusy, "locked");
        let s = e.into_command_err();
        assert_eq!(parse_command_err(&s), Some(ErrorCode::SourceBusy));
        let changed = ImportError::new(ErrorCode::HandoffChanged, "draft edited");
        assert_eq!(changed.retry, "retry");
        assert_eq!(
            parse_command_err(&changed.into_command_err()),
            Some(ErrorCode::HandoffChanged)
        );
    }

    #[test]
    fn refusal_origin_is_internal_and_not_an_error_code_whitelist() {
        let shared = ImportError::new(ErrorCode::SchemaUnsupported, "synthetic schema");
        let local = shared.clone().job_local();
        assert!(!shared.is_job_local());
        assert!(local.is_job_local());
        assert!(!local.clone().catalog_wide().is_job_local());
        let wire = serde_json::to_value(&local).unwrap();
        assert_eq!(wire.as_object().unwrap().len(), 4);
        let restored: ImportError = serde_json::from_value(wire).unwrap();
        assert_eq!(restored, local);
        assert!(
            !restored.is_job_local(),
            "wire cannot manufacture scheduling permission"
        );
    }
}
