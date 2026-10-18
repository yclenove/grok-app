//! Chord blacklist, dangerous text, and address-bar input checks.
//!
//! Checks are synchronous. They do not block, time out, cancel, or echo the
//! text they reject. Scheme and host checks strip whitespace, bidi marks,
//! BOM, zero-width characters, and soft hyphens, then case-fold.

use super::text::fold;
use super::{ActionIntent, Tool};
use crate::model::Role;
use crate::tree::{ProcessedNode, ProcessedTree};
use gcu_protocol::keys::{Key, KeyChord, Modifier, NamedKey};

/// What kind of chord `key` is, after modifier order is normalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChordClass {
    /// A letter, digit, arrow, or other unmodified typing key.
    Ordinary,
    /// Enter or Space, with no modifier or only Shift.
    Activate,
    /// A chord that deletes, submits, quits, or closes by itself.
    Sensitive,
    /// Lock, logout, force-quit, or the stop hotkey.
    Blacklisted,
    /// Any other modifier chord, or an unmodified function key.
    Custom,
    /// The chord was present but could not be parsed.
    Invalid,
}

/// Why input is refused before it is shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputBlock {
    /// The chord does not parse.
    InvalidKey,
    /// The chord is on the hard blacklist.
    BlacklistedChord,
    /// The text uses a dangerous scheme or a metadata host.
    DangerousInput,
    /// The focused or targeted field is an address bar.
    AddressBar,
}

/// Classify `raw`. An empty string is [`ChordClass::Invalid`].
///
/// Modifiers are sorted and de-duplicated. Character keys are compared
/// case-insensitively. Blocking: none. Timeout: none. Cancellation: none.
pub(crate) fn classify_chord(raw: &str) -> ChordClass {
    let Ok(chord) = KeyChord::parse(raw) else {
        return ChordClass::Invalid;
    };
    let (mods, key) = normalize(&chord);
    if blacklisted(&mods, &key) {
        ChordClass::Blacklisted
    } else if sensitive(&mods, &key) {
        ChordClass::Sensitive
    } else if activate(&mods, &key) {
        ChordClass::Activate
    } else if custom(&mods, &key) {
        ChordClass::Custom
    } else {
        ChordClass::Ordinary
    }
}

/// Block a chord or text payload. `None` means this guard does not apply.
///
/// Address-bar detection uses the focused node when the intent has no element
/// target, and the targeted node when it does. A click on the bar is not
/// blocked. Dangerous schemes are checked whenever `intent.text` is set,
/// including batch and script. A missing tree skips the address-bar focus
/// check and still checks schemes.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared
/// references only.
pub(crate) fn blocks_input(
    intent: &ActionIntent,
    tree: Option<&ProcessedTree>,
    target: Option<&ProcessedNode>,
) -> Option<InputBlock> {
    if let Some(key) = intent.key.as_deref() {
        match classify_chord(key) {
            ChordClass::Invalid => return Some(InputBlock::InvalidKey),
            ChordClass::Blacklisted => return Some(InputBlock::BlacklistedChord),
            ChordClass::Ordinary
            | ChordClass::Activate
            | ChordClass::Sensitive
            | ChordClass::Custom => {}
        }
    } else if intent.tool == Tool::PressKey {
        return Some(InputBlock::InvalidKey);
    }
    if intent.text.as_deref().is_some_and(dangerous_text) {
        return Some(InputBlock::DangerousInput);
    }
    if input_tool(intent.tool) && address_bar(intent, tree, target) {
        return Some(InputBlock::AddressBar);
    }
    None
}

fn input_tool(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::TypeText | Tool::SetValue | Tool::PressKey | Tool::Batch | Tool::Script
    )
}

fn address_bar(
    intent: &ActionIntent,
    tree: Option<&ProcessedTree>,
    target: Option<&ProcessedNode>,
) -> bool {
    if let Some(node) = target {
        return looks_like_address(node);
    }
    if intent.element_index.is_some() || intent.at.is_some() {
        return false;
    }
    tree.and_then(super::target::focused)
        .is_some_and(looks_like_address)
}

fn looks_like_address(node: &ProcessedNode) -> bool {
    let field = matches!(
        node.node.role,
        Role::TextField | Role::SearchField | Role::ComboBox | Role::Unknown
    );
    field
        && super::target::labels(node).iter().any(|label| {
            let folded = fold(label);
            ADDRESS_HINTS
                .iter()
                .any(|hint| super::text::phrase_in(&folded, hint))
        })
}

const ADDRESS_HINTS: &[&str] = &[
    "address",
    "url",
    "location",
    "omnibox",
    "地址",
    "网址",
    "アドレス",
    "주소",
];

pub(crate) fn dangerous_text(raw: &str) -> bool {
    let cleaned = clean(raw);
    if cleaned.is_empty() {
        return false;
    }
    if SCHEMES.iter().any(|scheme| cleaned.starts_with(scheme)) {
        return true;
    }
    if super::text::phrase_in(&cleaned, "metadata.google.internal") {
        return true;
    }
    cleaned
        .split(|c: char| !matches!(c, '0'..='9' | 'a'..='f' | 'x' | '.' | ':'))
        .any(|token| metadata_host(strip_port(token.trim_matches(|c| c == '[' || c == ']'))))
}

fn strip_port(host: &str) -> &str {
    if let Some((left, right)) = host.rsplit_once(':') {
        if !left.is_empty()
            && !left.contains(':')
            && !right.is_empty()
            && right.bytes().all(|byte| byte.is_ascii_digit())
        {
            return left;
        }
    }
    host
}

fn clean(raw: &str) -> String {
    fold(raw).chars().filter(|c| !ignored(*c)).collect()
}

fn ignored(c: char) -> bool {
    c.is_whitespace()
        || c == '\u{00ad}'
        || c == '\u{feff}'
        || ('\u{200b}'..='\u{200f}').contains(&c)
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2060}'..='\u{2064}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
}

const SCHEMES: &[&str] = &[
    "javascript:",
    "file:",
    "data:",
    "vbscript:",
    "blob:",
    "mailto:",
    "tel:",
    "sms:",
    "slack:",
    "zoommtg:",
    "msteams:",
    "facetime:",
    "calculator:",
    "ms-settings:",
    "vscode:",
    "cursor:",
    "itms:",
    "itms-apps:",
    "market:",
    "intent:",
    "shell:",
    "smb:",
    "ms-msdt:",
    "search-ms:",
];

fn metadata_host(host: &str) -> bool {
    if host == "metadata.google.internal" {
        return true;
    }
    if let Some(mapped) = host.strip_prefix("::ffff:") {
        if metadata_host(mapped) {
            return true;
        }
        let mut parts = mapped.split(':');
        if let (Some(high), Some(low), None) = (parts.next(), parts.next(), parts.next()) {
            if let (Ok(high), Ok(low)) =
                (u16::from_str_radix(high, 16), u16::from_str_radix(low, 16))
            {
                return is_metadata_ip((u32::from(high) << 16) | u32::from(low));
            }
        }
        return false;
    }
    ipv4_value(host).is_some_and(is_metadata_ip)
}

fn is_metadata_ip(value: u32) -> bool {
    const LINK_LOCAL: u32 = (169 << 24) | (254 << 16) | (169 << 8) | 254;
    const AWS: u32 = (169 << 24) | (254 << 16) | (170 << 8) | 2;
    const ALIBABA: u32 = (100 << 24) | (100 << 16) | (100 << 8) | 200;
    value == LINK_LOCAL || value == AWS || value == ALIBABA
}

fn ipv4_value(host: &str) -> Option<u32> {
    let parts: Vec<&str> = host.split('.').collect();
    match parts.as_slice() {
        [only] => parse_int(only),
        [a, b, c, d] => {
            let octets = [
                parse_octet(a)?,
                parse_octet(b)?,
                parse_octet(c)?,
                parse_octet(d)?,
            ];
            Some(u32::from_be_bytes(octets))
        }
        _ => None,
    }
}

fn parse_octet(part: &str) -> Option<u8> {
    let value = parse_int(part)?;
    u8::try_from(value).ok()
}

fn parse_int(part: &str) -> Option<u32> {
    if let Some(hex) = part.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).ok();
    }
    if part.len() > 1 && part.starts_with('0') && part.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return u32::from_str_radix(part, 8).ok();
    }
    part.parse().ok()
}

fn normalize(chord: &KeyChord) -> (Vec<Modifier>, Key) {
    let mut mods = chord.modifiers.clone();
    mods.sort_by_key(|modifier| match modifier {
        Modifier::Ctrl => 0,
        Modifier::Alt => 1,
        Modifier::Shift => 2,
        Modifier::Super => 3,
        Modifier::Cmd => 4,
        Modifier::Meta => 5,
        Modifier::Fn => 6,
    });
    mods.dedup();
    let key = match &chord.key {
        Key::Character(character) => Key::Character(character.to_ascii_lowercase()),
        other => other.clone(),
    };
    (mods, key)
}

fn only(mods: &[Modifier], expected: &[Modifier]) -> bool {
    mods.len() == expected.len() && expected.iter().all(|modifier| mods.contains(modifier))
}

fn blacklisted(mods: &[Modifier], key: &Key) -> bool {
    match key {
        Key::Named(NamedKey::Delete) => only(mods, &[Modifier::Ctrl, Modifier::Alt]),
        Key::Named(NamedKey::BackSpace) => {
            only(mods, &[Modifier::Ctrl, Modifier::Alt])
                || only(mods, &[Modifier::Ctrl, Modifier::Alt, Modifier::Shift])
        }
        Key::Character('l') => {
            only(mods, &[Modifier::Super])
                || only(mods, &[Modifier::Meta])
                || only(mods, &[Modifier::Ctrl, Modifier::Alt])
        }
        Key::Character('q') => {
            only(mods, &[Modifier::Cmd, Modifier::Ctrl])
                || only(mods, &[Modifier::Cmd, Modifier::Shift])
        }
        Key::Named(NamedKey::Escape) => {
            only(mods, &[Modifier::Cmd, Modifier::Alt])
                || only(mods, &[Modifier::Ctrl, Modifier::Shift])
        }
        _ => false,
    }
}

fn sensitive(mods: &[Modifier], key: &Key) -> bool {
    match key {
        Key::Named(NamedKey::Delete) => {
            only(mods, &[Modifier::Cmd]) || only(mods, &[Modifier::Shift])
        }
        Key::Named(NamedKey::Return | NamedKey::KeypadEnter) => {
            only(mods, &[Modifier::Ctrl]) || only(mods, &[Modifier::Cmd])
        }
        Key::Character('q') => only(mods, &[Modifier::Cmd]),
        Key::Named(NamedKey::Function(4)) => only(mods, &[Modifier::Alt]),
        _ => false,
    }
}

fn activate(mods: &[Modifier], key: &Key) -> bool {
    let plain = mods.is_empty() || only(mods, &[Modifier::Shift]);
    plain
        && matches!(
            key,
            Key::Named(NamedKey::Return | NamedKey::KeypadEnter | NamedKey::Space)
        )
}

fn custom(mods: &[Modifier], key: &Key) -> bool {
    !mods.is_empty() || matches!(key, Key::Named(NamedKey::Function(_)))
}

#[cfg(test)]
mod tests {
    use super::{blocks_input, classify_chord, dangerous_text, ChordClass, InputBlock};
    use crate::policy::{ActionIntent, Tool};

    #[test]
    fn guard_encodings_and_remaining_chords_match() {
        assert!(dangerous_text("::ffff:169.254.169.254"));
        assert!(dangerous_text("::ffff:a9fe:a9fe"));
        assert!(dangerous_text("0251.0376.0251.0376"));
        assert!(dangerous_text("169.254.170.2"));
        assert!(dangerous_text("100.100.100.200"));
        assert!(dangerous_text("http://169.254.169.254:80/latest"));
        assert!(!dangerous_text(" \u{200b} "));
        assert!(!dangerous_text("1.2.3"));
        assert!(matches!(classify_chord("super+l"), ChordClass::Blacklisted));
        assert!(matches!(classify_chord("meta+l"), ChordClass::Blacklisted));
        assert!(matches!(
            classify_chord("ctrl+alt+l"),
            ChordClass::Blacklisted
        ));
        assert!(matches!(
            classify_chord("cmd+alt+escape"),
            ChordClass::Blacklisted
        ));
        assert!(matches!(
            classify_chord("ctrl+shift+escape"),
            ChordClass::Blacklisted
        ));
        assert!(matches!(classify_chord("alt+f4"), ChordClass::Sensitive));
        assert!(matches!(classify_chord("cmd+q"), ChordClass::Sensitive));
        assert!(matches!(
            classify_chord("shift+enter"),
            ChordClass::Activate
        ));
        assert!(matches!(classify_chord("a"), ChordClass::Ordinary));
        assert!(matches!(classify_chord("fn+f5"), ChordClass::Custom));
        let intent = ActionIntent::new(Tool::PressKey);
        assert_eq!(
            blocks_input(&intent, None, None),
            Some(InputBlock::InvalidKey)
        );
    }
}
