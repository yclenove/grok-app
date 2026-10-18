//! Route B calls the same §7.2 comparison as route A.
//!
//! [`persistent_grant_matches`] borrows the wire identity and calls
//! [`gcu_protocol::grant::persistent_eq`]. `gcu-host-api` calls that function
//! from `AppIdentity::grant_eq`. This crate does not depend on host-api.
use gcu_protocol::grant::{GrantFacts, GrantPlatform, GrantSignature};

use crate::messages::{Identity, Platform, Signature};

/// Whether a stored Host identity still covers `current`.
///
/// A legacy record with an empty proof does not match. Blocking: none.
/// Timeout: none. Cancellation: none.
#[must_use]
pub fn persistent_grant_matches(saved: &Identity, current: &Identity) -> bool {
    gcu_protocol::grant::persistent_eq(facts(saved), facts(current))
}

fn facts(id: &Identity) -> GrantFacts<'_> {
    GrantFacts {
        platform: match id.platform {
            Platform::Macos => GrantPlatform::Macos,
            Platform::Windows => GrantPlatform::Windows,
            Platform::Linux => GrantPlatform::Linux,
        },
        signature: match id.signature {
            Signature::Valid => GrantSignature::Valid,
            Signature::Invalid => GrantSignature::Invalid,
            Signature::Unsigned => GrantSignature::Unsigned,
            Signature::Unknown => GrantSignature::Unknown,
        },
        key: &id.key,
        image_path: &id.image_path,
        sha256: id.sha256,
        team_id: id.team_id.as_deref(),
        publisher: id.publisher.as_deref(),
        cdhash: id.proof.cdhash.as_deref(),
        certificate_sha256: id.proof.certificate_sha256.as_deref(),
        product: id.proof.product.as_deref(),
        build: id.proof.build.as_deref(),
        file_dev: id.proof.file_dev,
        file_inode: id.proof.file_inode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::GrantProof;

    fn hex(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    fn mac() -> Identity {
        Identity {
            platform: Platform::Macos,
            key: "com.example.notes".into(),
            team_id: Some("TEAM123456".into()),
            publisher: Some("Example".into()),
            sha256: Some([0x11; 32]),
            signature: Signature::Valid,
            image_path: "/Applications/Notes.app/Contents/MacOS/Notes".into(),
            proof: GrantProof {
                cdhash: Some("0123456789abcdef0123456789abcdef01234567".into()),
                certificate_sha256: Some(hex(0x22)),
                product: Some("Notes".into()),
                build: Some("100".into()),
                ..GrantProof::default()
            },
        }
    }

    #[test]
    fn wire_round_trip_uses_the_same_grant_eq() {
        let granted = mac();
        let mut updated = granted.clone();
        updated.proof.cdhash = Some("ffffffffffffffffffffffffffffffffffffffff".into());
        assert!(persistent_grant_matches(&granted, &granted));
        assert!(!persistent_grant_matches(&granted, &updated));
        assert_eq!(
            gcu_protocol::grant::persistent_eq(facts(&granted), facts(&updated)),
            persistent_grant_matches(&granted, &updated)
        );
        let legacy = Identity {
            proof: GrantProof::default(),
            ..granted.clone()
        };
        assert!(!persistent_grant_matches(&legacy, &granted));
        assert!(!persistent_grant_matches(&legacy, &legacy));
    }

    #[test]
    fn windows_same_publisher_new_path_or_hash_does_not_match() {
        let granted = Identity {
            platform: Platform::Windows,
            key: r"C:\Notes\Notes.exe".into(),
            team_id: None,
            publisher: Some("CN=Example".into()),
            sha256: Some([0x22; 32]),
            signature: Signature::Valid,
            image_path: r"C:\Notes\Notes.exe".into(),
            proof: GrantProof {
                certificate_sha256: Some(hex(0x33)),
                product: Some("Notes".into()),
                ..GrantProof::default()
            },
        };
        assert!(persistent_grant_matches(&granted, &granted));
        let mut moved = granted.clone();
        moved.image_path = r"D:\Notes\Notes.exe".into();
        moved.key = moved.image_path.clone();
        moved.sha256 = Some([0x66; 32]);
        assert!(!persistent_grant_matches(&granted, &moved));
        assert_eq!(granted.publisher, moved.publisher);
    }

    #[test]
    fn linux_same_path_new_hash_does_not_match() {
        let granted = Identity {
            platform: Platform::Linux,
            key: "/usr/bin/notes".into(),
            team_id: None,
            publisher: None,
            sha256: Some([0x44; 32]),
            signature: Signature::Unsigned,
            image_path: "/usr/bin/notes".into(),
            proof: GrantProof {
                file_dev: Some(1),
                file_inode: Some(2),
                ..GrantProof::default()
            },
        };
        assert!(persistent_grant_matches(&granted, &granted));
        let mut rehashed = granted.clone();
        rehashed.sha256 = Some([0x99; 32]);
        assert!(!persistent_grant_matches(&granted, &rehashed));
        assert_eq!(granted.image_path, rehashed.image_path);
    }
}
