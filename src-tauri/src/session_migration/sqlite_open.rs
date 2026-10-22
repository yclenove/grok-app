//! Import-owned SQLite connections; retain caller access flags and real file locks.
use rusqlite::{Connection, OpenFlags, Result};
use std::path::Path;

pub(super) fn open(path: impl AsRef<Path>) -> Result<Connection> {
    open_with_flags(path, OpenFlags::default())
}

pub(super) fn open_with_flags(path: impl AsRef<Path>, flags: OpenFlags) -> Result<Connection> {
    #[cfg(windows)]
    {
        // Bundled SQLite registers this VFS with the same locking implementation as
        // win32, but a larger pathname buffer. Never use win32-longpath-none.
        Connection::open_with_flags_and_vfs(windows_path(path.as_ref())?, flags, "win32-longpath")
    }
    #[cfg(not(windows))]
    {
        Connection::open_with_flags(path, flags)
    }
}

/// A larger SQLite buffer alone does not opt CreateFileW into extended paths.
/// Resolve ordinary paths lexically before adding the Win32 verbatim prefix:
/// no filesystem canonicalization, directory creation, or source mutation.
#[cfg(windows)]
fn windows_path(path: &Path) -> Result<std::path::PathBuf> {
    use std::ffi::OsString;
    use std::path::{Component, PathBuf, Prefix};
    // Preserve SQLite special filenames/URI semantics; domain callers use files.
    if path.as_os_str().is_empty()
        || path == Path::new(":memory:")
        || path
            .to_str()
            .is_some_and(|value| value.starts_with("file:"))
    {
        return Ok(path.to_path_buf());
    }
    let absolute =
        std::path::absolute(path).map_err(|_| rusqlite::Error::InvalidPath(path.to_path_buf()))?;
    let mut components = absolute.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) => {
                let mut extended = OsString::from(r"\\?\");
                extended.push(absolute.as_os_str());
                Ok(PathBuf::from(extended))
            }
            Prefix::UNC(server, share) => {
                let mut extended = OsString::from(r"\\?\UNC\");
                extended.push(server);
                extended.push(r"\");
                extended.push(share);
                let mut extended = PathBuf::from(extended);
                for component in components {
                    if component != Component::RootDir {
                        extended.push(component.as_os_str());
                    }
                }
                Ok(extended)
            }
            _ => Ok(absolute), // Already verbatim/device-prefixed: never prefix twice.
        },
        _ => Ok(absolute),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_flags_preserve_readonly_no_create_and_normal_writes() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("sql-flags");
        let db = home.path.join("flags.sqlite");
        for flag in [
            OpenFlags::SQLITE_OPEN_READ_ONLY,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        ] {
            assert!(open_with_flags(&db, flag).is_err());
            assert!(!db.exists());
        }
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch("CREATE TABLE t (n INTEGER); INSERT INTO t VALUES (42)")
            .unwrap();
        let reader = open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            reader
                .query_row("SELECT n FROM t", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            42
        );
        assert!(reader.execute_batch("DELETE FROM t").is_err());
        // Cooperate with existing/default VFS users, not just two new connections.
        let other = open(&db).unwrap();
        other.busy_timeout(std::time::Duration::ZERO).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert_eq!(
            other
                .execute_batch("BEGIN IMMEDIATE")
                .unwrap_err()
                .sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy)
        );
        writer.execute_batch("ROLLBACK").unwrap();
        other
            .execute_batch("BEGIN IMMEDIATE; INSERT INTO t VALUES (43); COMMIT;")
            .unwrap();
    }

    #[test]
    fn memory_filename_keeps_sqlite_semantics() {
        let db = open(":memory:").unwrap();
        db.execute_batch("CREATE TABLE t (n INTEGER)").unwrap();
        let name: String = db
            .query_row("PRAGMA database_list", [], |r| r.get(2))
            .unwrap();
        assert!(name.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_conversion_handles_relative_unc_and_verbatim_without_io() {
        use std::path::PathBuf;
        assert_eq!(
            windows_path(Path::new(r"C:\folder\..\data\db.sqlite")).unwrap(),
            PathBuf::from(r"\\?\C:\data\db.sqlite")
        );
        assert_eq!(
            windows_path(Path::new(r"\\server\share\dir\db.sqlite")).unwrap(),
            PathBuf::from(r"\\?\UNC\server\share\dir\db.sqlite")
        );
        for name in [
            r"\\?\C:\dir\db.sqlite",
            r"\\?\UNC\server\share\dir\db.sqlite",
            ":memory:",
            "file:fixture?mode=memory&cache=private",
            "",
        ] {
            assert_eq!(windows_path(Path::new(name)).unwrap(), PathBuf::from(name));
        }
        let relative = windows_path(Path::new("target/../relative.sqlite")).unwrap();
        assert!(relative.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(relative.file_name().unwrap(), "relative.sqlite");
        assert!(!relative
            .components()
            .any(|p| p == std::path::Component::ParentDir));
    }
}
