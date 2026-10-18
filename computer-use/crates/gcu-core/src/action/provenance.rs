//! Lift driver provenance out of the protocol publisher string.
//!
//! macOS and Windows drivers store signature state, certificate, product, and
//! cdhash in that string. Linux stores device and inode. A string that is not
//! that object stays `Unknown`. This never invents `Valid` from a bare team or
//! subject. Empty strings are missing facts.

use gcu_host_api::{AppIdentity, SignatureState};
use serde_json::{Map, Value};

/// Copy driver facts onto `identity` when `publisher` is driver JSON.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: `&mut`.
pub(crate) fn apply_driver_provenance(identity: &mut AppIdentity) {
    let Some(raw) = identity.publisher.clone() else {
        return;
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    if object.contains_key("state") {
        apply_signed(&object, identity);
        return;
    }
    apply_linux_file(&object, identity);
}

fn apply_signed(object: &Map<String, Value>, identity: &mut AppIdentity) {
    let Some(signature) = object
        .get("state")
        .and_then(Value::as_str)
        .and_then(signature_from)
    else {
        return;
    };
    identity.signature = signature;
    identity.proof.cdhash = text(object, "cdhash");
    identity.proof.certificate_sha256 = text(object, "certificate_sha256");
    identity.proof.product = text(object, "product");
    identity.proof.build = text(object, "build");
    if identity
        .team_id
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .is_none()
    {
        identity.team_id = text(object, "team_id");
    }
    identity.publisher = text(object, "subject");
}

fn apply_linux_file(object: &Map<String, Value>, identity: &mut AppIdentity) {
    let (Some(dev), Some(inode)) = (u64_field(object, "dev"), u64_field(object, "inode")) else {
        return;
    };
    identity.proof.file_dev = Some(dev);
    identity.proof.file_inode = Some(inode);
    identity.publisher = None;
    if identity.signature == SignatureState::Unknown {
        identity.signature = SignatureState::Unsigned;
    }
}

fn signature_from(state: &str) -> Option<SignatureState> {
    match state {
        "valid" | "Valid" => Some(SignatureState::Valid),
        "unsigned" | "Unsigned" => Some(SignatureState::Unsigned),
        "invalid" | "Invalid" => Some(SignatureState::Invalid),
        _ => None,
    }
}

fn text(object: &Map<String, Value>, key: &str) -> Option<String> {
    let trimmed = object.get(key)?.as_str()?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn u64_field(object: &Map<String, Value>, key: &str) -> Option<u64> {
    object.get(key)?.as_u64()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_host_api::{GrantProof, Platform};

    fn base() -> AppIdentity {
        AppIdentity {
            platform: Platform::Macos,
            key: "com.example.notes".into(),
            team_id: None,
            publisher: None,
            sha256: Some([0x11; 32]),
            signature: SignatureState::Unknown,
            image_path: "/Applications/Notes.app/Contents/MacOS/Notes".into(),
            proof: GrantProof::default(),
        }
    }

    fn cert() -> String {
        "22".repeat(32)
    }

    #[test]
    fn macos_json_sets_valid_and_drops_the_blob() {
        let mut identity = base();
        identity.publisher = Some(
            serde_json::json!({
                "state": "valid",
                "team_id": "TEAM123456",
                "subject": " Example ",
                "certificate_sha256": cert(),
                "product": "Notes",
                "build": "100",
                "cdhash": "0123456789abcdef0123456789abcdef01234567",
            })
            .to_string(),
        );
        apply_driver_provenance(&mut identity);
        assert_eq!(identity.signature, SignatureState::Valid);
        assert_eq!(identity.team_id.as_deref(), Some("TEAM123456"));
        assert_eq!(identity.publisher.as_deref(), Some("Example"));
        assert!(identity.grant_eq(&identity));
    }

    #[test]
    fn macos_same_team_cdhash_change_does_not_match() {
        let mut granted = base();
        let mut updated = base();
        for (identity, cdhash) in [
            (&mut granted, "0123456789abcdef0123456789abcdef01234567"),
            (&mut updated, "ffffffffffffffffffffffffffffffffffffffff"),
        ] {
            identity.publisher = Some(
                serde_json::json!({
                    "state": "valid",
                    "team_id": "TEAM123456",
                    "subject": "Example",
                    "certificate_sha256": cert(),
                    "product": "Notes",
                    "build": "100",
                    "cdhash": cdhash,
                })
                .to_string(),
            );
            apply_driver_provenance(identity);
        }
        assert_eq!(granted.team_id, updated.team_id);
        assert!(!granted.grant_eq(&updated));
    }

    #[test]
    fn windows_valid_uses_subject_and_a_new_path_does_not_match() {
        let mut granted = base();
        granted.platform = Platform::Windows;
        granted.key = "C:\\Notes\\Notes.exe".into();
        granted.image_path = granted.key.clone();
        granted.publisher = Some(
            serde_json::json!({
                "state": "Valid",
                "subject": "CN=Example",
                "certificate_sha256": cert(),
                "product": "Notes",
            })
            .to_string(),
        );
        apply_driver_provenance(&mut granted);
        let mut moved = granted.clone();
        moved.key = "D:\\Notes\\Notes.exe".into();
        moved.image_path = moved.key.clone();
        assert_eq!(granted.publisher.as_deref(), Some("CN=Example"));
        assert_eq!(granted.signature, SignatureState::Valid);
        assert!(granted.grant_eq(&granted));
        assert!(!granted.grant_eq(&moved));
    }

    #[test]
    fn linux_dev_inode_json_becomes_unsigned_without_a_publisher_blob() {
        let mut identity = base();
        identity.platform = Platform::Linux;
        identity.key = "/usr/bin/notes".into();
        identity.image_path = identity.key.clone();
        identity.publisher = Some(serde_json::json!({"dev": 1, "inode": 2}).to_string());
        apply_driver_provenance(&mut identity);
        assert_eq!(identity.signature, SignatureState::Unsigned);
        assert!(identity.publisher.is_none());
        assert_eq!(identity.proof.file_dev, Some(1));
        assert_eq!(identity.proof.file_inode, Some(2));
        assert!(identity.grant_eq(&identity));
        let mut rehashed = identity.clone();
        rehashed.sha256 = Some([0x99; 32]);
        assert!(!identity.grant_eq(&rehashed));
    }

    #[test]
    fn bare_team_or_bad_state_stays_unknown() {
        let mut bare = base();
        bare.publisher = Some("TEAM123456".into());
        bare.team_id = Some("TEAM123456".into());
        apply_driver_provenance(&mut bare);
        assert_eq!(bare.signature, SignatureState::Unknown);
        assert!(!bare.grant_eq(&bare));

        let mut bad = base();
        bad.publisher = Some(
            serde_json::json!({
                "state": "nope",
                "cdhash": "0123456789abcdef0123456789abcdef01234567",
            })
            .to_string(),
        );
        apply_driver_provenance(&mut bad);
        assert_eq!(bad.signature, SignatureState::Unknown);
        assert!(bad.proof.cdhash.is_none());
    }

    #[test]
    fn present_team_is_not_replaced_by_json() {
        let mut identity = base();
        identity.team_id = Some("KEEPTEAM1".into());
        identity.publisher = Some(
            serde_json::json!({
                "state": "valid",
                "team_id": "OTHERRTEAM",
                "subject": "Example",
                "certificate_sha256": cert(),
                "product": "Notes",
                "build": "100",
                "cdhash": "0123456789abcdef0123456789abcdef01234567",
            })
            .to_string(),
        );
        apply_driver_provenance(&mut identity);
        assert_eq!(identity.team_id.as_deref(), Some("KEEPTEAM1"));
    }
}
