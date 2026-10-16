//! Preflight complete action plans before the first synthetic transition.
use super::ei::{EiSnapshot, InputEvent};
use crate::x11::keys::{key_to_keysym, modifier_to_keysym};
use gcu_core::driver::{
    Button, DriverAction, DriverError, KeyRoute, PointerRoute, Result, WindowRef,
};
use gcu_protocol::{keys::Modifier, KeyChord};

pub(crate) fn window(action: &DriverAction) -> Option<&WindowRef> {
    match action {
        DriverAction::PointerClick { win, .. }
        | DriverAction::PointerScroll { win, .. }
        | DriverAction::PointerDrag { win, .. }
        | DriverAction::PressKeys { win, .. }
        | DriverAction::TypeText { win, .. } => Some(win),
        _ => None,
    }
}
fn chord(events: &mut Vec<InputEvent>, snap: &EiSnapshot, chord: &KeyChord) -> Result<()> {
    if chord.modifiers.contains(&Modifier::Fn) {
        return Err(DriverError::CapabilityUnavailable);
    }
    let mut codes = Vec::new();
    for modifier in &chord.modifiers {
        let (code, _) = snap
            .keys
            .get(&modifier_to_keysym(*modifier))
            .ok_or(DriverError::CapabilityUnavailable)?;
        if !codes.contains(code) {
            codes.push(*code);
        }
    }
    let (code, shift) = snap
        .keys
        .get(&key_to_keysym(&chord.key))
        .ok_or(DriverError::CapabilityUnavailable)?;
    if *shift {
        let (shift, _) = snap
            .keys
            .get(&modifier_to_keysym(Modifier::Shift))
            .ok_or(DriverError::CapabilityUnavailable)?;
        if !codes.contains(shift) {
            codes.push(*shift);
        }
    }
    if !codes.contains(code) {
        codes.push(*code);
    }
    events.extend(codes.iter().map(|code| InputEvent::Key {
        code: *code,
        pressed: true,
    }));
    events.extend(codes.iter().rev().map(|code| InputEvent::Key {
        code: *code,
        pressed: false,
    }));
    Ok(())
}
pub(crate) fn plan(
    action: &DriverAction,
    snap: &EiSnapshot,
    size: (u32, u32),
) -> Result<Vec<InputEvent>> {
    let mut events = Vec::new();
    let button_code = |button: Button| match button {
        Button::Left => 0x110,
        Button::Right => 0x111,
        Button::Middle => 0x112,
    };
    let mut position = |at| -> Result<()> {
        let (x, y) = snap.region.map(size, at)?;
        events.push(InputEvent::Absolute(x, y));
        Ok(())
    };
    match action {
        DriverAction::PointerClick {
            at,
            button,
            count,
            route,
            ..
        } => {
            if *route != PointerRoute::AllowForeground {
                return Err(DriverError::CapabilityUnavailable);
            }
            if !(1..=3).contains(count) {
                return Err(DriverError::InvalidRequest);
            }
            position(*at)?;
            for _ in 0..*count {
                events.push(InputEvent::Button {
                    code: button_code(*button),
                    pressed: true,
                });
                events.push(InputEvent::Button {
                    code: button_code(*button),
                    pressed: false,
                });
            }
        }
        DriverAction::PointerScroll {
            at, dx, dy, route, ..
        } => {
            if *route != PointerRoute::AllowForeground {
                return Err(DriverError::CapabilityUnavailable);
            }
            position(*at)?;
            events.push(InputEvent::Scroll(*dx as f32, *dy as f32));
        }
        DriverAction::PointerDrag { path, route, .. } => {
            if *route != PointerRoute::AllowForeground {
                return Err(DriverError::CapabilityUnavailable);
            }
            if !(2..=256).contains(&path.len()) {
                return Err(DriverError::InvalidRequest);
            }
            let points = path
                .iter()
                .map(|p| snap.region.map(size, *p))
                .collect::<Result<Vec<_>>>()?;
            events.push(InputEvent::Absolute(points[0].0, points[0].1));
            events.push(InputEvent::Button {
                code: 0x110,
                pressed: true,
            });
            events.extend(points[1..].iter().map(|p| InputEvent::Absolute(p.0, p.1)));
            events.push(InputEvent::Button {
                code: 0x110,
                pressed: false,
            });
        }
        DriverAction::PressKeys {
            chord: keys, route, ..
        } => {
            if *route != KeyRoute::AllowForeground {
                return Err(DriverError::CapabilityUnavailable);
            }
            chord(&mut events, snap, keys)?;
        }
        DriverAction::TypeText {
            text, route, focus, ..
        } => {
            if *route != KeyRoute::AllowForeground
                || focus.is_some()
                || text.len() > 32768
                || text.contains('\0')
            {
                return Err(DriverError::CapabilityUnavailable);
            }
            if snap.text {
                // ei_text's maximum is 254 UTF-8 bytes per request; split only at scalar boundaries.
                let mut chunk = String::new();
                for ch in text.chars() {
                    if chunk.len() + ch.len_utf8() > 254 {
                        events.push(InputEvent::Text(std::mem::take(&mut chunk)));
                    }
                    chunk.push(ch);
                }
                if !chunk.is_empty() {
                    events.push(InputEvent::Text(chunk));
                }
            } else {
                for ch in text.chars() {
                    let key = match ch {
                        '\n' => {
                            gcu_protocol::keys::Key::Named(gcu_protocol::keys::NamedKey::Return)
                        }
                        '\t' => gcu_protocol::keys::Key::Named(gcu_protocol::keys::NamedKey::Tab),
                        _ => gcu_protocol::keys::Key::Character(ch),
                    };
                    chord(
                        &mut events,
                        snap,
                        &KeyChord {
                            modifiers: vec![],
                            key,
                        },
                    )?;
                }
            }
        }
        _ => return Err(DriverError::CapabilityUnavailable),
    }
    if events.len() > 65536 {
        return Err(DriverError::InvalidRequest);
    }
    Ok(events)
}
