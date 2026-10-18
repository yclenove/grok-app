//! Bounded external cleanup and side-effect checks.

use super::{Observation, Status};
use crate::{judge, Error, OracleSpec, Verdict, World};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Scan a declared ended-session data root for residual files/temporary images.
///
/// Blocking filesystem IO: native adapters must call this on a dedicated
/// worker. No mutation, no symlink traversal. At most 4096 entries and depth
/// 32; exceeding either limit is an error, not a clean result. Missing roots
/// are clean, empty directories are permitted. Permission/read errors fail.
/// Caller must provide a disposable session root, never a user home. Labels
/// omit paths and file contents. No timeout/cancellation inside this bounded scan.
pub fn scan_session_residue(root: &Path) -> Result<Observation, Error> {
    let mut pending: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut visited = 0usize;
    let mut violations = Vec::new();
    while let Some((path, depth)) = pending.pop() {
        visited += 1;
        if visited > 4096 || depth > 32 {
            return Err(Error::msg("session residue scan limit exceeded"));
        }
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound && path == root => continue,
            Err(_) => return Err(Error::msg("session residue scan could not read metadata")),
        };
        if metadata.is_dir() {
            let entries = std::fs::read_dir(&path)
                .map_err(|_| Error::msg("session residue scan could not read directory"))?;
            for entry in entries {
                let entry =
                    entry.map_err(|_| Error::msg("session residue scan could not read entry"))?;
                if pending.len() + visited >= 4096 {
                    return Err(Error::msg("session residue scan limit exceeded"));
                }
                pending.push((entry.path(), depth + 1));
            }
        } else {
            violations.push(
                if metadata.file_type().is_symlink() {
                    "residual session symlink"
                } else {
                    "residual session file/temporary image"
                }
                .into(),
            );
        }
    }
    Ok(Observation {
        status: if violations.is_empty() {
            Status::Pass
        } else {
            Status::Fail
        },
        reason: String::new(),
        violations,
        restart: None,
    })
}

/// Reuse the runner's external oracle for CU and non-CU side effects.
///
/// Pure comparison, no IO/timeout/cancellation. Requires explicit file,
/// process and network checks so an empty/partial scan cannot pass. Native
/// adapters gather these observations across the entire run, not just at exit.
pub fn scan_outside_effects(spec: &OracleSpec, world: &World) -> Observation {
    use crate::EffectCheck;
    let files = spec.effects.iter().any(|e| {
        matches!(
            e,
            EffectCheck::FileUnchanged { .. } | EffectCheck::FileAbsent { .. }
        )
    });
    let processes = spec
        .effects
        .iter()
        .any(|e| matches!(e, EffectCheck::ProcessAbsent { .. }));
    let network = spec
        .effects
        .iter()
        .any(|e| matches!(e, EffectCheck::NoNetwork));
    if !files || !processes || !network {
        return Observation::not_run("CU-external file/process/network scan coverage incomplete");
    }
    let judgment = judge(spec, world);
    Observation {
        status: if judgment.verdict == Verdict::Pass {
            Status::Pass
        } else {
            Status::Fail
        },
        reason: String::new(),
        violations: judgment.violations,
        restart: None,
    }
}
