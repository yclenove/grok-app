//! Cross-platform authorization identity regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::driver::DriverError;
use gcu_driver_macos::{
    identity::{
        identity_from_facts, needs_reauth, reauth_reason, MacIdentity, SignatureState, TRUST_NOTE,
    },
    testing::{hex_byte, hex_cdhash, signed_facts, unsigned_facts},
};

fn signed() -> MacIdentity {
    identity_from_facts(signed_facts(
        "com.example.Editor",
        "/Applications/Editor.app",
    ))
    .unwrap()
}

#[test]
fn signed_identity_binds_bundle_team_and_executable_hash() {
    let identity = signed();
    let app = identity.app_identity();
    assert_eq!(app.id, "com.example.Editor");
    assert_eq!(app.team_id.as_deref(), Some("TEAMID1234"));
    assert_eq!(app.code_hash.as_deref(), Some(identity.sha256.as_str()));
    assert_eq!(
        app.executable.as_deref(),
        Some(identity.executable_path.as_str())
    );
    let publisher: serde_json::Value =
        serde_json::from_str(app.publisher.as_deref().unwrap()).unwrap();
    assert_eq!(publisher["state"], "valid");
    assert_eq!(publisher["cdhash"], identity.cdhash.as_deref().unwrap());
    assert!(identity.trust_note.contains(TRUST_NOTE));
    assert!(!needs_reauth(&identity, &identity));
}

#[test]
fn unsigned_identity_binds_path_cdhash_and_sha256() {
    let mut facts = unsigned_facts("/Applications/Tool.app");
    facts.cdhash = Some(hex_cdhash(0x44));
    facts.sha256 = hex_byte(0x55);
    let identity = identity_from_facts(facts).unwrap();
    let app = identity.app_identity();
    assert_eq!(app.id, identity.executable_path);
    assert!(app.team_id.is_none());
    assert_eq!(app.code_hash.as_deref(), Some(identity.sha256.as_str()));
    let publisher: serde_json::Value =
        serde_json::from_str(app.publisher.as_deref().unwrap()).unwrap();
    assert_eq!(publisher["state"], "unsigned");
    assert_eq!(publisher["cdhash"], identity.cdhash.as_deref().unwrap());
}

#[test]
fn ad_hoc_signature_is_stored_as_unsigned() {
    let mut facts = signed_facts("com.example.Local", "/tmp/Local.app");
    facts.signature.team_id = None;
    let identity = identity_from_facts(facts).unwrap();
    assert_eq!(identity.signature.state, SignatureState::Unsigned);
    assert!(identity.app_identity().team_id.is_none());
    assert_eq!(identity.app_identity().id, "com.example.Local");
}

#[test]
fn valid_team_without_certificate_hash_is_refused() {
    let mut facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    facts.signature.certificate_sha256 = None;
    let error = identity_from_facts(facts).unwrap_err();
    assert_eq!(
        error,
        DriverError::PermissionMissing {
            permission: "verified code signature provenance".into(),
        }
    );
}

#[test]
fn malformed_hash_is_an_invalid_request() {
    let mut facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    facts.sha256 = "zz".into();
    assert_eq!(
        identity_from_facts(facts).unwrap_err(),
        DriverError::InvalidRequest
    );
    let mut facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    facts.cdhash = Some("abc".into());
    assert_eq!(
        identity_from_facts(facts).unwrap_err(),
        DriverError::InvalidRequest
    );
}

#[test]
fn normalize_path_collapses_slashes_and_rejects_relative() {
    use gcu_driver_macos::identity::normalize_path;
    assert_eq!(
        normalize_path("//Applications//Editor.app/").unwrap(),
        "/Applications/Editor.app"
    );
    assert_eq!(
        normalize_path("Applications/Editor.app").unwrap_err(),
        DriverError::InvalidRequest
    );
    assert_eq!(
        normalize_path("/tmp/bad\0app").unwrap_err(),
        DriverError::InvalidRequest
    );
}

#[test]
fn reauth_table_covers_signature_path_certificate_and_build() {
    let old = signed();
    let mut invalid = old.clone();
    invalid.signature.state = SignatureState::Invalid;
    assert_eq!(reauth_reason(&old, &invalid), Some("signature_invalid"));

    let mut replaced = old.clone();
    replaced.bundle_path = "/Applications/Other.app".into();
    assert_eq!(reauth_reason(&old, &replaced), Some("path_replaced"));
    let mut replaced = old.clone();
    replaced.executable_path = "/Applications/Editor.app/Contents/MacOS/Other".into();
    assert_eq!(reauth_reason(&old, &replaced), Some("path_replaced"));

    let mut same_name = old.clone();
    same_name.signature.certificate_sha256 = Some(hex_byte(0x99));
    assert_eq!(old.signature.subject, same_name.signature.subject);
    assert_eq!(
        reauth_reason(&old, &same_name),
        Some("same_name_certificate")
    );
    assert_ne!(
        old.app_identity().publisher,
        same_name.app_identity().publisher
    );

    let mut certificate = old.clone();
    certificate.signature.subject = Some("Other Signer".into());
    certificate.signature.certificate_sha256 = Some(hex_byte(0x77));
    assert_eq!(
        reauth_reason(&old, &certificate),
        Some("certificate_changed")
    );

    let mut product = old.clone();
    product.signature.product = Some("Renamed".into());
    assert_eq!(reauth_reason(&old, &product), Some("product_changed"));

    let mut build = old.clone();
    build.signature.build = Some("2".into());
    assert_eq!(reauth_reason(&old, &build), Some("build_changed"));
    let mut bytes = old.clone();
    bytes.sha256 = hex_byte(0x66);
    assert_eq!(reauth_reason(&old, &bytes), Some("build_changed"));
    let mut directory = old.clone();
    directory.cdhash = Some(hex_cdhash(0x67));
    assert_eq!(reauth_reason(&old, &directory), Some("build_changed"));

    let mut bundle = old.clone();
    bundle.bundle_id = Some("com.example.Other".into());
    assert_eq!(reauth_reason(&old, &bundle), Some("bundle_id_changed"));
    let mut team = old.clone();
    team.signature.team_id = Some("OTHERRTEAM".into());
    assert_eq!(reauth_reason(&old, &team), Some("team_id_changed"));
}

#[test]
fn writable_load_path_is_in_the_trust_note_and_requires_reauth() {
    let old = signed();
    let mut writable = old.clone();
    writable.writable_load_path = true;
    writable.trust_note = format!("{TRUST_NOTE} A writable load path was detected.");
    assert_eq!(reauth_reason(&old, &writable), Some("writable_load_path"));
    let facts_note = {
        let mut facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
        facts.writable_load_path = true;
        identity_from_facts(facts).unwrap()
    };
    assert!(facts_note.trust_note.contains(TRUST_NOTE));
    assert!(facts_note.trust_note.contains("writable load path"));
}

#[test]
fn display_name_alone_does_not_require_reauth() {
    let mut facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    facts.localized_name = "访达".into();
    let renamed = identity_from_facts(facts).unwrap();
    assert!(!needs_reauth(&signed(), &renamed));
    assert_eq!(signed().app_identity(), renamed.app_identity());
}
