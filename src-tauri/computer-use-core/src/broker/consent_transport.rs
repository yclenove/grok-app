//! Token-scoped Host handshake operations, sharing the consent authority lock.
use super::*;
use crate::task_consent::IssuedTransport;

impl ComputerUseBroker {
    /// Host lifecycle tick. No window/title/DOM/capture/input work is admitted.
    pub fn sweep_task_consent(&self) {
        let _change = self.changes.notice();
        let busy = self.busy_run_ids();
        self.inner.lock().consent.sweep_from_host(&busy);
        self.handoff_expired();
    }

    pub fn reserve_transport_connection(
        &self,
        session: &str,
    ) -> Result<IssuedTransport, BrokerError> {
        let _change = self.changes.notice();
        let result = {
            let mut inner = self.inner.lock();
            let epoch = inner.feature_epoch;
            let retained = super::consent::runtime_retained(&inner);
            inner.consent.reserve_connection(session, epoch, &retained)
        };
        let (issued, runs) = result?;
        self.handoff_fences(&runs);
        Ok(issued)
    }

    pub fn bind_transport_agent(&self, token: &str, agent: &str) -> Result<(), BrokerError> {
        let _change = self.changes.notice();
        let runs = self.inner.lock().consent.bind_transport(token, agent)?;
        self.handoff_fences(&runs);
        Ok(())
    }

    /// Idempotent and exact: a retired token cannot retire a newer connection.
    pub fn retire_transport_connection(&self, token: &str) {
        let _change = self.changes.notice();
        let runs = self.inner.lock().consent.retire_transport(token);
        self.handoff_fences(&runs);
    }
}
