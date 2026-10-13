//! Extract only the pinned Node executable and its license from Unix tarballs.
//! npm/corepack links and lifecycle scripts are never extracted or followed.

use super::PrepareError;
use crate::playwright_materialize::sha256_hex;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub fn extract(bytes: &[u8], dest: &Path, root: &str, expected: &str) -> Result<(), PrepareError> {
    let limits = crate::runtime_limits::node();
    if bytes.len() as u64 > limits.max_archive_bytes {
        return Err(PrepareError::new(
            "oversize",
            "compressed Node archive too large",
        ));
    }
    if sha256_hex(bytes) != expected {
        return Err(PrepareError::new(
            "hash_mismatch",
            "Node tar archive failed integrity check",
        ));
    }
    crate::runtime_tar::validate(bytes, limits)
        .map_err(|e| PrepareError::new(&e.code, e.message))?;
    fs::create_dir_all(dest).map_err(|e| PrepareError::new("io", e.to_string()))?;
    let mut archive = tar::Archive::new(
        crate::runtime_tar::decoder(bytes, limits)
            .map_err(|e| PrepareError::new(&e.code, e.message))?,
    );
    let mut seen = BTreeSet::new();
    let mut extracted = BTreeSet::new();
    let mut budget = crate::runtime_limits::TreeBudget::default();
    for entry in archive
        .entries()
        .map_err(|e| PrepareError::new("truncated", e.to_string()))?
    {
        let mut entry = entry.map_err(|e| PrepareError::new("truncated", e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| PrepareError::new("path", e.to_string()))?;
        let raw = path.to_string_lossy().replace('\\', "/");
        if raw.starts_with('/')
            || raw.contains(':')
            || raw.contains('\0')
            || raw.split('/').any(|part| part == ".." || part == ".")
            || raw.contains(['\t', '\r', '\n'])
        {
            return Err(PrepareError::new("path", "unsafe Node tar path"));
        }
        if !seen.insert(raw.trim_end_matches('/').to_ascii_lowercase()) {
            return Err(PrepareError::new(
                "oversize",
                "duplicate path or too many Node archive entries",
            ));
        }
        let kind = entry.header().entry_type();
        budget
            .add(
                limits,
                if kind.is_dir() {
                    None
                } else {
                    Some(entry.size())
                },
            )
            .map_err(|e| PrepareError::new(&e.code, e.message))?;
        if kind.is_dir() && entry.size() != 0 {
            return Err(PrepareError::new(
                "path",
                "Node archive directory has payload",
            ));
        }
        if raw.trim_end_matches('/') == root {
            if !entry.header().entry_type().is_dir() || entry.size() != 0 {
                return Err(PrepareError::new(
                    "path",
                    "Node archive root must be a directory",
                ));
            }
            continue;
        }
        let prefix = format!("{root}/");
        let Some(relative) = raw.strip_prefix(&prefix) else {
            return Err(PrepareError::new("path", "unexpected Node tar root"));
        };
        crate::runtime_limits::validate_path(relative.trim_end_matches('/'), limits.max_path_chars)
            .map_err(|e| PrepareError::new(&e.code, e.message))?;
        let output = match relative {
            "bin/node" => "node",
            "LICENSE" => "LICENSE",
            _ => {
                if !kind.is_file() && !kind.is_dir() && !kind.is_symlink() && !kind.is_hard_link() {
                    return Err(PrepareError::new(
                        "symlink",
                        "unsupported Node archive entry type",
                    ));
                }
                // Unselected entries are never materialized but still fully read.
                let size = entry.size();
                crate::runtime_tree::copy_exact(&mut entry, &mut std::io::sink(), size)
                    .map_err(|e| PrepareError::new(&e.code, e.message))?;
                continue;
            }
        };
        if !entry.header().entry_type().is_file() {
            return Err(PrepareError::new(
                "symlink",
                "Node executable/license must be regular files",
            ));
        }
        let path = dest.join(output);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| PrepareError::new("io", e.to_string()))?;
        let size = entry.size();
        crate::runtime_tree::copy_exact(&mut entry, &mut file, size)
            .map_err(|e| PrepareError::new(&e.code, e.message))?;
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if output == "node" { 0o755 } else { 0o644 };
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))
                .map_err(|e| PrepareError::new("io", e.to_string()))?;
        }
        extracted.insert(output);
    }
    if extracted.len() != 2 {
        return Err(PrepareError::new(
            "missing",
            "Node archive must contain bin/node and LICENSE",
        ));
    }
    if budget.bytes as f64 / bytes.len().max(1) as f64 > limits.max_compression_ratio {
        return Err(PrepareError::new(
            "oversize",
            "Node archive compression ratio exceeds limit",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    fn archive(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, kind, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(*kind);
            header.set_mode(0o755);
            header.set_size(contents.len() as u64);
            if kind.is_symlink() {
                header.set_link_name("../outside").unwrap();
            }
            header.set_cksum();
            builder.append_data(&mut header, path, *contents).unwrap();
        }
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&builder.into_inner().unwrap()).unwrap();
        gzip.finish().unwrap()
    }

    fn check(entries: &[(&str, tar::EntryType, &[u8])]) -> Result<(), PrepareError> {
        let bytes = archive(entries);
        let dest = std::env::temp_dir().join(format!("grok-cu-node-tar-{}", uuid::Uuid::new_v4()));
        let result = extract(&bytes, &dest, "node-root", &sha256_hex(&bytes));
        if dest.exists() {
            fs::remove_dir_all(dest).unwrap();
        }
        result
    }

    #[test]
    fn extracts_only_node_and_license_and_ignores_package_links() {
        let bytes = archive(&[
            ("node-root/", tar::EntryType::Directory, b""),
            ("node-root/bin/node", tar::EntryType::Regular, b"node"),
            ("node-root/LICENSE", tar::EntryType::Regular, b"license"),
            ("node-root/bin/npm", tar::EntryType::Symlink, b""),
        ]);
        let dest = std::env::temp_dir().join(format!("grok-cu-node-tar-{}", uuid::Uuid::new_v4()));
        extract(&bytes, &dest, "node-root", &sha256_hex(&bytes)).unwrap();
        assert_eq!(fs::read(dest.join("node")).unwrap(), b"node");
        assert_eq!(fs::read(dest.join("LICENSE")).unwrap(), b"license");
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(dest.join("node"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
        }
        fs::remove_dir_all(dest).unwrap();
    }

    #[test]
    fn root_entries_cannot_bypass_archive_type_or_duplicate_bounds() {
        assert_eq!(
            check(&[("node-root", tar::EntryType::Regular, b"payload")])
                .unwrap_err()
                .code,
            "path"
        );
        assert_eq!(
            check(&[
                ("node-root/", tar::EntryType::Directory, b""),
                ("node-root/", tar::EntryType::Directory, b""),
            ])
            .unwrap_err()
            .code,
            "oversize"
        );
    }

    #[test]
    fn selected_files_reject_links_and_require_both_payloads() {
        assert_eq!(
            check(&[("node-root/bin/node", tar::EntryType::Symlink, b"")])
                .unwrap_err()
                .code,
            "symlink"
        );
        assert_eq!(
            check(&[("node-root/bin/node", tar::EntryType::Regular, b"node")])
                .unwrap_err()
                .code,
            "missing"
        );
    }

    #[test]
    fn integrity_failure_does_not_create_destination() {
        let dest = std::env::temp_dir().join(format!("grok-cu-node-tar-{}", uuid::Uuid::new_v4()));
        assert_eq!(
            extract(b"not an archive", &dest, "node-root", &"0".repeat(64))
                .unwrap_err()
                .code,
            "hash_mismatch"
        );
        assert!(!dest.exists());
    }
}
