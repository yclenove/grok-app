use crate::error::BrokerError;
#[cfg(any(test, feature = "test-support"))]
use crate::pairing::ExtensionPairing;
use crate::pairing::PairingChallenge;

use super::super::types::*;
#[cfg(any(test, feature = "test-support"))]
use super::SharedOffer;
use super::{ExistingTabHost, HomeSnapshot, TabAttachment, TabRecord, TabWriteIdentity};

pub(super) fn invalidate_shared_tabs(state: &mut super::Inner) {
    state.extension_requests.clear();
    state.extension_completions.cancel_run(None);
    state.action_negotiation = None;
    state.shared.clear();
    state.shared_sequence = 0;
    for record in state
        .tabs
        .values_mut()
        .filter(|record| record.info.user_owned)
    {
        record.info.borrowed = false;
        record.info.generation = record.info.generation.saturating_add(1);
        record.info.preview_generation = 0;
        record.info.disconnect_reason = Some(TabDisconnect::Disconnect.as_str().into());
        record.disconnect = Some(TabDisconnect::Disconnect);
        record.current_observation = None;
        record.pairing_token = None;
    }
    super::completion::sweep(state);
}

pub(super) fn expire_pairing(state: &mut super::Inner) -> bool {
    super::transport::sweep_requests(state);
    super::completion::sweep(state);
    if !state.pairing.expire_connection() {
        return false;
    }
    invalidate_shared_tabs(state);
    true
}

impl ExistingTabHost {
    pub fn expire_pairing_connection(&self) -> bool {
        expire_pairing(&mut self.inner.lock())
    }

    #[cfg(test)]
    pub fn expire_pairing_connection_for_test(&self) {
        self.inner.lock().pairing.expire_connection_for_test();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn has_stored_pairing_connection_for_test(&self) -> bool {
        self.inner.lock().pairing.has_stored_connection_for_test()
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_installed_extension_id(&self, id: &str) {
        let mut g = self.inner.lock();
        invalidate_shared_tabs(&mut g);
        g.installed_extension_id = id.to_string();
        g.pairing.set_installed_extension_id(id);
    }

    pub fn installed_extension_id(&self) -> String {
        self.inner.lock().installed_extension_id.clone()
    }

    pub fn begin_pairing_challenge(&self) -> PairingChallenge {
        let mut state = self.inner.lock();
        invalidate_shared_tabs(&mut state);
        state.pairing.begin_challenge()
    }

    pub fn pending_pairing_challenge(&self) -> Option<PairingChallenge> {
        self.inner.lock().pairing.pending_challenge()
    }

    pub fn public_pairing_challenge(&self) -> Option<crate::pairing::PublicPairingChallenge> {
        self.inner.lock().pairing.public_pending()
    }

    #[cfg(test)]
    pub fn expire_pending_pairing(&self) {
        self.inner.lock().pairing.expire_pending();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn pairing_session_key(&self) -> Option<String> {
        self.inner
            .lock()
            .pairing
            .session_key()
            .map(|s| s.to_string())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn confirm_pairing_app(&self) -> Result<(), String> {
        self.inner.lock().pairing.confirm_app()
    }

    pub fn confirm_pairing_app_for(&self, nonce: &str) -> Result<(), String> {
        self.inner.lock().pairing.confirm_app_for(nonce)
    }

    pub fn complete_pairing_request(
        &self,
        proof: &crate::pairing::PairingProof,
    ) -> Result<crate::pairing::PairingSession, String> {
        self.inner.lock().pairing.complete_request(proof)
    }

    pub fn check_pairing_connection(
        &self,
        origin: &str,
        token: &str,
        connection: &crate::pairing::PairingConnection,
        disconnect: bool,
    ) -> Result<(), String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        if disconnect {
            state
                .pairing
                .authenticate_connection(origin, token, connection)?;
            state.pairing.revoke();
            invalidate_shared_tabs(&mut state);
        } else {
            state
                .pairing
                .heartbeat_connection(origin, token, connection)?;
        }
        Ok(())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn complete_pairing(&self, response: &str) -> Result<String, String> {
        self.inner.lock().pairing.complete(response)
    }

    pub fn revoke_pairing(&self) {
        let mut state = self.inner.lock();
        state.pairing.revoke();
        invalidate_shared_tabs(&mut state);
    }

    pub fn rotate_pairing(&self) -> Result<PairingChallenge, String> {
        let mut state = self.inner.lock();
        let challenge = state.pairing.rotate()?;
        invalidate_shared_tabs(&mut state);
        Ok(challenge)
    }

    /// Fixture helper only. Product pairing requires an extension possession proof.
    #[cfg(any(test, feature = "test-support"))]
    pub fn handshake_pairing(&self) -> Result<String, String> {
        let ch = self.begin_pairing_challenge();
        self.confirm_pairing_app()?;
        self.complete_pairing(&ExtensionPairing::extension_response(&ch))
    }

    /// Extension reports a tab the user explicitly shared. Not a model list.
    #[cfg(any(test, feature = "test-support"))]
    pub fn offer_shared_tab(&self, offer: SharedTabOffer<'_>) -> Result<(), String> {
        let SharedTabOffer {
            pairing_token,
            origin,
            extension_id,
            tab_id,
            title,
            url,
            browser_id,
            profile_id,
            document_generation,
            connection_generation,
            focused,
        } = offer;
        super::shared::validate_offer(&offer)?;
        let mut g = self.inner.lock();
        g.pairing
            .connect(origin, extension_id, Some(pairing_token))?;
        g.shared.insert(
            tab_id.to_string(),
            SharedOffer {
                picker_token: uuid::Uuid::new_v4().to_string(),
                title: title.to_string(),
                url: url.to_string(),
                browser_id: browser_id.to_string(),
                profile_id: profile_id.to_string(),
                origin: origin.to_string(),
                extension_id: extension_id.map(|s| s.to_string()),
                pairing_token: pairing_token.to_string(),
                document_generation,
                connection_generation,
                focused,
            },
        );
        Ok(())
    }

    /// Run-scoped picker grant for a user-shared candidate. Production Host API.
    pub fn grant_picker_tab(
        &self,
        session: &str,
        run_id: &str,
        selector: &str,
    ) -> Result<TabInfo, BrokerError> {
        self.acquire_picker_tab(session, run_id, selector)
            .map(|(info, _)| info)
    }

    /// Returns whether this call created the grant, under the same registry lock.
    pub(crate) fn acquire_picker_tab(
        &self,
        session: &str,
        run_id: &str,
        selector: &str,
    ) -> Result<(TabInfo, bool), BrokerError> {
        let (tab_id, picker_token) = selector
            .rsplit_once('@')
            .ok_or(BrokerError::IdentityMismatch("shared candidate changed"))?;
        let mut g = self.inner.lock();
        expire_pairing(&mut g);
        let offer = g
            .shared
            .get(tab_id)
            .cloned()
            .ok_or_else(|| BrokerError::Schema("shared tab is not in the picker".into()))?;
        if offer.picker_token != picker_token {
            return Err(BrokerError::IdentityMismatch("shared candidate changed"));
        }
        let ext = g.installed_extension_id.clone();
        let ext_ref = offer.extension_id.as_deref().or(Some(ext.as_str()));
        let already_borrowed = g
            .tabs
            .get(tab_id)
            .is_some_and(|record| record.info.borrowed && record.disconnect.is_none());
        let info = Self::attach_existing_locked(
            &mut g,
            TabAttachment {
                session,
                run_id,
                tab_id,
                title: &offer.title,
                url: &offer.url,
                origin: &offer.origin,
                extension_id: ext_ref,
                pairing_token: Some(&offer.pairing_token),
                home_index: None,
                document_generation: Some(offer.document_generation),
                connection_generation: Some(offer.connection_generation),
                focused: offer.focused,
            },
        )?;
        Ok((info, !already_borrowed))
    }

    /// Host picker list. Model tools must not call this.
    pub fn list_shared_candidates(&self) -> Vec<(String, String, String)> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .shared
            .iter()
            .map(|(id, o)| {
                (
                    format!("{id}@{}", o.picker_token),
                    o.title.clone(),
                    o.url.clone(),
                )
            })
            .collect()
    }

    /// App picker issues a run-scoped grant. Document and connection generation are required.
    pub fn picker_grant(&self, attachment: TabAttachment<'_>) -> Result<TabInfo, BrokerError> {
        let document_generation = attachment.document_generation.unwrap_or(0);
        let connection_generation = attachment.connection_generation.unwrap_or(0);
        if document_generation == 0 || connection_generation == 0 {
            return Err(BrokerError::Schema(
                "picker grant requires document and connection generation".into(),
            ));
        }
        if attachment.pairing_token.is_none() {
            return Err(BrokerError::Schema(
                "picker grant requires a pairing token".into(),
            ));
        }
        self.attach_existing(attachment)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn share_and_grant(&self, attachment: TabAttachment<'_>) -> Result<TabInfo, BrokerError> {
        let pairing_token = attachment
            .pairing_token
            .ok_or_else(|| BrokerError::Schema("pairing token required".into()))?;
        self.offer_shared_tab(SharedTabOffer {
            pairing_token,
            origin: attachment.origin,
            extension_id: attachment.extension_id,
            tab_id: attachment.tab_id,
            title: attachment.title,
            url: attachment.url,
            browser_id: "chrome",
            profile_id: "default",
            document_generation: attachment.document_generation.unwrap_or(0),
            connection_generation: attachment.connection_generation.unwrap_or(0),
            focused: attachment.focused,
        })
        .map_err(BrokerError::Schema)?;
        self.picker_grant(attachment)
    }

    pub fn attach_existing(&self, attachment: TabAttachment<'_>) -> Result<TabInfo, BrokerError> {
        Self::attach_existing_locked(&mut self.inner.lock(), attachment)
    }

    fn attach_existing_locked(
        g: &mut super::Inner,
        attachment: TabAttachment<'_>,
    ) -> Result<TabInfo, BrokerError> {
        let TabAttachment {
            session,
            run_id,
            tab_id,
            title,
            url,
            origin,
            extension_id: claimed_extension_id,
            pairing_token,
            home_index,
            document_generation,
            connection_generation,
            focused,
        } = attachment;
        expire_pairing(g);
        if g.stopped_runs.contains(run_id) {
            return Err(BrokerError::StopRequested);
        }
        if super::actions::has_tab(g, tab_id) {
            return Err(BrokerError::Schema(
                "existing action is still settling".into(),
            ));
        }
        g.pairing
            .connect(origin, claimed_extension_id, pairing_token)
            .map_err(BrokerError::Schema)?;
        let doc_gen = document_generation.unwrap_or(0);
        let conn_gen = connection_generation.unwrap_or(0);
        if doc_gen == 0 || conn_gen == 0 {
            return Err(BrokerError::Schema(
                "picker grant requires document and connection generation".into(),
            ));
        }
        let (title, url) = {
            let shared = g
                .shared
                .get(tab_id)
                .ok_or_else(|| BrokerError::Schema("tab was not shared by the user".into()))?;
            if shared.document_generation != doc_gen
                || shared.connection_generation != conn_gen
                || Some(shared.pairing_token.as_str()) != pairing_token
            {
                return Err(BrokerError::IdentityMismatch("shared candidate changed"));
            }
            let title = if title.is_empty() {
                shared.title.clone()
            } else {
                title.to_string()
            };
            let url = if url.is_empty() {
                shared.url.clone()
            } else {
                url.to_string()
            };
            (title, url)
        };
        if let Some(existing) = g.tabs.get(tab_id) {
            if (existing.info.run_id != run_id || existing.info.session != session)
                && existing.info.borrowed
            {
                return Err(BrokerError::Schema(
                    "tab is bound to another session/run; no global current tab".into(),
                ));
            }
            if existing.info.closed {
                return Err(BrokerError::DeadTarget);
            }
            if existing.info.borrowed && existing.disconnect.is_none() {
                return Ok(existing.info.clone());
            }
        }
        if g.tabs.contains_key(tab_id) {
            let rec = g
                .tabs
                .get_mut(tab_id)
                .ok_or(BrokerError::TargetUnauthorized)?;
            rec.info.title = title;
            rec.info.session = session.to_string();
            rec.info.run_id = run_id.to_string();
            rec.info.url = url.clone();
            rec.info.user_owned = true;
            rec.info.borrowed = true;
            rec.info.home_index = home_index;
            rec.info.document_generation = doc_gen;
            rec.info.connection_generation = conn_gen;
            rec.info.focused = focused;
            rec.info.disconnect_reason = None;
            rec.info.generation = rec.info.generation.saturating_add(1);
            rec.info.preview_generation = rec.info.preview_generation.max(1);
            rec.origin = origin.to_string();
            rec.extension_identity = claimed_extension_id.map(|s| s.to_string());
            rec.pairing_token = pairing_token.map(|s| s.to_string());
            rec.home = Some(HomeSnapshot {
                index: home_index,
                url: url.clone(),
                focused,
            });
            rec.user_navigated = false;
            rec.disconnect = None;
            rec.current_observation = None;
            return Ok(rec.info.clone());
        }
        let rec = TabRecord {
            info: TabInfo {
                tab_id: tab_id.to_string(),
                run_id: run_id.to_string(),
                session: session.to_string(),
                title,
                url: url.clone(),
                user_owned: true,
                borrowed: true,
                closed: false,
                generation: 1,
                home_index,
                document_generation: doc_gen,
                connection_generation: conn_gen,
                focused,
                disconnect_reason: None,
                preview_generation: 1,
            },
            origin: origin.to_string(),
            extension_identity: claimed_extension_id.map(|s| s.to_string()),
            pairing_token: pairing_token.map(|s| s.to_string()),
            profile: None,
            worker_page_id: Some(tab_id.to_string()),
            home: Some(HomeSnapshot {
                index: home_index,
                url: url.clone(),
                focused,
            }),
            user_navigated: false,
            disconnect: None,
            current_observation: None,
        };
        let info = rec.info.clone();
        g.tabs.insert(tab_id.to_string(), rec);
        Ok(info)
    }

    pub fn list_for_run(&self, run_id: &str) -> Vec<TabInfo> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .tabs
            .values()
            .filter(|t| {
                t.info.run_id == run_id
                    && !t.info.closed
                    && t.disconnect.is_none()
                    && (!t.info.user_owned || t.info.borrowed)
            })
            .map(|t| t.info.clone())
            .collect()
    }

    pub(super) fn require_live_grant(rec: &TabRecord, run_id: &str) -> Result<(), BrokerError> {
        if rec.info.run_id != run_id {
            return Err(BrokerError::Schema(
                "tab is bound to another session/run; no global current tab".into(),
            ));
        }
        if rec.info.closed {
            return Err(BrokerError::DeadTarget);
        }
        if rec.disconnect.is_some() {
            return Err(BrokerError::Schema(
                "tab disconnected; reconnect with a fresh identity proof".into(),
            ));
        }
        if rec.info.user_owned && !rec.info.borrowed {
            return Err(BrokerError::TargetUnauthorized);
        }
        Ok(())
    }

    pub(crate) fn tab_write_identity(
        &self,
        run_id: &str,
        tab_id: &str,
    ) -> Result<TabWriteIdentity, BrokerError> {
        let g = self.inner.lock();
        let rec = g.tabs.get(tab_id).ok_or(BrokerError::TargetUnauthorized)?;
        Self::require_live_grant(rec, run_id)?;
        if rec.info.user_owned {
            g.pairing
                .connect(
                    &rec.origin,
                    rec.extension_identity.as_deref(),
                    rec.pairing_token.as_deref(),
                )
                .map_err(BrokerError::Schema)?;
        }
        if rec.info.closed || rec.disconnect.is_some() {
            return Err(BrokerError::DeadTarget);
        }
        Ok(TabWriteIdentity {
            page_id: rec.worker_page_id.clone().unwrap_or_default(),
            page_generation: rec.info.document_generation.max(1),
            snapshot_id: rec
                .current_observation
                .as_ref()
                .map(|obs| obs.snapshot_id.clone()),
            refs: rec
                .current_observation
                .as_ref()
                .map(|obs| obs.refs.clone())
                .unwrap_or_default(),
        })
    }

    pub fn observe(&self, run_id: &str, tab_id: &str) -> Result<TabInfo, BrokerError> {
        let g = self.inner.lock();
        let rec = g.tabs.get(tab_id).ok_or(BrokerError::TargetUnauthorized)?;
        Self::require_live_grant(rec, run_id)?;
        if rec.info.user_owned {
            g.pairing
                .connect(
                    &rec.origin,
                    rec.extension_identity.as_deref(),
                    rec.pairing_token.as_deref(),
                )
                .map_err(BrokerError::Schema)?;
        }
        Ok(rec.info.clone())
    }

    pub fn observe_with_document(
        &self,
        run_id: &str,
        tab_id: &str,
        document_generation: u64,
    ) -> Result<TabInfo, BrokerError> {
        let info = self.observe(run_id, tab_id)?;
        if info.document_generation != document_generation {
            return Err(BrokerError::IdentityMismatch("stale document generation"));
        }
        Ok(info)
    }

    pub fn act(&self, run_id: &str, tab_id: &str) -> Result<TabInfo, BrokerError> {
        self.observe(run_id, tab_id)
    }

    pub fn act_with_document(
        &self,
        run_id: &str,
        tab_id: &str,
        document_generation: u64,
    ) -> Result<TabInfo, BrokerError> {
        self.observe_with_document(run_id, tab_id, document_generation)
    }

    /// Return a borrowed user tab to its original index. Does not close it.
    /// Does not overwrite a URL the user navigated to during the borrow.
    pub fn return_borrowed(&self, run_id: &str, tab_id: &str) -> Result<TabInfo, BrokerError> {
        let mut g = self.inner.lock();
        let rec = g
            .tabs
            .get_mut(tab_id)
            .ok_or(BrokerError::TargetUnauthorized)?;
        if rec.info.run_id != run_id {
            return Err(BrokerError::Schema(
                "tab is bound to another session/run; no global current tab".into(),
            ));
        }
        if !rec.info.user_owned {
            return Err(BrokerError::Schema(
                "only borrowed user tabs can be returned".into(),
            ));
        }
        if !rec.info.borrowed {
            return Err(BrokerError::Schema("tab is not borrowed".into()));
        }
        if rec.info.closed {
            return Err(BrokerError::DeadTarget);
        }
        if !rec.user_navigated {
            if let Some(home) = &rec.home {
                if rec.info.url != home.url {
                    rec.info.document_generation = rec.info.document_generation.saturating_add(1);
                }
                rec.info.url = home.url.clone();
                rec.info.home_index = home.index;
                rec.info.focused = home.focused;
            }
        }
        rec.info.borrowed = false;
        rec.info.disconnect_reason = None;
        rec.disconnect = None;
        rec.info.generation = rec.info.generation.saturating_add(1);
        rec.info.preview_generation = 0;
        rec.current_observation = None;
        let info = rec.info.clone();
        expire_pairing(&mut g);
        Ok(info)
    }

    /// Host-only: the user navigated this tab during a borrow. Return must keep this URL.
    pub fn note_user_navigation(&self, tab_id: &str, url: &str) -> Result<TabInfo, BrokerError> {
        let mut g = self.inner.lock();
        let rec = g
            .tabs
            .get_mut(tab_id)
            .ok_or(BrokerError::TargetUnauthorized)?;
        rec.user_navigated = true;
        rec.info.url = url.to_string();
        rec.info.document_generation = rec.info.document_generation.saturating_add(1);
        rec.info.generation = rec.info.generation.saturating_add(1);
        let info = rec.info.clone();
        expire_pairing(&mut g);
        Ok(info)
    }

    pub fn disconnect_tab(
        &self,
        tab_id: &str,
        reason: TabDisconnect,
    ) -> Result<TabInfo, BrokerError> {
        let mut g = self.inner.lock();
        let rec = g
            .tabs
            .get_mut(tab_id)
            .ok_or(BrokerError::TargetUnauthorized)?;
        rec.disconnect = Some(reason);
        rec.info.disconnect_reason = Some(reason.as_str().into());
        rec.info.borrowed = false;
        rec.info.generation = rec.info.generation.saturating_add(1);
        rec.info.preview_generation = 0;
        match reason {
            TabDisconnect::TabClose => rec.info.closed = true,
            TabDisconnect::Stop
            | TabDisconnect::Disconnect
            | TabDisconnect::ExtensionUpdate
            | TabDisconnect::BrowserExit
            | TabDisconnect::AppExit => {
                rec.info.closed = !rec.info.user_owned;
            }
        }
        let info = rec.info.clone();
        expire_pairing(&mut g);
        Ok(info)
    }

    pub fn reconnect_tab(
        &self,
        run_id: &str,
        tab_id: &str,
        document_generation: u64,
        connection_generation: u64,
    ) -> Result<TabInfo, BrokerError> {
        let mut g = self.inner.lock();
        expire_pairing(&mut g);
        if g.stopped_runs.contains(run_id) || super::actions::has_tab(&g, tab_id) {
            return Err(BrokerError::Schema(
                "existing action is still settling or stopped".into(),
            ));
        }
        {
            let rec = g.tabs.get(tab_id).ok_or(BrokerError::TargetUnauthorized)?;
            if rec.info.run_id != run_id {
                return Err(BrokerError::Schema(
                    "tab is bound to another session/run; no global current tab".into(),
                ));
            }
            if rec.info.closed {
                return Err(BrokerError::DeadTarget);
            }
            if rec.disconnect.is_none() {
                return Err(BrokerError::Schema("tab is not disconnected".into()));
            }
            if rec.info.user_owned {
                g.pairing
                    .connect(
                        &rec.origin,
                        rec.extension_identity.as_deref(),
                        rec.pairing_token.as_deref(),
                    )
                    .map_err(BrokerError::Schema)?;
            }
            if document_generation != rec.info.document_generation {
                return Err(BrokerError::IdentityMismatch("stale document generation"));
            }
            if connection_generation <= rec.info.connection_generation {
                return Err(BrokerError::IdentityMismatch("stale connection generation"));
            }
        }
        let rec = g
            .tabs
            .get_mut(tab_id)
            .ok_or(BrokerError::TargetUnauthorized)?;
        rec.info.connection_generation = connection_generation;
        rec.info.borrowed = rec.info.user_owned;
        rec.disconnect = None;
        rec.info.disconnect_reason = None;
        rec.info.generation = rec.info.generation.saturating_add(1);
        rec.info.preview_generation = 0;
        Ok(rec.info.clone())
    }

    /// Pause/takeover: abort the current write and revoke borrowed grants.
    /// App-owned profiles stay open.
    pub fn cancel_actions(&self, run_id: &str) -> Result<Vec<TabInfo>, BrokerError> {
        let mut g = self.inner.lock();
        g.extension_completions.cancel_run(Some(run_id));
        let mut out = Vec::new();
        for rec in g.tabs.values_mut() {
            if rec.info.run_id != run_id {
                continue;
            }
            rec.info.borrowed = false;
            rec.info.generation = rec.info.generation.saturating_add(1);
            rec.current_observation = None;
            rec.info.preview_generation = 0;
            out.push(rec.info.clone());
        }
        super::transport::sweep_requests(&mut g);
        super::completion::sweep(&mut g);
        Ok(out)
    }

    /// Cancel a run: close App-owned tabs only. User-owned tabs stay open.
    pub fn cancel_run(&self, run_id: &str) -> Result<Vec<TabInfo>, BrokerError> {
        let cleanup_required = {
            let mut g = self.inner.lock();
            // Fence before remote I/O. Failed cleanup must not leave a usable
            // local grant, nor may a late open response resurrect this owner.
            g.stopped_runs.insert(run_id.to_string());
            g.extension_completions.cancel_run(Some(run_id));
            for rec in g.tabs.values_mut().filter(|rec| rec.info.run_id == run_id) {
                rec.info.borrowed = false;
                if rec.disconnect != Some(TabDisconnect::Stop) {
                    rec.info.generation = rec.info.generation.saturating_add(1);
                }
                rec.disconnect = Some(TabDisconnect::Stop);
                rec.info.disconnect_reason = Some(TabDisconnect::Stop.as_str().into());
                rec.current_observation = None;
                rec.info.preview_generation = 0;
            }
            super::transport::sweep_requests(&mut g);
            super::completion::sweep(&mut g);
            g.managed_runs.contains_key(run_id)
                || g.profiles.values().any(|owner| owner == run_id)
                || g.opening_profiles.values().any(|owner| owner == run_id)
        };
        if cleanup_required {
            self.pinned_worker(run_id)?
                .cancel_run(run_id)
                .map_err(BrokerError::BrowserWorker)?;
        }
        let mut g = self.inner.lock();
        if g.opening_profiles.values().any(|owner| owner == run_id)
            || g.profiles.iter().any(|(profile, owner)| {
                owner == run_id
                    && g.clearing_profiles.get(profile) == Some(&super::ProfileClearState::InFlight)
            })
        {
            return Err(BrokerError::BrowserWorker(WorkerError::unknown(
                409,
                "run_cleanup_pending",
                "managed profile transition is still settling",
            )));
        }
        Self::confirm_managed_cleanup(&mut g, run_id);
        let mut out = Vec::new();
        for rec in g.tabs.values_mut() {
            if rec.info.run_id != run_id {
                continue;
            }
            if !rec.info.user_owned {
                rec.info.closed = true;
            }
            out.push(rec.info.clone());
        }
        g.profiles.retain(|_, owner| owner != run_id);
        let removed_tabs = g
            .tabs
            .values()
            .filter(|record| record.info.run_id == run_id)
            .map(|record| record.info.tab_id.clone())
            .collect::<std::collections::HashSet<_>>();
        g.profile_primary_tabs
            .retain(|_, tab_id| !removed_tabs.contains(tab_id));
        Ok(out)
    }

    pub fn is_closed(&self, tab_id: &str) -> bool {
        self.inner
            .lock()
            .tabs
            .get(tab_id)
            .map(|t| t.info.closed)
            .unwrap_or(true)
    }
}
