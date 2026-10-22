//! Test-only RAII isolation for GROK_APP_HOME.
//! Recovers a poisoned *test* lock so one assertion failure does not cascade.
//! Production locks must not use this helper.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::MutexGuard;

#[path = "job_read_fixture.rs"]
pub(crate) mod job_read;

/// Owns the existing process-global lock and restores its byte-exact inherited value.
/// For older fixtures that already choose their own directory and cleanup strategy.
/// Restoration runs on normal return and unwinding, before the lock is released.
pub(crate) struct AppHomeEnvGuard {
    _lock: MutexGuard<'static, ()>,
    previous: Option<OsString>,
    user_home: Vec<(&'static str, Option<OsString>)>,
}

impl AppHomeEnvGuard {
    pub(crate) fn acquire() -> Self {
        let lock = crate::paths::APP_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self {
            _lock: lock,
            previous: std::env::var_os("GROK_APP_HOME"),
            user_home: Vec::new(),
        }
    }
}

impl Drop for AppHomeEnvGuard {
    fn drop(&mut self) {
        for (key, previous) in self.user_home.iter().rev() {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        match &self.previous {
            Some(value) => std::env::set_var("GROK_APP_HOME", value),
            None => std::env::remove_var("GROK_APP_HOME"),
        }
    }
}

pub struct IsolatedAppHome {
    _environment: AppHomeEnvGuard,
    pub path: PathBuf,
}

impl IsolatedAppHome {
    pub fn acquire(prefix: &str) -> Self {
        Self::with_setup(prefix, |_| {})
    }

    /// Hold the same environment lock while isolating official CLI config too.
    /// Overrides restore on unwind and normal drop, before releasing the lock.
    pub(crate) fn acquire_with_user_home(prefix: &str) -> Self {
        let mut home = Self::acquire(prefix);
        let user = home.path.join("user");
        std::fs::create_dir(&user).unwrap();
        for key in ["HOME", "USERPROFILE"] {
            home._environment
                .user_home
                .push((key, std::env::var_os(key)));
            std::env::set_var(key, &user);
        }
        home
    }

    fn with_setup(prefix: &str, setup: impl FnOnce(&std::path::Path)) -> Self {
        let environment = AppHomeEnvGuard::acquire();
        let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        // Own cleanup before any fallible initialization; restore before unlocking.
        let home = Self {
            _environment: environment,
            path,
        };
        std::fs::create_dir_all(&home.path).unwrap();
        std::env::set_var("GROK_APP_HOME", &home.path);
        setup(&home.path);
        crate::paths::ensure_app_dirs().expect("initialize isolated app home");
        home
    }
}

impl Drop for IsolatedAppHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
        // The environment field restores the inherited value before unlocking.
    }
}

#[path = "test_env.rs"]
mod environment;
pub(crate) use environment::{run_fixture_env_variants_contract, TestEnvGuard};

#[path = "test_home_contract.rs"]
mod contracts;
pub(crate) use contracts::{
    assert_fixture_home_contract, run_fixture_home_contract, run_fixture_home_variants_contract,
};

// Each parent launches exactly one child test; each selected fixture must restore
// its inherited home independently. Child entry points are no-ops in ordinary runs.
macro_rules! fixture_home_contract_tests {
    ($($fixture:path),+ $(,)?) => {
        #[test]
        fn inherited_home_contract_child() {
            $(crate::session_migration::test_home::run_fixture_home_contract($fixture);)+
        }
        #[test]
        fn inherited_home_contract_in_fresh_process() {
            let qualified = concat!(module_path!(), "::inherited_home_contract_child");
            let child = qualified.split_once("::").expect("crate-qualified test").1;
            crate::session_migration::test_home::assert_fixture_home_contract(child);
        }
    };
}
pub(crate) use fixture_home_contract_tests;

macro_rules! fixture_home_variants_contract_tests {
    ($($fixture:path),+ $(,)?) => {
        #[test]
        fn inherited_home_variants_child() {
            $(crate::session_migration::test_home::run_fixture_home_variants_contract($fixture);)+
        }
        #[test]
        fn inherited_home_variants_in_fresh_process() {
            let qualified = concat!(module_path!(), "::inherited_home_variants_child");
            let child = qualified.split_once("::").expect("crate-qualified test").1;
            crate::session_migration::test_home::assert_fixture_home_contract(child);
        }
    };
}
pub(crate) use fixture_home_variants_contract_tests;

macro_rules! fixture_env_contract_tests {
    ([$($key:literal),+ $(,)?], $fixture:path) => {
        #[test]
        fn inherited_env_variants_child() {
            crate::session_migration::test_home::run_fixture_env_variants_contract(&[$($key),+], $fixture);
        }
        #[test]
        fn inherited_env_variants_in_fresh_process() {
            let qualified = concat!(module_path!(), "::inherited_env_variants_child");
            let child = qualified.split_once("::").expect("crate-qualified test").1;
            crate::session_migration::test_home::assert_fixture_home_contract(child);
        }
    };
}
pub(crate) use fixture_env_contract_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn guard_restores_after_unwind_and_recovers_poison() {
        let inherited = std::env::var_os("GROK_APP_HOME");
        let result = std::panic::catch_unwind(|| {
            let _guard = AppHomeEnvGuard::acquire();
            std::env::set_var("GROK_APP_HOME", "fixture-unwind-only");
            panic!("intentional fixture unwind");
        });
        assert!(result.is_err());
        assert_eq!(std::env::var_os("GROK_APP_HOME"), inherited);
        let _guard = AppHomeEnvGuard::acquire();
        assert_eq!(std::env::var_os("GROK_APP_HOME"), inherited);
    }

    fn guard_preserves_absence_empty_and_non_unicode_values() {
        // Invoked only by the one-test child process, never in a concurrent suite.
        let inherited = std::env::var_os("GROK_APP_HOME").unwrap();
        #[cfg(windows)]
        let raw = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0x61, 0xd800, 0x62])
        };
        #[cfg(unix)]
        let raw = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![b'a', 0xff, b'b'])
        };
        #[cfg(not(any(windows, unix)))]
        let raw = OsString::from("opaque-home");
        for value in [None, Some(OsString::new()), Some(raw)] {
            match &value {
                Some(v) => std::env::set_var("GROK_APP_HOME", v),
                None => std::env::remove_var("GROK_APP_HOME"),
            }
            {
                let _guard = AppHomeEnvGuard::acquire();
                std::env::set_var("GROK_APP_HOME", "temporary-value");
            }
            assert_eq!(std::env::var_os("GROK_APP_HOME"), value);
        }
        std::env::set_var("GROK_APP_HOME", inherited);
    }

    fn isolated_home_cleans_up_on_unwind() {
        let inherited = std::env::var_os("GROK_APP_HOME");
        let mut path = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let home = IsolatedAppHome::acquire("guard-unwind");
            path = Some(home.path.clone());
            std::fs::write(home.path.join("fixture-only"), b"test").unwrap();
            panic!("intentional isolated-home unwind");
        }));
        assert!(result.is_err());
        assert!(!path.unwrap().exists());
        assert_eq!(std::env::var_os("GROK_APP_HOME"), inherited);
    }

    fixture_home_contract_tests!(
        guard_restores_after_unwind_and_recovers_poison,
        guard_preserves_absence_empty_and_non_unicode_values,
        isolated_home_cleans_up_on_unwind,
    );
}

#[path = "isolated_home_tests.rs"]
mod initialization_tests;
