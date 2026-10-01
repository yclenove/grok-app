//! Computer Use privacy: sentinels, audience-filtered traces, support bundle
//! staging→redact→zip→reopen scan, and fail-closed owned-path cleanup.

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::broker::TraceAudience;

pub const OWNER_MARKER: &str = ".cu-owner";
pub const MAX_RECORDS_PER_RUN: usize = 64;
pub const MAX_GLOBAL_RECORDS: usize = 256;
pub const MAX_RECORD_BYTES: usize = 1024;
pub const MAX_STORE_BYTES: u64 = 256 * 1024;
pub const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
pub const TRACE_TRUNCATED: &str = "trace_truncated";

/// Threat inventory sentinels. A support bundle or persisted trace that contains
/// any of these is a leak.
pub const SENTINELS: &[&str] = &[
    "sk-b1-privacy-openai-sentinel-do-not-leak",
    "sk-b1-privacy-grok-sentinel-do-not-leak",
    "Bearer cu-privacy-sentinel",
    "http://127.0.0.1:9/cu-privacy-proxy-sentinel",
    "cu-privacy-query-sentinel",
    "cu-privacy-form-sentinel",
    "cu-privacy-cookie-sid=",
    "cu-privacy-pairing-secret",
    "\u{89}PNG",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupKind {
    Traces,
    Staging,
    ManagedProfiles,
}

impl CleanupKind {
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Traces => "traces",
            Self::Staging => "staging",
            Self::ManagedProfiles => "browser-profiles",
        }
    }
}

pub fn contains_sentinel(text: &str) -> Option<&'static str> {
    SENTINELS.iter().copied().find(|s| text.contains(s))
}

pub fn sanitize_trace_detail(detail: &str, audience: TraceAudience) -> String {
    if detail.contains('\u{89}') && detail.contains("PNG") {
        return "[redacted:screenshot]".into();
    }
    let mut out = String::with_capacity(detail.len().min(MAX_RECORD_BYTES));
    for token in detail.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        if lower.contains("cookie")
            || lower.contains("bearer")
            || lower.starts_with("sk-")
            || lower.starts_with("xai-")
            || lower.contains("pairing") && lower.contains("secret")
        {
            out.push_str("[redacted]");
            out.push(' ');
            continue;
        }
        let stripped = strip_url_secrets(token);
        out.push_str(&stripped);
        out.push(' ');
    }
    let mut out = out.trim().to_string();
    if audience != TraceAudience::Support {
        if let Some(hit) = contains_sentinel(&out) {
            out = out.replace(hit, "[redacted:sentinel]");
        }
    }
    if out.len() > MAX_RECORD_BYTES {
        out.truncate(MAX_RECORD_BYTES);
        out.push_str(TRACE_TRUNCATED);
    }
    out
}

fn strip_url_secrets(token: &str) -> String {
    let Some(scheme) = token.find("://") else {
        if token.contains('?') || token.contains('#') {
            let cut = token.find(['?', '#']).unwrap_or(token.len());
            return token[..cut].to_string();
        }
        return token.to_string();
    };
    if scheme > 8 {
        return token.to_string();
    }
    let rest = &token[scheme + 3..];
    let host_end = rest.find(['?', '#']).unwrap_or(rest.len());
    format!("{}{}", &token[..scheme + 3], &rest[..host_end])
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredTrace {
    pub kind: String,
    pub run_id: String,
    pub detail: String,
    pub ms: u64,
    pub audience: String,
    pub unix_secs: u64,
}

pub struct TraceStore {
    root: PathBuf,
    owner: String,
    max_age: Duration,
}

impl TraceStore {
    pub fn open(root: PathBuf, owner: &str) -> Result<Self, String> {
        if owner.trim().is_empty() {
            return Err("trace store owner is empty".into());
        }
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        write_owner(&root, owner)?;
        Ok(Self {
            root,
            owner: owner.to_string(),
            max_age: MAX_AGE,
        })
    }

    pub fn with_max_age(mut self, age: Duration) -> Self {
        self.max_age = age;
        self
    }

    pub fn append(&self, event: StoredTrace) -> Result<(), String> {
        admit_owned_dir(&self.root, &self.owner)?;
        let mut event = event;
        event.detail = sanitize_trace_detail(
            &event.detail,
            match event.audience.as_str() {
                "model" => TraceAudience::Model,
                "support" => TraceAudience::Support,
                _ => TraceAudience::Ui,
            },
        );
        if event.unix_secs > now_secs().saturating_add(300) {
            event.unix_secs = now_secs();
        }
        let run = sanitize_run_id(&event.run_id)?;
        let path = self.root.join(format!("{run}.jsonl"));
        let mut records = read_jsonl(&path)?;
        records.retain(|r| now_secs().saturating_sub(r.unix_secs) <= self.max_age.as_secs());
        records.push(event);
        if records.len() > MAX_RECORDS_PER_RUN {
            let drop_n = records.len() - MAX_RECORDS_PER_RUN;
            records.drain(0..drop_n);
        }
        write_jsonl(&path, &records)?;
        self.gc_store()
    }

    pub fn load_run(&self, run_id: &str) -> Result<Vec<StoredTrace>, String> {
        admit_owned_dir(&self.root, &self.owner)?;
        let run = sanitize_run_id(run_id)?;
        let mut records = read_jsonl(&self.root.join(format!("{run}.jsonl")))?;
        records.retain(|r| now_secs().saturating_sub(r.unix_secs) <= self.max_age.as_secs());
        Ok(records)
    }

    pub fn clear(&self) -> Result<(), String> {
        clear_owned_tree(&self.root, &self.owner)
    }

    fn gc_store(&self) -> Result<(), String> {
        let mut files: Vec<(SystemTime, PathBuf, u64)> = Vec::new();
        let rd = match fs::read_dir(&self.root) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };
        for ent in rd.flatten() {
            let path = ent.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let meta = match path.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            files.push((meta.modified().unwrap_or(UNIX_EPOCH), path, meta.len()));
        }
        files.sort_by_key(|(t, _, _)| *t);
        let mut total: u64 = files.iter().map(|(_, _, n)| *n).sum();
        let mut count = files
            .iter()
            .filter_map(|(_, p, _)| read_jsonl(p).ok().map(|r| r.len()))
            .sum::<usize>();
        while (total > MAX_STORE_BYTES || count > MAX_GLOBAL_RECORDS) && files.len() > 1 {
            let (_, path, len) = files.remove(0);
            let n = read_jsonl(&path).map(|r| r.len()).unwrap_or(0);
            let _ = fs::remove_file(&path);
            total = total.saturating_sub(len);
            count = count.saturating_sub(n);
        }
        Ok(())
    }
}

fn sanitize_run_id(run_id: &str) -> Result<String, String> {
    let run = run_id.trim();
    if run.is_empty()
        || run.len() > 80
        || !run
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("illegal run id".into());
    }
    Ok(run.to_string())
}

fn read_jsonl(path: &Path) -> Result<Vec<StoredTrace>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(row) = serde_json::from_str::<StoredTrace>(line) {
            out.push(row);
        }
    }
    Ok(out)
}

fn write_jsonl(path: &Path, rows: &[StoredTrace]) -> Result<(), String> {
    let mut buf = String::new();
    for row in rows {
        buf.push_str(&serde_json::to_string(row).map_err(|e| e.to_string())?);
        buf.push('\n');
    }
    fs::write(path, buf).map_err(|e| e.to_string())
}

pub fn write_owner(dir: &Path, owner: &str) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(dir.join(OWNER_MARKER), owner).map_err(|e| e.to_string())
}

pub fn admit_owned_dir(dir: &Path, owner: &str) -> Result<PathBuf, String> {
    admit_cleanup_path(dir, owner)
}

pub fn admit_cleanup_path(path: &Path, owner: &str) -> Result<PathBuf, String> {
    let raw = path.as_os_str();
    if raw.is_empty() {
        return Err("cleanup path is empty".into());
    }
    let s = path.to_string_lossy();
    if s.trim().is_empty() {
        return Err("cleanup path is empty".into());
    }
    if s.starts_with("\\\\") || s.starts_with("//") {
        return Err("cleanup refuses UNC paths".into());
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("cleanup refuses parent-dir traversal".into());
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("cleanup path missing: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err("cleanup refuses symlink".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE: u32 = 0x400;
        if meta.file_attributes() & REPARSE != 0 {
            return Err("cleanup refuses junction/reparse".into());
        }
    }
    if !meta.is_dir() {
        return Err("cleanup target is not a directory".into());
    }
    let parent = path.parent();
    if parent.is_none() || parent == Some(Path::new("")) {
        return Err("cleanup refuses filesystem root".into());
    }
    if let Some(prefix) = path.components().next() {
        if matches!(prefix, Component::Prefix(_)) && path.components().count() <= 2 {
            let rest: Vec<_> = path.components().collect();
            if rest.len() <= 2 {
                return Err("cleanup refuses drive root".into());
            }
        }
    }
    let marker = path.join(OWNER_MARKER);
    let got = fs::read_to_string(&marker).map_err(|_| "cleanup refuses non-owner directory")?;
    if got.trim() != owner.trim() {
        return Err("cleanup refuses non-owner directory".into());
    }
    Ok(path.to_path_buf())
}

pub fn clear_owned_tree(dir: &Path, owner: &str) -> Result<(), String> {
    let admitted = admit_cleanup_path(dir, owner)?;
    for ent in fs::read_dir(&admitted).map_err(|e| e.to_string())? {
        let path = ent.map_err(|e| e.to_string())?.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == OWNER_MARKER {
            continue;
        }
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            return Err("cleanup refuses symlink child".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const REPARSE: u32 = 0x400;
            if meta.file_attributes() & REPARSE != 0 {
                return Err("cleanup refuses junction child".into());
            }
        }
        if meta.is_dir() {
            fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
        } else {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct BundleSpec {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub capability: serde_json::Value,
    pub runtime_digest: String,
    pub grants_summary: serde_json::Value,
    pub worker_health: String,
    pub recent_errors: Vec<String>,
    pub schema_version: String,
}

pub fn export_support_bundle(
    cu_root: &Path,
    owner: &str,
    spec: &BundleSpec,
) -> Result<PathBuf, String> {
    admit_cleanup_path(cu_root, owner).or_else(|_| {
        write_owner(cu_root, owner)?;
        admit_cleanup_path(cu_root, owner)
    })?;
    let stamp = now_secs();
    let staging = cu_root.join(format!("bundle-staging-{stamp}"));
    write_owner(&staging, owner)?;
    let finish = |staging: &Path, err: String| {
        let _ = fs::remove_dir_all(staging);
        err
    };
    let files = [
        (
            "meta.json",
            serde_json::json!({
                "appVersion": spec.app_version,
                "os": spec.os,
                "arch": spec.arch,
                "schemaVersion": spec.schema_version,
            }),
        ),
        ("capability.json", spec.capability.clone()),
        (
            "runtime.json",
            serde_json::json!({ "digest": spec.runtime_digest }),
        ),
        ("grants.json", spec.grants_summary.clone()),
        (
            "health.json",
            serde_json::json!({
                "worker": spec.worker_health,
                "errors": spec.recent_errors,
            }),
        ),
    ];
    for (name, value) in files {
        let raw =
            serde_json::to_string_pretty(&value).map_err(|e| finish(&staging, e.to_string()))?;
        let redacted = sanitize_trace_detail(&raw, TraceAudience::Support);
        if let Some(hit) = contains_sentinel(&redacted) {
            return Err(finish(
                &staging,
                format!("sentinel {hit} in staging {name}"),
            ));
        }
        fs::write(staging.join(name), redacted).map_err(|e| finish(&staging, e.to_string()))?;
    }
    let zip_path = cu_root.join(format!("support-{stamp}.zip"));
    compress_dir(&staging, &zip_path).map_err(|e| finish(&staging, e))?;
    if let Err(e) = rescan_archive(&zip_path) {
        let _ = fs::remove_file(&zip_path);
        return Err(finish(&staging, e));
    }
    let _ = fs::remove_dir_all(&staging);
    Ok(zip_path)
}

fn compress_dir(staging: &Path, zip_path: &Path) -> Result<(), String> {
    let file = fs::File::create(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for ent in fs::read_dir(staging).map_err(|e| e.to_string())? {
        let path = ent.map_err(|e| e.to_string())?.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("bundle entry name")?;
        if name == OWNER_MARKER {
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn rescan_archive(zip_path: &Path) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let mut buf = String::new();
        entry
            .read_to_string(&mut buf)
            .map_err(|e| format!("reopen {name}: {e}"))?;
        if let Some(hit) = contains_sentinel(&buf) {
            return Err(format!("archive sentinel {hit} in {name}"));
        }
    }
    Ok(())
}

pub fn run_privacy_gates() -> Result<(), String> {
    sentinel_suite()?;
    println!("gate: privacy_sentinel_suite");
    bounded_store_gates()?;
    println!("gate: privacy_bounded_trace_store");
    bundle_rescan_gates()?;
    println!("gate: privacy_bundle_rescan");
    cleanup_fail_closed_gates()?;
    println!("gate: privacy_cleanup_fail_closed");
    Ok(())
}

fn sentinel_suite() -> Result<(), String> {
    for s in SENTINELS {
        if contains_sentinel(s).is_none() {
            return Err(format!("sentinel not detected: {s}"));
        }
        let clean = sanitize_trace_detail(s, TraceAudience::Ui);
        if contains_sentinel(&clean).is_some() {
            return Err(format!("ui sanitize leaked {s}"));
        }
    }
    let q = sanitize_trace_detail(
        "https://example.test/page?secret=cu-privacy-query-sentinel#frag",
        TraceAudience::Ui,
    );
    if q.contains("secret=") || q.contains("cu-privacy-query-sentinel") || q.contains("#frag") {
        return Err(format!("query/fragment leaked: {q}"));
    }
    Ok(())
}

fn bounded_store_gates() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!(
        "cu-privacy-store-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let store =
        TraceStore::open(root.clone(), "run-privacy").map_err(|e| format!("open store: {e}"))?;
    let fail = |e: String| {
        let _ = fs::remove_dir_all(&root);
        e
    };
    for i in 0..(MAX_RECORDS_PER_RUN * 10) {
        store
            .append(StoredTrace {
                kind: "act".into(),
                run_id: "run-a".into(),
                detail: format!("n={i} {}", "x".repeat(32)),
                ms: i as u64,
                audience: "ui".into(),
                unix_secs: now_secs(),
            })
            .map_err(&fail)?;
    }
    let rows = store.load_run("run-a").map_err(&fail)?;
    if rows.len() > MAX_RECORDS_PER_RUN {
        return Err(fail(format!(
            "per-run cap broken: {} > {MAX_RECORDS_PER_RUN}",
            rows.len()
        )));
    }
    let disk: u64 = fs::read_dir(&root)
        .map_err(|e| fail(e.to_string()))?
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    if disk > MAX_STORE_BYTES * 2 {
        return Err(fail(format!("store unbounded: {disk}")));
    }
    store
        .append(StoredTrace {
            kind: "secret".into(),
            run_id: "run-a".into(),
            detail: SENTINELS[0].into(),
            ms: 1,
            audience: "ui".into(),
            unix_secs: now_secs(),
        })
        .map_err(&fail)?;
    let rows = store.load_run("run-a").map_err(&fail)?;
    if rows.iter().any(|r| contains_sentinel(&r.detail).is_some()) {
        return Err(fail("persisted sentinel".into()));
    }
    store.clear().map_err(&fail)?;
    let after = store.load_run("run-a").map_err(&fail)?;
    if !after.is_empty() {
        return Err(fail("clear left records".into()));
    }
    let _ = fs::remove_dir_all(&root);
    Ok(())
}

fn bundle_rescan_gates() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!(
        "cu-privacy-bundle-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    write_owner(&root, "run-privacy")?;
    let spec = BundleSpec {
        app_version: "0.2.33".into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        capability: serde_json::json!({"backend": "test"}),
        runtime_digest: "abc".into(),
        grants_summary: serde_json::json!({"authorized": false}),
        worker_health: "ok".into(),
        recent_errors: vec!["stale_snapshot".into()],
        schema_version: "1".into(),
    };
    let zip = export_support_bundle(&root, "run-privacy", &spec)?;
    rescan_archive(&zip)?;
    let mut leak = spec.clone();
    leak.recent_errors = vec![SENTINELS[0].into()];
    match export_support_bundle(&root, "run-privacy", &leak) {
        Err(e) if e.contains("sentinel") => {}
        other => {
            let _ = fs::remove_dir_all(&root);
            return Err(format!("leaky bundle must fail rescan, got {other:?}"));
        }
    }
    let leftover: Vec<_> = fs::read_dir(&root)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("bundle-staging-")
        })
        .collect();
    if !leftover.is_empty() {
        let _ = fs::remove_dir_all(&root);
        return Err("staging leftover after bundle".into());
    }
    let _ = fs::remove_dir_all(&root);
    Ok(())
}

fn cleanup_fail_closed_gates() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!(
        "cu-privacy-clean-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    write_owner(&root, "owner-a")?;
    if admit_cleanup_path(Path::new(""), "owner-a").is_ok() {
        return Err("empty path admitted".into());
    }
    if admit_cleanup_path(&root.join(".."), "owner-a").is_ok() {
        return Err("parent traversal admitted".into());
    }
    if admit_cleanup_path(Path::new("\\\\server\\share\\cu"), "owner-a").is_ok() {
        return Err("UNC admitted".into());
    }
    let other = root.join("other");
    fs::create_dir_all(&other).map_err(|e| e.to_string())?;
    if admit_cleanup_path(&other, "owner-a").is_ok() {
        return Err("non-owner admitted".into());
    }
    write_owner(&other, "owner-b")?;
    if admit_cleanup_path(&other, "owner-a").is_ok() {
        return Err("wrong owner admitted".into());
    }
    let traces = root.join(CleanupKind::Traces.dir_name());
    write_owner(&traces, "owner-a")?;
    fs::write(traces.join("keep.txt"), b"x").map_err(|e| e.to_string())?;
    let staging = root.join(CleanupKind::Staging.dir_name());
    write_owner(&staging, "owner-a")?;
    fs::write(staging.join("dl.bin"), b"y").map_err(|e| e.to_string())?;
    clear_owned_tree(&traces, "owner-a")?;
    if traces.join("keep.txt").exists() {
        return Err("traces not cleared".into());
    }
    if !staging.join("dl.bin").is_file() {
        return Err("staging cleared by traces cleanup".into());
    }
    #[cfg(windows)]
    {
        let link = root.join("junc");
        let _ = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.to_string_lossy(),
                &other.to_string_lossy(),
            ])
            .status();
        if link.exists() && admit_cleanup_path(&link, "owner-a").is_ok() {
            let _ = fs::remove_dir(&link);
            let _ = fs::remove_dir_all(&root);
            return Err("junction admitted".into());
        }
        let _ = fs::remove_dir(&link);
    }
    let _ = fs::remove_dir_all(&root);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_gates_pass() {
        run_privacy_gates().expect("privacy gates");
    }

    #[test]
    fn ten_x_oversize_stays_bounded() {
        bounded_store_gates().expect("10x oversize");
    }
}
