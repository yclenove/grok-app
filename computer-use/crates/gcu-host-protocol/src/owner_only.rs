//! Owner-only files and directories for Computer Use persistent state.
//!
//! Unix directories are `0700` and files are `0600`. `fchmod` errors refuse the
//! path. Windows sets a protected DACL for the current user SID and SYSTEM and
//! for nobody else. Symlinks, reparse points, and extra hard links fail closed.

use std::io;
#[cfg(not(unix))]
use std::io::Write;
use std::path::Path;
#[cfg(not(unix))]
use std::sync::{Mutex, OnceLock};

#[cfg(unix)]
use crate::owner_unix_imp as imp;
#[cfg(windows)]
use crate::owner_windows_imp as imp;

/// Whether [`atomic_write`] treats `sync_all` failure as fatal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncPolicy {
    /// A sync failure deletes the temporary file and returns the error.
    Required,
    /// A sync failure is warned once and the rename still proceeds.
    BestEffort,
}

/// Held exclusive lock for a private directory. Drop releases it.
pub struct OwnerLock {
    _inner: imp::DirLock,
}

/// Create or tighten `path` as an owner-only directory.
///
/// Missing ancestors are created with the process umask. Only `path` itself is
/// restricted, so a shared parent such as the system temp directory is left
/// unchanged. Blocking: directory creation and permission update.
pub fn ensure_dir(path: &Path) -> io::Result<()> {
    imp::ensure_dir(path)
}

/// Tighten an existing file or directory. A symlink is refused and not followed.
///
/// Blocking: one permission update. Failure means the caller must not use `path`.
pub fn restrict(path: &Path) -> io::Result<()> {
    imp::restrict(path)
}

/// Create a new owner-only file with `O_EXCL` / `create_new`. The returned file
/// is the descriptor that was checked.
///
/// Blocking: create plus permission update. On failure the new path is removed.
pub fn create_new(path: &Path) -> io::Result<std::fs::File> {
    imp::create_new(path)
}

/// Replace `path` using a unique temporary name, `create_new`, and rename.
///
/// An existing symlink is left in place and returns an error. Blocking: the
/// write and rename. A failed sync follows `sync`.
pub fn atomic_write(path: &Path, bytes: &[u8], sync: SyncPolicy) -> io::Result<()> {
    imp::atomic_write(path, bytes, matches!(sync, SyncPolicy::Required))
}

/// Read at most `max` bytes from a regular owner-only file without following links.
///
/// Unix opens with `O_NOFOLLOW` and checks owner, mode, and `nlink == 1` on that
/// descriptor. Windows rejects reparse points and extra links, then checks the DACL.
/// Blocking: one read. Over-long files return [`io::ErrorKind::InvalidData`].
pub fn read_no_follow(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    imp::read_no_follow(path, max)
}

/// Confirm `path` is an owner-only directory and not a link.
///
/// Blocking: one metadata check. Timeout: none.
pub fn verify_dir(path: &Path) -> io::Result<()> {
    imp::verify_dir(path)
}

/// Hold an exclusive lock inside `dir` so a later process can see it is live.
///
/// Blocking until the lock is acquired. The lock file is owner-only.
pub fn hold_dir_lock(dir: &Path) -> io::Result<OwnerLock> {
    Ok(OwnerLock {
        _inner: imp::hold_dir_lock(dir)?,
    })
}

/// Delete unlocked child directories named with `prefix`, and leftover `.gcu-tmp-` names.
///
/// `keep` is never removed. A lock that is already held means the directory is live.
/// Delete failures are warned once and left for the next call. Symlinks are not followed.
/// Blocking: a directory scan. Timeout: none.
pub fn reap_unlocked_children(parent: &Path, prefix: &str, keep: &Path) -> io::Result<()> {
    imp::reap_unlocked_children(parent, prefix, keep, ORPHAN_GRACE)
}

/// A directory touched more recently than this is never reaped. A new session
/// creates its directory before it can take the lock, so without a grace period
/// a concurrent scan (another process, or another session in this one) could
/// delete a live directory in that window. A genuine orphan is older than this
/// by the time of the next start.
pub(crate) const ORPHAN_GRACE: std::time::Duration = std::time::Duration::from_secs(60);

/// Metadata warning, once per category and path. The OS error kind is included.
/// File bytes are not.
pub fn warn_meta(category: &str, path: &Path, error: &io::Error) {
    #[cfg(unix)]
    {
        imp::warn_meta(category, path, error);
    }
    #[cfg(not(unix))]
    warn_here(category, path, error);
}

/// Whether [`warn_meta`] has already recorded this category and path.
#[cfg(test)]
pub fn warned_path(category: &str, path: &Path) -> bool {
    #[cfg(unix)]
    {
        imp::warned_path(category, path)
    }
    #[cfg(not(unix))]
    {
        let key = format!("{category}:{}", path.display());
        warned_set()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&key)
    }
}

#[cfg(not(unix))]
fn warn_here(category: &str, path: &Path, error: &io::Error) {
    let key = format!("{category}:{}", path.display());
    let mut set = warned_set()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !set.insert(key) {
        return;
    }
    let _ = writeln!(
        io::stderr(),
        "WARN category={category} location={} kind={:?}",
        path.display(),
        error.kind()
    );
}

#[cfg(not(unix))]
fn warned_set() -> &'static Mutex<std::collections::HashSet<String>> {
    static SET: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// Protected DACL whose allow entries are only `user_sid`, `OW`, and optionally SYSTEM.
///
/// Inherited ACEs and any other trustee are rejected. `AI`/`AR` flags may appear
/// beside the required protected (`P`) flag.
#[must_use]
pub fn dacl_allows_only_user_and_system(sddl: &str, user_sid: &str) -> bool {
    let Some(rest) = sddl.strip_prefix("D:") else {
        return false;
    };
    let Some(open) = rest.find('(') else {
        return false;
    };
    let flags = &rest[..open];
    if !flags.contains('P') {
        return false;
    }
    let mut remaining = flags;
    while !remaining.is_empty() {
        if let Some(next) = remaining
            .strip_prefix("AI")
            .or_else(|| remaining.strip_prefix("AR"))
            .or_else(|| remaining.strip_prefix('P'))
        {
            remaining = next;
        } else {
            return false;
        }
    }
    let mut saw_owner = false;
    let mut rest_aces = &rest[open..];
    if rest_aces.is_empty() {
        return false;
    }
    while !rest_aces.is_empty() {
        let Some(end) = rest_aces.find(')') else {
            return false;
        };
        let ace = &rest_aces[..=end];
        rest_aces = &rest_aces[end + 1..];
        let Some(body) = ace
            .strip_prefix('(')
            .and_then(|value| value.strip_suffix(')'))
        else {
            return false;
        };
        let mut parts = body.split(';');
        let Some(kind) = parts.next() else {
            return false;
        };
        let Some(ace_flags) = parts.next() else {
            return false;
        };
        let Some(rights) = parts.next() else {
            return false;
        };
        let Some(_object) = parts.next() else {
            return false;
        };
        let Some(_inherit) = parts.next() else {
            return false;
        };
        let Some(sid) = parts.next() else {
            return false;
        };
        // `OICI` is the inheritance a directory carries so that its existing and
        // future children are owner-only too. `ID` (inherited from elsewhere) is not allowed.
        let inherit_ok = matches!(ace_flags, "" | "OICI" | "CIOI");
        if parts.next().is_some() || kind != "A" || !inherit_ok {
            return false;
        }
        if !rights.eq_ignore_ascii_case("FA") && !rights.eq_ignore_ascii_case("0x1F01FF") {
            return false;
        }
        if is_user(sid, user_sid) || sid.eq_ignore_ascii_case("OW") {
            saw_owner = true;
        } else if !is_system(sid) {
            return false;
        }
    }
    saw_owner
}

/// Whether trustee `sid` (as SDDL prints it) is the caller. SDDL prints the
/// built-in Administrator account (RID 500, the account GitHub's `runneradmin`
/// uses) with the alias `LA` instead of its full SID, so that alias matches only
/// when the caller itself is a `S-1-5-21-…-500` SID.
fn is_user(sid: &str, user_sid: &str) -> bool {
    sid.eq_ignore_ascii_case(user_sid)
        || (sid.eq_ignore_ascii_case("LA")
            && user_sid.starts_with("S-1-5-21-")
            && user_sid.ends_with("-500"))
}

fn is_system(sid: &str) -> bool {
    sid.eq_ignore_ascii_case("SY") || sid.eq_ignore_ascii_case("S-1-5-18")
}

/// Read the DACL back as SDDL. Windows only.
#[cfg(windows)]
pub fn security_sddl(path: &Path) -> io::Result<String> {
    imp::security_sddl(path)
}

/// Current user SID string. Windows only.
#[cfg(windows)]
pub fn current_user_sid() -> io::Result<String> {
    imp::current_user_sid()
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use super::*;
    use std::fs::File;

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "owner-only files are not implemented on this platform",
        )
    }

    pub(crate) struct DirLock;

    pub(crate) fn ensure_dir(_: &Path) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn restrict(_: &Path) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn create_new(_: &Path) -> io::Result<File> {
        Err(unsupported())
    }
    pub(crate) fn atomic_write(_: &Path, _: &[u8], _: bool) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn read_no_follow(_: &Path, _: u64) -> io::Result<Vec<u8>> {
        Err(unsupported())
    }
    pub(crate) fn verify_dir(_: &Path) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn hold_dir_lock(_: &Path) -> io::Result<DirLock> {
        Err(unsupported())
    }
    pub(crate) fn reap_unlocked_children(
        _: &Path,
        _: &str,
        _: &Path,
        _: std::time::Duration,
    ) -> io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::io::Write;

    fn scratch() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gcu-owner-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn dacl_string_allows_only_the_user_and_optional_system() {
        let sid = "S-1-5-21-1-2-3-1001";
        assert!(dacl_allows_only_user_and_system(
            &format!("D:P(A;;FA;;;{sid})(A;;FA;;;SY)"),
            sid
        ));
        assert!(dacl_allows_only_user_and_system("D:PAI(A;;FA;;;OW)", sid));
        assert!(dacl_allows_only_user_and_system(
            &format!("D:P(A;;0x1F01FF;;;{sid})(A;;FA;;;S-1-5-18)"),
            sid
        ));
        assert!(!dacl_allows_only_user_and_system(
            &format!("D:P(A;;FA;;;{sid})(A;;FA;;;WD)"),
            sid
        ));
        // SDDL prints the RID-500 account as LA; it is the caller only when the caller is RID 500.
        let admin = "S-1-5-21-1-2-3-500";
        assert!(dacl_allows_only_user_and_system(
            "D:PAI(A;;FA;;;LA)(A;;FA;;;SY)",
            admin
        ));
        assert!(!dacl_allows_only_user_and_system(
            "D:PAI(A;;FA;;;LA)(A;;FA;;;SY)",
            sid
        ));
        assert!(!dacl_allows_only_user_and_system(
            "D:PAI(A;;FA;;;LG)(A;;FA;;;SY)",
            "S-1-5-21-1-2-3-501"
        ));
        assert!(dacl_allows_only_user_and_system(
            &format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"),
            sid
        ));
        assert!(!dacl_allows_only_user_and_system(
            &format!("D:P(A;OICIID;FA;;;{sid})(A;;FA;;;SY)"),
            sid
        ));
        assert!(!dacl_allows_only_user_and_system("D:(A;;FA;;;OW)", sid));
        assert!(!dacl_allows_only_user_and_system("D:P(A;ID;FA;;;OW)", sid));
        assert!(!dacl_allows_only_user_and_system("D:P(A;;FR;;;OW)", sid));
        assert!(!dacl_allows_only_user_and_system(
            &format!("D:P(A;;FA;;;{sid})(A;;FA;;;SY)extra"),
            sid
        ));
    }

    #[cfg(unix)]
    #[test]
    fn directory_and_file_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        ensure_dir(&dir).unwrap();
        let nested = dir.join("nested");
        ensure_dir(&nested).unwrap();
        let file = nested.join("secret");
        let mut created = create_new(&file).unwrap();
        created.write_all(b"ok").unwrap();
        drop(created);
        let dir_mode = fs::metadata(&nested).unwrap().permissions().mode() & 0o777;
        let file_mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        assert_eq!(read_no_follow(&file, 8).unwrap(), b"ok");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn chmod_failure_refuses_the_path() {
        let uid = unsafe { libc::geteuid() };
        if uid == 0 {
            return;
        }
        let path = std::path::Path::new("/etc/passwd");
        let before = fs::symlink_metadata(path).unwrap();
        let error = restrict(path).unwrap_err();
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
        let after = fs::symlink_metadata(path).unwrap();
        assert_eq!(before.modified().ok(), after.modified().ok());
        assert_eq!(before.len(), after.len());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_hardlink_replacement_are_refused() {
        use std::os::unix::fs::symlink;
        let dir = scratch();
        ensure_dir(&dir).unwrap();
        let file = dir.join("data");
        atomic_write(&file, b"keep", SyncPolicy::Required).unwrap();
        let link = dir.join("link");
        symlink(&file, &link).unwrap();
        assert!(read_no_follow(&link, 16).is_err());
        assert!(atomic_write(&link, b"new", SyncPolicy::Required).is_err());
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&file).unwrap(), b"keep");
        let extra = dir.join("extra");
        fs::hard_link(&file, &extra).unwrap();
        assert!(read_no_follow(&file, 16).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_uses_a_unique_temp_and_leaves_no_fixed_name() {
        let dir = scratch();
        ensure_dir(&dir).unwrap();
        let path = dir.join("policy.json");
        atomic_write(&path, b"{\"v\":1}", SyncPolicy::Required).unwrap();
        assert!(!dir.join("policy.json.tmp").exists());
        assert_eq!(fs::read(&path).unwrap(), b"{\"v\":1}");
        for entry in fs::read_dir(&dir).unwrap() {
            let name = entry.unwrap().file_name();
            let text = name.to_string_lossy();
            assert!(!text.starts_with(".gcu-tmp-"), "{text}");
            assert!(!text.ends_with(".tmp"), "{text}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn orphan_session_directory_without_a_live_lock_is_removed() {
        let parent = scratch();
        let orphan = parent.join("gcu-session-dead");
        fs::create_dir(&orphan).unwrap();
        fs::write(orphan.join("shot.png"), b"png").unwrap();
        let live = parent.join("gcu-session-live");
        fs::create_dir(&live).unwrap();
        let lock = hold_dir_lock(&live).unwrap();
        // A just-created directory is inside the grace period and survives.
        reap_unlocked_children(&parent, "gcu-session-", &live).unwrap();
        assert!(orphan.exists(), "a fresh directory is not reaped");
        imp::reap_unlocked_children(&parent, "gcu-session-", &live, std::time::Duration::ZERO)
            .unwrap();
        assert!(!orphan.exists());
        assert!(live.is_dir());
        drop(lock);
        let _ = fs::remove_dir_all(&parent);
    }

    #[cfg(windows)]
    #[test]
    fn tightening_a_directory_keeps_existing_children_readable() {
        // Regression: a protected, non-inheritable directory DACL made Windows strip
        // the inherited ACEs of files already inside, leaving them unreadable.
        let dir = scratch();
        let old = dir.join("app.log.old");
        fs::write(&old, b"before").unwrap();
        ensure_dir(&dir).unwrap();
        assert_eq!(fs::read(&old).unwrap(), b"before");
        fs::remove_file(&old).unwrap();
        let fresh = dir.join("app.log.new");
        fs::write(&fresh, b"after").unwrap();
        assert_eq!(fs::read(&fresh).unwrap(), b"after");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn created_file_dacl_is_user_and_system_only() {
        let dir = scratch();
        ensure_dir(&dir).unwrap();
        let file = dir.join("secret");
        atomic_write(&file, b"x", SyncPolicy::Required).unwrap();
        let sddl = security_sddl(&file).unwrap();
        let sid = current_user_sid().unwrap();
        assert!(dacl_allows_only_user_and_system(&sddl, &sid), "{sddl}");
        let _ = fs::remove_dir_all(&dir);
    }
}
