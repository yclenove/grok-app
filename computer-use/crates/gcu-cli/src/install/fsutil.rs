//! Backups and atomic writes shared by every client installer.
//!
//! `apply_plan` is synchronous. It blocks on filesystem IO and then on the
//! injected command executor. It does not start worker threads of its own.
//! A noop plan returns before confirmation, backup, or any write.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::error::{read_err, write_err, InstallError};
use super::exec::CommandSpec;
use super::plan::{ApplyOutcome, FileChange, Plan};
use super::InstallCtx;

/// Apply `plan`. Writes nothing when the plan is a noop, including no `~/.grok` prompt.
pub(crate) fn apply_plan(ctx: &InstallCtx<'_>, plan: &Plan) -> Result<ApplyOutcome, InstallError> {
    if plan.is_noop() {
        return Ok(ApplyOutcome {
            backups: Vec::new(),
            noop: true,
        });
    }
    confirm_grok(ctx, plan)?;
    let stamp = backup_stamp(ctx)?;
    verify_unchanged(plan)?;
    let backups = write_backups(plan, &stamp)?;
    write_changes(plan)?;
    run_commands(ctx, plan)?;
    Ok(ApplyOutcome {
        backups,
        noop: false,
    })
}

fn confirm_grok(ctx: &InstallCtx<'_>, plan: &Plan) -> Result<(), InstallError> {
    let root = ctx.home.join(".grok");
    let touches = plan
        .files
        .iter()
        .any(|change| change.is_rewrite() && change.path.starts_with(&root));
    if !touches {
        return Ok(());
    }
    if ctx.yes {
        return Ok(());
    }
    if !ctx.interactive {
        return Err(InstallError::ConfirmationRequired);
    }
    let accepted = ctx.confirmer.confirm(
        "gcu will modify files under ~/.grok (MCP config and the Stop hook). Continue? [y/N]",
    )?;
    if accepted {
        Ok(())
    } else {
        Err(InstallError::ConfirmationDeclined)
    }
}

fn backup_stamp(ctx: &InstallCtx<'_>) -> Result<String, InstallError> {
    let stamp = match ctx.backup_stamp {
        Some(stamp) => stamp.to_owned(),
        None => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_owned()),
    };
    if stamp_ok(&stamp) {
        Ok(stamp)
    } else {
        Err(InstallError::InvalidBackupStamp)
    }
}

fn stamp_ok(stamp: &str) -> bool {
    !stamp.is_empty()
        && !stamp.contains(['/', '\\', ':', ' '])
        && !stamp.chars().any(char::is_whitespace)
        && stamp != "."
        && stamp != ".."
}

fn verify_unchanged(plan: &Plan) -> Result<(), InstallError> {
    for change in &plan.files {
        match &change.before {
            Some(expected) => {
                let current = fs::read(&change.path).map_err(|source| {
                    if source.kind() == std::io::ErrorKind::NotFound {
                        InstallError::ChangedDuringPlan {
                            path: change.path.clone(),
                        }
                    } else {
                        read_err(&change.path, source)
                    }
                })?;
                if current.as_slice() != expected.as_slice() {
                    return Err(InstallError::ChangedDuringPlan {
                        path: change.path.clone(),
                    });
                }
            }
            None => {
                if change.path.exists() {
                    return Err(InstallError::ChangedDuringPlan {
                        path: change.path.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn write_backups(plan: &Plan, stamp: &str) -> Result<Vec<PathBuf>, InstallError> {
    let mut backups = Vec::new();
    for change in &plan.files {
        if change.before.is_none() {
            continue;
        }
        let dest = backup_destination(&change.path, stamp)?;
        copy_new(&change.path, &dest)?;
        backups.push(dest);
    }
    Ok(backups)
}

fn backup_destination(path: &Path, stamp: &str) -> Result<PathBuf, InstallError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            write_err(
                path,
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "file name is not utf-8"),
            )
        })?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let first = parent.join(format!("{name}.gcu-backup-{stamp}"));
    if !first.exists() {
        return Ok(first);
    }
    for suffix in 2..10_000 {
        let candidate = parent.join(format!("{name}.gcu-backup-{stamp}-{suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(write_err(
        path,
        std::io::Error::new(std::io::ErrorKind::AlreadyExists, "backup name exhausted"),
    ))
}

fn copy_new(src: &Path, dest: &Path) -> Result<(), InstallError> {
    let bytes = fs::read(src).map_err(|source| read_err(src, source))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|source| write_err(dest, source))?;
    file.write_all(&bytes)
        .map_err(|source| write_err(dest, source))?;
    #[cfg(unix)]
    if let Ok(meta) = fs::metadata(src) {
        let _ = file.set_permissions(meta.permissions());
    }
    Ok(())
}

fn write_changes(plan: &Plan) -> Result<(), InstallError> {
    for change in &plan.files {
        if change.backup_only {
            continue;
        }
        match &change.after {
            Some(bytes) => write_atomic(&change.path, bytes, mode_for(change))?,
            None => {
                fs::remove_file(&change.path).map_err(|source| write_err(&change.path, source))?
            }
        }
    }
    Ok(())
}

fn mode_for(change: &FileChange) -> u32 {
    if let Some(mode) = change.mode {
        return mode & 0o777;
    }
    #[cfg(unix)]
    if change.before.is_some() {
        if let Ok(meta) = fs::metadata(&change.path) {
            use std::os::unix::fs::PermissionsExt;
            return meta.permissions().mode() & 0o777;
        }
    }
    0o644
}

fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), InstallError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| write_err(parent, source))?;
        }
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    static TEMPS: AtomicU64 = AtomicU64::new(0);
    let n = TEMPS.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{file_name}.gcu-tmp-{}-{n}", std::process::id()));
    let write_result = write_temp(&tmp, bytes, mode);
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp);
        return write_result;
    }
    if let Err(source) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(write_err(path, source));
    }
    Ok(())
}

fn write_temp(tmp: &Path, bytes: &[u8], mode: u32) -> Result<(), InstallError> {
    // Permission bits only apply on Unix; Windows ignores them.
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)
        .map_err(|source| write_err(tmp, source))?;
    file.write_all(bytes)
        .map_err(|source| write_err(tmp, source))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|source| write_err(tmp, source))?;
    }
    file.sync_all().map_err(|source| write_err(tmp, source))?;
    Ok(())
}

fn run_commands(ctx: &InstallCtx<'_>, plan: &Plan) -> Result<(), InstallError> {
    for command in &plan.commands {
        let spec = CommandSpec {
            program: command.program.clone(),
            args: command.args.clone(),
            cwd: Some(command.cwd.clone()),
            env: command.env.clone(),
        };
        ctx.executor.run(&spec)?;
    }
    Ok(())
}

pub(crate) fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, InstallError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(read_err(path, source)),
    }
}
