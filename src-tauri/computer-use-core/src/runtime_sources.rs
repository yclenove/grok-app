//! Source-pair audit shares the production materializer but never publishes a
//! seed/manifest, prepares a worker/browser, or grants Computer Use authority.

use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceReport {
    pub target: String,
    pub js_runtime_version: String,
    pub playwright_version: String,
    pub node_sha256: String,
    pub node_license_sha256: String,
    pub playwright_tree_sha256: String,
    pub playwright_files: u32,
    pub playwright_bytes: u64,
    pub import_probe: String,
    /// This audit cannot be used as a full prepare/check/bundle receipt.
    pub runtime_prepared: bool,
}

pub fn verify_sources(ctx: &PrepareContext) -> Result<SourceReport, PrepareError> {
    if !supported_prepare_target(&ctx.target) {
        return Err(PrepareError::new(
            "unsupported_target",
            "unsupported source audit target",
        ));
    }
    let lock = load_lock(&ctx.lock_path).map_err(|e| PrepareError::new(&e.code, e.message))?;
    require_lock_target(ctx, &lock)?;
    let owner = new_owner(
        &ctx.seed_dir,
        &ctx.repo_root,
        format!("sources-{}", uuid::Uuid::new_v4()),
    );
    let _guard = try_acquire_exclusive(&ctx.seed_dir, owner.clone()).map_err(mutation_err)?;
    let parent = ctx
        .seed_dir
        .parent()
        .ok_or_else(|| PrepareError::new("path", "seed must have a parent"))?;
    let staging = parent.join(format!(".source-audit-{}", uuid::Uuid::new_v4()));
    write_staging_owner(&staging, &owner).map_err(mutation_err)?;
    let result = (|| {
        materialize(&staging, ctx, &lock).map_err(|e| e.at("materialize sources"))?;
        let (node, pw) = check_files(&staging, &lock).map_err(|e| e.at("validate sources"))?;
        if import_probe_status(&lock) == "passed" {
            check_module_import(&node, &pw).map_err(|e| e.at("import sources"))?;
        }
        Ok(SourceReport {
            target: lock.target.clone(),
            js_runtime_version: lock.js_runtime.version.clone(),
            playwright_version: lock.playwright.version.clone(),
            node_sha256: lock.js_runtime.executable_sha256.clone(),
            node_license_sha256: sha256_file(&staging.join("bin/LICENSE"))?,
            playwright_tree_sha256: lock.playwright.tree_sha256.clone(),
            playwright_files: lock.playwright.tree_files,
            playwright_bytes: lock.playwright.tree_bytes,
            import_probe: import_probe_status(&lock).into(),
            runtime_prepared: false,
        })
    })();
    finish_staging(result, remove_staging(&staging, &owner))
}

pub(super) fn materialize(
    staging: &Path,
    ctx: &PrepareContext,
    lock: &RuntimeLock,
) -> Result<(), PrepareError> {
    let node_archive = ensure_archive(
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
    let pw_archive = ensure_archive(
        ctx,
        lock,
        &lock.playwright.url,
        &lock.playwright.archive_sha256,
        lock.playwright.archive_bytes,
        &[ctx.seed_dir.join(playwright_archive_relpath())],
    )?;
    let node_bytes = fs::read(node_archive).map_err(|e| PrepareError::new("io", e.to_string()))?;
    let target =
        RuntimeTarget::parse(&lock.target).map_err(|e| PrepareError::new(&e.code, e.message))?;
    let node_dest = staging.join("bin");
    if target == RuntimeTarget::WindowsX64 {
        let mut policy = ZipPolicy::node_win_x64();
        policy.expected_root = lock.js_runtime.archive_root.clone();
        extract_zip(
            &node_bytes,
            &node_dest,
            &policy,
            Some(&lock.js_runtime.archive_sha256),
        )
        .map_err(|e| PrepareError::new(&e.code, e.message))?;
    } else {
        node_archive::extract(
            &node_bytes,
            &node_dest,
            &lock.js_runtime.archive_root,
            &lock.js_runtime.archive_sha256,
        )?;
    }
    let tgz = fs::read(&pw_archive).map_err(|e| PrepareError::new("io", e.to_string()))?;
    let pw_dest = staging.join(&lock.playwright.tree_relpath);
    let tree = playwright_materialize::materialize_playwright_tgz(
        &tgz,
        &pw_dest,
        Some(&lock.playwright.archive_sha256),
    )
    .map_err(|e| PrepareError::new(&e.code, e.message))?;
    fs::copy(pw_archive, staging.join(playwright_archive_relpath()))
        .map_err(|e| PrepareError::new("io", e.to_string()))?;
    fs::write(
        staging.join("playwright/playwright-runtime.sha256list"),
        tree_listing(&pw_dest).map_err(|e| PrepareError::new(&e.code, e.message))?,
    )
    .map_err(|e| PrepareError::new("io", e.to_string()))?;
    if tree.listing_sha256 != lock.playwright.tree_sha256 {
        return Err(PrepareError::new(
            "hash_mismatch",
            "playwright-runtime tree digest does not match lock",
        ));
    }
    Ok(())
}

pub(super) fn check_files(
    root: &Path,
    lock: &RuntimeLock,
) -> Result<(PathBuf, PathBuf), PrepareError> {
    let target =
        RuntimeTarget::parse(&lock.target).map_err(|e| PrepareError::new(&e.code, e.message))?;
    let node = root.join(target.node_relpath());
    let bytes = fs::read(&node)
        .map_err(|_| PrepareError::new("missing", "pack Node executable missing"))?;
    if !looks_like_native_binary(&bytes) {
        return Err(PrepareError::new(
            "not_executable",
            "pack Node is not a native binary",
        ));
    }
    if sha256_hex(&bytes) != lock.js_runtime.executable_sha256
        || bytes.len() as u64 != lock.js_runtime.executable_bytes
    {
        return Err(PrepareError::new(
            "hash_mismatch",
            "Node executable hash/size does not match lock",
        ));
    }
    let license = fs::symlink_metadata(root.join("bin/LICENSE"))
        .map_err(|_| PrepareError::new("missing", "Node LICENSE is missing"))?;
    if license.file_type().is_symlink() {
        return Err(PrepareError::new(
            "symlink",
            "Node LICENSE must be a regular file",
        ));
    }
    if !license.is_file() || license.len() == 0 {
        return Err(PrepareError::new(
            "missing",
            "Node LICENSE is missing or empty",
        ));
    }
    let tgz = root.join(playwright_archive_relpath());
    let tgz_bytes = fs::read(&tgz)
        .map_err(|_| PrepareError::new("missing", "Playwright archive is missing"))?;
    if sha256_hex(&tgz_bytes) != lock.playwright.archive_sha256
        || tgz_bytes.len() as u64 != lock.playwright.archive_bytes
    {
        return Err(PrepareError::new(
            "hash_mismatch",
            "Playwright archive hash/size does not match lock",
        ));
    }
    let pw = root.join(&lock.playwright.tree_relpath);
    if !pw.is_dir() {
        return Err(PrepareError::new(
            "missing",
            "materialized Playwright tree is missing",
        ));
    }
    let (digest, bytes, files) =
        tree_manifest(&pw).map_err(|e| PrepareError::new(&e.code, e.message))?;
    if digest != lock.playwright.tree_sha256
        || bytes != lock.playwright.tree_bytes
        || files != lock.playwright.tree_files
    {
        return Err(PrepareError::new(
            "hash_mismatch",
            "Playwright tree digest/count/bytes do not match lock",
        ));
    }
    Ok((node, pw))
}

#[cfg(test)]
#[path = "runtime_sources_tests.rs"]
mod tests;
