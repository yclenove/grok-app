use super::ComputerUseBroker;
use crate::browser::extension_action::{ActionDispatch, ActionNegotiation, ActionResult};
use crate::pairing::PairingConnection;

impl ComputerUseBroker {
    pub fn negotiate_existing_actions(
        &self,
        origin: &str,
        token: &str,
        negotiation: ActionNegotiation,
    ) -> Result<(), String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs
            .negotiate_existing_actions(origin, token, negotiation)
    }

    pub fn poll_existing_action(
        &self,
        origin: &str,
        token: &str,
        connection: &PairingConnection,
    ) -> Result<Option<ActionDispatch>, String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.poll_existing_action(origin, token, connection)
    }

    pub fn claim_existing_action(
        &self,
        origin: &str,
        token: &str,
        dispatch: &ActionDispatch,
    ) -> Result<(), String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.claim_existing_action(origin, token, dispatch)
    }

    pub fn complete_existing_action(
        &self,
        origin: &str,
        token: &str,
        result: ActionResult,
    ) -> Result<(), String> {
        let gate = self.inner.lock();
        if !gate.feature_enabled {
            return Err("computer use is disabled".into());
        }
        self.tabs.complete_existing_action(origin, token, result)
    }
}
