//! Structural equality, hashing, and architecture §7.2 grant matching.

use gcu_host_api::{AppIdentity, GrantProof, Platform, SignatureState};
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
        proof: GrantProof {
            cdhash: Some("0123456789abcdef0123456789abcdef01234567".into()),
            certificate_sha256: Some("22".repeat(32)),
            product: Some("Notes".into()),
            build: Some("100".into()),
            ..GrantProof::default()
        },
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
        proof: GrantProof {
            certificate_sha256: Some("33".repeat(32)),
            product: Some("Notes".into()),
            ..GrantProof::default()
        },
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
        proof: GrantProof {
            file_dev: Some(1),
            file_inode: Some(2),
            ..GrantProof::default()
        },
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
        (
            "cdhash",
            AppIdentity {
                proof: GrantProof {
                    cdhash: Some("ff".repeat(20)),
                    ..base.proof.clone()
                },
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

// The old test name said a signed macOS grant ignores path and digest.
// §7.2 forbids that wide match, so a path, hash, cdhash, or certificate change
// asks again. A legacy row that omits the proof does not match, even itself.
#[test]
fn app_identity_grant_eq_signed_macos_binds_path_hash_cdhash_and_certificate() {
    let granted = macos_signed();
    assert!(granted.grant_eq(&macos_signed()));

    let mut updated = granted.clone();
    updated.image_path = "/Applications/Moved.app/Contents/MacOS/Notes".into();
    updated.sha256 = Some([0x55; 32]);
    assert!(!granted.grant_eq(&updated));

    let mut other_cdhash = granted.clone();
    other_cdhash.proof.cdhash = Some("ff".repeat(20));
    assert_eq!(granted.team_id, other_cdhash.team_id);
    assert!(!granted.grant_eq(&other_cdhash));

    let mut other_cert = granted.clone();
    other_cert.proof.certificate_sha256 = Some("99".repeat(32));
    assert_eq!(granted.publisher, other_cert.publisher);
    assert!(!granted.grant_eq(&other_cert));

    let mut legacy = granted.clone();
    legacy.proof = GrantProof::default();
    assert!(!legacy.grant_eq(&legacy));
    assert!(!granted.grant_eq(&legacy));

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
}

// The old test allowed a new path and digest when the publisher subject matched.
// §7.2 requires the path, content hash, and certificate as well.
#[test]
fn app_identity_grant_eq_signed_windows_binds_path_hash_publisher_and_certificate() {
    let granted = windows_signed();
    assert!(granted.grant_eq(&windows_signed()));

    let mut updated = granted.clone();
    updated.sha256 = Some([0x99; 32]);
    updated.image_path = "D:\\Notes\\Notes.exe".into();
    updated.key = updated.image_path.clone();
    assert!(!granted.grant_eq(&updated));

    let mut other_publisher = granted.clone();
    other_publisher.publisher = Some("CN=Other".into());
    assert!(!granted.grant_eq(&other_publisher));

    let mut other_cert = granted.clone();
    other_cert.proof.certificate_sha256 = Some("99".repeat(32));
    assert_eq!(granted.publisher, other_cert.publisher);
    assert!(!granted.grant_eq(&other_cert));

    let mut lower_key = granted.clone();
    lower_key.key = "c:\\program files\\notes\\notes.exe".into();
    lower_key.image_path = lower_key.key.clone();
    assert_ne!(granted, lower_key);
    assert!(!granted.grant_eq(&lower_key));

    let mut missing_publisher = granted.clone();
    missing_publisher.publisher = None;
    assert!(!granted.grant_eq(&missing_publisher));
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

    let mut missing_inode = granted.clone();
    missing_inode.proof.file_inode = None;
    assert!(!granted.grant_eq(&missing_inode));
    assert!(!missing_inode.grant_eq(&missing_inode));
}

#[test]
fn app_identity_hash_set_does_not_answer_grant_eq() {
    let granted = macos_signed();
    let mut moved = granted.clone();
    moved.image_path = "/Applications/Moved.app/Contents/MacOS/Notes".into();
    let mut set = HashSet::new();
    set.insert(granted.clone());
    assert!(!set.contains(&moved));
    assert!(!set.iter().any(|stored| stored.grant_eq(&moved)));

    let mut folded = granted.clone();
    folded.proof.cdhash = granted.proof.cdhash.as_deref().map(str::to_ascii_uppercase);
    assert_ne!(granted, folded);
    assert!(!set.contains(&folded));
    assert!(granted.grant_eq(&folded));
}
