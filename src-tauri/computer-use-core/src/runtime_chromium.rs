//! Locked Chromium trees. Only the five links shipped in the pinned macOS
//! framework are allowed; other components continue to reject all symlinks.

use crate::playwright_materialize::{self, sha256_hex, MaterializeError};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn error(code: &str, message: impl Into<String>) -> MaterializeError {
    MaterializeError {
        code: code.into(),
        message: message.into(),
    }
}

pub fn macos_links() -> BTreeMap<String, String> {
    let base = "Chromium.app/Contents/Frameworks/Chromium Framework.framework";
    [
        ("Resources", "Versions/Current/Resources"),
        ("Versions/Current", "130.0.6723.31"),
        ("Libraries", "Versions/Current/Libraries"),
        ("Chromium Framework", "Versions/Current/Chromium Framework"),
        ("Helpers", "Versions/Current/Helpers"),
    ]
    .into_iter()
    .map(|(path, target)| (format!("{base}/{path}"), target.into()))
    .collect()
}

pub fn is_macos(arch: &str) -> bool {
    matches!(arch, "aarch64-macos" | "x86_64-macos")
}

pub fn allowed_link(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    let Some(expected) = macos_links().get(&relative).cloned() else {
        return false;
    };
    let Ok(target) = fs::read_link(path) else {
        return false;
    };
    if target.to_string_lossy().replace('\\', "/") != expected {
        return false;
    }
    match (fs::canonicalize(root), fs::canonicalize(path)) {
        (Ok(root), Ok(target)) => target.starts_with(root),
        _ => false,
    }
}

pub fn tree_manifest(root: &Path, arch: &str) -> Result<(String, u64, u32), MaterializeError> {
    if !is_macos(arch) {
        return playwright_materialize::tree_manifest(root);
    }
    let mut rows = BTreeMap::new();
    collect(root, root, &mut rows)?;
    for path in macos_links().keys() {
        if !allowed_link(root, &root.join(path)) {
            return Err(error(
                "symlink",
                format!("missing or invalid pinned Chromium framework link {path}"),
            ));
        }
    }
    let mut listing = String::new();
    let mut size = 0;
    for (path, (bytes, digest)) in &rows {
        listing.push_str(&format!("{path}\t{bytes}\t{digest}\n"));
        size += bytes;
    }
    Ok((sha256_hex(listing.as_bytes()), size, rows.len() as u32))
}

fn collect(
    root: &Path,
    path: &Path,
    rows: &mut BTreeMap<String, (u64, String)>,
) -> Result<(), MaterializeError> {
    let meta = fs::symlink_metadata(path).map_err(|e| error("io", e.to_string()))?;
    let relative = path
        .strip_prefix(root)
        .map_err(|e| error("path", e.to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    if meta.file_type().is_symlink() {
        if !allowed_link(root, path) {
            return Err(error(
                "symlink",
                format!("untrusted Chromium link {relative}"),
            ));
        }
        let target = fs::read_link(path)
            .map_err(|e| error("io", e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        rows.insert(
            relative,
            (target.len() as u64, sha256_hex(target.as_bytes())),
        );
    } else if meta.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| error("io", e.to_string()))? {
            collect(
                root,
                &entry.map_err(|e| error("io", e.to_string()))?.path(),
                rows,
            )?;
        }
    } else if meta.is_file() {
        let bytes = fs::read(path).map_err(|e| error("io", e.to_string()))?;
        rows.insert(relative, (bytes.len() as u64, sha256_hex(&bytes)));
    } else {
        return Err(error("path", "Chromium tree contains a special file"));
    }
    Ok(())
}

pub fn copy_tree(src: &Path, dest: &Path, arch: &str) -> Result<(), MaterializeError> {
    if !is_macos(arch) {
        return playwright_materialize::copy_tree_secure(src, dest);
    }
    tree_manifest(src, arch)?;
    copy_regular(src, dest)?;
    // Current must exist before aliases resolving through it can be validated.
    let mut links: Vec<_> = macos_links().into_iter().collect();
    links.sort_by_key(|(path, _)| !path.ends_with("/Versions/Current"));
    for (path, target) in links {
        create_link(
            &dest.join(&path),
            Path::new(&target),
            src.join(&path).is_dir(),
        )?;
    }
    tree_manifest(dest, arch)?;
    Ok(())
}

fn copy_regular(src: &Path, dest: &Path) -> Result<(), MaterializeError> {
    let meta = fs::symlink_metadata(src).map_err(|e| error("io", e.to_string()))?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        fs::create_dir_all(dest).map_err(|e| error("io", e.to_string()))?;
        for entry in fs::read_dir(src).map_err(|e| error("io", e.to_string()))? {
            let entry = entry.map_err(|e| error("io", e.to_string()))?;
            copy_regular(&entry.path(), &dest.join(entry.file_name()))?;
        }
    } else if meta.is_file() {
        fs::copy(src, dest).map_err(|e| error("io", e.to_string()))?;
    } else {
        return Err(error("path", "Chromium tree contains a special file"));
    }
    Ok(())
}

pub fn create_link(path: &Path, target: &Path, directory: bool) -> Result<(), MaterializeError> {
    #[cfg(unix)]
    let result = {
        let _ = directory;
        std::os::unix::fs::symlink(target, path)
    };
    #[cfg(windows)]
    let result = if directory {
        std::os::windows::fs::symlink_dir(target, path)
    } else {
        std::os::windows::fs::symlink_file(target, path)
    };
    result.map_err(|e| error("symlink", e.to_string()))
}
