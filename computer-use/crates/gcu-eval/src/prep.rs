//! Temporary directory helpers for the local evaluation provider.

use crate::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct Rm(pub(crate) PathBuf);
impl Drop for Rm {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn unique_root() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("gcu-ev-{}-{n}", std::process::id()))
}

pub(crate) fn remove_root(path: &Path) -> Result<bool, Error> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err) => Err(Error::msg(format!("destroy: {err}"))),
    }
}

pub(crate) fn lock_dir(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| Error::msg(format!("chmod: {err}")))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

pub(crate) fn write_shims(dir: &Path) -> Result<(), Error> {
    for name in ["curl", "wget", "ssh", "nc", "mail"] {
        write_shim(dir, name)?;
    }
    Ok(())
}

pub(crate) fn write_shim(dir: &Path, name: &str) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let path = dir.join(name);
        let body = "#!/bin/sh\necho \"process $(basename \"$0\")\" >> \"$GCU_EVAL_SIDE_LOG\"\necho \"network $*\" >> \"$GCU_EVAL_SIDE_LOG\"\nexit 1\n";
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o755)
            .open(&path)
            .map_err(|err| Error::msg(format!("shim: {err}")))?;
        file.write_all(body.as_bytes())
            .map_err(|err| Error::msg(format!("shim: {err}")))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let path = dir.join(format!("{name}.cmd"));
        let body = "@echo off\r\necho process %~n0>>\"%GCU_EVAL_SIDE_LOG%\"\r\necho network %*>>\"%GCU_EVAL_SIDE_LOG%\"\r\nexit /b 1\r\n";
        std::fs::write(path, body).map_err(|err| Error::msg(format!("shim: {err}")))
    }
}

/// Copy the evaluation login into a task's temporary `GROK_HOME`. The file is never read here and the
/// copy is deleted with the task directory. Only `auth.json` is taken from `seed`.
pub(crate) fn seed_auth(seed: &Path, grok_home: &Path) -> Result<(), Error> {
    let from = seed.join("auth.json");
    if !from.is_file() {
        return Err(Error::msg("auth seed has no auth.json"));
    }
    let to = grok_home.join("auth.json");
    std::fs::copy(&from, &to).map_err(|_| Error::msg("could not copy the auth seed"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| Error::msg("could not restrict the auth seed copy"))?;
    }
    Ok(())
}

/// A fixture app started for one task and stopped when this value is dropped.
pub(crate) struct Fixture(std::process::Child);

impl Fixture {
    pub(crate) fn start(bin: &Path, env: &crate::runner::Environment) -> Result<Self, Error> {
        let mut command = std::process::Command::new(bin);
        command
            .env_clear()
            .env("PATH", system_path())
            .env("HOME", &env.home)
            .env("TMPDIR", &env.work)
            .env("GCU_FIXTURE_STATE", &env.state_path)
            .current_dir(&env.work)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // A display server and accessibility bus are session facts, not secrets.
        for key in [
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XAUTHORITY",
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let child = command
            .spawn()
            .map_err(|err| Error::msg(format!("fixture spawn: {err}")))?;
        let mut fixture = Self(child);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let ready = std::fs::read(&env.state_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .is_some_and(|value| value.get("counter").is_some());
            if ready {
                // Give the window a moment to be listed by the driver.
                std::thread::sleep(std::time::Duration::from_millis(1500));
                return Ok(fixture);
            }
            if matches!(fixture.0.try_wait(), Ok(Some(_))) {
                return Err(Error::msg("fixture exited before writing its state file"));
            }
            if std::time::Instant::now() >= deadline {
                return Err(Error::msg("fixture did not write its state file in time"));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// System directories only. Callers put the side-effect shims in front of this.
pub(crate) fn system_path() -> &'static str {
    if cfg!(windows) {
        ""
    } else {
        "/usr/bin:/bin:/usr/sbin:/sbin"
    }
}

#[cfg(all(test, unix))]
mod tests {
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
}
