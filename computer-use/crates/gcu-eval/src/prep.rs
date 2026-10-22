//! Temporary directory helpers for the local evaluation provider.
//!
//! The cleanup guard is installed before any directory is created. It deletes
//! a tree only after this process exclusively created that tree. A pre-existing
//! path, including a symlink, is refused and left in place.

use crate::runner::{Environment, RunStatus};
use crate::Error;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Deletes `root` on drop only after [`Cleanup::arm`].
pub(crate) struct Cleanup {
    root: PathBuf,
    armed: bool,
}

impl Cleanup {
    /// Does not delete `root`. Arm it only after exclusive creation succeeds.
    pub(crate) fn standby(root: PathBuf) -> Self {
        Self { root, armed: false }
    }

    pub(crate) fn arm(&mut self) {
        self.armed = true;
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }

    /// Remove an armed root. Failure stays an error for the caller to report.
    pub(crate) fn remove(&mut self) -> Result<(), Error> {
        if !self.armed {
            return Err(Error::msg("cleanup_failed"));
        }
        remove_root(&self.root)?;
        if self.root.exists() {
            return Err(Error::msg("cleanup_failed"));
        }
        self.armed = false;
        Ok(())
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// Downgrade a finished row when deleting its tree failed.
pub(crate) fn apply_cleanup_failure(status: &mut RunStatus, reason: &mut String) {
    if matches!(
        *status,
        RunStatus::Pass | RunStatus::Partial | RunStatus::NotRun
    ) {
        *status = RunStatus::Fail;
    }
    if reason.contains("cleanup_failed") {
        return;
    }
    if reason.is_empty() {
        reason.push_str("cleanup_failed");
    } else {
        *reason = format!("cleanup_failed; {reason}");
    }
}

pub(crate) struct HeldEnv {
    pub env: Environment,
    guard: Cleanup,
}

impl HeldEnv {
    pub(crate) fn disarm(&mut self) {
        self.guard.disarm();
    }

    pub(crate) fn cleanup(&mut self) -> Result<(), Error> {
        self.guard.remove()
    }
}

/// Create a fresh tree. The returned guard is armed only after the root exists.
pub(crate) fn begin_env() -> Result<HeldEnv, Error> {
    let root = fresh_root()?;
    if crate::access::canonical_under_configs(&root, &crate::access::user_config_dirs())? {
        return Err(Error::msg(
            "refusing to provision inside a user config directory",
        ));
    }
    let mut guard = Cleanup::standby(root.clone());
    reject_existing(&root)?;
    std::fs::create_dir(&root).map_err(|err| {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            Error::msg("temp root already exists")
        } else {
            Error::msg(format!("temp root: {err}"))
        }
    })?;
    guard.arm();
    if let Err(err) = lock_dir(&root).and_then(|()| populate(&root)) {
        if guard.remove().is_err() {
            return Err(Error::msg(format!("cleanup_failed; {err}")));
        }
        return Err(err);
    }
    Ok(HeldEnv {
        env: layout(&root),
        guard,
    })
}

fn fresh_root() -> Result<PathBuf, Error> {
    let mut bytes = [0u8; 16];
    random_bytes(&mut bytes)?;
    let mut name = String::from("gcu-ev-");
    for byte in bytes {
        name.push(hex(byte >> 4));
        name.push(hex(byte & 0x0f));
    }
    Ok(std::env::temp_dir().join(name))
}

fn hex(nibble: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(nibble)])
}

#[cfg(unix)]
fn random_bytes(buf: &mut [u8]) -> Result<(), Error> {
    use std::io::Read;
    let mut file =
        std::fs::File::open("/dev/urandom").map_err(|_| Error::msg("could not read randomness"))?;
    file.read_exact(buf)
        .map_err(|_| Error::msg("could not read randomness"))
}

#[cfg(windows)]
fn random_bytes(buf: &mut [u8]) -> Result<(), Error> {
    crate::ffi::random_bytes(buf)
}

fn reject_existing(path: &Path) -> Result<(), Error> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(Error::msg("refusing a symlink temp root"))
        }
        Ok(_) => Err(Error::msg("temp root already exists")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::msg(format!("temp root: {err}"))),
    }
}

fn layout(root: &Path) -> Environment {
    Environment {
        home: root.join("home"),
        grok_home: root.join("grok"),
        work: root.join("work"),
        state_path: root.join("state.json"),
        shim_dir: root.join("shims"),
        side_log: root.join("side.log"),
        tool_log: root.join("tools.log"),
        transcript: root.join("grok").join("transcript.json"),
        root: root.to_path_buf(),
    }
}

fn populate(root: &Path) -> Result<(), Error> {
    let env = layout(root);
    for dir in [&env.home, &env.grok_home, &env.work, &env.shim_dir] {
        make_dir(dir)?;
    }
    write_new(&env.state_path, b"{}\n")?;
    write_new(&env.side_log, b"")?;
    write_shims(&env.shim_dir)?;
    Ok(())
}

fn make_dir(path: &Path) -> Result<(), Error> {
    reject_existing(path).map_err(|_| Error::msg("refusing a pre-existing startup path"))?;
    std::fs::create_dir(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            Error::msg("refusing a pre-existing startup path")
        } else {
            Error::msg(format!("mkdir: {err}"))
        }
    })?;
    lock_dir(path)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(Error::msg("refusing a pre-existing startup path"));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| Error::msg("refusing a pre-existing startup path"))?;
    file.write_all(bytes)
        .map_err(|err| Error::msg(format!("create: {err}")))
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
        let body = "#!/bin/sh\nname=${0##*/}\necho \"process $name\" >> \"$GCU_EVAL_SIDE_LOG\"\necho \"network argc=$#\" >> \"$GCU_EVAL_SIDE_LOG\"\nexit 1\n";
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
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
        let body = "@echo off\r\necho process %~n0>>\"%GCU_EVAL_SIDE_LOG%\"\r\necho network argc>>\"%GCU_EVAL_SIDE_LOG%\"\r\nexit /b 1\r\n";
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|err| Error::msg(format!("shim: {err}")))?;
        file.write_all(body.as_bytes())
            .map_err(|err| Error::msg(format!("shim: {err}")))
    }
}

/// Copy the evaluation login into a task's temporary `GROK_HOME`.
///
/// Symlinks are refused. A seed whose canonical path is under `~/.grok`,
/// `~/.codex`, `~/.gemini`, or `~/.claude` is refused. The file contents are
/// not logged. Only `auth.json` is taken from `seed`.
pub(crate) fn seed_auth(seed: &Path, grok_home: &Path) -> Result<(), Error> {
    seed_auth_in(seed, grok_home, &crate::access::user_config_dirs())
}

pub(crate) fn seed_auth_in(
    seed: &Path,
    grok_home: &Path,
    configs: &[PathBuf],
) -> Result<(), Error> {
    if symlink_path(seed)? {
        return Err(Error::msg("refusing a symlink auth seed"));
    }
    let from = seed.join("auth.json");
    if symlink_path(&from)? {
        return Err(Error::msg("refusing a symlink auth seed"));
    }
    let meta =
        std::fs::symlink_metadata(&from).map_err(|_| Error::msg("auth seed has no auth.json"))?;
    if !meta.is_file() {
        return Err(Error::msg("auth seed has no auth.json"));
    }
    let seed_canon =
        std::fs::canonicalize(seed).map_err(|_| Error::msg("auth seed has no auth.json"))?;
    let file_canon =
        std::fs::canonicalize(&from).map_err(|_| Error::msg("auth seed has no auth.json"))?;
    if crate::access::canonical_under_configs(&seed_canon, configs)?
        || crate::access::canonical_under_configs(&file_canon, configs)?
    {
        return Err(Error::msg(
            "refusing an auth seed under a personal config directory",
        ));
    }
    let bytes = std::fs::read(&from).map_err(|_| Error::msg("could not copy the auth seed"))?;
    let to = grok_home.join("auth.json");
    if std::fs::symlink_metadata(&to).is_ok() {
        return Err(Error::msg("could not copy the auth seed"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&to)
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    file.write_all(&bytes)
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    Ok(())
}

fn symlink_path(path: &Path) -> Result<bool, Error> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.file_type().is_symlink()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Error::msg("auth seed has no auth.json")),
    }
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
#[path = "prep_tests.rs"]
mod tests;
