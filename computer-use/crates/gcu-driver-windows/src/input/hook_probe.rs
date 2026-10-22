//! Injectable mouse proof policy. Idle/cursor queries never confer a health lease.
use std::{
    ffi::OsStr,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Instant,
};

pub(crate) const ABSOLUTE_PROBE_IDLE_MS: u64 = 2000;
pub(crate) const PROBE_TAG: usize = 0x47435532;
pub(crate) type Dispatch = (u32, u32, u128);

/// Longest a thread waits for the injection gate. Far below the 100ms lease: a stuck holder
/// refuses the waiting action or probe cycle (fail closed) instead of starving the heartbeat.
pub(crate) const INJECTION_GATE_WAIT: std::time::Duration = std::time::Duration::from_millis(40);
/// The gate was not free within the wait. Nothing was sent.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct GateBusy;
/// Serializes an action's pointer `SendInput` with a probe's cursor sample, absolute publish and
/// unchanged-cursor check. Real hardware (2026-10-21, independent WH_MOUSE_LL log): the probe
/// sampled the old cursor, the action moved, then the probe published the old position and a
/// coordinate-less button landed there. The probe runs in the host process and the action's
/// `SendInput` in the isolated worker, so the production gate is a named kernel mutex shared by
/// both (`GCU_HOST_PID` names the host). Not reentrant across threads, not a lease, never held
/// across an await, and the low-level hooks (listener thread) never take it.
#[cfg(all(windows, not(test)))]
mod gate {
    use std::{sync::OnceLock, time::Duration};
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
            System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        },
    };
    /// `0` means no mutex is held (debug-only experiment switch).
    pub(super) struct Held(isize);
    impl Drop for Held {
        fn drop(&mut self) {
            if self.0 != 0 {
                // SAFETY: releases the mutex this thread acquired in `acquire`.
                unsafe {
                    let _ = ReleaseMutex(HANDLE(self.0 as *mut _));
                }
            }
        }
    }
    pub(super) fn acquire(wait: Duration) -> Result<Held, super::GateBusy> {
        // Debug/test workers only: lets the real-hardware stress test compare gate on and off.
        #[cfg(all(feature = "test-worker-faults", debug_assertions))]
        if std::env::var_os("GCU_POINTER_GATE_OFF").is_some() {
            return Ok(Held(0));
        }
        static HANDLE_BITS: OnceLock<isize> = OnceLock::new();
        let bits = *HANDLE_BITS.get_or_init(|| {
            let host = std::env::var("GCU_HOST_PID")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or_else(std::process::id);
            let name: Vec<u16> = format!("Local\\gcu-pointer-gate-{host}")
                .encode_utf16()
                .chain([0])
                .collect();
            // SAFETY: NUL-terminated name that outlives the call; the handle lives for the process.
            unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
                .map(|h| h.0 as isize)
                .unwrap_or(0)
        });
        if bits == 0 {
            return Err(super::GateBusy);
        }
        let millis = u32::try_from(wait.as_millis()).unwrap_or(40);
        // SAFETY: waits on the process-lifetime mutex handle.
        let status = unsafe { WaitForSingleObject(HANDLE(bits as *mut _), millis) };
        if status == WAIT_OBJECT_0 || status == WAIT_ABANDONED {
            Ok(Held(bits))
        } else {
            Err(super::GateBusy)
        }
    }
}
#[cfg(not(all(windows, not(test))))]
mod gate {
    use std::{
        sync::{Mutex, MutexGuard, TryLockError},
        time::{Duration, Instant},
    };
    // Unit tests run in parallel threads: each gets its own gate so tests cannot starve one
    // another. A holder and a waiter on the same thread still contend (not reentrant).
    #[cfg(test)]
    thread_local! {
        static GATE: &'static Mutex<()> = Box::leak(Box::new(Mutex::new(())));
    }
    #[cfg(not(test))]
    static GATE: Mutex<()> = Mutex::new(());
    pub(super) struct Held(MutexGuard<'static, ()>);
    pub(super) fn acquire(wait: Duration) -> Result<Held, super::GateBusy> {
        #[cfg(test)]
        let gate: &'static Mutex<()> = GATE.with(|gate| *gate);
        #[cfg(not(test))]
        let gate: &'static Mutex<()> = &GATE;
        let deadline = Instant::now() + wait;
        loop {
            match gate.try_lock() {
                Ok(guard) => return Ok(Held(guard)),
                Err(TryLockError::Poisoned(poison)) => return Ok(Held(poison.into_inner())),
                Err(TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(super::GateBusy);
                    }
                    std::thread::sleep(Duration::from_micros(300));
                }
            }
        }
    }
}
#[cfg(test)]
thread_local! {
    static INJECTION_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn gate_held() -> bool {
    INJECTION_DEPTH.with(|depth| depth.get() > 0)
}
pub(crate) fn with_injection_gate_for<R>(
    wait: std::time::Duration,
    body: impl FnOnce() -> R,
) -> Result<R, GateBusy> {
    let started = Instant::now();
    let _held = gate::acquire(wait)?;
    // Metadata only. A long wait is the first thing to rule out when a lease lapses near a gate.
    let waited = started.elapsed();
    if waited > std::time::Duration::from_millis(3) {
        tracing::info!(target: "gcu_windows_timing", stage = "pointer_gate_wait", wait_ms = waited.as_millis() as u64);
    }
    #[cfg(test)]
    struct Reset;
    #[cfg(test)]
    impl Drop for Reset {
        fn drop(&mut self) {
            INJECTION_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        }
    }
    #[cfg(test)]
    INJECTION_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
    #[cfg(test)]
    let _reset = Reset;
    Ok(body())
}
pub(crate) fn with_injection_gate<R>(body: impl FnOnce() -> R) -> Result<R, GateBusy> {
    with_injection_gate_for(INJECTION_GATE_WAIT, body)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MouseProbe {
    ZeroMove,
    ZeroWheel,
    ZeroHWheel,
    AbsoluteSamePosition,
    #[cfg(any(test, all(feature = "test-worker-faults", debug_assertions)))]
    RelativePair,
    #[cfg(any(test, all(feature = "test-worker-faults", debug_assertions)))]
    XButtonPair,
}

impl MouseProbe {
    pub(crate) fn requested(self) -> u32 {
        #[cfg(any(test, all(feature = "test-worker-faults", debug_assertions)))]
        if matches!(self, Self::RelativePair | Self::XButtonPair) {
            return 2;
        }
        1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cursor {
    pub x: i32,
    pub y: i32,
}
pub(crate) struct Desktop {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}
#[derive(Clone, Copy)]
pub(crate) struct RecentInput {
    pub now: u64,
    pub last: u32,
}
impl RecentInput {
    fn idle(self) -> bool {
        // LASTINPUTINFO is 32-bit and its ticks need not increase monotonically.
        // Never guess an epoch after rollover or subtract with wrapping arithmetic.
        self.now <= u64::from(u32::MAX)
            && self
                .now
                .checked_sub(u64::from(self.last))
                .is_some_and(|age| age >= ABSOLUTE_PROBE_IDLE_MS)
    }
}

/// Permission for the fourth send only, independent of all Stop/physical clocks.
/// Sample once before any probe input. Own injections cannot refresh or revoke
/// it; physical input revokes only the unsolicited initial attempt.
#[derive(Default)]
pub(crate) struct ProbeIdle(AtomicU8);
impl ProbeIdle {
    const UNCHECKED: u8 = 0;
    const ALLOWED: u8 = 1;
    const DENIED: u8 = 2;

    fn sample(&self, api: &mut impl MouseProbeApi) {
        if self.0.load(Ordering::Acquire) == Self::UNCHECKED {
            let state = if api.recent_input().is_ok_and(RecentInput::idle) {
                Self::ALLOWED
            } else {
                Self::DENIED
            };
            // A physical callback during the native read wins over this sample.
            let _ = self.0.compare_exchange(
                Self::UNCHECKED,
                state,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
    pub(crate) fn observe(&self, injected: bool, ours: bool) {
        if !injected && !ours {
            self.0.store(Self::DENIED, Ordering::Release);
        }
    }
    pub(crate) fn allowed(&self) -> bool {
        self.0.load(Ordering::Acquire) == Self::ALLOWED
    }
}

pub(crate) trait MouseProbeApi {
    // Must verify the interactive desktop; unknown input time is an error.
    fn recent_input(&mut self) -> Result<RecentInput, String>;
    fn cursor(&mut self) -> Result<Cursor, String>;
    fn desktop(&mut self) -> Result<Desktop, String>;
    fn send(&mut self, mode: MouseProbe, absolute: Option<(i32, i32)>) -> Result<Dispatch, String>;
}

pub(crate) fn normalize(position: i32, origin: i32, size: i32) -> Result<i32, String> {
    let offset = i64::from(position) - i64::from(origin);
    if !(1..=65536).contains(&size) || offset < 0 || offset >= i64::from(size) {
        return Err("cursor outside representable physical virtual desktop".into());
    }
    // Center of the integer bin that maps back to this physical pixel. Ceil the
    // lower bound so even a 65535/65536-pixel desktop never rounds a pixel left.
    let offset = offset as u64;
    let size = size as u64;
    let low = (offset * 65536).div_ceil(size);
    let high = ((offset + 1) * 65536 - 1) / size;
    Ok(((low + high) / 2) as i32)
}

pub(crate) fn dispatch(
    api: &mut impl MouseProbeApi,
    mode: MouseProbe,
    idle: &ProbeIdle,
) -> Result<Dispatch, String> {
    with_injection_gate(|| dispatch_inner(api, mode, idle, false))
        .unwrap_or_else(|GateBusy| Err("probe not sent: pointer input in flight".into()))
}

pub(crate) fn dispatch_for_action(
    api: &mut impl MouseProbeApi,
    mode: MouseProbe,
    idle: &ProbeIdle,
) -> Result<Dispatch, String> {
    // Cursor sample, absolute publish and the unchanged-cursor check stay one section with the
    // action's pointer SendInput, so a stale same-position move cannot land between a move and
    // a coordinate-less button.
    with_injection_gate(|| dispatch_inner(api, mode, idle, true))
        .unwrap_or_else(|GateBusy| Err("probe not sent: pointer input in flight".into()))
}
fn dispatch_inner(
    api: &mut impl MouseProbeApi,
    mode: MouseProbe,
    idle: &ProbeIdle,
    action: bool,
) -> Result<Dispatch, String> {
    if !action {
        idle.sample(api);
    }
    if mode != MouseProbe::AbsoluteSamePosition {
        return api.send(mode, None);
    }
    if !action && !idle.allowed() {
        return Err("absolute mouse probe not sent: initial input idle unknown/below 2000ms, tick wrap, or physical input during probes".into());
    }
    let before = api.cursor()?;
    let desktop = api.desktop()?;
    let absolute = (
        normalize(before.x, desktop.left, desktop.width)?,
        normalize(before.y, desktop.top, desktop.height)?,
    );
    if !action && !idle.allowed() {
        return Err("absolute mouse probe not sent: physical input during cursor queries".into());
    }
    let result = api.send(mode, Some(absolute))?;
    // A racing user move or rounding invalidates this proof. Never repair/retry.
    if api.cursor()? != before {
        return Err("cursor changed during probe".into());
    }
    if !action && !idle.allowed() {
        return Err("physical input during absolute mouse probe".into());
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProbeRequest {
    pub mode: MouseProbe,
    pub tag: usize,
}
struct Attempt {
    request: ProbeRequest,
    complete: bool,
    callback: Option<Instant>,
}
pub(crate) struct MouseProof {
    idle: Arc<ProbeIdle>,
    override_mode: Option<MouseProbe>,
    preferred: Option<MouseProbe>,
    index: usize,
    serial: usize,
    active: Option<Attempt>,
    proven: Option<Instant>,
    proven_tag: Option<usize>,
    failed: bool,
    action: bool,
    /// Last tagged callback seen (tag, flags), for diagnostics only.
    last_seen: Option<(usize, u32)>,
    /// The one in-lease retry of a missing absolute callback has been used since the last proof.
    retry_used: bool,
}
impl MouseProof {
    pub(crate) fn new(override_mode: Option<MouseProbe>) -> Self {
        Self {
            idle: Arc::default(),
            override_mode,
            preferred: None,
            index: 0,
            serial: PROBE_TAG,
            active: None,
            proven: None,
            proven_tag: None,
            failed: false,
            action: false,
            last_seen: None,
            retry_used: false,
        }
    }
    /// Metadata only: why `next` may offer nothing.
    pub(crate) fn debug_state(&self) -> String {
        format!(
            "failed={}; active_tag={:?}; complete={:?}; callback={:?}; mode={:?}; last_seen={:?}",
            self.failed,
            self.active.as_ref().map(|a| a.request.tag),
            self.active.as_ref().map(|a| a.complete),
            self.active.as_ref().map(|a| a.callback.is_some()),
            self.active.as_ref().map(|a| a.request.mode),
            self.last_seen
        )
    }
    /// A refusal invalidates this cycle, never the listener's next action.
    pub(crate) fn begin_cycle(&mut self, action: bool) {
        self.action = action;
        self.index = 0;
        self.active = None;
        self.proven = None;
        self.proven_tag = None;
        self.failed = false;
        self.retry_used = false;
    }
    pub(crate) fn tagged(&self, tag: usize) -> bool {
        (PROBE_TAG..=self.serial).contains(&tag)
    }
    pub(crate) fn current_tag(&self) -> Option<usize> {
        self.active.as_ref().map(|a| a.request.tag)
    }
    pub(crate) fn idle(&self) -> &Arc<ProbeIdle> {
        &self.idle
    }
    pub(crate) fn next(&mut self) -> Option<ProbeRequest> {
        if self.failed {
            return None;
        }
        let mut retry = None;
        if let Some(active) = &self.active {
            if !active.complete {
                return None;
            }
            if active.callback.is_some() {
                self.preferred = Some(active.request.mode);
                self.index = 0;
                self.retry_used = false;
            } else if self.action
                && active.request.mode == MouseProbe::AbsoluteSamePosition
                && !self.retry_used
            {
                // One bounded retry of the same kind, only inside an action window. The attempt
                // reached SendInput but its tagged callback never reported (about 2% of repeated
                // drags on real hardware). The retry proves nothing by itself: it still needs a
                // fresh tagged injected callback from both hooks, and the 100ms lease is unchanged.
                self.retry_used = true;
                retry = Some(active.request.mode);
            } else if self.preferred.is_some()
                || self.override_mode.is_some()
                || active.request.mode == MouseProbe::AbsoluteSamePosition
            {
                // One attempt with the known working kind per proof cycle.
                // Missing delivery waits for failure/deadline, never retries.
                return None;
            }
        }
        let defaults = [
            MouseProbe::ZeroMove,
            MouseProbe::ZeroWheel,
            MouseProbe::ZeroHWheel,
            MouseProbe::AbsoluteSamePosition,
        ];
        let mode = retry
            .or(self.override_mode)
            .or(self.preferred)
            .or_else(|| defaults.get(self.index).copied())?;
        if retry.is_none() {
            self.index += 1;
        }
        self.serial = self.serial.checked_add(1)?;
        let request = ProbeRequest {
            mode,
            tag: self.serial,
        };
        self.active = Some(Attempt {
            request,
            complete: false,
            callback: None,
        });
        Some(request)
    }
    pub(crate) fn callback(&mut self, tag: usize, flags: u32, at: Instant) -> bool {
        let tagged = self.tagged(tag);
        if tagged {
            self.last_seen = Some((tag, flags));
        }
        self.idle.observe(flags & 3 != 0, tagged);
        let Some(active) = self.active.as_mut().filter(|a| a.request.tag == tag) else {
            return tagged;
        };
        if !self.action
            && active.request.mode == MouseProbe::AbsoluteSamePosition
            && !self.idle.allowed()
        {
            self.failed = true;
            self.proven = None;
            self.proven_tag = None;
        }
        if flags & 3 != 0 && !self.failed {
            let tag = active.request.tag;
            active.callback = Some(at);
            if active.complete {
                self.preferred = Some(active.request.mode);
                self.proven = Some(at);
                self.proven_tag = Some(tag);
            }
        }
        true // Tagged callbacks with stripped flags are never physical activity.
    }
    pub(crate) fn complete(&mut self, result: Result<(), String>) -> Result<(), String> {
        let result = result.and_then(|()| {
            if !self.action
                && self
                    .active
                    .as_ref()
                    .is_some_and(|a| a.request.mode == MouseProbe::AbsoluteSamePosition)
                && !self.idle.allowed()
            {
                Err("absolute mouse probe invalidated by physical input or unknown idle".into())
            } else {
                Ok(())
            }
        });
        if result.is_err() {
            self.failed = true;
            self.proven = None;
            self.proven_tag = None;
        }
        result?;
        if let Some(active) = &mut self.active {
            active.complete = true;
            if let Some(at) = active.callback {
                let tag = active.request.tag;
                self.preferred = Some(active.request.mode);
                self.proven = Some(at);
                self.proven_tag = Some(tag);
            }
        }
        Ok(())
    }
    pub(crate) fn proven(&self) -> Option<Instant> {
        self.proven
    }
    pub(crate) fn proven_tag(&self) -> Option<usize> {
        self.proven_tag
    }
}

/// Keyboard stamp that belongs to the same attempt as `mouse_tag`.
/// A newer key-up paired with the previous mouse `proven` is not a lease and
/// not hook removal. The caller leaves that keyboard out of `confirm`.
pub(crate) fn keyboard_stamp_for_mouse(
    mouse_tag: Option<usize>,
    keyboard: Option<(usize, Instant)>,
) -> Option<Instant> {
    let (tag, at) = keyboard?;
    (mouse_tag == Some(tag)).then_some(at)
}

pub(crate) fn configured_mode(mode: Option<&OsStr>) -> Result<Option<MouseProbe>, String> {
    let Some(mode) = mode else {
        return Ok(None);
    };
    #[cfg(all(feature = "test-worker-faults", debug_assertions))]
    {
        match mode.to_str() {
            Some("zero_move") => Ok(Some(MouseProbe::ZeroMove)),
            Some("zero_wheel") => Ok(Some(MouseProbe::ZeroWheel)),
            Some("zero_hwheel") => Ok(Some(MouseProbe::ZeroHWheel)),
            Some("absolute_same_position") => Ok(Some(MouseProbe::AbsoluteSamePosition)),
            Some("relative_pair") => Ok(Some(MouseProbe::RelativePair)),
            Some("xbutton_pair") => Ok(Some(MouseProbe::XButtonPair)),
            _ => Err("unsupported debug hook mouse probe; foreground disabled".into()),
        }
    }
    #[cfg(not(all(feature = "test-worker-faults", debug_assertions)))]
    {
        let _ = mode;
        Err("GCU_HOOK_MOUSE_PROBE requires test-worker-faults and debug assertions; foreground disabled".into())
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::keyboard_stamp_for_mouse;
    use std::time::Instant;

    #[test]
    fn cross_attempt_keyboard_is_not_part_of_the_mouse_pair() {
        let at = Instant::now();
        assert_eq!(keyboard_stamp_for_mouse(Some(1), Some((2, at))), None);
        assert_eq!(keyboard_stamp_for_mouse(None, Some((2, at))), None);
        assert_eq!(keyboard_stamp_for_mouse(Some(2), Some((2, at))), Some(at));
    }
}
