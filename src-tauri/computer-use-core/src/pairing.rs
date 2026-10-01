//! Existing-tab extension pairing. Origin / extension-id strings are not identity.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const PAIRING_TTL_MS: u64 = 5 * 60 * 1000;
pub const EXTENSION_ID: &str = "bgegbabkegkanjbmjbeaockdijnbkjgi";
pub const PAIRING_PROTOCOL: u32 = 1;
pub const CONNECTION_LEASE: Duration = Duration::from_secs(30);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct PairingChallenge {
    pub nonce: String,
    pub instance_id: String,
    /// One-time proof material shown only in the App. It is never returned by
    /// the loopback pairing endpoints or placed in the browser URL.
    pub verification_code: String,
    pub installed_extension_id: String,
    pub expires_at_ms: u64,
}

impl std::fmt::Debug for PairingChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingChallenge")
            .field("nonce", &self.nonce)
            .field("instance_id", &self.instance_id)
            .field("verification_code", &"[redacted]")
            .field("installed_extension_id", &self.installed_extension_id)
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PairingProof {
    pub protocol: u32,
    pub nonce: String,
    pub instance: String,
    pub ext: String,
    pub expires_at: u64,
    pub connection_nonce: String,
    pub response: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSession {
    pub session_key: String,
    pub instance_id: String,
    pub connection_nonce: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PairingConnection {
    pub instance_id: String,
    pub connection_nonce: String,
    pub generation: u64,
}

impl PairingChallenge {
    pub fn expired(&self) -> bool {
        now_ms() >= self.expires_at_ms
    }

    pub fn public_view(&self) -> PublicPairingChallenge {
        PublicPairingChallenge {
            nonce: self.nonce.clone(),
            instance_id: self.instance_id.clone(),
            installed_extension_id: self.installed_extension_id.clone(),
            expires_at_ms: self.expires_at_ms,
        }
    }
}

/// Public challenge. Never includes the verification code or session key.
#[derive(Debug, Clone)]
pub struct PublicPairingChallenge {
    pub nonce: String,
    pub instance_id: String,
    pub installed_extension_id: String,
    pub expires_at_ms: u64,
}

pub struct ExtensionPairing {
    instance_id: String,
    installed_extension_id: String,
    pending: Option<PairingChallenge>,
    app_confirmed: bool,
    session_key: Option<String>,
    connection_nonce: Option<String>,
    generation: u64,
    lease_deadline: Option<Instant>,
}

impl ExtensionPairing {
    pub fn new(instance_id: impl Into<String>, installed_extension_id: impl Into<String>) -> Self {
        Self {
            instance_id: instance_id.into(),
            installed_extension_id: installed_extension_id.into(),
            pending: None,
            app_confirmed: false,
            session_key: None,
            connection_nonce: None,
            generation: 0,
            lease_deadline: None,
        }
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub fn installed_extension_id(&self) -> &str {
        &self.installed_extension_id
    }

    pub fn set_installed_extension_id(&mut self, id: impl Into<String>) {
        self.installed_extension_id = id.into();
        self.revoke();
    }

    pub fn pending_challenge(&self) -> Option<PairingChallenge> {
        self.pending.clone()
    }

    pub fn session_key(&self) -> Option<&str> {
        self.lease_deadline
            .filter(|deadline| Instant::now() < *deadline)
            .and(self.session_key.as_deref())
    }

    pub fn live_connection(&self) -> Option<PairingConnection> {
        self.session_key()?;
        Some(PairingConnection {
            instance_id: self.instance_id.clone(),
            connection_nonce: self.connection_nonce.clone()?,
            generation: self.generation,
        })
    }

    pub fn begin_challenge(&mut self) -> PairingChallenge {
        self.revoke();
        // UUID v4 embeds fixed bits; use all 80 random bits for the human code.
        let mut bytes = [0u8; 10];
        getrandom::getrandom(&mut bytes).expect("OS randomness is required for pairing");
        let code: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let ch = PairingChallenge {
            nonce: Uuid::new_v4().to_string(),
            instance_id: self.instance_id.clone(),
            verification_code: format_verification_code(&code),
            installed_extension_id: self.installed_extension_id.clone(),
            expires_at_ms: now_ms().saturating_add(PAIRING_TTL_MS),
        };
        self.pending = Some(ch.clone());
        self.app_confirmed = false;
        ch
    }

    pub fn public_pending(&self) -> Option<PublicPairingChallenge> {
        self.pending.as_ref().map(PairingChallenge::public_view)
    }

    pub fn confirm_app(&mut self) -> Result<(), String> {
        let Some(ch) = self.pending.as_ref() else {
            return Err("no pairing challenge".into());
        };
        if ch.expired() {
            return Err("pairing challenge expired".into());
        }
        self.app_confirmed = true;
        Ok(())
    }

    pub fn confirm_app_for(&mut self, nonce: &str) -> Result<(), String> {
        if self.pending.as_ref().map(|ch| ch.nonce.as_str()) != Some(nonce) {
            return Err("pairing challenge changed".into());
        }
        self.confirm_app()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn extension_response(ch: &PairingChallenge) -> String {
        Self::proof_for(ch, "00000000-0000-4000-8000-000000000001").response
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn proof_for(ch: &PairingChallenge, connection_nonce: &str) -> PairingProof {
        let mut proof = PairingProof {
            protocol: PAIRING_PROTOCOL,
            nonce: ch.nonce.clone(),
            instance: ch.instance_id.clone(),
            ext: ch.installed_extension_id.clone(),
            expires_at: ch.expires_at_ms,
            connection_nonce: connection_nonce.into(),
            response: String::new(),
        };
        proof.response = pair_mac(
            &normalize_verification_code(&ch.verification_code),
            &proof_data(&proof),
        );
        proof
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn complete(&mut self, response: &str) -> Result<String, String> {
        let ch = self.pending.as_ref().ok_or("no pairing challenge")?;
        let mut proof = Self::proof_for(ch, "00000000-0000-4000-8000-000000000001");
        proof.response = response.into();
        self.complete_request(&proof)
            .map(|session| session.session_key)
    }

    pub fn complete_request(&mut self, proof: &PairingProof) -> Result<PairingSession, String> {
        let Some(ch) = self.pending.clone() else {
            return Err("no pairing challenge".into());
        };
        if ch.expired() {
            self.revoke();
            return Err("pairing challenge expired".into());
        }
        if !self.app_confirmed {
            return Err("pairing requires confirmation in the App".into());
        }
        if proof.protocol != PAIRING_PROTOCOL
            || proof.nonce != ch.nonce
            || proof.instance != ch.instance_id
            || proof.ext != ch.installed_extension_id
            || proof.expires_at != ch.expires_at_ms
            || Uuid::parse_str(&proof.connection_nonce).is_err()
        {
            return Err("pairing identity mismatch".into());
        }
        if !verify_pair_mac(
            &normalize_verification_code(&ch.verification_code),
            &proof_data(proof),
            &proof.response,
        ) {
            return Err("pairing challenge response mismatch".into());
        }
        let key = self.mint_session_key();
        self.connection_nonce = Some(proof.connection_nonce.clone());
        Ok(PairingSession {
            session_key: key,
            instance_id: self.instance_id.clone(),
            connection_nonce: proof.connection_nonce.clone(),
            generation: self.generation,
        })
    }

    fn mint_session_key(&mut self) -> String {
        let key = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        self.session_key = Some(key.clone());
        self.lease_deadline = Some(Instant::now() + CONNECTION_LEASE);
        self.generation = self.generation.saturating_add(1);
        self.pending = None;
        self.app_confirmed = false;
        key
    }

    #[cfg(test)]
    pub fn expire_pending(&mut self) {
        if let Some(ch) = self.pending.as_mut() {
            ch.expires_at_ms = 1;
        }
    }

    pub fn connect(
        &self,
        origin: &str,
        claimed_id: Option<&str>,
        token: Option<&str>,
    ) -> Result<(), String> {
        self.connect_at(origin, claimed_id, token, Instant::now())
    }

    fn connect_at(
        &self,
        origin: &str,
        claimed_id: Option<&str>,
        token: Option<&str>,
        now: Instant,
    ) -> Result<(), String> {
        if self.lease_deadline.is_none_or(|deadline| now >= deadline) {
            return Err("pairing connection lease expired".into());
        }
        let Some(token) = token.filter(|t| !t.is_empty()) else {
            return Err("pairing challenge required; Origin/extension id is not identity".into());
        };
        let Some(key) = self.session_key.as_deref() else {
            return Err("no active pairing session".into());
        };
        // Fixed-length HMAC verification avoids a secret-dependent string comparison.
        let mut actual = Hmac::<Sha256>::new_from_slice(token.as_bytes()).unwrap();
        actual.update(b"grok-cu-session-token-v1");
        let mut expected = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
        expected.update(b"grok-cu-session-token-v1");
        if token.len() != 64
            || expected
                .verify_slice(&actual.finalize().into_bytes())
                .is_err()
        {
            return Err("pairing token revoked or rotated".into());
        }
        if claimed_id != Some(self.installed_extension_id.as_str()) {
            return Err("extension identity does not match the installed pairing token".into());
        }
        let expected = format!("chrome-extension://{}", self.installed_extension_id);
        if origin != expected && origin != format!("{expected}/") {
            return Err("extension Origin does not match pairing identity".into());
        }
        Ok(())
    }

    pub fn authenticate_connection(
        &self,
        origin: &str,
        token: &str,
        connection: &PairingConnection,
    ) -> Result<(), String> {
        self.authenticate_connection_at(origin, token, connection, Instant::now())
    }

    fn authenticate_connection_at(
        &self,
        origin: &str,
        token: &str,
        connection: &PairingConnection,
        now: Instant,
    ) -> Result<(), String> {
        self.connect_at(origin, Some(&self.installed_extension_id), Some(token), now)?;
        if connection.instance_id != self.instance_id
            || self.connection_nonce.as_deref() != Some(connection.connection_nonce.as_str())
            || connection.generation != self.generation
        {
            return Err("pairing connection changed".into());
        }
        Ok(())
    }

    pub fn heartbeat_connection(
        &mut self,
        origin: &str,
        token: &str,
        connection: &PairingConnection,
    ) -> Result<(), String> {
        self.heartbeat_at(origin, token, connection, Instant::now())
    }

    fn heartbeat_at(
        &mut self,
        origin: &str,
        token: &str,
        connection: &PairingConnection,
        now: Instant,
    ) -> Result<(), String> {
        self.authenticate_connection_at(origin, token, connection, now)?;
        self.lease_deadline = Some(now + CONNECTION_LEASE);
        Ok(())
    }

    /// Maintenance clears stale state; dispatch independently checks the same deadline.
    pub fn expire_connection(&mut self) -> bool {
        if self.session_key.is_some()
            && self
                .lease_deadline
                .is_none_or(|deadline| Instant::now() >= deadline)
        {
            self.revoke();
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn expire_connection_for_test(&mut self) {
        self.lease_deadline = Some(Instant::now());
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn has_stored_connection_for_test(&self) -> bool {
        self.session_key.is_some()
    }

    pub fn revoke(&mut self) {
        self.session_key = None;
        self.pending = None;
        self.app_confirmed = false;
        self.connection_nonce = None;
        self.lease_deadline = None;
        self.generation = self.generation.saturating_add(1);
    }

    pub fn rotate(&mut self) -> Result<PairingChallenge, String> {
        if self.session_key.is_none() {
            return Err("no active pairing session".into());
        }
        self.session_key = None;
        Ok(self.begin_challenge())
    }
}

#[cfg(test)]
mod lease_tests;

fn proof_data(proof: &PairingProof) -> String {
    serde_json::json!([
        "grok-cu-pairing-v1",
        proof.nonce,
        proof.instance,
        proof.ext,
        proof.expires_at,
        proof.connection_nonce
    ])
    .to_string()
}

#[cfg(any(test, feature = "test-support"))]
fn pair_mac(secret: &str, data: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts verification codes of any length");
    mac.update(data.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn verify_pair_mac(secret: &str, data: &str, response: &str) -> bool {
    let Some(bytes) = decode_hex_32(response) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts verification codes of any length");
    mac.update(data.as_bytes());
    mac.verify_slice(&bytes).is_ok()
}

fn decode_hex_32(value: &str) -> Option<[u8; 32]> {
    let value = value.as_bytes();
    if value.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        let high = (value[index * 2] as char).to_digit(16)? as u8;
        let low = (value[index * 2 + 1] as char).to_digit(16)? as u8;
        *byte = (high << 4) | low;
    }
    Some(out)
}

fn normalize_verification_code(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .flat_map(char::to_uppercase)
        .collect()
}

fn format_verification_code(value: &str) -> String {
    value
        .as_bytes()
        .chunks(5)
        .map(|chunk| String::from_utf8_lossy(chunk).to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abandoned_connection_expires_without_a_disconnect_request() {
        let mut pairing = ExtensionPairing::new("lease-instance", "lease-extension");
        let challenge = pairing.begin_challenge();
        pairing.confirm_app().unwrap();
        let session = pairing
            .complete_request(&ExtensionPairing::proof_for(
                &challenge,
                "00000000-0000-4000-8000-000000000001",
            ))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_secs(31));
        assert!(
            pairing
                .connect(
                    "chrome-extension://lease-extension",
                    Some("lease-extension"),
                    Some(&session.session_key)
                )
                .is_err(),
            "an abandoned connection must not keep Host authority after its lease"
        );
    }

    #[test]
    fn pending_challenge_and_session_key_are_readable() {
        let mut p = ExtensionPairing::new("inst", "pw-ext-installed");
        assert!(p.pending_challenge().is_none());
        assert!(p.session_key().is_none());
        let ch = p.begin_challenge();
        assert_eq!(p.pending_challenge().unwrap().nonce, ch.nonce);
        p.confirm_app().unwrap();
        let key = p
            .complete(&ExtensionPairing::extension_response(&ch))
            .unwrap();
        assert_eq!(p.session_key(), Some(key.as_str()));
        assert!(p.pending_challenge().is_none());
    }

    #[test]
    fn origin_or_id_alone_cannot_connect() {
        let mut p = ExtensionPairing::new("inst", "pw-ext-installed");
        assert!(p
            .connect(
                "chrome-extension://pw-ext-installed/",
                Some("pw-ext-installed"),
                None
            )
            .is_err());
        let ch = p.begin_challenge();
        p.confirm_app().unwrap();
        let key = p
            .complete(&ExtensionPairing::extension_response(&ch))
            .unwrap();
        assert!(p
            .connect(
                "chrome-extension://pw-ext-installed/",
                Some("pw-ext-installed"),
                Some(&key)
            )
            .is_ok());
    }

    #[test]
    fn unconfirmed_complete_fails() {
        let mut p = ExtensionPairing::new("inst", "pw-ext-installed");
        p.begin_challenge();
        p.confirm_app().unwrap();
        assert!(p.complete("wrong-proof").unwrap_err().contains("response"));
    }

    #[test]
    fn revoke_and_rotate_drop_old_key() {
        let mut p = ExtensionPairing::new("inst", "pw-ext-installed");
        let ch = p.begin_challenge();
        p.confirm_app().unwrap();
        let old = p
            .complete(&ExtensionPairing::extension_response(&ch))
            .unwrap();
        p.revoke();
        assert!(p
            .connect(
                "chrome-extension://pw-ext-installed/",
                Some("pw-ext-installed"),
                Some(&old)
            )
            .is_err());
        let ch = p.begin_challenge();
        p.confirm_app().unwrap();
        let new_key = p
            .complete(&ExtensionPairing::extension_response(&ch))
            .unwrap();
        let ch2 = p.rotate().unwrap();
        assert!(p
            .connect(
                "chrome-extension://pw-ext-installed/",
                Some("pw-ext-installed"),
                Some(&new_key)
            )
            .is_err());
        p.confirm_app().unwrap();
        let rotated = p
            .complete(&ExtensionPairing::extension_response(&ch2))
            .unwrap();
        assert_ne!(rotated, new_key);
        assert!(p
            .connect(
                "chrome-extension://pw-ext-installed/",
                Some("pw-ext-installed"),
                Some(&rotated)
            )
            .is_ok());
    }

    #[test]
    fn public_challenge_omits_secret_and_expiry_blocks_complete() {
        let mut p = ExtensionPairing::new("inst", "pw-ext-installed");
        let ch = p.begin_challenge();
        let public = p.public_pending().unwrap();
        assert_eq!(public.nonce, ch.nonce);
        assert!(ch.expires_at_ms > now_ms());
        let dumped = format!("{public:?}");
        assert!(!dumped.contains(&ch.verification_code));
        p.confirm_app().unwrap();
        p.expire_pending();
        let proof = ExtensionPairing::extension_response(&ch);
        assert!(p.complete(&proof).unwrap_err().contains("expired"));
        assert!(p.session_key().is_none());
    }

    #[test]
    fn completion_requires_app_confirmation_and_extension_proof() {
        let mut p = ExtensionPairing::new("inst", "pw-ext-installed");
        let ch = p.begin_challenge();
        let proof = ExtensionPairing::extension_response(&ch);
        assert!(p.complete(&proof).unwrap_err().contains("App"));
        p.confirm_app().unwrap();
        assert!(p.complete("0").unwrap_err().contains("response"));
        assert!(p.complete(&proof).is_ok());
        assert!(p.complete(&proof).unwrap_err().contains("no pairing"));
    }

    #[test]
    fn verification_code_is_human_grouped_but_keeps_eighty_bits() {
        let mut p = ExtensionPairing::new("inst", "pw-ext-installed");
        let ch = p.begin_challenge();
        assert_eq!(ch.verification_code.len(), 23);
        assert_eq!(ch.verification_code.matches('-').count(), 3);
        assert_eq!(normalize_verification_code(&ch.verification_code).len(), 20);
        assert!(ch
            .verification_code
            .chars()
            .all(|ch| ch == '-' || ch.is_ascii_hexdigit()));
        assert!(!format!("{ch:?}").contains(&ch.verification_code));
    }
}
