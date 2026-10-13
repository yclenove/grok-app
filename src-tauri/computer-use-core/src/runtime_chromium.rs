//! Locked Chromium trees. Only the five links shipped in the pinned macOS
//! framework are allowed; other components continue to reject all symlinks.

use crate::playwright_materialize::{self, MaterializeError};
use crate::runtime_pins::REQUIRED_CHROMIUM_VERSION;
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
        ("Versions/Current", REQUIRED_CHROMIUM_VERSION),
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
    let links = if is_macos(arch) {
        macos_links()
    } else {
        BTreeMap::new()
    };
    let listing = crate::runtime_tree::listing(root, crate::runtime_limits::chromium(), &links)?;
    Ok(crate::runtime_tree::manifest(&listing))
}

pub fn copy_tree(src: &Path, dest: &Path, arch: &str) -> Result<(), MaterializeError> {
    tree_manifest(src, arch)?;
    if !is_macos(arch) {
        playwright_materialize::copy_tree_with_limits(
            src,
            dest,
            crate::runtime_limits::chromium(),
        )?;
        tree_manifest(dest, arch)?;
        return Ok(());
    }
    copy_regular(src, dest, &mut Default::default())?;
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

fn copy_regular(
    src: &Path,
    dest: &Path,
    budget: &mut crate::runtime_limits::TreeBudget,
) -> Result<(), MaterializeError> {
    let meta = fs::symlink_metadata(src).map_err(|e| error("io", e.to_string()))?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    budget.add(
        crate::runtime_limits::chromium(),
        if meta.is_dir() {
            None
        } else {
            Some(meta.len())
        },
    )?;
    if meta.is_dir() {
        fs::create_dir_all(dest).map_err(|e| error("io", e.to_string()))?;
        for entry in fs::read_dir(src).map_err(|e| error("io", e.to_string()))? {
            let entry = entry.map_err(|e| error("io", e.to_string()))?;
            copy_regular(&entry.path(), &dest.join(entry.file_name()), budget)?;
        }
    } else if meta.is_file() {
        crate::runtime_tree::copy_file(src, dest, meta.len())?;
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
