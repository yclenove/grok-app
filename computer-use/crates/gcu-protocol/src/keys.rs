//! Pure xdotool chord parsing; no IO, waiting, or cancellation is involved.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// A portable modifier; drivers map it to their platform's physical key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    /// Control.
    Ctrl,
    /// Alt / Option.
    Alt,
    /// Shift.
    Shift,
    /// Super / Windows.
    Super,
    /// macOS Command.
    Cmd,
    /// Meta.
    Meta,
    /// Function modifier.
    Fn,
}

impl Modifier {
    fn parse(input: &str) -> Option<Self> {
        match input.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "ctl" | "control_l" | "control_r" | "ctrl_l" | "ctrl_r" => {
                Some(Self::Ctrl)
            }
            "alt" | "option" | "alt_l" | "alt_r" => Some(Self::Alt),
            "shift" | "shift_l" | "shift_r" => Some(Self::Shift),
            "super" | "win" | "windows" | "super_l" | "super_r" => Some(Self::Super),
            "cmd" | "command" => Some(Self::Cmd),
            "meta" | "meta_l" | "meta_r" => Some(Self::Meta),
            "fn" | "function" => Some(Self::Fn),
            _ => None,
        }
    }
}

impl fmt::Display for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
            Self::Shift => "shift",
            Self::Super => "super",
            Self::Cmd => "cmd",
            Self::Meta => "meta",
            Self::Fn => "fn",
        })
    }
}

/// A non-character key, independent of OS keycodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NamedKey {
    /// Enter / Return.
    Return,
    /// Tab.
    Tab,
    /// Escape.
    Escape,
    /// Backspace.
    BackSpace,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up / Prior.
    PageUp,
    /// Page Down / Next.
    PageDown,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Space bar.
    Space,
    /// Function key, 1–24 when returned by the parser.
    Function(u8),
    /// Numeric keypad digit, 0–9 when returned by the parser.
    KeypadDigit(u8),
    /// Keypad Enter.
    KeypadEnter,
    /// Keypad addition.
    KeypadAdd,
    /// Keypad subtraction.
    KeypadSubtract,
    /// Keypad multiplication.
    KeypadMultiply,
    /// Keypad division.
    KeypadDivide,
    /// Keypad decimal.
    KeypadDecimal,
    /// Caps Lock.
    CapsLock,
    /// Num Lock.
    NumLock,
    /// Scroll Lock.
    ScrollLock,
    /// Print Screen.
    Print,
    /// Pause.
    Pause,
    /// Context menu key.
    Menu,
}

/// A named key or a single Unicode scalar (case is preserved).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    /// One printable character.
    Character(char),
    /// A key with no character representation.
    Named(NamedKey),
}

impl Key {
    fn parse(input: &str) -> Result<Self, KeyParseError> {
        let lower = input.to_ascii_lowercase();
        let named = match lower.as_str() {
            "return" | "enter" => NamedKey::Return,
            "tab" => NamedKey::Tab,
            "escape" | "esc" => NamedKey::Escape,
            "backspace" | "back_space" | "bs" => NamedKey::BackSpace,
            "delete" | "del" => NamedKey::Delete,
            "insert" | "ins" => NamedKey::Insert,
            "home" => NamedKey::Home,
            "end" => NamedKey::End,
            "page_up" | "pageup" | "prior" | "pgup" => NamedKey::PageUp,
            "page_down" | "pagedown" | "next" | "pgdn" => NamedKey::PageDown,
            "up" => NamedKey::Up,
            "down" => NamedKey::Down,
            "left" => NamedKey::Left,
            "right" => NamedKey::Right,
            "space" => NamedKey::Space,
            "kp_enter" => NamedKey::KeypadEnter,
            "kp_add" => NamedKey::KeypadAdd,
            "kp_subtract" => NamedKey::KeypadSubtract,
            "kp_multiply" => NamedKey::KeypadMultiply,
            "kp_divide" => NamedKey::KeypadDivide,
            "kp_decimal" => NamedKey::KeypadDecimal,
            "caps_lock" | "capslock" => NamedKey::CapsLock,
            "num_lock" | "numlock" => NamedKey::NumLock,
            "scroll_lock" | "scrolllock" => NamedKey::ScrollLock,
            "print" | "printscreen" => NamedKey::Print,
            "pause" | "break" => NamedKey::Pause,
            "menu" => NamedKey::Menu,
            _ => {
                if let Some(number) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                    if (1..=24).contains(&number) && lower == format!("f{number}") {
                        return Ok(Self::Named(NamedKey::Function(number)));
                    }
                }
                if let Some(digit) = lower
                    .strip_prefix("kp_")
                    .filter(|n| n.len() == 1)
                    .and_then(|n| n.parse::<u8>().ok())
                {
                    return Ok(Self::Named(NamedKey::KeypadDigit(digit)));
                }
                let symbol = match lower.as_str() {
                    "plus" => Some('+'),
                    "minus" => Some('-'),
                    "equal" => Some('='),
                    "comma" => Some(','),
                    "period" => Some('.'),
                    "slash" => Some('/'),
                    "backslash" => Some('\\'),
                    "semicolon" => Some(';'),
                    "colon" => Some(':'),
                    "apostrophe" => Some('\''),
                    "quotedbl" => Some('"'),
                    "grave" => Some('`'),
                    "bracketleft" => Some('['),
                    "bracketright" => Some(']'),
                    _ => None,
                };
                if let Some(character) = symbol {
                    return Ok(Self::Character(character));
                }
                let mut chars = input.chars();
                if let (Some(character), None) = (chars.next(), chars.next()) {
                    return Ok(Self::Character(character));
                }
                return Err(KeyParseError::UnknownKey(input.into()));
            }
        };
        Ok(Self::Named(named))
    }
}

/// A validated chord produced by [`KeyChord::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyChord {
    /// Modifiers in input order, without duplicates.
    pub modifiers: Vec<Modifier>,
    /// Exactly one non-modifier key.
    pub key: Key,
}

/// A diagnostic for malformed xdotool input.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyParseError {
    /// The input was empty.
    #[error("key chord is empty")]
    Empty,
    /// A separator has no key on one side.
    #[error("key chord contains an empty component")]
    EmptyComponent,
    /// Whitespace must be written as a named key.
    #[error("whitespace is not allowed; use the named key space")]
    Whitespace,
    /// Control characters are not keys.
    #[error("control characters are not allowed")]
    ControlCharacter,
    /// No final key was given.
    #[error("key chord contains only modifiers")]
    OnlyModifiers,
    /// Aliases resolve to the same modifier.
    #[error("duplicate modifier: {0}")]
    DuplicateModifier(Modifier),
    /// A non-final token is not a recognized modifier.
    #[error("unknown modifier: {0}")]
    UnknownModifier(String),
    /// A final token is not a recognized key.
    #[error("unknown key: {0}")]
    UnknownKey(String),
}

impl KeyChord {
    /// Parse `+`-separated xdotool keys, accepting case-insensitive aliases.
    /// This is synchronous, performs no IO, and requires no cancellation or timeout.
    pub fn parse(input: &str) -> Result<Self, KeyParseError> {
        if input.is_empty() {
            return Err(KeyParseError::Empty);
        }
        if input.chars().any(char::is_whitespace) {
            return Err(KeyParseError::Whitespace);
        }
        if input.chars().any(char::is_control) {
            return Err(KeyParseError::ControlCharacter);
        }
        let mut parts = input.split('+').peekable();
        let mut modifiers = Vec::new();
        while let Some(part) = parts.next() {
            if part.is_empty() {
                return Err(KeyParseError::EmptyComponent);
            }
            if let Some(modifier) = Modifier::parse(part) {
                if modifiers.contains(&modifier) {
                    return Err(KeyParseError::DuplicateModifier(modifier));
                }
                modifiers.push(modifier);
                if parts.peek().is_none() {
                    return Err(KeyParseError::OnlyModifiers);
                }
            } else if parts.peek().is_none() {
                return Ok(Self {
                    modifiers,
                    key: Key::parse(part)?,
                });
            } else {
                return Err(KeyParseError::UnknownModifier(part.into()));
            }
        }
        Err(KeyParseError::OnlyModifiers)
    }
}
