//! Structural equality, hashing, and architecture §7.2 grant matching.

use gcu_host_api::{AppIdentity, Platform, SignatureState};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

fn hash_of(identity: &AppIdentity) -> u64 {
    let mut hasher = DefaultHasher::new();
    identity.hash(&mut hasher);
    hasher.finish()
}

fn macos_signed() -> AppIdentity {
    AppIdentity {
        platform: Platform::Macos,
        key: "com.example.notes".into(),
        team_id: Some("TEAM123456".into()),
        publisher: Some("Example Publisher".into()),
        sha256: Some([0x11; 32]),
        signature: SignatureState::Valid,
        image_path: "/Applications/Notes.app/Contents/MacOS/Notes".into(),
    }
}

fn windows_signed() -> AppIdentity {
    AppIdentity {
        platform: Platform::Windows,
        key: "C:\\Program Files\\Notes\\Notes.exe".into(),
        team_id: None,
        publisher: Some("CN=Example Publisher".into()),
        sha256: Some([0x22; 32]),
        signature: SignatureState::Valid,
        image_path: "C:\\Program Files\\Notes\\Notes.exe".into(),
    }
}

fn linux_app() -> AppIdentity {
    AppIdentity {
        platform: Platform::Linux,
        key: "org.example.Notes.desktop".into(),
        team_id: None,
        publisher: None,
        sha256: Some([0x33; 32]),
        signature: SignatureState::Unsigned,
        image_path: "/usr/bin/notes".into(),
    }
}

#[test]
fn app_identity_equal_observations_hash_equally() {
    let left = macos_signed();
    let right = macos_signed();
    assert_eq!(left, right);
    assert_eq!(hash_of(&left), hash_of(&right));
    let mut set = HashSet::new();
    set.insert(left);
    assert!(set.contains(&right));
}

#[test]
fn app_identity_any_field_change_is_not_equal() {
    let base = macos_signed();
    let mutants = [
        (
            "platform",
            AppIdentity {
                platform: Platform::Windows,
                ..base.clone()
            },
        ),
        (
            "key",
            AppIdentity {
                key: "com.example.other".into(),
                ..base.clone()
            },
        ),
        (
            "team_id",
            AppIdentity {
                team_id: Some("OTHERTEAM1".into()),
                ..base.clone()
            },
        ),
        (
            "team_id_cleared",
            AppIdentity {
                team_id: None,
                ..base.clone()
            },
        ),
        (
            "publisher",
            AppIdentity {
                publisher: Some("Other Publisher".into()),
                ..base.clone()
            },
        ),
        (
            "publisher_cleared",
            AppIdentity {
                publisher: None,
                ..base.clone()
            },
        ),
        (
            "sha256",
            AppIdentity {
                sha256: Some([0x44; 32]),
                ..base.clone()
            },
        ),
        (
            "sha256_cleared",
            AppIdentity {
                sha256: None,
                ..base.clone()
            },
        ),
        (
            "signature_invalid",
            AppIdentity {
                signature: SignatureState::Invalid,
                ..base.clone()
            },
        ),
        (
            "signature_unsigned",
            AppIdentity {
                signature: SignatureState::Unsigned,
                ..base.clone()
            },
        ),
        (
            "signature_unknown",
            AppIdentity {
                signature: SignatureState::Unknown,
                ..base.clone()
            },
        ),
        (
            "image_path",
            AppIdentity {
                image_path: "/Applications/Moved.app/Contents/MacOS/Notes".into(),
                ..base.clone()
            },
        ),
    ];
    let mut set = HashSet::new();
    set.insert(base.clone());
    for (name, mutated) in mutants {
        assert_ne!(&mutated, &base, "{name}");
        assert_ne!(hash_of(&mutated), hash_of(&base), "{name}");
        assert!(!set.contains(&mutated), "{name}");
    }
}

#[test]
fn app_identity_grant_eq_signed_macos_ignores_path_and_digest() {
    let granted = macos_signed();
    let mut updated = granted.clone();
    updated.image_path = "/Applications/Moved.app/Contents/MacOS/Notes".into();
    updated.sha256 = None;
    updated.publisher = None;
    assert_ne!(granted, updated);
    assert_ne!(hash_of(&granted), hash_of(&updated));
    assert!(granted.grant_eq(&updated));
    assert!(updated.grant_eq(&granted));

    let mut other_team = granted.clone();
    other_team.team_id = Some("OTHERTEAM1".into());
    assert!(!granted.grant_eq(&other_team));

    let mut other_bundle = granted.clone();
    other_bundle.key = "com.example.other".into();
    assert!(!granted.grant_eq(&other_bundle));

    let mut unsigned = granted.clone();
    unsigned.signature = SignatureState::Unsigned;
    assert!(!granted.grant_eq(&unsigned));
    assert!(!unsigned.grant_eq(&granted));

    let mut invalid = granted.clone();
    invalid.signature = SignatureState::Invalid;
    assert!(!granted.grant_eq(&invalid));

    let mut unknown = granted.clone();
    unknown.signature = SignatureState::Unknown;
    assert!(!granted.grant_eq(&unknown));

    let mut missing_team = granted.clone();
    missing_team.team_id = None;
    assert!(!granted.grant_eq(&missing_team));
    let mut moved_unsigned_style = missing_team.clone();
    moved_unsigned_style.image_path = "/Applications/Moved.app/Contents/MacOS/Notes".into();
    assert!(!missing_team.grant_eq(&moved_unsigned_style));
}

#[test]
fn app_identity_grant_eq_signed_windows_binds_publisher_and_exact_key() {
    let granted = windows_signed();
    let mut updated = granted.clone();
    updated.sha256 = Some([0x99; 32]);
    updated.image_path = "D:\\Notes\\Notes.exe".into();
    updated.team_id = Some("IGNORED".into());
    assert_ne!(granted, updated);
    assert!(granted.grant_eq(&updated));

    let mut other_publisher = granted.clone();
    other_publisher.publisher = Some("CN=Other".into());
    assert!(!granted.grant_eq(&other_publisher));

    let mut lower_key = granted.clone();
    lower_key.key = "c:\\program files\\notes\\notes.exe".into();
    assert_ne!(granted, lower_key);
    assert!(!granted.grant_eq(&lower_key));

    let mut missing_publisher = granted.clone();
    missing_publisher.publisher = None;
    assert!(!granted.grant_eq(&missing_publisher));
    let mut rehashed = missing_publisher.clone();
    rehashed.sha256 = Some([0x01; 32]);
    assert!(!missing_publisher.grant_eq(&rehashed));
}

#[test]
fn app_identity_grant_eq_unsigned_linux_binds_path_and_digest() {
    let granted = linux_app();
    assert!(granted.grant_eq(&linux_app()));

    let mut moved = granted.clone();
    moved.image_path = "/usr/local/bin/notes".into();
    assert!(!granted.grant_eq(&moved));

    let mut rehashed = granted.clone();
    rehashed.sha256 = Some([0x44; 32]);
    assert!(!granted.grant_eq(&rehashed));

    let mut other_desktop = granted.clone();
    other_desktop.key = "org.example.Other.desktop".into();
    assert!(!granted.grant_eq(&other_desktop));
}

#[test]
fn app_identity_hash_set_does_not_answer_grant_eq() {
    let granted = macos_signed();
    let mut updated = granted.clone();
    updated.image_path = "/Applications/Moved.app/Contents/MacOS/Notes".into();
    let mut set = HashSet::new();
    set.insert(granted.clone());
    assert!(!set.contains(&updated));
    assert!(set.iter().any(|stored| stored.grant_eq(&updated)));
}
