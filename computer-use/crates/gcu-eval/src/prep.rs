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
#[derive(Debug)]
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
        #[cfg(debug_assertions)]
        if fault_inject::take_removal() {
            // Leave the tree. Drop must not erase the failure the report records.
            self.armed = false;
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

/// Why [`begin_env`] did not return a tree.
#[derive(Debug)]
pub(crate) struct BeginFault {
    /// Safe-to-print reason.
    pub message: String,
    /// Root left on disk when populate failed and cleanup also failed.
    pub leftover: Option<PathBuf>,
}

impl BeginFault {
    fn msg(text: impl Into<String>) -> Self {
        Self {
            message: text.into(),
            leftover: None,
        }
    }
}

impl From<BeginFault> for Error {
    fn from(fault: BeginFault) -> Self {
        Error::msg(fault.message)
    }
}

impl From<Error> for BeginFault {
    fn from(err: Error) -> Self {
        Self::msg(err.to_string())
    }
}

#[cfg(debug_assertions)]
mod fault_inject {
    use std::cell::Cell;

    // Thread-local so a parallel test cannot consume another test's fault.
    // `begin_env` runs on the caller's thread under the current-thread runtime.
    thread_local! {
        static FAIL_REMOVAL: Cell<bool> = const { Cell::new(false) };
        static FAIL_POPULATE: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn fail_next_removal() {
        FAIL_REMOVAL.with(|flag| flag.set(true));
    }

    pub(crate) fn fail_next_populate() {
        FAIL_POPULATE.with(|flag| flag.set(true));
    }

    pub(crate) fn take_removal() -> bool {
        FAIL_REMOVAL.with(|flag| flag.replace(false))
    }

    pub(crate) fn take_populate() -> bool {
        FAIL_POPULATE.with(|flag| flag.replace(false))
    }
}

/// The next cleanup of an armed root fails and leaves the tree in place.
///
/// Debug builds only. The flag is thread-local and consumed once. Drop will
/// not delete that tree.
#[cfg(debug_assertions)]
pub fn testing_fail_next_removal() {
    fault_inject::fail_next_removal();
}

/// The next `populate` fails. Pair with [`testing_fail_next_removal`] to keep the root.
#[cfg(debug_assertions)]
pub fn testing_fail_next_populate() {
    fault_inject::fail_next_populate();
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

#[derive(Debug)]
pub(crate) struct HeldEnv {
    pub env: Environment,
    guard: Cleanup,
}

impl HeldEnv {
    pub(crate) fn arm(&mut self) {
        self.guard.arm();
    }

    pub(crate) fn disarm(&mut self) {
        self.guard.disarm();
    }

    pub(crate) fn cleanup(&mut self) -> Result<(), Error> {
        self.guard.remove()
    }
}

/// Create a fresh tree under the process temp directory.
pub(crate) fn begin_env() -> Result<HeldEnv, BeginFault> {
    begin_env_under(&std::env::temp_dir())
}

/// Create a fresh tree under `parent`.
///
/// On Unix the root is created with mode `0700`. A parent that others can write,
/// and that is not sticky, is refused. Windows real mode is refused by the
/// runner; this function does not pretend a Windows directory is owner-only.
pub(crate) fn begin_env_under(parent: &Path) -> Result<HeldEnv, BeginFault> {
    reject_unsafe_parent(parent)?;
    let root = fresh_root_in(parent)?;
    if crate::access::canonical_under_configs(&root, &crate::access::user_config_dirs())? {
        return Err(BeginFault::msg(
            "refusing to provision inside a user config directory",
        ));
    }
    let mut guard = Cleanup::standby(root.clone());
    reject_existing(&root)?;
    create_private_dir(&root)?;
    guard.arm();
    if let Err(err) = populate(&root) {
        if guard.remove().is_err() {
            return Err(BeginFault {
                message: format!("cleanup_failed; {err}"),
                leftover: Some(root),
            });
        }
        return Err(BeginFault::msg(err.to_string()));
    }
    Ok(HeldEnv {
        env: layout(&root),
        guard,
    })
}

fn fresh_root_in(parent: &Path) -> Result<PathBuf, Error> {
    let mut bytes = [0u8; 16];
    random_bytes(&mut bytes)?;
    let mut name = String::from("gcu-ev-");
    for byte in bytes {
        name.push(hex(byte >> 4));
        name.push(hex(byte & 0x0f));
    }
    Ok(parent.join(name))
}

fn reject_unsafe_parent(parent: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(parent)
            .map_err(|err| Error::msg(format!("temp parent: {err}")))?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(Error::msg("temp parent is not a real directory"));
        }
        let mode = meta.mode();
        let writable_by_others = mode & 0o022 != 0;
        let sticky = mode & 0o1000 != 0;
        if writable_by_others && !sticky {
            return Err(Error::msg(
                "refusing a temp parent that others can write and that is not sticky",
            ));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
    }
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<(), Error> {
    reject_existing(path).map_err(|_| Error::msg("refusing a pre-existing startup path"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    Error::msg("refusing a pre-existing startup path")
                } else {
                    Error::msg(format!("mkdir: {err}"))
                }
            })?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                Error::msg("refusing a pre-existing startup path")
            } else {
                Error::msg(format!("mkdir: {err}"))
            }
        })?;
    }
    Ok(())
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
    #[cfg(debug_assertions)]
    if fault_inject::take_populate() {
        let _ = std::fs::write(root.join("leftover"), b"x");
        return Err(Error::msg("populate_failed"));
    }
    let env = layout(root);
    for dir in [&env.home, &env.grok_home, &env.work, &env.shim_dir] {
        make_dir(dir)?;
    }
    write_new(&env.state_path, b"{}\n")?;
    // Tool and side logs stay absent until the fake backend writes a handshake.
    // A missing or empty file is unknown, not a measured zero.
    write_shims(&env.shim_dir)?;
    Ok(())
}

fn make_dir(path: &Path) -> Result<(), Error> {
    create_private_dir(path)
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

#[path = "prep_seed.rs"]
mod seed;
pub(crate) use seed::seed_auth;
#[cfg(all(test, unix))]
pub(crate) use seed::seed_auth_in;

/// S3 uses only a trusted in-memory fake manifest, never fixture state JSON.
pub(crate) async fn start_scope(
    task: &crate::tasks::Task,
    cfg: &crate::runner::RunConfig,
    env: &Environment,
) -> Result<Option<crate::scope::Supervisor>, Error> {
    let Some(scope) = cfg.fake_scope.clone() else {
        return Ok(None);
    };
    crate::scope::validate(cfg)?;
    let binary = cfg
        .gcu_bin
        .as_deref()
        .ok_or_else(|| Error::msg("scope_needs_gcu_binary"))?;
    let run = env
        .root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::msg("scope_run_missing"))?
        .to_owned();
    crate::scope::Supervisor::start(
        binary,
        env,
        &task.scene,
        cfg.mcp_profile.as_deref().unwrap_or("default"),
        run,
        scope,
    )
    .await
    .map(Some)
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
