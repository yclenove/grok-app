//! Authorization provenance for a macOS bundle.
//!
//! A signature covers the bundle's sealed code directory. It does not prove that
//! every byte running in the process is trusted: plugins and writable load paths
//! can still change that code. [`MacIdentity::trust_note`] says so on every grant.
//!
//! Bundle id is the primary key, with team id beside it. An unsigned or ad-hoc
//! app binds the canonical executable path plus SHA-256 and cdhash. Reauthorization
//! is a pure comparison; it does not touch the filesystem.

use crate::api::BundleFacts;
use gcu_core::driver::{DriverError, Result};
use gcu_protocol::types::AppIdentity;
use serde::{Deserialize, Serialize};

/// What the OS reported about the bundle signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureState {
    /// `SecStaticCodeCheckValidity` succeeded and a team id was present.
    Valid,
    /// No usable signature, or an ad-hoc signature without a team id.
    Unsigned,
    /// The signature or its requirement check failed.
    Invalid,
}

/// Signer and product facts. A subject string alone never identifies a grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureInfo {
    /// OS trust result before ad-hoc downgrade.
    pub state: SignatureState,
    /// Signing team, when the OS reported one.
    pub team_id: Option<String>,
    /// Signing identifier (`kSecCodeInfoIdentifier`).
    pub signing_id: Option<String>,
    /// Certificate subject summary. Empty subjects are not names.
    pub subject: Option<String>,
    /// SHA-256 of the leaf certificate DER.
    pub certificate_sha256: Option<String>,
    /// Product name from the bundle.
    pub product: Option<String>,
    /// `CFBundleVersion`.
    pub build: Option<String>,
}

/// Driver-private provenance stored next to the portable [`AppIdentity`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacIdentity {
    /// Bundle identifier. Absent for unpackaged tools.
    pub bundle_id: Option<String>,
    /// Canonical bundle path. Case is preserved.
    pub bundle_path: String,
    /// Canonical executable path.
    pub executable_path: String,
    /// Code-directory hash, lowercase hex.
    pub cdhash: Option<String>,
    /// SHA-256 of the executable, lowercase hex. Always bound, signed or not.
    pub sha256: String,
    /// Signature facts after ad-hoc downgrade.
    pub signature: SignatureInfo,
    /// Writable PlugIns, Frameworks, or executable directory.
    pub writable_load_path: bool,
    /// Host-facing explanation of the trust boundary.
    pub trust_note: String,
}

/// Entry signature does not cover plugins or injected code.
pub const TRUST_NOTE: &str = "A code signature does not prove that every byte running inside the process is trusted. Plugins and writable load paths can still change that code.";

const WRITABLE_NOTE: &str = " A writable load path was detected.";

/// Collapse duplicate slashes and a trailing slash. Case is preserved.
/// Relative paths and NUL are rejected. This does not resolve symlinks.
pub fn normalize_path(path: &str) -> Result<String> {
    if path.contains('\0') || !path.starts_with('/') {
        return Err(DriverError::InvalidRequest);
    }
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        if ch == '/' && out.ends_with('/') {
            continue;
        }
        out.push(ch);
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    if out.is_empty() {
        return Err(DriverError::InvalidRequest);
    }
    Ok(out)
}

/// Build a grant from OS facts. No IO.
///
/// A valid signature with a team id but no certificate fingerprint is refused:
/// the provenance is incomplete. A valid signature without a team id is ad-hoc
/// and is stored as unsigned so the grant binds path and hashes instead.
pub fn identity_from_facts(facts: BundleFacts) -> Result<MacIdentity> {
    let sha256 = require_hex(&facts.sha256, &[64])?;
    let cdhash = match facts.cdhash.as_deref() {
        None => None,
        Some(value) => Some(require_hex(value, &[40, 64])?),
    };
    let mut signature = facts.signature;
    signature.team_id = nonempty(signature.team_id);
    signature.subject = nonempty(signature.subject);
    signature.signing_id = nonempty(signature.signing_id);
    signature.product = nonempty(signature.product);
    signature.build = nonempty(signature.build);
    signature.certificate_sha256 = match signature.certificate_sha256.as_deref() {
        None => None,
        Some(value) => Some(require_hex(value, &[64])?),
    };
    let team = signature.team_id.is_some();
    if signature.state == SignatureState::Valid && team && signature.certificate_sha256.is_none() {
        return Err(DriverError::PermissionMissing {
            permission: "verified code signature provenance".into(),
        });
    }
    if signature.state == SignatureState::Valid && !team {
        signature.state = SignatureState::Unsigned;
        signature.team_id = None;
    }
    let bundle_path = normalize_path(&facts.bundle_path)?;
    let executable_path = normalize_path(&facts.executable_path)?;
    let trust_note = if facts.writable_load_path {
        format!("{TRUST_NOTE}{WRITABLE_NOTE}")
    } else {
        TRUST_NOTE.to_owned()
    };
    Ok(MacIdentity {
        bundle_id: nonempty(facts.bundle_id),
        bundle_path,
        executable_path,
        cdhash,
        sha256,
        signature,
        writable_load_path: facts.writable_load_path,
        trust_note,
    })
}

impl MacIdentity {
    /// Portable grant. `publisher` is JSON provenance so two certificates that
    /// share a subject cannot collide, and so signature state is part of the grant.
    /// `code_hash` is always the executable SHA-256. `team_id` is set only for a
    /// valid team-signed bundle. The id is the bundle id, or the executable path
    /// when the bundle id is missing.
    pub fn app_identity(&self) -> AppIdentity {
        let team_id = match self.signature.state {
            SignatureState::Valid => self.signature.team_id.clone(),
            SignatureState::Unsigned | SignatureState::Invalid => None,
        };
        let publisher = serde_json::json!({
            "state": self.signature.state,
            "team_id": self.signature.team_id,
            "subject": self.signature.subject,
            "certificate_sha256": self.signature.certificate_sha256,
            "signing_id": self.signature.signing_id,
            "product": self.signature.product,
            "build": self.signature.build,
            "cdhash": self.cdhash,
            "writable_load_path": self.writable_load_path,
        })
        .to_string();
        AppIdentity {
            id: self
                .bundle_id
                .clone()
                .unwrap_or_else(|| self.executable_path.clone()),
            executable: Some(self.executable_path.clone()),
            team_id,
            publisher: Some(publisher),
            code_hash: Some(self.sha256.clone()),
        }
    }
}

/// Why a stored grant no longer matches a fresh read.
///
/// Display-name changes are not a reason: the name is not part of the identity.
/// The first matching row wins.
pub fn reauth_reason(old: &MacIdentity, new: &MacIdentity) -> Option<&'static str> {
    if signature_invalid(old, new) {
        return Some("signature_invalid");
    }
    if old.bundle_path != new.bundle_path || old.executable_path != new.executable_path {
        return Some("path_replaced");
    }
    if same_name_certificate(old, new) {
        return Some("same_name_certificate");
    }
    if old.signature.certificate_sha256 != new.signature.certificate_sha256 {
        return Some("certificate_changed");
    }
    if old.signature.product != new.signature.product {
        return Some("product_changed");
    }
    if old.signature.build != new.signature.build
        || old.sha256 != new.sha256
        || old.cdhash != new.cdhash
    {
        return Some("build_changed");
    }
    if old.bundle_id != new.bundle_id {
        return Some("bundle_id_changed");
    }
    if old.signature.team_id != new.signature.team_id {
        return Some("team_id_changed");
    }
    if old.writable_load_path != new.writable_load_path {
        return Some("writable_load_path");
    }
    None
}

/// Table-driven invalidation. Identical provenance does not require reauth.
pub fn needs_reauth(old: &MacIdentity, new: &MacIdentity) -> bool {
    reauth_reason(old, new).is_some()
}

fn signature_invalid(old: &MacIdentity, new: &MacIdentity) -> bool {
    let previous = old.signature.state;
    let current = new.signature.state;
    (previous == SignatureState::Valid && current != SignatureState::Valid)
        || (current == SignatureState::Invalid && previous != SignatureState::Invalid)
        || (previous == SignatureState::Invalid && current == SignatureState::Valid)
}

fn same_name_certificate(old: &MacIdentity, new: &MacIdentity) -> bool {
    match (
        old.signature.subject.as_deref(),
        new.signature.subject.as_deref(),
        old.signature.certificate_sha256.as_deref(),
        new.signature.certificate_sha256.as_deref(),
    ) {
        (Some(left), Some(right), Some(old_hash), Some(new_hash)) => {
            !left.is_empty() && left == right && old_hash != new_hash
        }
        _ => false,
    }
}

fn require_hex(value: &str, lengths: &[usize]) -> Result<String> {
    let lower = value.to_ascii_lowercase();
    if lengths.contains(&lower.len()) && lower.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(lower)
    } else {
        Err(DriverError::InvalidRequest)
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}
