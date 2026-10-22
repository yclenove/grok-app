// Pointer-gate acquire order.
// Included from `hook_probe.rs` so the `#[path]` policy tests compile the same items.
// The std gate below is the test double: same-thread reentry matches a Win32 mutex, and each
// test can share one instance across threads. It is not the kernel object. The named mutex is
// `cfg(all(windows, not(test)))` and is what `tests/pointer_gate.rs` calls.
// One desktop has one cursor, so every host and worker in the session opens this one name.
use std::time::Duration;

/// Session-wide pointer gate. Not per process: two hosts must not move one cursor at once.
pub(super) const POINTER_GATE_NAME: &str = "Local\\gcu-pointer-gate-v1";

/// Stashed outside the mutex. `wait` is only `WaitForSingleObject` (or the test double's acquire),
/// not admission, insertion, or settle. A real code of 0 is stored as `u32::MAX` so it cannot
/// look like success.
#[derive(Clone, Copy)]
struct DiagNote {
    wait: Option<Duration>,
    release_hresult: Option<u32>,
    wait_failed: Option<u32>,
    create_hresult: Option<u32>,
}
impl DiagNote {
    const EMPTY: Self = Self {
        wait: None,
        release_hresult: None,
        wait_failed: None,
        create_hresult: None,
    };
}
thread_local! {
    static DIAG: std::cell::Cell<DiagNote> = const { std::cell::Cell::new(DiagNote::EMPTY) };
}
fn with_note(update: impl FnOnce(&mut DiagNote)) {
    DIAG.with(|cell| {
        let mut note = cell.get();
        update(&mut note);
        cell.set(note);
    });
}
fn note_wait(waited: Duration) {
    with_note(|note| {
        note.wait = Some(match note.wait {
            Some(previous) if previous >= waited => previous,
            _ => waited,
        });
    });
}
fn note_release(hresult: u32) {
    with_note(|note| {
        if note.release_hresult.is_none() {
            note.release_hresult = Some(hresult);
        }
    });
}
fn note_wait_failed(code: u32) {
    with_note(|note| {
        if note.wait_failed.is_none() {
            note.wait_failed = Some(code);
        }
    });
}
fn note_create(hresult: u32) {
    with_note(|note| {
        if note.create_hresult.is_none() {
            note.create_hresult = Some(hresult);
        }
    });
}
#[cfg(all(windows, not(test), feature = "test-support"))]
fn clear_wait_note() {
    with_note(|note| note.wait = None);
}
/// `FACILITY_WIN32` (`0x8007xxxx`) down to its 16-bit Win32 code.
fn win32_from_hresult(raw: u32) -> Option<u32> {
    if raw & 0xFFFF_0000 == 0x8007_0000 {
        let win32 = raw & 0xFFFF;
        (win32 != 0).then_some(win32)
    } else {
        None
    }
}
pub(super) fn format_stored_error(raw: u32) -> String {
    match win32_from_hresult(raw) {
        Some(win32) => format!("HRESULT {raw:08X}, Win32 {win32:08X}"),
        None => format!("HRESULT {raw:08X}"),
    }
}
/// Emit stashed diagnostics. The caller must not own the pointer mutex or the input ledger.
pub(super) fn flush_pointer_gate_diag() {
    let note = DIAG.with(|cell| {
        let note = cell.get();
        cell.set(DiagNote::EMPTY);
        note
    });
    if let Some(raw) = note.create_hresult {
        tracing::warn!(
            target: "gcu_windows_timing",
            stage = "pointer_gate_create",
            hresult = raw,
            win32 = win32_from_hresult(raw).unwrap_or(0),
            "pointer gate mutex could not be created"
        );
    }
    if let Some(waited) = note.wait {
        if waited > Duration::from_millis(3) {
            tracing::info!(
                target: "gcu_windows_timing",
                stage = "pointer_gate_wait",
                wait_ms = waited.as_millis() as u64,
            );
        }
    }
    if let Some(raw) = note.release_hresult {
        tracing::warn!(
            target: "gcu_windows_timing",
            stage = "pointer_gate_release",
            hresult = raw,
            win32 = win32_from_hresult(raw).unwrap_or(0),
            "ReleaseMutex failed"
        );
    }
    if let Some(code) = note.wait_failed {
        tracing::warn!(
            target: "gcu_windows_timing",
            stage = "pointer_gate_wait_failed",
            win32 = code,
            "WaitForSingleObject failed"
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
        let held = acquire(wait)?;
        let outcome = admit().and_then(|()| body());
        drop(held);
        // Acquire time is stashed inside `acquire`. Emit it after the ledger unlocks.
        Ok(outcome)
    }
}

#[cfg(all(windows, not(test)))]
mod win32 {
    use super::POINTER_GATE_NAME;
    use std::{
        marker::PhantomData,
        sync::{
            atomic::{AtomicBool, Ordering},
            OnceLock,
        },
        time::Duration,
    };
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{GetLastError, HANDLE, WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0},
            System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        },
    };

    struct Created {
        bits: isize,
        /// HRESULT when `bits` is 0. Never 0: a zero code would look like a silent success.
        error: u32,
    }

    pub(super) struct Held {
        bits: isize,
        _thread: PhantomData<*const ()>,
    }
    impl Drop for Held {
        fn drop(&mut self) {
            if self.bits != 0 {
                // SAFETY: releases the mutex this thread acquired in `acquire`.
                // A failure is stashed and logged after this thread no longer owns the mutex.
                let released = unsafe { ReleaseMutex(HANDLE(self.bits as *mut _)) };
                if let Err(error) = released {
                    let code = error.code().0 as u32;
                    super::note_release(if code == 0 { u32::MAX } else { code });
                }
            }
        }
    }

    static CREATED: OnceLock<Created> = OnceLock::new();
    static LOGGED: AtomicBool = AtomicBool::new(false);
    #[cfg(feature = "test-support")]
    static NAME_OVERRIDE: OnceLock<&'static str> = OnceLock::new();

    fn gate_name() -> &'static str {
        #[cfg(feature = "test-support")]
        if let Some(name) = NAME_OVERRIDE.get().copied() {
            return name;
        }
        POINTER_GATE_NAME
    }

    fn name_utf16() -> Vec<u16> {
        gate_name().encode_utf16().chain([0]).collect()
    }

    /// Inject the mutex name before the first `CreateMutexW`. Production builds do not call this.
    #[cfg(feature = "test-support")]
    pub(super) fn set_name_for_test(name: &'static str) -> Result<(), &'static str> {
        if CREATED.get().is_some() {
            return Err("pointer gate name is already bound");
        }
        NAME_OVERRIDE
            .set(name)
            .map_err(|_| "pointer gate name override is already set")
    }

    fn created() -> &'static Created {
        CREATED.get_or_init(|| {
            let name = name_utf16();
            // SAFETY: NUL-terminated name that outlives the call; the handle lives for the process.
            match unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) } {
                Ok(handle) => Created {
                    bits: handle.0 as isize,
                    error: 0,
                },
                Err(error) => {
                    let code = error.code().0 as u32;
                    Created {
                        bits: 0,
                        error: if code == 0 { u32::MAX } else { code },
                    }
                }
            }
        })
    }

    pub(super) fn create_error_code() -> Option<u32> {
        CREATED
            .get()
            .and_then(|created| (created.bits == 0 && created.error != 0).then_some(created.error))
    }

    fn stash_create_failure_once(code: u32) {
        if LOGGED.swap(true, Ordering::Relaxed) {
            return;
        }
        // Still under the input ledger. The warn is emitted by `flush_pointer_gate_diag`.
        super::note_create(code);
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
        let created = created();
        if created.bits == 0 {
            stash_create_failure_once(created.error);
            return Err(super::super::GateBusy);
        }
        let bits = created.bits;
        let millis = u32::try_from(wait.as_millis()).unwrap_or(40);
        // SAFETY: waits on the process-lifetime mutex handle. WAIT_ABANDONED grants this thread
        // ownership after the previous owner died; it is not a new proof and not a failure.
        // GetLastError is read before any other call when the wait itself fails.
        let started = std::time::Instant::now();
        let status = unsafe { WaitForSingleObject(HANDLE(bits as *mut _), millis) };
        let failed = if status == WAIT_FAILED {
            let code = unsafe { GetLastError() }.0;
            Some(if code == 0 { u32::MAX } else { code })
        } else {
            None
        };
        super::note_wait(started.elapsed());
        if let Some(code) = failed {
            super::note_wait_failed(code);
            return Err(super::super::GateBusy);
        }
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
pub(super) fn create_error_code() -> Option<u32> {
    win32::create_error_code()
}

/// Busy message for a refused pointer packet. A failed `CreateMutexW` keeps its HRESULT
/// and, when the facility is Win32, the Win32 code as well.
pub(super) fn refusal_message() -> String {
    #[cfg(all(windows, not(test)))]
    if let Some(code) = create_error_code() {
        return format!(
            "pointer gate unavailable ({}); nothing was sent",
            format_stored_error(code)
        );
    }
    "pointer input gate busy; nothing was sent".into()
}

#[cfg(all(windows, not(test), feature = "test-support"))]
pub(super) fn set_name_for_test(name: &'static str) -> Result<(), &'static str> {
    win32::set_name_for_test(name)
}

#[cfg(all(windows, not(test), feature = "test-support"))]
pub(super) fn production_name() -> &'static str {
    POINTER_GATE_NAME
}

#[cfg(all(windows, not(test)))]
use win32::acquire;

#[cfg(all(windows, not(test), feature = "test-support"))]
use std::marker::PhantomData;

/// Same-thread guard for the production named mutex.
///
/// Drop releases it on the acquiring thread. Forgetting the guard and ending that
/// thread abandons the mutex; the next wait is granted ownership (`WAIT_ABANDONED`).
/// Not an input permission and not a lease. Integration tests use this; unit tests
/// do not pretend it is the kernel object. Absent from production builds that do not
/// enable `test-support`.
#[cfg(all(windows, not(test), feature = "test-support"))]
pub struct PointerGateGuard {
    // Unread on purpose: dropping the field releases the mutex on this thread.
    #[allow(dead_code)]
    held: win32::Held,
    _thread: PhantomData<*const ()>,
}

#[cfg(all(windows, not(test), feature = "test-support"))]
pub(super) fn enter_for_test(wait: Duration) -> Result<PointerGateGuard, String> {
    match acquire(wait) {
        Ok(held) => {
            // The guard is still held. Drop the acquire note so a later flush on this thread
            // cannot attribute it to a different critical section.
            clear_wait_note();
            Ok(PointerGateGuard {
                held,
                _thread: PhantomData,
            })
        }
        Err(_) => {
            let message = refusal_message();
            flush_pointer_gate_diag();
            Err(message)
        }
    }
}

#[cfg(test)]
mod shared {
    use super::note_wait;
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
            state
                .owner
                .as_ref()
                .is_some_and(|owner| owner.thread == std::thread::current().id() && owner.depth > 0)
        }
        pub(crate) fn acquire(
            self: &Arc<Self>,
            wait: Duration,
        ) -> Result<SharedHeld, super::super::GateBusy> {
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
                state.owner = Some(Owner {
                    thread: me,
                    depth: 1,
                });
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
                    state.owner = Some(Owner {
                        thread: me,
                        depth: 1,
                    });
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
            self.state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
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
        let acquired = gate.acquire(wait);
        // Acquire time only. The body and any later log stay outside this measurement.
        note_wait(started.elapsed());
        let held = acquired?;
        let outcome = admit().and_then(|()| body());
        drop(held);
        Ok(outcome)
    }
}

#[cfg(test)]
pub(crate) use shared::{with_admitted_gate_on, SharedGate};

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

#[cfg(test)]
mod tests {
    use super::{format_stored_error, refusal_message, POINTER_GATE_NAME};

    #[test]
    fn the_pointer_gate_name_is_one_per_session() {
        assert_eq!(POINTER_GATE_NAME, "Local\\gcu-pointer-gate-v1");
    }

    #[test]
    fn a_contended_gate_names_busy_without_a_win32_code() {
        assert_eq!(
            refusal_message(),
            "pointer input gate busy; nothing was sent"
        );
    }

    #[test]
    fn a_win32_hresult_is_not_labeled_as_only_a_win32_code() {
        assert_eq!(
            format_stored_error(0x8007_0006),
            "HRESULT 80070006, Win32 00000006"
        );
        assert_eq!(format_stored_error(0x8000_4005), "HRESULT 80004005");
    }
}
