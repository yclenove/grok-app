//! Exact-process contracts for temporary test environment overrides.
use std::ffi::OsString;

/// Captures raw values while holding the shared test environment lock.
/// Fields drop in declaration order: override values restore before home/unlock.
/// Do not acquire while an AppHomeEnvGuard/IsolatedAppHome is already held.
pub(crate) struct TestEnvGuard {
    _overrides: Restore,
    _home: super::AppHomeEnvGuard,
}
impl TestEnvGuard {
    pub(crate) fn acquire(keys: &[&'static str]) -> Self {
        let home = super::AppHomeEnvGuard::acquire();
        Self {
            _overrides: Restore(
                keys.iter()
                    .map(|&key| (key, std::env::var_os(key)))
                    .collect(),
            ),
            _home: home,
        }
    }
}

struct Restore(Vec<(&'static str, Option<OsString>)>);
impl Drop for Restore {
    fn drop(&mut self) {
        for (key, value) in self.0.iter().rev() {
            set(key, value.as_ref());
        }
    }
}
fn set(key: &str, value: Option<&OsString>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}
fn opaque_value() -> OsString {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0x61, 0xd800, 0x62])
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![b'a', 0xff, b'b'])
    }
    #[cfg(not(any(windows, unix)))]
    OsString::from("opaque-value")
}

/// Only enters in one exact fresh-process child. Each fixture owns its own lock.
/// Covers the Cartesian product, not merely identical values for all keys.
pub(crate) fn run_fixture_env_variants_contract(keys: &[&'static str], fixture: impl Fn()) {
    super::run_fixture_home_contract(|| {
        assert!(!keys.is_empty() && keys.len() <= 4);
        let _restore = Restore(
            keys.iter()
                .map(|&key| (key, std::env::var_os(key)))
                .collect(),
        );
        let variants = [
            None,
            Some(OsString::new()),
            Some(opaque_value()),
            Some(OsString::from("inherited-fixture-value")),
        ];
        for case in 0..4_usize.pow(keys.len() as u32) {
            let expected: Vec<_> = keys
                .iter()
                .enumerate()
                .map(|(i, &key)| {
                    let value = variants[(case / 4_usize.pow(i as u32)) % 4].clone();
                    set(key, value.as_ref());
                    (key, value)
                })
                .collect();
            fixture();
            for (key, value) in expected {
                assert_eq!(
                    std::env::var_os(key),
                    value,
                    "fixture changed inherited {key}; case {case}"
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEYS: &[&str] = &[
        "GROK_CLI_REQUIRE_CHECKSUM",
        "GROK_CLI_ALLOW_UNVERIFIED",
        "GROK_LEADER_SOCKET",
        "GROK_APP_VOICE",
    ];

    fn check_guard(panic_inside: bool) {
        let result = std::panic::catch_unwind(|| {
            let _guard = TestEnvGuard::acquire(KEYS);
            for key in KEYS {
                std::env::set_var(key, "temporary-override");
            }
            if panic_inside {
                panic!("intentional environment guard unwind");
            }
        });
        assert_eq!(result.is_err(), panic_inside);
        // A panic must not prevent acquiring the shared fixture lock again.
        let _guard = TestEnvGuard::acquire(KEYS);
    }

    mod normal {
        fn fixture() {
            super::check_guard(false);
        }
        crate::session_migration::test_home::fixture_env_contract_tests!(
            [
                "GROK_CLI_REQUIRE_CHECKSUM",
                "GROK_CLI_ALLOW_UNVERIFIED",
                "GROK_LEADER_SOCKET",
                "GROK_APP_VOICE"
            ],
            fixture
        );
    }
    mod unwind {
        fn fixture() {
            super::check_guard(true);
        }
        crate::session_migration::test_home::fixture_env_contract_tests!(
            [
                "GROK_CLI_REQUIRE_CHECKSUM",
                "GROK_CLI_ALLOW_UNVERIFIED",
                "GROK_LEADER_SOCKET",
                "GROK_APP_VOICE"
            ],
            fixture
        );
    }
}
