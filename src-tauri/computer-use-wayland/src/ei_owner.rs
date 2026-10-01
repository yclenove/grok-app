//! All libei calls and object references live on one blocking owner thread.
//! Only the portal fd is accepted. No environment socket or Notify* fallback.
use crate::{ei_ffi as ffi, input::*, StreamGrant};
use std::{
    collections::BTreeSet,
    ffi::CStr,
    os::{
        fd::{IntoRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    ptr,
    sync::mpsc,
    time::{Duration, Instant},
};

#[cfg(feature = "owned-acceptance-diagnostics")]
#[path = "ei_diagnostics.rs"]
mod diagnostics;

struct Device {
    raw: *mut ffi::Device,
    resumed: bool,
    started: bool,
    emulation_sequence: u32,
    keys: BTreeSet<u32>,
    buttons: BTreeSet<u32>,
    scrolling: bool,
}
impl Device {
    fn has(&self, cap: u32) -> bool {
        self.resumed && unsafe { ffi::ei_device_has_capability(self.raw, cap) }
    }
    fn begin(&mut self) {
        if !self.started {
            self.emulation_sequence = self.emulation_sequence.wrapping_add(1);
            unsafe { ffi::ei_device_start_emulating(self.raw, self.emulation_sequence) };
            self.started = true;
        }
    }
    fn neutral(&mut self, now: u64) {
        if self.resumed && self.started {
            // Only release this owner's synthetic state, never physical keys.
            unsafe {
                for key in &self.keys {
                    ffi::ei_device_keyboard_key(self.raw, *key, false);
                }
                for button in &self.buttons {
                    ffi::ei_device_button_button(self.raw, *button, false);
                }
                if self.scrolling {
                    ffi::ei_device_scroll_cancel(self.raw, true, true);
                }
                ffi::ei_device_frame(self.raw, now);
                ffi::ei_device_stop_emulating(self.raw);
            }
        }
        self.started = false;
        self.keys.clear();
        self.buttons.clear();
        self.scrolling = false;
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            ffi::ei_device_unref(self.raw);
        }
    }
}
struct Native {
    raw: *mut ffi::Ei,
    devices: Vec<Device>,
    connected: bool,
    generation: u64,
    sequence: u64,
    mapping: Option<String>,
}
impl Drop for Native {
    fn drop(&mut self) {
        let now = unsafe { ffi::ei_now(self.raw) };
        for device in &mut self.devices {
            device.neutral(now);
        }
        self.devices.clear();
        unsafe {
            ffi::ei_unref(self.raw);
        }
    }
}
impl Native {
    fn new(fd: OwnedFd, stream: StreamGrant) -> Result<Self, String> {
        // libei's fd backend does not set O_NONBLOCK. A silent/partial peer must
        // not trap setup/dispatch and prevent the native owner from stopping.
        let socket = UnixStream::from(fd);
        socket
            .set_nonblocking(true)
            .map_err(|e| format!("could not make EIS transport nonblocking: {e}"))?;
        let raw = unsafe { ffi::ei_new_sender(ptr::null_mut()) };
        if raw.is_null() {
            return Err("libei context allocation failed".into());
        }
        let result = Self {
            raw,
            devices: Vec::new(),
            connected: false,
            generation: 0,
            sequence: 0,
            mapping: stream.mapping_id,
        };
        unsafe {
            ffi::ei_configure_name(raw, c"Grok Computer Use".as_ptr());
        }
        // libei takes ownership even on setup failure; Native::drop retires it.
        let status = unsafe { ffi::ei_setup_backend_fd(raw, socket.into_raw_fd()) };
        if status != 0 {
            return Err(format!("libei granted-fd setup failed: {status}"));
        }
        Ok(result)
    }
    fn regions(&self) -> Vec<(usize, InputRegion)> {
        let Some(mapping) = self.mapping.as_deref() else {
            return Vec::new();
        };
        let mut matches = Vec::new();
        for (index, device) in self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.has(ffi::ABSOLUTE))
        {
            if unsafe { ffi::ei_device_get_type(device.raw) } != 1 {
                continue;
            }
            // Bound enumeration. A server over this limit is ambiguous, not a
            // reason to pick the first matching monitor and emit global input.
            for i in 0..=128 {
                let region = unsafe { ffi::ei_device_get_region(device.raw, i) };
                if region.is_null() {
                    break;
                }
                if i == 128 {
                    return Vec::new();
                }
                let id = unsafe { ffi::ei_region_get_mapping_id(region) };
                if id.is_null() || unsafe { CStr::from_ptr(id) }.to_bytes() != mapping.as_bytes() {
                    continue;
                }
                let r = unsafe {
                    InputRegion {
                        mapping_id: mapping.into(),
                        x: ffi::ei_region_get_x(region),
                        y: ffi::ei_region_get_y(region),
                        width: ffi::ei_region_get_width(region),
                        height: ffi::ei_region_get_height(region),
                        physical_scale: ffi::ei_region_get_physical_scale(region),
                    }
                };
                if r.width == 0
                    || r.height == 0
                    || !r.physical_scale.is_finite()
                    || r.physical_scale <= 0.0
                {
                    return Vec::new();
                }
                matches.push((index, r));
            }
        }
        matches
    }
    fn unique_device(&self, cap: u32) -> Option<usize> {
        // Prefer the exactly paired absolute device for buttons/scroll. Mutter
        // may also expose a relative device with those same capabilities.
        if matches!(cap, ffi::BUTTON | ffi::SCROLL) {
            let regions = self.regions();
            if regions.len() == 1 && self.devices[regions[0].0].has(cap) {
                return Some(regions[0].0);
            }
        }
        let mut candidates = self.devices.iter().enumerate().filter(|(_, d)| {
            // This API accepts logical-pixel motion/scroll, never physical mm.
            // Do not silently route the same values to a physical EI device.
            d.has(cap) && unsafe { ffi::ei_device_get_type(d.raw) } == 1
        });
        let (index, _) = candidates.next()?;
        candidates.next().is_none().then_some(index)
    }
    fn capabilities(&self) -> InputCapabilities {
        let mut regions = self.regions();
        InputCapabilities {
            generation: self.generation,
            keyboard: self.unique_device(ffi::KEYBOARD).is_some(),
            relative_pointer: self.unique_device(ffi::POINTER).is_some(),
            buttons: self.unique_device(ffi::BUTTON).is_some(),
            scroll: self.unique_device(ffi::SCROLL).is_some(),
            absolute_region: if regions.len() == 1 {
                Some(regions.remove(0).1)
            } else {
                None
            },
        }
    }
    fn events(&mut self) -> Result<(), String> {
        unsafe {
            ffi::ei_dispatch(self.raw);
        }
        for _ in 0..256 {
            let raw = unsafe { ffi::ei_get_event(self.raw) };
            if raw.is_null() {
                return Ok(());
            }
            struct Event(*mut ffi::Event);
            impl Drop for Event {
                fn drop(&mut self) {
                    unsafe {
                        ffi::ei_event_unref(self.0);
                    }
                }
            }
            let event = Event(raw);
            let kind = unsafe { ffi::ei_event_get_type(event.0) };
            match kind {
                1 => self.connected = true,
                2 => return Err("EIS disconnected".into()),
                3 => {
                    let seat = unsafe { ffi::ei_event_get_seat(event.0) };
                    if seat.is_null() {
                        return Err("invalid EI seat event".into());
                    }
                    unsafe {
                        ffi::ei_seat_bind_capabilities(
                            seat,
                            ffi::POINTER,
                            ffi::ABSOLUTE,
                            ffi::KEYBOARD,
                            ffi::BUTTON,
                            ffi::SCROLL,
                            ptr::null::<std::ffi::c_void>(),
                        );
                    }
                }
                4 => return Err("EIS seat removed; new consent required".into()),
                5 => {
                    if self.devices.iter().any(|d| d.started) {
                        return Err("EI topology changed during emulation".into());
                    }
                    let raw = unsafe { ffi::ei_event_get_device(event.0) };
                    if raw.is_null()
                        || self.devices.len() >= 64
                        || self.devices.iter().any(|d| d.raw == raw)
                    {
                        return Err("invalid or excessive EI devices".into());
                    }
                    self.devices.push(Device {
                        raw: unsafe { ffi::ei_device_ref(raw) },
                        resumed: false,
                        started: false,
                        emulation_sequence: 0,
                        keys: BTreeSet::new(),
                        buttons: BTreeSet::new(),
                        scrolling: false,
                    });
                }
                6..=8 => {
                    let raw = unsafe { ffi::ei_event_get_device(event.0) };
                    let index = self
                        .devices
                        .iter()
                        .position(|d| d.raw == raw)
                        .ok_or("unknown EI device state event")?;
                    if kind != 8 {
                        self.devices[index].resumed = false;
                        if self.devices[index].started {
                            return Err(
                                "EIS revoked an emulating device; input result uncertain".into()
                            );
                        }
                    }
                    if kind == 6 {
                        self.devices.remove(index);
                    } else {
                        self.devices[index].resumed = kind == 8;
                    }
                }
                _ => {} // Unknown events still unref; no permission inferred.
            }
            if (1..=8).contains(&kind) {
                self.generation = self
                    .generation
                    .checked_add(1)
                    .ok_or("EI generation exhausted")?;
            }
        }
        Err("excessive EI event burst; authority revoked".into())
    }
    fn compound(
        &mut self,
        actions: Vec<InputAction>,
        cancellation: &grok_computer_use_core::execution::ActionCancellation,
    ) -> Result<(), String> {
        crate::sequence::validate_balanced(&actions)?;
        if self
            .devices
            .iter()
            .any(|d| !d.keys.is_empty() || !d.buttons.is_empty() || d.scrolling)
        {
            return Err("compound input requires neutral owned devices".into());
        }
        // Check ALL capabilities and wire coordinates before the first event.
        // The same owner/generation and frame locks cover preflight + submission.
        for action in &actions {
            self.destination(action)?;
        }
        crate::sequence::execute(actions, cancellation, |a| self.action(a))
    }
    fn destination(&self, action: &InputAction) -> Result<(usize, Option<(f64, f64)>), String> {
        let cap = match action {
            InputAction::Key { .. } => ffi::KEYBOARD,
            InputAction::Button { .. } => ffi::BUTTON,
            InputAction::Relative { .. } => ffi::POINTER,
            InputAction::Absolute { .. } => ffi::ABSOLUTE,
            _ => ffi::SCROLL,
        };
        if let InputAction::Absolute { x, y } = *action {
            let mut matches = self.regions();
            if matches.len() != 1 {
                return Err("missing or ambiguous exact EI mapping".into());
            }
            let (i, r) = matches.remove(0);
            Ok((i, Some(r.wire_position(x, y)?)))
        } else {
            Ok((
                self.unique_device(cap)
                    .ok_or("EI capability missing, paused or ambiguous")?,
                None,
            ))
        }
    }
    fn action(&mut self, action: InputAction) -> Result<(), String> {
        action.validate()?;
        let now = unsafe { ffi::ei_now(self.raw) };
        if matches!(action, InputAction::ReleaseAll) {
            for device in &mut self.devices {
                device.neutral(now);
            }
            return Ok(());
        }
        let (index, position) = self.destination(&action)?;
        let device = &mut self.devices[index];
        // Reject duplicate presses/unmatched releases, not libei silent no-ops.
        if let InputAction::Key { code, pressed } | InputAction::Button { code, pressed } = action {
            let set = if matches!(action, InputAction::Key { .. }) {
                &device.keys
            } else {
                &device.buttons
            };
            if set.contains(&code) == pressed {
                return Err("EI key/button state transition is not owned".into());
            }
        }
        device.begin();
        unsafe {
            match action {
                InputAction::Key { code, pressed } => {
                    ffi::ei_device_keyboard_key(device.raw, code, pressed);
                    if pressed {
                        device.keys.insert(code);
                    } else {
                        device.keys.remove(&code);
                    }
                }
                InputAction::Button { code, pressed } => {
                    ffi::ei_device_button_button(device.raw, code, pressed);
                    if pressed {
                        device.buttons.insert(code);
                    } else {
                        device.buttons.remove(&code);
                    }
                }
                InputAction::Relative { dx, dy } => {
                    ffi::ei_device_pointer_motion(device.raw, dx, dy)
                }
                InputAction::Absolute { .. } => {
                    let (x, y) = position.expect("checked absolute position");
                    ffi::ei_device_pointer_motion_absolute(device.raw, x, y);
                }
                InputAction::Scroll { dx, dy } => {
                    ffi::ei_device_scroll_delta(device.raw, dx, dy);
                    device.scrolling |= dx != 0.0 || dy != 0.0;
                }
                InputAction::ScrollDiscrete { dx, dy } => {
                    ffi::ei_device_scroll_discrete(device.raw, dx, dy);
                    device.scrolling |= dx != 0 || dy != 0;
                }
                InputAction::CancelScroll => {
                    ffi::ei_device_scroll_cancel(device.raw, true, true);
                    device.scrolling = false;
                }
                InputAction::ReleaseAll => unreachable!(),
            }
            ffi::ei_device_frame(device.raw, now);
        }
        Ok(())
    }
}
pub(crate) fn run(
    fd: OwnedFd,
    stream: StreamGrant,
    run_id: String,
    gate: &InputGate,
    receive: mpsc::Receiver<Request>,
) -> Result<(), String> {
    let mut native = Native::new(fd, stream)?;
    #[cfg(feature = "owned-acceptance-diagnostics")]
    let mut diagnostics = diagnostics::Diagnostics::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if gate.state() == InputState::Revoked {
            return Ok(());
        }
        native.events()?;
        #[cfg(feature = "owned-acceptance-diagnostics")]
        diagnostics.observe(&native);
        let caps = native.capabilities();
        {
            let mut state = gate.0.lock().map_err(|_| "EI authority lock poisoned")?;
            if *state == InputState::Revoked {
                return Ok(());
            }
            if native.connected
                && (caps.keyboard
                    || caps.relative_pointer
                    || caps.buttons
                    || caps.scroll
                    || caps.absolute_region.is_some())
            {
                *state = InputState::Ready(caps);
            } else {
                *state = InputState::Starting;
                if Instant::now() >= deadline {
                    return Err("EI device negotiation timed out".into());
                }
            }
        }
        match receive.try_recv() {
            Ok(request) => {
                if request.response.is_closed() {
                    continue;
                }
                // Serialize revoke with native dispatch. When revoke returns no
                // new action can start, including an already queued request.
                let state = gate.0.lock().map_err(|_| "EI authority lock poisoned")?;
                let result = if Instant::now() > request.deadline {
                    Err("EI command expired without submission".into())
                } else if !matches!(*state, InputState::Ready(ref c) if c.generation == request.generation)
                {
                    Err("EI authority or device generation changed".into())
                } else {
                    let action = || {
                        request.cancellation.check()?;
                        if request.following.is_empty() {
                            native.action(request.action)
                        } else {
                            let mut actions = vec![request.action];
                            actions.extend(request.following);
                            native.compound(actions, &request.cancellation)
                        }
                    };
                    let submitted = match request.frame {
                        Some(frame) => frame.dispatch(action),
                        None => action(),
                    };
                    submitted.map(|()| {
                        native.sequence += 1;
                        InputSubmission {
                            run_id: run_id.clone(),
                            generation: native.generation,
                            sequence: native.sequence,
                        }
                    })
                };
                drop(state);
                if request.response.send(result).is_err() {
                    gate.revoke();
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let mut fd = libc::pollfd {
            fd: unsafe { ffi::ei_get_fd(native.raw) },
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut fd, 1, 10) };
        if result < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err("EI native poll failed".into());
        }
    }
}
