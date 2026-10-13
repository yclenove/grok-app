//! Safe materialization of the pinned playwright-core tarball.
//! Never runs npm/lifecycle scripts. Never follows archive links.

use crate::runtime::{PLAYWRIGHT_CORE_TGZ_SHA256, REQUIRED_PLAYWRIGHT_CORE_VERSION};
use crate::runtime_pins::playwright_archive_relpath;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tar::{Archive, EntryType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaywrightTree {
    pub listing_sha256: String,
    pub total_bytes: u64,
    pub file_count: u32,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializeError {
    pub code: String,
    pub message: String,
}

impl MaterializeError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn materialize_playwright_tgz(
    tgz: &[u8],
    dest_core_dir: &Path,
    expected_sha256: Option<&str>,
) -> Result<PlaywrightTree, MaterializeError> {
    if tgz.len() as u64 > crate::runtime_limits::playwright().max_archive_bytes {
        return Err(MaterializeError::new(
            "oversize",
            "compressed Playwright archive too large",
        ));
    }
    if let Some(expected) = expected_sha256 {
        let actual = sha256_hex(tgz);
        if actual != expected {
            return Err(MaterializeError::new(
                "hash_mismatch",
                "playwright-core archive failed integrity check",
            ));
        }
    }
    let parent = dest_core_dir.parent().ok_or_else(|| {
        MaterializeError::new("invalid_dest", "playwright-core dest must have a parent")
    })?;
    fs::create_dir_all(parent).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    let staging = parent.join(format!(".playwright-staging-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        fs::create_dir_all(&staging).map_err(|e| MaterializeError::new("io", e.to_string()))?;
        extract_package_root(tgz, &staging)?;
        let pkg = fs::read(staging.join("package.json"))
            .map_err(|_| MaterializeError::new("import_failed", "package.json missing"))?;
        let value: serde_json::Value = serde_json::from_slice(&pkg)
            .map_err(|_| MaterializeError::new("import_failed", "package.json invalid"))?;
        if value.get("name").and_then(|v| v.as_str()) != Some("playwright-core") {
            return Err(MaterializeError::new(
                "import_failed",
                "package name is not playwright-core",
            ));
        }
        let version = value
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if version != REQUIRED_PLAYWRIGHT_CORE_VERSION {
            return Err(MaterializeError::new(
                "version_mismatch",
                format!("playwright-core version {version}"),
            ));
        }
        let tree = tree_manifest(&staging)?;
        if dest_core_dir.exists() {
            fs::remove_dir_all(dest_core_dir)
                .map_err(|e| MaterializeError::new("io", e.to_string()))?;
        }
        fs::rename(&staging, dest_core_dir)
            .map_err(|e| MaterializeError::new("io", e.to_string()))?;
        Ok(PlaywrightTree {
            listing_sha256: tree.0,
            total_bytes: tree.1,
            file_count: tree.2,
            version,
        })
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

pub fn tree_manifest(dir: &Path) -> Result<(String, u64, u32), MaterializeError> {
    let listing = tree_listing(dir)?;
    Ok(crate::runtime_tree::manifest(&listing))
}

pub fn tree_listing(dir: &Path) -> Result<String, MaterializeError> {
    crate::runtime_tree::listing(
        dir,
        crate::runtime_limits::playwright(),
        &Default::default(),
    )
}

fn extract_package_root(tgz: &[u8], staging: &Path) -> Result<(), MaterializeError> {
    let limits = crate::runtime_limits::playwright();
    crate::runtime_tar::validate(tgz, limits)?;
    let decoder = crate::runtime_tar::decoder(tgz, limits)?;
    let mut archive = Archive::new(decoder);
    archive.set_preserve_permissions(false);
    archive.set_unpack_xattrs(false);
    let mut budget = crate::runtime_limits::TreeBudget::default();
    let mut seen = BTreeSet::new();
    let mut saw_package = false;
    let staging_canon =
        fs::canonicalize(staging).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    let entries = archive
        .entries()
        .map_err(|e| MaterializeError::new("truncated", e.to_string()))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| MaterializeError::new("truncated", e.to_string()))?;
        let kind = entry.header().entry_type();
        budget.add(
            limits,
            if kind.is_dir() {
                None
            } else {
                Some(entry.size())
            },
        )?;
        if matches!(
            kind,
            EntryType::Symlink
                | EntryType::Link
                | EntryType::Char
                | EntryType::Block
                | EntryType::Fifo
                | EntryType::Continuous
        ) {
            return Err(MaterializeError::new(
                "symlink",
                format!("archive entry type {kind:?} is not allowed"),
            ));
        }
        let path = entry
            .path()
            .map_err(|e| MaterializeError::new("path", e.to_string()))?;
        let raw = path.to_string_lossy();
        if raw.contains('\0') || raw.contains(':') {
            return Err(MaterializeError::new(
                "path",
                "archive path contains NUL, ADS, or drive prefix",
            ));
        }
        let norm = raw.replace('\\', "/");
        if norm.starts_with('/') || norm.starts_with("//") {
            return Err(MaterializeError::new("path", "absolute archive path"));
        }
        if !norm.starts_with("package/") && norm != "package" {
            return Err(MaterializeError::new(
                "path",
                "archive contains a root other than package/",
            ));
        }
        if !seen.insert(norm.trim_end_matches('/').to_ascii_lowercase()) {
            return Err(MaterializeError::new(
                "duplicate_path",
                "duplicate Playwright archive path",
            ));
        }
        saw_package = true;
        let stripped = norm.strip_prefix("package/").unwrap_or("");
        if stripped.is_empty() {
            if !kind.is_dir() || entry.size() != 0 {
                return Err(MaterializeError::new(
                    "path",
                    "package root must be an empty directory",
                ));
            }
            continue;
        }
        validate_relpath(stripped)?;
        if stripped.chars().count() > limits.max_path_chars {
            return Err(MaterializeError::new("path", "archive path too long"));
        }
        let dest = staging.join(stripped.replace('/', std::path::MAIN_SEPARATOR_STR));
        if kind.is_dir() {
            if entry.size() != 0 {
                return Err(MaterializeError::new(
                    "path",
                    "archive directory has payload",
                ));
            }
            fs::create_dir_all(&dest).map_err(|e| MaterializeError::new("io", e.to_string()))?;
            assert_inside(&staging_canon, &dest)?;
            continue;
        }
        if !kind.is_file() {
            return Err(MaterializeError::new(
                "symlink",
                format!("archive entry type {kind:?} is not allowed"),
            ));
        }
        let size = entry.size();
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| MaterializeError::new("io", e.to_string()))?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(|e| MaterializeError::new("io", e.to_string()))?;
        crate::runtime_tree::copy_exact(&mut entry, &mut file, size)?;
        drop(file);
        assert_inside(&staging_canon, &dest)?;
    }
    if !saw_package {
        return Err(MaterializeError::new(
            "path",
            "archive is missing the package/ root",
        ));
    }
    let ratio = budget.bytes as f64 / (tgz.len().max(1) as f64);
    if ratio > limits.max_compression_ratio {
        return Err(MaterializeError::new(
            "oversize",
            format!("compression ratio {ratio:.1} exceeds limit"),
        ));
    }
    Ok(())
}

fn validate_relpath(rel: &str) -> Result<(), MaterializeError> {
    if rel.is_empty() {
        return Err(MaterializeError::new("path", "empty archive path"));
    }
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(MaterializeError::new(
                "path",
                "archive path must stay under package/",
            ));
        }
        if part.contains('\0') || part.contains(':') {
            return Err(MaterializeError::new("path", "illegal path segment"));
        }
    }
    Ok(())
}

fn assert_inside(root: &Path, child: &Path) -> Result<(), MaterializeError> {
    let child =
        fs::canonicalize(child).map_err(|e| MaterializeError::new("path", e.to_string()))?;
    let root_s = root
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let child_s = child
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let root_s = root_s.trim_start_matches("//?/").to_string();
    let child_s = child_s.trim_start_matches("//?/").to_string();
    if child_s != root_s && !child_s.starts_with(&(root_s + "/")) {
        return Err(MaterializeError::new(
            "path",
            "canonical path escaped staging",
        ));
    }
    Ok(())
}

pub fn copy_tree_secure(src: &Path, dest: &Path) -> Result<(), MaterializeError> {
    copy_tree_with_limits(src, dest, crate::runtime_limits::playwright())
}

pub(crate) fn copy_tree_with_limits(
    src: &Path,
    dest: &Path,
    limits: crate::runtime_limits::TreeLimits,
) -> Result<(), MaterializeError> {
    if src.is_symlink() {
        return Err(MaterializeError::new("symlink", "source tree is a symlink"));
    }
    fs::create_dir_all(dest).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    let dest_canon =
        fs::canonicalize(dest).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    copy_walk(src, src, dest, &dest_canon, limits, &mut Default::default())
}

fn copy_walk(
    root: &Path,
    current: &Path,
    dest_root: &Path,
    dest_canon: &Path,
    limits: crate::runtime_limits::TreeLimits,
    budget: &mut crate::runtime_limits::TreeBudget,
) -> Result<(), MaterializeError> {
    let meta =
        fs::symlink_metadata(current).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    if meta.file_type().is_symlink() {
        return Err(MaterializeError::new("symlink", "refusing to copy symlink"));
    }
    let rel = current.strip_prefix(root).unwrap_or(Path::new(""));
    if !rel.as_os_str().is_empty() {
        crate::runtime_limits::validate_path(
            &rel.to_string_lossy().replace('\\', "/"),
            limits.max_path_chars,
        )?;
    }
    budget.add(
        limits,
        if meta.is_dir() {
            None
        } else {
            Some(meta.len())
        },
    )?;
    let dest = dest_root.join(rel);
    if meta.is_dir() {
        fs::create_dir_all(&dest).map_err(|e| MaterializeError::new("io", e.to_string()))?;
        assert_inside(dest_canon, &dest)?;
        for entry in
            fs::read_dir(current).map_err(|e| MaterializeError::new("io", e.to_string()))?
        {
            let entry = entry.map_err(|e| MaterializeError::new("io", e.to_string()))?;
            copy_walk(root, &entry.path(), dest_root, dest_canon, limits, budget)?;
        }
        return Ok(());
    }
    if !meta.is_file() {
        return Err(MaterializeError::new(
            "not_executable",
            "refusing to copy special file",
        ));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    }
    crate::runtime_tree::copy_file(current, &dest, meta.len())?;
    assert_inside(dest_canon, &dest)?;
    Ok(())
}

pub fn official_tgz_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("resources")
        .join("computer-use")
        .join("seed")
        .join(playwright_archive_relpath())
}

pub fn prepare_seed(seed: &Path) -> Result<PlaywrightTree, MaterializeError> {
    let tgz_path = seed.join(playwright_archive_relpath());
    let tgz = fs::read(&tgz_path).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    let dest = seed
        .join("playwright")
        .join("node_modules")
        .join("playwright-core");
    let tree = materialize_playwright_tgz(&tgz, &dest, Some(PLAYWRIGHT_CORE_TGZ_SHA256))?;
    let listing = tree_listing(&dest)?;
    fs::write(
        seed.join("playwright")
            .join("playwright-runtime.sha256list"),
        listing,
    )
    .map_err(|e| MaterializeError::new("io", e.to_string()))?;
    Ok(tree)
}

pub fn official_node_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("resources")
        .join("computer-use")
        .join("seed")
        .join("bin")
        .join(if cfg!(windows) { "node.exe" } else { "node" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grok-cu-pw-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn gzip_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            header.set_entry_type(tar::EntryType::Regular);
            builder.append_data(&mut header, name, *data).unwrap();
        }
        let raw = builder.into_inner().unwrap();
        gzip_bytes(&raw)
    }

    fn gzip_bytes(raw: &[u8]) -> Vec<u8> {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(raw).unwrap();
        enc.finish().unwrap()
    }

    fn ustar_file(name: &str, data: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        let name_bytes = name.as_bytes();
        header[..name_bytes.len()].copy_from_slice(name_bytes);
        let size = format!("{:011o}", data.len());
        header[124..135].copy_from_slice(size.as_bytes());
        header[156] = b'0';
        header[257..262].copy_from_slice(b"ustar");
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|b| *b as u32).sum();
        let checksum = format!("{sum:06o}");
        header[148..154].copy_from_slice(checksum.as_bytes());
        header[154] = 0;
        header[155] = b' ';
        let mut out = header.to_vec();
        out.extend_from_slice(data);
        let pad = (512 - (data.len() % 512)) % 512;
        out.extend(vec![0u8; pad]);
        out.extend_from_slice(&[0u8; 1024]);
        out
    }

    #[test]
    fn official_tgz_materializes_and_app_owned_node_imports_1_48_0() {
        let tgz = fs::read(
            crate::runtime_test_seed::path().join("playwright/playwright-core-1.48.0.tgz"),
        )
        .expect("official tgz");
        assert_eq!(sha256_hex(&tgz), PLAYWRIGHT_CORE_TGZ_SHA256);
        let root = temp_dir();
        let dest = root.join("node_modules").join("playwright-core");
        let tree = materialize_playwright_tgz(&tgz, &dest, Some(PLAYWRIGHT_CORE_TGZ_SHA256))
            .expect("materialize official tgz");
        assert_eq!(tree.version, REQUIRED_PLAYWRIGHT_CORE_VERSION);
        assert!(tree.file_count >= 100, "file_count {}", tree.file_count);
        let pkg: serde_json::Value =
            serde_json::from_slice(&fs::read(dest.join("package.json")).unwrap()).unwrap();
        assert_eq!(pkg["name"], "playwright-core");
        assert_eq!(pkg["version"], "1.48.0");
        assert!(!root.join(".playwright-staging").exists());
        let node = crate::runtime_test_seed::node();
        assert!(node.is_file(), "missing {}", node.display());
        crate::runtime_prepare::runtime_import::check_playwright_import(&node, &dest)
            .expect("materialized package loads with the packaged native Node");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn traversal_symlink_extra_root_truncated_wrong_hash_and_version_fail_closed() {
        let root = temp_dir();
        let dest = root.join("playwright-core");
        fs::write(dest.join("keep-me"), b"previous").ok();
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("keep-me"), b"previous").unwrap();

        let traversal = gzip_bytes(&ustar_file("package/../../evil.txt", b"pwn"));
        let err = materialize_playwright_tgz(&traversal, &dest, None).unwrap_err();
        assert_eq!(err.code, "path", "{err:?}");
        assert_eq!(fs::read(dest.join("keep-me")).unwrap(), b"previous");
        assert!(
            fs::read_dir(&root)
                .unwrap()
                .filter_map(|e| e.ok())
                .all(|e| !e
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".playwright-staging")),
            "staging leftover in {}",
            root.display()
        );

        let extra = gzip_tar(&[
            ("other/root.txt", b"nope"),
            (
                "package/package.json",
                br#"{"name":"playwright-core","version":"1.48.0"}"#,
            ),
        ]);
        let err = materialize_playwright_tgz(&extra, &dest, None).unwrap_err();
        assert_eq!(err.code, "path", "{err:?}");
        assert_eq!(fs::read(dest.join("keep-me")).unwrap(), b"previous");

        let mut official = fs::read(
            crate::runtime_test_seed::path().join("playwright/playwright-core-1.48.0.tgz"),
        )
        .unwrap();
        official.truncate(64);
        let err = materialize_playwright_tgz(&official, &dest, None).unwrap_err();
        assert!(
            err.code == "truncated" || err.code == "path" || err.code == "import_failed",
            "{err:?}"
        );
        assert_eq!(fs::read(dest.join("keep-me")).unwrap(), b"previous");

        let wrong_hash = gzip_tar(&[(
            "package/package.json",
            br#"{"name":"playwright-core","version":"1.48.0"}"#,
        )]);
        let err = materialize_playwright_tgz(&wrong_hash, &dest, Some(PLAYWRIGHT_CORE_TGZ_SHA256))
            .unwrap_err();
        assert_eq!(err.code, "hash_mismatch", "{err:?}");

        let wrong_ver = gzip_tar(&[(
            "package/package.json",
            br#"{"name":"playwright-core","version":"9.9.9"}"#,
        )]);
        let err = materialize_playwright_tgz(&wrong_ver, &dest, None).unwrap_err();
        assert_eq!(err.code, "version_mismatch", "{err:?}");
        assert_eq!(fs::read(dest.join("keep-me")).unwrap(), b"previous");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn oversize_and_symlink_entries_are_rejected() {
        let root = temp_dir();
        let dest = root.join("playwright-core");
        let big = vec![0u8; (crate::runtime_limits::playwright().max_file_bytes as usize) + 1];
        let err = materialize_playwright_tgz(&gzip_tar(&[("package/huge.bin", &big)]), &dest, None)
            .unwrap_err();
        assert_eq!(err.code, "oversize", "{err:?}");
        assert!(
            !dest.exists()
                || dest
                    .read_dir()
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(true)
        );

        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_link_name("C:/Windows/notepad.exe").unwrap();
        header.set_cksum();
        builder
            .append_data(&mut header, "package/link", std::io::empty())
            .unwrap();
        let raw = builder.into_inner().unwrap();
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw).unwrap();
        let symlink_tgz = enc.finish().unwrap();
        let err = materialize_playwright_tgz(&symlink_tgz, &dest, None).unwrap_err();
        assert_eq!(err.code, "symlink", "{err:?}");
        let _ = fs::remove_dir_all(root);
    }
}
