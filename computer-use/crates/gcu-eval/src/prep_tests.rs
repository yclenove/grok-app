#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::runner::Environment;

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("gcu-prep-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn environment(root: &Path) -> Environment {
    let make = |name: &str| {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    };
    Environment {
        home: make("home"),
        grok_home: make("grok"),
        work: make("work"),
        shim_dir: make("shims"),
        state_path: root.join("state.json"),
        side_log: root.join("side.log"),
        tool_log: root.join("tools.log"),
        transcript: root.join("grok").join("transcript.json"),
        root: root.to_path_buf(),
    }
}

#[test]
fn auth_seed_copies_only_the_login_file_with_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let seed = scratch("seed");
    std::fs::write(seed.join("auth.json"), b"opaque").unwrap();
    std::fs::write(seed.join("config.toml"), b"must not travel").unwrap();
    std::fs::create_dir_all(seed.join("sessions")).unwrap();
    let home = scratch("home");
    seed_auth(&seed, &home).unwrap();
    assert_eq!(std::fs::read(home.join("auth.json")).unwrap(), b"opaque");
    let mode = std::fs::metadata(home.join("auth.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert!(!home.join("config.toml").exists());
    assert!(!home.join("sessions").exists());
    let _ = std::fs::remove_dir_all(&seed);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn auth_seed_without_a_login_file_is_refused() {
    let seed = scratch("noseed");
    let home = scratch("nohome");
    assert!(seed_auth(&seed, &home).is_err());
    let _ = std::fs::remove_dir_all(&seed);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn auth_seed_symlink_is_refused_and_not_copied() {
    let seed = scratch("seedlink");
    let real = scratch("reallogin");
    std::fs::write(real.join("auth.json"), b"opaque-secret").unwrap();
    std::os::unix::fs::symlink(real.join("auth.json"), seed.join("auth.json")).unwrap();
    let home = scratch("homelink");
    let err = seed_auth(&seed, &home).unwrap_err();
    assert!(
        !home.join("auth.json").exists(),
        "a symlink seed must not be copied"
    );
    assert!(!err.to_string().contains("opaque-secret"));
    let _ = std::fs::remove_dir_all(&seed);
    let _ = std::fs::remove_dir_all(&real);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn auth_seed_directory_symlink_is_refused() {
    let real = scratch("realdir");
    std::fs::write(real.join("auth.json"), b"opaque").unwrap();
    let parent = scratch("linkparent");
    let link = parent.join("seed");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let home = scratch("homedirlink");
    assert!(seed_auth(&link, &home).is_err());
    assert!(!home.join("auth.json").exists());
    let _ = std::fs::remove_dir_all(&real);
    let _ = std::fs::remove_dir_all(&parent);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn shim_log_records_the_command_and_not_the_arguments() {
    let root = scratch("shimlog");
    let shims = root.join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    write_shims(&shims).unwrap();
    let log = root.join("side.log");
    std::fs::write(&log, b"").unwrap();
    let status = std::process::Command::new(shims.join("curl"))
        .arg("https://user:SENSITIVEVALUE@example.invalid/secret")
        .env("GCU_EVAL_SIDE_LOG", &log)
        .env("PATH", &shims)
        .status()
        .unwrap();
    assert!(!status.success());
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("process curl"), "{text}");
    assert!(
        !text.contains("SENSITIVEVALUE"),
        "shim log kept the argument: {text}"
    );
    assert!(
        !text.contains("example.invalid"),
        "shim log kept the argument: {text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn fixture_is_started_with_the_state_path_and_stopped_on_drop() {
    use std::os::unix::fs::PermissionsExt;
    let root = scratch("fixture");
    let env = environment(&root);
    let script = root.join("fixture.sh");
    std::fs::write(
            &script,
            "#!/bin/sh\nprintf '{\"counter\":0,\"pid\":%s}' \"$$\" > \"$GCU_FIXTURE_STATE\"\nsleep 300\n",
        )
        .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let fixture = Fixture::start(&script, &env).unwrap();
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&env.state_path).unwrap()).unwrap();
    let pid = state["pid"].as_u64().unwrap().to_string();
    let alive = |pid: &str| {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    assert!(alive(&pid), "fixture runs while the task runs");
    drop(fixture);
    assert!(!alive(&pid), "fixture is stopped with the task");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_fixture_that_never_writes_state_fails_instead_of_hanging_the_run() {
    use std::os::unix::fs::PermissionsExt;
    let root = scratch("silent");
    let env = environment(&root);
    let script = root.join("silent.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The fixture exits at once and never writes state: fail fast, do not wait out the deadline.
    let started = std::time::Instant::now();
    let result = Fixture::start(&script, &env);
    assert!(result.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn existing_temp_root_is_refused_without_deleting_it() {
    let parent = scratch("exist");
    let root = parent.join("tree");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("marker"), b"keep").unwrap();
    let err = reject_existing(&root).unwrap_err();
    assert!(err.to_string().contains("already exists"), "{err}");
    assert_eq!(std::fs::read(root.join("marker")).unwrap(), b"keep");
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn symlink_temp_root_is_refused_without_deleting_the_target() {
    let parent = scratch("symroot");
    let real = parent.join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("marker"), b"keep").unwrap();
    let link = parent.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let err = reject_existing(&link).unwrap_err();
    assert!(err.to_string().contains("symlink"), "{err}");
    {
        let _guard = Cleanup::standby(link.clone());
    }
    assert_eq!(std::fs::read(real.join("marker")).unwrap(), b"keep");
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn auth_seed_under_a_synthetic_config_root_is_refused() {
    let base = scratch("personal");
    let grok = base.join("vault").join(".grok");
    let seed = grok.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    std::fs::write(seed.join("auth.json"), b"opaque-secret").unwrap();
    let home = base.join("home");
    std::fs::create_dir(&home).unwrap();
    let err = seed_auth_in(&seed, &home, std::slice::from_ref(&grok)).unwrap_err();
    assert!(err.to_string().contains("personal config"), "{err}");
    assert!(!err.to_string().contains("opaque-secret"));
    assert!(!home.join("auth.json").exists());
    let outside = base.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let alias = outside.join("alias");
    std::os::unix::fs::symlink(&grok, &alias).unwrap();
    let home2 = base.join("home2");
    std::fs::create_dir(&home2).unwrap();
    assert!(seed_auth_in(&alias.join("seed"), &home2, &[grok]).is_err());
    assert!(!home2.join("auth.json").exists());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn cleanup_failure_turns_pass_into_fail() {
    let mut status = crate::runner::RunStatus::Pass;
    let mut reason = String::new();
    apply_cleanup_failure(&mut status, &mut reason);
    assert_eq!(status, crate::runner::RunStatus::Fail);
    assert_eq!(reason, "cleanup_failed");
    let mut kept = crate::runner::RunStatus::NotRun;
    let mut prior = String::from("client binary missing");
    apply_cleanup_failure(&mut kept, &mut prior);
    assert_eq!(kept, crate::runner::RunStatus::Fail);
    assert!(prior.contains("cleanup_failed"), "{prior}");
}

#[test]
fn auth_seed_hard_link_is_refused_and_not_copied() {
    let seed = scratch("hardseed");
    let home = scratch("hardhome");
    std::fs::write(seed.join("auth.json"), b"opaque-secret").unwrap();
    std::fs::hard_link(seed.join("auth.json"), seed.join("alias.json")).unwrap();
    let err = seed_auth(&seed, &home).unwrap_err();
    assert!(
        err.to_string().contains("linked") || err.to_string().contains("non-regular"),
        "{err}"
    );
    assert!(!err.to_string().contains("opaque-secret"));
    assert!(!home.join("auth.json").exists());
    let _ = std::fs::remove_dir_all(&seed);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn auth_seed_fifo_returns_without_blocking() {
    let seed = scratch("fifo");
    let home = scratch("fifohome");
    let fifo = seed.join("auth.json");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success(), "mkfifo");
    let started = std::time::Instant::now();
    let err = seed_auth(&seed, &home).unwrap_err();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "opening the fifo blocked: {err}"
    );
    assert!(!home.join("auth.json").exists());
    assert!(!err.to_string().contains("opaque"));
    let _ = std::fs::remove_dir_all(&seed);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn auth_seed_over_the_size_cap_is_refused() {
    let seed = scratch("bigseed");
    let home = scratch("bighome");
    let file = std::fs::File::create(seed.join("auth.json")).unwrap();
    file.set_len(1024 * 1024 + 1).unwrap();
    let err = seed_auth(&seed, &home).unwrap_err();
    assert!(err.to_string().contains("size cap"), "{err}");
    assert!(!home.join("auth.json").exists());
    let _ = std::fs::remove_dir_all(&seed);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fresh_root_is_owner_only_when_it_is_created() {
    use std::os::unix::fs::PermissionsExt;
    let parent = scratch("privparent");
    let mut held = begin_env_under(&parent).unwrap();
    let mode = std::fs::metadata(&held.env.root)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "root mode is {mode:o}");
    let child = std::fs::metadata(&held.env.home)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(child & 0o077, 0, "home mode is {child:o}");
    held.cleanup().unwrap();
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn world_writable_parent_without_sticky_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let parent = scratch("openparent");
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o777)).unwrap();
    let err = begin_env_under(&parent).unwrap_err();
    assert!(err.message.contains("not sticky"), "{}", err.message);
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn sticky_world_writable_parent_is_accepted() {
    use std::os::unix::fs::PermissionsExt;
    let parent = scratch("stickyparent");
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o1777)).unwrap();
    let mut held = begin_env_under(&parent).unwrap();
    assert!(held.env.root.is_dir());
    held.cleanup().unwrap();
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn fresh_roots_are_distinct_and_absent() {
    let left = fresh_root_in(&std::env::temp_dir()).unwrap();
    let right = fresh_root_in(&std::env::temp_dir()).unwrap();
    assert_ne!(left, right);
    assert!(!left.exists());
    assert!(!right.exists());
}
