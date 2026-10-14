//! Keysym mappings, xkbcommon translation, and X11 keycode resolution/remapping.

use crate::x11::server::XServer;
use gcu_core::driver::DriverError;
use gcu_protocol::keys::{Key, Modifier, NamedKey};

// X11 Keysym constants
/// Backspace key.
pub const XK_BACKSPACE: u32 = 0xff08;
/// Tab key.
pub const XK_TAB: u32 = 0xff09;
/// Return / Enter key.
pub const XK_RETURN: u32 = 0xff0d;
/// Escape key.
pub const XK_ESCAPE: u32 = 0xff1b;
/// Delete key.
pub const XK_DELETE: u32 = 0xffff;
/// Home key.
pub const XK_HOME: u32 = 0xff50;
/// Left arrow key.
pub const XK_LEFT: u32 = 0xff51;
/// Up arrow key.
pub const XK_UP: u32 = 0xff52;
/// Right arrow key.
pub const XK_RIGHT: u32 = 0xff53;
/// Down arrow key.
pub const XK_DOWN: u32 = 0xff54;
/// Page Up key.
pub const XK_PAGE_UP: u32 = 0xff55;
/// Page Down key.
pub const XK_PAGE_DOWN: u32 = 0xff56;
/// End key.
pub const XK_END: u32 = 0xff57;
/// Print Screen key.
pub const XK_PRINT: u32 = 0xff61;
/// Insert key.
pub const XK_INSERT: u32 = 0xff63;
/// Menu key.
pub const XK_MENU: u32 = 0xff67;
/// Pause key.
pub const XK_PAUSE: u32 = 0xff13;
/// Scroll Lock key.
pub const XK_SCROLL_LOCK: u32 = 0xff14;
/// Num Lock key.
pub const XK_NUM_LOCK: u32 = 0xff7f;
/// Caps Lock key.
pub const XK_CAPS_LOCK: u32 = 0xffe5;
/// Left Shift modifier.
pub const XK_SHIFT_L: u32 = 0xffe1;
/// Left Control modifier.
pub const XK_CONTROL_L: u32 = 0xffe3;
/// Left Alt modifier.
pub const XK_ALT_L: u32 = 0xffe9;
/// Left Super / Windows modifier.
pub const XK_SUPER_L: u32 = 0xffeb;
/// Left Meta modifier.
pub const XK_META_L: u32 = 0xffe7;
/// Space character keysym.
pub const XK_SPACE: u32 = 0x0020;

/// Table of static named key to X11 keysym mappings.
const NAMED_KEY_TABLE: &[(NamedKey, u32)] = &[
    (NamedKey::Return, XK_RETURN),
    (NamedKey::Tab, XK_TAB),
    (NamedKey::Escape, XK_ESCAPE),
    (NamedKey::BackSpace, XK_BACKSPACE),
    (NamedKey::Delete, XK_DELETE),
    (NamedKey::Insert, XK_INSERT),
    (NamedKey::Home, XK_HOME),
    (NamedKey::End, XK_END),
    (NamedKey::PageUp, XK_PAGE_UP),
    (NamedKey::PageDown, XK_PAGE_DOWN),
    (NamedKey::Up, XK_UP),
    (NamedKey::Down, XK_DOWN),
    (NamedKey::Left, XK_LEFT),
    (NamedKey::Right, XK_RIGHT),
    (NamedKey::Space, XK_SPACE),
    (NamedKey::KeypadEnter, 0xff8d),
    (NamedKey::KeypadMultiply, 0xffaa),
    (NamedKey::KeypadAdd, 0xffab),
    (NamedKey::KeypadSubtract, 0xffad),
    (NamedKey::KeypadDecimal, 0xffae),
    (NamedKey::KeypadDivide, 0xffaf),
    (NamedKey::CapsLock, XK_CAPS_LOCK),
    (NamedKey::NumLock, XK_NUM_LOCK),
    (NamedKey::ScrollLock, XK_SCROLL_LOCK),
    (NamedKey::Print, XK_PRINT),
    (NamedKey::Pause, XK_PAUSE),
    (NamedKey::Menu, XK_MENU),
];

/// Table of modifier to keysym mappings.
const MODIFIER_TABLE: &[(Modifier, u32)] = &[
    (Modifier::Ctrl, XK_CONTROL_L),
    (Modifier::Alt, XK_ALT_L),
    (Modifier::Shift, XK_SHIFT_L),
    (Modifier::Super, XK_SUPER_L),
    (Modifier::Cmd, XK_SUPER_L),
    (Modifier::Meta, XK_META_L),
    (Modifier::Fn, XK_CONTROL_L),
];

/// Map a [`Modifier`] to its canonical X11 keysym.
pub fn modifier_to_keysym(modifier: Modifier) -> u32 {
    for &(m, sym) in MODIFIER_TABLE {
        if m == modifier {
            return sym;
        }
    }
    XK_CONTROL_L
}

/// Map a [`NamedKey`] to its corresponding X11 keysym.
pub fn named_key_to_keysym(named: NamedKey) -> u32 {
    match named {
        NamedKey::Function(n) => {
            if (1..=24).contains(&n) {
                0xffbe + (n as u32 - 1)
            } else {
                0xffbe // Fallback to F1
            }
        }
        NamedKey::KeypadDigit(d) => {
            if d <= 9 {
                0xffb0 + d as u32
            } else {
                0xffb0
            }
        }
        other => {
            for &(k, sym) in NAMED_KEY_TABLE {
                if k == other {
                    return sym;
                }
            }
            0
        }
    }
}

/// Convert a Unicode scalar to its X11 keysym.
///
/// On Linux, uses `xkbcommon` to perform standard conversions, handling Unicode codepoints
/// via the standard `0x01000000 + cp` encoding.
#[cfg(target_os = "linux")]
pub fn char_to_keysym(ch: char) -> u32 {
    let cp = ch as u32;
    // Standard ASCII and Latin-1 match Unicode codepoints
    if (0x20..=0x7e).contains(&cp) || (0xa0..=0xff).contains(&cp) {
        return cp;
    }
    let sym = xkbcommon::xkb::utf32_to_keysym(cp);
    let raw: u32 = sym.into();
    if raw != 0 {
        raw
    } else {
        // Standard X11 Unicode Keysym: 0x01000000 | codepoint
        0x0100_0000 | cp
    }
}

/// Fallback character to keysym conversion for non-Linux cross-compilation targets.
#[cfg(not(target_os = "linux"))]
pub fn char_to_keysym(ch: char) -> u32 {
    let cp = ch as u32;
    if (0x20..=0x7e).contains(&cp) || (0xa0..=0xff).contains(&cp) {
        cp
    } else {
        0x0100_0000 | cp
    }
}

/// Map a protocol [`Key`] to its X11 keysym.
pub fn key_to_keysym(key: &Key) -> u32 {
    match key {
        Key::Named(named) => named_key_to_keysym(*named),
        Key::Character(ch) => char_to_keysym(*ch),
    }
}

/// Resolved keycode lookup result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedKeycode {
    /// X11 keycode to press.
    pub keycode: u8,
    /// Whether the Shift modifier is required to produce this keysym.
    pub needs_shift: bool,
    /// If a keycode was dynamically remapped, this contains the keycode that must be restored.
    pub remapped_keycode: Option<u8>,
}

/// Resolve a keysym to an existing keycode from the X11 server mapping,
/// or dynamically remap an unused keycode and return it for subsequent restoration.
pub fn resolve_or_remap_keysym(
    x: &dyn XServer,
    keysym: u32,
) -> Result<ResolvedKeycode, DriverError> {
    let (min_kc, max_kc) = x.keyboard_bounds().map_err(|e| DriverError::Native {
        message: e.to_string(),
        maybe_executed: false,
    })?;

    if max_kc < min_kc {
        return Err(DriverError::CapabilityUnavailable);
    }

    let count = max_kc.saturating_sub(min_kc).saturating_add(1);
    let (keysyms, per_kc) =
        x.get_keyboard_mapping(min_kc, count)
            .map_err(|e| DriverError::Native {
                message: e.to_string(),
                maybe_executed: false,
            })?;

    let per = per_kc.max(1) as usize;
    let mut unused_candidate: Option<u8> = None;

    // Scan for existing mapping
    for (i, chunk) in keysyms.chunks(per).enumerate() {
        let kc = min_kc.saturating_add(i as u8);
        let is_all_zero = chunk.iter().all(|&s| s == 0);
        if is_all_zero && unused_candidate.is_none() && kc >= 8 {
            unused_candidate = Some(kc);
        }

        // Check unshifted (level 0)
        if let Some(&s0) = chunk.first() {
            if s0 == keysym {
                return Ok(ResolvedKeycode {
                    keycode: kc,
                    needs_shift: false,
                    remapped_keycode: None,
                });
            }
        }

        // Check shifted (level 1)
        if let Some(&s1) = chunk.get(1) {
            if s1 == keysym {
                return Ok(ResolvedKeycode {
                    keycode: kc,
                    needs_shift: true,
                    remapped_keycode: None,
                });
            }
        }
    }

    // Keysym not currently bound: dynamically remap an unused keycode
    let keysym_slice = vec![keysym; per];
    if let Some(unused_kc) = unused_candidate {
        x.change_keyboard_mapping(unused_kc, per as u8, &keysym_slice)
            .map_err(|e| DriverError::Native {
                message: e.to_string(),
                maybe_executed: false,
            })?;
        let _ = x.flush();
        Ok(ResolvedKeycode {
            keycode: unused_kc,
            needs_shift: false,
            remapped_keycode: Some(unused_kc),
        })
    } else {
        // Fallback: If no keycode was unbound, attempt to remap the highest keycode (e.g. 255)
        let fallback_kc = max_kc;
        x.change_keyboard_mapping(fallback_kc, per as u8, &keysym_slice)
            .map_err(|e| DriverError::Native {
                message: e.to_string(),
                maybe_executed: false,
            })?;
        let _ = x.flush();
        Ok(ResolvedKeycode {
            keycode: fallback_kc,
            needs_shift: false,
            remapped_keycode: Some(fallback_kc),
        })
    }
}

/// Keycodes and chars planned for one typing chunk (see [`plan_text_chunk`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPlan {
    /// How many leading chars of the input the plan covers (always at least one).
    pub consumed: usize,
    /// Resolved keycode per char, aligned with the consumed prefix.
    pub keys: Vec<ResolvedKeycode>,
    /// Distinct keycodes that were remapped for this chunk and must be restored afterwards.
    pub remapped: Vec<u8>,
}

/// Plan the keycodes for a prefix of `chars` in a single mapping scan.
///
/// Characters that already have a keycode reuse it. Every other distinct keysym gets its **own**
/// unused keycode, all remapped up front, so no keycode changes meaning while a client may still
/// be reading it (re-using one keycode per character raced the client's asynchronous
/// `MappingNotify` refresh and typed the wrong characters on slow machines). When the free keycodes
/// run out the plan stops early and the caller types the covered prefix, then plans the rest.
///
/// After the remap this performs an X round trip, so the server has applied the new mapping
/// before the function returns; the caller still waits for clients to refresh their key tables.
pub fn plan_text_chunk(x: &dyn XServer, chars: &[char]) -> Result<TextPlan, DriverError> {
    let native = |e: crate::x11::server::XError| DriverError::Native {
        message: e.to_string(),
        maybe_executed: false,
    };
    let (min_kc, max_kc) = x.keyboard_bounds().map_err(native)?;
    if max_kc < min_kc || chars.is_empty() {
        return Err(DriverError::CapabilityUnavailable);
    }
    let count = max_kc.saturating_sub(min_kc).saturating_add(1);
    let (keysyms, per_kc) = x.get_keyboard_mapping(min_kc, count).map_err(native)?;
    let per = per_kc.max(1) as usize;

    let mut bound: std::collections::HashMap<u32, (u8, bool)> = std::collections::HashMap::new();
    let mut free: Vec<u8> = Vec::new();
    for (i, chunk) in keysyms.chunks(per).enumerate() {
        let kc = min_kc.saturating_add(i as u8);
        if chunk.iter().all(|&s| s == 0) {
            if kc >= 8 {
                free.push(kc);
            }
            continue;
        }
        if let Some(&s0) = chunk.first() {
            bound.entry(s0).or_insert((kc, false));
        }
        if let Some(&s1) = chunk.get(1) {
            bound.entry(s1).or_insert((kc, true));
        }
    }
    free.reverse(); // pop() hands out the lowest free keycode first

    let mut assigned: std::collections::HashMap<u32, u8> = std::collections::HashMap::new();
    let mut keys = Vec::new();
    let mut remapped: Vec<u8> = Vec::new();
    for &ch in chars {
        let sym = char_to_keysym(ch);
        if let Some(&(keycode, needs_shift)) = bound.get(&sym) {
            keys.push(ResolvedKeycode {
                keycode,
                needs_shift,
                remapped_keycode: None,
            });
        } else if let Some(&keycode) = assigned.get(&sym) {
            keys.push(ResolvedKeycode {
                keycode,
                needs_shift: false,
                remapped_keycode: Some(keycode),
            });
        } else if let Some(keycode) = free.pop() {
            assigned.insert(sym, keycode);
            remapped.push(keycode);
            keys.push(ResolvedKeycode {
                keycode,
                needs_shift: false,
                remapped_keycode: Some(keycode),
            });
        } else {
            break;
        }
    }
    if keys.is_empty() {
        return Err(DriverError::CapabilityUnavailable);
    }
    for (&sym, &keycode) in &assigned {
        x.change_keyboard_mapping(keycode, per as u8, &vec![sym; per])
            .map_err(native)?;
    }
    if !remapped.is_empty() {
        let _ = x.flush();
        // A request with a reply returns only after the server has applied the new mapping.
        x.get_keyboard_mapping(min_kc, 1).map_err(native)?;
    }
    Ok(TextPlan {
        consumed: keys.len(),
        keys,
        remapped,
    })
}

/// Restore a previously dynamically remapped keycode back to NoSymbol.
pub fn restore_remapped_keycode(x: &dyn XServer, keycode: u8) {
    let per = if let Ok((_, per_kc)) = x.get_keyboard_mapping(keycode, 1) {
        per_kc.max(1) as usize
    } else {
        2
    };
    let zero_slice = vec![0u32; per];
    if let Err(e) = x.change_keyboard_mapping(keycode, per as u8, &zero_slice) {
        tracing::warn!(keycode, "failed to restore remapped keycode: {e}");
    }
    let _ = x.flush();
}
