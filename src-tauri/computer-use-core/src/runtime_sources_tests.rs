//! Archive/contract fixtures are not browser acceptance. Only the native test
//! uses a genuine pack Node; cross-target reports must never claim execution.
use super::*;
use crate::archive_zip::write_test_zip;
use crate::runtime::REQUIRED_PLAYWRIGHT_CORE_VERSION;
use crate::runtime_prepare::{fixture, tests::gzip_tar};

const MODULE: &str = "export const chromium = { name: () => 'chromium', launch() { throw new Error('must not launch'); }, launchPersistentContext() {}, connectOverCDP() {} };";

struct Sources {
    root: PathBuf,
    ctx: PrepareContext,
    lock: RuntimeLock,
}

impl Sources {
    fn new(target: RuntimeTarget, node: &[u8]) -> Self {
        let root = std::env::temp_dir().join(format!("grok-cu-sources-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("cache/sha256")).unwrap();
        fs::create_dir(root.join("seed")).unwrap();
        fs::write(root.join("seed/keep.txt"), b"previous usable seed").unwrap();
        let ctx = PrepareContext {
            repo_root: root.clone(),
            seed_dir: root.join("seed"),
            cache_dir: root.join("cache"),
            lock_path: root.join("lock.json"),
            worker_src_dir: root.join("absent-worker"),
            target: target.arch().into(),
        };
        let mut lock = fixture::source_lock(target);
        // A mistaken browser fetch must fail locally, never contact a network.
        lock.allowed_redirect_hosts.clear();
        let mut sources = Self { root, ctx, lock };
        sources.set_node(node, Some(b"fixture Node license"));
        sources.set_module(MODULE);
        sources
    }

    fn cross() -> Self {
        let target = if crate::runtime::host_arch() == RuntimeTarget::LinuxX64.arch() {
            RuntimeTarget::WindowsX64
        } else {
            RuntimeTarget::LinuxX64
        };
        Self::new(target, fixture::chromium_executable(target))
    }

    fn write_lock(&self) {
        fs::write(&self.ctx.lock_path, serde_json::to_vec(&self.lock).unwrap()).unwrap();
    }

    fn cache(&self, digest: &str, bytes: &[u8]) {
        fs::write(self.ctx.cache_dir.join("sha256").join(digest), bytes).unwrap();
    }

    fn set_node(&mut self, bytes: &[u8], license: Option<&[u8]>) {
        let executable = format!(
            "{}/{}",
            self.lock.js_runtime.archive_root, self.lock.js_runtime.executable_relpath
        );
        let license_path = format!("{}/LICENSE", self.lock.js_runtime.archive_root);
        let mut entries = vec![(executable.as_str(), bytes)];
        if let Some(license) = license {
            entries.push((license_path.as_str(), license));
        }
        let archive = if self.lock.target == RuntimeTarget::WindowsX64.arch() {
            write_test_zip(&entries)
        } else {
            gzip_tar(&entries)
        };
        self.lock.js_runtime.archive_sha256 = sha256_hex(&archive);
        self.lock.js_runtime.archive_bytes = archive.len() as u64;
        self.lock.js_runtime.executable_sha256 = sha256_hex(bytes);
        self.lock.js_runtime.executable_bytes = bytes.len() as u64;
        self.cache(&self.lock.js_runtime.archive_sha256, &archive);
        self.write_lock();
    }

    fn set_module(&mut self, module: &str) {
        let package = serde_json::to_vec(&serde_json::json!({
            "name": "playwright-core", "version": REQUIRED_PLAYWRIGHT_CORE_VERSION,
        }))
        .unwrap();
        let tgz = gzip_tar(&[
            ("package/package.json", &package),
            ("package/index.mjs", module.as_bytes()),
        ]);
        let tree_dir = self.root.join("tree-fixture");
        let tree = playwright_materialize::materialize_playwright_tgz(
            &tgz,
            &tree_dir,
            Some(&sha256_hex(&tgz)),
        )
        .unwrap();
        self.lock.playwright.archive_sha256 = sha256_hex(&tgz);
        self.lock.playwright.archive_bytes = tgz.len() as u64;
        self.lock.playwright.tree_sha256 = tree.listing_sha256;
        self.lock.playwright.tree_files = tree.file_count;
        self.lock.playwright.tree_bytes = tree.total_bytes;
        self.cache(&self.lock.playwright.archive_sha256, &tgz);
        self.write_lock();
    }

    fn verify(&self) -> Result<SourceReport, PrepareError> {
        let before = snapshot_seed(&self.ctx.seed_dir).unwrap();
        let result = verify_sources(&self.ctx);
        assert_eq!(
            snapshot_seed(&self.ctx.seed_dir).unwrap(),
            before,
            "audit must not change the seed"
        );
        for entry in fs::read_dir(&self.root).unwrap() {
            assert!(
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".source-audit-"),
                "owned staging and marker must retire on success/error"
            );
        }
        assert!(!self.ctx.worker_src_dir.exists());
        assert!(!self.ctx.seed_dir.join("manifest.json").exists());
        assert!(!self.ctx.seed_dir.join("chromium").exists());
        result
    }
}

impl Drop for Sources {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn native_sources_import_without_browser_worker_or_seed_publication() {
    let target = RuntimeTarget::parse(&crate::runtime::host_arch()).unwrap();
    let sources = Sources::new(target, &fs::read(crate::runtime_test_seed::node()).unwrap());
    let report = sources.verify().unwrap();
    assert_eq!(report.import_probe, "passed");
    assert!(!report.runtime_prepared);
    assert_eq!(
        report.node_sha256,
        sources.lock.js_runtime.executable_sha256
    );
    assert_eq!(
        report.node_license_sha256,
        sha256_hex(b"fixture Node license")
    );
    assert_eq!(
        report.playwright_tree_sha256,
        sources.lock.playwright.tree_sha256
    );
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["runtimePrepared"], false);
    assert_eq!(json["playwrightFiles"], 2);
    assert_eq!(json["playwrightBytes"], sources.lock.playwright.tree_bytes);
    assert_eq!(check(&sources.ctx).unwrap_err().code, "missing");
}

#[test]
fn native_import_error_retires_staging_without_forging_a_pass() {
    let target = RuntimeTarget::parse(&crate::runtime::host_arch()).unwrap();
    let mut sources = Sources::new(target, &fs::read(crate::runtime_test_seed::node()).unwrap());
    for module in [
        "process.exit(0);",
        "export const chromium = {};",
        "import './absent.mjs';",
    ] {
        sources.set_module(module);
        let error = sources.verify().unwrap_err();
        assert_eq!(error.code, "import_failed");
        assert!(error.message.starts_with("import sources:"));
    }
}

#[test]
fn cross_target_sources_never_claim_native_import_or_prepared_runtime() {
    for target in [
        RuntimeTarget::WindowsX64,
        RuntimeTarget::LinuxX64,
        RuntimeTarget::MacosArm64,
        RuntimeTarget::MacosX64,
    ] {
        if target.arch() == crate::runtime::host_arch() {
            continue;
        }
        let sources = Sources::new(target, fixture::chromium_executable(target));
        let report = sources.verify().unwrap();
        assert_eq!(report.import_probe, "not_run_cross_target");
        assert_eq!(report.target, target.arch());
        assert!(!report.runtime_prepared);
    }
}

#[test]
fn source_audit_obeys_the_seed_mutation_lock() {
    let sources = Sources::cross();
    let owner = new_owner(&sources.ctx.seed_dir, &sources.root, "live-test-owner");
    let guard = try_acquire_exclusive(&sources.ctx.seed_dir, owner).unwrap();
    assert_eq!(sources.verify().unwrap_err().code, "busy");
    drop(guard);
    assert!(sources.verify().is_ok());
}

#[test]
fn source_audit_does_not_create_an_absent_seed() {
    let mut sources = Sources::cross();
    sources.ctx.seed_dir = sources.root.join("never-created-seed");
    assert!(sources.verify().is_ok());
    assert!(!sources.ctx.seed_dir.exists());
    assert_eq!(check(&sources.ctx).unwrap_err().code, "missing");
}

#[test]
fn versions_layouts_and_target_mismatch_fail_before_materialization() {
    let mut sources = Sources::cross();
    let original = sources.lock.clone();
    for field in ["node-version", "pw-version", "node-layout", "pw-layout"] {
        sources.lock = original.clone();
        let code = match field {
            "node-version" => {
                sources.lock.js_runtime.version = "0.0.1".into();
                "version_mismatch"
            }
            "pw-version" => {
                sources.lock.playwright.version = "0.0.1".into();
                "version_mismatch"
            }
            "node-layout" => {
                sources.lock.js_runtime.archive_root = "another-arch".into();
                "layout_mismatch"
            }
            _ => {
                sources.lock.playwright.tree_relpath = "another/tree".into();
                "layout_mismatch"
            }
        };
        sources.write_lock();
        assert_eq!(sources.verify().unwrap_err().code, code, "{field}");
    }
    sources.lock = original;
    sources.write_lock();
    sources.ctx.target = RuntimeTarget::MacosX64.arch().into();
    assert_eq!(sources.verify().unwrap_err().code, "arch_mismatch");
    sources.ctx.target = "unsupported-target".into();
    assert_eq!(sources.verify().unwrap_err().code, "unsupported_target");
    fs::remove_file(&sources.ctx.lock_path).unwrap();
    sources.ctx.target = sources.lock.target.clone();
    assert_eq!(sources.verify().unwrap_err().code, "missing");
}

#[test]
fn invalid_node_bytes_sizes_and_absent_or_empty_notices_fail_closed() {
    let mut sources = Sources::cross();
    let node = b"MZ\x90\x00fixture-node";
    sources.set_node(node, Some(b"license"));
    let original = sources.lock.clone();
    for field in ["hash", "bytes"] {
        sources.lock = original.clone();
        if field == "hash" {
            sources.lock.js_runtime.executable_sha256 = "0".repeat(64);
        } else {
            sources.lock.js_runtime.executable_bytes += 1;
        }
        sources.write_lock();
        assert_eq!(sources.verify().unwrap_err().code, "hash_mismatch");
    }
    sources.set_node(b"not-native", Some(b"license"));
    assert_eq!(sources.verify().unwrap_err().code, "not_executable");
    for license in [None, Some(b"".as_slice())] {
        sources.set_node(node, license);
        assert_eq!(sources.verify().unwrap_err().code, "missing");
    }
}

#[test]
fn playwright_tree_digest_count_and_bytes_are_all_enforced() {
    let mut sources = Sources::cross();
    let original = sources.lock.clone();
    for field in ["digest", "count", "bytes"] {
        sources.lock = original.clone();
        match field {
            "digest" => sources.lock.playwright.tree_sha256 = "0".repeat(64),
            "count" => sources.lock.playwright.tree_files += 1,
            _ => sources.lock.playwright.tree_bytes += 1,
        }
        sources.write_lock();
        assert_eq!(
            sources.verify().unwrap_err().code,
            "hash_mismatch",
            "{field}"
        );
    }
}

#[test]
fn corrupt_cached_sources_are_rejected_when_no_trusted_replacement_is_available() {
    let sources = Sources::cross();
    for digest in [
        &sources.lock.js_runtime.archive_sha256,
        &sources.lock.playwright.archive_sha256,
    ] {
        let path = sources.ctx.cache_dir.join("sha256").join(digest);
        let original = fs::read(&path).unwrap();
        fs::write(&path, b"corrupt cached archive").unwrap();
        // Production prepare discards bad cache bytes, then attempts its
        // hash-checked bootstrap/download path. Our offline fixture forbids
        // that host, so this is a typed path rejection, not a hash receipt.
        assert_eq!(sources.verify().unwrap_err().code, "path");
        assert!(!path.exists());
        fs::write(path, original).unwrap();
    }
}

#[test]
fn corrupt_cache_can_only_recover_from_a_hash_checked_bootstrap() {
    let sources = Sources::cross();
    let node_cache = sources
        .ctx
        .cache_dir
        .join("sha256")
        .join(&sources.lock.js_runtime.archive_sha256);
    let node_bytes = fs::read(&node_cache).unwrap();
    let node_bootstrap = sources
        .root
        .join("cache")
        .join(&sources.lock.js_runtime.archive_sha256);
    fs::write(node_bootstrap, &node_bytes).unwrap();
    let pw_cache = sources
        .ctx
        .cache_dir
        .join("sha256")
        .join(&sources.lock.playwright.archive_sha256);
    let pw_bytes = fs::read(&pw_cache).unwrap();
    let pw_bootstrap = sources.ctx.seed_dir.join(playwright_archive_relpath());
    fs::create_dir_all(pw_bootstrap.parent().unwrap()).unwrap();
    fs::write(pw_bootstrap, &pw_bytes).unwrap();
    fs::write(&node_cache, b"corrupt").unwrap();
    fs::write(&pw_cache, b"corrupt").unwrap();
    assert!(sources.verify().is_ok());
    assert_eq!(fs::read(node_cache).unwrap(), node_bytes);
    assert_eq!(fs::read(pw_cache).unwrap(), pw_bytes);
}

#[test]
fn missing_or_changed_materialized_files_keep_typed_errors() {
    let sources = Sources::cross();
    let stage = sources.root.join("checker-fixture");
    materialize(&stage, &sources.ctx, &sources.lock).unwrap();
    check_files(&stage, &sources.lock).unwrap();
    let tgz = stage.join(playwright_archive_relpath());
    let original = fs::read(&tgz).unwrap();
    fs::remove_file(&tgz).unwrap();
    assert_eq!(
        check_files(&stage, &sources.lock).unwrap_err().code,
        "missing"
    );
    fs::write(&tgz, b"tampered").unwrap();
    assert_eq!(
        check_files(&stage, &sources.lock).unwrap_err().code,
        "hash_mismatch"
    );
    fs::write(&tgz, original).unwrap();
    let mut lock = sources.lock.clone();
    lock.playwright.archive_bytes += 1;
    assert_eq!(
        check_files(&stage, &lock).unwrap_err().code,
        "hash_mismatch"
    );
    let pw = stage.join(&sources.lock.playwright.tree_relpath);
    fs::write(pw.join("unexpected-file"), b"extra").unwrap();
    assert_eq!(
        check_files(&stage, &sources.lock).unwrap_err().code,
        "hash_mismatch"
    );
    fs::remove_dir_all(pw).unwrap();
    assert_eq!(
        check_files(&stage, &sources.lock).unwrap_err().code,
        "missing"
    );
}
