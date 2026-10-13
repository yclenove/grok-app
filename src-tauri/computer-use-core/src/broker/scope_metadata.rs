//! Exact selected-instance metadata. Discovery is only a Host picker action.
use super::*;
use crate::task_consent::{PrepareTicket, ScopeMember};

impl ComputerUseBroker {
    pub(super) fn scope_member_info(
        &self,
        run_id: &str,
        member: &ScopeMember,
        recheck: impl Fn() -> Result<(), BrokerError>,
    ) -> Result<TargetInfo, BrokerError> {
        recheck()?;
        let proof = member
            .native
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?;
        self.recheck_scope_proof(&member.target_id, member.surface, proof)?;
        let executor = self.require_surface_executor(member.surface)?;
        if executor.generation != proof.executor_generation {
            return Err(BrokerError::IdentityMismatch("scope executor"));
        }
        // A native proof query may itself block or retire the request/run.
        // Check authority again before the first title/metadata callback.
        recheck()?;
        let row = executor
            .adapter
            .scope_target_info(run_id, &member.target_id)
            .map_err(BrokerError::Adapter)?;
        recheck()?;
        self.recheck_scope_proof(&member.target_id, member.surface, proof)?;
        recheck()?;
        if row.target_id != member.target_id
            || SurfaceRouter::classify_target_id(&row.target_id) != member.surface
            || row.backend != executor.adapter.backend_id()
            || (member.surface == SurfaceKind::Desktop && row.title.is_empty())
            || row.title.len() > 4096
            || (member.surface == SurfaceKind::Desktop
                && crate::adapter::skip_desktop_target(&row.title, &row.app_name))
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        Ok(row)
    }

    pub(super) fn authorize_prepared_desktop(
        &self,
        ticket: &PrepareTicket,
    ) -> Result<u64, BrokerError> {
        self.prepare_authorize(&ticket.run_id)?;
        self.reject_cross_surface(&ticket.run_id, SurfaceKind::Desktop)?;
        let member = ticket
            .members
            .iter()
            .find(|member| {
                member.target_id == ticket.target_id && member.surface == SurfaceKind::Desktop
            })
            .ok_or(BrokerError::TargetUnauthorized)?;
        let row = self.scope_member_info(&ticket.run_id, member, || {
            if self.preparation_current(ticket.id) {
                Ok(())
            } else {
                Err(BrokerError::StopRequested)
            }
        })?;
        self.recheck_prepared_scope(ticket)?;
        if !self.preparation_current(ticket.id) {
            return Err(BrokerError::StopRequested);
        }
        let executor = self.require_surface_executor(SurfaceKind::Desktop)?;
        if member
            .native
            .as_ref()
            .map(|proof| proof.executor_generation)
            != Some(executor.generation)
        {
            return Err(BrokerError::IdentityMismatch("scope executor"));
        }
        executor
            .adapter
            .claim_target_for_run(&ticket.run_id, &ticket.target_id)
            .map_err(BrokerError::Adapter)?;
        let result = (|| {
            self.recheck_prepared_scope(ticket)?;
            if !self.preparation_current(ticket.id) {
                return Err(BrokerError::StopRequested);
            }
            self.commit_authorize(
                &ticket.run_id,
                &ticket.target_id,
                row.title,
                SurfaceKind::Desktop,
                executor.generation,
                None,
            )
        })();
        if result.is_err() {
            executor
                .adapter
                .release_target_for_run(&ticket.run_id, &ticket.target_id);
        }
        result
    }
}

mod existing;
