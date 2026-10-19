//! Atomic policy file. A failed write leaves the previous file in place.
use std::path::Path;

use super::office::settings::Snapshot;
use crate::messages::Policy;
use crate::owner_only::{self, SyncPolicy};

const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Read a policy snapshot. Missing, replaced, or unreadable files return None.
pub fn read_policy(path: &Path) -> Option<Snapshot> {
    let bytes = owner_only::read_no_follow(path, MAX_BYTES).ok()?;
    serde_json::from_slice(&bytes).ok().or_else(|| {
        serde_json::from_slice::<Policy>(&bytes)
            .ok()
            .map(|policy| Snapshot {
                policy,
                grants: Vec::new(),
                computer_use_enabled: false,
            })
    })
}

/// Replace `path` with `policy`. The parent directory is owner-only. Returns
/// false on any IO or serialization failure. An existing symlink is not replaced.
/// Nonblocking aside from the write itself; callers are already off the approval wait.
pub fn write_policy(path: &Path, policy: &Snapshot) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if parent.as_os_str().is_empty() || owner_only::ensure_dir(parent).is_err() {
        return false;
    }
    let Ok(bytes) = serde_json::to_vec(policy) else {
        return false;
    };
    owner_only::atomic_write(path, &bytes, SyncPolicy::Required).is_ok()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::messages::{ConfirmationLevel, ForegroundPolicy, Policy};
    use std::fs;

    fn scratch() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gcu-policy-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            policy: Policy {
                version: 1,
                always_allow_enabled: false,
                sensitive_action_confirmation: ConfirmationLevel::Standard,
                timeout_secs: 120,
                allow_foreground: ForegroundPolicy::Ask,
                macos_private_spi: false,
                user_idle_ms: 3000,
                stop_hotkey: "default".into(),
                max_tree_nodes: 1200,
                max_image_edge: 1568,
                image_format: "png".into(),
                extra_denied_apps: Vec::new(),
                always: Vec::new(),
            },
            grants: Vec::new(),
            computer_use_enabled: false,
        }
    }

    #[test]
    fn policy_symlink_is_not_loaded_or_replaced() {
        use std::os::unix::fs::symlink;
        let dir = scratch();
        let target = dir.join("real.json");
        fs::write(&target, b"{\"keep\":true}").unwrap();
        let link = dir.join("policy.json");
        symlink(&target, &link).unwrap();
        assert!(read_policy(&link).is_none());
        assert!(!write_policy(&link, &snapshot()));
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&target).unwrap(), b"{\"keep\":true}");
        assert!(!dir.join("policy.json.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
