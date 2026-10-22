//! A fixed-location, create-new diagnostic sink. Never rotates or discards data.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub(super) const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 16 * 1024;
pub(super) const MAX_DIRECTORY_ENTRIES: usize = 4096;

pub(super) struct TelemetryWriter {
    file: File,
    _budget_lock: File,
    directories: Vec<Directory>,
    path: PathBuf,
    written: u64,
    failed: bool,
}

struct Directory {
    path: PathBuf,
    file: File,
}

pub(super) fn error(_: impl std::fmt::Display) -> String {
    // An OS error can contain private paths. The IPC contract is code-only.
    "telemetry_io_failed".into()
}

fn reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

impl Directory {
    fn open(path: &Path) -> Result<Self, String> {
        let meta = fs::symlink_metadata(path).map_err(error)?;
        if !meta.is_dir() || reparse(&meta) {
            return Err("telemetry_unsafe_directory".into());
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Pin every ancestor against rename/deletion for this sink's life;
            // open reparse points themselves rather than following them.
            options
                .share_mode(0x1 | 0x2)
                .custom_flags(0x02000000 | 0x00200000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW);
        }
        let file = options.open(path).map_err(error)?;
        let opened = file.metadata().map_err(error)?;
        if !opened.is_dir() || reparse(&opened) {
            return Err("telemetry_unsafe_directory".into());
        }
        Ok(Self {
            path: path.to_owned(),
            file,
        })
    }

    fn validate(&self) -> Result<(), String> {
        let named = fs::symlink_metadata(&self.path).map_err(error)?;
        let opened = self.file.metadata().map_err(error)?;
        if !named.is_dir() || reparse(&named) || !same_identity(&named, &opened) {
            return Err("telemetry_directory_changed".into());
        }
        Ok(())
    }

    fn open_file(&self, path: &Path, create_new: bool) -> Result<File, String> {
        #[cfg(unix)]
        {
            super::unix_io::open_file(
                &self.file,
                path.file_name().ok_or("telemetry_invalid_path")?,
                create_new,
            )
        }
        #[cfg(not(unix))]
        {
            let mut options = OpenOptions::new();
            options
                .read(true)
                .write(true)
                .create(true)
                .create_new(create_new)
                .append(create_new);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // Readers may sample; no other writer/rename may mutate it.
                options
                    .share_mode(if create_new { 0x1 } else { 0x1 | 0x2 })
                    .custom_flags(0x00200000);
            }
            options.open(path).map_err(error)
        }
    }

    fn create_child(&self, name: &str) -> Result<Self, String> {
        let path = self.path.join(name);
        #[cfg(unix)]
        {
            let file = super::unix_io::create_directory(&self.file, std::ffi::OsStr::new(name))?;
            Ok(Self { path, file })
        }
        #[cfg(not(unix))]
        {
            match fs::create_dir(&path) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(io_error) => return Err(error(io_error)),
            }
            Self::open(&path)
        }
    }

    fn bytes(&self) -> Result<u64, String> {
        #[cfg(unix)]
        {
            super::unix_io::directory_bytes(&self.file)
        }
        #[cfg(not(unix))]
        {
            directory_bytes(&self.path)
        }
    }
}

fn same_identity(named: &fs::Metadata, opened: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        named.ino() == opened.ino() && named.dev() == opened.dev()
    }
    #[cfg(not(unix))]
    {
        // Windows pins each directory/file without FILE_SHARE_DELETE. A
        // replacement cannot pass while the original handles remain open.
        named.is_dir() == opened.is_dir() && named.is_file() == opened.is_file()
    }
}

pub(super) fn own_filename(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.strip_prefix("native-soak-")
                .and_then(|name| name.strip_suffix(".jsonl"))
                .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

#[cfg(not(unix))]
fn directory_bytes(path: &Path) -> Result<u64, String> {
    let mut bytes = 0u64;
    for (index, entry) in fs::read_dir(path).map_err(error)?.enumerate() {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err("telemetry_directory_limit".into());
        }
        let path = entry.map_err(error)?.path();
        if !own_filename(&path) {
            continue;
        }
        let meta = fs::symlink_metadata(path).map_err(error)?;
        if !meta.is_file() || reparse(&meta) {
            return Err("telemetry_unsafe_file".into());
        }
        bytes = bytes.checked_add(meta.len()).ok_or("telemetry_disk_cap")?;
    }
    Ok(bytes)
}

impl TelemetryWriter {
    pub(super) fn open(home: &Path, pid: u32) -> Result<Self, String> {
        if !home.is_absolute()
            || home.parent().is_none()
            || home
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err("telemetry_invalid_home".into());
        }
        Self::open_pinned(home, pid, pinned_home_directories(home)?)
    }

    fn open_pinned(home: &Path, pid: u32, mut directories: Vec<Directory>) -> Result<Self, String> {
        let diagnostics = directories
            .last()
            .ok_or("telemetry_missing_directory")?
            .create_child("diagnostics")?;
        directories.push(diagnostics);
        for directory in &directories {
            directory.validate()?;
        }
        let directory = home.join("diagnostics");
        let leaf = directories.last().ok_or("telemetry_missing_directory")?;
        // A zero-byte diagnostic-only lock makes the directory budget atomic
        // across app processes. It never waits or touches an import/store lock.
        let budget_lock = leaf.open_file(&directory.join("native-soak-budget.lock"), false)?;
        let lock_meta = budget_lock.metadata().map_err(error)?;
        if !lock_meta.is_file() || reparse(&lock_meta) || lock_meta.len() != 0 {
            return Err("telemetry_unsafe_budget_lock".into());
        }
        fs2::FileExt::try_lock_exclusive(&budget_lock).map_err(|_| "telemetry_home_busy")?;
        if leaf.bytes()? >= MAX_BYTES {
            return Err("telemetry_disk_cap".into());
        }
        let path = directory.join(format!("native-soak-{pid}.jsonl"));
        let file = leaf.open_file(&path, true)?;
        if !file.metadata().map_err(error)?.is_file() {
            return Err("telemetry_unsafe_file".into());
        }
        Ok(Self {
            file,
            _budget_lock: budget_lock,
            directories,
            path,
            written: 0,
            failed: false,
        })
    }

    pub(super) fn append(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.failed {
            return Err("telemetry_sink_failed".into());
        }
        let result = self.append_checked(bytes);
        // In particular, never append after a potentially partial write.
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn append_checked(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > MAX_LINE_BYTES || bytes.last() != Some(&b'\n') {
            return Err("telemetry_line_limit".into());
        }
        for directory in &self.directories {
            directory.validate()?;
        }
        let opened = self.file.metadata().map_err(error)?;
        let leaf = self
            .directories
            .last()
            .ok_or("telemetry_missing_directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let named = super::unix_io::metadata(
                &leaf.file,
                self.path.file_name().ok_or("telemetry_invalid_path")?,
            )?;
            if super::unix_io::regular_size(&named)? != self.written
                || named.st_dev as u64 != opened.dev()
                || named.st_ino as u64 != opened.ino()
                || opened.len() != self.written
            {
                return Err("telemetry_file_changed".into());
            }
        }
        #[cfg(not(unix))]
        {
            let named = fs::symlink_metadata(&self.path).map_err(error)?;
            if !named.is_file()
                || reparse(&named)
                || !same_identity(&named, &opened)
                || named.len() != self.written
                || opened.len() != self.written
            {
                return Err("telemetry_file_changed".into());
            }
        }
        let byte_count = bytes.len() as u64;
        if leaf.bytes()?.saturating_add(byte_count) > MAX_BYTES {
            return Err("telemetry_disk_cap".into());
        }
        // This is diagnostic I/O only, not a production import durability
        // boundary. No fsync or lock policy is changed by instrumentation.
        self.file.write_all(bytes).map_err(error)?;
        self.file.flush().map_err(error)?;
        self.written += byte_count;
        Ok(())
    }
}

fn pinned_home_directories(home: &Path) -> Result<Vec<Directory>, String> {
    let mut directories: Vec<Directory> = Vec::new();
    for ancestor in home.ancestors().collect::<Vec<_>>().into_iter().rev() {
        #[cfg(unix)]
        let directory = if let Some(parent) = directories.last() {
            let name = ancestor.file_name().ok_or("telemetry_invalid_path")?;
            Directory {
                path: ancestor.to_owned(),
                file: super::unix_io::open_directory(&parent.file, name)?,
            }
        } else {
            Directory::open(ancestor)?
        };
        #[cfg(not(unix))]
        let directory = Directory::open(ancestor)?;
        directories.push(directory);
    }
    Ok(directories)
}

#[cfg(all(test, unix))]
#[path = "writer_unix_tests.rs"]
mod unix_tests;
