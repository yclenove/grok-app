//! Linux application identity resolution, verification, and re-authorization checks.
//!
//! Linux applications are identified by:
//! 1. Canonical executable path (`fs::canonicalize`, resolving symlinks)
//! 2. File identity (`dev` and `inode`)
//! 3. SHA-256 binary content digest
//! 4. Process instance verification (`pid` + `start_time` from `/proc/<pid>/stat`)
//!
//! Note: `desktop_id` is used strictly for discovery, never as an authorization key.

use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

/// Verified identity of a Linux application process and its backing executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    /// Canonical path of the executable (symlinks resolved).
    pub exe_path: PathBuf,
    /// Filesystem device ID.
    pub dev: u64,
    /// Inode of the executable file.
    pub inode: u64,
    /// SHA-256 digest of the binary file content.
    pub sha256: String,
    /// Process ID.
    pub pid: u32,
    /// Process start time (jiffies since boot from `/proc/<pid>/stat`).
    pub start_time: u64,
    /// Desktop entry ID (e.g. `"org.gnome.gedit.desktop"`), used ONLY for discovery.
    pub desktop_id: Option<String>,
}

/// Compute SHA-256 hash of a file at `path`.
pub fn compute_sha256(path: &Path) -> Result<String, io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let digest = hasher.finalize();
    Ok(format!("{digest:x}"))
}

/// Read process start time from `/proc/<pid>/stat`.
///
/// On Linux, `/proc/<pid>/stat` contains space-separated fields where field 2 is `(comm)`.
/// Field 22 (19th field after the closing parenthesis) is the `starttime` in clock ticks.
pub fn get_process_start_time(pid: u32) -> Result<u64, io::Error> {
    #[cfg(target_os = "linux")]
    {
        let stat_path = format!("/proc/{pid}/stat");
        let content = std::fs::read_to_string(&stat_path)?;
        parse_stat_start_time(&content).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to parse starttime from {stat_path}"),
            )
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Portable fallback for compilation and non-Linux test runners
        let _ = pid;
        Ok(1000)
    }
}

/// Parse the starttime field from the contents of a `/proc/<pid>/stat` file.
pub fn parse_stat_start_time(stat_content: &str) -> Option<u64> {
    // Locate the last closing parenthesis, which marks the end of `(comm)`
    let last_paren = stat_content.rfind(')')?;
    let remainder = stat_content.get(last_paren + 1..)?;
    let fields: Vec<&str> = remainder.split_whitespace().collect();
    // In `/proc/<pid>/stat`:
    // Field 1: pid
    // Field 2: comm
    // Field 3 (remainder[0]): state
    // ...
    // Field 22 (remainder[19]): starttime
    fields.get(19).and_then(|val| val.parse::<u64>().ok())
}

impl AppIdentity {
    /// Inspect a running process by PID and construct its verified `AppIdentity`.
    pub fn from_pid(pid: u32, desktop_id: Option<String>) -> Result<Self, io::Error> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            let exe_symlink = format!("/proc/{pid}/exe");
            let canonical_exe = std::fs::canonicalize(&exe_symlink)?;
            let meta = std::fs::metadata(&canonical_exe)?;
            let dev = meta.dev();
            let inode = meta.ino();
            let sha256 = compute_sha256(&canonical_exe)?;
            let start_time = get_process_start_time(pid)?;

            Ok(Self {
                exe_path: canonical_exe,
                dev,
                inode,
                sha256,
                pid,
                start_time,
                desktop_id,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (pid, desktop_id);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process inspection is only supported on Linux",
            ))
        }
    }
}

/// Evaluate whether changes between `old` and `new` identities require user re-authorization.
///
/// Rules:
/// - In-place binary modification or update (`sha256` changed) -> `true`
/// - In-place inode or device replacement -> `true`
/// - Path replacement (different canonical path) -> `true`
/// - PID reuse (different start time or different PID) -> `true`
/// - Same canonical path, same dev/inode, same hash, same process instance -> `false`
/// - `desktop_id` changes alone do NOT require re-authorization (discovery only).
pub fn needs_reauth(old: &AppIdentity, new: &AppIdentity) -> bool {
    if old.exe_path != new.exe_path {
        return true;
    }
    if old.dev != new.dev || old.inode != new.inode {
        return true;
    }
    if old.sha256 != new.sha256 {
        return true;
    }
    if old.pid != new.pid || old.start_time != new.start_time {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_identity(
        path: &str,
        dev: u64,
        inode: u64,
        hash: &str,
        pid: u32,
        start_time: u64,
        desktop_id: Option<&str>,
    ) -> AppIdentity {
        AppIdentity {
            exe_path: PathBuf::from(path),
            dev,
            inode,
            sha256: hash.to_string(),
            pid,
            start_time,
            desktop_id: desktop_id.map(ToString::to_string),
        }
    }

    #[test]
    fn parse_proc_stat_starttime() {
        let sample = "1234 (gedit (main)) S 1000 1234 1234 0 -1 4194304 123 0 0 0 10 20 0 0 20 0 5 0 987654 12345 6789";
        let start_time = parse_stat_start_time(sample);
        assert_eq!(start_time, Some(987654));
    }

    #[test]
    fn table_driven_reauth_scenarios() {
        struct Case {
            name: &'static str,
            old: AppIdentity,
            new: AppIdentity,
            expected_reauth: bool,
        }

        let cases = vec![
            Case {
                name: "identical_process_and_binary",
                old: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a",
                    1234,
                    555,
                    Some("gedit.desktop"),
                ),
                new: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a",
                    1234,
                    555,
                    Some("gedit.desktop"),
                ),
                expected_reauth: false,
            },
            Case {
                name: "desktop_file_changed_entry_to_different_binary",
                old: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a",
                    1234,
                    555,
                    Some("gedit.desktop"),
                ),
                new: sample_identity(
                    "/usr/bin/gnome-text-editor",
                    1,
                    200,
                    "hash_b",
                    1234,
                    555,
                    Some("gedit.desktop"),
                ),
                expected_reauth: true,
            },
            Case {
                name: "in_place_binary_update_hash_changed",
                old: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a",
                    1234,
                    555,
                    Some("gedit.desktop"),
                ),
                new: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a_updated",
                    1234,
                    555,
                    Some("gedit.desktop"),
                ),
                expected_reauth: true,
            },
            Case {
                name: "in_place_binary_replaced_inode_changed",
                old: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a",
                    1234,
                    555,
                    Some("gedit.desktop"),
                ),
                new: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    101,
                    "hash_a",
                    1234,
                    555,
                    Some("gedit.desktop"),
                ),
                expected_reauth: true,
            },
            Case {
                name: "pid_reused_different_start_time",
                old: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a",
                    1234,
                    555,
                    Some("gedit.desktop"),
                ),
                new: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a",
                    1234,
                    999,
                    Some("gedit.desktop"),
                ),
                expected_reauth: true,
            },
            Case {
                name: "desktop_id_only_changed_same_executable",
                old: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a",
                    1234,
                    555,
                    Some("old.desktop"),
                ),
                new: sample_identity(
                    "/usr/bin/gedit",
                    1,
                    100,
                    "hash_a",
                    1234,
                    555,
                    Some("new.desktop"),
                ),
                expected_reauth: false,
            },
            Case {
                name: "symlink_wrapper_resolved_to_different_target",
                old: sample_identity(
                    "/usr/lib/gedit/gedit-bin",
                    1,
                    100,
                    "hash_a",
                    1234,
                    555,
                    None,
                ),
                new: sample_identity(
                    "/usr/lib/gedit/gedit-wrapped",
                    1,
                    102,
                    "hash_b",
                    1234,
                    555,
                    None,
                ),
                expected_reauth: true,
            },
        ];

        for case in cases {
            let actual = needs_reauth(&case.old, &case.new);
            assert_eq!(
                actual, case.expected_reauth,
                "case '{}' failed: expected reauth={}, got={}",
                case.name, case.expected_reauth, actual
            );
        }
    }
}
