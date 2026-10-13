use super::*;
use crate::runtime_lock::{load_lock, validate_lock, RuntimeLock, RuntimeTarget};
use std::path::PathBuf;

const TARGETS: [RuntimeTarget; 4] = [
    RuntimeTarget::WindowsX64,
    RuntimeTarget::MacosArm64,
    RuntimeTarget::MacosX64,
    RuntimeTarget::LinuxX64,
];

fn resources() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/computer-use")
}

fn lock(target: RuntimeTarget) -> RuntimeLock {
    load_lock(&resources().join(target.lock_name())).unwrap()
}

#[test]
fn all_four_source_locks_and_pack_catalog_match_the_compiled_pins() {
    for target in TARGETS {
        let source = lock(target);
        assert_eq!(source.target, target.arch());
        assert_eq!(source.playwright.archive_sha256, PLAYWRIGHT_CORE_TGZ_SHA256);
        assert_eq!(source.js_runtime.version, REQUIRED_JS_RUNTIME_VERSION);
        assert_eq!(source.playwright.version, REQUIRED_PLAYWRIGHT_CORE_VERSION);
        assert_eq!(source.chromium.revision, REQUIRED_CHROMIUM_REVISION);
        assert_eq!(source.chromium.browser_version, REQUIRED_CHROMIUM_VERSION);
        assert!(source.playwright.url.ends_with(&playwright_archive_name()));
    }
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(resources().join("pack-targets.json")).unwrap())
            .unwrap();
    assert_eq!(catalog["jsRuntimeVersion"], REQUIRED_JS_RUNTIME_VERSION);
    assert_eq!(
        catalog["playwrightCoreVersion"],
        REQUIRED_PLAYWRIGHT_CORE_VERSION
    );
    let actual: std::collections::BTreeSet<_> = catalog["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["arch"].as_str().unwrap())
        .collect();
    let expected = TARGETS.iter().map(|target| target.arch()).collect();
    assert_eq!(actual, expected);
}

#[test]
fn archive_and_seed_paths_derive_from_the_same_playwright_pin() {
    assert_eq!(
        playwright_archive_relpath(),
        format!("playwright/playwright-core-{REQUIRED_PLAYWRIGHT_CORE_VERSION}.tgz")
    );
    assert!(
        crate::playwright_materialize::official_tgz_path().ends_with(playwright_archive_relpath())
    );
    assert_eq!(
        crate::archive_zip::ZipPolicy::node_win_x64().expected_root,
        RuntimeTarget::WindowsX64.node_archive_root()
    );
}

#[test]
fn a_json_version_edit_does_not_change_any_compiled_runtime_pin() {
    for target in TARGETS {
        let source = lock(target);
        for field in ["node", "playwright", "revision", "browser"] {
            let mut changed = source.clone();
            match field {
                "node" => changed.js_runtime.version = "0.0.1".into(),
                "playwright" => changed.playwright.version = "0.0.1".into(),
                "revision" => changed.chromium.revision = "0".into(),
                "browser" => changed.chromium.browser_version = "0.0.1".into(),
                _ => unreachable!(),
            }
            assert_eq!(
                validate_lock(&changed).unwrap_err().code,
                "version_mismatch",
                "{} / {field}",
                target.arch()
            );
        }
    }
}

#[test]
fn wrong_layout_is_rejected_for_every_target_and_component() {
    for target in TARGETS {
        let source = lock(target);
        for field in [
            "node-root",
            "node-exe",
            "pw-tree",
            "chrome-root",
            "chrome-tree",
            "chrome-exe",
        ] {
            let mut changed = source.clone();
            match field {
                "node-root" => changed.js_runtime.archive_root = "other-architecture".into(),
                "node-exe" => changed.js_runtime.executable_relpath = "other/node".into(),
                "pw-tree" => changed.playwright.tree_relpath = "other/playwright-core".into(),
                "chrome-root" => changed.chromium.archive_root = "other-browser".into(),
                "chrome-tree" => changed.chromium.tree_relpath = "other/chrome".into(),
                "chrome-exe" => changed.chromium.executable_relpath = "other/chrome".into(),
                _ => unreachable!(),
            }
            assert_eq!(
                validate_lock(&changed).unwrap_err().code,
                "layout_mismatch",
                "{} / {field}",
                target.arch()
            );
        }
    }
}

#[test]
fn swapping_architectures_does_not_reinterpret_a_source_lock() {
    for from in TARGETS {
        for to in TARGETS {
            if from == to {
                continue;
            }
            let mut changed = lock(from);
            changed.target = to.arch().into();
            assert_eq!(validate_lock(&changed).unwrap_err().code, "layout_mismatch");
        }
    }
}

#[test]
fn macos_framework_current_alias_uses_the_compiled_browser_version() {
    let links = crate::runtime_chromium::macos_links();
    assert_eq!(links.len(), 5);
    assert_eq!(
        links
            .iter()
            .find(|(path, _)| path.ends_with("/Versions/Current"))
            .unwrap()
            .1,
        REQUIRED_CHROMIUM_VERSION
    );
}
