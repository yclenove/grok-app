//! Key chords and Unicode chunks for `postToPid` / HID taps.
//!
//! Character keys use the ANSI virtual-key map. Unmapped characters are one
//! Unicode event so a non-ASCII chord is not dropped. Text is split on UTF-16
//! boundaries of 20 units, preserving grapheme clusters (P1-07).

use gcu_core::driver::{DriverError, KeyChord, Result};
use gcu_protocol::keys::{Key, Modifier, NamedKey};

use super::events::{
    key_event, EventKind, InputTarget, PostPath, SyntheticEvent, FLAG_ALTERNATE, FLAG_COMMAND,
    FLAG_CONTROL, FLAG_FN, FLAG_SHIFT,
};

/// Key downs then matching ups. The ledger wraps each down before it is posted.
pub fn plan_chord(
    target: &InputTarget,
    chord: &KeyChord,
    path: PostPath,
) -> Result<Vec<SyntheticEvent>> {
    let mut flags = 0u64;
    let mut downs = Vec::new();
    for modifier in &chord.modifiers {
        let (code, bit) = modifier_key(*modifier)?;
        flags |= bit;
        downs.push(key_event(
            target,
            path,
            EventKind::KeyDown,
            code,
            flags,
            true,
            None,
        ));
    }
    let resolved = resolve_key(&chord.key)?;
    if resolved.shift && flags & FLAG_SHIFT == 0 {
        flags |= FLAG_SHIFT;
        downs.push(key_event(
            target,
            path,
            EventKind::KeyDown,
            0x38,
            flags,
            true,
            None,
        ));
    }
    downs.push(match resolved.unicode {
        Some(text) => key_event(target, path, EventKind::Unicode, 0, flags, true, Some(text)),
        None => key_event(
            target,
            path,
            EventKind::KeyDown,
            resolved.code,
            flags,
            true,
            None,
        ),
    });
    let mut events = downs.clone();
    for down in downs.into_iter().rev() {
        if let Some(up) = down.release_event() {
            let mut up = up;
            up.flags = flags;
            events.push(up);
        }
    }
    Ok(events)
}

/// Unicode key pairs. More than 8192 scalars is [`DriverError::InvalidRequest`].
pub fn plan_text(target: &InputTarget, text: &str, path: PostPath) -> Result<Vec<SyntheticEvent>> {
    if text.is_empty() || text.chars().count() > 8192 {
        return Err(DriverError::InvalidRequest);
    }
    let mut events = Vec::new();
    for chunk in super::text::grapheme_chunks(text)? {
        let down = key_event(target, path, EventKind::Unicode, 0, 0, true, Some(chunk));
        let up = down.release_event().ok_or(DriverError::InvalidRequest)?;
        events.push(down);
        events.push(up);
    }
    Ok(events)
}

/// Compatibility helper; invalid oversized graphemes produce no events.
/// Use `text::grapheme_chunks` when the rejection must be reported.
pub fn utf16_chunks(text: &str) -> Vec<String> {
    super::text::grapheme_chunks(text).unwrap_or_default()
}

struct Resolved {
    code: u16,
    shift: bool,
    unicode: Option<String>,
}

fn resolve_key(key: &Key) -> Result<Resolved> {
    match key {
        Key::Named(named) => Ok(Resolved {
            code: named_key(*named)?,
            shift: false,
            unicode: None,
        }),
        Key::Character(ch) => {
            if let Some(code) = ansi_key(*ch) {
                return Ok(Resolved {
                    code,
                    shift: needs_shift(*ch),
                    unicode: None,
                });
            }
            Ok(Resolved {
                code: 0,
                shift: false,
                unicode: Some(ch.to_string()),
            })
        }
    }
}

fn modifier_key(modifier: Modifier) -> Result<(u16, u64)> {
    Ok(match modifier {
        Modifier::Shift => (0x38, FLAG_SHIFT),
        Modifier::Ctrl => (0x3B, FLAG_CONTROL),
        Modifier::Alt => (0x3A, FLAG_ALTERNATE),
        Modifier::Cmd | Modifier::Super | Modifier::Meta => (0x37, FLAG_COMMAND),
        Modifier::Fn => (0x3F, FLAG_FN),
    })
}

fn named_key(key: NamedKey) -> Result<u16> {
    Ok(match key {
        NamedKey::Return | NamedKey::KeypadEnter => {
            if matches!(key, NamedKey::KeypadEnter) {
                0x4C
            } else {
                0x24
            }
        }
        NamedKey::Tab => 0x30,
        NamedKey::Space => 0x31,
        NamedKey::BackSpace => 0x33,
        NamedKey::Escape => 0x35,
        NamedKey::Delete => 0x75,
        NamedKey::Left => 0x7B,
        NamedKey::Right => 0x7C,
        NamedKey::Down => 0x7D,
        NamedKey::Up => 0x7E,
        NamedKey::Home => 0x73,
        NamedKey::End => 0x77,
        NamedKey::PageUp => 0x74,
        NamedKey::PageDown => 0x79,
        NamedKey::CapsLock => 0x39,
        NamedKey::Function(n) => function_key(n)?,
        NamedKey::KeypadDigit(n) if n <= 9 => keypad_digit(n),
        NamedKey::KeypadAdd => 0x45,
        NamedKey::KeypadSubtract => 0x4E,
        NamedKey::KeypadMultiply => 0x43,
        NamedKey::KeypadDivide => 0x4B,
        NamedKey::KeypadDecimal => 0x41,
        _ => return Err(DriverError::InvalidRequest),
    })
}

fn function_key(n: u8) -> Result<u16> {
    Ok(match n {
        1 => 0x7A,
        2 => 0x78,
        3 => 0x63,
        4 => 0x76,
        5 => 0x60,
        6 => 0x61,
        7 => 0x62,
        8 => 0x64,
        9 => 0x65,
        10 => 0x6D,
        11 => 0x67,
        12 => 0x6F,
        13 => 0x69,
        14 => 0x6B,
        15 => 0x71,
        16 => 0x6A,
        17 => 0x40,
        18 => 0x4F,
        19 => 0x50,
        20 => 0x5A,
        _ => return Err(DriverError::InvalidRequest),
    })
}

fn keypad_digit(n: u8) -> u16 {
    match n {
        0 => 0x52,
        1 => 0x53,
        2 => 0x54,
        3 => 0x55,
        4 => 0x56,
        5 => 0x57,
        6 => 0x58,
        7 => 0x59,
        8 => 0x5B,
        _ => 0x5C,
    }
}

fn needs_shift(ch: char) -> bool {
    ch.is_ascii_uppercase()
        || matches!(
            ch,
            '!' | '@'
                | '#'
                | '$'
                | '%'
                | '^'
                | '&'
                | '*'
                | '('
                | ')'
                | '_'
                | '+'
                | '{'
                | '}'
                | '|'
                | ':'
                | '"'
                | '<'
                | '>'
                | '?'
                | '~'
        )
}

fn ansi_key(ch: char) -> Option<u16> {
    const LETTERS: &[(char, u16)] = &[
        ('a', 0x00),
        ('s', 0x01),
        ('d', 0x02),
        ('f', 0x03),
        ('h', 0x04),
        ('g', 0x05),
        ('z', 0x06),
        ('x', 0x07),
        ('c', 0x08),
        ('v', 0x09),
        ('b', 0x0B),
        ('q', 0x0C),
        ('w', 0x0D),
        ('e', 0x0E),
        ('r', 0x0F),
        ('y', 0x10),
        ('t', 0x11),
        ('o', 0x1F),
        ('u', 0x20),
        ('i', 0x22),
        ('p', 0x23),
        ('l', 0x25),
        ('j', 0x26),
        ('k', 0x28),
        ('n', 0x2D),
        ('m', 0x2E),
    ];
    let lower = ch.to_ascii_lowercase();
    if let Some((_, code)) = LETTERS.iter().find(|(c, _)| *c == lower) {
        return Some(*code);
    }
    Some(match ch {
        '1' | '!' => 0x12,
        '2' | '@' => 0x13,
        '3' | '#' => 0x14,
        '4' | '$' => 0x15,
        '5' | '%' => 0x17,
        '6' | '^' => 0x16,
        '7' | '&' => 0x1A,
        '8' | '*' => 0x1C,
        '9' | '(' => 0x19,
        '0' | ')' => 0x1D,
        '-' | '_' => 0x1B,
        '=' | '+' => 0x18,
        '[' | '{' => 0x21,
        ']' | '}' => 0x1E,
        '\\' | '|' => 0x2A,
        ';' | ':' => 0x29,
        '\'' | '"' => 0x27,
        ',' | '<' => 0x2B,
        '.' | '>' => 0x2F,
        '/' | '?' => 0x2C,
        '`' | '~' => 0x32,
        ' ' => 0x31,
        _ => return None,
    })
}
