//! Original catalog-file boundary for retention repair, not a reader grant.
//! Mutable size/timestamps are excluded: normal SQLite writes preserve identity.
//! This does not detect arbitrary same-inode in-place byte rollback.
use std::fs::{self, File, OpenOptions};
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::super::super::{catalog, errors::ImportError};
use super::{deferred, receipt};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum Identity {
    Windows { volume: u64, file: u64 },
    Unix { device: u64, inode: u64 },
}

impl Identity {
    pub(super) fn supported(&self) -> bool {
        match self {
            Self::Windows { file, .. } => cfg!(windows) && *file != 0,
            Self::Unix { inode, .. } => cfg!(unix) && *inode != 0,
        }
    }
}

pub(super) struct BoundFile {
    file: File,
    pub identity: Identity,
}

impl BoundFile {
    pub(super) fn open(conn: &Connection) -> Result<Self, ImportError> {
        let path = catalog::catalog_path();
        receipt::checked_path(&path)?;
        let connection_path = conn
            .path()
            .ok_or_else(|| deferred("catalog connection has no file path"))?;
        // win32-longpath keeps a verbatim prefix in sqlite3_db_filename.
        // Compare resolved paths, not those two different spellings; identity
        // and the live VFS/OS handle checks below remain mandatory.
        if fs::canonicalize(connection_path).map_err(deferred)?
            != fs::canonicalize(&path).map_err(deferred)?
        {
            return Err(deferred("connection is not the original catalog path"));
        }
        let file = open_regular(&path)?;
        let identity = identity(&file)?;
        if !identity.supported() {
            return Err(deferred("catalog file identity is unsupported"));
        }
        let bound = Self { file, identity };
        bound.verify(conn, &bound.identity)?;
        Ok(bound)
    }

    pub(super) fn verify(&self, conn: &Connection, expected: &Identity) -> Result<(), ImportError> {
        if !expected.supported()
            || identity(&self.file)? != *expected
            || identity(&open_regular(&catalog::catalog_path())?)? != *expected
        {
            return Err(deferred("recovery belongs to a different catalog file"));
        }
        // SQLite's Unix VFS compares the connection's own fd inode with its
        // pathname. This rejects a stale connection even if the new path was
        // substituted before opening our independent handle. The Windows VFS
        // and our handle both deny delete sharing for the whole operation.
        #[cfg(unix)]
        {
            let mut moved: std::ffi::c_int = 1;
            // SAFETY: conn and main's nul-terminated name are live; the output
            // pointer addresses a correctly sized integer for HAS_MOVED.
            let code = unsafe {
                rusqlite::ffi::sqlite3_file_control(
                    conn.handle(),
                    c"main".as_ptr(),
                    rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
                    (&mut moved as *mut std::ffi::c_int).cast(),
                )
            };
            if code != rusqlite::ffi::SQLITE_OK || moved != 0 {
                return Err(deferred("catalog connection identity is unresolved"));
            }
        }
        #[cfg(not(unix))]
        let _ = conn;
        Ok(())
    }
}

fn open_regular(path: &Path) -> Result<File, ImportError> {
    receipt::checked_path(path)?;
    let metadata = fs::symlink_metadata(path).map_err(deferred)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(deferred("catalog is not a regular file"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    options.open(path).map_err(deferred)
}

#[cfg(windows)]
fn identity(file: &File) -> Result<Identity, ImportError> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: file owns a live handle and info is the correctly sized output.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
        .map_err(deferred)?;
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0) != 0 {
        return Err(deferred(
            "catalog handle crosses a nonregular/reparse boundary",
        ));
    }
    Ok(Identity::Windows {
        volume: info.dwVolumeSerialNumber as u64,
        file: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    })
}

#[cfg(unix)]
fn identity(file: &File) -> Result<Identity, ImportError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(deferred)?;
    if !metadata.is_file() {
        return Err(deferred("catalog handle is not a regular file"));
    }
    Ok(Identity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(any(windows, unix)))]
fn identity(_: &File) -> Result<Identity, ImportError> {
    Err(deferred("catalog file identity is unsupported on this OS"))
}

#[cfg(test)]
mod preview_lifecycle_tests {
    use super::*;
    use crate::session_migration::test_home::IsolatedAppHome;

    #[test]
    fn physical_identity_survives_normal_catalog_writes_and_vfs_path_spelling() {
        let _home = IsolatedAppHome::acquire("retention-file-normal-writes");
        let conn = catalog::open_catalog().unwrap();
        let bound = BoundFile::open(&conn).unwrap();
        let before = bound.identity.clone();
        let writer = catalog::open_catalog().unwrap();
        writer
            .execute(
                "INSERT INTO meta(key,value) VALUES('synthetic_write',?1)",
                [&"x".repeat(131072)],
            )
            .unwrap();
        bound.verify(&conn, &before).unwrap();
        bound.verify(&writer, &before).unwrap();
    }

    #[test]
    fn same_uuid_copy_is_foreign_but_renamed_original_can_retry() {
        let home = IsolatedAppHome::acquire("retention-file-rename-control");
        let conn = catalog::open_catalog().unwrap();
        let bound = BoundFile::open(&conn).unwrap();
        let expected = bound.identity.clone();
        let epoch = catalog::retention_catalog_epoch(&conn).unwrap();
        let path = catalog::catalog_path();
        let copy = home.path.join("same-uuid-copy.sqlite");
        fs::copy(&path, &copy).unwrap();
        drop(bound);
        drop(conn);
        let original = home.path.join("original.sqlite");
        fs::rename(&path, &original).unwrap();
        fs::rename(&copy, &path).unwrap();
        let replacement = catalog::open_catalog_readonly().unwrap();
        assert_eq!(
            catalog::retention_catalog_epoch(&replacement).unwrap(),
            epoch
        );
        let foreign = BoundFile::open(&replacement).unwrap();
        assert!(foreign.verify(&replacement, &expected).is_err());
        drop(foreign);
        drop(replacement);
        fs::remove_file(&path).unwrap();
        fs::rename(&original, &path).unwrap();
        let restored = catalog::open_catalog_readonly().unwrap();
        BoundFile::open(&restored)
            .unwrap()
            .verify(&restored, &expected)
            .unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn live_catalog_owner_denies_delete_sharing_but_allows_sql_writes() {
        let home = IsolatedAppHome::acquire("retention-file-live-sharing");
        let conn = catalog::open_catalog().unwrap();
        let bound = BoundFile::open(&conn).unwrap();
        assert!(fs::rename(catalog::catalog_path(), home.path.join("moved.sqlite")).is_err());
        conn.execute(
            "INSERT INTO meta(key,value) VALUES('synthetic_sharing','normal')",
            [],
        )
        .unwrap();
        bound.verify(&conn, &bound.identity).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn stale_sqlite_connection_and_symlink_cannot_bind_the_replacement() {
        let home = IsolatedAppHome::acquire("retention-file-stale-connection");
        let conn = catalog::open_catalog().unwrap();
        let path = catalog::catalog_path();
        let copy = home.path.join("same-epoch.sqlite");
        let original = home.path.join("original.sqlite");
        fs::copy(&path, &copy).unwrap();
        fs::rename(&path, &original).unwrap();
        fs::rename(&copy, &path).unwrap();
        assert!(BoundFile::open(&conn).is_err());
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&original, &path).unwrap();
        assert!(BoundFile::open(&conn).is_err());
        fs::remove_file(&path).unwrap();
        fs::rename(&original, &path).unwrap();
        BoundFile::open(&conn).unwrap();
    }
}
