// Pointer-gate acquire order and the host/worker name.
// Included from `hook_probe.rs` so the `#[path]` policy tests compile the same items.
// The std gate below is the test double: same-thread reentry matches a Win32 mutex, and each
// test can share one instance across threads. It is not the kernel object. The named mutex is
// `cfg(all(windows, not(test)))` and is what `tests/pointer_gate.rs` calls.
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};
#[cfg(windows)]
use std::marker::PhantomData;

pub(super) const HOST_PID_ENV: &str = "GCU_HOST_PID";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GateRole {
    Host,
    Worker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkerHostPidError {
    Missing,
    Invalid,
}

impl WorkerHostPidError {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::Missing => {
                "worker refused to start: GCU_HOST_PID is missing; the pointer gate would not match the host"
            }
            Self::Invalid => {
                "worker refused to start: GCU_HOST_PID is invalid; the pointer gate would not match the host"
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum Bound {
    Worker(u32),
    Refused,
}

/// Read `GCU_HOST_PID` through `read`. Tests pass a closure; production passes `std::env::var`.
pub(super) fn inherited_host_pid(read: impl Fn(&str) -> Option<String>) -> Option<String> {
    read(HOST_PID_ENV)
}

pub(super) fn parse_worker_host_pid(value: Option<&str>) -> Result<u32, WorkerHostPidError> {
    let Some(raw) = value.map(str::trim).filter(|text| !text.is_empty()) else {
        return Err(WorkerHostPidError::Missing);
    };
    match raw.parse::<u32>() {
        Ok(pid) if pid != 0 => Ok(pid),
        _ => Err(WorkerHostPidError::Invalid),
    }
}

/// Host always uses `own_pid` and ignores `inherited`. Worker uses only a validated parent pid.
pub(super) fn gate_pid(
    role: GateRole,
    own_pid: u32,
    inherited: Option<&str>,
) -> Result<u32, WorkerHostPidError> {
    match role {
        GateRole::Host => Ok(own_pid),
        GateRole::Worker => parse_worker_host_pid(inherited),
    }
}

pub(super) fn pointer_gate_name(pid: u32) -> String {
    format!("Local\\gcu-pointer-gate-{pid}")
}

/// Install a worker binding. A missing or invalid value refuses the slot so a later acquire
/// cannot fall back to this process's pid. The same pid may be installed twice.
pub(super) fn install_worker_pid(
    slot: &OnceLock<Bound>,
    value: Option<&str>,
) -> Result<u32, WorkerHostPidError> {
    match parse_worker_host_pid(value) {
        Ok(pid) => match slot.set(Bound::Worker(pid)) {
            Ok(()) => Ok(pid),
            Err(_) => match slot.get() {
                Some(Bound::Worker(existing)) if *existing == pid => Ok(pid),
                _ => Err(WorkerHostPidError::Invalid),
            },
        },
        Err(error) => {
            let _ = slot.set(Bound::Refused);
            Err(error)
        }
    }
}

pub(super) fn owner_from_binding(
    bound: Option<&Bound>,
    own_pid: u32,
) -> Result<u32, super::GateBusy> {
    match bound {
        Some(Bound::Worker(pid)) => Ok(*pid),
        Some(Bound::Refused) => Err(super::GateBusy),
        // Host role: `inherited` is ignored. Unbound means this process.
        None => match gate_pid(GateRole::Host, own_pid, None) {
            Ok(pid) => Ok(pid),
            Err(_) => Err(super::GateBusy),
        },
    }
}

#[cfg(all(windows, not(test)))]
static BOUND: OnceLock<Bound> = OnceLock::new();

#[cfg(all(windows, not(test)))]
pub(super) fn bind_from_process_env() -> gcu_core::driver::Result<()> {
    let value = inherited_host_pid(|key| std::env::var(key).ok());
    // Same parser the tests drive. Own pid is not a fallback: `gate_pid(Worker)` never reads it.
    // A failure must refuse the slot before return, or a later acquire would use this process.
    if gate_pid(GateRole::Worker, std::process::id(), value.as_deref()).is_err() {
        let _ = BOUND.set(Bound::Refused);
    }
    install_worker_pid(&BOUND, value.as_deref())
        .map(|_| ())
        .map_err(|error| gcu_core::driver::DriverError::Native {
            message: error.message().to_string(),
            maybe_executed: false,
        })
}

#[cfg(windows)]
pub(super) fn owner_pid_for_test() -> Result<u32, super::GateBusy> {
    #[cfg(not(test))]
    {
        owner_from_binding(BOUND.get(), std::process::id())
    }
    #[cfg(test)]
    {
        Ok(std::process::id())
    }
}

fn log_wait(started: Instant) {
    let waited = started.elapsed();
    if waited > Duration::from_millis(3) {
        tracing::info!(
            target: "gcu_windows_timing",
            stage = "pointer_gate_wait",
            wait_ms = waited.as_millis() as u64
        );
    }
}

/// Take the gate, run `admit`, then `body`, then release.
/// `Err(GateBusy)` means nothing ran. `Ok(Err(_))` means admission failed, `body` did not run,
/// and the gate was released.
pub(crate) fn with_admitted_gate<E, R>(
    wait: Duration,
    admit: impl FnOnce() -> Result<(), E>,
    body: impl FnOnce() -> Result<R, E>,
) -> Result<Result<R, E>, super::GateBusy> {
    #[cfg(test)]
    {
        with_admitted_gate_on(&current_gate(), wait, admit, body)
    }
    #[cfg(not(test))]
    {
        let started = Instant::now();
        let held = acquire(wait)?;
        log_wait(started);
        let outcome = admit().and_then(|()| body());
        drop(held);
        Ok(outcome)
    }
}

#[cfg(all(windows, not(test)))]
mod win32 {
    use super::{owner_from_binding, pointer_gate_name, BOUND};
    use std::{marker::PhantomData, sync::OnceLock, time::Duration};
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
            System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        },
    };

    pub(super) struct Held {
        bits: isize,
        _thread: PhantomData<*const ()>,
    }
    impl Drop for Held {
        fn drop(&mut self) {
            if self.bits != 0 {
                // SAFETY: releases the mutex this thread acquired in `acquire`.
                unsafe {
                    let _ = ReleaseMutex(HANDLE(self.bits as *mut _));
                }
            }
        }
    }
    pub(super) fn acquire(wait: Duration) -> Result<Held, super::super::GateBusy> {
        // Debug/test workers only: lets the real-hardware stress test compare gate on and off.
        #[cfg(all(feature = "test-worker-faults", debug_assertions))]
        if std::env::var_os("GCU_POINTER_GATE_OFF").is_some() {
            return Ok(Held {
                bits: 0,
                _thread: PhantomData,
            });
        }
        static HANDLE_BITS: OnceLock<isize> = OnceLock::new();
        let bits = *HANDLE_BITS.get_or_init(|| {
            let Ok(host) = owner_from_binding(BOUND.get(), std::process::id()) else {
                return 0;
            };
            let name: Vec<u16> = pointer_gate_name(host).encode_utf16().chain([0]).collect();
            // SAFETY: NUL-terminated name that outlives the call; the handle lives for the process.
            unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
                .map(|handle| handle.0 as isize)
                .unwrap_or(0)
        });
        if bits == 0 {
            return Err(super::super::GateBusy);
        }
        let millis = u32::try_from(wait.as_millis()).unwrap_or(40);
        // SAFETY: waits on the process-lifetime mutex handle. WAIT_ABANDONED grants this thread
        // ownership after the previous owner died; it is not a new proof and not a failure.
        let status = unsafe { WaitForSingleObject(HANDLE(bits as *mut _), millis) };
        if status == WAIT_OBJECT_0 || status == WAIT_ABANDONED {
            Ok(Held {
                bits,
                _thread: PhantomData,
            })
        } else {
            Err(super::super::GateBusy)
        }
    }
}

#[cfg(all(windows, not(test)))]
use win32::acquire;

/// Same-thread guard for the production named mutex.
///
/// Drop releases it on the acquiring thread. Forgetting the guard and ending that
/// thread abandons the mutex; the next wait is granted ownership (`WAIT_ABANDONED`).
/// Not an input permission and not a lease. Integration tests use this; unit tests
/// do not pretend it is the kernel object.
#[cfg(all(windows, not(test)))]
pub struct PointerGateGuard {
    // Unread on purpose: dropping the field releases the mutex on this thread.
    #[allow(dead_code)]
    held: win32::Held,
    _thread: PhantomData<*const ()>,
}

#[cfg(all(windows, not(test)))]
pub(super) fn enter_for_test(wait: Duration) -> Result<PointerGateGuard, super::GateBusy> {
    acquire(wait).map(|held| PointerGateGuard {
        held,
        _thread: PhantomData,
    })
}

#[cfg(test)]
mod shared {
    use super::log_wait;
    use std::{
        marker::PhantomData,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Condvar, Mutex, MutexGuard,
        },
        thread::ThreadId,
        time::{Duration, Instant},
    };

    struct Owner {
        thread: ThreadId,
        depth: u32,
    }
    struct State {
        owner: Option<Owner>,
    }

    /// Reentrant on the owning thread, exclusive across threads. Poisoned state is taken so a
    /// panic in the critical section does not stick the gate shut. Thread abandonment of the
    /// Windows mutex is not modeled here.
    pub(crate) struct SharedGate {
        state: Mutex<State>,
        cond: Condvar,
        waiters: AtomicUsize,
    }
    impl SharedGate {
        pub(crate) fn new() -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new(State { owner: None }),
                cond: Condvar::new(),
                waiters: AtomicUsize::new(0),
            })
        }
        pub(crate) fn waiters(&self) -> usize {
            self.waiters.load(Ordering::SeqCst)
        }
        pub(crate) fn held_by_current(&self) -> bool {
            let state = self.lock();
            state.owner.as_ref().is_some_and(|owner| {
                owner.thread == std::thread::current().id() && owner.depth > 0
            })
        }
        pub(crate) fn acquire(self: &Arc<Self>, wait: Duration) -> Result<SharedHeld, super::super::GateBusy> {
            let mut state = self.lock();
            let me = std::thread::current().id();
            if let Some(owner) = state.owner.as_mut() {
                if owner.thread == me {
                    owner.depth = owner.depth.saturating_add(1);
                    return Ok(SharedHeld {
                        gate: Arc::clone(self),
                        _thread: PhantomData,
                    });
                }
            } else {
                state.owner = Some(Owner { thread: me, depth: 1 });
                return Ok(SharedHeld {
                    gate: Arc::clone(self),
                    _thread: PhantomData,
                });
            }
            if wait.is_zero() {
                return Err(super::super::GateBusy);
            }
            self.waiters.fetch_add(1, Ordering::SeqCst);
            let deadline = Instant::now() + wait;
            let result = loop {
                let now = Instant::now();
                if now >= deadline {
                    break Err(super::super::GateBusy);
                }
                let waited = self.cond.wait_timeout(state, deadline - now);
                let (guard, timeout) = match waited {
                    Ok(pair) => pair,
                    Err(poison) => poison.into_inner(),
                };
                state = guard;
                if state.owner.is_none() {
                    state.owner = Some(Owner { thread: me, depth: 1 });
                    break Ok(());
                }
                if timeout.timed_out() && Instant::now() >= deadline {
                    break Err(super::super::GateBusy);
                }
            };
            self.waiters.fetch_sub(1, Ordering::SeqCst);
            result.map(|()| SharedHeld {
                gate: Arc::clone(self),
                _thread: PhantomData,
            })
        }
        fn lock(&self) -> MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(|poison| poison.into_inner())
        }
        fn release_current(&self) {
            let mut state = self.lock();
            let me = std::thread::current().id();
            let Some(owner) = state.owner.as_mut() else {
                return;
            };
            if owner.thread != me || owner.depth == 0 {
                return;
            }
            owner.depth -= 1;
            if owner.depth == 0 {
                state.owner = None;
                self.cond.notify_one();
            }
        }
    }

    pub(crate) struct SharedHeld {
        gate: Arc<SharedGate>,
        _thread: PhantomData<*const ()>,
    }
    impl Drop for SharedHeld {
        fn drop(&mut self) {
            self.gate.release_current();
        }
    }

    pub(crate) fn with_admitted_gate_on<E, R>(
        gate: &Arc<SharedGate>,
        wait: Duration,
        admit: impl FnOnce() -> Result<(), E>,
        body: impl FnOnce() -> Result<R, E>,
    ) -> Result<Result<R, E>, super::super::GateBusy> {
        let started = Instant::now();
        let held = gate.acquire(wait)?;
        log_wait(started);
        let outcome = admit().and_then(|()| body());
        drop(held);
        Ok(outcome)
    }
}

#[cfg(test)]
pub(crate) use shared::{with_admitted_gate_on, SharedGate};
#[cfg(all(windows, test))]
use shared::SharedHeld;

#[cfg(test)]
thread_local! {
    static GATE: std::cell::RefCell<std::sync::Arc<SharedGate>> =
        std::cell::RefCell::new(SharedGate::new());
}

#[cfg(test)]
pub(super) fn current_gate() -> std::sync::Arc<SharedGate> {
    GATE.with(|gate| std::sync::Arc::clone(&gate.borrow()))
}

#[cfg(test)]
pub(crate) fn with_shared_gate<R>(gate: std::sync::Arc<SharedGate>, body: impl FnOnce() -> R) -> R {
    GATE.with(|slot| {
        let previous = slot.replace(gate);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
        slot.replace(previous);
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
}

/// Test build of the gate guard. The Windows integration test links the non-test
/// library and therefore uses the named mutex, not this value.
#[cfg(all(windows, test))]
pub struct PointerGateGuard {
    // Unread on purpose: dropping the field releases the stand-in gate.
    #[allow(dead_code)]
    held: SharedHeld,
    _thread: PhantomData<*const ()>,
}

#[cfg(all(windows, test))]
pub(super) fn enter_for_test(wait: Duration) -> Result<PointerGateGuard, super::GateBusy> {
    current_gate()
        .acquire(wait)
        .map(|held| PointerGateGuard {
            held,
            _thread: PhantomData,
        })
}

#[cfg(test)]
mod tests {
    use super::{
        gate_pid, inherited_host_pid, install_worker_pid, owner_from_binding, pointer_gate_name,
        Bound, GateRole, WorkerHostPidError,
    };
    use std::sync::OnceLock;

    #[test]
    fn host_role_ignores_inherited_gcu_host_pid() {
        for inherited in [None, Some(""), Some("0"), Some("999"), Some("nope")] {
            let read = inherited_host_pid(|_| inherited.map(str::to_string));
            assert_eq!(gate_pid(GateRole::Host, 42, read.as_deref()), Ok(42));
        }
        assert_eq!(pointer_gate_name(42), "Local\\gcu-pointer-gate-42");
    }

    #[test]
    fn worker_role_rejects_a_missing_or_invalid_host_pid() {
        assert!(WorkerHostPidError::Missing.message().contains("missing"));
        assert!(WorkerHostPidError::Invalid.message().contains("invalid"));
        assert_eq!(
            gate_pid(GateRole::Worker, 7, None),
            Err(WorkerHostPidError::Missing)
        );
        assert_eq!(
            gate_pid(GateRole::Worker, 7, Some("")),
            Err(WorkerHostPidError::Missing)
        );
        assert_eq!(
            gate_pid(GateRole::Worker, 7, Some("0")),
            Err(WorkerHostPidError::Invalid)
        );
        assert_eq!(
            gate_pid(GateRole::Worker, 7, Some("nope")),
            Err(WorkerHostPidError::Invalid)
        );
        assert_eq!(
            gate_pid(GateRole::Worker, 7, Some("-1")),
            Err(WorkerHostPidError::Invalid)
        );
    }

    #[test]
    fn worker_role_uses_the_validated_host_pid() {
        let value = inherited_host_pid(|key| {
            assert_eq!(key, "GCU_HOST_PID");
            Some("101".into())
        });
        assert_eq!(gate_pid(GateRole::Worker, 7, value.as_deref()), Ok(101));
        assert_eq!(pointer_gate_name(101), "Local\\gcu-pointer-gate-101");
    }

    #[test]
    fn worker_bind_is_idempotent_and_a_bad_value_refuses_the_slot() {
        let slot = OnceLock::new();
        assert_eq!(install_worker_pid(&slot, Some("42")), Ok(42));
        assert_eq!(install_worker_pid(&slot, Some("42")), Ok(42));
        assert_eq!(
            install_worker_pid(&slot, Some("7")),
            Err(WorkerHostPidError::Invalid)
        );
        assert!(matches!(slot.get(), Some(Bound::Worker(42))));
        let refused = OnceLock::new();
        assert_eq!(
            install_worker_pid(&refused, None),
            Err(WorkerHostPidError::Missing)
        );
        assert!(matches!(refused.get(), Some(Bound::Refused)));
        assert_eq!(
            install_worker_pid(&refused, Some("42")),
            Err(WorkerHostPidError::Invalid)
        );
        assert_eq!(owner_from_binding(None, 9), Ok(9));
        assert_eq!(owner_from_binding(Some(&Bound::Worker(4)), 9), Ok(4));
        assert!(owner_from_binding(Some(&Bound::Refused), 9).is_err());
    }
}
