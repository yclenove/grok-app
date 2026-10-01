//! Deterministic prepare/check for each pinned Computer Use runtime target.

use crate::archive_zip::{extract_zip, ZipPolicy};
use crate::playwright_materialize::{self, sha256_hex, tree_listing, tree_manifest};
use crate::runtime::{looks_like_native_binary, PACK_SCHEMA_VERSION};
use crate::runtime_lock::{host_allowed, load_lock, RuntimeLock, RuntimeTarget};
use crate::runtime_mutation::{
    new_owner, remove_staging, try_acquire_exclusive, write_staging_owner, MutationError,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

#[path = "runtime_node_archive.rs"]
mod node_archive;
#[path = "runtime_import.rs"]
pub(crate) mod runtime_import;
#[path = "runtime_publish.rs"]
mod runtime_publish;
use runtime_publish::{publish_seed, recover_seed};

#[cfg(test)]
#[path = "runtime_prepare_fixture.rs"]
mod fixture;

#[cfg(test)]
#[path = "runtime_publish_tests.rs"]
mod publish_tests;

pub const WORKER_FILES: &[&str] = &[
    "loopback.mjs",
    "filename.mjs",
    "observation-extract.mjs",
    "observation-state.mjs",
    "page-state.mjs",
    "run-operations.mjs",
    "typed-act.mjs",
    "typed-drag.mjs",
    "input-cleanup.mjs",
    "context-close.mjs",
    "navigation.mjs",
    "download.mjs",
    "worker-ledger.mjs",
    "browser-pids.mjs",
    "owned-browser-process.mjs",
    "profile.mjs",
    "immutable-chrome.mjs",
    "worker-errors.mjs",
];

const FORBIDDEN_WORKER_SNIPPETS: &[&str] = &[
    "pathname === \"/state\"",
    "pathname === \"/marker\"",
    "pathname === \"/crash\"",
    "pathname === \"/evaluate\"",
];

const PLACEHOLDER_WORKER: &str = "export const grokComputerUsePlaywrightWorker = true;\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareError {
    pub code: String,
    pub message: String,
}

impl PrepareError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    fn at(mut self, phase: &str) -> Self {
        self.message = format!("{phase}: {}", self.message);
        self
    }
}

#[derive(Debug, Clone)]
pub struct PrepareContext {
    pub repo_root: PathBuf,
    pub seed_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub lock_path: PathBuf,
    pub worker_src_dir: PathBuf,
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareReport {
    pub target: String,
    pub seed: String,
    pub manifest_sha256: String,
    pub tree_sha256: String,
    /// Lock digest that `check_seed` just verified against the chromium tree.
    pub chromium_tree_sha256: String,
    /// Cross-target prepares verify bytes and layout but cannot execute Node.
    pub import_probe: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnap {
    pub size: u64,
    pub mtime: u64,
    pub hash: String,
}

impl PrepareContext {
    pub fn shipped(repo_root: PathBuf, target: String) -> Self {
        let seed_dir = repo_root
            .join("src-tauri")
            .join("resources")
            .join("computer-use")
            .join("seed");
        let lock_name = RuntimeTarget::parse(&target)
            .map(|t| t.lock_name())
            .unwrap_or("unsupported-target.lock.json");
        let lock_path = repo_root
            .join("src-tauri")
            .join("resources")
            .join("computer-use")
            .join(lock_name);
        let worker_src_dir = repo_root.join("tools").join("computer-use-browser");
        let cache_dir = std::env::var("GROK_CU_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join(".cache").join("computer-use"));
        Self {
            repo_root,
            seed_dir,
            cache_dir,
            lock_path,
            worker_src_dir,
            target,
        }
    }
}

pub fn supported_prepare_target(target: &str) -> bool {
    RuntimeTarget::parse(target).is_ok()
}

/// Load the pinned Chromium module without launching a browser. The caller must
/// verify the pack's hashes/ownership first; no system executable is searched.
pub fn check_module_import(node: &Path, playwright_root: &Path) -> Result<(), PrepareError> {
    runtime_import::check_playwright_import(node, playwright_root)
}

pub fn bundle_resource_globs(target: &str) -> Result<Vec<&'static str>, PrepareError> {
    match target {
        "x86_64-windows" | "x86_64-pc-windows-msvc" => Ok(vec![
            "resources/computer-use/windows-x64.lock.json",
            "resources/computer-use/pack-targets.json",
            "resources/computer-use/README.md",
            "resources/computer-use/seed/manifest.json",
            "resources/computer-use/seed/bin/*",
            "resources/computer-use/seed/playwright/**/*",
            "resources/computer-use/seed/chromium/chrome-win/**/*",
        ]),
        "aarch64-apple-darwin" | "aarch64-macos" | "x86_64-apple-darwin" | "x86_64-macos" => {
            Ok(vec![
                "resources/computer-use/macos-arm64.lock.json",
                "resources/computer-use/macos-x64.lock.json",
                "resources/computer-use/pack-targets.json",
                "resources/computer-use/README.md",
                "resources/computer-use/seed/manifest.json",
                "resources/computer-use/seed/bin/*",
                "resources/computer-use/seed/playwright/**/*",
                "resources/computer-use/seed/chromium/chrome-mac/**/*",
            ])
        }
        "x86_64-unknown-linux-gnu" | "x86_64-linux" => Ok(vec![
            "resources/computer-use/linux-x64.lock.json",
            "resources/computer-use/pack-targets.json",
            "resources/computer-use/README.md",
            "resources/computer-use/seed/manifest.json",
            "resources/computer-use/seed/bin/*",
            "resources/computer-use/seed/playwright/**/*",
            "resources/computer-use/seed/chromium/chrome-linux/**/*",
        ]),
        _ => Err(PrepareError::new(
            "unsupported_target",
            format!("no Computer Use resource map for {target}"),
        )),
    }
}

pub fn package_deny_list_violations(root: &Path) -> Vec<String> {
    let mut hits = Vec::new();
    let _ = walk_deny(root, root, &mut hits);
    hits.sort();
    hits
}

fn walk_deny(root: &Path, current: &Path, hits: &mut Vec<String>) -> Result<(), PrepareError> {
    let meta = match fs::symlink_metadata(current) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if meta.file_type().is_symlink() {
        let chrome = root.join("chromium/chrome-mac");
        if !crate::runtime_chromium::allowed_link(&chrome, current) {
            hits.push(rel_display(root, current) + " (symlink)");
        }
        return Ok(());
    }
    let name = current
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let rel = rel_display(root, current)
        .replace('\\', "/")
        .to_ascii_lowercase();
    let deny = name == "chromium-win64.zip"
        || name.ends_with(".zip") && rel.contains("/chromium/") && !rel.contains("/chrome-win/")
        || rel.contains("/.cache/")
        || rel.contains("/.run/")
        || rel.contains("/fixtures/")
        || name == "fixtures"
        || name.ends_with(".test.mjs")
        || name.ends_with(".dump")
        || name == ".staging"
        || name == "cu_probe.exe"
        || name == "grok-computer-use-probe.exe"
        || rel.contains("h:/aicoding/")
        || rel.contains("c:/users/");
    if deny && current != root {
        hits.push(rel_display(root, current));
    }
    if meta.is_dir() {
        if let Ok(entries) = fs::read_dir(current) {
            for entry in entries.flatten() {
                walk_deny(root, &entry.path(), hits)?;
            }
        }
    }
    Ok(())
}

fn rel_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn snapshot_seed(seed: &Path) -> Result<BTreeMap<String, FileSnap>, PrepareError> {
    let mut out = BTreeMap::new();
    if !seed.exists() {
        return Ok(out);
    }
    collect_snap(seed, seed, &mut out)?;
    Ok(out)
}

fn collect_snap(
    root: &Path,
    current: &Path,
    out: &mut BTreeMap<String, FileSnap>,
) -> Result<(), PrepareError> {
    let meta = fs::symlink_metadata(current).map_err(|e| PrepareError::new("io", e.to_string()))?;
    if meta.file_type().is_symlink() {
        if !crate::runtime_chromium::allowed_link(&root.join("chromium/chrome-mac"), current) {
            return Err(PrepareError::new(
                "symlink",
                "seed contains an untrusted symlink",
            ));
        }
        let link = fs::read_link(current)
            .map_err(|e| PrepareError::new("io", e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        out.insert(
            rel_display(root, current),
            FileSnap {
                size: link.len() as u64,
                mtime: 0,
                hash: sha256_hex(link.as_bytes()),
            },
        );
        return Ok(());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(current).map_err(|e| PrepareError::new("io", e.to_string()))? {
            collect_snap(
                root,
                &entry
                    .map_err(|e| PrepareError::new("io", e.to_string()))?
                    .path(),
                out,
            )?;
        }
        return Ok(());
    }
    if !meta.is_file() {
        return Err(PrepareError::new("path", "seed contains a special file"));
    }
    let rel = rel_display(root, current);
    let bytes = fs::read(current).map_err(|e| PrepareError::new("io", e.to_string()))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    out.insert(
        rel,
        FileSnap {
            size: bytes.len() as u64,
            mtime,
            hash: sha256_hex(&bytes),
        },
    );
    Ok(())
}

pub fn seed_tree_digest(seed: &Path) -> Result<String, PrepareError> {
    let listing = snapshot_seed(seed)?
        .into_iter()
        .map(|(path, entry)| format!("{path}\t{}\t{}\n", entry.size, entry.hash))
        .collect::<String>();
    Ok(sha256_hex(listing.as_bytes()))
}

fn require_lock_target(ctx: &PrepareContext, lock: &RuntimeLock) -> Result<(), PrepareError> {
    let target =
        RuntimeTarget::parse(&ctx.target).map_err(|e| PrepareError::new(&e.code, e.message))?;
    if target.arch() != lock.target {
        return Err(PrepareError::new(
            "arch_mismatch",
            format!(
                "requested {} but lock targets {}",
                target.arch(),
                lock.target
            ),
        ));
    }
    Ok(())
}

fn import_probe_status(lock: &RuntimeLock) -> &'static str {
    if lock.target == crate::runtime::host_arch() {
        "passed"
    } else {
        "not_run_cross_target"
    }
}

fn mutation_err(err: MutationError) -> PrepareError {
    PrepareError::new(&err.code, err.message)
}

fn finish_staging<T>(
    result: Result<T, PrepareError>,
    cleanup: Result<(), MutationError>,
) -> Result<T, PrepareError> {
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(mutation_err(error).at("remove owned staging")),
        (Err(mut error), Err(cleanup)) => {
            error.message = format!(
                "{}; staging cleanup also failed ({}): {}",
                error.message, cleanup.code, cleanup.message
            );
            Err(error)
        }
    }
}

pub fn prepare(ctx: &PrepareContext) -> Result<PrepareReport, PrepareError> {
    if !supported_prepare_target(&ctx.target) {
        return Err(PrepareError::new(
            "unsupported_target",
            format!("unsupported Computer Use runtime target {}", ctx.target),
        ));
    }
    let owner = new_owner(
        &ctx.seed_dir,
        &ctx.repo_root,
        format!("prepare-{}", uuid::Uuid::new_v4()),
    );
    let _guard = try_acquire_exclusive(&ctx.seed_dir, owner.clone()).map_err(mutation_err)?;
    recover_seed(&ctx.seed_dir).map_err(|error| error.at("recover seed"))?;
    let lock = load_lock(&ctx.lock_path).map_err(|e| PrepareError::new(&e.code, e.message))?;
    require_lock_target(ctx, &lock)?;
    fs::create_dir_all(&ctx.cache_dir)
        .map_err(|e| PrepareError::new("io", e.to_string()).at("create archive cache"))?;
    let parent = ctx
        .seed_dir
        .parent()
        .ok_or_else(|| PrepareError::new("path", "seed must have a parent"))?;
    let staging = parent.join(format!(".seed-staging-{}", uuid::Uuid::new_v4()));
    write_staging_owner(&staging, &owner)
        .map_err(|error| mutation_err(error).at("create owned staging"))?;
    let result = (|| {
        materialize_into(&staging, ctx, &lock).map_err(|error| error.at("materialize staging"))?;
        check_seed(&staging, ctx, &lock).map_err(|error| error.at("validate staging"))?;
        publish_seed(&staging, &ctx.seed_dir).map_err(|error| error.at("publish seed"))?;
        let manifest_sha256 = sha256_file(&ctx.seed_dir.join("manifest.json"))?;
        let tree_sha256 = seed_tree_digest(&ctx.seed_dir)?;
        Ok(PrepareReport {
            target: ctx.target.clone(),
            seed: ctx.seed_dir.display().to_string(),
            manifest_sha256,
            tree_sha256,
            chromium_tree_sha256: lock.chromium.tree_sha256.clone(),
            import_probe: import_probe_status(&lock).into(),
        })
    })();
    finish_staging(result, remove_staging(&staging, &owner))
}

#[cfg(test)]
mod prepare_error_tests {
    use super::{finish_staging, MutationError, PrepareError};

    fn cleanup_error() -> MutationError {
        MutationError {
            code: "io".into(),
            message: "owned directory busy".into(),
        }
    }

    #[test]
    fn phase_keeps_the_structured_error_code() {
        let error =
            PrepareError::new("digest_mismatch", "archive differs").at("materialize staging");
        assert_eq!(error.code, "digest_mismatch");
        assert_eq!(error.message, "materialize staging: archive differs");
    }

    #[test]
    fn cleanup_cannot_hide_the_prepare_error() {
        let primary =
            PrepareError::new("import_failed", "module load failed").at("validate staging");
        let error = finish_staging::<()>(Err(primary), Err(cleanup_error())).unwrap_err();
        assert_eq!(error.code, "import_failed");
        assert_eq!(error.message, "validate staging: module load failed; staging cleanup also failed (io): owned directory busy");
    }

    #[test]
    fn cleanup_failure_cannot_turn_a_successful_prepare_green() {
        let error = finish_staging(Ok(7), Err(cleanup_error())).unwrap_err();
        assert_eq!(error.code, "io");
        assert_eq!(error.message, "remove owned staging: owned directory busy");
    }

    #[test]
    fn confirmed_cleanup_preserves_original_result() {
        assert_eq!(finish_staging(Ok(7), Ok(())), Ok(7));
        let error = PrepareError::new("io", "primary");
        assert_eq!(finish_staging::<()>(Err(error.clone()), Ok(())), Err(error));
    }
}

pub fn check(ctx: &PrepareContext) -> Result<PrepareReport, PrepareError> {
    if !supported_prepare_target(&ctx.target) {
        return Err(PrepareError::new(
            "unsupported_target",
            format!("unsupported Computer Use runtime target {}", ctx.target),
        ));
    }
    let owner = new_owner(
        &ctx.seed_dir,
        &ctx.repo_root,
        format!("check-{}", uuid::Uuid::new_v4()),
    );
    let _guard = try_acquire_exclusive(&ctx.seed_dir, owner).map_err(mutation_err)?;
    let lock = load_lock(&ctx.lock_path).map_err(|e| PrepareError::new(&e.code, e.message))?;
    require_lock_target(ctx, &lock)?;
    check_seed(&ctx.seed_dir, ctx, &lock)?;
    let manifest_sha256 = sha256_file(&ctx.seed_dir.join("manifest.json"))?;
    let tree_sha256 = seed_tree_digest(&ctx.seed_dir)?;
    Ok(PrepareReport {
        target: ctx.target.clone(),
        seed: ctx.seed_dir.display().to_string(),
        manifest_sha256,
        tree_sha256,
        chromium_tree_sha256: lock.chromium.tree_sha256.clone(),
        import_probe: import_probe_status(&lock).into(),
    })
}

fn materialize_into(
    staging: &Path,
    ctx: &PrepareContext,
    lock: &RuntimeLock,
) -> Result<(), PrepareError> {
    let node_zip = ensure_archive(
        ctx,
        lock,
        &lock.js_runtime.url,
        &lock.js_runtime.archive_sha256,
        lock.js_runtime.archive_bytes,
        &[ctx
            .seed_dir
            .join("..")
            .join("cache")
            .join(&lock.js_runtime.archive_sha256)],
    )?;
    let pw_tgz = ensure_archive(
        ctx,
        lock,
        &lock.playwright.url,
        &lock.playwright.archive_sha256,
        lock.playwright.archive_bytes,
        &[ctx
            .seed_dir
            .join("playwright")
            .join("playwright-core-1.48.0.tgz")],
    )?;
    let chrome_zip = ensure_archive(
        ctx,
        lock,
        &lock.chromium.url,
        &lock.chromium.archive_sha256,
        lock.chromium.archive_bytes,
        &[ctx.seed_dir.join("chromium").join("chromium-win64.zip")],
    )?;

    let node_bytes = fs::read(&node_zip).map_err(|e| PrepareError::new("io", e.to_string()))?;
    let node_extract = staging.join("bin");
    let target =
        RuntimeTarget::parse(&lock.target).map_err(|e| PrepareError::new(&e.code, e.message))?;
    if target == RuntimeTarget::WindowsX64 {
        let mut node_policy = ZipPolicy::node_win_x64();
        node_policy.expected_root = lock.js_runtime.archive_root.clone();
        extract_zip(
            &node_bytes,
            &node_extract,
            &node_policy,
            Some(&lock.js_runtime.archive_sha256),
        )
        .map_err(|e| PrepareError::new(&e.code, e.message))?;
    } else {
        node_archive::extract(
            &node_bytes,
            &node_extract,
            &lock.js_runtime.archive_root,
            &lock.js_runtime.archive_sha256,
        )?;
    }

    let tgz = fs::read(&pw_tgz).map_err(|e| PrepareError::new("io", e.to_string()))?;
    let pw_dest = staging
        .join("playwright")
        .join("node_modules")
        .join("playwright-core");
    let tree = playwright_materialize::materialize_playwright_tgz(
        &tgz,
        &pw_dest,
        Some(&lock.playwright.archive_sha256),
    )
    .map_err(|e| PrepareError::new(&e.code, e.message))?;
    fs::create_dir_all(staging.join("playwright"))
        .map_err(|e| PrepareError::new("io", e.to_string()))?;
    fs::copy(
        &pw_tgz,
        staging
            .join("playwright")
            .join("playwright-core-1.48.0.tgz"),
    )
    .map_err(|e| PrepareError::new("io", e.to_string()))?;
    let pw_listing = tree_listing(&pw_dest).map_err(|e| PrepareError::new(&e.code, e.message))?;
    fs::write(
        staging
            .join("playwright")
            .join("playwright-runtime.sha256list"),
        pw_listing,
    )
    .map_err(|e| PrepareError::new("io", e.to_string()))?;
    if tree.listing_sha256 != lock.playwright.tree_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            "playwright-runtime tree digest does not match lock",
        ));
    }

    let chrome_bytes = fs::read(&chrome_zip).map_err(|e| PrepareError::new("io", e.to_string()))?;
    let mut chrome_policy = ZipPolicy::chromium();
    chrome_policy.expected_root = lock.chromium.archive_root.clone();
    chrome_policy.max_total_bytes = 768 * 1024 * 1024;
    chrome_policy.max_file_bytes = 600 * 1024 * 1024;
    if crate::runtime_chromium::is_macos(&lock.target) {
        chrome_policy.allowed_symlinks = crate::runtime_chromium::macos_links();
    }
    let chrome_dest = staging.join(&lock.chromium.tree_relpath);
    extract_zip(
        &chrome_bytes,
        &chrome_dest,
        &chrome_policy,
        Some(&lock.chromium.archive_sha256),
    )
    .map_err(|e| PrepareError::new(&e.code, e.message))?;

    pack_worker(staging, &ctx.worker_src_dir)?;
    write_seed_manifest(staging, lock)?;
    Ok(())
}

fn pack_worker(seed: &Path, src_dir: &Path) -> Result<(), PrepareError> {
    let dest_dir = seed.join("playwright");
    fs::create_dir_all(&dest_dir).map_err(|e| PrepareError::new("io", e.to_string()))?;
    let worker_src = src_dir.join("server.mjs");
    fs::copy(&worker_src, dest_dir.join("worker.mjs"))
        .map_err(|e| PrepareError::new("io", format!("missing production worker: {e}")))?;
    let mut listing = Vec::new();
    for name in WORKER_FILES {
        let src = src_dir.join(name);
        if !src.is_file() {
            return Err(PrepareError::new(
                "missing",
                format!("missing production sibling {name}"),
            ));
        }
        fs::copy(&src, dest_dir.join(name)).map_err(|e| PrepareError::new("io", e.to_string()))?;
        let bytes =
            fs::read(dest_dir.join(name)).map_err(|e| PrepareError::new("io", e.to_string()))?;
        listing.push(format!("{name}\t{}\t{}", bytes.len(), sha256_hex(&bytes)));
    }
    listing.sort();
    fs::write(
        dest_dir.join("modules.sha256list"),
        listing.join("\n") + "\n",
    )
    .map_err(|e| PrepareError::new("io", e.to_string()))?;
    Ok(())
}

fn write_seed_manifest(seed: &Path, lock: &RuntimeLock) -> Result<(), PrepareError> {
    let target =
        RuntimeTarget::parse(&lock.target).map_err(|e| PrepareError::new(&e.code, e.message))?;
    let node = seed.join(target.node_relpath());
    let tgz = seed.join("playwright").join("playwright-core-1.48.0.tgz");
    let worker = seed.join("playwright").join("worker.mjs");
    let modules = seed.join("playwright").join("modules.sha256list");
    let pw = seed
        .join("playwright")
        .join("node_modules")
        .join("playwright-core");
    let chrome = seed.join(&lock.chromium.tree_relpath);
    let (pw_digest, pw_bytes, _) =
        tree_manifest(&pw).map_err(|e| PrepareError::new(&e.code, e.message))?;
    let (chrome_digest, chrome_bytes, _) =
        crate::runtime_chromium::tree_manifest(&chrome, &lock.target)
            .map_err(|e| PrepareError::new(&e.code, e.message))?;
    let worker_hash = sha256_file(&worker)?;
    let manifest = serde_json::json!({
        "pack_id": "seed",
        "schema_version": PACK_SCHEMA_VERSION,
        "compatibility": {
            "protocol": 1,
            "jsRuntimeVersion": lock.js_runtime.version,
            "playwrightCoreVersion": lock.playwright.version,
            "workerSourceSha256": worker_hash,
        },
        "components": {
            "js-runtime": component("js-runtime", &lock.js_runtime.version, target.arch(), target.node_relpath(), &sha256_file(&node)?, file_len(&node)?, "file"),
            "playwright-archive": component("playwright-archive", &lock.playwright.version, "any", "playwright/playwright-core-1.48.0.tgz", &sha256_file(&tgz)?, file_len(&tgz)?, "file"),
            "playwright-runtime": component("playwright-runtime", &lock.playwright.version, "any", "playwright/node_modules/playwright-core", &pw_digest, pw_bytes, "tree"),
            "browser-worker": component("browser-worker", "app-embed", "any", "playwright/worker.mjs", &worker_hash, file_len(&worker)?, "file"),
            "chromium": component("chromium", &lock.chromium.browser_version, target.arch(), &lock.chromium.tree_relpath, &chrome_digest, chrome_bytes, "tree"),
            "browser-worker-modules": component("browser-worker-modules", "app-embed", "any", "playwright/modules.sha256list", &sha256_file(&modules)?, file_len(&modules)?, "file"),
        }
    });
    fs::write(
        seed.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)
            .map_err(|e| PrepareError::new("schema", e.to_string()))?,
    )
    .map_err(|e| PrepareError::new("io", e.to_string()))?;
    Ok(())
}

fn component(
    id: &str,
    version: &str,
    arch: &str,
    relpath: &str,
    sha256: &str,
    size: u64,
    kind: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "version": version,
        "arch": arch,
        "relpath": relpath,
        "sha256": sha256,
        "size": size,
        "kind": kind,
    })
}

fn file_len(path: &Path) -> Result<u64, PrepareError> {
    Ok(fs::metadata(path)
        .map_err(|e| PrepareError::new("io", e.to_string()))?
        .len())
}

fn sha256_file(path: &Path) -> Result<String, PrepareError> {
    let bytes =
        fs::read(path).map_err(|e| PrepareError::new("io", format!("{}: {e}", path.display())))?;
    Ok(sha256_hex(&bytes))
}

fn check_seed(seed: &Path, ctx: &PrepareContext, lock: &RuntimeLock) -> Result<(), PrepareError> {
    if !seed.is_dir() {
        return Err(PrepareError::new(
            "missing",
            "runtime seed directory is missing",
        ));
    }
    let denials = package_deny_list_violations(seed);
    if !denials.is_empty() {
        return Err(PrepareError::new(
            "extra_file",
            format!("seed contains deny-list paths: {denials:?}"),
        ));
    }
    let target =
        RuntimeTarget::parse(&lock.target).map_err(|e| PrepareError::new(&e.code, e.message))?;
    let node = seed.join(target.node_relpath());
    let node_bytes =
        fs::read(&node).map_err(|_| PrepareError::new("missing", "seed node.exe missing"))?;
    if !looks_like_native_binary(&node_bytes) {
        return Err(PrepareError::new(
            "not_executable",
            "seed node.exe is not a native binary",
        ));
    }
    if sha256_hex(&node_bytes) != lock.js_runtime.executable_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            "node.exe hash does not match lock",
        ));
    }
    if node_bytes.len() as u64 != lock.js_runtime.executable_bytes {
        return Err(PrepareError::new(
            "hash_mismatch",
            "node.exe size does not match lock",
        ));
    }

    let tgz = seed.join("playwright").join("playwright-core-1.48.0.tgz");
    let tgz_bytes =
        fs::read(&tgz).map_err(|_| PrepareError::new("missing", "playwright tgz missing"))?;
    if sha256_hex(&tgz_bytes) != lock.playwright.archive_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            "playwright tgz hash does not match lock",
        ));
    }

    let pw = seed
        .join("playwright")
        .join("node_modules")
        .join("playwright-core");
    if !pw.is_dir() {
        return Err(PrepareError::new(
            "missing",
            "materialized playwright-core tree is missing; run --prepare",
        ));
    }
    let (pw_digest, pw_bytes, pw_files) =
        tree_manifest(&pw).map_err(|e| PrepareError::new(&e.code, e.message))?;
    if pw_digest != lock.playwright.tree_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            format!(
                "playwright-runtime digest mismatch expected={} actual={}",
                lock.playwright.tree_sha256, pw_digest
            ),
        ));
    }
    if pw_bytes != lock.playwright.tree_bytes || pw_files != lock.playwright.tree_files {
        return Err(PrepareError::new(
            "hash_mismatch",
            "playwright-runtime file count/bytes do not match lock",
        ));
    }

    let chrome = seed.join(&lock.chromium.tree_relpath);
    if !chrome.is_dir() {
        return Err(PrepareError::new(
            "missing",
            "pack Chromium chrome-win tree missing; run --prepare",
        ));
    }
    let (chrome_digest, chrome_bytes, chrome_files) =
        crate::runtime_chromium::tree_manifest(&chrome, &lock.target)
            .map_err(|e| PrepareError::new(&e.code, e.message))?;
    if chrome_digest != lock.chromium.tree_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            format!(
                "chromium tree digest mismatch expected={} actual={}",
                lock.chromium.tree_sha256, chrome_digest
            ),
        ));
    }
    if chrome_bytes != lock.chromium.tree_bytes || chrome_files != lock.chromium.tree_files {
        return Err(PrepareError::new(
            "hash_mismatch",
            "chromium tree file count/bytes do not match lock",
        ));
    }
    let exe = chrome.join(&lock.chromium.executable_relpath);
    let exe_bytes = fs::read(&exe).map_err(|_| {
        PrepareError::new(
            "missing",
            "pack Chromium chrome.exe missing; run Chromium prepare",
        )
    })?;
    if !looks_like_native_binary(&exe_bytes) {
        return Err(PrepareError::new(
            "not_executable",
            "pack Chromium executable is not a native binary",
        ));
    }
    if sha256_hex(&exe_bytes) != lock.chromium.executable_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            "chrome.exe hash does not match lock",
        ));
    }

    check_worker(seed, &ctx.worker_src_dir)?;
    check_manifest_matches_lock(seed, lock)?;
    check_no_unexpected_files(seed)?;
    if import_probe_status(lock) == "passed" {
        check_module_import(&node, &pw)?;
    }
    Ok(())
}

fn check_worker(seed: &Path, src_dir: &Path) -> Result<(), PrepareError> {
    let worker = seed.join("playwright").join("worker.mjs");
    let src = src_dir.join("server.mjs");
    let worker_bytes =
        fs::read(&worker).map_err(|_| PrepareError::new("missing", "worker entry missing"))?;
    let src_bytes =
        fs::read(&src).map_err(|_| PrepareError::new("missing", "source server.mjs missing"))?;
    if worker_bytes == PLACEHOLDER_WORKER.as_bytes() {
        return Err(PrepareError::new(
            "placeholder",
            "seed worker is still the one-line placeholder",
        ));
    }
    if worker_bytes != src_bytes {
        return Err(PrepareError::new(
            "hash_mismatch",
            "seed worker.mjs drifted from tools/computer-use-browser/server.mjs",
        ));
    }
    let text = String::from_utf8_lossy(&worker_bytes);
    for snippet in FORBIDDEN_WORKER_SNIPPETS {
        if text.contains(snippet) {
            return Err(PrepareError::new(
                "placeholder",
                format!("production worker contains forbidden snippet {snippet}"),
            ));
        }
    }
    let listing_path = seed.join("playwright").join("modules.sha256list");
    let listing = fs::read_to_string(&listing_path)
        .map_err(|_| PrepareError::new("missing", "modules.sha256list missing"))?;
    for name in WORKER_FILES {
        let dest = seed.join("playwright").join(name);
        let source = src_dir.join(name);
        let dest_bytes = fs::read(&dest).map_err(|_| {
            PrepareError::new("missing_sibling", format!("worker sibling missing {name}"))
        })?;
        let source_bytes = fs::read(&source).map_err(|_| {
            PrepareError::new(
                "missing_sibling",
                format!("worker sibling source missing {name}"),
            )
        })?;
        if dest_bytes != source_bytes {
            return Err(PrepareError::new(
                "hash_mismatch",
                format!("worker sibling drifted {name}"),
            ));
        }
        if name.ends_with(".test.mjs") {
            return Err(PrepareError::new(
                "extra_file",
                format!("test file packed {name}"),
            ));
        }
        let line = format!("{name}\t{}\t{}", dest_bytes.len(), sha256_hex(&dest_bytes));
        if !listing.contains(&line) {
            return Err(PrepareError::new(
                "missing_sibling",
                format!("modules listing missing {name}"),
            ));
        }
    }
    Ok(())
}

fn check_manifest_matches_lock(seed: &Path, lock: &RuntimeLock) -> Result<(), PrepareError> {
    let spec: serde_json::Value = serde_json::from_slice(
        &fs::read(seed.join("manifest.json"))
            .map_err(|_| PrepareError::new("missing", "seed manifest.json missing"))?,
    )
    .map_err(|e| PrepareError::new("schema", e.to_string()))?;
    let target =
        RuntimeTarget::parse(&lock.target).map_err(|e| PrepareError::new(&e.code, e.message))?;
    for (id, path) in [
        ("js-runtime", target.node_relpath()),
        ("chromium", lock.chromium.tree_relpath.as_str()),
    ] {
        if spec["components"][id]["arch"] != target.arch()
            || spec["components"][id]["relpath"] != path
        {
            return Err(PrepareError::new(
                "arch_mismatch",
                "seed manifest target/layout does not match source lock",
            ));
        }
    }
    if spec["components"]["chromium"]["sha256"] != lock.chromium.tree_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            "manifest chromium digest does not match lock (must equal live tree, not prestored)",
        ));
    }
    if spec["components"]["playwright-runtime"]["sha256"] != lock.playwright.tree_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            "manifest playwright-runtime digest does not match lock",
        ));
    }
    if spec["components"]["js-runtime"]["sha256"] != lock.js_runtime.executable_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            "manifest node digest does not match lock",
        ));
    }
    Ok(())
}

fn check_no_unexpected_files(seed: &Path) -> Result<(), PrepareError> {
    if seed.join("chromium").join("chromium-win64.zip").exists() {
        return Err(PrepareError::new(
            "extra_file",
            "Chromium zip must not remain in the published seed",
        ));
    }
    Ok(())
}

fn cache_path(cache_dir: &Path, sha256: &str) -> PathBuf {
    cache_dir.join("sha256").join(sha256)
}

fn ensure_archive(
    ctx: &PrepareContext,
    lock: &RuntimeLock,
    url: &str,
    sha256: &str,
    bytes: u64,
    bootstrap: &[PathBuf],
) -> Result<PathBuf, PrepareError> {
    let dest = cache_path(&ctx.cache_dir, sha256);
    if dest.is_file() {
        let actual = sha256_file(&dest)?;
        if actual == sha256 && file_len(&dest)? == bytes {
            return Ok(dest);
        }
        let _ = fs::remove_file(&dest);
    }
    for candidate in bootstrap {
        if candidate.is_file() {
            let actual = sha256_file(candidate)?;
            if actual == sha256 && file_len(candidate)? == bytes {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| PrepareError::new("io", e.to_string()))?;
                }
                fs::copy(candidate, &dest).map_err(|e| PrepareError::new("io", e.to_string()))?;
                return Ok(dest);
            }
        }
    }
    download_archive(lock, url, sha256, bytes, &dest)?;
    Ok(dest)
}

fn download_archive(
    lock: &RuntimeLock,
    url: &str,
    sha256: &str,
    bytes: u64,
    dest: &Path,
) -> Result<(), PrepareError> {
    let parsed = reqwest::Url::parse(url).map_err(|e| PrepareError::new("path", e.to_string()))?;
    let host = parsed.host_str().unwrap_or("");
    if !host_allowed(lock, host) {
        return Err(PrepareError::new(
            "path",
            format!("download host {host} is not allowed"),
        ));
    }
    let allowed = lock.allowed_redirect_hosts.clone();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            let host = attempt.url().host_str().unwrap_or("").to_string();
            if allowed.iter().any(|h| h.eq_ignore_ascii_case(&host)) {
                if attempt.previous().len() > 5 {
                    attempt.error("too many redirects")
                } else {
                    attempt.follow()
                }
            } else {
                attempt.error(format!("redirect host {host} is not allowed"))
            }
        }))
        .build()
        .map_err(|e| PrepareError::new("io", e.to_string()))?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| PrepareError::new("io", e.to_string()))?;
    if !response.status().is_success() {
        return Err(PrepareError::new(
            "io",
            format!("download {} failed {}", url, response.status()),
        ));
    }
    if let Some(len) = response.content_length() {
        if len != bytes {
            return Err(PrepareError::new(
                "hash_mismatch",
                format!("content-length {len} != lock {bytes}"),
            ));
        }
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| PrepareError::new("io", e.to_string()))?;
    }
    let tmp = dest.with_extension("tmp");
    let mut file = fs::File::create(&tmp).map_err(|e| PrepareError::new("io", e.to_string()))?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = response
            .read(&mut buf)
            .map_err(|e| PrepareError::new("truncated", e.to_string()))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > bytes {
            let _ = fs::remove_file(&tmp);
            return Err(PrepareError::new(
                "oversize",
                "download exceeded lock bytes",
            ));
        }
        file.write_all(&buf[..n])
            .map_err(|e| PrepareError::new("io", e.to_string()))?;
    }
    drop(file);
    if total != bytes {
        let _ = fs::remove_file(&tmp);
        return Err(PrepareError::new(
            "truncated",
            format!("download {total} bytes, lock {bytes}"),
        ));
    }
    let actual = sha256_file(&tmp)?;
    if actual != sha256 {
        let _ = fs::remove_file(&tmp);
        return Err(PrepareError::new(
            "hash_mismatch",
            "downloaded archive hash mismatch",
        ));
    }
    fs::rename(&tmp, dest).map_err(|e| PrepareError::new("io", e.to_string()))?;
    Ok(())
}

pub fn windows_bundle_contains_zip_glob(globs: &[&str]) -> bool {
    globs.iter().any(|g| {
        let n = g.replace('\\', "/").to_ascii_lowercase();
        n.contains("chromium-win64.zip") || n.contains("seed/chromium/**")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grok-cu-prep-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(super) fn gzip_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            header.set_entry_type(tar::EntryType::Regular);
            builder.append_data(&mut header, *name, *data).unwrap();
        }
        let raw = builder.into_inner().unwrap();
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw).unwrap();
        enc.finish().unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn tiny_lock(
        mut lock: RuntimeLock,
        node_zip: &[u8],
        node_exe: &[u8],
        tgz: &[u8],
        pw_tree: &str,
        pw_files: u32,
        pw_bytes: u64,
        chrome_zip: &[u8],
        chrome_exe: &[u8],
        chrome_tree: &str,
        chrome_files: u32,
        chrome_bytes: u64,
    ) -> RuntimeLock {
        lock.js_runtime.archive_sha256 = sha256_hex(node_zip);
        lock.js_runtime.archive_bytes = node_zip.len() as u64;
        lock.js_runtime.executable_sha256 = sha256_hex(node_exe);
        lock.js_runtime.executable_bytes = node_exe.len() as u64;
        lock.playwright.archive_sha256 = sha256_hex(tgz);
        lock.playwright.archive_bytes = tgz.len() as u64;
        lock.playwright.tree_sha256 = pw_tree.into();
        lock.playwright.tree_files = pw_files;
        lock.playwright.tree_bytes = pw_bytes;
        lock.chromium.archive_sha256 = sha256_hex(chrome_zip);
        lock.chromium.archive_bytes = chrome_zip.len() as u64;
        lock.chromium.tree_sha256 = chrome_tree.into();
        lock.chromium.tree_files = chrome_files;
        lock.chromium.tree_bytes = chrome_bytes;
        lock.chromium.executable_sha256 = sha256_hex(chrome_exe);
        lock
    }

    fn write_worker_src(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("server.mjs"),
            "export const grokComputerUseWorkerEntry = 1;\n",
        )
        .unwrap();
        for name in WORKER_FILES {
            fs::write(dir.join(name), format!("export const {name} = 1;\n")).unwrap();
        }
    }

    #[test]
    fn bundle_resource_map_isolates_windows_seed() {
        let win = bundle_resource_globs("x86_64-windows").unwrap();
        assert!(!windows_bundle_contains_zip_glob(&win));
        assert!(win.iter().any(|g| g.contains("chrome-win")));
        for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
            let resources = bundle_resource_globs(target).unwrap();
            assert!(resources.iter().any(|g| g.contains("chrome-mac")));
            assert!(!resources.iter().any(|g| g.contains("chrome-win")));
        }
        let linux_resources = bundle_resource_globs("x86_64-unknown-linux-gnu").unwrap();
        assert!(linux_resources.iter().any(|g| g.contains("chrome-linux")));
        assert!(!linux_resources.iter().any(|g| g.contains("chrome-win")));
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let base = fs::read_to_string(repo.join("src-tauri/tauri.conf.json")).unwrap();
        assert!(
            !base.contains("resources/computer-use/**/*"),
            "base tauri.conf.json must not glob all computer-use resources"
        );
        let windows = fs::read_to_string(repo.join("src-tauri/tauri.windows.conf.json")).unwrap();
        assert!(
            windows.contains("resources/computer-use/seed/chromium/chrome-win"),
            "{windows}"
        );
        assert!(!windows.contains("chromium-win64.zip"), "{windows}");
        let macos = fs::read_to_string(repo.join("src-tauri/tauri.macos.conf.json")).unwrap();
        let linux = fs::read_to_string(repo.join("src-tauri/tauri.linux.conf.json")).unwrap();
        assert!(macos.contains("resources/computer-use/seed/chromium/chrome-mac"));
        assert!(linux.contains("resources/computer-use/seed/chromium/chrome-linux"));
    }

    #[test]
    fn build_hooks_require_prepare_check() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pkg = fs::read_to_string(repo.join("package.json")).unwrap();
        assert!(
            pkg.contains("prepare-computer-use-runtime.mjs")
                || pkg.contains("tauri-before-build.mjs")
        );
        let tauri = fs::read_to_string(repo.join("src-tauri/tauri.conf.json")).unwrap();
        assert!(
            tauri.contains("tauri-before-build.mjs")
                || tauri.contains("prepare-computer-use-runtime.mjs"),
            "{tauri}"
        );
        assert!(
            !tauri.contains("\"beforeBuildCommand\": \"pnpm build:ui\""),
            "Windows bundle must not skip runtime prepare"
        );
        let build_ui = pkg.contains("\"build:ui\": \"tsc -b && vite build\"");
        assert!(build_ui, "pnpm build:ui must stay UI-only");
        let local = fs::read_to_string(repo.join("scripts/build-local.sh")).unwrap();
        assert!(
            local.contains("prepare-computer-use-runtime.mjs")
                || local.contains("tauri-before-build.mjs"),
            "{local}"
        );
        let ci = fs::read_to_string(repo.join(".github/workflows/ci.yml")).unwrap();
        let release = fs::read_to_string(repo.join(".github/workflows/release.yml")).unwrap();
        assert!(ci.contains("prepare-computer-use-runtime.mjs"), "{ci}");
        assert!(
            release.contains("prepare-computer-use-runtime.mjs"),
            "{release}"
        );
    }

    #[test]
    fn tiny_prepare_is_deterministic_check_is_readonly_and_chromium_tamper_fails() {
        let root = temp_dir();
        let worker = root.join("worker-src");
        write_worker_src(&worker);
        let target = RuntimeTarget::parse(&crate::runtime::host_arch()).unwrap();
        let source_lock = fixture::source_lock(target);
        let official_node = crate::runtime_test_seed::node();
        let node_exe = fs::read(&official_node).expect("prepared native Node for import probe");
        let node_zip = fixture::node_archive(&source_lock, &node_exe);
        let pkg = br#"{"name":"playwright-core","version":"1.48.0"}"#;
        // Import-contract fixture only: no browser is launched by prepare.
        let module = br#"export const chromium = {
            name: () => 'chromium', launch() {}, launchPersistentContext() {}, connectOverCDP() {}
        };"#;
        let tgz = gzip_tar(&[("package/package.json", pkg), ("package/index.mjs", module)]);
        let chrome_exe = fixture::chromium_executable(target);
        let credits = b"credits";
        let chrome_zip = fixture::chromium_archive(&source_lock, chrome_exe);
        let cache = root.join("cache");
        fs::create_dir_all(cache.join("sha256")).unwrap();
        // Materialize playwright tree once to learn digest.
        let pw_tmp = root.join("pw-tmp");
        let tree = playwright_materialize::materialize_playwright_tgz(
            &tgz,
            &pw_tmp,
            Some(&sha256_hex(&tgz)),
        )
        .unwrap();
        let chrome_tmp = root.join("chrome-tmp");
        let mut policy = ZipPolicy::chromium();
        policy.expected_root = target.chromium_root().into();
        if crate::runtime_chromium::is_macos(target.arch()) {
            policy.allowed_symlinks = crate::runtime_chromium::macos_links();
        }
        extract_zip(&chrome_zip, &chrome_tmp, &policy, None).unwrap();
        let (chrome_digest, chrome_bytes, chrome_files) =
            crate::runtime_chromium::tree_manifest(&chrome_tmp, target.arch()).unwrap();
        let lock = tiny_lock(
            source_lock,
            &node_zip,
            &node_exe,
            &tgz,
            &tree.listing_sha256,
            tree.file_count,
            tree.total_bytes,
            &chrome_zip,
            chrome_exe,
            &chrome_digest,
            chrome_files,
            chrome_bytes,
        );
        let lock_path = root.join("lock.json");
        fs::write(&lock_path, serde_json::to_vec_pretty(&lock).unwrap()).unwrap();
        fs::write(
            cache.join("sha256").join(&lock.js_runtime.archive_sha256),
            &node_zip,
        )
        .unwrap();
        fs::write(
            cache.join("sha256").join(&lock.playwright.archive_sha256),
            &tgz,
        )
        .unwrap();
        fs::write(
            cache.join("sha256").join(&lock.chromium.archive_sha256),
            &chrome_zip,
        )
        .unwrap();

        let ctx = PrepareContext {
            repo_root: root.clone(),
            seed_dir: root.join("seed"),
            cache_dir: cache,
            lock_path,
            worker_src_dir: worker,
            target: target.arch().into(),
        };
        let first = prepare(&ctx).expect("first prepare");
        let second = prepare(&ctx).expect("second prepare");
        assert_eq!(
            first.import_probe, "passed",
            "native test must execute the import"
        );
        assert_eq!(first.tree_sha256, second.tree_sha256);
        assert_eq!(first.manifest_sha256, second.manifest_sha256);

        let before = snapshot_seed(&ctx.seed_dir).unwrap();
        check(&ctx).expect("check green");
        let after = snapshot_seed(&ctx.seed_dir).unwrap();
        assert_eq!(before, after, "check must be read-only");

        let chromium = ctx.seed_dir.join(&lock.chromium.tree_relpath);
        let credits_path = chromium.join("CREDITS.html");
        fs::write(&credits_path, b"tampered-credits").unwrap();
        let err = check(&ctx).unwrap_err();
        assert_eq!(err.code, "hash_mismatch", "{err:?}");
        fs::write(&credits_path, credits).unwrap();
        check(&ctx).expect("restored credits");

        let exe = chromium.join(target.chromium_executable());
        let original_exe = fs::read(&exe).unwrap();
        fs::write(&exe, b"MZ\x90\x00tampered-exe").unwrap();
        let err = check(&ctx).unwrap_err();
        assert!(
            err.code == "hash_mismatch" || err.code == "not_executable",
            "{err:?}"
        );
        fs::write(&exe, original_exe).unwrap();

        fs::remove_file(&credits_path).unwrap();
        let err = check(&ctx).unwrap_err();
        assert!(
            err.code == "hash_mismatch" || err.code == "missing",
            "{err:?}"
        );

        fs::write(&credits_path, credits).unwrap();
        fs::write(chromium.join("extra.txt"), b"nope").unwrap();
        let err = check(&ctx).unwrap_err();
        assert_eq!(
            err.code, "hash_mismatch",
            "extra chromium file must fail tree check: {err:?}"
        );

        fs::write(ctx.seed_dir.join("chromium/chromium-win64.zip"), b"zip").unwrap();
        let hits = package_deny_list_violations(&ctx.seed_dir);
        assert!(
            hits.iter().any(|h| h.contains("chromium-win64.zip")),
            "{hits:?}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_keeps_previous_seed_when_staging_check_fails() {
        let root = temp_dir();
        let seed = root.join("seed");
        fs::create_dir_all(&seed).unwrap();
        fs::write(seed.join("keep.txt"), b"healthy").unwrap();
        let ctx = PrepareContext {
            repo_root: root.clone(),
            seed_dir: seed.clone(),
            cache_dir: root.join("cache"),
            lock_path: root.join("missing-lock.json"),
            worker_src_dir: root.join("worker"),
            target: "x86_64-windows".into(),
        };
        let err = prepare(&ctx).unwrap_err();
        assert_eq!(err.code, "missing", "{err:?}");
        assert_eq!(fs::read(seed.join("keep.txt")).unwrap(), b"healthy");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generated_runtime_seed_is_gitignored() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let gi = fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert!(
            gi.contains("src-tauri/resources/computer-use/seed/"),
            "generated seed must be gitignored"
        );
        assert!(
            gi.contains("src-tauri/resources/computer-use/.cu-mutation.lock"),
            "mutation lock must be gitignored"
        );
        assert!(repo
            .join("src-tauri/resources/computer-use/windows-x64.lock.json")
            .is_file());
        let tracked_hint = gi
            .lines()
            .any(|l| l.trim() == "src-tauri/resources/computer-use/windows-x64.lock.json");
        assert!(!tracked_hint, "lock file must not be gitignored");
    }

    #[test]
    fn gate_fails_on_wrong_target() {
        let ctx = PrepareContext {
            repo_root: PathBuf::from("."),
            seed_dir: PathBuf::from("missing"),
            cache_dir: PathBuf::from("missing"),
            lock_path: PathBuf::from("missing"),
            worker_src_dir: PathBuf::from("missing"),
            target: "riscv64-unknown-linux-gnu".into(),
        };
        assert_eq!(prepare(&ctx).unwrap_err().code, "unsupported_target");
        assert_eq!(check(&ctx).unwrap_err().code, "unsupported_target");
    }
}
