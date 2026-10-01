//! Read the authorized target from its retained executor, not the Host picker.
//! Discovery candidates and synthetic labels are not an authorization receipt.
use super::{
    surface_router::SurfaceRouter, BrokerError, ComputerUseBroker, SurfaceKind, TargetInfo,
};
use std::sync::Arc;

impl ComputerUseBroker {
    /// Exact metadata for an already-authorized run. No ambient discovery or
    /// cross-surface fallback is allowed, including for browser profile pickers.
    /// Adapter callbacks run without the Broker lock; revalidate the original
    /// binding afterward so Stop and same-id reauthorization reject late data.
    pub fn authorized_target_info(
        &self,
        run_id: &str,
        surface: SurfaceKind,
    ) -> Result<TargetInfo, BrokerError> {
        let (binding, executor) = self.authorized_binding(run_id)?;
        if binding.surface != surface {
            return Err(BrokerError::IdentityMismatch("surface"));
        }
        let expected = (binding.target_id.clone(), binding.target_generation);
        if self.authorized_target(run_id)? != expected {
            return Err(BrokerError::IdentityMismatch("targetGeneration"));
        }
        let result = executor
            .adapter
            .list_targets_for_run(run_id)
            .map_err(BrokerError::Adapter)
            .and_then(|rows| {
                let mut matches = rows
                    .into_iter()
                    .filter(|row| row.target_id == binding.target_id);
                let row = matches.next().ok_or(BrokerError::TargetUnauthorized)?;
                if matches.next().is_some() {
                    return Err(BrokerError::IdentityMismatch("ambiguousTargetMetadata"));
                }
                if SurfaceRouter::classify_target_id(&row.target_id) != surface {
                    return Err(BrokerError::IdentityMismatch("surface"));
                }
                if !executor.adapter.target_alive(&row.target_id) {
                    return Err(BrokerError::DeadTarget);
                }
                Ok(row)
            });
        let current = self.executor_for_target(&binding)?;
        if !Arc::ptr_eq(&current.adapter, &executor.adapter) {
            return Err(BrokerError::IdentityMismatch("surfaceExecutorGeneration"));
        }
        if self.authorized_target(run_id)? != expected {
            return Err(BrokerError::IdentityMismatch("targetGeneration"));
        }
        result
    }
}
