//! OS image captured from a real approval request.
use crate::messages::{App, Identity};

use super::text::blank_to_none;
use super::{hex_decode_32, hex_encode, ImageBinding};

impl ImageBinding {
    /// Capture the OS image. Blank team and publisher strings are absent.
    pub fn from_app(app: &App) -> Self {
        Self {
            platform: app.identity.platform,
            key: app.identity.key.clone(),
            team_id: blank_to_none(app.identity.team_id.clone()),
            publisher: blank_to_none(app.identity.publisher.clone()),
            sha256_hex: app.identity.sha256.as_ref().map(|bytes| hex_encode(bytes)),
            signature: app.identity.signature,
            image_path: app.identity.image_path.clone(),
            pid: app.pid,
            proof: app.identity.proof.clone(),
        }
    }

    pub(crate) fn to_identity(&self) -> Identity {
        Identity {
            platform: self.platform,
            key: self.key.clone(),
            team_id: self.team_id.clone(),
            publisher: self.publisher.clone(),
            sha256: self.sha256_hex.as_deref().and_then(hex_decode_32),
            signature: self.signature,
            image_path: self.image_path.clone(),
            proof: self.proof.clone(),
        }
    }
}
