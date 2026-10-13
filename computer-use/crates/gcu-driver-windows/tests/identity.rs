//! Cross-platform authorization identity regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_driver_windows::{
    identity::{identity_for_path, needs_reauth, SignatureState},
    testing::{signed_image, FakeWinApi},
};

#[test]
fn identity_junction_alias_binds_canonical_image() {
    let api = FakeWinApi::default();
    api.image(
        "C:\\alias\\Editor.exe",
        signed_image("C:\\Program Files\\Editor\\Editor.exe"),
    );
    let id = identity_for_path(&api, "C:\\alias\\Editor.exe").unwrap();
    assert_eq!(id.path, "c:\\program files\\editor\\editor.exe");
    assert_eq!(
        id.app_identity().code_hash.as_deref(),
        Some(id.sha256.as_str())
    );
}

#[test]
fn authorization_identity_changes_require_reauthorization() {
    let api = FakeWinApi::default();
    api.image("editor", signed_image("C:\\Editor.exe"));
    let old = identity_for_path(&api, "editor").unwrap();
    let mut cases = Vec::new();
    let mut changed = old.clone();
    changed.path = "c:\\other.exe".into();
    cases.push(changed);
    let mut changed = old.clone();
    changed.sha256 = "b".repeat(64);
    cases.push(changed);
    let mut changed = old.clone();
    changed.signature.certificate_sha256 = Some("b".repeat(64));
    cases.push(changed);
    let mut changed = old.clone();
    changed.signature.product = Some("Different product".into());
    cases.push(changed);
    let mut changed = old.clone();
    changed.signature.state = SignatureState::Invalid;
    cases.push(changed);
    let mut changed = old.clone();
    changed.signature.publisher = Some("Different publisher".into());
    cases.push(changed);
    for new in cases {
        assert!(needs_reauth(&old, &new));
        assert_ne!(old.app_identity(), new.app_identity());
    }
    assert!(!needs_reauth(&old, &old));
}

#[test]
fn identity_unsigned_replacement_requires_reauthorization() {
    let api = FakeWinApi::default();
    let mut image = signed_image("C:\\Editor.exe");
    image.signature.state = SignatureState::Unsigned;
    image.signature.publisher = None;
    image.signature.certificate_sha256 = None;
    api.image("editor", image.clone());
    let old = identity_for_path(&api, "editor").unwrap();
    image.sha256 = "c".repeat(64);
    api.image("editor", image);
    let new = identity_for_path(&api, "editor").unwrap();
    assert!(needs_reauth(&old, &new));
    assert_eq!(old.app_identity().publisher, None);
}

#[test]
fn identity_same_subject_different_certificate_invalidates_grant() {
    let api = FakeWinApi::default();
    api.image("editor", signed_image("C:\\Editor.exe"));
    let old = identity_for_path(&api, "editor").unwrap();
    let mut new = old.clone();
    new.signature.certificate_sha256 = Some("d".repeat(64));
    assert_eq!(old.signature.publisher, new.signature.publisher);
    assert!(needs_reauth(&old, &new));
    assert_ne!(old.app_identity().publisher, new.app_identity().publisher);
}

#[test]
fn identity_writable_dll_path_reports_trust_boundary() {
    let api = FakeWinApi::default();
    let mut image = signed_image("C:\\Editor.exe");
    api.image("editor", image.clone());
    let old = identity_for_path(&api, "editor").unwrap();
    image.writable_load_path = true;
    api.image("editor", image);
    let new = identity_for_path(&api, "editor").unwrap();
    assert_eq!(old.sha256, new.sha256);
    assert!(new.trust_note.contains("writable"));
    assert!(new.trust_note.contains("DLL"));
    assert!(needs_reauth(&old, &new));
    assert_ne!(old.app_identity(), new.app_identity());
}

#[test]
fn signature_status_valid_unsigned_bad_chain_expired_revoked_are_distinct() {
    for (raw, expected) in [
        (0u32, SignatureState::Valid),
        (0x800b0100, SignatureState::Unsigned),
        (0x80096010, SignatureState::Invalid),
        (0x800b0101, SignatureState::Invalid),
        (0x800b010c, SignatureState::Invalid),
        (0xffffffff, SignatureState::Invalid),
    ] {
        assert_eq!(SignatureState::from_wintrust_status(raw as i32), expected);
    }
}
#[test]
fn canonical_path_normalizes_extended_unc_and_rejects_drive_relative() {
    use gcu_driver_windows::identity::normalize_path;
    assert_eq!(
        normalize_path("\\\\?\\C:\\Editor.exe").unwrap(),
        "c:\\editor.exe"
    );
    assert_eq!(
        normalize_path("\\\\?\\UNC\\Server\\Share\\Editor.exe").unwrap(),
        "\\\\server\\share\\editor.exe"
    );
    for path in ["C:editor.exe", "editor.exe", "C:\\file\0.exe"] {
        assert!(normalize_path(path).is_err());
    }
}
#[test]
fn signature_valid_without_verified_certificate_provenance_refused() {
    let api = FakeWinApi::default();
    let mut image = signed_image("C:\\Editor.exe");
    image.signature.certificate_sha256 = None;
    api.image("editor", image);
    assert!(matches!(
        identity_for_path(&api, "editor"),
        Err(gcu_core::driver::DriverError::PermissionMissing { .. })
    ));
}
