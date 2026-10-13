//! Exact selected-page metadata queries pin the existing worker/run admission.
//! No profile enumeration, content read, input, or ownership transfer occurs.
use super::*;
use crate::browser::ManagedScopeIdentity;

impl ExistingTabHost {
    pub(crate) fn managed_scope_identity(
        &self,
        target_id: &str,
    ) -> Result<ManagedScopeIdentity, BrokerError> {
        let (owner, profile, page_id, session, generation) = {
            let g = self.inner.lock();
            let rec = g.tabs.get(target_id).ok_or(BrokerError::DeadTarget)?;
            self.check_request_locked(&g, &rec.info.run_id)?;
            if rec.info.closed
                || rec.info.user_owned
                || rec.disconnect.is_some()
                || !g.managed_runs.contains_key(&rec.info.run_id)
            {
                return Err(BrokerError::DeadTarget);
            }
            let profile = rec.profile.clone().ok_or(BrokerError::TargetUnauthorized)?;
            let page_id = rec
                .worker_page_id
                .clone()
                .ok_or(BrokerError::TargetUnauthorized)?;
            if g.profiles.get(&profile) != Some(&rec.info.run_id)
                || g.clearing_profiles.contains_key(&profile)
                || g.opening_profiles.contains_key(&profile)
            {
                return Err(BrokerError::TargetUnauthorized);
            }
            (
                rec.info.run_id.clone(),
                profile,
                page_id,
                rec.info.session.clone(),
                rec.info.generation,
            )
        };
        let cancellation = self
            .request
            .as_ref()
            .map(|request| request.identity.cancellation().clone())
            .unwrap_or_default();
        let request = self.admit_managed_request(&owner, cancellation)?;
        // Bind and HTTP work are outside the Host/Broker locks. A worker-slot
        // replacement cannot retarget this run's original Arc or revision.
        let scoped = self.for_managed_request(&request)?;
        let identity = scoped
            .worker()?
            .scope_identity(&owner, &profile, &page_id)
            .map_err(BrokerError::BrowserWorker)?;
        if identity.owner != owner
            || identity.profile != profile
            || identity.page_id != page_id
            || identity.target_id().as_deref() != Some(target_id)
        {
            return Err(BrokerError::IdentityMismatch("managed scope instance"));
        }
        let g = self.inner.lock();
        scoped.check_request_locked(&g, &owner)?;
        let rec = g.tabs.get(target_id).ok_or(BrokerError::DeadTarget)?;
        if rec.info.closed
            || rec.info.user_owned
            || rec.disconnect.is_some()
            || rec.info.run_id != owner
            || rec.info.session != session
            || rec.info.generation != generation
            || rec.profile.as_deref() != Some(profile.as_str())
            || rec.worker_page_id.as_deref() != Some(page_id.as_str())
            || g.profiles.get(&profile) != Some(&owner)
            || g.clearing_profiles.contains_key(&profile)
            || g.opening_profiles.contains_key(&profile)
        {
            return Err(BrokerError::DeadTarget);
        }
        Ok(identity)
    }
}
