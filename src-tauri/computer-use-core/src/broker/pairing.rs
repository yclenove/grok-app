use super::ComputerUseBroker;
use crate::browser::ScopePairing;
use crate::pairing::{PairingChallenge, PairingProof, PairingSession};

impl ComputerUseBroker {
    pub fn claim_extension_observation(
        &self,
        origin: &str,
        token: &str,
        proof: &crate::browser::extension_observation::ObservationProof,
    ) -> Result<(), String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.claim_extension_observation(origin, token, proof)
    }

    pub fn negotiate_extension_observations(
        &self,
        origin: &str,
        token: &str,
        connection: &crate::pairing::PairingConnection,
    ) -> Result<(), String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.read_pairing_connection(origin, token, connection)
    }

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
        {
            let gate = self.inner.lock();
            if !gate.feature_enabled {
                return Err("computer use is disabled".into());
            }
        }
        // Host revalidates the original grant after decode. Do not prevent
        // feature-off or Pause from cancelling it while image validation runs.
        self.tabs.complete_extension_request(origin, token, result)
    }

    pub fn offer_connected_tab(
        &self,
        connection: &crate::pairing::PairingConnection,
        sequence: u64,
        offer: crate::browser::SharedTabOffer<'_>,
    ) -> Result<(), String> {
        self.offer_connected_tab_document(connection, sequence, offer, None)
    }

    pub fn offer_connected_tab_document(
        &self,
        connection: &crate::pairing::PairingConnection,
        sequence: u64,
        offer: crate::browser::SharedTabOffer<'_>,
        document_id: Option<&str>,
    ) -> Result<(), String> {
        let mut gate = self.inner.lock();
        self.admit_existing_share_update(&mut gate, connection)?;
        self.tabs
            .offer_connected_tab_document(connection, sequence, offer, document_id)
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
        let mut gate = self.inner.lock();
        self.admit_existing_share_update(&mut gate, connection)?;
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
        let mut gate = self.inner.lock();
        self.sweep_existing_share_preparation(&mut gate);
        let prepared = gate.existing_share_preparation.as_ref().is_some_and(|prep| {
            matches!(&prep.pairing, ScopePairing::Challenge(challenge) if challenge.nonce == proof.nonce)
        });
        if !gate.feature_enabled && !prepared {
            return Err("computer use is disabled".into());
        }
        let session = self.tabs.complete_pairing_request(proof)?;
        if prepared {
            if let Some(prep) = gate.existing_share_preparation.as_mut() {
                prep.pairing = ScopePairing::Connection(crate::pairing::PairingConnection {
                    instance_id: session.instance_id.clone(),
                    connection_nonce: session.connection_nonce.clone(),
                    generation: session.generation,
                });
            }
        }
        Ok(session)
    }

    pub fn public_pairing_challenge(
        &self,
    ) -> Result<Option<crate::pairing::PublicPairingChallenge>, String> {
        let mut gate = self.inner.lock();
        self.sweep_existing_share_preparation(&mut gate);
        if !gate.feature_enabled
            && !gate
                .existing_share_preparation
                .as_ref()
                .is_some_and(|prep| matches!(prep.pairing, ScopePairing::Challenge(_)))
        {
            return Err("computer use is disabled".into());
        }
        Ok(self.tabs.public_pairing_challenge())
    }

    /// Status keeps a completed Pair visible after task preparation ends. Only
    /// an admitted preparation or enabled feature can renew the connection.
    pub fn pairing_connection_status(
        &self,
        origin: &str,
        token: &str,
        connection: &crate::pairing::PairingConnection,
    ) -> Result<bool, String> {
        let mut gate = self.inner.lock();
        if self
            .admit_existing_share_connection(&mut gate, connection)
            .is_ok()
        {
            self.tabs
                .check_pairing_connection(origin, token, connection, false)?;
        } else {
            self.tabs
                .read_pairing_connection(origin, token, connection)?;
        }
        Ok(gate.feature_enabled)
    }

    /// Explicit renewal/retirement still requires an admitted connection.
    /// Readiness is a hint, not an execution grant.
    pub fn check_pairing_connection(
        &self,
        origin: &str,
        token: &str,
        connection: &crate::pairing::PairingConnection,
        disconnect: bool,
    ) -> Result<bool, String> {
        let mut gate = self.inner.lock();
        self.admit_existing_share_connection(&mut gate, connection)?;
        self.tabs
            .check_pairing_connection(origin, token, connection, disconnect)?;
        Ok(gate.feature_enabled)
    }

    pub fn register_extension_browser(
        &self,
        origin: &str,
        token: &str,
        connection: &crate::pairing::PairingConnection,
        current_id: &str,
        previous_id: Option<&str>,
        cleanups: &[crate::browser::extension_completion::BrowserExitCleanup],
    ) -> Result<Vec<(String, &'static str)>, String> {
        let mut gate = self.inner.lock();
        self.admit_existing_share_connection(&mut gate, connection)?;
        // Off-mode preparation has no execution occupancy to retire. Do not
        // accept a historical cleanup list through this metadata-only channel.
        if !gate.feature_enabled && !cleanups.is_empty() {
            return Err("execution cleanup unavailable during metadata preparation".into());
        }
        self.tabs
            .browser_restart(origin, token, connection, current_id, previous_id, cleanups)
    }
}
