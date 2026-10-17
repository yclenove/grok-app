//! The allowlist is derived only from the extension manifest compiled into gcu.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest, Sha256};

use super::Error;

/// Immutable Chrome/Edge extension identity derived from SHA-256 of its public key.
#[derive(Clone, Debug)]
pub struct ExtensionIdentity(String);

impl ExtensionIdentity {
    /// Read a manifest's base64 public `key`; missing/placeholder keys fail closed.
    /// Pure and bounded; no IO, blocking, timeout or cancellation.
    pub fn from_manifest(manifest: &str) -> Result<Self, Error> {
        if manifest.len() > 32_768 {
            return Err(Error::Identity);
        }
        let value: serde_json::Value =
            serde_json::from_str(manifest).map_err(|_| Error::Identity)?;
        let key = value
            .get("key")
            .and_then(|k| k.as_str())
            .ok_or(Error::Identity)?;
        let bytes = STANDARD.decode(key).map_err(|_| Error::Identity)?;
        if bytes.is_empty() || bytes.len() > 8192 {
            return Err(Error::Identity);
        }
        let digest = Sha256::digest(bytes);
        let id = digest[..16]
            .iter()
            .flat_map(|b| [char::from(b'a' + (b >> 4)), char::from(b'a' + (b & 15))])
            .collect();
        Ok(Self(id))
    }

    /// Production identity. No environment, argv or incoming message can override it.
    pub fn embedded() -> Result<Self, Error> {
        Self::from_manifest(include_str!(
            "../../../../tools/browser-extension/manifest.json"
        ))
    }

    /// Chrome's fixed 32-character ID (letters a–p).
    pub fn id(&self) -> &str {
        &self.0
    }

    /// The only allowed native messaging origin, including the final slash.
    pub fn origin(&self) -> String {
        format!("chrome-extension://{}/", self.0)
    }
}
