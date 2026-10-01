use super::*;
use crate::runtime_lock::RuntimeTarget;

// Contract-only fixture; successful import is not browser acceptance.
const VALID_MODULE: &str = r#"
export const chromium = {
  name: () => 'chromium',
  launch() { throw new Error('fixture must not launch'); },
  launchPersistentContext() { throw new Error('fixture must not launch'); },
  connectOverCDP() { throw new Error('fixture must not connect'); },
};
"#;

fn seed() -> PathBuf {
    crate::runtime_test_seed::path()
}

fn node() -> PathBuf {
    let target = RuntimeTarget::parse(&crate::runtime::host_arch()).unwrap();
    let node = seed().join(target.node_relpath());
    assert!(
        node.is_file(),
        "prepare the native runtime seed before running import tests"
    );
    node
}

fn fixture(source: &str, name: &str, version: &str) -> ProbeDirectory {
    let root = ProbeDirectory::create().unwrap();
    let pw = root.0.join("module with spaces 中文");
    fs::create_dir(&pw).unwrap();
    fs::write(
        pw.join("package.json"),
        serde_json::to_vec(&serde_json::json!({
            "name": name, "version": version,
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(pw.join("index.mjs"), source).unwrap();
    root
}

fn check_fixture(root: &ProbeDirectory) -> Result<(), PrepareError> {
    check_with_timeout(
        &node(),
        &root.0.join("module with spaces 中文"),
        IMPORT_TIMEOUT,
    )
}

#[test]
fn pinned_real_package_loads_chromium_apis() {
    check_playwright_import(
        &node(),
        &seed().join("playwright/node_modules/playwright-core"),
    )
    .unwrap();
}

#[test]
fn valid_module_loads_with_empty_path_and_scrubbed_environment() {
    let source = format!("if (Object.keys(process.env).some(key => !['PATH', 'SYSTEMROOT'].includes(key.toUpperCase()))) throw new Error('inherited environment');\n{VALID_MODULE}");
    let root = fixture(&source, "playwright-core", REQUIRED_PLAYWRIGHT_CORE_VERSION);
    check_fixture(&root).unwrap();
}

#[test]
fn metadata_alone_cannot_pass_missing_entry() {
    let root = fixture(
        VALID_MODULE,
        "playwright-core",
        REQUIRED_PLAYWRIGHT_CORE_VERSION,
    );
    fs::remove_file(root.0.join("module with spaces 中文/index.mjs")).unwrap();
    assert_eq!(check_fixture(&root).unwrap_err().code, "import_failed");
}

#[test]
fn broken_transitive_import_and_syntax_are_rejected() {
    for source in [
        "import './missing-module.mjs';",
        "export const chromium = ;",
    ] {
        let root = fixture(source, "playwright-core", REQUIRED_PLAYWRIGHT_CORE_VERSION);
        assert_eq!(check_fixture(&root).unwrap_err().code, "import_failed");
    }
}

#[test]
fn wrong_version_and_package_identity_are_rejected() {
    for (name, version, code) in [
        ("playwright-core", "0.0.0", "version_mismatch"),
        (
            "not-playwright",
            REQUIRED_PLAYWRIGHT_CORE_VERSION,
            "import_failed",
        ),
    ] {
        let root = fixture(VALID_MODULE, name, version);
        assert_eq!(check_fixture(&root).unwrap_err().code, code);
    }
}

#[test]
fn incomplete_chromium_contract_is_rejected() {
    for source in [
        "export const chromium = {};",
        "export default {};",
        "export const chromium = { name: () => 'chromium', launch() {}, connectOverCDP() {} };",
    ] {
        let root = fixture(source, "playwright-core", REQUIRED_PLAYWRIGHT_CORE_VERSION);
        assert_eq!(check_fixture(&root).unwrap_err().code, "import_failed");
    }
}

#[test]
fn early_success_exit_cannot_forge_an_import_pass() {
    let root = fixture(
        "process.exit(0);",
        "playwright-core",
        REQUIRED_PLAYWRIGHT_CORE_VERSION,
    );
    assert_eq!(check_fixture(&root).unwrap_err().code, "import_failed");
}

#[test]
fn missing_pack_node_does_not_fall_back_to_system_node() {
    let root = fixture(
        VALID_MODULE,
        "playwright-core",
        REQUIRED_PLAYWRIGHT_CORE_VERSION,
    );
    let err =
        check_with_timeout(&root.0.join("missing-node"), &root.0, IMPORT_TIMEOUT).unwrap_err();
    assert_eq!(err.code, "import_failed");
}

#[test]
fn hanging_module_has_a_deadline_and_sanitized_error() {
    let root = fixture(
        "await new Promise(() => setInterval(() => {}, 1000));",
        "playwright-core",
        REQUIRED_PLAYWRIGHT_CORE_VERSION,
    );
    let started = Instant::now();
    let err = check_with_timeout(
        &node(),
        &root.0.join("module with spaces 中文"),
        Duration::from_secs(2),
    )
    .unwrap_err();
    assert_eq!(err.code, "import_timeout");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(!err.message.contains(&root.0.to_string_lossy().to_string()));
}

#[test]
fn deadline_terminates_and_reaps_the_owned_child() {
    let mut command = Command::new(fs::canonicalize(node()).unwrap());
    command
        .args(["-e", "setInterval(() => {}, 1000)"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = ProbeChild(command.spawn().unwrap());
    assert_eq!(
        wait_for_probe(&mut child.0, Duration::from_millis(300))
            .unwrap_err()
            .code,
        "import_timeout"
    );
    assert!(
        child.0.try_wait().unwrap().is_some(),
        "timeout must reap the owned process before returning"
    );
}

#[test]
fn module_diagnostics_are_not_copied_to_errors() {
    let root = fixture(
        "throw new Error('sensitive-fixture-marker');",
        "playwright-core",
        REQUIRED_PLAYWRIGHT_CORE_VERSION,
    );
    let err = check_fixture(&root).unwrap_err();
    assert_eq!(err.code, "import_failed");
    assert!(!err.message.contains("sensitive-fixture-marker"));
}
