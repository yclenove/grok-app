//! Table-driven xdotool compatibility and malformed-input checks.
#![allow(clippy::unwrap_used)]
use gcu_protocol::keys::{Key, KeyChord, Modifier, NamedKey};

#[test]
fn key_chord_valid_table_resolves_aliases_and_named_keys() {
    let mut cases = vec![
        ("Return".into(), vec![], Key::Named(NamedKey::Return)),
        ("enter".into(), vec![], Key::Named(NamedKey::Return)),
        ("Tab".into(), vec![], Key::Named(NamedKey::Tab)),
        ("Escape".into(), vec![], Key::Named(NamedKey::Escape)),
        ("esc".into(), vec![], Key::Named(NamedKey::Escape)),
        ("BackSpace".into(), vec![], Key::Named(NamedKey::BackSpace)),
        ("Delete".into(), vec![], Key::Named(NamedKey::Delete)),
        ("Home".into(), vec![], Key::Named(NamedKey::Home)),
        ("End".into(), vec![], Key::Named(NamedKey::End)),
        ("Page_Up".into(), vec![], Key::Named(NamedKey::PageUp)),
        ("Prior".into(), vec![], Key::Named(NamedKey::PageUp)),
        ("Page_Down".into(), vec![], Key::Named(NamedKey::PageDown)),
        ("Next".into(), vec![], Key::Named(NamedKey::PageDown)),
        ("Up".into(), vec![], Key::Named(NamedKey::Up)),
        ("Down".into(), vec![], Key::Named(NamedKey::Down)),
        ("Left".into(), vec![], Key::Named(NamedKey::Left)),
        ("Right".into(), vec![], Key::Named(NamedKey::Right)),
        ("KP_Enter".into(), vec![], Key::Named(NamedKey::KeypadEnter)),
        ("space".into(), vec![], Key::Named(NamedKey::Space)),
        ("Insert".into(), vec![], Key::Named(NamedKey::Insert)),
        ("a".into(), vec![], Key::Character('a')),
        ("A".into(), vec![], Key::Character('A')),
        ("中".into(), vec![], Key::Character('中')),
        ("🙂".into(), vec![], Key::Character('🙂')),
        ("1".into(), vec![], Key::Character('1')),
        ("plus".into(), vec![], Key::Character('+')),
        (
            "ctrl+plus".into(),
            vec![Modifier::Ctrl],
            Key::Character('+'),
        ),
        (
            "ctrl+alt+shift+Delete".into(),
            vec![Modifier::Ctrl, Modifier::Alt, Modifier::Shift],
            Key::Named(NamedKey::Delete),
        ),
        (
            "cmd+shift+s".into(),
            vec![Modifier::Cmd, Modifier::Shift],
            Key::Character('s'),
        ),
    ];
    for (alias, modifier) in [
        ("ctrl", Modifier::Ctrl),
        ("control", Modifier::Ctrl),
        ("ctl", Modifier::Ctrl),
        ("CTRL", Modifier::Ctrl),
        ("Control_L", Modifier::Ctrl),
        ("control_r", Modifier::Ctrl),
        ("alt", Modifier::Alt),
        ("option", Modifier::Alt),
        ("Alt_L", Modifier::Alt),
        ("ALT", Modifier::Alt),
        ("alt_r", Modifier::Alt),
        ("shift", Modifier::Shift),
        ("SHIFT", Modifier::Shift),
        ("shift_l", Modifier::Shift),
        ("Shift_R", Modifier::Shift),
        ("super", Modifier::Super),
        ("win", Modifier::Super),
        ("windows", Modifier::Super),
        ("Super_L", Modifier::Super),
        ("super_r", Modifier::Super),
        ("cmd", Modifier::Cmd),
        ("command", Modifier::Cmd),
        ("CMD", Modifier::Cmd),
        ("meta", Modifier::Meta),
        ("META", Modifier::Meta),
        ("Meta_L", Modifier::Meta),
        ("meta_r", Modifier::Meta),
        ("fn", Modifier::Fn),
        ("FN", Modifier::Fn),
        ("function", Modifier::Fn),
    ] {
        cases.push((format!("{alias}+a"), vec![modifier], Key::Character('a')));
    }
    for n in 1..=24 {
        cases.push((format!("F{n}"), vec![], Key::Named(NamedKey::Function(n))));
    }
    for n in 0..=9 {
        cases.push((
            format!("KP_{n}"),
            vec![],
            Key::Named(NamedKey::KeypadDigit(n)),
        ));
    }
    assert_eq!(cases.len(), 93);
    for (input, modifiers, key) in cases {
        assert_eq!(
            KeyChord::parse(&input).unwrap(),
            KeyChord { modifiers, key },
            "{input}"
        );
    }
}

#[test]
fn key_chord_invalid_table_returns_specific_diagnostics() {
    let cases = [
        ("", "empty"),
        ("ctrl", "only modifiers"),
        ("ctrl+alt", "only modifiers"),
        ("ctrl+ctrl+a", "duplicate"),
        ("CTRL+control+a", "duplicate"),
        ("alt+option+x", "duplicate"),
        ("win+super+a", "duplicate"),
        ("cmd+command+a", "duplicate"),
        ("shift+Shift_R+a", "duplicate"),
        ("unknown_key", "unknown key"),
        ("ctrl+unknown_key", "unknown key"),
        ("hyper+a", "unknown modifier"),
        ("a+b", "unknown modifier"),
        ("ctrl+", "empty component"),
        ("+a", "empty component"),
        ("ctrl++a", "empty component"),
        ("+", "empty component"),
        ("F0", "unknown key"),
        ("F25", "unknown key"),
        ("F01", "unknown key"),
        ("KP_10", "unknown key"),
        ("KP_-1", "unknown key"),
        (" ", "whitespace"),
        ("ctrl +a", "whitespace"),
        ("ctrl+ a", "whitespace"),
        ("\n", "whitespace"),
        ("\u{0}", "control"),
        ("ab", "unknown key"),
        ("👩‍💻", "unknown key"),
    ];
    for (input, diagnostic) in cases {
        let error = KeyChord::parse(input).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{input:?}: {error}");
    }
}
