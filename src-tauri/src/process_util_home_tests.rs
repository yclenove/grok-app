//! HOME/USERPROFILE test fixtures. Commands are inspected, never executed.
use super::{ensure_home_env_std, home_env_present, user_home, PathBuf, StdCommand};
use crate::session_migration::test_home::{
    assert_fixture_home_contract, run_fixture_home_contract,
};
use std::ffi::{OsStr, OsString};

#[derive(Clone, Copy)]
enum Mode {
    Fallback,
    Existing,
}

fn restore(name: &str, value: &Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

struct HomeEnvironment {
    previous_home: Option<OsString>,
    previous_profile: Option<OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl HomeEnvironment {
    fn acquire() -> Self {
        let lock = crate::paths::APP_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self {
            previous_home: std::env::var_os("HOME"),
            previous_profile: std::env::var_os("USERPROFILE"),
            _lock: lock,
        }
    }
}

impl Drop for HomeEnvironment {
    fn drop(&mut self) {
        restore("HOME", &self.previous_home);
        restore("USERPROFILE", &self.previous_profile);
    }
}

fn with_home_fixture(mode: Mode, body: impl FnOnce()) {
    let _environment = HomeEnvironment::acquire();
    match mode {
        Mode::Fallback => {
            std::env::remove_var("HOME");
            std::env::set_var("USERPROFILE", "/tmp/grok-app-win-home-sim");
        }
        Mode::Existing => std::env::set_var("HOME", "/tmp/grok-app-home-test"),
    }
    body();
}

fn assert_fixture_behavior(mode: Mode) {
    let mut command = StdCommand::new("true");
    match mode {
        Mode::Fallback => {
            assert!(!home_env_present());
            assert_eq!(user_home(), PathBuf::from("/tmp/grok-app-win-home-sim"));
            ensure_home_env_std(&mut command);
            assert!(command.get_envs().any(|(key, value)| {
                key == OsStr::new("HOME") && value == Some(OsStr::new("/tmp/grok-app-win-home-sim"))
            }));
        }
        Mode::Existing => {
            assert!(home_env_present());
            ensure_home_env_std(&mut command);
            assert!(!command.get_envs().any(|(key, _)| key == OsStr::new("HOME")));
        }
    }
}

pub(super) fn check_fallback() {
    with_home_fixture(Mode::Fallback, || assert_fixture_behavior(Mode::Fallback));
}

pub(super) fn check_existing() {
    with_home_fixture(Mode::Existing, || assert_fixture_behavior(Mode::Existing));
}

// The exact child is single-test and isolated; never mutate variants in a parallel suite.
fn check_home_variants(mode: Mode, unwind: bool) {
    run_fixture_home_contract(|| {
        struct OuterRestore(Option<OsString>, Option<OsString>);
        impl Drop for OuterRestore {
            fn drop(&mut self) {
                restore("HOME", &self.0);
                restore("USERPROFILE", &self.1);
            }
        }
        let inherited_home = std::env::var_os("HOME");
        let inherited_profile = std::env::var_os("USERPROFILE");
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
        let homes = [
            None,
            Some(OsString::new()),
            Some(raw.clone()),
            inherited_home.clone(),
        ];
        let profiles = [
            None,
            Some(OsString::new()),
            Some(raw),
            inherited_profile.clone(),
        ];
        for home in homes {
            for profile in &profiles {
                let _outer = OuterRestore(inherited_home.clone(), inherited_profile.clone());
                restore("HOME", &home);
                restore("USERPROFILE", profile);
                let result = std::panic::catch_unwind(|| {
                    with_home_fixture(mode, || {
                        assert_fixture_behavior(mode);
                        assert!(!unwind, "intentional HOME/USERPROFILE fixture unwind");
                    });
                });
                assert_eq!(result.is_err(), unwind);
                assert_eq!(std::env::var_os("HOME"), home, "fixture leaked HOME");
                assert_eq!(
                    std::env::var_os("USERPROFILE"),
                    *profile,
                    "fixture leaked USERPROFILE"
                );
            }
        }
    });
}

macro_rules! home_contract {
    ($name:ident, $mode:expr, $unwind:expr) => {
        mod $name {
            use super::*;
            #[test]
            fn home_environment_child() {
                check_home_variants($mode, $unwind);
            }
            #[test]
            fn home_environment_in_fresh_process() {
                let qualified = concat!(module_path!(), "::home_environment_child");
                assert_fixture_home_contract(qualified.split_once("::").unwrap().1);
            }
        }
    };
}
home_contract!(fallback_normal, Mode::Fallback, false);
home_contract!(fallback_unwind, Mode::Fallback, true);
home_contract!(existing_normal, Mode::Existing, false);
home_contract!(existing_unwind, Mode::Existing, true);
