use super::{existing::expire_pairing, ExistingTabHost};
use crate::browser::TabInfo;

impl ExistingTabHost {
    pub fn extension_connected(&self) -> bool {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state.pairing.live_connection().is_some()
    }

    /// Idempotent release of one captured grant. Never closes or navigates the tab.
    pub(crate) fn release_existing_grant(&self, expected: &TabInfo) -> bool {
        let mut state = self.inner.lock();
        let Some(record) = state.tabs.get_mut(&expected.tab_id) else {
            return false;
        };
        if !record.info.user_owned
            || !record.info.borrowed
            || record.info.run_id != expected.run_id
            || record.info.session != expected.session
            || record.info.generation != expected.generation
        {
            return false;
        }
        record.info.borrowed = false;
        record.info.generation = record.info.generation.saturating_add(1);
        record.info.preview_generation = 0;
        record.current_observation = None;
        super::transport::sweep_requests(&mut state);
        super::completion::sweep(&mut state);
        true
    }

    /// Metadata only; requires the current authenticated share and borrowed grant.
    pub fn existing_target_info(&self, tab: &str) -> Option<TabInfo> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        let record = state.tabs.get(tab)?;
        Self::require_live_grant(record, &record.info.run_id).ok()?;
        if !record.info.user_owned {
            return None;
        }
        let offer = state.shared.get(tab)?;
        let connection = state.pairing.live_connection()?;
        if record.info.connection_generation != connection.generation
            || offer.connection_generation != connection.generation
            || offer.document_generation != record.info.document_generation
            || record.pairing_token.as_deref() != state.pairing.session_key()
        {
            return None;
        }
        Some(record.info.clone())
    }
}
