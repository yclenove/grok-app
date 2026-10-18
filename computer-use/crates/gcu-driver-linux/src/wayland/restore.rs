//! Wayland portal restore token persistence, origin binding, and lifecycle management.
//!
//! Enforces filesystem permissions (0700 for directory, 0600 for tokens), origin binding
//! (compositor, source type, window identity, process instance, authorization version),
//! validity bounds (at most 24h, not exceeding authorization expiry), and automatic cleanup
//! on expiry, feature disable, revocation, or session closure.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[cfg(unix)]
#[allow(dead_code)]
#[path = "../../../gcu-host-protocol/src/owner_unix.rs"]
mod owner_unix;

/// Maximum allowable lifetime for a Wayland portal restore token (24 hours).
pub const MAX_TOKEN_LIFETIME_SECS: u64 = 24 * 3600;

/// Errors arising from restore token validation and persistence.
#[derive(Debug, Error)]
pub enum RestoreError {
    /// Underlying filesystem I/O error.
    #[error("token storage I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Token record serialization or deserialization failure.
    #[error("token serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Restore token not found for specified key.
    #[error("restore token not found: {0}")]
    NotFound(String),

    /// Restore token has exceeded its 24-hour lifetime.
    #[error(
        "restore token expired: issued at {issued_at}, expires at {expires_at}, current time {now}"
    )]
    Expired {
        /// Timestamp when token was issued.
        issued_at: u64,
        /// Calculated token expiration time.
        expires_at: u64,
        /// Timestamp of check.
        now: u64,
    },

    /// Parent authorization has expired before token lifetime.
    #[error("parent authorization expired: auth expires at {auth_expires_at}, current time {now}")]
    AuthExpired {
        /// Timestamp when parent authorization expires.
        auth_expires_at: u64,
        /// Timestamp of check.
        now: u64,
    },

    /// Any bound origin item changed, invalidating the restore token.
    #[error("restore token origin mismatch: {0}")]
    OriginMismatch(String),

    /// Specified key contains invalid characters or path traversal.
    #[error("invalid token storage key: {0}")]
    InvalidKey(String),
}

/// Precise origin identity bound to a restore token.
///
/// If any bound field differs upon subsequent restore, the restore token
/// MUST be rejected and evicted to prevent cross-origin capture leakage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginBinding {
    /// Compositor family (e.g., "gnome", "kde", "wlroots", "other", "unknown").
    pub compositor: String,
    /// Granted capture source type (e.g., "window" or "monitor").
    pub source_kind: String,
    /// Window or display identity (e.g. window ID, surface handle, title, or display ID).
    pub window_identity: String,
    /// Executable image path and instance identity (e.g. "pid:starttime" or image dev/inode).
    pub process_instance: String,
    /// Policy or authorization version string.
    pub auth_version: String,
}

impl OriginBinding {
    /// Verify that this recorded binding matches the current execution environment.
    pub fn verify_match(&self, current: &OriginBinding) -> Result<(), RestoreError> {
        if self.compositor != current.compositor {
            return Err(RestoreError::OriginMismatch(format!(
                "compositor changed from '{}' to '{}'",
                self.compositor, current.compositor
            )));
        }
        if self.source_kind != current.source_kind {
            return Err(RestoreError::OriginMismatch(format!(
                "source kind changed from '{}' to '{}'",
                self.source_kind, current.source_kind
            )));
        }
        if self.window_identity != current.window_identity {
            return Err(RestoreError::OriginMismatch(format!(
                "window identity changed from '{}' to '{}'",
                self.window_identity, current.window_identity
            )));
        }
        if self.process_instance != current.process_instance {
            return Err(RestoreError::OriginMismatch(format!(
                "process instance changed from '{}' to '{}'",
                self.process_instance, current.process_instance
            )));
        }
        if self.auth_version != current.auth_version {
            return Err(RestoreError::OriginMismatch(format!(
                "authorization version changed from '{}' to '{}'",
                self.auth_version, current.auth_version
            )));
        }
        Ok(())
    }
}

/// Persisted record storing a restore token and its cryptographic/security bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreRecord {
    /// Opaque portal restore token returned by `org.freedesktop.portal.ScreenCast`.
    pub token: String,
    /// Origin identity bound to this token.
    pub binding: OriginBinding,
    /// Unix timestamp in seconds when the token was issued.
    pub issued_at_secs: u64,
    /// Calculated expiration timestamp (at most issued_at + 24 hours).
    pub expires_at_secs: u64,
    /// Expiration timestamp of the parent authorization.
    pub auth_expires_at_secs: u64,
}

impl RestoreRecord {
    /// Create a new restore record, bounding lifetime to min(24h, auth_expires_at).
    pub fn new(
        token: impl Into<String>,
        binding: OriginBinding,
        issued_at_secs: u64,
        auth_expires_at_secs: u64,
    ) -> Self {
        let max_24h = issued_at_secs.saturating_add(MAX_TOKEN_LIFETIME_SECS);
        let expires_at_secs = max_24h.min(auth_expires_at_secs);
        Self {
            token: token.into(),
            binding,
            issued_at_secs,
            expires_at_secs,
            auth_expires_at_secs,
        }
    }

    /// Check whether this token record has expired according to the provided clock.
    pub fn is_expired(&self, now_secs: u64) -> bool {
        now_secs >= self.expires_at_secs
            || now_secs >= self.auth_expires_at_secs
            || now_secs.saturating_sub(self.issued_at_secs) > MAX_TOKEN_LIFETIME_SECS
    }
}

/// Secure token store backed by user-private disk files.
#[derive(Debug, Clone)]
pub struct RestoreTokenStore {
    base_dir: PathBuf,
}

impl RestoreTokenStore {
    /// Create a token store rooted at the specified directory.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Default secure directory for token storage ($XDG_DATA_HOME/grok-computer-use/wayland-restore).
    pub fn default_dir() -> PathBuf {
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".local").join("share")
            });
        base.join("grok-computer-use").join("wayland-restore")
    }

    /// Path to the storage directory.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    fn sanitize_key(&self, key: &str) -> Result<PathBuf, RestoreError> {
        if key.is_empty()
            || key.contains('/')
            || key.contains('\\')
            || key.contains("..")
            || key.contains('\0')
        {
            return Err(RestoreError::InvalidKey(key.to_string()));
        }
        Ok(self.base_dir.join(format!("{key}.json")))
    }

    fn ensure_base_dir(&self) -> Result<(), RestoreError> {
        #[cfg(unix)]
        {
            owner_unix::ensure_dir(&self.base_dir)?;
        }
        #[cfg(not(unix))]
        {
            std::fs::create_dir_all(&self.base_dir)?;
        }
        Ok(())
    }

    /// Persist a restore record under the specified key with 0600 permissions.
    pub fn save(&self, key: &str, record: &RestoreRecord) -> Result<(), RestoreError> {
        self.ensure_base_dir()?;
        let path = self.sanitize_key(key)?;
        let serialized = serde_json::to_vec_pretty(record)?;
        #[cfg(unix)]
        owner_unix::atomic_write(&path, &serialized, true)?;
        #[cfg(not(unix))]
        std::fs::write(&path, &serialized)?;
        Ok(())
    }

    /// Load a restore record, validating expiration and origin binding against the current environment.
    ///
    /// If expired or if any origin binding field mismatches, the token file is deleted and an error is returned.
    pub fn load_and_validate(
        &self,
        key: &str,
        current: &OriginBinding,
        now_secs: u64,
    ) -> Result<String, RestoreError> {
        let path = self.sanitize_key(key)?;
        let content = read_token(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RestoreError::NotFound(key.to_string())
            } else {
                RestoreError::Io(error)
            }
        })?;
        let record: RestoreRecord = match serde_json::from_slice(&content) {
            Ok(r) => r,
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                return Err(RestoreError::Serialization(e));
            }
        };

        // 1. Verify parent authorization validity bound
        if now_secs >= record.auth_expires_at_secs {
            let _ = std::fs::remove_file(&path);
            return Err(RestoreError::AuthExpired {
                auth_expires_at: record.auth_expires_at_secs,
                now: now_secs,
            });
        }

        // 2. Verify 24h validity bound
        if now_secs >= record.expires_at_secs
            || now_secs.saturating_sub(record.issued_at_secs) > MAX_TOKEN_LIFETIME_SECS
        {
            let _ = std::fs::remove_file(&path);
            return Err(RestoreError::Expired {
                issued_at: record.issued_at_secs,
                expires_at: record.expires_at_secs,
                now: now_secs,
            });
        }

        // 3. Verify origin binding matching
        if let Err(mismatch) = record.binding.verify_match(current) {
            let _ = std::fs::remove_file(&path);
            return Err(mismatch);
        }

        Ok(record.token)
    }

    /// Revoke and delete a specific restore token by key.
    pub fn revoke(&self, key: &str) -> Result<(), RestoreError> {
        let path = self.sanitize_key(key)?;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Clear all tokens in this store (e.g. on feature disable or session termination).
    pub fn clear_all(&self) -> Result<usize, RestoreError> {
        if !self.base_dir.exists() {
            return Ok(0);
        }
        let mut count = 0;
        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                let _ = std::fs::remove_file(path);
                count += 1;
            }
        }
        Ok(count)
    }

    /// List all active token keys in the store.
    pub fn list_keys(&self) -> Result<Vec<String>, RestoreError> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }
        let mut keys = Vec::new();
        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    keys.push(stem.to_string());
                }
            }
        }
        keys.sort();
        Ok(keys)
    }
}

fn read_token(path: &Path) -> std::io::Result<Vec<u8>> {
    #[cfg(unix)]
    {
        owner_unix::read_no_follow(path, 64 * 1024)
    }
    #[cfg(not(unix))]
    {
        std::fs::read(path)
    }
}
