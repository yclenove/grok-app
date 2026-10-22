//! Copy selected attachments into content-addressed blobs.
//! `stored` only after bytes, digest, size, and mime are verified.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use sha2::{Digest, Sha256};

use super::budget::{ATTACHMENT_MAX_BYTES, IMAGE_MAX_BYTES};
use super::errors::{ErrorCode, ImportError};
use super::hash::sha256_bytes;
#[cfg(test)]
use super::types::SourceSpan;
use super::types::{AdapterOutput, AssetRecord, AssetState, ImportIssue};

const COPY_CHUNK: usize = 64 * 1024;

pub fn blob_file(blobs_dir: &Path, digest: &str) -> PathBuf {
    blobs_dir.join(digest)
}

pub fn materialize(
    adapter: &mut AdapterOutput,
    source_root: &Path,
    dest_blobs: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<(), ImportError> {
    for asset in &mut adapter.assets {
        if cancel() {
            break;
        }
        *asset = ingest(source_root, dest_blobs, asset.clone(), true, cancel)?;
        match asset.state {
            AssetState::Missing => adapter.issues.push(
                ImportIssue::new("asset-missing", "attachment file is missing")
                    .with_span(asset.asset_id.clone()),
            ),
            AssetState::Oversize => adapter.issues.push(
                ImportIssue::new("asset-oversize", "attachment exceeds size limit")
                    .with_span(asset.asset_id.clone())
                    .with_error(ErrorCode::ResourceLimit),
            ),
            AssetState::Unresolved => adapter.issues.push(
                ImportIssue::new(
                    "asset-unresolved",
                    "attachment path is not inside the source root",
                )
                .with_span(asset.asset_id.clone()),
            ),
            AssetState::Excluded | AssetState::Stored => {}
        }
    }
    adapter.recount();
    Ok(())
}

pub fn ingest(
    source_root: &Path,
    dest_blobs: &Path,
    mut asset: AssetRecord,
    selected: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<AssetRecord, ImportError> {
    if !selected {
        asset.state = AssetState::Excluded;
        asset.digest = None;
        return Ok(asset);
    }
    let Some(source_ref) = asset.source_ref.clone() else {
        asset.state = AssetState::Missing;
        return Ok(asset);
    };
    // Server-side IDs and invalid references are never filesystem locators.
    if source_ref.starts_with("opaque:") {
        asset.state = AssetState::Unresolved;
        return Ok(asset);
    }
    if let Some(url) = source_ref.strip_prefix("url:") {
        asset.state = classify_remote(url);
        return Ok(asset);
    }
    if let Some(spec) = source_ref.strip_prefix("ssh:") {
        let _ = spec;
        asset.state = AssetState::Unresolved;
        return Ok(asset);
    }
    if let Some(rest) = source_ref.strip_prefix("data:") {
        return ingest_inline(dest_blobs, asset, rest, cancel);
    }
    let path = source_ref
        .strip_prefix("path:")
        .unwrap_or(source_ref.as_str());
    ingest_path(source_root, dest_blobs, asset, Path::new(path), cancel)
}

fn classify_remote(_url: &str) -> AssetState {
    // Remote/SSH/URL refs stay unresolved. First version does not fetch them.
    AssetState::Unresolved
}

fn ingest_inline(
    dest_blobs: &Path,
    mut asset: AssetRecord,
    rest: &str,
    cancel: &dyn Fn() -> bool,
) -> Result<AssetRecord, ImportError> {
    let Some((mime, payload)) = parse_data_ref(rest) else {
        asset.state = AssetState::Unresolved;
        return Ok(asset);
    };
    if payload.is_empty() {
        asset.state = AssetState::Missing;
        return Ok(asset);
    }
    asset.mime = mime.or(asset.mime.take());
    if oversize(payload.len() as u64, asset.mime.as_deref()) {
        asset.size = payload.len() as u64;
        asset.state = AssetState::Oversize;
        return Ok(asset);
    }
    if cancel() {
        asset.state = AssetState::Unresolved;
        return Ok(asset);
    }
    finish_blob(dest_blobs, asset, &payload, cancel)
}

fn parse_data_ref(rest: &str) -> Option<(Option<String>, Vec<u8>)> {
    let (meta, data) = rest.split_once(',')?;
    let mime = meta
        .split(';')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "base64")
        .map(|s| s.to_string());
    let bytes = if meta.split(';').any(|part| part == "base64") {
        // Accept padded/unpadded legacy data while rejecting invalid symbols,
        // misplaced padding and nonzero trailing bits instead of inventing bytes.
        let cleaned: Vec<_> = data.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        let config = base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent);
        base64::engine::GeneralPurpose::new(&base64::alphabet::STANDARD, config)
            .decode(cleaned)
            .ok()?
    } else {
        data.as_bytes().to_vec()
    };
    Some((mime, bytes))
}

fn ingest_path(
    source_root: &Path,
    dest_blobs: &Path,
    mut asset: AssetRecord,
    path: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<AssetRecord, ImportError> {
    let raw = path.to_string_lossy();
    if raw.contains("://") || raw.starts_with("ssh:") {
        asset.state = AssetState::Unresolved;
        return Ok(asset);
    }
    let path = strip_file_url(path);
    let path = if path.is_absolute() {
        path
    } else {
        source_root.join(path)
    };
    if !path_in_root(&path, source_root) {
        asset.state = AssetState::Unresolved;
        return Ok(asset);
    }
    if !path.exists() {
        asset.state = AssetState::Missing;
        return Ok(asset);
    }
    let meta = fs::symlink_metadata(&path).ok();
    if meta.as_ref().is_some_and(|m| m.file_type().is_symlink()) {
        let target = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !path_in_root(&target, source_root) {
            asset.state = AssetState::Unresolved;
            return Ok(asset);
        }
    }
    let resolved = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let size = meta
        .as_ref()
        .map(|m| m.len())
        .or_else(|| fs::metadata(&resolved).ok().map(|m| m.len()))
        .unwrap_or(0);
    let sniffed = sniff_mime(&resolved).or(asset.mime.clone());
    if oversize(size, sniffed.as_deref()) {
        asset.size = size;
        asset.mime = sniffed;
        asset.state = AssetState::Oversize;
        return Ok(asset);
    }
    if cancel() {
        asset.state = AssetState::Unresolved;
        return Ok(asset);
    }
    copy_regular(&resolved, dest_blobs, asset, sniffed, size, cancel)
}

fn strip_file_url(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("file://") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn path_in_root(path: &Path, root: &Path) -> bool {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut ancestor = path.to_path_buf();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        if let Some(name) = ancestor.file_name() {
            suffix.push(name.to_owned());
        }
        if !ancestor.pop() {
            break;
        }
    }
    let mut resolved = if ancestor.exists() {
        fs::canonicalize(&ancestor).unwrap_or(ancestor)
    } else {
        ancestor
    };
    for part in suffix.into_iter().rev() {
        if part == ".." {
            resolved.pop();
        } else if part != "." {
            resolved.push(part);
        }
    }
    resolved.starts_with(&root)
}

fn oversize(size: u64, mime: Option<&str>) -> bool {
    let limit = if mime.is_some_and(|m| m.starts_with("image/")) {
        IMAGE_MAX_BYTES
    } else {
        ATTACHMENT_MAX_BYTES
    };
    size > limit
}

fn sniff_mime(path: &Path) -> Option<String> {
    let mut head = [0u8; 16];
    let Ok(mut file) = File::open(path) else {
        return mime_from_ext(path);
    };
    let n = file.read(&mut head).unwrap_or(0);
    mime_from_magic(&head[..n]).or_else(|| mime_from_ext(path))
}

fn mime_from_magic(head: &[u8]) -> Option<String> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png".into())
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg".into())
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("image/gif".into())
    } else {
        None
    }
}

fn mime_from_ext(path: &Path) -> Option<String> {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png".into()),
        "jpg" | "jpeg" => Some("image/jpeg".into()),
        "gif" => Some("image/gif".into()),
        "webp" => Some("image/webp".into()),
        _ => None,
    }
}

fn copy_regular(
    from: &Path,
    dest_blobs: &Path,
    mut asset: AssetRecord,
    mime: Option<String>,
    size: u64,
    cancel: &dyn Fn() -> bool,
) -> Result<AssetRecord, ImportError> {
    fs::create_dir_all(dest_blobs)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let tmp = dest_blobs.join(format!(
        ".partial-{}",
        sha256_bytes(asset.asset_id.as_bytes())
    ));
    let mut input = File::open(from)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut output = File::create(&tmp)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; COPY_CHUNK];
    let mut written = 0u64;
    loop {
        if cancel() {
            drop(output);
            let _ = fs::remove_file(&tmp);
            asset.state = AssetState::Unresolved;
            return Ok(asset);
        }
        let n = input
            .read(&mut buffer)
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        if n == 0 {
            break;
        }
        output
            .write_all(&buffer[..n])
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        hash.update(&buffer[..n]);
        written += n as u64;
    }
    output
        .sync_all()
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    drop(output);
    let digest = hex::encode(hash.finalize());
    let dest = blob_file(dest_blobs, &digest);
    if dest.exists() {
        let _ = fs::remove_file(&tmp);
    } else {
        fs::rename(&tmp, &dest)
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    }
    let mime = mime.or_else(|| mime_from_magic(&[]));
    asset.digest = Some(digest);
    asset.size = written.max(size);
    asset.mime = mime.or(asset.mime);
    if asset.mime.is_none() {
        asset.mime = Some("application/octet-stream".into());
    }
    asset.state = AssetState::Stored;
    Ok(asset)
}

fn finish_blob(
    dest_blobs: &Path,
    mut asset: AssetRecord,
    bytes: &[u8],
    cancel: &dyn Fn() -> bool,
) -> Result<AssetRecord, ImportError> {
    if cancel() {
        asset.state = AssetState::Unresolved;
        return Ok(asset);
    }
    fs::create_dir_all(dest_blobs)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let digest = sha256_bytes(bytes);
    let dest = blob_file(dest_blobs, &digest);
    if !dest.exists() {
        let mut output = File::create(&dest)
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        output
            .write_all(bytes)
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        output
            .sync_all()
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    }
    if asset.mime.is_none() {
        asset.mime = mime_from_magic(bytes).or(Some("application/octet-stream".into()));
    }
    asset.digest = Some(digest);
    asset.size = bytes.len() as u64;
    asset.state = AssetState::Stored;
    Ok(asset)
}

#[cfg(test)]
pub fn span(locator: &str, index: u32, block: u32) -> SourceSpan {
    SourceSpan {
        artifact_id: locator.into(),
        record: format!("record:{index}"),
        block: Some(block),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_migration::types::{
        ImportOutcome, Inventory, NormalizedGraph, Provider, SourceEvidence,
    };

    fn tiny_png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&[0u8; 24]);
        bytes
    }

    fn home() -> (
        std::path::PathBuf,
        crate::session_migration::test_home::AppHomeEnvGuard,
    ) {
        let lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let path = std::env::temp_dir().join(format!("import-assets-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        std::env::set_var("GROK_APP_HOME", &path);
        let _ = crate::paths::ensure_app_dirs();
        (path, lock)
    }

    fn rec(id: &str, source_ref: Option<&str>) -> AssetRecord {
        AssetRecord {
            asset_id: id.into(),
            digest: None,
            mime: None,
            size: 0,
            state: AssetState::Unresolved,
            source_span: span("s.jsonl", 0, 0),
            source_ref: source_ref.map(|s| s.to_string()),
        }
    }

    crate::session_migration::test_home::fixture_home_contract_tests!(
        stored_only_after_bytes_and_digest,
        missing_url_ssh_and_unselected_are_distinct,
        stale_absolute_path_does_not_match_same_name_in_new_root,
        parent_dir_escape_is_unresolved,
        image_and_generic_size_limits,
        cancel_during_copy_drops_partial_blob,
        materialize_marks_partial_and_does_not_claim_complete,
        inline_png_is_content_addressed,
        opaque_and_malformed_inline_never_become_stored,
    );

    #[test]
    fn stored_only_after_bytes_and_digest() {
        let (home, _lock) = home();
        let root = home.join("src");
        let blobs = home.join("blobs");
        fs::create_dir_all(&root).unwrap();
        let png = tiny_png();
        fs::write(root.join("pic.png"), &png).unwrap();
        let got = ingest(
            &root,
            &blobs,
            rec("a1", Some("path:pic.png")),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(got.state, AssetState::Stored);
        let digest = got.digest.clone().unwrap();
        assert_eq!(digest, sha256_bytes(&png));
        assert_eq!(got.size, png.len() as u64);
        assert_eq!(got.mime.as_deref(), Some("image/png"));
        assert_eq!(fs::read(blob_file(&blobs, &digest)).unwrap(), png);
        fs::remove_file(root.join("pic.png")).unwrap();
        assert!(
            blob_file(&blobs, &digest).is_file(),
            "preview survives source delete"
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn missing_url_ssh_and_unselected_are_distinct() {
        let (home, _lock) = home();
        let root = home.join("src");
        let blobs = home.join("blobs");
        fs::create_dir_all(&root).unwrap();
        let missing = ingest(
            &root,
            &blobs,
            rec("m", Some("path:nope.png")),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(missing.state, AssetState::Missing);
        let url = ingest(
            &root,
            &blobs,
            rec("u", Some("url:https://example.com/a.png")),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(url.state, AssetState::Unresolved);
        let ssh = ingest(
            &root,
            &blobs,
            rec("s", Some("ssh:host:/var/img.png")),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(ssh.state, AssetState::Unresolved);
        let skip = ingest(
            &root,
            &blobs,
            rec("e", Some("path:pic.png")),
            false,
            &|| false,
        )
        .unwrap();
        assert_eq!(skip.state, AssetState::Excluded);
        assert!(blobs.read_dir().map(|d| d.count()).unwrap_or(0) == 0);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn opaque_and_malformed_inline_never_become_stored() {
        let (home, _lock) = home();
        let root = home.join("src");
        let blobs = home.join("blobs");
        fs::create_dir_all(&root).unwrap();
        // A provider's opaque ID must not open a same-named local file.
        fs::write(root.join("same.png"), tiny_png()).unwrap();
        for reference in [
            "opaque:codex-file:same.png",
            "data:image/png;base64,%%%%",
            "data:image/png;base64,A",
            "data:image/png;base64,AA=AA",
            "data:image/png;base64,AB==",
            "data:image/png;base64",
        ] {
            let out = ingest(
                &root,
                &blobs,
                rec("invalid", Some(reference)),
                true,
                &|| false,
            )
            .unwrap();
            assert_eq!(out.state, AssetState::Unresolved, "{reference}");
            assert!(out.digest.is_none());
        }
        assert!(!blobs.exists());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn stale_absolute_path_does_not_match_same_name_in_new_root() {
        let (home, _lock) = home();
        let root = home.join("new-root");
        let blobs = home.join("blobs");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("pic.png"), tiny_png()).unwrap();
        let stale = home.join("old-root").join("pic.png");
        let got = ingest(
            &root,
            &blobs,
            rec("a", Some(&format!("path:{}", stale.display()))),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(got.state, AssetState::Unresolved);
        assert!(got.digest.is_none());
        assert!(
            !blobs.join(sha256_bytes(&tiny_png())).exists(),
            "must not copy the same-named file from the new root"
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn parent_dir_escape_is_unresolved() {
        let (home, _lock) = home();
        let root = home.join("src");
        let blobs = home.join("blobs");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(home.join("secret.png"), tiny_png()).unwrap();
        let escape = root.join("sub").join("..").join("..").join("secret.png");
        let got = ingest(
            &root,
            &blobs,
            rec("x", Some(&format!("path:{}", escape.display()))),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(got.state, AssetState::Unresolved);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn image_and_generic_size_limits() {
        let (home, _lock) = home();
        let root = home.join("src");
        let blobs = home.join("blobs");
        fs::create_dir_all(&root).unwrap();
        let img = root.join("big.png");
        File::create(&img)
            .unwrap()
            .set_len(IMAGE_MAX_BYTES + 1)
            .unwrap();
        let over_img = ingest(
            &root,
            &blobs,
            rec("i", Some(&format!("path:{}", img.display()))),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(over_img.state, AssetState::Oversize);
        assert!(!blobs
            .read_dir()
            .map(|mut d| d.next().is_some())
            .unwrap_or(false));
        let bin = root.join("payload.bin");
        File::create(&bin)
            .unwrap()
            .set_len(ATTACHMENT_MAX_BYTES + 1)
            .unwrap();
        let over_bin = ingest(
            &root,
            &blobs,
            rec("b", Some(&format!("path:{}", bin.display()))),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(over_bin.state, AssetState::Oversize);
        let ok_img = root.join("ok.png");
        File::create(&ok_img)
            .unwrap()
            .set_len(IMAGE_MAX_BYTES)
            .unwrap();
        let mut header = File::options().write(true).open(&ok_img).unwrap();
        header.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
        drop(header);
        let stored = ingest(
            &root,
            &blobs,
            rec("ok", Some(&format!("path:{}", ok_img.display()))),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(stored.state, AssetState::Stored);
        assert_eq!(stored.size, IMAGE_MAX_BYTES);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn cancel_during_copy_drops_partial_blob() {
        let (home, _lock) = home();
        let root = home.join("src");
        let blobs = home.join("blobs");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("chunk.bin");
        File::create(&path).unwrap().set_len(512 * 1024).unwrap();
        let count = std::cell::Cell::new(0);
        let got = ingest(
            &root,
            &blobs,
            rec("c", Some(&format!("path:{}", path.display()))),
            true,
            &|| {
                count.set(count.get() + 1);
                count.get() > 2
            },
        )
        .unwrap();
        assert_ne!(got.state, AssetState::Stored);
        let leftovers: Vec<_> = fs::read_dir(&blobs)
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        assert!(
            leftovers.is_empty(),
            "partial blob must not remain: {leftovers:?}"
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn materialize_marks_partial_and_does_not_claim_complete() {
        let (home, _lock) = home();
        let root = home.join("src");
        let blobs = home.join("blobs");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("ok.png"), tiny_png()).unwrap();
        let mut adapter = AdapterOutput {
            evidence: SourceEvidence {
                provider: Provider::GenericFile,
                source_version: None,
                schema_fingerprint: "t".into(),
                adapter_version: "1".into(),
                platform: "test".into(),
                artifacts: vec![],
            },
            graph: NormalizedGraph {
                events: vec![],
                relations: vec![],
                active_leaf: None,
                native_session_id: "n".into(),
                title: None,
                cwd: None,
            },
            inventory: Inventory::default(),
            issues: vec![],
            assets: vec![
                rec("ok", Some("path:ok.png")),
                rec("gone", Some("path:missing.png")),
                rec("web", Some("url:https://cdn.example/x.png")),
            ],
            outcome: ImportOutcome::Complete,
        };
        materialize(&mut adapter, &root, &blobs, &|| false).unwrap();
        assert_eq!(adapter.assets[0].state, AssetState::Stored);
        assert_eq!(adapter.assets[1].state, AssetState::Missing);
        assert_eq!(adapter.assets[2].state, AssetState::Unresolved);
        assert_eq!(adapter.outcome, ImportOutcome::Partial);
        assert!(adapter.issues.iter().any(|i| i.code == "asset-missing"));
        assert!(adapter.issues.iter().any(|i| i.code == "asset-unresolved"));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn inline_png_is_content_addressed() {
        let (home, _lock) = home();
        let blobs = home.join("blobs");
        let png = tiny_png();
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for chunk in png.chunks(3) {
                let a = chunk[0] as u32;
                let b = *chunk.get(1).unwrap_or(&0) as u32;
                let c = *chunk.get(2).unwrap_or(&0) as u32;
                let n = (a << 16) | (b << 8) | c;
                s.push(T[(n >> 18) as usize] as char);
                s.push(T[((n >> 12) & 63) as usize] as char);
                if chunk.len() > 1 {
                    s.push(T[((n >> 6) & 63) as usize] as char);
                }
                if chunk.len() > 2 {
                    s.push(T[(n & 63) as usize] as char);
                }
            }
            s
        };
        let got = ingest(
            &home,
            &blobs,
            rec("inline", Some(&format!("data:image/png;base64,{b64}"))),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(got.state, AssetState::Stored);
        assert_eq!(got.mime.as_deref(), Some("image/png"));
        assert_eq!(
            fs::read(blob_file(&blobs, got.digest.as_ref().unwrap())).unwrap()[..8],
            png[..8]
        );
        let _ = fs::remove_dir_all(&home);
    }
}
