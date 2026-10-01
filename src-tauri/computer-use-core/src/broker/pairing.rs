use super::ComputerUseBroker;
use crate::pairing::{PairingChallenge, PairingProof, PairingSession};

impl ComputerUseBroker {
    pub fn claim_existing_completion(
        &self,
        origin: &str,
        token: &str,
        proof: &crate::browser::extension_completion::CompletionProof,
    ) -> Result<(), String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.claim_existing_completion(origin, token, proof)
    }

    pub fn poll_extension_request(
        &self,
        origin: &str,
        token: &str,
        connection: &crate::pairing::PairingConnection,
    ) -> Result<Option<crate::browser::extension_protocol::ExtensionRequest>, String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.poll_extension_request(origin, token, connection)
    }

    pub fn complete_extension_request(
        &self,
        origin: &str,
        token: &str,
        result: crate::browser::extension_protocol::ExtensionResult,
    ) -> Result<(), String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.complete_extension_request(origin, token, result)
    }

    pub fn offer_connected_tab(
        &self,
        connection: &crate::pairing::PairingConnection,
        sequence: u64,
        offer: crate::browser::SharedTabOffer<'_>,
    ) -> Result<(), String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.offer_connected_tab(connection, sequence, offer)
    }

    pub fn unoffer_connected_tab(
        &self,
        origin: &str,
        token: &str,
        connection: &crate::pairing::PairingConnection,
        sequence: u64,
        tab_id: &str,
        document: u64,
    ) -> Result<bool, String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs
            .unoffer_connected_tab(origin, token, connection, sequence, tab_id, document)
    }

    // Use the same lock ordering as feature-off: Broker gate -> tab registry.
    pub fn begin_pairing(&self) -> Result<PairingChallenge, String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        Ok(self.tabs.begin_pairing_challenge())
    }

    pub fn confirm_pairing(&self, nonce: &str) -> Result<(), String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.confirm_pairing_app_for(nonce)
    }

    pub fn complete_pairing(&self, proof: &PairingProof) -> Result<PairingSession, String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.complete_pairing_request(proof)
    }
}
