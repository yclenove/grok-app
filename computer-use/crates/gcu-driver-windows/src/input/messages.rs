//! Portable message/key/text planning; no Win32 calls or policy decisions.
use super::api::Event;
use gcu_core::{
    driver::{DriverError, Result},
    model::WindowPoint,
};
use gcu_protocol::{
    keys::{Key, KeyChord, Modifier, NamedKey},
    tools::MouseButton,
};

/// Maximum UTF-16 units per input packet (pairs of down/up events).
pub const TEXT_CHUNK_UNITS: usize = 256;
/// Maximum UTF-16 units accepted by this single-batch planner.
pub const TEXT_MAX_UNITS: usize = 8192;
/// Known window classes that cannot accept ordinary posted keyboard/mouse input.
pub fn ineffective_class(class: &str) -> bool {
    [
        "Chrome_RenderWidgetHostHWND",
        "Windows.UI.Core.CoreWindow",
        "ApplicationFrameWindow",
        "Chrome_WidgetWin_0",
        "Chrome_WidgetWin_1",
    ]
    .iter()
    .any(|c| class.eq_ignore_ascii_case(c))
}
/// Preserve Unicode scalars and CRLF across packet boundaries; never split a surrogate pair.
pub fn text_chunks(text: &str) -> Result<Vec<Vec<Event>>> {
    if text.encode_utf16().count() > TEXT_MAX_UNITS {
        return Err(DriverError::InvalidRequest);
    }
    let mut chunks = Vec::new();
    let mut packet = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let mut units = c.encode_utf16(&mut [0; 2]).to_vec();
        if c == '\r' && chars.peek() == Some(&'\n') {
            chars.next();
            units.push(10);
        }
        if packet.len() / 2 + units.len() > TEXT_CHUNK_UNITS {
            chunks.push(std::mem::take(&mut packet));
        }
        for u in units {
            packet.push(Event::Unicode(u, true));
            packet.push(Event::Unicode(u, false));
        }
    }
    if !packet.is_empty() {
        chunks.push(packet);
    }
    Ok(chunks)
}
/// Plan supported virtual keys. Character VKs are resolved in the target's
/// keyboard layout by the native adapter; Fn is unavailable on Windows.
pub fn named_key(key: NamedKey) -> Result<(u16, bool)> {
    use NamedKey::*;
    Ok(match key {
        Return | KeypadEnter => (0x0d, key == KeypadEnter),
        Tab => (9, false),
        Escape => (0x1b, false),
        BackSpace => (8, false),
        Delete => (0x2e, true),
        Insert => (0x2d, true),
        Home => (0x24, true),
        End => (0x23, true),
        PageUp => (0x21, true),
        PageDown => (0x22, true),
        Up => (0x26, true),
        Down => (0x28, true),
        Left => (0x25, true),
        Right => (0x27, true),
        Space => (0x20, false),
        Function(n) if (1..=24).contains(&n) => (0x6f + u16::from(n), false),
        KeypadDigit(n) if n <= 9 => (0x60 + u16::from(n), false),
        KeypadAdd => (0x6b, false),
        KeypadSubtract => (0x6d, false),
        KeypadMultiply => (0x6a, false),
        KeypadDivide => (0x6f, true),
        KeypadDecimal => (0x6e, false),
        CapsLock => (0x14, false),
        NumLock => (0x90, true),
        ScrollLock => (0x91, false),
        Print => (0x2c, true),
        Pause => (0x13, false),
        Menu => (0x5d, true),
        _ => return Err(DriverError::InvalidRequest),
    })
}
/// Chord with an already resolved character key and layout-required modifiers.
pub fn chord_events(
    chord: &KeyChord,
    character: Option<(u16, Vec<Modifier>)>,
) -> Result<Vec<Event>> {
    let (key, extended, extras) = match chord.key {
        Key::Named(n) => {
            let (k, e) = named_key(n)?;
            (k, e, Vec::new())
        }
        Key::Character(_) => {
            let (k, m) = character.ok_or(DriverError::CapabilityUnavailable)?;
            (k, false, m)
        }
    };
    let mut mods = chord.modifiers.clone();
    for m in extras {
        if !mods.contains(&m) {
            mods.push(m);
        }
    }
    let alt = mods.contains(&Modifier::Alt);
    let mut keys = Vec::new();
    for m in mods {
        keys.push(match m {
            Modifier::Ctrl => (0x11, false),
            Modifier::Alt => (0x12, false),
            Modifier::Shift => (0x10, false),
            Modifier::Super | Modifier::Cmd | Modifier::Meta => (0x5b, true),
            Modifier::Fn => return Err(DriverError::CapabilityUnavailable),
        });
    }
    keys.push((key, extended));
    let mut events: Vec<_> = keys
        .iter()
        .map(|(k, e)| Event::Key(*k, true, *e, alt))
        .collect();
    events.extend(
        keys.iter()
            .rev()
            .map(|(k, e)| Event::Key(*k, false, *e, alt)),
    );
    Ok(events)
}
/// Interpolate a drag with at most ten physical pixels between samples.
pub fn drag_events(path: &[WindowPoint]) -> Result<Vec<Event>> {
    if path.len() < 2 || path.len() > 512 {
        return Err(DriverError::InvalidRequest);
    }
    let mut events = vec![
        Event::Move(path[0], false),
        Event::Button(MouseButton::Left, true, path[0], false),
    ];
    for pair in path.windows(2) {
        let dx = pair[1].x - pair[0].x;
        let dy = pair[1].y - pair[0].y;
        let steps = (dx.hypot(dy) / 10.).ceil().max(1.) as usize;
        if events.len().saturating_add(steps) > 4096 {
            return Err(DriverError::InvalidRequest);
        }
        for n in 1..=steps {
            let t = n as f64 / steps as f64;
            events.push(Event::Move(
                WindowPoint {
                    x: pair[0].x + dx * t,
                    y: pair[0].y + dy * t,
                },
                true,
            ));
        }
    }
    events.push(Event::Button(
        MouseButton::Left,
        false,
        path[path.len() - 1],
        false,
    ));
    Ok(events)
}
