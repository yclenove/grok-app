//! Process-lifetime ownership of RESTORED clipboard originals. This does not
//! prove paste completion and must never be used to restore an unfinished input.
//! Dropping the last control owner requests manager handoff, never destruction.
//! Explicit keeper preparation transfers to a process; App exit still needs a gate.
use super::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

const MAX_SERVICES: usize = 8;
static LIVE_SERVICES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionState {
    Starting,
    Serving,
    Faulted,
    Closed,
}

/// Metadata/errors only. Never contains a clipboard payload or target name.
#[derive(Clone, Debug)]
pub struct RetentionStatus {
    pub state: RetentionState,
    pub handoff: HandoffStatus,
    pub keeper: KeeperStatus,
    pub error: Option<String>,
    pub worker_finished: bool,
}

struct Shared {
    status: Mutex<RetentionStatus>,
    handoff_requested: AtomicBool,
    keeper_requested: AtomicBool,
    keeper_process: Mutex<Option<KeeperProcess>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Shared {
    fn status(&self) -> RetentionStatus {
        let mut status = self.status.lock().clone();
        status.worker_finished = self
            .worker
            .lock()
            .as_ref()
            .is_some_and(JoinHandle::is_finished);
        status
    }
}

/// Read-only status observation does not keep an action/adapter control alive.
#[derive(Clone)]
pub struct ClipboardMonitor {
    shared: Arc<Shared>,
}
impl ClipboardMonitor {
    pub fn status(&self) -> RetentionStatus {
        self.shared.status()
    }
    pub fn keeper_process(&self) -> Option<KeeperProcess> {
        self.shared.keeper_process.lock().clone()
    }
}
struct Control {
    shared: Arc<Shared>,
}
impl Drop for Control {
    fn drop(&mut self) {
        // The worker owns the selection connection independently of Control.
        // Neither dropping a handle nor an abort destroys that connection.
        self.shared.handoff_requested.store(true, Ordering::Release);
    }
}

#[derive(Clone)]
pub struct RetainedClipboard {
    control: Arc<Control>,
}
impl RetainedClipboard {
    /// Explicit nonblocking exit preparation; inspect status, not just spawn.
    pub fn request_process_handoff(&self) {
        self.control
            .shared
            .keeper_requested
            .store(true, Ordering::Release);
    }
    /// A nonblocking request, never a promise that closing is safe. If a manager
    /// was absent, an explicit later request may discover a newly started one;
    /// a dispatched SAVE_TARGETS request is never replayed.
    pub fn request_handoff(&self) {
        self.control
            .shared
            .handoff_requested
            .store(true, Ordering::Release);
    }
    pub fn status(&self) -> RetentionStatus {
        self.control.shared.status()
    }
    pub fn monitor(&self) -> ClipboardMonitor {
        ClipboardMonitor {
            shared: self.control.shared.clone(),
        }
    }
}

#[derive(Default)]
pub struct ClipboardRetention {
    retained: Mutex<Vec<RetainedClipboard>>,
}
impl ClipboardRetention {
    /// Caller keeps the exact service and lease on EVERY failure, including
    /// admission/spawn/channel errors. No fallible handoff may silently Drop it.
    pub fn retain(
        &self,
        pending: &mut Option<(ClipboardOwner, ClipboardLease)>,
    ) -> Result<RetainedClipboard, String> {
        let (owner, lease) = pending
            .as_ref()
            .ok_or("missing restored clipboard service")?;
        owner.check_lease(lease)?;
        if owner.state() != ClipboardState::Restored {
            return Err("only restored originals may enter clipboard retention".into());
        }
        let permit = Permit::acquire()?;
        let shared = Arc::new(Shared {
            status: Mutex::new(RetentionStatus {
                state: RetentionState::Starting,
                handoff: owner.handoff_status(),
                keeper: owner.keeper_status(),
                error: None,
                worker_finished: false,
            }),
            handoff_requested: AtomicBool::new(false),
            keeper_requested: AtomicBool::new(false),
            keeper_process: Mutex::new(None),
            worker: Mutex::new(None),
        });
        let (tx, rx) = mpsc::sync_channel::<(ClipboardOwner, ClipboardLease)>(1);
        let state = shared.clone();
        let worker = std::thread::Builder::new()
            .name("cu-x11-clipboard".into())
            .spawn(move || {
                let _permit = permit;
                if let Ok((mut owner, lease)) = rx.recv() {
                    serve(&mut owner, &lease, &state);
                }
            })
            .map_err(|_| "could not start clipboard retention worker")?;
        *shared.worker.lock() = Some(worker);
        let service = pending
            .take()
            .ok_or("restored clipboard service disappeared")?;
        if let Err(error) = tx.send(service) {
            *pending = Some(error.0);
            return Err("clipboard retention worker did not accept ownership".into());
        }
        let handle = RetainedClipboard {
            control: Arc::new(Control { shared }),
        };
        let mut retained = self.retained.lock();
        retained.retain(|h| !finished(h));
        retained.push(handle.clone());
        Ok(handle)
    }
    pub fn statuses(&self) -> Vec<RetentionStatus> {
        let mut retained = self.retained.lock();
        retained.retain(|h| !finished(h));
        retained.iter().map(RetainedClipboard::status).collect()
    }
    pub fn request_handoff(&self) {
        for handle in self.retained.lock().iter() {
            handle.request_handoff();
        }
    }
}

fn finished(handle: &RetainedClipboard) -> bool {
    let status = handle.status();
    status.state == RetentionState::Closed && status.worker_finished
}

struct Permit;
impl Permit {
    fn acquire() -> Result<Self, String> {
        LIVE_SERVICES
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_SERVICES).then_some(n + 1)
            })
            .map_err(|_| "clipboard retention capacity occupied; retain caller's service")?;
        Ok(Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        LIVE_SERVICES.fetch_sub(1, Ordering::AcqRel);
    }
}

fn serve(owner: &mut ClipboardOwner, lease: &ClipboardLease, shared: &Shared) {
    // A Result error never exits this loop with a still-owned clipboard. The
    // original service keeps its pinned transfers, X connection and snapshot.
    loop {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owner.pump()?;
            if shared.keeper_requested.swap(false, Ordering::AcqRel)
                && owner.state() == ClipboardState::Restored
                && !owner.saved_was_empty()
                && owner.keeper_status() == KeeperStatus::NotRequested
            {
                owner.begin_keeper(lease, &Default::default())?;
                *shared.keeper_process.lock() = owner.keeper_process();
            } else if shared.handoff_requested.swap(false, Ordering::AcqRel)
                && owner.state() == ClipboardState::Restored
                && !owner.saved_was_empty()
                && !owner.keeper_pending()
                && matches!(
                    owner.handoff_status(),
                    HandoffStatus::NotRequested | HandoffStatus::Unavailable
                )
            {
                owner.begin_handoff(lease, &Default::default())?;
            }
            // Distinct busy result; native errors must remain visible.
            owner.try_close()
        }));
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                *shared.status.lock() = RetentionStatus {
                    state: RetentionState::Faulted,
                    handoff: owner.handoff_status(),
                    keeper: owner.keeper_status(),
                    error: Some(
                        "clipboard worker panicked; retained connection requires explicit recovery"
                            .into(),
                    ),
                    worker_finished: false,
                };
                // Do not unwind/drop the original. Unknown internal invariants
                // cannot justify more native work or a fabricated safe close.
                loop {
                    std::thread::park_timeout(Duration::from_secs(60));
                }
            }
        };
        let closed = matches!(result, Ok(true));
        let error = result.err();
        *shared.status.lock() = RetentionStatus {
            state: if closed {
                RetentionState::Closed
            } else if error.is_some() {
                RetentionState::Faulted
            } else {
                RetentionState::Serving
            },
            handoff: owner.handoff_status(),
            keeper: owner.keeper_status(),
            error,
            worker_finished: false,
        };
        if closed {
            return;
        }
        std::thread::sleep(Duration::from_millis(if owner.active_transfers() > 0 {
            1
        } else {
            5
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn global_admission_bound_releases_only_its_own_permits() {
        let held: Vec<_> = (0..MAX_SERVICES)
            .map(|_| Permit::acquire().unwrap())
            .collect();
        assert!(Permit::acquire().is_err());
        let mut held = held;
        drop(held.pop());
        let replacement = Permit::acquire().unwrap();
        assert!(Permit::acquire().is_err());
        drop(replacement);
        drop(held);
        assert_eq!(LIVE_SERVICES.load(Ordering::Acquire), 0);
    }
}
