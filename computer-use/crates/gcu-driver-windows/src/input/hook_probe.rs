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
    dispatch_inner(api, mode, idle, false)
}

pub(crate) fn dispatch_for_action(
    api: &mut impl MouseProbeApi,
    mode: MouseProbe,
    idle: &ProbeIdle,
) -> Result<Dispatch, String> {
    dispatch_inner(api, mode, idle, true)
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
        }
    }
    /// A refusal invalidates this cycle, never the listener's next action.
    pub(crate) fn begin_cycle(&mut self, action: bool) {
        self.action = action;
        self.index = 0;
        self.active = None;
        self.proven = None;
        self.proven_tag = None;
        self.failed = false;
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
        if let Some(active) = &self.active {
            if !active.complete {
                return None;
            }
            if active.callback.is_some() {
                self.preferred = Some(active.request.mode);
                self.index = 0;
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
        let mode = self
            .override_mode
            .or(self.preferred)
            .or_else(|| defaults.get(self.index).copied())?;
        self.index += 1;
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
