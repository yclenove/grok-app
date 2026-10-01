//! Cross-process lock for Computer Use runtime seed mutation.
//!
//! `--prepare` and any test that mutates a seed tree must hold an exclusive lock.
//! `--check` is read-only: it takes the same exclusive lock so it never observes a
//! half-renamed tree, and returns typed `busy` instead of tearing the seed.
//! Isolated clones live on the same volume as the source so hardlinks work.

use crate::lease::pid_is_alive;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationOwner {
    pub pid: u32,
    pub run_id: String,
    pub seed: String,
    pub repo: String,
    pub started_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationError {
    pub code: String,
    pub message: String,
}

impl MutationError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn busy(owner: &MutationOwner) -> Self {
        Self::new(
            "busy",
            format!(
                "runtime mutation lock held by pid={} runId={} seed={}",
                owner.pid, owner.run_id, owner.seed
            ),
        )
    }
}

pub struct MutationLockGuard {
    file: File,
    path: PathBuf,
    owner: MutationOwner,
}

impl std::fmt::Debug for MutationLockGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MutationLockGuard")
            .field("path", &self.path)
            .field("owner", &self.owner)
            .finish()
    }
}

impl MutationLockGuard {
    pub fn owner(&self) -> &MutationOwner {
        &self.owner
    }
}

impl Drop for MutationLockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn mutation_lock_path(seed: &Path) -> PathBuf {
    seed.parent().unwrap_or(seed).join(".cu-mutation.lock")
}

pub fn mutation_owner_path(seed: &Path) -> PathBuf {
    PathBuf::from(format!("{}.owner.json", mutation_lock_path(seed).display()))
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn new_owner(seed: &Path, repo: &Path, run_id: impl Into<String>) -> MutationOwner {
    MutationOwner {
        pid: std::process::id(),
        run_id: run_id.into(),
        seed: seed.display().to_string(),
        repo: repo.display().to_string(),
        started_at_unix: now_unix(),
    }
}

pub fn try_acquire_exclusive(
    seed: &Path,
    owner: MutationOwner,
) -> Result<MutationLockGuard, MutationError> {
    let path = mutation_lock_path(seed);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| MutationError::new("io", e.to_string()))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| MutationError::new("io", e.to_string()))?;
    file.try_lock_exclusive().map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock
            || e.raw_os_error() == fs2::lock_contended_error().raw_os_error()
        {
            match read_owner_sidecar(seed) {
                Some(existing) => MutationError::busy(&existing),
                None => MutationError::new("busy", "runtime mutation lock held"),
            }
        } else {
            MutationError::new("io", e.to_string())
        }
    })?;
    // Exclusive lock is already held. A leftover sidecar from a dead process is
    // overwritten. Live owners cannot still hold this lock.
    let encoded =
        serde_json::to_vec_pretty(&owner).map_err(|e| MutationError::new("io", e.to_string()))?;
    fs::write(mutation_owner_path(seed), encoded)
        .map_err(|e| MutationError::new("io", e.to_string()))?;
    Ok(MutationLockGuard { file, path, owner })
}

fn read_owner_sidecar(seed: &Path) -> Option<MutationOwner> {
    let bytes = fs::read(mutation_owner_path(seed)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn staging_owner_path(staging: &Path) -> PathBuf {
    PathBuf::from(format!("{}.owner.json", staging.display()))
}

pub fn write_staging_owner(staging: &Path, owner: &MutationOwner) -> Result<(), MutationError> {
    fs::create_dir_all(staging).map_err(|e| MutationError::new("io", e.to_string()))?;
    let path = staging_owner_path(staging);
    let encoded =
        serde_json::to_vec_pretty(owner).map_err(|e| MutationError::new("io", e.to_string()))?;
    fs::write(&path, encoded).map_err(|e| MutationError::new("io", e.to_string()))?;
    Ok(())
}

pub fn remove_staging(staging: &Path, caller: &MutationOwner) -> Result<(), MutationError> {
    let marker = staging_owner_path(staging);
    if marker.is_file() {
        let bytes = fs::read(&marker).map_err(|e| MutationError::new("io", e.to_string()))?;
        if let Ok(owner) = serde_json::from_slice::<MutationOwner>(&bytes) {
            if owner.run_id != caller.run_id {
                if pid_is_alive(owner.pid) {
                    return Err(MutationError::new(
                        "not_owner",
                        format!(
                            "refusing to delete staging owned by runId={} pid={}",
                            owner.run_id, owner.pid
                        ),
                    ));
                }
                let seed_match = owner.seed == caller.seed;
                if !seed_match {
                    return Err(MutationError::new(
                        "not_owner",
                        "stale staging seed path does not match caller",
                    ));
                }
            }
        }
    }
    if staging.exists() {
        fs::remove_dir_all(staging).map_err(|e| MutationError::new("io", e.to_string()))?;
    }
    if marker.exists() {
        fs::remove_file(&marker).map_err(|e| MutationError::new("io", e.to_string()))?;
    }
    Ok(())
}

pub fn isolated_clone_dir(repo: &Path) -> PathBuf {
    repo.join("tools")
        .join("computer-use-probe")
        .join(".run")
        .join("isolated-seeds")
        .join(uuid::Uuid::new_v4().to_string())
}

pub fn clone_tree_hardlink(src: &Path, dst: &Path) -> Result<(), MutationError> {
    let meta = fs::symlink_metadata(src).map_err(|e| MutationError::new("io", e.to_string()))?;
    if meta.file_type().is_symlink() {
        return Err(MutationError::new(
            "symlink",
            format!("refusing to follow symlink {}", src.display()),
        ));
    }
    if meta.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| MutationError::new("io", e.to_string()))?;
        }
        fs::hard_link(src, dst).map_err(|e| {
            MutationError::new(
                "io",
                format!(
                    "hard_link {} -> {}: {e} (clone must stay on the same volume)",
                    src.display(),
                    dst.display()
                ),
            )
        })?;
        return Ok(());
    }
    fs::create_dir_all(dst).map_err(|e| MutationError::new("io", e.to_string()))?;
    for entry in fs::read_dir(src).map_err(|e| MutationError::new("io", e.to_string()))? {
        let entry = entry.map_err(|e| MutationError::new("io", e.to_string()))?;
        let name = entry.file_name();
        if name == ".cu-mutation.lock" || name == "owner.json" {
            continue;
        }
        clone_tree_hardlink(&entry.path(), &dst.join(name))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_seed() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grok-cu-mut-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("seed")
    }

    #[test]
    fn second_mutation_request_is_typed_busy() {
        let seed = temp_seed();
        fs::create_dir_all(&seed).unwrap();
        let repo = seed.parent().unwrap().to_path_buf();
        let a = try_acquire_exclusive(&seed, new_owner(&seed, &repo, "run-a")).expect("a");
        let err = try_acquire_exclusive(&seed, new_owner(&seed, &repo, "run-b")).unwrap_err();
        assert_eq!(err.code, "busy", "{err:?}");
        assert!(err.message.contains("run-a"), "{}", err.message);
        drop(a);
        try_acquire_exclusive(&seed, new_owner(&seed, &repo, "run-b")).expect("after release");
        let _ = fs::remove_dir_all(seed.parent().unwrap());
    }

    #[test]
    fn dead_pid_stale_lock_recovers_when_seed_matches() {
        let seed = temp_seed();
        fs::create_dir_all(&seed).unwrap();
        let repo = seed.parent().unwrap().to_path_buf();
        let path = mutation_lock_path(&seed);
        let leftover = MutationOwner {
            pid: 1,
            run_id: "ghost".into(),
            seed: seed.display().to_string(),
            repo: repo.display().to_string(),
            started_at_unix: 1,
        };
        fs::write(&path, b"lock-handle").unwrap();
        fs::write(
            mutation_owner_path(&seed),
            serde_json::to_vec(&leftover).unwrap(),
        )
        .unwrap();
        try_acquire_exclusive(&seed, new_owner(&seed, &repo, "fresh")).expect("stale recover");
        let _ = fs::remove_dir_all(seed.parent().unwrap());
    }

    #[test]
    fn non_owner_cannot_delete_live_staging() {
        let root = std::env::temp_dir().join(format!("grok-cu-stg-{}", uuid::Uuid::new_v4()));
        let staging = root.join("staging");
        let seed = root.join("seed");
        fs::create_dir_all(&seed).unwrap();
        let owner = MutationOwner {
            pid: std::process::id(),
            run_id: "owner-a".into(),
            seed: seed.display().to_string(),
            repo: root.display().to_string(),
            started_at_unix: now_unix(),
        };
        write_staging_owner(&staging, &owner).unwrap();
        fs::write(staging.join("keep.txt"), b"secret-pack").unwrap();
        assert!(staging_owner_path(&staging).is_file());
        let other = MutationOwner {
            pid: std::process::id(),
            run_id: "owner-b".into(),
            seed: seed.display().to_string(),
            repo: root.display().to_string(),
            started_at_unix: now_unix(),
        };
        let err = remove_staging(&staging, &other).unwrap_err();
        assert_eq!(err.code, "not_owner", "{err:?}");
        assert!(staging.join("keep.txt").is_file());
        remove_staging(&staging, &owner).expect("owner can delete");
        assert!(!staging.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn isolated_hardlink_clone_does_not_trample_source() {
        let src = std::env::temp_dir().join(format!("grok-cu-hl-src-{}", uuid::Uuid::new_v4()));
        let dst = src
            .parent()
            .unwrap()
            .join(format!("grok-cu-hl-dst-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(src.join("playwright")).unwrap();
        fs::write(src.join("playwright/loopback.mjs"), b"shared-sibling").unwrap();
        clone_tree_hardlink(&src, &dst).unwrap();
        fs::remove_file(dst.join("playwright/loopback.mjs")).unwrap();
        assert_eq!(
            fs::read(src.join("playwright/loopback.mjs")).unwrap(),
            b"shared-sibling"
        );
        assert!(!dst.join("playwright/loopback.mjs").exists());
        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }
}
