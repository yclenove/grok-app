//! Opaque original share/connection/document proof. No content or execution.
use super::{existing::expire_pairing, ExistingTabHost, Inner, TabAttachment};
use crate::adapter::{NativeScopeIdentity, ScopeCandidate, ScopeCandidatePage, TargetInfo};
use crate::browser::TabInfo;
use crate::error::BrokerError;
use crate::pairing::PairingConnection;

/// Only the authenticated Host registry can construct this proof. No token,
/// Cookie, title or URL enters it; Debug never exposes connection/share IDs.
#[derive(Clone, PartialEq, Eq)]
pub struct ExistingScopeIdentity {
    connection: PairingConnection,
    tab_id: String,
    share_id: String,
    document_id: String,
    document_generation: u64,
    browser_id: String,
    profile_id: String,
}

impl std::fmt::Debug for ExistingScopeIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExistingScopeIdentity")
            .finish_non_exhaustive()
    }
}

impl ExistingScopeIdentity {
    pub(crate) fn target_id(&self) -> String {
        format!("tab:{}", self.tab_id)
    }
}

fn tab_id(target: &str) -> Result<&str, BrokerError> {
    let id = target
        .strip_prefix("tab:")
        .ok_or(BrokerError::TargetUnauthorized)?;
    let value = id
        .parse::<u64>()
        .map_err(|_| BrokerError::TargetUnauthorized)?;
    if value > crate::protocol::JS_MAX_SAFE_INTEGER || value.to_string() != id {
        return Err(BrokerError::TargetUnauthorized);
    }
    Ok(id)
}

fn identity(state: &Inner, tab: &str) -> Result<ExistingScopeIdentity, BrokerError> {
    // Legacy offers accept opaque IDs; the task adapter only routes canonical
    // Chromium numeric IDs. Do not emit a candidate it cannot later bind.
    tab_id(&format!("tab:{tab}"))?;
    let connection = state
        .pairing
        .live_connection()
        .ok_or(BrokerError::TargetUnauthorized)?;
    let offer = state
        .shared
        .get(tab)
        .ok_or(BrokerError::TargetUnauthorized)?;
    state
        .pairing
        .authenticate_connection(&offer.origin, &offer.pairing_token, &connection)
        .map_err(|_| BrokerError::TargetUnauthorized)?;
    let document_id = offer
        .document_id
        .as_ref()
        .ok_or(BrokerError::SurfaceUnavailable {
            surface: crate::adapter::SurfaceKind::ExistingTab,
        })?;
    if offer.connection_generation != connection.generation
        || offer.extension_id.as_deref() != Some(state.installed_extension_id.as_str())
        || state.tabs.get(tab).is_some_and(|row| row.info.closed)
    {
        return Err(BrokerError::TargetUnauthorized);
    }
    Ok(ExistingScopeIdentity {
        connection,
        tab_id: tab.into(),
        share_id: offer.picker_token.clone(),
        document_id: document_id.clone(),
        document_generation: offer.document_generation,
        browser_id: offer.browser_id.clone(),
        profile_id: offer.profile_id.clone(),
    })
}

fn info(state: &Inner, proof: &ExistingScopeIdentity) -> Result<TargetInfo, BrokerError> {
    let offer = state
        .shared
        .get(&proof.tab_id)
        .ok_or(BrokerError::TargetUnauthorized)?;
    Ok(TargetInfo {
        target_id: proof.target_id(),
        title: offer.title.clone(),
        app_name: offer.browser_id.clone(),
        kind: "existing-tab".into(),
        pid: None,
        backend: "chromium-extension".into(),
        execution_mode: "borrow".into(),
        replay_policy: "never".into(),
        lifecycle_stamp: offer.document_generation,
        display_id: "existing-tabs".into(),
        coordinate_space: "image-pixels".into(),
        scope_label: "explicit-user-share".into(),
    })
}

impl ExistingTabHost {
    /// Counts only original authenticated shared metadata. No tab enumeration,
    /// content capture, retained proof or page borrowing takes place here.
    pub(crate) fn existing_scope_availability(
        &self,
        expected: &PairingConnection,
    ) -> Result<(usize, usize, usize), BrokerError> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        if state.pairing.live_connection().as_ref() != Some(expected) {
            return Err(BrokerError::TargetUnauthorized);
        }
        let (mut available, mut incompatible, mut borrowed) = (0, 0, 0);
        for tab in state.shared.keys() {
            if identity(&state, tab).is_err() {
                incompatible += 1;
            } else if state.tabs.get(tab).is_some_and(|row| row.info.borrowed) {
                borrowed += 1;
            } else {
                available += 1;
            }
        }
        let bound = crate::broker::MAX_SCOPE_CANDIDATES;
        Ok((
            available.min(bound),
            incompatible.min(bound),
            borrowed.min(bound),
        ))
    }

    pub(crate) fn existing_scope_identity(
        &self,
        target: &str,
    ) -> Result<ExistingScopeIdentity, BrokerError> {
        let tab = tab_id(target)?;
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        identity(&state, tab)
    }

    /// The caller already holds the Broker authority gate. Keep the original
    /// shares pinned through its short authority-only commit, in Broker -> Host
    /// lock order. The callback must not query an adapter or reenter this Host.
    pub(crate) fn with_existing_scope_authority<T>(
        &self,
        expected: &[&ExistingScopeIdentity],
        commit: impl FnOnce() -> Result<T, BrokerError>,
    ) -> Result<T, BrokerError> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        for proof in expected {
            if identity(&state, &proof.tab_id)? != **proof {
                return Err(BrokerError::DeadTarget);
            }
        }
        let result = commit();
        drop(state);
        result
    }

    pub(crate) fn existing_scope_info(
        &self,
        run: &str,
        target: &str,
    ) -> Result<TargetInfo, BrokerError> {
        let tab = tab_id(target)?;
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        let proof = identity(&state, tab)?;
        if state
            .tabs
            .get(tab)
            .is_some_and(|row| row.info.borrowed && row.info.run_id != run)
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        info(&state, &proof)
    }

    pub(crate) fn existing_scope_candidates(
        &self,
        limit: usize,
        current: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<ScopeCandidatePage, BrokerError> {
        if !current() {
            return Err(BrokerError::StopRequested);
        }
        // Snapshot only authenticated shared metadata; never enumerate browser tabs.
        let ids = {
            let mut state = self.inner.lock();
            expire_pairing(&mut state);
            let mut ids = state.shared.keys().cloned().collect::<Vec<_>>();
            ids.sort();
            ids
        };
        let mut candidates = Vec::new();
        let mut truncated = false;
        for tab in ids {
            if !current() {
                return Err(BrokerError::StopRequested);
            }
            let row = {
                let mut state = self.inner.lock();
                expire_pairing(&mut state);
                if state.tabs.get(&tab).is_some_and(|row| row.info.borrowed) {
                    continue;
                }
                let Ok(proof) = identity(&state, &tab) else {
                    continue;
                };
                ScopeCandidate {
                    info: info(&state, &proof)?,
                    identity: NativeScopeIdentity::ExistingPage {
                        identity: Box::new(proof),
                    },
                }
            };
            if candidates.len() >= limit {
                truncated = true;
                break;
            }
            candidates.push(row);
        }
        if !current() {
            return Err(BrokerError::StopRequested);
        }
        Ok(ScopeCandidatePage {
            candidates,
            truncated,
        })
    }

    /// Reserve only the original proof's share, under the same lock as attach.
    /// The caller owns the original Host task preparation, not a legacy run.
    pub(crate) fn acquire_scope_tab(
        &self,
        session: &str,
        run: &str,
        expected: &ExistingScopeIdentity,
    ) -> Result<(TabInfo, bool), BrokerError> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        if identity(&state, &expected.tab_id)? != *expected {
            return Err(BrokerError::DeadTarget);
        }
        let offer = state
            .shared
            .get(&expected.tab_id)
            .cloned()
            .ok_or(BrokerError::DeadTarget)?;
        let already = state
            .tabs
            .get(&expected.tab_id)
            .is_some_and(|row| row.info.borrowed && row.disconnect.is_none());
        let acquired = Self::attach_existing_locked(
            &mut state,
            TabAttachment {
                session,
                run_id: run,
                tab_id: &expected.tab_id,
                title: &offer.title,
                url: &offer.url,
                origin: &offer.origin,
                extension_id: offer.extension_id.as_deref(),
                pairing_token: Some(&offer.pairing_token),
                home_index: None,
                document_generation: Some(offer.document_generation),
                connection_generation: Some(offer.connection_generation),
                focused: offer.focused,
            },
        )?;
        Ok((acquired, !already))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_authority_callback_retains_registry_lock_until_commit_returns() {
        // Pure metadata contract; no browser or user permission is involved.
        let host = ExistingTabHost::new();
        host.with_existing_scope_authority(&[], || {
            assert!(host.inner.try_lock().is_none());
            Ok(())
        })
        .unwrap();
        assert!(host.inner.try_lock().is_some());
    }
}
