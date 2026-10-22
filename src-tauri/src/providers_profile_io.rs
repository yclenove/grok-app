//! Bounded, private filesystem snapshots for new main execution homes.
//! No paths, bytes or identity stamps from this module enter logs or UI.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

fn unavailable() -> String {
    "execution profile input changed or is unavailable".into()
}

pub(super) fn open_node(path: &Path, directory: bool) -> Result<File, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| unavailable())?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(unavailable());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | if directory { libc::O_DIRECTORY } else { 0 });
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0);
    }
    let file = options.open(path).map_err(|_| unavailable())?;
    let actual = file.metadata().map_err(|_| unavailable())?;
    if actual.is_dir() != directory || (!directory && !actual.is_file()) {
        return Err(unavailable());
    }
    node_id(&file)?;
    Ok(file)
}

#[cfg(windows)]
pub(super) fn node_id(file: &File) -> Result<[u64; 3], String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        },
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: file owns a live handle and info is an initialized output struct.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
        .map_err(|_| unavailable())?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(unavailable());
    }
    Ok([
        info.dwVolumeSerialNumber as u64,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        (u64::from(info.ftCreationTime.dwHighDateTime) << 32)
            | u64::from(info.ftCreationTime.dwLowDateTime),
    ])
}

#[cfg(unix)]
pub(super) fn node_id(file: &File) -> Result<[u64; 3], String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(|_| unavailable())?;
    Ok([metadata.dev(), metadata.ino(), 0])
}

#[cfg(not(any(unix, windows)))]
pub(super) fn node_id(_file: &File) -> Result<[u64; 3], String> {
    Err("execution profiles are unsupported on this platform".into())
}

#[derive(PartialEq, Eq)]
enum Entry {
    Directory([u64; 3]),
    File {
        identity: [u64; 3],
        bytes: Vec<u8>,
        mode: u32,
        modified: Option<std::time::SystemTime>,
    },
}

#[derive(PartialEq, Eq)]
pub(super) struct Tree {
    root: PathBuf,
    identity: Option<[u64; 3]>,
    entries: BTreeMap<PathBuf, Entry>,
    resources: bool,
}

#[derive(PartialEq, Eq)]
enum SealedEntry {
    Directory([u64; 3]),
    File {
        identity: [u64; 3],
        digest: [u8; 32],
        mode: u32,
        modified: Option<std::time::SystemTime>,
    },
}

/// Private, compact verification data. Do not retain media bytes for a live
/// process, or expose their fingerprints through serialized receipts or logs.
#[derive(PartialEq, Eq)]
pub(super) struct TreeSeal {
    root: PathBuf,
    identity: Option<[u64; 3]>,
    entries: BTreeMap<PathBuf, SealedEntry>,
    resources: bool,
}

impl TreeSeal {
    pub(super) fn unchanged(&self) -> Result<(), String> {
        if Tree::capture_tree(&self.root, self.resources)?.seal() != *self {
            return Err(unavailable());
        }
        Ok(())
    }
}

fn file_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    }
    #[cfg(not(unix))]
    {
        u32::from(metadata.permissions().readonly())
    }
}

fn root_omitted(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    matches!(
        name.as_ref(),
        "sessions"
            | "logs"
            | "bin"
            | "cache"
            | "dashboard"
            | "auth.json"
            | "config.toml"
            | "execution-profile.json"
            | "worktrees"
            | "worktree_pool"
    ) || name.ends_with(".lock")
        || name.ends_with(".sock")
        || name.ends_with(".tmp")
        || name.starts_with("auth.json.")
        || name.starts_with(".execution-profile-")
}

impl Tree {
    pub(super) fn seal(self) -> TreeSeal {
        TreeSeal {
            root: self.root,
            identity: self.identity,
            resources: self.resources,
            entries: self
                .entries
                .into_iter()
                .map(|(path, entry)| {
                    let entry = match entry {
                        Entry::Directory(id) => SealedEntry::Directory(id),
                        Entry::File {
                            identity,
                            bytes,
                            mode,
                            modified,
                        } => SealedEntry::File {
                            identity,
                            digest: Sha256::digest(bytes).into(),
                            mode,
                            modified,
                        },
                    };
                    (path, entry)
                })
                .collect(),
        }
    }

    pub(super) fn capture(root: &Path) -> Result<Self, String> {
        Self::capture_tree(root, true)
    }

    /// Selected agent histories retain every ordinary file, including names
    /// excluded from the runtime resource tree. Never enumerate sibling agents.
    pub(super) fn capture_history(root: &Path) -> Result<Self, String> {
        let tree = Self::capture_tree(root, false)?;
        if tree.identity.is_none() {
            return Err(unavailable());
        }
        Ok(tree)
    }

    fn capture_tree(root: &Path, resources: bool) -> Result<Self, String> {
        let mut tree = Self {
            root: root.to_owned(),
            identity: None,
            entries: BTreeMap::new(),
            resources,
        };
        match fs::symlink_metadata(root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(tree),
            Err(_) => return Err(unavailable()),
            Ok(_) => {}
        }
        let file = open_node(root, true)?;
        let canonical = root.canonicalize().map_err(|_| unavailable())?;
        tree.identity = Some(node_id(&file)?);
        let mut total = 0;
        tree.walk(root, &canonical, Path::new(""), 0, &mut total)?;
        if node_id(&open_node(root, true)?)? != tree.identity.unwrap() {
            return Err(unavailable());
        }
        Ok(tree)
    }

    fn walk(
        &mut self,
        path: &Path,
        canonical: &Path,
        relative: &Path,
        depth: usize,
        total: &mut usize,
    ) -> Result<(), String> {
        if depth >= 64 || self.entries.len() >= 16_384 {
            return Err("execution profile resource limit exceeded".into());
        }
        for entry in fs::read_dir(path).map_err(|_| unavailable())? {
            let entry = entry.map_err(|_| unavailable())?;
            if self.resources && relative.as_os_str().is_empty() && root_omitted(&entry.file_name())
            {
                continue;
            }
            let next_relative = relative.join(entry.file_name());
            let path = entry.path();
            if path.canonicalize().map_err(|_| unavailable())? != canonical.join(&next_relative) {
                return Err(unavailable());
            }
            let metadata = fs::symlink_metadata(&path).map_err(|_| unavailable())?;
            let mut file = open_node(&path, metadata.is_dir())?;
            let identity = node_id(&file)?;
            if metadata.is_dir() {
                self.entries
                    .insert(next_relative.clone(), Entry::Directory(identity));
                self.walk(&path, canonical, &next_relative, depth + 1, total)?;
            } else {
                let modified = file.metadata().map_err(|_| unavailable())?.modified().ok();
                let mut bytes = Vec::new();
                Read::by_ref(&mut file)
                    .take(64 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| unavailable())?;
                *total = total.checked_add(bytes.len()).ok_or_else(unavailable)?;
                if bytes.len() > 64 * 1024 * 1024 || *total > 256 * 1024 * 1024 {
                    return Err("execution profile resource limit exceeded".into());
                }
                self.entries.insert(
                    next_relative,
                    Entry::File {
                        identity,
                        bytes,
                        mode: file_mode(&file.metadata().map_err(|_| unavailable())?),
                        modified,
                    },
                );
            }
            if self.entries.len() > 16_384
                || node_id(&open_node(&path, metadata.is_dir())?)? != identity
            {
                return Err(unavailable());
            }
        }
        Ok(())
    }

    pub(super) fn unchanged(&self) -> Result<(), String> {
        if Self::capture_tree(&self.root, self.resources)? != *self {
            return Err(unavailable());
        }
        Ok(())
    }

    pub(super) fn same_contents(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self
                .entries
                .iter()
                .all(|(path, entry)| match (entry, other.entries.get(path)) {
                    (Entry::Directory(_), Some(Entry::Directory(_))) => true,
                    (
                        Entry::File { bytes, mode, .. },
                        Some(Entry::File {
                            bytes: actual,
                            mode: actual_mode,
                            ..
                        }),
                    ) => bytes == actual && mode & 0o777 == actual_mode & 0o777,
                    _ => false,
                })
    }

    pub(super) fn copy_into(&self, home: &Path) -> Result<(), String> {
        for (relative, entry) in &self.entries {
            let path = home.join(relative);
            match entry {
                Entry::Directory(_) => fs::create_dir(&path).map_err(|_| unavailable())?,
                Entry::File { bytes, mode, .. } => {
                    write_new(&path, bytes)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(path, fs::Permissions::from_mode(*mode & 0o777))
                            .map_err(|_| unavailable())?;
                    }
                    #[cfg(not(unix))]
                    if *mode != 0 {
                        let mut permissions = fs::metadata(&path)
                            .map_err(|_| unavailable())?
                            .permissions();
                        permissions.set_readonly(true);
                        fs::set_permissions(path, permissions).map_err(|_| unavailable())?;
                    }
                }
            }
        }
        Ok(())
    }
}

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    read_optional_bounded(path, 16 * 1024 * 1024)
}

pub(super) fn read_optional_bounded(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let before = input_stamp(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(unavailable()),
        Ok(_) => {}
    }
    let mut file = open_node(path, false)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    if bytes.len() as u64 > limit {
        return Err("execution profile input limit exceeded".into());
    }
    if input_stamp(path)? != before {
        return Err(unavailable());
    }
    Ok(Some(bytes))
}

#[derive(PartialEq, Eq)]
pub(super) struct InputStamp {
    parent: Option<[u64; 3]>,
    file: Option<([u64; 3], u64, Option<std::time::SystemTime>)>,
}

pub(super) fn input_stamp(path: &Path) -> Result<InputStamp, String> {
    let parent = path.parent().ok_or_else(unavailable)?;
    let parent = match fs::symlink_metadata(parent) {
        Ok(_) => Some(node_id(&open_node(parent, true)?)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(unavailable()),
    };
    let file = match fs::symlink_metadata(path) {
        Ok(_) => {
            let file = open_node(path, false)?;
            let metadata = file.metadata().map_err(|_| unavailable())?;
            Some((node_id(&file)?, metadata.len(), metadata.modified().ok()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(unavailable()),
    };
    Ok(InputStamp { parent, file })
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| unavailable())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| unavailable())
}
