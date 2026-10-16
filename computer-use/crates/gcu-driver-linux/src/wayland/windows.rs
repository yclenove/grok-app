//! Exact compositor window identity. Titles/app IDs are discovery metadata only.

use super::Compositor;
use crate::identity::{needs_reauth, same_process_instance, AppIdentity};
use async_trait::async_trait;
use gcu_core::driver::Cancel;
use std::{future::Future, sync::Arc, time::Duration};

/// Bound for all asynchronous identity/native calls; uses Tokio's injectable clock.
pub const IDENTITY_TIMEOUT: Duration = Duration::from_secs(5);

/// Metadata-only identity failure; no automatic retry of possibly delivered activation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    /// Compositor or requested operation is unsupported.
    #[error("capability unavailable")]
    Unsupported,
    /// Discovery is available, but authoritative window/process binding is not.
    #[error("handoff required: window identity cannot be proven")]
    Handoff,
    /// The exact window disappeared; never substitute a similar title.
    #[error("window gone")]
    WindowGone,
    /// Compositor epoch or window lifetime changed.
    #[error("window identity changed; observe again")]
    StaleWindow,
    /// Process instance or executable changed; the old approval is invalid.
    #[error("process image changed; authorize again")]
    Reauthorize,
    /// Portal/PipeWire session, source, serial or window does not match.
    #[error("stream does not belong to the authorized window")]
    StreamMismatch,
    /// Deadline elapsed.
    #[error("identity operation timed out (maybe_executed={maybe_executed})")]
    Timeout {
        /// Whether activation may have been delivered.
        maybe_executed: bool,
    },
    /// Caller cancelled.
    #[error("identity operation cancelled (maybe_executed={maybe_executed})")]
    Cancelled {
        /// Whether activation may have been delivered.
        maybe_executed: bool,
    },
    /// Transport ended or returned malformed data; payloads are deliberately omitted.
    #[error("compositor transport failed (maybe_executed={maybe_executed})")]
    Transport {
        /// Whether activation may have been delivered.
        maybe_executed: bool,
    },
}

/// A compositor-owned window lifetime. No model-supplied PID/title is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRecord {
    /// Exact compositor ID (not a title or desktop ID).
    pub id: String,
    /// Monotonic object lifetime counter; changes even when an ID is reused.
    pub generation: u64,
    /// Untrusted display metadata.
    pub title: String,
    /// Untrusted application/desktop ID, for discovery only.
    pub app_id: String,
    /// Compositor-reported PID, if available.
    pub pid: Option<u32>,
    /// True only for a native Wayland peer credential, never `_NET_WM_PID`.
    pub peer_pid: bool,
}

/// A coherent read from a pinned compositor instance and extension/script lifetime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    /// Verified compositor backend, not inferred authority from environment variables.
    pub compositor: Compositor,
    /// Unique D-Bus owner + extension generation, or Wayland connection lifetime.
    pub epoch: String,
    /// Read-only window inventory.
    pub windows: Vec<WindowRecord>,
}

/// Server-owned binding retained across authorization and rechecked before delivery.
/// It is intentionally not serializable and is never accepted from model parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowBinding {
    /// Compositor family.
    pub compositor: Compositor,
    /// Compositor and helper lifetime.
    pub epoch: String,
    /// Exact window lifetime.
    pub record: WindowRecord,
    /// OS-inspected executable and process instance.
    pub image: AppIdentity,
}

/// Evidence supplied by the trusted portal adapter after exact source validation.
/// A title/app-id/size match must never construct this evidence. Unknown scope is handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamIdentity {
    /// Owning live portal session (not a reusable restore-token label).
    pub session: String,
    /// PipeWire node.
    pub node_id: u32,
    /// PipeWire object serial, preventing node reuse.
    pub serial: u64,
    /// Exact backend-proven window source, never a monitor/desktop approximation.
    pub source: String,
    /// Proven source window and image binding.
    pub window: WindowBinding,
}

/// Native calls stay behind this trait; fakes work on every CI platform.
#[async_trait]
pub trait WindowBackend: Send + Sync {
    /// Backend family. Unknown families must refuse enumeration and mutation.
    fn compositor(&self) -> Compositor;
    /// Read a coherent snapshot, with a hard timeout and no detached native tasks.
    async fn snapshot(&self) -> Result<WindowSnapshot, IdentityError>;
    /// Recheck the exact epoch, ID, generation and PID inside the compositor before
    /// activation. No title search, no fallback. A lost reply reports maybe-executed.
    async fn activate(
        &self,
        _expected: &WindowSnapshot,
        _window: &WindowRecord,
        _image: &AppIdentity,
    ) -> Result<(), IdentityError> {
        Err(IdentityError::Unsupported)
    }
}

/// OS process/image inspection, isolated from D-Bus and replaceable in tests.
#[async_trait]
pub trait ProcessSource: Send + Sync {
    /// Inspect the running image afresh; do not cache a digest across grant uses.
    /// Implementations bound/cancel blocking work and reject unreadable/replaced images.
    async fn inspect(&self, pid: u32) -> Result<AppIdentity, IdentityError>;
}

/// Identity service shared by discovery, activation and portal source checks.
pub struct WaylandWindows {
    backend: Arc<dyn WindowBackend>,
    processes: Arc<dyn ProcessSource>,
}

impl WaylandWindows {
    /// Construct from trusted backend/process adapters; does not open any connection.
    pub fn new(backend: Arc<dyn WindowBackend>, processes: Arc<dyn ProcessSource>) -> Self {
        Self { backend, processes }
    }

    /// List discovery metadata; cancellable, at most five seconds. Unknown compositors refuse.
    pub async fn list(&self, cancel: &Cancel) -> Result<WindowSnapshot, IdentityError> {
        if !matches!(
            self.backend.compositor(),
            Compositor::Gnome | Compositor::Kde | Compositor::Wlroots
        ) {
            return Err(IdentityError::Unsupported);
        }
        let snapshot = bounded(cancel, false, self.backend.snapshot()).await?;
        if snapshot.compositor != self.backend.compositor()
            || snapshot.epoch.is_empty()
            || snapshot.windows.len() > 4096
        {
            return Err(IdentityError::Handoff);
        }
        let mut ids = std::collections::HashSet::new();
        for window in &snapshot.windows {
            if window.id.is_empty() || window.generation == 0 || !ids.insert(&window.id) {
                return Err(IdentityError::Handoff);
            }
        }
        Ok(snapshot)
    }

    /// Resolve only an exact compositor ID and bind to the OS-inspected approved image.
    /// No blocking on the reactor; cancellation/timeout occur before any mutation.
    pub async fn bind(
        &self,
        id: &str,
        approved: &AppIdentity,
        cancel: &Cancel,
    ) -> Result<WindowBinding, IdentityError> {
        let snapshot = self.list(cancel).await?;
        let record = snapshot
            .windows
            .iter()
            .find(|w| w.id == id)
            .ok_or(IdentityError::WindowGone)?
            .clone();
        let image = self.inspect(&record, cancel).await?;
        if !same_process_instance(approved, &image) || needs_reauth(approved, &image) {
            return Err(IdentityError::Reauthorize);
        }
        let binding = WindowBinding {
            compositor: snapshot.compositor,
            epoch: snapshot.epoch,
            record,
            image,
        };
        self.revalidate(&binding, cancel).await?;
        Ok(binding)
    }

    async fn inspect(
        &self,
        record: &WindowRecord,
        cancel: &Cancel,
    ) -> Result<AppIdentity, IdentityError> {
        if !record.peer_pid {
            return Err(IdentityError::Handoff);
        }
        let pid = record
            .pid
            .filter(|p| *p > 0)
            .ok_or(IdentityError::Handoff)?;
        let image = bounded(cancel, false, self.processes.inspect(pid)).await?;
        if image.pid != pid
            || image.start_time == 0
            || !image.exe_path.is_absolute()
            || image.sha256.len() != 64
            || !image.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(IdentityError::Handoff);
        }
        Ok(image)
    }

    /// Revalidate window lifetime and process/image after approval, before every use.
    /// Titles and desktop IDs may change without changing identity. Cancellable, bounded.
    pub async fn revalidate(
        &self,
        binding: &WindowBinding,
        cancel: &Cancel,
    ) -> Result<WindowSnapshot, IdentityError> {
        let snapshot = self.list(cancel).await?;
        if snapshot.compositor != binding.compositor || snapshot.epoch != binding.epoch {
            return Err(IdentityError::StaleWindow);
        }
        let record = snapshot
            .windows
            .iter()
            .find(|w| w.id == binding.record.id)
            .ok_or(IdentityError::WindowGone)?;
        if record.generation != binding.record.generation
            || record.pid != binding.record.pid
            || record.peer_pid != binding.record.peer_pid
        {
            return Err(IdentityError::StaleWindow);
        }
        let actual = self.inspect(record, cancel).await?;
        if needs_reauth(&binding.image, &actual) || !same_process_instance(&binding.image, &actual)
        {
            return Err(IdentityError::Reauthorize);
        }
        Ok(snapshot)
    }

    /// Activate only when the Router explicitly permits foreground delivery.
    /// The backend fences identity again in the compositor; cancellation/lost replies
    /// after dispatch report maybe-executed and must never be automatically retried.
    pub async fn activate(
        &self,
        binding: &WindowBinding,
        allow_foreground: bool,
        cancel: &Cancel,
    ) -> Result<(), IdentityError> {
        if !allow_foreground {
            return Err(IdentityError::Unsupported);
        }
        let snapshot = self.revalidate(binding, cancel).await?;
        let record = snapshot
            .windows
            .iter()
            .find(|w| w.id == binding.record.id)
            .ok_or(IdentityError::WindowGone)?;
        bounded(
            cancel,
            true,
            self.backend.activate(&snapshot, record, &binding.image),
        )
        .await
    }

    /// Check current stream evidence against the server-retained grant and window.
    /// P7-02 must independently prove window-only scope; matching labels cannot do so.
    pub async fn check_stream(
        &self,
        binding: &WindowBinding,
        granted: &StreamIdentity,
        actual: &StreamIdentity,
        cancel: &Cancel,
    ) -> Result<(), IdentityError> {
        if granted != actual
            || &granted.window != binding
            || granted.session.is_empty()
            || granted.source.is_empty()
            || granted.node_id == 0
            || granted.serial == 0
        {
            return Err(IdentityError::StreamMismatch);
        }
        self.revalidate(binding, cancel).await.map(|_| ())
    }
}

pub(crate) async fn bounded<T>(
    cancel: &Cancel,
    maybe_executed: bool,
    future: impl Future<Output = Result<T, IdentityError>>,
) -> Result<T, IdentityError> {
    if cancel.is_cancelled() {
        return Err(IdentityError::Cancelled {
            maybe_executed: false,
        });
    }
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(IdentityError::Cancelled { maybe_executed }),
        result = tokio::time::timeout(IDENTITY_TIMEOUT, future) => result.unwrap_or(Err(IdentityError::Timeout { maybe_executed })),
    }
}

/// Live Linux process inspector. Uses an opened `/proc/PID/exe` image, not a desktop entry.
#[derive(Debug, Default)]
pub struct NativeProcesses;

#[async_trait]
impl ProcessSource for NativeProcesses {
    async fn inspect(&self, pid: u32) -> Result<AppIdentity, IdentityError> {
        // Dropping the caller (timeout/cancellation) signals the blocking hashing loop.
        struct Stop(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Stop {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        let stop = Stop(Arc::new(std::sync::atomic::AtomicBool::new(false)));
        let flag = stop.0.clone();
        tokio::time::timeout(
            IDENTITY_TIMEOUT,
            tokio::task::spawn_blocking(move || inspect_process(pid, &flag)),
        )
        .await
        .map_err(|_| IdentityError::Timeout {
            maybe_executed: false,
        })?
        .map_err(|_| IdentityError::Transport {
            maybe_executed: false,
        })?
    }
}

#[cfg(target_os = "linux")]
fn inspect_process(
    pid: u32,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<AppIdentity, IdentityError> {
    use sha2::{Digest, Sha256};
    use std::{fs, io::Read, os::unix::fs::MetadataExt, sync::atomic::Ordering};
    let inspect = || -> std::io::Result<AppIdentity> {
        let start_time = crate::identity::get_process_start_time(pid)?;
        let link = format!("/proc/{pid}/exe");
        let exe_path = fs::canonicalize(&link)?;
        let mut file = fs::File::open(&link)?;
        let before = file.metadata()?;
        let stamp = |m: &fs::Metadata| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
            )
        };
        if !before.is_file()
            || before.len() > 256 * 1024 * 1024
            || stamp(&before) != stamp(&fs::metadata(&exe_path)?)
        {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        let mut total = 0u64;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > 256 * 1024 * 1024 {
                return Err(std::io::ErrorKind::InvalidData.into());
            }
            hash.update(&buffer[..count]);
        }
        if total != before.len()
            || stamp(&before) != stamp(&file.metadata()?)
            || stamp(&before) != stamp(&fs::metadata(&exe_path)?)
            || stamp(&before) != stamp(&fs::metadata(&link)?)
            || exe_path != fs::canonicalize(&link)?
            || start_time != crate::identity::get_process_start_time(pid)?
        {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        Ok(AppIdentity {
            exe_path,
            dev: before.dev(),
            inode: before.ino(),
            sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
            pid,
            start_time,
            desktop_id: None,
        })
    };
    inspect().map_err(|_| IdentityError::Handoff)
}

#[cfg(not(target_os = "linux"))]
fn inspect_process(
    _pid: u32,
    _cancelled: &std::sync::atomic::AtomicBool,
) -> Result<AppIdentity, IdentityError> {
    Err(IdentityError::Unsupported)
}
