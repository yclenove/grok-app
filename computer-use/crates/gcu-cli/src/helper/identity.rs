//! Ordinary and scoped helper identities. Path strings only. No store IO.
//!
//! The scoped helper never reads or writes the ordinary socket, auth store,
//! Stop marker, or config. A session [`StoreLedger`] counts the stores it
//! records. The ordinary total must stay zero.

use std::path::{Path, PathBuf};

/// Per-user socket for `gcu agent`.
pub(super) fn ordinary_socket(temp: &Path, uid: u32) -> PathBuf {
    temp.join("grok-computer-use").join(format!("{uid}.sock"))
}

/// Per-user socket for the scoped helper. A different directory, not a suffix
/// of the ordinary socket directory.
pub(super) fn scoped_socket(temp: &Path, uid: u32) -> PathBuf {
    temp.join("grok-computer-use-scoped")
        .join(format!("{uid}.sock"))
}

/// Bundle id of the ordinary helper for this build kind.
pub(super) fn ordinary_bundle(debug: bool) -> &'static str {
    super::auth::bundle_id(debug)
}

/// Bundle id of the scoped helper. It does not match [`ordinary_bundle`].
pub(super) fn scoped_bundle(debug: bool) -> &'static str {
    if debug {
        "ai.grok.computer-use.scoped-helper.dev"
    } else {
        "ai.grok.computer-use.scoped-helper"
    }
}

/// Shape of the ordinary Stop and auth root. Does not create it and does not
/// call [`gcu_host_protocol::stop_reason::state_dir`].
pub(super) fn ordinary_state_root() -> PathBuf {
    state_root("grok-computer-use")
}

/// Shape of the scoped Stop and auth root. Not the ordinary directory.
pub(super) fn scoped_state_root() -> PathBuf {
    state_root("grok-computer-use-scoped")
}

/// True when the scoped helper cannot land on an ordinary endpoint.
///
/// Memory and environment reads only. No directory is created. No timeout or
/// cancellation.
pub(crate) fn identities_are_distinct(temp: &Path, uid: u32) -> bool {
    let ordinary_sock = ordinary_socket(temp, uid);
    let scoped_sock = scoped_socket(temp, uid);
    if ordinary_sock == scoped_sock {
        return false;
    }
    if ordinary_bundle(false) == scoped_bundle(false)
        || ordinary_bundle(true) == scoped_bundle(true)
    {
        return false;
    }
    let ordinary = ordinary_state_root();
    let scoped = scoped_state_root();
    ordinary != scoped && !scoped.starts_with(&ordinary) && !ordinary.starts_with(&scoped)
}

fn state_root(folder: &str) -> PathBuf {
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    let dir = root.unwrap_or_default().join(folder);
    #[cfg(any(windows, target_os = "macos"))]
    let dir = dir.join("state");
    dir
}

/// Uid used only to name a socket. Not an authorization.
pub(crate) fn local_uid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: geteuid reads the process credential and takes no pointer.
        unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// Which helper identity a store operation belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreRole {
    /// Owner's ordinary helper. The scoped path must not construct this.
    ///
    /// Production never builds it. Tests do, to prove a touch would count.
    #[cfg_attr(not(test), allow(dead_code))]
    Ordinary,
    /// Dedicated scoped helper stores.
    Scoped,
}

/// Read and write counts for the two identities. Not process-global.
#[derive(Debug, Default)]
pub(crate) struct StoreLedger {
    ordinary_reads: std::sync::atomic::AtomicUsize,
    ordinary_writes: std::sync::atomic::AtomicUsize,
    scoped_reads: std::sync::atomic::AtomicUsize,
    scoped_writes: std::sync::atomic::AtomicUsize,
}

impl StoreLedger {
    /// Empty counters. Memory only. No timeout or cancellation.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record one store operation for `role`. Memory only.
    pub(crate) fn touch(&self, role: StoreRole, write: bool) {
        let slot = match (role, write) {
            (StoreRole::Ordinary, false) => &self.ordinary_reads,
            (StoreRole::Ordinary, true) => &self.ordinary_writes,
            (StoreRole::Scoped, false) => &self.scoped_reads,
            (StoreRole::Scoped, true) => &self.scoped_writes,
        };
        slot.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Ordinary reads plus writes. Memory only.
    #[must_use]
    pub(crate) fn ordinary_total(&self) -> usize {
        self.ordinary_reads
            .load(std::sync::atomic::Ordering::SeqCst)
            + self
                .ordinary_writes
                .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Scoped reads plus writes. Memory only.
    #[must_use]
    pub(crate) fn scoped_total(&self) -> usize {
        self.scoped_reads.load(std::sync::atomic::Ordering::SeqCst)
            + self.scoped_writes.load(std::sync::atomic::Ordering::SeqCst)
    }
}
