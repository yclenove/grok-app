//! Bounded tree inventory. Content hashes retain the existing listing format.

use crate::playwright_materialize::{sha256_hex, MaterializeError};
use crate::runtime_limits::{error, validate_path, TreeBudget, TreeLimits};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

pub(crate) fn listing(
    root: &Path,
    limits: TreeLimits,
    links: &BTreeMap<String, String>,
) -> Result<String, MaterializeError> {
    if !fs::symlink_metadata(root)
        .map_err(|e| {
            error(
                if e.kind() == std::io::ErrorKind::NotFound {
                    "missing_file"
                } else {
                    "io"
                },
                e.to_string(),
            )
        })?
        .is_dir()
    {
        return Err(error("path", "runtime tree root must be a directory"));
    }
    let mut budget = TreeBudget::default();
    let mut rows = BTreeMap::new();
    let mut seen = BTreeSet::new();
    collect(root, root, limits, links, &mut budget, &mut rows, &mut seen)?;
    for path in links.keys() {
        if !crate::runtime_chromium::allowed_link(root, &root.join(path)) {
            return Err(error(
                "symlink",
                format!("missing or invalid pinned Chromium link {path}"),
            ));
        }
    }
    Ok(rows
        .into_iter()
        .map(|(path, (size, hash))| format!("{path}\t{size}\t{hash}\n"))
        .collect())
}

pub(crate) fn manifest(listing: &str) -> (String, u64, u32) {
    let mut bytes = 0;
    let mut files = 0;
    for line in listing.lines() {
        // Only produced by the bounded collector, never parsed from an external listing.
        bytes += line.split('\t').nth(1).unwrap().parse::<u64>().unwrap();
        files += 1;
    }
    (sha256_hex(listing.as_bytes()), bytes, files)
}

fn collect(
    root: &Path,
    path: &Path,
    limits: TreeLimits,
    links: &BTreeMap<String, String>,
    budget: &mut TreeBudget,
    rows: &mut BTreeMap<String, (u64, String)>,
    seen: &mut BTreeSet<String>,
) -> Result<(), MaterializeError> {
    let meta = fs::symlink_metadata(path).map_err(|e| error("io", e.to_string()))?;
    let relative = path
        .strip_prefix(root)
        .map_err(|e| error("path", e.to_string()))?
        .to_string_lossy()
        .replace('\u{005c}', "/");
    if !relative.is_empty() {
        validate_path(&relative, limits.max_path_chars)?;
        if !seen.insert(relative.to_ascii_lowercase()) {
            return Err(error(
                "duplicate_path",
                "runtime paths collide after case fold",
            ));
        }
    }
    if meta.file_type().is_symlink() {
        let expected = links
            .get(&relative)
            .ok_or_else(|| error("symlink", "runtime tree link is not allowlisted"))?;
        if !crate::runtime_chromium::allowed_link(root, path) {
            return Err(error(
                "symlink",
                "runtime tree link does not match pinned layout",
            ));
        }
        budget.add(limits, Some(expected.len() as u64))?;
        rows.insert(
            relative,
            (expected.len() as u64, sha256_hex(expected.as_bytes())),
        );
    } else if reparse_point(&meta) {
        return Err(error("symlink", "runtime tree contains a reparse point"));
    } else if meta.is_dir() {
        budget.add(limits, None)?;
        for entry in fs::read_dir(path).map_err(|e| error("io", e.to_string()))? {
            collect(
                root,
                &entry.map_err(|e| error("io", e.to_string()))?.path(),
                limits,
                links,
                budget,
                rows,
                seen,
            )?;
        }
    } else if meta.is_file() {
        budget.add(limits, Some(meta.len()))?;
        rows.insert(relative, (meta.len(), hash_file(path, meta.len())?));
    } else {
        return Err(error("path", "runtime tree contains a special file"));
    }
    Ok(())
}

fn reparse_point(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

fn hash_file(path: &Path, size: u64) -> Result<String, MaterializeError> {
    let file = open_regular(path, size)?;
    let meta = file.metadata().map_err(|e| error("io", e.to_string()))?;
    let mut hash = Sha256::new();
    let mut reader = (&file).take(
        size.checked_add(1)
            .ok_or_else(|| error("oversize", "file size overflow"))?,
    );
    let mut buffer = [0u8; 64 * 1024];
    let mut count = 0;
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| error("io", e.to_string()))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        count += n as u64;
    }
    check_unchanged(&file, &meta, count)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn open_regular(path: &Path, size: u64) -> Result<fs::File, MaterializeError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(path).map_err(|e| error("io", e.to_string()))?;
    let meta = file.metadata().map_err(|e| error("io", e.to_string()))?;
    if !meta.is_file() || reparse_point(&meta) || meta.len() != size {
        return Err(error("truncated", "runtime file changed before hashing"));
    }
    Ok(file)
}

fn check_unchanged(
    file: &fs::File,
    meta: &fs::Metadata,
    count: u64,
) -> Result<(), MaterializeError> {
    let after = file.metadata().map_err(|e| error("io", e.to_string()))?;
    if count != meta.len()
        || after.len() != meta.len()
        || after.modified().ok() != meta.modified().ok()
    {
        return Err(error("truncated", "runtime file changed during hashing"));
    }
    Ok(())
}

pub(crate) fn copy_file(src: &Path, dest: &Path, size: u64) -> Result<(), MaterializeError> {
    let mut source = open_regular(src, size)?;
    let meta = source.metadata().map_err(|e| error("io", e.to_string()))?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|e| error("io", e.to_string()))?;
    copy_exact(&mut source, &mut output, size)?;
    drop(output);
    check_unchanged(&source, &meta, size)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            dest,
            fs::Permissions::from_mode(meta.permissions().mode() & 0o777),
        )
        .map_err(|e| error("io", e.to_string()))?;
    }
    Ok(())
}

/// Fixed-size transfer shared by archive extraction; caller enforces declared quotas first.
pub(crate) fn copy_exact(
    reader: &mut impl Read,
    writer: &mut impl Write,
    size: u64,
) -> Result<(), MaterializeError> {
    let limit = size
        .checked_add(1)
        .ok_or_else(|| error("oversize", "file size overflow"))?;
    let copied = std::io::copy(&mut reader.take(limit), writer)
        .map_err(|e| error("truncated", e.to_string()))?;
    if copied != size {
        return Err(error("truncated", "archive file size mismatch"));
    }
    Ok(())
}
