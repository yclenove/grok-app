//! Embedded, user-only helper publication. No download, shell, enable or consent.
//! Status is read-only; interrupted publication requires an explicit repair.
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, Path, PathBuf};

pub(super) const UUID: &str = "computer-use@grok-app.local";
const MARKER: &str = ".grok-installation.json";
const RESTART: &str = ".grok-restart-owner.json";
const OWNER: &[u8] =
    b"{\"owner\":\"com.grokapp.desktop\",\"uuid\":\"computer-use@grok-app.local\",\"format\":1}\n";
const FILES: &[(&str, &[u8])] = &[
    (
        "metadata.json",
        include_bytes!("../../../../tools/computer-use-gnome-shell/metadata.json"),
    ),
    (
        "extension.js",
        include_bytes!("../../../../tools/computer-use-gnome-shell/extension.js"),
    ),
    (
        "policy.js",
        include_bytes!("../../../../tools/computer-use-gnome-shell/policy.js"),
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Installation {
    Missing,
    Current,
    Modified,
    RecoveryRequired,
    Conflict,
}

pub(super) struct Bundle {
    root: PathBuf,
    current: PathBuf,
    pending: PathBuf,
    previous: PathBuf,
    journal: PathBuf,
}

/// Bus identity plus the pinned Shell owner: unique names alone can be reused
/// after a bus restart. Files on disk never prove which ES modules Shell loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ShellStamp {
    pub bus: String,
    pub owner: String,
    pub process: String,
}
impl ShellStamp {
    fn valid(&self) -> bool {
        self.bus.len() == 32
            && self.bus.bytes().all(|c| c.is_ascii_hexdigit())
            && self.owner.starts_with(':')
            && self.owner.len() <= 255
            && self
                .owner
                .bytes()
                .all(|c| c.is_ascii_digit() || c == b':' || c == b'.')
            && self
                .process
                .split(':')
                .collect::<Vec<_>>()
                .as_slice()
                .first()
                .is_some_and(|boot| uuid::Uuid::parse_str(boot).is_ok())
            && self.process.split(':').count() == 3
            && self
                .process
                .split(':')
                .skip(1)
                .all(|n| n.parse::<u64>().is_ok_and(|n| n > 0))
    }
}

fn error(e: impl std::fmt::Display) -> String {
    format!("computer_use_helper_files: {e}")
}

// Reject ambiguous ancestors rather than following a user's unrelated tree.
// Mutation paths are Host-derived, never supplied by the renderer or model.
pub(super) fn safe_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(error("absolute, non-traversing path required"));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                return Err(error("unsafe installation directory"))
            }
            Ok(m) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let uid = unsafe { libc::geteuid() };
                    if (m.uid() != uid && m.uid() != 0)
                        || (m.mode() & 0o022 != 0 && m.mode() & libc::S_ISVTX == 0)
                    {
                        return Err(error("untrusted writable installation ancestor"));
                    }
                }
                #[cfg(not(unix))]
                let _ = m;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(error(e)),
        }
    }
    Ok(())
}

fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(error(e)),
    }
}

fn regular_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let m = fs::symlink_metadata(path).map_err(error)?;
    if !m.file_type().is_file() || m.len() > 1024 * 1024 {
        return Err(error("unsafe helper file"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let f = options.open(path).map_err(error)?;
    if !f.metadata().map_err(error)?.is_file() {
        return Err(error("not a regular file"));
    }
    let mut bytes = Vec::new();
    f.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() > 1024 * 1024 {
        return Err(error("helper file too large"));
    }
    Ok(bytes)
}

fn owned(path: &Path) -> Result<bool, String> {
    if !exists(path)? {
        return Ok(false);
    }
    safe_path(path)?;
    if regular_bytes(&path.join(MARKER)).ok().as_deref() != Some(OWNER) {
        return Ok(false);
    }
    for item in fs::read_dir(path).map_err(error)? {
        let item = item.map_err(error)?;
        let name = item.file_name();
        if name != MARKER && name != RESTART && !FILES.iter().any(|(n, _)| name == *n) {
            return Ok(false);
        }
        if !item.file_type().map_err(error)?.is_file() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn current(path: &Path) -> Result<bool, String> {
    Ok(owned(path)?
        && stamp(path).is_ok()
        && FILES
            .iter()
            .all(|(n, b)| regular_bytes(&path.join(n)).ok().as_deref() == Some(*b)))
}

fn stamp(path: &Path) -> Result<ShellStamp, String> {
    let stamp: ShellStamp =
        serde_json::from_slice(&regular_bytes(&path.join(RESTART))?).map_err(error)?;
    if !stamp.valid() {
        return Err(error("invalid restart identity"));
    }
    Ok(stamp)
}

// A durable parent journal owns an interrupted, possibly marker-less staging
// directory. It never grants ownership of a foreign current/previous directory.
fn journal_stage(path: &Path) -> Result<bool, String> {
    safe_path(path)?;
    for entry in fs::read_dir(path).map_err(error)? {
        let entry = entry.map_err(error)?;
        let name = entry.file_name();
        if !entry.file_type().map_err(error)?.is_file()
            || (name != MARKER && name != RESTART && !FILES.iter().any(|(n, _)| name == *n))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn sync_dir(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(path).and_then(|f| f.sync_all()).map_err(error)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path).map_err(error)?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(error)
}

impl Bundle {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            current: root.join(UUID),
            pending: root.join(format!(".{UUID}.pending")),
            previous: root.join(format!(".{UUID}.previous")),
            journal: root.join(format!(".{UUID}.transaction")),
            root,
        }
    }
    pub(super) fn path(&self) -> &Path {
        &self.current
    }
    pub(super) fn inspect(&self) -> Result<Installation, String> {
        safe_path(&self.root)?;
        let journal = exists(&self.journal)?;
        if journal && regular_bytes(&self.journal).ok().as_deref() != Some(OWNER) {
            return Ok(Installation::Conflict);
        }
        for p in [&self.current, &self.previous] {
            if exists(p)? && !owned(p)? {
                return Ok(Installation::Conflict);
            }
        }
        if exists(&self.pending)?
            && !(owned(&self.pending)? || journal && journal_stage(&self.pending)?)
        {
            return Ok(Installation::Conflict);
        }
        if journal || exists(&self.pending)? || exists(&self.previous)? {
            return Ok(Installation::RecoveryRequired);
        }
        if !exists(&self.current)? {
            return Ok(Installation::Missing);
        }
        Ok(if current(&self.current)? {
            Installation::Current
        } else {
            Installation::Modified
        })
    }
    /// Caller serializes across processes and first confirms Shell is inactive.
    pub(super) fn restart_required(&self, shell: &ShellStamp) -> bool {
        // A bus reconnect/new unique name in the SAME Shell process does not
        // invalidate GJS's ES-module cache. Require a new process incarnation.
        stamp(&self.current).map_or(true, |previous| previous.process == shell.process)
    }
    pub(super) fn install(&self, repair: bool, shell: &ShellStamp) -> Result<(), String> {
        if !shell.valid() {
            return Err(error("invalid Shell identity"));
        }
        let state = self.inspect()?;
        if state == Installation::Conflict {
            return Err(error("conflicting or unowned extension; no files changed"));
        }
        if !repair && state != Installation::Missing {
            return Err(error(
                "installation already exists; explicit repair required",
            ));
        }
        safe_path(&self.root)?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.root)
            .map_err(error)?;
        safe_path(&self.root)?;
        if !exists(&self.journal)? {
            write_new(&self.journal, OWNER)?;
            sync_dir(&self.root)?;
        }
        // Original transaction owns these fixed slots. Never delete an
        // unrelated or symlinked directory, even during explicit recovery.
        if exists(&self.previous)? {
            if !exists(&self.current)? {
                fs::rename(&self.previous, &self.current).map_err(error)?;
            } else {
                fs::remove_dir_all(&self.previous).map_err(error)?;
            }
            sync_dir(&self.root)?;
        }
        if exists(&self.pending)? {
            fs::remove_dir_all(&self.pending).map_err(error)?;
            sync_dir(&self.root)?;
        }
        // Restrict at creation, not by a later chmod after a permissive umask.
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&self.pending)
            .map_err(error)?;
        write_new(&self.pending.join(MARKER), OWNER)?;
        sync_dir(&self.pending)?;
        sync_dir(&self.root)?;
        for (name, bytes) in FILES {
            write_new(&self.pending.join(name), bytes)?;
        }
        write_new(
            &self.pending.join(RESTART),
            &serde_json::to_vec(shell).map_err(error)?,
        )?;
        sync_dir(&self.pending)?;
        if !current(&self.pending)? {
            return Err(error("staged helper verification failed"));
        }
        if exists(&self.current)? {
            fs::rename(&self.current, &self.previous).map_err(error)?;
            sync_dir(&self.root)?;
        }
        fs::rename(&self.pending, &self.current).map_err(error)?;
        sync_dir(&self.root)?;
        if !current(&self.current)? {
            return Err(error("published helper verification failed"));
        }
        if exists(&self.previous)? {
            fs::remove_dir_all(&self.previous).map_err(error)?;
            sync_dir(&self.root)?;
        }
        fs::remove_file(&self.journal).map_err(error)?;
        sync_dir(&self.root)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "bundle_tests.rs"]
mod tests;
