//! Pure Rust reis 0.7 sender. Only portal-provided FDs; never LIBEI_SOCKET/environment.
use super::{EiSnapshot, EiTransport, InputEvent, InputRegion};
use futures_core::Stream;
use gcu_core::driver::{DriverError, Result, DRIVER_DEADLINE};
use reis::{ei, PendingRequestResult};
use std::{
    collections::{BTreeMap, HashMap},
    future::poll_fn,
    net::Shutdown,
    os::{fd::OwnedFd, unix::net::UnixStream},
    pin::Pin,
    sync::{Arc, Mutex},
};
use tokio::{sync::Notify, task::JoinHandle, time::Instant};
#[path = "ei_keys.rs"]
mod keys;

fn fault() -> DriverError {
    DriverError::Native {
        message: "EI transport failed".into(),
        maybe_executed: true,
    }
}
#[derive(Default)]
struct Device {
    interfaces: HashMap<String, reis::Object>,
    regions: Vec<InputRegion>,
    next_mapping: Option<String>,
    keys: BTreeMap<u32, (u32, bool)>,
    done: bool,
    resumed: bool,
}
impl Device {
    fn interface<T: ei::Interface>(&self) -> Option<T> {
        self.interfaces.get(T::NAME)?.clone().downcast()
    }
}
struct State {
    context: ei::Context,
    devices: HashMap<ei::Device, Device>,
    masks: HashMap<ei::Seat, u64>,
    mapping: String,
    serial: u32,
    sequence: u32,
    generation: u64,
    active: bool,
    epoch: Instant,
}
impl State {
    fn snapshot(&self) -> Option<EiSnapshot> {
        if !self.active {
            return None;
        }
        let pointer = self.devices.values().find(|d| {
            d.done
                && d.resumed
                && d.interface::<ei::PointerAbsolute>().is_some()
                && d.interface::<ei::Button>().is_some()
                && d.interface::<ei::Scroll>().is_some()
                && d.regions.iter().any(|r| r.mapping_id == self.mapping)
        })?;
        let keyboard = self
            .devices
            .values()
            .find(|d| d.done && d.resumed && d.interface::<ei::Keyboard>().is_some())?;
        Some(EiSnapshot {
            generation: self.generation,
            region: pointer
                .regions
                .iter()
                .find(|r| r.mapping_id == self.mapping)?
                .clone(),
            keys: keyboard.keys.clone(),
            text: self
                .devices
                .values()
                .any(|d| d.done && d.resumed && d.interface::<ei::Text>().is_some()),
        })
    }
    fn invalidate(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }
    fn event(&mut self, event: ei::Event) -> Result<()> {
        match event {
            ei::Event::Connection(_, ei::connection::Event::Seat { seat }) => {
                if self.masks.len() >= 16 {
                    return Err(fault());
                }
                self.masks.insert(seat, 0);
            }
            ei::Event::Connection(_, ei::connection::Event::Ping { ping }) => ping.done(0),
            ei::Event::Connection(
                _,
                ei::connection::Event::Disconnected { .. }
                | ei::connection::Event::InvalidObject { .. },
            ) => return Err(fault()),
            ei::Event::Seat(seat, ei::seat::Event::Capability { mask, interface }) => {
                if matches!(
                    interface.as_str(),
                    "ei_pointer_absolute" | "ei_keyboard" | "ei_button" | "ei_scroll" | "ei_text"
                ) {
                    *self.masks.get_mut(&seat).ok_or_else(fault)? |= mask;
                }
            }
            ei::Event::Seat(seat, ei::seat::Event::Done) => {
                seat.bind(*self.masks.get(&seat).ok_or_else(fault)?)
            }
            ei::Event::Seat(_, ei::seat::Event::Device { device }) => {
                if self.devices.len() >= 32 {
                    return Err(fault());
                }
                self.devices.insert(device, Device::default());
                self.invalidate();
            }
            ei::Event::Seat(_, ei::seat::Event::Destroyed { .. }) => return Err(fault()),
            ei::Event::Device(device, ei::device::Event::Destroyed { serial }) => {
                self.serial = serial;
                self.devices.remove(&device);
                self.invalidate();
            }
            ei::Event::Device(device, event) => {
                let data = self.devices.get_mut(&device).ok_or_else(fault)?;
                match event {
                    ei::device::Event::Interface { object } => {
                        if data.done || data.interfaces.len() >= 16 {
                            return Err(fault());
                        }
                        data.interfaces.insert(object.interface().into(), object);
                    }
                    ei::device::Event::RegionMappingId { mapping_id } => {
                        if data.done || data.next_mapping.is_some() {
                            return Err(fault());
                        }
                        data.next_mapping = Some(mapping_id);
                    }
                    ei::device::Event::Region {
                        offset_x,
                        offset_y,
                        width,
                        hight,
                        ..
                    } => {
                        if data.done || data.regions.len() >= 32 {
                            return Err(fault());
                        }
                        data.regions.push(InputRegion {
                            mapping_id: data.next_mapping.take().unwrap_or_default(),
                            x: offset_x,
                            y: offset_y,
                            width,
                            height: hight,
                        });
                    }
                    ei::device::Event::Done => {
                        data.done = true;
                    }
                    ei::device::Event::Resumed { serial } => {
                        if !data.done {
                            return Err(fault());
                        }
                        self.serial = serial;
                        data.resumed = true;
                        self.sequence = self.sequence.checked_add(1).ok_or_else(fault)?;
                        device.start_emulating(serial, self.sequence);
                        self.invalidate();
                    }
                    ei::device::Event::Paused { serial } => {
                        self.serial = serial;
                        data.resumed = false;
                        self.invalidate();
                    }
                    _ => {}
                }
            }
            ei::Event::Keyboard(
                _,
                ei::keyboard::Event::Modifiers {
                    serial,
                    group,
                    locked,
                    latched,
                    ..
                },
            ) => {
                self.serial = serial;
                if group != 0 || locked != 0 || latched != 0 {
                    self.active = false;
                    self.invalidate();
                }
            }
            ei::Event::Keyboard(_, ei::keyboard::Event::Destroyed { .. })
            | ei::Event::Button(_, ei::button::Event::Destroyed { .. })
            | ei::Event::PointerAbsolute(_, ei::pointer_absolute::Event::Destroyed { .. }) => {
                self.active = false;
                self.invalidate();
            }
            _ => {}
        }
        self.context.flush().map_err(|_| fault())
    }
    fn send(&mut self, event: &InputEvent, generation: Option<u64>) -> Result<()> {
        if generation.is_some_and(|g| !self.active || g != self.generation)
            || generation.is_none()
                && !matches!(
                    event,
                    InputEvent::Key { pressed: false, .. }
                        | InputEvent::Button { pressed: false, .. }
                )
        {
            return Err(fault());
        }
        // Emergency ups can address the original objects even after a generation changed.
        // Paused devices already have neutral state by EI protocol and cannot receive requests.
        let pair = self.devices.iter().find(|(_, d)| {
            d.done
                && d.resumed
                && match event {
                    InputEvent::Key { .. } => d.interface::<ei::Keyboard>().is_some(),
                    InputEvent::Text(_) => d.interface::<ei::Text>().is_some(),
                    _ => d.regions.iter().any(|r| r.mapping_id == self.mapping),
                }
        });
        let Some((device, data)) = pair else {
            return Err(fault());
        };
        match event {
            InputEvent::Key { code, pressed } => {
                data.interface::<ei::Keyboard>().ok_or_else(fault)?.key(
                    *code,
                    if *pressed {
                        ei::keyboard::KeyState::Press
                    } else {
                        ei::keyboard::KeyState::Released
                    },
                )
            }
            InputEvent::Button { code, pressed } => {
                data.interface::<ei::Button>().ok_or_else(fault)?.button(
                    *code,
                    if *pressed {
                        ei::button::ButtonState::Press
                    } else {
                        ei::button::ButtonState::Released
                    },
                )
            }
            InputEvent::Absolute(x, y) => data
                .interface::<ei::PointerAbsolute>()
                .ok_or_else(fault)?
                .motion_absolute(*x, *y),
            InputEvent::Scroll(x, y) => {
                let scroll = data.interface::<ei::Scroll>().ok_or_else(fault)?;
                scroll.scroll(*x, *y);
                device.frame(self.serial, self.epoch.elapsed().as_micros() as u64);
                scroll.scroll_stop(u32::from(*x != 0.0), u32::from(*y != 0.0), 0);
            }
            InputEvent::Text(text) => data.interface::<ei::Text>().ok_or_else(fault)?.utf8(text),
        }
        device.frame(self.serial, self.epoch.elapsed().as_micros() as u64);
        self.context.flush().map_err(|_| fault())
    }
}

/// A nonblocking reis sender with an independent asynchronous event pump.
/// Device pause/removal/layout changes invalidate all previously compiled actions.
pub struct ReisTransport {
    state: Arc<Mutex<State>>,
    socket: UnixStream,
    pump: Mutex<Option<JoinHandle<()>>>,
}
impl ReisTransport {
    /// Handshake and wait for resumed stream-paired devices within DRIVER_DEADLINE.
    /// Dropping this future/transport shuts down its socket and stops its event pump.
    pub async fn connect(fd: OwnedFd, mapping_id: String) -> Result<Arc<Self>> {
        let socket = UnixStream::from(fd);
        socket.set_nonblocking(true).map_err(|_| fault())?;
        let context =
            ei::Context::new(socket.try_clone().map_err(|_| fault())?).map_err(|_| fault())?;
        let mut events = reis::tokio::EiEventStream::new(context.clone()).map_err(|_| fault())?;
        let handshake = tokio::time::timeout(
            DRIVER_DEADLINE,
            reis::tokio::ei_handshake(&mut events, "gcu", ei::handshake::ContextType::Sender),
        )
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
        .map_err(|_| fault())?;
        let transport = Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                context,
                devices: HashMap::new(),
                masks: HashMap::new(),
                mapping: mapping_id,
                serial: handshake.serial,
                sequence: 0,
                generation: 1,
                active: true,
                epoch: Instant::now(),
            })),
            socket,
            pump: Mutex::new(None),
        });
        let state = transport.state.clone();
        let changed = Arc::new(Notify::new());
        let wake = changed.clone();
        let pump = tokio::spawn(async move {
            while let Some(Ok(PendingRequestResult::Request(event))) =
                poll_fn(|cx| Pin::new(&mut events).poll_next(cx)).await
            {
                if let ei::Event::Keyboard(
                    ref keyboard,
                    ei::keyboard::Event::Keymap {
                        ref keymap,
                        size,
                        keymap_type,
                    },
                ) = event
                {
                    if keymap_type != ei::keyboard::KeymapType::Xkb {
                        break;
                    }
                    let Ok(fd) = keymap.try_clone() else { break };
                    let Ok(decoded) = keys::decode(fd, size).await else {
                        break;
                    };
                    let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(data) = state
                        .devices
                        .values_mut()
                        .find(|d| d.interface::<ei::Keyboard>().as_ref() == Some(keyboard))
                    {
                        data.keys = decoded;
                    } else {
                        break;
                    }
                }
                if state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .event(event)
                    .is_err()
                {
                    break;
                }
                wake.notify_waiters();
            }
            state.lock().unwrap_or_else(|p| p.into_inner()).active = false;
            wake.notify_waiters();
        });
        *transport.pump.lock().unwrap_or_else(|p| p.into_inner()) = Some(pump);
        tokio::time::timeout(DRIVER_DEADLINE, async {
            loop {
                let notified = changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if transport.snapshot().is_some() {
                    return Ok(());
                }
                if !transport
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .active
                {
                    return Err(fault());
                }
                notified.await;
            }
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })??;
        Ok(transport)
    }
}
impl EiTransport for ReisTransport {
    fn snapshot(&self) -> Option<EiSnapshot> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .snapshot()
    }
    fn send(&self, event: &InputEvent, generation: Option<u64>) -> Result<()> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .send(event, generation)
    }
    fn disconnect(&self) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).active = false;
        if let Some(pump) = self.pump.lock().unwrap_or_else(|p| p.into_inner()).take() {
            pump.abort();
        }
        if self.socket.shutdown(Shutdown::Both).is_err() {
            tracing::debug!("EI socket already closed");
        }
    }
}
impl Drop for ReisTransport {
    fn drop(&mut self) {
        self.disconnect();
    }
}
