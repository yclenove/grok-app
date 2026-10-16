//! Nonblocking EI transport boundary and exact stream-to-region coordinate mapping.

use gcu_core::{
    driver::{DriverError, Result},
    model::WindowPoint,
};
use std::collections::BTreeMap;

#[cfg(target_os = "linux")]
#[path = "ei_native.rs"]
mod native;
#[cfg(target_os = "linux")]
pub use native::ReisTransport;

/// A single synthetic transition. Debug deliberately omits text and coordinates.
#[derive(Clone, PartialEq)]
pub enum InputEvent {
    /// Linux evdev key transition (XKB keycode minus eight).
    Key {
        /// Evdev code.
        code: u32,
        /// Down when true.
        pressed: bool,
    },
    /// Linux BTN_* transition.
    Button {
        /// Evdev button code.
        code: u32,
        /// Down when true.
        pressed: bool,
    },
    /// Absolute compositor logical coordinates, paired by mapping ID.
    Absolute(f32, f32),
    /// Continuous scroll delta in logical pixels.
    Scroll(f32, f32),
    /// Exact UTF-8, only when the compositor negotiates ei_text.
    Text(String),
}
impl std::fmt::Debug for InputEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Key { .. } => "Key",
            Self::Button { .. } => "Button",
            Self::Absolute(..) => "Absolute",
            Self::Scroll(..) => "Scroll",
            Self::Text(_) => "Text",
        })
    }
}

/// A held transition recorded before calling the native transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HeldInput {
    /// Evdev key.
    Key(u32),
    /// Evdev button.
    Button(u32),
}
impl HeldInput {
    pub(crate) fn up(self) -> InputEvent {
        match self {
            Self::Key(code) => InputEvent::Key {
                code,
                pressed: false,
            },
            Self::Button(code) => InputEvent::Button {
                code,
                pressed: false,
            },
        }
    }
}
impl InputEvent {
    pub(crate) fn held(&self) -> Option<(HeldInput, bool)> {
        match *self {
            Self::Key { code, pressed } => Some((HeldInput::Key(code), pressed)),
            Self::Button { code, pressed } => Some((HeldInput::Button(code), pressed)),
            _ => None,
        }
    }
}

/// Compositor-advertised EI region. A matching size/title is never source proof.
#[derive(Debug, Clone, PartialEq)]
pub struct InputRegion {
    /// Exact mapping ID shared with the portal stream.
    pub mapping_id: String,
    /// Logical offset.
    pub x: u32,
    /// Logical offset.
    pub y: u32,
    /// Logical width.
    pub width: u32,
    /// Logical height.
    pub height: u32,
}
impl InputRegion {
    /// Convert full-window physical pixels, checking bounds again after f32 wire rounding.
    /// Cropped/rotated images must be normalized by capture before constructing this mapping.
    pub fn map(&self, size: (u32, u32), point: WindowPoint) -> Result<(f32, f32)> {
        if size.0 == 0
            || size.1 == 0
            || self.width == 0
            || self.height == 0
            || self.mapping_id.is_empty()
            || !point.x.is_finite()
            || !point.y.is_finite()
            || point.x < 0.0
            || point.y < 0.0
            || point.x >= f64::from(size.0)
            || point.y >= f64::from(size.1)
        {
            return Err(DriverError::OutOfBounds);
        }
        let right = self
            .x
            .checked_add(self.width)
            .ok_or(DriverError::OutOfBounds)?;
        let bottom = self
            .y
            .checked_add(self.height)
            .ok_or(DriverError::OutOfBounds)?;
        let x = (f64::from(self.x) + point.x * f64::from(self.width) / f64::from(size.0)) as f32;
        let y = (f64::from(self.y) + point.y * f64::from(self.height) / f64::from(size.1)) as f32;
        if f64::from(x) < f64::from(self.x)
            || f64::from(x) >= f64::from(right)
            || f64::from(y) < f64::from(self.y)
            || f64::from(y) >= f64::from(bottom)
        {
            return Err(DriverError::OutOfBounds);
        }
        Ok((x, y))
    }
}

/// An immutable device generation, invalidated on pause, layout change or removal.
#[derive(Clone)]
pub struct EiSnapshot {
    /// Monotonic generation; never reused on reconnect.
    pub generation: u64,
    /// Exact stream-paired absolute pointer region.
    pub region: InputRegion,
    /// XKB keysym -> (evdev code, requires Shift), read from the advertised keymap.
    pub keys: BTreeMap<u32, (u32, bool)>,
    /// Negotiated UTF-8 interface, with no clipboard/Unicode-entry fallback.
    pub text: bool,
}

/// Native EI operations, replaceable on every platform.
/// Implementations MUST use nonblocking sockets and bounded in-memory critical sections;
/// neither send nor disconnect may await the action/approval queue or perform blocking IO.
pub trait EiTransport: Send + Sync {
    /// Current generation; None after disconnection or until required devices resume.
    fn snapshot(&self) -> Option<EiSnapshot>;
    /// Send exactly one transition and one device frame. A generation is mandatory for
    /// ordinary input; None is reserved for emergency ups. Failure may mean delivery.
    fn send(&self, event: &InputEvent, generation: Option<u64>) -> Result<()>;
    /// Immediately shut down the EI socket; stops processing already queued requests.
    fn disconnect(&self);
}
