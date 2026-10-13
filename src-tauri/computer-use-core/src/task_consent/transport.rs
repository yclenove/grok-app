//! Exact handshake ownership. A late old handshake cannot mutate its successor.
use std::collections::HashSet;

use crate::error::BrokerError;

use super::{BearerClass, ConsentState, IssuedTransport};

impl ConsentState {
    pub fn reserve_connection(
        &mut self,
        session: &str,
        epoch: u64,
        retained: &HashSet<String>,
    ) -> Result<(IssuedTransport, Vec<String>), BrokerError> {
        let existed = self.owners.get(session).is_some_and(|owner| !owner.retired);
        // Validate capacity first; do not abandon a live resource on overflow.
        let issued = self.issue(session, epoch, retained)?;
        if !existed {
            return Ok((issued, Vec::new()));
        }
        let runs = self.rotate_connection(session)?;
        self.owners
            .get_mut(session)
            .expect("reserved owner")
            .agent_session_id = None;
        Ok((self.issue(session, epoch, retained)?, runs))
    }

    pub fn bind_transport(&mut self, token: &str, agent: &str) -> Result<Vec<String>, BrokerError> {
        let BearerClass::Live(context) = self.classify(token) else {
            return Err(BrokerError::IdentityMismatch("transport"));
        };
        self.require_owner(&context, false)?;
        // A handshake binds once. Replacing an agent needs a fresh reservation
        // and catalog; silently rotating here strands the installed MCP child.
        if context
            .agent_session_id
            .as_deref()
            .is_some_and(|current| current != agent)
        {
            return Err(BrokerError::IdentityMismatch("agentSessionId"));
        }
        self.bind_agent(&context.app_session_id, agent)
    }

    pub fn retire_transport(&mut self, token: &str) -> Vec<String> {
        let BearerClass::Live(context) = self.classify(token) else {
            return Vec::new();
        };
        if self.require_owner(&context, false).is_err() {
            return Vec::new();
        }
        self.retire_owner(&context.app_session_id)
    }
}
