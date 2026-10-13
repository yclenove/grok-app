use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "grok-cu-native-loader-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn fixture_manifest(root: &Path) -> serde_json::Value {
    let mut paths = vec![
        "lib/libpipewire-0.3.so.0".to_owned(),
        "lib/spa-0.2/support/libspa-support.so".into(),
        "lib/spa-0.2/audioconvert/libspa-audioconvert.so".into(),
        "share/pipewire/client.conf".into(),
        "share/licenses/pipewire/COPYING".into(),
        "share/licenses/pipewire/LICENSE".into(),
        "share/licenses/pipewire/0001-CVE-2026-14330.patch".into(),
        "share/licenses/pipewire/SOURCE.txt".into(),
        "share/licenses/libei/COPYING".into(),
    ];
    for module in [
        "rt",
        "protocol-native",
        "client-node",
        "client-device",
        "adapter",
        "metadata",
        "session-manager",
    ] {
        paths.push(format!("lib/pipewire-0.3/libpipewire-module-{module}.so"));
    }
    let files = paths
        .iter()
        .map(|path| {
            let actual = root.join("runtime").join(path);
            fs::create_dir_all(actual.parent().unwrap()).unwrap();
            let bytes = b"inert loader contract fixture, not a native library";
            fs::write(&actual, bytes).unwrap();
            json!({"path":path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))})
        })
        .collect::<Vec<_>>();
    let (target, glibc) = native_policy(std::env::consts::ARCH).unwrap();
    json!({"schemaVersion":1,"target":target,"pipewireVersion":"1.6.9","glibcBaseline":glibc,"files":files})
}

#[test]
fn architecture_and_abi_policy_must_match_the_compiled_target() {
    let fixture = Fixture::new();
    let mut manifest = fixture_manifest(&fixture.0);
    for (arch, target, baseline, foreign_arch) in [
        ("x86_64", "x86_64-unknown-linux-gnu", "2.35", "aarch64"),
        ("aarch64", "aarch64-unknown-linux-gnu", "2.39", "x86_64"),
    ] {
        manifest["target"] = json!(target);
        manifest["glibcBaseline"] = json!(baseline);
        let bytes = serde_json::to_vec(&manifest).unwrap();
        assert!(parse_manifest_for_arch(&bytes, arch).is_ok());
        assert!(parse_manifest_for_arch(&bytes, foreign_arch).is_err());
        assert!(parse_manifest_for_arch(&bytes, "riscv64").is_err());
        manifest["glibcBaseline"] = json!("2.99");
        assert!(parse_manifest_for_arch(&serde_json::to_vec(&manifest).unwrap(), arch).is_err());
    }
}

#[test]
fn manifest_and_exact_files_pass_without_native_claim() {
    let fixture = Fixture::new();
    let json = fixture_manifest(&fixture.0);
    let manifest = parse_manifest(&serde_json::to_vec(&json).unwrap()).unwrap();
    verify_files(&fixture.0, &manifest).unwrap();
}

#[test]
fn incompatible_policy_and_file_records_rejected() {
    let fixture = Fixture::new();
    let valid = fixture_manifest(&fixture.0);
    for key in [
        "schemaVersion",
        "target",
        "pipewireVersion",
        "glibcBaseline",
    ] {
        let mut bad = valid.clone();
        bad[key] = json!("wrong-policy");
        assert!(parse_manifest(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    for path in ["../escape", "/absolute", "", "lib/not-required"] {
        let mut bad = valid.clone();
        bad["files"][0]["path"] = json!(path);
        assert!(parse_manifest(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    for key in ["bytes", "sha256"] {
        let mut bad = valid.clone();
        bad["files"][0][key] = if key == "bytes" {
            json!(0)
        } else {
            json!("bad")
        };
        assert!(parse_manifest(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let mut duplicate = valid;
    duplicate["files"][1] = duplicate["files"][0].clone();
    assert!(parse_manifest(&serde_json::to_vec(&duplicate).unwrap()).is_err());
}

#[test]
fn missing_corrupt_and_symlink_file_fail() {
    let fixture = Fixture::new();
    let json = fixture_manifest(&fixture.0);
    let manifest = parse_manifest(&serde_json::to_vec(&json).unwrap()).unwrap();
    let path = fixture.0.join("runtime/lib/libpipewire-0.3.so.0");
    let original = fs::read(&path).unwrap();
    fs::write(&path, vec![b'x'; original.len()]).unwrap();
    assert!(verify_files(&fixture.0, &manifest)
        .unwrap_err()
        .contains("hash mismatch"));
    fs::remove_file(&path).unwrap();
    assert!(verify_files(&fixture.0, &manifest).is_err());
    std::os::unix::fs::symlink("/does-not-exist", &path).unwrap();
    assert!(verify_files(&fixture.0, &manifest)
        .unwrap_err()
        .contains("symlink"));
}

#[test]
fn installed_and_cargo_layout_are_executable_relative() {
    let fixture = Fixture::new();
    let exe = fixture.0.join("usr/bin/grok-app");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    let resource = fixture.0.join("usr/lib/grok-app").join(RESOURCE);
    fs::create_dir_all(&resource).unwrap();
    assert_eq!(resource_from_exe(&exe).unwrap(), resource);
    let debug = fixture.0.join("target/debug/grok-app");
    let local = debug.parent().unwrap().join(RESOURCE);
    fs::create_dir_all(&local).unwrap();
    assert_eq!(resource_from_exe(&debug).unwrap(), local);
}

#[test]
fn missing_or_parent_linked_resource_is_not_relocated() {
    let fixture = Fixture::new();
    let exe = fixture.0.join("usr/bin/grok-app");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    assert!(resource_from_exe(&exe).is_err());
    let outside = fixture.0.join("outside");
    fs::create_dir_all(outside.join(RESOURCE)).unwrap();
    fs::create_dir_all(fixture.0.join("usr/lib")).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.0.join("usr/lib/grok-app")).unwrap();
    assert!(resource_from_exe(&exe).unwrap_err().contains("symlink"));
}

#[test]
fn private_search_precedes_host_and_drops_empty_or_duplicate_entries() {
    let private = Path::new("/private/sdk with spaces/lib");
    let old = std::ffi::OsStr::new("/usr/lib::/private/sdk with spaces/lib:/usr/lib64");
    let value = library_search_path(private, Some(old)).unwrap();
    assert_eq!(
        std::env::split_paths(&value).collect::<Vec<_>>(),
        vec![private, Path::new("/usr/lib"), Path::new("/usr/lib64")]
    );
}

#[test]
fn unexpected_modules_config_dropins_and_empty_directories_fail() {
    for extra in [
        "share/pipewire/client.conf.d/override.conf",
        "lib/unapproved.so",
        "unexpected-dir",
    ] {
        let fixture = Fixture::new();
        let json = fixture_manifest(&fixture.0);
        let manifest = parse_manifest(&serde_json::to_vec(&json).unwrap()).unwrap();
        let path = fixture.0.join("runtime").join(extra);
        if extra == "unexpected-dir" {
            fs::create_dir(&path).unwrap();
        } else {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"unapproved").unwrap();
        }
        assert!(verify_files(&fixture.0, &manifest)
            .unwrap_err()
            .contains("Unexpected"));
    }
}

#[test]
fn fifo_component_and_manifest_are_rejected_without_blocking_read() {
    use std::os::unix::ffi::OsStrExt;
    let fixture = Fixture::new();
    let json = fixture_manifest(&fixture.0);
    let manifest = parse_manifest(&serde_json::to_vec(&json).unwrap()).unwrap();
    let path = fixture.0.join("runtime/share/pipewire/client.conf");
    fs::remove_file(&path).unwrap();
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    assert!(verify_files(&fixture.0, &manifest).is_err());
    assert!(open_regular(&path)
        .unwrap_err()
        .contains("not a regular file"));
}

#[test]
fn non_utf8_installation_path_is_preserved() {
    use std::os::unix::ffi::OsStringExt;
    let fixture = Fixture::new();
    let base = fixture.0.join(OsString::from_vec(b"install-\xff".to_vec()));
    let exe = base.join("bin/grok-app");
    let resource = base.join("lib/grok-app").join(RESOURCE);
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::create_dir_all(&resource).unwrap();
    assert_eq!(resource_from_exe(&exe).unwrap(), resource);
    assert_eq!(
        std::env::split_paths(&library_search_path(&resource, None).unwrap())
            .next()
            .unwrap(),
        resource
    );
}
