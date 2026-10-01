//! Safe materialization of the pinned playwright-core tarball.
//! Never runs npm/lifecycle scripts. Never follows archive links.

use crate::runtime::{PLAYWRIGHT_CORE_TGZ_SHA256, REQUIRED_PLAYWRIGHT_CORE_VERSION};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use tar::{Archive, EntryType};

pub const MAX_FILES: u32 = 2000;
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_PATH_CHARS: usize = 240;
pub const MAX_COMPRESSION_RATIO: f64 = 20.0;

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
    let digest = sha256_hex(listing.as_bytes());
    let mut total = 0u64;
    let mut count = 0u32;
    for line in listing.lines() {
        let mut parts = line.split('\t');
        let _path = parts.next();
        if let Some(size) = parts.next().and_then(|s| s.parse::<u64>().ok()) {
            total += size;
            count += 1;
        }
    }
    Ok((digest, total, count))
}

pub fn tree_listing(dir: &Path) -> Result<String, MaterializeError> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    let mut seen = BTreeSet::new();
    for (rel, size, hash) in files {
        if !seen.insert(rel.clone()) {
            return Err(MaterializeError::new(
                "duplicate_path",
                format!("duplicate path {rel}"),
            ));
        }
        out.push_str(&rel);
        out.push('\t');
        out.push_str(&size.to_string());
        out.push('\t');
        out.push_str(&hash);
        out.push('\n');
    }
    Ok(out)
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<(String, u64, String)>,
) -> Result<(), MaterializeError> {
    let meta =
        fs::symlink_metadata(current).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    if meta.file_type().is_symlink() {
        return Err(MaterializeError::new(
            "symlink",
            "symlink is not allowed in playwright-core tree",
        ));
    }
    if meta.is_dir() {
        for entry in
            fs::read_dir(current).map_err(|e| MaterializeError::new("io", e.to_string()))?
        {
            let entry = entry.map_err(|e| MaterializeError::new("io", e.to_string()))?;
            collect_files(root, &entry.path(), files)?;
        }
        return Ok(());
    }
    if !meta.is_file() {
        return Err(MaterializeError::new(
            "not_executable",
            "non-file entry in playwright-core tree",
        ));
    }
    let rel = current
        .strip_prefix(root)
        .map_err(|_| MaterializeError::new("path", "file escaped tree root"))?
        .to_string_lossy()
        .replace('\\', "/");
    let bytes = fs::read(current).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    files.push((rel, bytes.len() as u64, sha256_hex(&bytes)));
    Ok(())
}

fn extract_package_root(tgz: &[u8], staging: &Path) -> Result<(), MaterializeError> {
    let decoder = GzDecoder::new(Cursor::new(tgz));
    let mut archive = Archive::new(decoder);
    archive.set_preserve_permissions(false);
    archive.set_unpack_xattrs(false);
    let mut files = 0u32;
    let mut total = 0u64;
    let mut saw_package = false;
    let mut extra_root = false;
    let staging_canon =
        fs::canonicalize(staging).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    let entries = archive
        .entries()
        .map_err(|e| MaterializeError::new("truncated", e.to_string()))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| MaterializeError::new("truncated", e.to_string()))?;
        let kind = entry.header().entry_type();
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
            extra_root = true;
            continue;
        }
        saw_package = true;
        let stripped = norm.strip_prefix("package/").unwrap_or("");
        if stripped.is_empty() {
            continue;
        }
        validate_relpath(stripped)?;
        if stripped.chars().count() > MAX_PATH_CHARS {
            return Err(MaterializeError::new("path", "archive path too long"));
        }
        let dest = staging.join(stripped.replace('/', std::path::MAIN_SEPARATOR_STR));
        if kind.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| MaterializeError::new("io", e.to_string()))?;
            assert_inside(&staging_canon, &dest)?;
            continue;
        }
        if !kind.is_file() && !matches!(kind, EntryType::Regular | EntryType::GNUSparse) {
            return Err(MaterializeError::new(
                "symlink",
                format!("archive entry type {kind:?} is not allowed"),
            ));
        }
        files += 1;
        if files > MAX_FILES {
            return Err(MaterializeError::new("oversize", "too many archive files"));
        }
        let size = entry.size();
        if size > MAX_FILE_BYTES {
            return Err(MaterializeError::new("oversize", "archive file too large"));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| MaterializeError::new("io", e.to_string()))?;
        }
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| MaterializeError::new("truncated", e.to_string()))?;
        if buf.len() as u64 != size && size != 0 {
            return Err(MaterializeError::new(
                "truncated",
                "archive file size mismatch",
            ));
        }
        total += buf.len() as u64;
        if total > MAX_TOTAL_BYTES {
            return Err(MaterializeError::new(
                "oversize",
                "uncompressed archive too large",
            ));
        }
        fs::write(&dest, &buf).map_err(|e| MaterializeError::new("io", e.to_string()))?;
        assert_inside(&staging_canon, &dest)?;
    }
    if extra_root {
        return Err(MaterializeError::new(
            "path",
            "archive contains a root other than package/",
        ));
    }
    if !saw_package {
        return Err(MaterializeError::new(
            "path",
            "archive is missing the package/ root",
        ));
    }
    let ratio = total as f64 / (tgz.len().max(1) as f64);
    if ratio > MAX_COMPRESSION_RATIO {
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
    if src.is_symlink() {
        return Err(MaterializeError::new("symlink", "source tree is a symlink"));
    }
    fs::create_dir_all(dest).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    let dest_canon =
        fs::canonicalize(dest).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    copy_walk(src, src, dest, &dest_canon)
}

fn copy_walk(
    root: &Path,
    current: &Path,
    dest_root: &Path,
    dest_canon: &Path,
) -> Result<(), MaterializeError> {
    let meta =
        fs::symlink_metadata(current).map_err(|e| MaterializeError::new("io", e.to_string()))?;
    if meta.file_type().is_symlink() {
        return Err(MaterializeError::new("symlink", "refusing to copy symlink"));
    }
    let rel = current.strip_prefix(root).unwrap_or(Path::new(""));
    let dest = dest_root.join(rel);
    if meta.is_dir() {
        fs::create_dir_all(&dest).map_err(|e| MaterializeError::new("io", e.to_string()))?;
        assert_inside(dest_canon, &dest)?;
        for entry in
            fs::read_dir(current).map_err(|e| MaterializeError::new("io", e.to_string()))?
        {
            let entry = entry.map_err(|e| MaterializeError::new("io", e.to_string()))?;
            copy_walk(root, &entry.path(), dest_root, dest_canon)?;
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
    fs::copy(current, &dest).map_err(|e| {
        MaterializeError::new(
            "io",
            format!("copy {} -> {}: {e}", current.display(), dest.display()),
        )
    })?;
    assert_inside(dest_canon, &dest)?;
    Ok(())
}

pub fn official_tgz_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("resources")
        .join("computer-use")
        .join("seed")
        .join("playwright")
        .join("playwright-core-1.48.0.tgz")
}

pub fn prepare_seed(seed: &Path) -> Result<PlaywrightTree, MaterializeError> {
    let tgz_path = seed.join("playwright").join("playwright-core-1.48.0.tgz");
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
        let big = vec![0u8; (MAX_FILE_BYTES as usize) + 1];
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
