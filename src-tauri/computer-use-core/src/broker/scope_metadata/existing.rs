//! Task preparation acquires an exact borrowed share, never a legacy grant.
use super::*;
use crate::adapter::NativeScopeIdentity;

impl ComputerUseBroker {
    pub(in crate::broker) fn authorize_prepared_existing(
        &self,
        ticket: &PrepareTicket,
    ) -> Result<u64, BrokerError> {
        self.prepare_authorize(&ticket.run_id)?;
        self.reject_cross_surface(&ticket.run_id, SurfaceKind::ExistingTab)?;
        let member = ticket
            .members
            .iter()
            .find(|member| {
                member.target_id == ticket.target_id && member.surface == SurfaceKind::ExistingTab
            })
            .ok_or(BrokerError::TargetUnauthorized)?;
        let proof = member
            .native
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?;
        let NativeScopeIdentity::ExistingPage { identity } = &proof.identity else {
            return Err(BrokerError::TargetUnauthorized);
        };
        let row = self.scope_member_info(&ticket.run_id, member, || {
            self.preparation_current(ticket.id)
                .then_some(())
                .ok_or(BrokerError::StopRequested)
        })?;
        let executor = self.require_surface_executor(SurfaceKind::ExistingTab)?;
        if executor.generation != proof.executor_generation {
            return Err(BrokerError::TargetUnauthorized);
        }
        let _guard = self.existing_authorization.lock();
        self.recheck_prepared_scope(ticket)?;
        if !self.preparation_current(ticket.id) {
            return Err(BrokerError::StopRequested);
        }
        let (borrowed, acquired) =
            self.tabs
                .acquire_scope_tab(&ticket.session_id, &ticket.run_id, identity)?;
        let result = (|| {
            self.recheck_prepared_scope(ticket)?;
            if !self.preparation_current(ticket.id) {
                return Err(BrokerError::StopRequested);
            }
            self.commit_authorize(
                &ticket.run_id,
                &ticket.target_id,
                row.title,
                SurfaceKind::ExistingTab,
                executor.generation,
                None,
            )
        })();
        if result.is_err() && acquired {
            self.tabs.release_existing_grant(&borrowed);
        }
        result
    }
}
