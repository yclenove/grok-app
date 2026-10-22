//! Fresh-process constructor failure regressions, not native acceptance.
use super::CapacityHome;
use crate::session_migration::test_home::fixture_home_variants_contract_tests;
use std::path::Path;

const ENV_KEYS: [&str; 4] = ["GROK_APP_HOME", "HOME", "USERPROFILE", "GROK_HOME"];

fn failed_initialization_restores(setup: impl FnOnce(&Path)) {
    let inherited = ENV_KEYS.map(std::env::var_os);
    let mut owned_root = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _home = CapacityHome::with_setup(|app| {
            owned_root = Some(app.parent().unwrap().to_path_buf());
            setup(app);
        });
    }));
    assert!(
        result.is_err(),
        "constructor must propagate initialization failure"
    );
    let root = owned_root.expect("constructor reached initialization");
    assert_eq!(
        ENV_KEYS.map(std::env::var_os),
        inherited,
        "failed constructor leaked its home override"
    );
    assert!(
        !root.exists(),
        "failed constructor leaked {}",
        root.display()
    );
    // A poisoned test-only lock must remain usable after the unwind.
    let home = CapacityHome::new();
    assert!(home.root.join("app/projects").is_dir());
    drop(home);
    assert_eq!(ENV_KEYS.map(std::env::var_os), inherited);
}

mod filesystem_error {
    use super::*;
    fn blocked_projects_directory() {
        failed_initialization_restores(|app| {
            std::fs::write(app.join("projects"), b"not a directory").unwrap();
        });
    }
    fixture_home_variants_contract_tests!(blocked_projects_directory);
}

mod setup_panic {
    use super::*;
    fn setup_unwinds() {
        failed_initialization_restores(|_| panic!("intentional constructor setup unwind"));
    }
    fixture_home_variants_contract_tests!(setup_unwinds);
}

mod normal_lifecycle {
    use super::*;
    fn initialized_home_restores() {
        let inherited = ENV_KEYS.map(std::env::var_os);
        let home = CapacityHome::new();
        let root = home.root.clone();
        assert!(root.join("app/projects").is_dir());
        assert_eq!(
            ENV_KEYS.map(std::env::var_os),
            ["app", "user", "user", "grok"]
                .map(|directory| Some(root.join(directory).into_os_string()))
        );
        assert_eq!(crate::process_util::user_home(), root.join("user"));
        drop(home);
        assert_eq!(ENV_KEYS.map(std::env::var_os), inherited);
        assert!(!root.exists());
    }
    fixture_home_variants_contract_tests!(initialized_home_restores);
}

mod partial_filesystem_error {
    use super::*;
    fn blocked_logs_directory() {
        failed_initialization_restores(|app| {
            std::fs::write(app.join("logs"), b"not a directory").unwrap();
        });
    }
    fixture_home_variants_contract_tests!(blocked_logs_directory);
}
