//! Constructor failure contracts for test fixtures, not native acceptance.
use super::IsolatedAppHome;
use std::path::Path;

fn failed_initialization_restores(setup: impl FnOnce(&Path)) {
    let inherited = std::env::var_os("GROK_APP_HOME");
    let mut owned_root = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _home = IsolatedAppHome::with_setup("isolated-home-init", |app| {
            owned_root = Some(app.to_path_buf());
            setup(app);
        });
    }));
    assert!(
        result.is_err(),
        "constructor must propagate initialization failure"
    );
    let root = owned_root.expect("constructor reached initialization");
    assert_eq!(std::env::var_os("GROK_APP_HOME"), inherited);
    assert!(
        !root.exists(),
        "failed constructor leaked {}",
        root.display()
    );
    let home = IsolatedAppHome::acquire("isolated-home-after-failure");
    assert!(home.path.join("projects").is_dir());
    drop(home);
    assert_eq!(std::env::var_os("GROK_APP_HOME"), inherited);
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

mod partial_filesystem_error {
    use super::*;
    fn blocked_logs_directory() {
        failed_initialization_restores(|app| {
            std::fs::write(app.join("logs"), b"not a directory").unwrap();
        });
    }
    fixture_home_variants_contract_tests!(blocked_logs_directory);
}

mod setup_panic {
    use super::*;
    fn setup_unwinds() {
        failed_initialization_restores(|_| panic!("intentional isolated-home setup unwind"));
    }
    fixture_home_variants_contract_tests!(setup_unwinds);
}

mod normal_lifecycle {
    use super::*;
    fn initialized_home_restores() {
        let inherited = std::env::var_os("GROK_APP_HOME");
        let home = IsolatedAppHome::acquire("isolated-home-normal");
        let root = home.path.clone();
        assert!(root.join("projects").is_dir());
        assert_eq!(
            std::env::var_os("GROK_APP_HOME"),
            Some(root.clone().into_os_string())
        );
        drop(home);
        assert_eq!(std::env::var_os("GROK_APP_HOME"), inherited);
        assert!(!root.exists());
    }
    fixture_home_variants_contract_tests!(initialized_home_restores);
}

mod official_user_home {
    use super::*;

    fn user_home_restores_after_normal_and_unwind_drop() {
        let inherited = ["GROK_APP_HOME", "HOME", "USERPROFILE"].map(std::env::var_os);
        for unwind in [false, true] {
            let mut owned = None;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let home = IsolatedAppHome::acquire_with_user_home("isolated-official-user");
                owned = Some(home.path.clone());
                assert_eq!(crate::process_util::user_home(), home.path.join("user"));
                assert!(home.path.join("user").is_dir());
                if unwind {
                    panic!("intentional user-home fixture unwind");
                }
            }));
            assert_eq!(result.is_err(), unwind);
            assert_eq!(
                ["GROK_APP_HOME", "HOME", "USERPROFILE"].map(std::env::var_os),
                inherited
            );
            assert!(!owned.unwrap().exists());
        }
    }

    crate::session_migration::test_home::fixture_env_contract_tests!(
        ["HOME", "USERPROFILE"],
        user_home_restores_after_normal_and_unwind_drop
    );
}
