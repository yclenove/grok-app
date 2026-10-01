//! Restricted zip extraction for Node and Chromium archives.
//! Never follows links, never runs archive scripts, never writes outside staging.

use crate::playwright_materialize::sha256_hex;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use zip::CompressionMethod;
use zip::ZipArchive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipError {
    pub code: String,
    pub message: String,
}

impl ZipError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ZipPolicy {
    pub expected_root: String,
    pub max_files: u32,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_path_chars: usize,
    pub max_compression_ratio: f64,
    /// If set, only these paths (relative to expected_root) are written.
    /// All entries are still validated.
    pub extract_allowlist: Option<Vec<String>>,
    /// Exact relative aliases from the hash-pinned macOS Chromium framework.
    /// Empty for every other archive. Never accept arbitrary archive links.
    pub allowed_symlinks: BTreeMap<String, String>,
}

impl ZipPolicy {
    pub fn chromium() -> Self {
        Self {
            expected_root: "chrome-win".into(),
            max_files: 200,
            max_file_bytes: 400 * 1024 * 1024,
            max_total_bytes: 512 * 1024 * 1024,
            max_path_chars: 240,
            max_compression_ratio: 20.0,
            extract_allowlist: None,
            allowed_symlinks: BTreeMap::new(),
        }
    }

    pub fn node_win_x64() -> Self {
        Self {
            expected_root: "node-v20.18.0-win-x64".into(),
            max_files: 8000,
            max_file_bytes: 120 * 1024 * 1024,
            max_total_bytes: 512 * 1024 * 1024,
            max_path_chars: 240,
            max_compression_ratio: 20.0,
            extract_allowlist: Some(vec!["node.exe".into(), "LICENSE".into()]),
            allowed_symlinks: BTreeMap::new(),
        }
    }
}

pub fn extract_zip(
    bytes: &[u8],
    dest: &Path,
    policy: &ZipPolicy,
    expected_sha256: Option<&str>,
) -> Result<u32, ZipError> {
    if let Some(expected) = expected_sha256 {
        let actual = sha256_hex(bytes);
        if actual != expected {
            return Err(ZipError::new(
                "hash_mismatch",
                "zip archive failed integrity check",
            ));
        }
    }
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|e| ZipError::new("io", e.to_string()))?;
    }
    fs::create_dir_all(dest).map_err(|e| ZipError::new("io", e.to_string()))?;
    let staging_parent = dest
        .parent()
        .ok_or_else(|| ZipError::new("invalid_dest", "zip dest must have a parent"))?;
    let staging = staging_parent.join(format!(".zip-staging-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        fs::create_dir_all(&staging).map_err(|e| ZipError::new("io", e.to_string()))?;
        let written = extract_into(&staging, bytes, policy)?;
        fs::rename(&staging, dest).map_err(|e| ZipError::new("io", e.to_string()))?;
        Ok(written)
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    if result.is_err() && dest.exists() {
        let _ = fs::remove_dir_all(dest);
    }
    result
}

fn extract_into(staging: &Path, bytes: &[u8], policy: &ZipPolicy) -> Result<u32, ZipError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ZipError::new("truncated", e.to_string()))?;
    let staging_canon =
        fs::canonicalize(staging).map_err(|e| ZipError::new("io", e.to_string()))?;
    let mut files = 0u32;
    let mut total = 0u64;
    let mut seen_case = HashSet::new();
    let mut saw_root = false;
    let allow = policy
        .extract_allowlist
        .as_ref()
        .map(|v| v.iter().cloned().collect::<HashSet<_>>());

    let mut links = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| ZipError::new("truncated", e.to_string()))?;
        if entry.encrypted() {
            return Err(ZipError::new("path", "encrypted zip entry is not allowed"));
        }
        match entry.compression() {
            CompressionMethod::Stored | CompressionMethod::Deflated => {}
            other => {
                return Err(ZipError::new(
                    "path",
                    format!("unsupported zip compression {other:?}"),
                ));
            }
        }
        let is_symlink = entry.is_symlink();
        if let Some(mode) = entry.unix_mode() {
            // S_IFLNK | S_IFCHR | S_IFBLK | S_IFIFO | S_IFSOCK
            let kind = mode & 0o170000;
            if matches!(kind, 0o020000 | 0o060000 | 0o010000 | 0o140000) {
                return Err(ZipError::new(
                    "symlink",
                    "zip special unix mode is not allowed",
                ));
            }
        }
        let raw = entry.name().to_string();
        let norm = validate_zip_name(&raw, policy.max_path_chars)?;
        let rel = classify_zip_path(&norm, &policy.expected_root)?;
        if matches!(rel, ZipRel::Root) || entry.is_dir() || norm.ends_with('/') {
            if matches!(rel, ZipRel::Root) {
                saw_root = true;
            }
            continue;
        }
        let ZipRel::Inside(rel) = rel else {
            return Err(ZipError::new(
                "path",
                format!(
                    "archive contains a root other than {}/",
                    policy.expected_root
                ),
            ));
        };
        if is_symlink && !policy.allowed_symlinks.contains_key(&rel) {
            return Err(ZipError::new("symlink", "zip symlink is not allowlisted"));
        }
        saw_root = true;
        let folded = rel.to_ascii_lowercase();
        if !seen_case.insert(folded) {
            return Err(ZipError::new(
                "duplicate_path",
                format!("duplicate zip path after case fold: {rel}"),
            ));
        }
        files += 1;
        if files > policy.max_files {
            return Err(ZipError::new("oversize", "too many archive files"));
        }
        let size = entry.size();
        if size > policy.max_file_bytes {
            return Err(ZipError::new("oversize", "archive file too large"));
        }
        total = total.saturating_add(size);
        if total > policy.max_total_bytes {
            return Err(ZipError::new("oversize", "uncompressed archive too large"));
        }
        let should_write = allow.as_ref().map(|set| set.contains(&rel)).unwrap_or(true);
        if !should_write {
            let mut sink = vec![0u8; 8192];
            let mut read_total = 0u64;
            loop {
                let n = entry
                    .read(&mut sink)
                    .map_err(|e| ZipError::new("truncated", e.to_string()))?;
                if n == 0 {
                    break;
                }
                read_total += n as u64;
                if read_total > size && size != 0 {
                    return Err(ZipError::new("truncated", "zip file size mismatch"));
                }
            }
            continue;
        }
        let dest = staging.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| ZipError::new("io", e.to_string()))?;
        }
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| ZipError::new("truncated", e.to_string()))?;
        if size != 0 && buf.len() as u64 != size {
            return Err(ZipError::new("truncated", "zip file size mismatch"));
        }
        if is_symlink {
            let target = std::str::from_utf8(&buf)
                .map_err(|_| ZipError::new("symlink", "invalid symlink target"))?;
            if policy.allowed_symlinks.get(&rel).map(String::as_str) != Some(target) {
                return Err(ZipError::new(
                    "symlink",
                    "zip symlink target does not match pinned layout",
                ));
            }
            links.push((rel, target.to_string()));
            continue;
        }
        fs::write(&dest, &buf).map_err(|e| ZipError::new("io", e.to_string()))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            // Preserve ordinary executable bits, never setuid/setgid/sticky.
            fs::set_permissions(&dest, fs::Permissions::from_mode(mode & 0o777))
                .map_err(|e| ZipError::new("io", e.to_string()))?;
        }
        assert_inside(&staging_canon, &dest)?;
    }
    links.sort_by_key(|(path, _)| !path.ends_with("/Versions/Current"));
    for (relative, target) in links {
        let path = staging.join(relative);
        let resolved = path.parent().unwrap().join(&target);
        assert_inside(&staging_canon, &resolved)?;
        crate::runtime_chromium::create_link(&path, Path::new(&target), resolved.is_dir())
            .map_err(|e| ZipError::new(&e.code, e.message))?;
        assert_inside(&staging_canon, &path)?;
    }
    if !saw_root {
        return Err(ZipError::new(
            "path",
            format!("archive is missing the {}/ root", policy.expected_root),
        ));
    }
    let ratio = total as f64 / (bytes.len().max(1) as f64);
    if ratio > policy.max_compression_ratio {
        return Err(ZipError::new(
            "oversize",
            format!("compression ratio {ratio:.1} exceeds limit"),
        ));
    }
    let written = match &policy.extract_allowlist {
        Some(list) => list
            .iter()
            .filter(|name| staging.join(*name).is_file())
            .count() as u32,
        None => files,
    };
    Ok(written)
}

fn validate_zip_name(raw: &str, max_chars: usize) -> Result<String, ZipError> {
    if raw.contains('\0') {
        return Err(ZipError::new("path", "archive path contains NUL"));
    }
    if raw.contains(':') {
        return Err(ZipError::new(
            "path",
            "archive path contains drive, UNC, or ADS",
        ));
    }
    let mut s = raw.replace('\\', "/");
    while s.starts_with("./") {
        s = s[2..].to_string();
    }
    if s.starts_with('/') || s.starts_with("//") {
        return Err(ZipError::new("path", "absolute archive path"));
    }
    if s.starts_with("\\\\") || s.to_ascii_lowercase().starts_with("//?/") {
        return Err(ZipError::new("path", "UNC archive path"));
    }
    if s.chars().count() > max_chars {
        return Err(ZipError::new("path", "archive path too long"));
    }
    for part in s.split('/') {
        if part.is_empty() {
            continue;
        }
        if part == "." || part == ".." {
            return Err(ZipError::new("path", "archive path must not traverse"));
        }
        if part.contains('\0') || part.contains(':') {
            return Err(ZipError::new("path", "illegal path segment"));
        }
    }
    Ok(s.trim_end_matches('/').to_string())
}

enum ZipRel {
    Root,
    Inside(String),
}

fn classify_zip_path(norm: &str, expected_root: &str) -> Result<ZipRel, ZipError> {
    let trimmed = norm.trim_end_matches('/');
    if trimmed == expected_root {
        return Ok(ZipRel::Root);
    }
    let prefix = format!("{expected_root}/");
    if let Some(rest) = trimmed.strip_prefix(&prefix) {
        if rest.is_empty() {
            return Ok(ZipRel::Root);
        }
        return Ok(ZipRel::Inside(rest.to_string()));
    }
    Err(ZipError::new(
        "path",
        format!("archive contains a root other than {expected_root}/"),
    ))
}

fn assert_inside(root: &Path, child: &Path) -> Result<(), ZipError> {
    let child = fs::canonicalize(child).map_err(|e| ZipError::new("path", e.to_string()))?;
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
    if child_s != root_s && !child_s.starts_with(&(root_s.clone() + "/")) {
        return Err(ZipError::new("path", "canonical path escaped staging"));
    }
    Ok(())
}

pub fn write_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, data) in entries {
            zip.start_file(*name, opts).expect("start zip file");
            zip.write_all(data).expect("write zip file");
        }
        zip.finish().expect("finish zip");
    }
    cursor.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grok-cu-zip-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn policy_tiny() -> ZipPolicy {
        ZipPolicy {
            expected_root: "root".into(),
            max_files: 8,
            max_file_bytes: 1024,
            max_total_bytes: 4096,
            max_path_chars: 80,
            max_compression_ratio: 20.0,
            extract_allowlist: None,
            allowed_symlinks: BTreeMap::new(),
        }
    }

    #[test]
    fn extracts_expected_root_and_rejects_unsafe_entries() {
        let dir = temp_dir();
        let dest = dir.join("out");
        let ok = write_test_zip(&[("root/a.txt", b"hello"), ("root/sub/b.txt", b"world")]);
        let n = extract_zip(&ok, &dest, &policy_tiny(), None).expect("extract ok");
        assert_eq!(n, 2);
        assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"hello");
        assert_eq!(fs::read(dest.join("sub/b.txt")).unwrap(), b"world");

        let keep = dir.join("keep");
        fs::create_dir_all(&keep).unwrap();
        fs::write(keep.join("prev.txt"), b"previous").unwrap();

        for (name, code) in [
            ("/abs/a.txt", "path"),
            ("C:/Windows/a.txt", "path"),
            ("//unc/share/a.txt", "path"),
            ("root/a.txt:stream", "path"),
            ("root/foo\0bar.txt", "path"),
            ("root/../evil.txt", "path"),
            ("other/root.txt", "path"),
            ("./root/../x.txt", "path"),
        ] {
            let bytes = write_test_zip(&[(name, b"x")]);
            let err = extract_zip(&bytes, &keep.join("x"), &policy_tiny(), None).unwrap_err();
            assert_eq!(err.code, code, "{name} -> {err:?}");
            assert_eq!(fs::read(keep.join("prev.txt")).unwrap(), b"previous");
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_case_and_oversize_and_wrong_hash_fail_closed() {
        let dir = temp_dir();
        let dest = dir.join("out");
        let dup = write_test_zip(&[("root/A.txt", b"one"), ("root/a.txt", b"two")]);
        let err = extract_zip(&dup, &dest, &policy_tiny(), None).unwrap_err();
        assert_eq!(err.code, "duplicate_path", "{err:?}");
        assert!(
            !dest.exists()
                || dest
                    .read_dir()
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(true)
        );

        let big = vec![0u8; 2048];
        let over = write_test_zip(&[("root/huge.bin", &big)]);
        let err = extract_zip(&over, &dest, &policy_tiny(), None).unwrap_err();
        assert_eq!(err.code, "oversize", "{err:?}");

        let ok = write_test_zip(&[("root/a.txt", b"hello")]);
        let err = extract_zip(&ok, &dest, &policy_tiny(), Some("deadbeef")).unwrap_err();
        assert_eq!(err.code, "hash_mismatch", "{err:?}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn allowlist_extracts_only_named_files() {
        let dir = temp_dir();
        let dest = dir.join("out");
        let mut policy = ZipPolicy::node_win_x64();
        policy.expected_root = "node-v20.18.0-win-x64".into();
        let bytes = write_test_zip(&[
            ("node-v20.18.0-win-x64/node.exe", b"MZ-node"),
            ("node-v20.18.0-win-x64/LICENSE", b"license"),
            ("node-v20.18.0-win-x64/npm.cmd", b"should-skip"),
        ]);
        extract_zip(&bytes, &dest, &policy, None).unwrap();
        assert_eq!(fs::read(dest.join("node.exe")).unwrap(), b"MZ-node");
        assert_eq!(fs::read(dest.join("LICENSE")).unwrap(), b"license");
        assert!(!dest.join("npm.cmd").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
