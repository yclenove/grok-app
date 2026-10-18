//! Adapter from [`crate::AppIdentity`] to the shared §7.2 comparison.
//!
//! The match itself is [`gcu_protocol::grant::persistent_eq`]. Route B calls
//! that function too. This module only borrows the fields.

use gcu_protocol::grant::{GrantFacts, GrantPlatform, GrantSignature};

use crate::{AppIdentity, Platform, SignatureState};

pub(crate) fn persistent_eq(granted: &AppIdentity, current: &AppIdentity) -> bool {
    gcu_protocol::grant::persistent_eq(facts(granted), facts(current))
}

fn facts(id: &AppIdentity) -> GrantFacts<'_> {
    GrantFacts {
        platform: match id.platform {
            Platform::Macos => GrantPlatform::Macos,
            Platform::Windows => GrantPlatform::Windows,
            Platform::Linux => GrantPlatform::Linux,
        },
        signature: match id.signature {
            SignatureState::Valid => GrantSignature::Valid,
            SignatureState::Invalid => GrantSignature::Invalid,
            SignatureState::Unsigned => GrantSignature::Unsigned,
            SignatureState::Unknown => GrantSignature::Unknown,
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
