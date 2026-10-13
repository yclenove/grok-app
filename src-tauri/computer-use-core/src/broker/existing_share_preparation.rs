//! An off-mode metadata admission belongs to one original Host picker, not a model.
use super::*;
use crate::browser::ScopePairing;
use crate::pairing::{PairingChallenge, PairingConnection};
use crate::task_consent::{HostRequestKey, ScopeSelectionPermit};

const PREPARATION_TTL: Duration = Duration::from_secs(5 * 60);

pub(super) struct ExistingSharePreparation {
    id: String,
    permit: ScopeSelectionPermit,
    key: HostRequestKey,
    deadline: Instant,
    pub(super) pairing: ScopePairing,
}

pub(super) struct ExistingSharePreparationGuard {
    broker: Arc<ComputerUseBroker>,
    id: String,
}

impl ComputerUseBroker {
    pub(super) fn begin_existing_share_preparation(
        self: &Arc<Self>,
        inner: &mut Inner,
        permit: &ScopeSelectionPermit,
        key: &HostRequestKey,
    ) -> Result<ExistingSharePreparationGuard, BrokerError> {
        self.sweep_existing_share_preparation(inner);
        if inner.existing_share_preparation.is_some() {
            return Err(BrokerError::Schema("pairing preparation busy".into()));
        }
        self.require_surface_executor(SurfaceKind::ExistingTab)?;
        let pairing = self
            .tabs
            .prepare_scope_pairing()
            .map_err(BrokerError::Adapter)?;
        let id = Uuid::new_v4().simple().to_string();
        inner.existing_share_preparation = Some(ExistingSharePreparation {
            id: id.clone(),
            permit: permit.clone(),
            key: key.clone(),
            deadline: Instant::now() + PREPARATION_TTL,
            pairing,
        });
        Ok(ExistingSharePreparationGuard {
            broker: self.clone(),
            id,
        })
    }

    pub(super) fn sweep_existing_share_preparation(&self, inner: &mut Inner) {
        let retired = inner
            .existing_share_preparation
            .as_ref()
            .is_some_and(|prep| {
                Instant::now() >= prep.deadline
                    || inner
                        .consent
                        .require_host_request(&prep.permit.context, &prep.key)
                        .is_err()
                    || inner
                        .consent
                        .recheck_existing_pairing_preparation(
                            &prep.permit,
                            inner.feature_enabled,
                            inner.feature_epoch,
                        )
                        .is_err()
                    || !self.tabs.scope_pairing_current(&prep.pairing)
            });
        if retired {
            if let Some(prep) = inner.existing_share_preparation.take() {
                self.tabs.retire_scope_pairing(&prep.pairing);
            }
        }
    }

    /// Called by the original IPC maintenance owner; no detached lifetime.
    pub(crate) fn expire_existing_share_preparation(&self) {
        self.sweep_existing_share_preparation(&mut self.inner.lock());
    }

    #[cfg(test)]
    pub(crate) fn expire_existing_share_preparation_for_test(&self) {
        if let Some(prep) = self.inner.lock().existing_share_preparation.as_mut() {
            prep.deadline = Instant::now();
        }
        self.expire_existing_share_preparation();
    }

    pub(super) fn admit_existing_share_connection(
        &self,
        inner: &mut Inner,
        connection: &PairingConnection,
    ) -> Result<(), String> {
        self.sweep_existing_share_preparation(inner);
        if inner.feature_enabled || inner.existing_share_preparation.as_ref().is_some_and(|prep| {
            matches!(&prep.pairing, ScopePairing::Connection(expected) if expected == connection)
        }) {
            Ok(())
        } else {
            Err("computer use metadata preparation unavailable".into())
        }
    }

    pub(super) fn admit_existing_share_update(
        &self,
        inner: &mut Inner,
        connection: &PairingConnection,
    ) -> Result<(), String> {
        self.admit_existing_share_connection(inner, connection)?;
        if !inner.feature_enabled {
            let prep = inner
                .existing_share_preparation
                .as_ref()
                .ok_or("computer use metadata preparation unavailable")?;
            inner
                .consent
                .recheck_scope_selection(&prep.permit, inner.feature_epoch)
                .map_err(|_| "shared selection is already reserved")?;
        }
        Ok(())
    }
}

impl ExistingSharePreparationGuard {
    pub(super) fn view(&self) -> Result<HostExistingSharePreparationView, BrokerError> {
        let mut inner = self.broker.inner.lock();
        self.broker.sweep_existing_share_preparation(&mut inner);
        let prep = inner
            .existing_share_preparation
            .as_ref()
            .filter(|prep| prep.id == self.id)
            .ok_or(BrokerError::StopRequested)?;
        inner
            .consent
            .recheck_scope_selection(&prep.permit, inner.feature_epoch)?;
        let (challenge, confirmed, counts) = match &prep.pairing {
            ScopePairing::Challenge(ch) => (
                Some(ch.clone()),
                self.broker.tabs.scope_pairing_confirmed(ch),
                (0, 0, 0),
            ),
            ScopePairing::Connection(connection) => (
                None,
                false,
                self.broker.tabs.existing_scope_availability(connection)?,
            ),
        };
        Ok(HostExistingSharePreparationView {
            challenge,
            pairing_confirmed: confirmed,
            available_tabs: counts.0,
            incompatible_tabs: counts.1,
            borrowed_tabs: counts.2,
        })
    }

    pub(super) fn challenge(&self) -> Result<Option<PairingChallenge>, BrokerError> {
        let mut inner = self.broker.inner.lock();
        self.broker.sweep_existing_share_preparation(&mut inner);
        let prep = inner
            .existing_share_preparation
            .as_ref()
            .filter(|prep| prep.id == self.id)
            .ok_or(BrokerError::StopRequested)?;
        Ok(match &prep.pairing {
            ScopePairing::Challenge(challenge) => Some(challenge.clone()),
            ScopePairing::Connection(_) => None,
        })
    }

    pub(super) fn confirm(&self, nonce: &str) -> Result<(), BrokerError> {
        let mut inner = self.broker.inner.lock();
        self.broker.sweep_existing_share_preparation(&mut inner);
        let prep = inner
            .existing_share_preparation
            .as_ref()
            .filter(|prep| prep.id == self.id)
            .ok_or(BrokerError::StopRequested)?;
        if !matches!(&prep.pairing, ScopePairing::Challenge(challenge) if challenge.nonce == nonce)
        {
            return Err(BrokerError::IdentityMismatch("pairing challenge"));
        }
        self.broker
            .tabs
            .confirm_pairing_app_for(nonce)
            .map_err(BrokerError::Adapter)
    }
}

impl Drop for ExistingSharePreparationGuard {
    fn drop(&mut self) {
        let mut inner = self.broker.inner.lock();
        if inner
            .existing_share_preparation
            .as_ref()
            .is_some_and(|prep| prep.id == self.id)
        {
            if let Some(prep) = inner.existing_share_preparation.take() {
                self.broker.tabs.retire_scope_pairing(&prep.pairing);
            }
        }
    }
}
