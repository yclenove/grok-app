//! Fresh-process contracts for fixtures that temporarily replace the application home.
//! Child processes receive fallback homes inside the sandbox, even on the failing path.
use std::path::{Path, PathBuf};
const MARKER: &str = "GROK_FIXTURE_HOME_CONTRACT";
pub(crate) fn run_fixture_home_contract(fixture: impl FnOnce()) {
    let Some(root) = std::env::var_os(MARKER).map(PathBuf::from) else {
        return;
    };
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("gfh-"));
    let expected = root.join("app");
    assert_eq!(
        std::env::var_os("GROK_APP_HOME"),
        Some(expected.clone().into_os_string())
    );
    fixture();
    assert_eq!(
        std::env::var_os("GROK_APP_HOME"),
        Some(expected.clone().into_os_string()),
        "fixture removed or replaced the inherited sandbox"
    );
    assert_parent_unchanged(&expected);
}
fn assert_parent_unchanged(app: &Path) {
    assert_eq!(
        std::fs::read(app.join("sentinel")).unwrap(),
        b"parent-home-unchanged"
    );
    let names = std::fs::read_dir(app)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![std::ffi::OsString::from("sentinel")],
        "fixture wrote into its inherited parent home"
    );
}
pub(crate) fn assert_fixture_home_contract(child: &str) {
    let root = std::env::temp_dir().join(format!(
        "gfh-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..16]
    ));
    for dir in [
        "app",
        "user",
        "roaming",
        "local",
        "tmp",
        "grok",
        "xdg-config",
        "xdg-data",
        "xdg-cache",
    ] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("app/sentinel"), b"parent-home-unchanged").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", child, "--test-threads=1", "--nocapture"])
        .env(MARKER, &root)
        .env("GROK_APP_HOME", root.join("app"))
        .env("GROK_HOME", root.join("grok"))
        .env("HOME", root.join("user"))
        .env("USERPROFILE", root.join("user"))
        .env("APPDATA", root.join("roaming"))
        .env("LOCALAPPDATA", root.join("local"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("TEMP", root.join("tmp"))
        .env("TMP", root.join("tmp"))
        .env("TMPDIR", root.join("tmp"))
        .env("GROK_APP_ACP", "mock")
        .env("GROK_DISABLE_DEFAULT_ROOTS", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture home contract failed; sandbox retained at {}\n{}\n{}",
        root.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
        "child contract was not selected: {child}"
    );
    assert_parent_unchanged(&root.join("app"));
    for dir in [
        "user",
        "roaming",
        "local",
        "grok",
        "xdg-config",
        "xdg-data",
        "xdg-cache",
    ] {
        assert_eq!(
            std::fs::read_dir(root.join(dir)).unwrap().count(),
            0,
            "fixture touched fallback home {dir}"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

// Only the exact fresh-process child enters this helper. No global-lock scope is
// held around a fixture: each fixture must own and restore its own environment.
pub(crate) fn run_fixture_home_variants_contract(fixture: impl Fn()) {
    run_fixture_home_contract(|| {
        use std::ffi::OsString;
        struct Restore(Option<OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var("GROK_APP_HOME", value),
                    None => std::env::remove_var("GROK_APP_HOME"),
                }
            }
        }
        let inherited = std::env::var_os("GROK_APP_HOME");
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
        for value in [None, Some(OsString::new()), Some(raw), inherited.clone()] {
            let restore = Restore(inherited.clone());
            match &value {
                Some(value) => std::env::set_var("GROK_APP_HOME", value),
                None => std::env::remove_var("GROK_APP_HOME"),
            }
            fixture();
            assert_eq!(
                std::env::var_os("GROK_APP_HOME"),
                value,
                "fixture must restore absence, empty, opaque and inherited values exactly"
            );
            drop(restore);
        }
    });
}
