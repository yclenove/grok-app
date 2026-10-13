//! Dry-run plans. Building a plan reads files and does not write them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::ClientId;

/// Whether the plan adds the `grok-computer-use` entry or removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanAction {
    /// Register `grok-computer-use` and any client hook that belongs with it.
    Install,
    /// Remove only `grok-computer-use` and hooks this installer owns.
    Uninstall,
}

impl PlanAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
        }
    }
}

/// One file a plan will create, replace, or delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// Absolute path.
    pub path: PathBuf,
    /// Bytes observed while planning. `None` if the file did not exist.
    pub before: Option<Vec<u8>>,
    /// Bytes to write. `None` deletes the file.
    pub after: Option<Vec<u8>>,
    /// Unix mode for a rewrite. `None` keeps the existing mode, or `0o644` for a new file.
    pub mode: Option<u32>,
    /// Copy the file aside and let an external CLI be the writer.
    pub backup_only: bool,
}

impl FileChange {
    pub(crate) fn rewrite(
        path: &Path,
        before: Option<Vec<u8>>,
        after: Vec<u8>,
        mode: Option<u32>,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            before,
            after: Some(after),
            mode,
            backup_only: false,
        }
    }

    pub(crate) fn delete(path: &Path, before: Vec<u8>) -> Self {
        Self {
            path: path.to_path_buf(),
            before: Some(before),
            after: None,
            mode: None,
            backup_only: false,
        }
    }

    pub(crate) fn backup_only(path: &Path, before: Vec<u8>) -> Self {
        Self {
            path: path.to_path_buf(),
            before: Some(before.clone()),
            after: Some(before),
            mode: None,
            backup_only: true,
        }
    }

    pub(crate) fn is_rewrite(&self) -> bool {
        !self.backup_only && self.before != self.after
    }
}

/// A client CLI invocation included in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCommand {
    /// Program to spawn.
    pub program: PathBuf,
    /// Arguments, not including the program.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Environment overlay. `HOME` is the injected home for Antigravity.
    pub env: Vec<(String, String)>,
}

/// Read-only description of an install or uninstall.
///
/// [`Self::render_diff`] is the `--dry-run` view. It allocates a string and
/// does not create directories, backups, or processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Client this plan targets.
    pub client: ClientId,
    /// Install or uninstall.
    pub action: PlanAction,
    /// File rewrites, deletes, and backup-only copies.
    pub files: Vec<FileChange>,
    /// Commands to run after backups, in order.
    pub commands: Vec<PlannedCommand>,
}

impl Plan {
    pub(crate) fn new(
        client: ClientId,
        action: PlanAction,
        files: Vec<FileChange>,
        commands: Vec<PlannedCommand>,
    ) -> Self {
        Self {
            client,
            action,
            files,
            commands,
        }
    }

    /// True when apply would not write, delete, or spawn.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.commands.is_empty() && self.files.iter().all(|change| !change.is_rewrite())
    }

    /// Unified diff of file rewrites plus the commands that would run.
    ///
    /// Does not touch the filesystem. Non-UTF-8 files are summarized by length.
    #[must_use]
    pub fn render_diff(&self) -> String {
        if self.is_noop() {
            return format!("# {} {}: no changes\n", self.client, self.action.as_str());
        }
        let mut out = format!("# {} {}\n", self.client, self.action.as_str());
        for change in &self.files {
            if change.backup_only {
                let _ = writeln!(out, "# backup {}", change.path.display());
                continue;
            }
            if !change.is_rewrite() {
                continue;
            }
            push_file_diff(&mut out, change);
        }
        for command in &self.commands {
            let _ = writeln!(
                out,
                "# run: {} {}",
                command.program.display(),
                command.args.join(" "),
            );
            for (key, value) in &command.env {
                let _ = writeln!(out, "# env: {key}={value}");
            }
        }
        out
    }
}

/// What `apply` or `uninstall` did. A noop outcome has an empty backup list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    /// `*.gcu-backup-<stamp>` files written before mutation.
    pub backups: Vec<PathBuf>,
    /// True when the plan had nothing to do. No confirmation prompt runs for a noop.
    pub noop: bool,
}

/// Whether the client's `grok-computer-use` registration is present and points at this `gcu`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientStatus {
    /// The `grok-computer-use` entry or a hook this installer owns exists.
    pub present: bool,
    /// `present` and an install plan for this context would change nothing.
    pub matches_gcu: bool,
    /// Human-readable mismatches. Empty when `matches_gcu` is true.
    pub notes: Vec<String>,
}

fn push_file_diff(out: &mut String, change: &FileChange) {
    let before = change.before.as_deref().unwrap_or(b"");
    let after = change.after.as_deref().unwrap_or(b"");
    let (Ok(before), Ok(after)) = (std::str::from_utf8(before), std::str::from_utf8(after)) else {
        let _ = writeln!(
            out,
            "# {} ({} bytes -> {} bytes, not utf-8)",
            change.path.display(),
            before.len(),
            after.len(),
        );
        return;
    };
    let _ = writeln!(out, "--- {}", change.path.display());
    let _ = writeln!(out, "+++ {}", change.path.display());
    let b_lines = split_lines(before);
    let a_lines = split_lines(after);
    if b_lines.len().saturating_mul(a_lines.len()) > 200_000 {
        let _ = writeln!(
            out,
            "# large change: {} lines -> {} lines",
            b_lines.len(),
            a_lines.len(),
        );
        return;
    }
    push_hunk(out, &b_lines, &a_lines);
}

fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn push_hunk(out: &mut String, before: &[&str], after: &[&str]) {
    let mut start = 0;
    while start < before.len() && start < after.len() && before[start] == after[start] {
        start += 1;
    }
    let mut bend = before.len();
    let mut aend = after.len();
    while bend > start && aend > start && before[bend - 1] == after[aend - 1] {
        bend -= 1;
        aend -= 1;
    }
    let pre = start.saturating_sub(3);
    let b_post = (bend + 3).min(before.len());
    let a_post = (aend + 3).min(after.len());
    let b_count = b_post - pre;
    let a_count = a_post.saturating_sub(pre);
    let _ = writeln!(out, "@@ -{},{b_count} +{},{a_count} @@", pre + 1, pre + 1);
    for line in &before[pre..start] {
        let _ = writeln!(out, " {line}");
    }
    for line in &before[start..bend] {
        let _ = writeln!(out, "-{line}");
    }
    for line in &after[start..aend] {
        let _ = writeln!(out, "+{line}");
    }
    for line in &before[bend..b_post] {
        let _ = writeln!(out, " {line}");
    }
}
