//! Failures from planning or applying a client install.

use std::path::PathBuf;

/// Why an install plan or apply stopped.
///
/// Library paths return this instead of panicking. Filesystem and process
/// calls that fail leave the caller's config unmodified when the error is
/// reported before the first write; see [`super::Plan`].
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// A change under `<home>/.grok` needs a person, and the context is not interactive.
    #[error(
        "confirmation required to modify files under ~/.grok; pass --yes or run interactively"
    )]
    ConfirmationRequired,
    /// The confirmer declined a change under `<home>/.grok`.
    #[error("confirmation to modify files under ~/.grok was declined")]
    ConfirmationDeclined,
    /// The file is not valid TOML, JSON, or UTF-8. It was not modified.
    #[error("cannot parse {path}: {source}")]
    Unparseable {
        /// File that failed to parse.
        path: PathBuf,
        /// Parser or encoding error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The file parsed, but a key this editor must traverse has the wrong type.
    #[error("{path} has an unexpected {detail}")]
    UnexpectedShape {
        /// File left untouched.
        path: PathBuf,
        /// Dotted key or JSON pointer that has the wrong type.
        detail: String,
    },
    /// Antigravity's user scope needs an `agy` path and none was injected.
    #[error("client binary for {client} is not set")]
    MissingClientBinary {
        /// Client that needed the binary.
        client: super::ClientId,
    },
    /// The injected executor returned a non-zero status, or the program failed to spawn.
    #[error("command {program} exited with status {status}: {stderr}")]
    CommandFailed {
        /// Program that was spawned.
        program: String,
        /// Exit status, or `-1` when the process had none.
        status: i32,
        /// Stderr captured before the process exited, truncated.
        stderr: String,
    },
    /// The injected process executor killed the child after its timeout.
    #[error("command {program} timed out after {timeout_ms}ms")]
    CommandTimeout {
        /// Program that was killed.
        program: String,
        /// Timeout that elapsed.
        timeout_ms: u64,
    },
    /// `InstallCtx::gcu_exe` was not absolute.
    #[error("gcu executable path must be absolute")]
    GcuPathNotAbsolute,
    /// `home` or `project_dir` was not absolute.
    #[error("{what} must be an absolute path")]
    PathNotAbsolute {
        /// `home` or `project_dir`.
        what: &'static str,
    },
    /// A path that must be stored in a config file was not UTF-8.
    #[error("{what} path is not valid UTF-8")]
    PathNotUnicode {
        /// Which path failed.
        what: &'static str,
    },
    /// The backup suffix was empty or contained a path separator.
    #[error("backup stamp is empty or contains path separators")]
    InvalidBackupStamp,
    /// A hook script already occupies our file name and is not ours.
    #[error("refusing to overwrite foreign file {path}")]
    ForeignFile {
        /// File left untouched.
        path: PathBuf,
    },
    /// The file's bytes changed after the plan was built.
    #[error("{path} changed between planning and applying")]
    ChangedDuringPlan {
        /// File left untouched.
        path: PathBuf,
    },
    /// A read failed for a reason other than a missing file.
    #[error("failed to read {path}: {source}")]
    Read {
        /// Path that was read.
        path: PathBuf,
        /// OS error.
        #[source]
        source: std::io::Error,
    },
    /// A write, rename, or directory create failed.
    #[error("failed to write {path}: {source}")]
    Write {
        /// Path that was written.
        path: PathBuf,
        /// OS error.
        #[source]
        source: std::io::Error,
    },
    /// `RecordingExecutor`'s mutex was poisoned.
    #[error("install state lock was poisoned")]
    LockPoisoned,
}

pub(crate) fn unparseable(
    path: &std::path::Path,
    source: impl std::error::Error + Send + Sync + 'static,
) -> InstallError {
    InstallError::Unparseable {
        path: path.to_path_buf(),
        source: Box::new(source),
    }
}

pub(crate) fn unexpected(path: &std::path::Path, detail: impl Into<String>) -> InstallError {
    InstallError::UnexpectedShape {
        path: path.to_path_buf(),
        detail: detail.into(),
    }
}

pub(crate) fn read_err(path: &std::path::Path, source: std::io::Error) -> InstallError {
    InstallError::Read {
        path: path.to_path_buf(),
        source,
    }
}

pub(crate) fn write_err(path: &std::path::Path, source: std::io::Error) -> InstallError {
    InstallError::Write {
        path: path.to_path_buf(),
        source,
    }
}
