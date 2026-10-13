//! Authorization provenance. Entry signatures do not attest loaded DLLs/plugins.
use crate::api::WinApi;
use gcu_core::driver::{DriverError, Result};
use gcu_protocol::types::AppIdentity;
use serde::{Deserialize, Serialize};

/// OS trust-verification result, not merely presence of a certificate blob.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureState {
    /// WinVerifyTrust accepted the signature and chain.
    Valid,
    /// No embedded or supported catalog signature could be verified.
    Unsigned,
    /// Signature/chain/revocation validation failed.
    Invalid,
}

impl SignatureState {
    /// Map WinVerifyTrust's LONG result without trusting certificate presence.
    /// This pure mapping also runs in portable tests.
    pub fn from_wintrust_status(status: i32) -> Self {
        match status {
            0 => Self::Valid,
            value if value as u32 == 0x800b_0100 => Self::Unsigned,
            _ => Self::Invalid,
        }
    }
}

/// Verified signer and signed product identity. Subject strings alone are unsafe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureInfo {
    /// OS trust result.
    pub state: SignatureState,
    /// Human-readable signer subject, only populated for a valid signature.
    pub publisher: Option<String>,
    /// SHA-256 of the exact signing certificate's DER encoding.
    pub certificate_sha256: Option<String>,
    /// Signed version-resource OriginalFilename/ProductName identity.
    pub product: Option<String>,
}

/// Driver-private provenance retained alongside the portable AppIdentity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsIdentity {
    /// Canonical lower-case executable path (junctions already resolved by OS).
    pub path: String,
    /// SHA-256 of the entry executable, even if it is signed.
    pub sha256: String,
    /// Verification state, publisher, certificate and product.
    pub signature: SignatureInfo,
    /// Writable loading directory was detected. Not an attestation of all code.
    pub writable_load_path: bool,
    /// Metadata for the host's trust explanation; no application payloads.
    pub trust_note: String,
}

/// Normalize a final Win32 path. This does not resolve filesystem aliases itself.
pub fn normalize_path(path: &str) -> Result<String> {
    let path = path.replace('/', "\\");
    let path = if let Some(rest) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else {
        path.strip_prefix("\\\\?\\").unwrap_or(&path).to_owned()
    };
    let absolute_drive = path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && path.as_bytes().get(1) == Some(&b':')
        && path.as_bytes().get(2) == Some(&b'\\');
    if path.contains('\0') || !(path.starts_with("\\\\") || absolute_drive) {
        return Err(DriverError::InvalidRequest);
    }
    Ok(path.to_lowercase())
}

/// Resolve and verify a file on a native worker. No IO is performed by reauth.
pub fn identity_for_path(api: &dyn WinApi, path: &str) -> Result<WindowsIdentity> {
    let image = api.image(path)?;
    if image.sha256.len() != 64 || !image.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(DriverError::InvalidRequest);
    }
    if image.signature.state == SignatureState::Valid
        && (image
            .signature
            .publisher
            .as_ref()
            .is_none_or(String::is_empty)
            || image
                .signature
                .certificate_sha256
                .as_ref()
                .is_none_or(|hash| {
                    hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit())
                }))
    {
        return Err(DriverError::PermissionMissing {
            permission: "verified Authenticode provenance".into(),
        });
    }
    let trust_note = if image.writable_load_path {
        "Entry signature does not attest loaded code; writable DLL/plugin loading paths detected."
    } else {
        "Entry signature does not attest loaded code; DLL/plugins and writable loading paths remain a risk."
    };
    Ok(WindowsIdentity {
        path: normalize_path(&image.canonical_path)?,
        sha256: image.sha256.to_lowercase(),
        signature: image.signature,
        writable_load_path: image.writable_load_path,
        trust_note: trust_note.into(),
    })
}

impl WindowsIdentity {
    /// Portable grant key. `publisher` includes certificate/product provenance,
    /// not just the display subject. Invalid signatures cannot retain signed grants.
    /// `code_hash` always binds the executable bytes, including signed builds.
    pub fn app_identity(&self) -> AppIdentity {
        let publisher =
            if self.signature.state == SignatureState::Unsigned && !self.writable_load_path {
                None
            } else {
                // JSON is an unambiguous string representation within the existing
                // protocol field; same-subject certificates cannot collide by delimiter.
                Some(
                    serde_json::json!({
                        "state": self.signature.state,
                        "subject": self.signature.publisher,
                        "certificate_sha256": self.signature.certificate_sha256,
                        "product": self.signature.product,
                        "writable_load_path": self.writable_load_path,
                    })
                    .to_string(),
                )
            };
        AppIdentity {
            id: self.path.clone(),
            executable: Some(self.path.clone()),
            publisher,
            code_hash: Some(self.sha256.clone()),
            team_id: None,
        }
    }
}

/// Pure invalidation rule. Builds, paths, certificate/product changes and newly
/// writable loading paths invalidate grants, regardless of matching display names.
pub fn needs_reauth(old: &WindowsIdentity, new: &WindowsIdentity) -> bool {
    old.path != new.path
        || old.sha256 != new.sha256
        || old.signature != new.signature
        || old.writable_load_path != new.writable_load_path
}
