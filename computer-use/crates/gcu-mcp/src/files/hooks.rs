//! Test pauses and ledger holds. Production does not compile this module.

use super::{Area, SessionFiles};
use std::{
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::Duration,
};

/// One-shot pause around a session-file write in a test.
///
/// Memory only. The selected [`SessionFiles::stage`] waits until [`Self::release`].
/// Later writes do not wait. Panics after 15 seconds. No cancellation. Production never
/// constructs this value.
pub(crate) struct FilePause {
    state: Mutex<PauseState>,
    cv: Condvar,
}

struct PauseState {
    armed: bool,
    used: bool,
    parked: bool,
    released: bool,
    finished: bool,
    /// Writes to ignore before the one that parks. Zero parks the next write.
    skip: u32,
}

impl FilePause {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(PauseState {
                armed: false,
                used: false,
                parked: false,
                released: false,
                finished: false,
                skip: 0,
            }),
            cv: Condvar::new(),
        })
    }

    fn arm_skip(&self, skip: u32) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.armed = true;
        guard.released = false;
        guard.parked = false;
        guard.finished = false;
        guard.used = false;
        guard.skip = skip;
    }

    fn pause_once(&self) -> bool {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if !guard.armed || guard.used {
            return false;
        }
        if guard.skip > 0 {
            guard.skip -= 1;
            return false;
        }
        guard.used = true;
        guard.parked = true;
        self.cv.notify_all();
        let (mut guard, timeout) = self
            .cv
            .wait_timeout_while(guard, Duration::from_secs(15), |state| !state.released)
            .unwrap_or_else(|err| err.into_inner());
        let expired = timeout.timed_out() && !guard.released;
        guard.parked = false;
        guard.armed = false;
        drop(guard);
        assert!(
            !expired,
            "file test gate was not released within 15 seconds"
        );
        true
    }

    /// Unblock the write parked in this gate.
    ///
    /// Memory only. No timeout or cancellation.
    pub(crate) fn release(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.released = true;
        guard.armed = false;
        self.cv.notify_all();
    }

    /// Whether the selected write is waiting. Memory only. No timeout or cancellation.
    pub(crate) fn is_parked(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .parked
    }

    /// The staged file from this gate was dropped or committed.
    ///
    /// Memory only. No timeout or cancellation.
    pub(crate) fn has_finished(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .finished
    }

    /// Record that the staged file was dropped or committed.
    ///
    /// Called from [`super::StagedFile`]'s destructor. Memory only. No timeout
    /// or cancellation.
    pub(super) fn note_finished(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        guard.finished = true;
        self.cv.notify_all();
    }
}

/// Area mutex held by a test. Drop releases it and does not remove rows.
pub(crate) struct AreaHold<'a> {
    _guard: MutexGuard<'a, Area>,
}

impl SessionFiles {
    /// Run `hook` once, on the next [`Self::export`], before it locks the ledger.
    ///
    /// Test only. The hook runs on the exporter thread and must not take this
    /// ledger lock. Memory only. No timeout or cancellation.
    pub(crate) fn arm_before_export_lock(&self, hook: impl Fn() + Send + Sync + 'static) {
        *self
            .before_export
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Box::new(hook));
    }

    /// Observe the next [`Self::close`] or drop while the owner lock is held.
    ///
    /// Test only. The callback receives `(owner lock and directory still held,
    /// every registered path already unlinked)`. It runs before the directory
    /// is removed. Memory only. No timeout or cancellation.
    pub(crate) fn arm_purge_probe(&self, probe: impl Fn(bool, bool) + Send + Sync + 'static) {
        *self
            .purge_probe
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Box::new(probe));
    }

    /// Hold the ledger lock until the guard drops. Rows stay as they are.
    ///
    /// Test only. Blocking: the area mutex. No timeout or cancellation.
    pub(crate) fn lock_area_for_test(&self) -> AreaHold<'_> {
        AreaHold {
            _guard: self.area.lock().unwrap_or_else(|error| error.into_inner()),
        }
    }

    /// Park the next file write until the returned gate is released.
    ///
    /// Test only. Memory only. No timeout or cancellation. The pause happens
    /// before the file-area mutex is taken, so export can run while it waits.
    /// Only the first write parks.
    pub(crate) fn arm_file_gate(&self) -> Arc<FilePause> {
        self.arm_file_gate_at(1)
    }

    /// Park the `nth` file write (1-based). Earlier writes do not wait.
    ///
    /// Test only. Memory only. No timeout or cancellation. The pause happens
    /// before the file-area mutex is taken. `1` matches [`Self::arm_file_gate`].
    pub(crate) fn arm_file_gate_at(&self, nth: u32) -> Arc<FilePause> {
        let pause = FilePause::new();
        pause.arm_skip(nth.saturating_sub(1));
        *self.gate.lock().unwrap_or_else(|err| err.into_inner()) = Some(Arc::clone(&pause));
        pause
    }

    /// Take the armed pause, if this write is the one selected to wait.
    ///
    /// Test only. Called from [`SessionFiles::stage`] before the area mutex.
    /// Memory only. No timeout or cancellation aside from the gate's own bound.
    pub(super) fn pause_for_test(&self) -> Option<Arc<FilePause>> {
        let gate = self
            .gate
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        gate.filter(|gate| gate.pause_once())
    }

    /// Whether any file in this session directory contains `needle`.
    ///
    /// Includes names that are not in the export ledger. Test only. May wait on
    /// the area mutex and reads the directory. No timeout or cancellation.
    pub(crate) fn contains_bytes(&self, needle: &[u8]) -> bool {
        if needle.is_empty() {
            return false;
        }
        let root = {
            let area = self.area.lock().unwrap_or_else(|err| err.into_inner());
            area.directory.as_ref().map(|dir| dir.path().to_path_buf())
        };
        let Some(root) = root else {
            return false;
        };
        let Ok(entries) = std::fs::read_dir(root) else {
            return false;
        };
        for entry in entries.flatten() {
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            if bytes.windows(needle.len()).any(|window| window == needle) {
                return true;
            }
        }
        false
    }
}
