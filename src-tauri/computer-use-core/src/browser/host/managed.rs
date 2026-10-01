use crate::adapter::CaptureOptions;
use crate::error::BrokerError;

use super::super::action_ledger;
use super::super::host_allowlist;
use super::super::types::*;
use super::super::validation::*;
use super::{
    apply_managed_worker_page, managed_tab_id, rec_ledger_begin, ExistingTabHost, TabRecord,
};

impl ExistingTabHost {
    pub fn managed_target_info(&self, target_id: &str) -> Option<TabInfo> {
        let g = self.inner.lock();
        let record = g.tabs.get(target_id)?;
        (!record.info.user_owned
            && record.profile.is_some()
            && record.worker_page_id.is_some()
            && !record.info.closed
            && record.disconnect.is_none())
        .then(|| record.info.clone())
    }

    pub fn list_managed_for_run(&self, run_id: &str) -> Vec<TabInfo> {
        self.inner
            .lock()
            .tabs
            .values()
            .filter(|record| {
                record.info.run_id == run_id
                    && !record.info.user_owned
                    && record.profile.is_some()
                    && record.worker_page_id.is_some()
                    && !record.info.closed
                    && record.disconnect.is_none()
            })
            .map(|record| record.info.clone())
            .collect()
    }

    pub fn new_managed_tab(
        &self,
        run_id: &str,
        profile: &str,
        action_id: &str,
    ) -> Result<ManagedPage, BrokerError> {
        self.new_managed_tab_with_action(run_id, profile, action_id)
    }

    pub fn new_managed_tab_with_action(
        &self,
        run_id: &str,
        profile: &str,
        action_id: &str,
    ) -> Result<ManagedPage, BrokerError> {
        let profile = validate_profile_name(profile)?;
        validate_browser_action_id(action_id)?;
        self.require_profile_owner(run_id, &profile)?;
        let page = self
            .worker()?
            .new_page(run_id, &profile, action_id)
            .map_err(BrokerError::BrowserWorker)?;
        self.register_managed_page(run_id, &profile, &page)?;
        Ok(page)
    }

    pub fn list_managed_pages(
        &self,
        run_id: &str,
        profile: &str,
    ) -> Result<Vec<ManagedPage>, BrokerError> {
        let profile = validate_profile_name(profile)?;
        self.require_profile_owner(run_id, &profile)?;
        let pages = self
            .worker()?
            .list_pages(run_id, &profile)
            .map_err(BrokerError::BrowserWorker)?;
        self.sync_managed_pages(run_id, &profile, &pages)?;
        Ok(pages)
    }

    /// Reauthorization may follow a cancelled click that really navigated. Its
    /// old response cannot publish state, so refresh the exact page explicitly
    /// under the new user admission instead of retrying with a stale generation.
    pub(crate) fn refresh_managed_target(
        &self,
        run_id: &str,
        tab_id: &str,
    ) -> Result<(), BrokerError> {
        let (profile, _, _) = self.managed_tab_route(run_id, tab_id)?;
        self.list_managed_pages(run_id, &profile)?;
        self.managed_tab_route(run_id, tab_id).map(|_| ())
    }

    pub fn open_managed_popup(
        &self,
        run_id: &str,
        profile: &str,
        action_id: &str,
        element_ref: &str,
    ) -> Result<ManagedPage, BrokerError> {
        let profile = validate_profile_name(profile)?;
        self.require_profile_owner(run_id, &profile)?;
        let (tab_id, page_generation) = self.managed_primary_tab_identity(run_id, &profile)?;
        let snapshot_id = {
            let g = self.inner.lock();
            self.check_request_locked(&g, run_id)?;
            let rec = g.tabs.get(&tab_id).ok_or(BrokerError::TargetUnauthorized)?;
            rec.current_observation
                .as_ref()
                .map(|obs| obs.snapshot_id.clone())
                .unwrap_or_default()
        };
        self.act_managed_tab(
            run_id,
            &tab_id,
            ManagedTabAction {
                page_generation,
                snapshot_id: &snapshot_id,
                action_id,
                kind: "click",
                locator: ManagedLocator {
                    element_ref: Some(element_ref.to_string()),
                },
                params: serde_json::json!({}),
            },
        )?;
        self.list_managed_pages(run_id, &profile)?
            .into_iter()
            .find(|page| page.popup)
            .ok_or_else(|| BrokerError::Adapter("popup was not opened".into()))
    }

    pub fn list_managed_frames(
        &self,
        run_id: &str,
        profile: &str,
    ) -> Result<Vec<String>, BrokerError> {
        let profile = validate_profile_name(profile)?;
        self.require_profile_owner(run_id, &profile)?;
        let (page_id, page_generation) = self.managed_page_identity(run_id, &profile)?;
        self.worker()?
            .list_frames(run_id, &profile, &page_id, page_generation)
            .map_err(BrokerError::BrowserWorker)
    }

    pub fn observe_managed(
        &self,
        run_id: &str,
        profile: &str,
    ) -> Result<ManagedObservation, BrokerError> {
        let profile = validate_profile_name(profile)?;
        self.require_profile_owner(run_id, &profile)?;
        let (tab_id, page_generation) = self.managed_primary_tab_identity(run_id, &profile)?;
        self.observe_managed_tab(run_id, &tab_id, page_generation)
    }

    pub fn observe_managed_tab(
        &self,
        run_id: &str,
        tab_id: &str,
        page_generation: u64,
    ) -> Result<ManagedObservation, BrokerError> {
        self.capture_managed_tab(run_id, tab_id, page_generation, CaptureOptions::model(true))
    }

    pub fn capture_managed_tab(
        &self,
        run_id: &str,
        tab_id: &str,
        page_generation: u64,
        options: CaptureOptions,
    ) -> Result<ManagedObservation, BrokerError> {
        options.cancellation.check().map_err(BrokerError::Adapter)?;
        let (profile, page_id) = self.managed_tab_identity(run_id, tab_id, page_generation)?;
        let obs = self
            .worker()?
            .capture_page(run_id, &profile, &page_id, page_generation, options.clone())
            .map_err(BrokerError::BrowserWorker)?;
        options.cancellation.check().map_err(BrokerError::Adapter)?;
        if obs.page_id != page_id || obs.page_generation != page_generation {
            return Err(BrokerError::IdentityMismatch("managed pageId"));
        }
        // Validate the live grant and publish under one lock. A late worker
        // response must not restore refs after navigation/cancellation.
        let mut g = self.inner.lock();
        self.check_request_locked(&g, run_id)?;
        let rec = g
            .tabs
            .get_mut(tab_id)
            .ok_or(BrokerError::TargetUnauthorized)?;
        Self::require_live_grant(rec, run_id)?;
        if rec.info.user_owned
            || rec.worker_page_id.as_deref() != Some(page_id.as_str())
            || rec.info.document_generation != page_generation
        {
            return Err(BrokerError::IdentityMismatch(
                "managed observation generation",
            ));
        }
        options.cancellation.check().map_err(BrokerError::Adapter)?;
        if options.for_model {
            rec.info.url = obs.url.clone();
            rec.current_observation = Some(host_allowlist::HostObservation::from_managed(&obs));
        }
        Ok(obs)
    }

    pub fn act_managed(
        &self,
        run_id: &str,
        profile: &str,
        action_id: &str,
        kind: &str,
        locator: ManagedLocator,
        params: serde_json::Value,
    ) -> Result<(), BrokerError> {
        let profile = validate_profile_name(profile)?;
        let (tab_id, page_generation) = self.managed_primary_tab_identity(run_id, &profile)?;
        let snapshot_id = {
            let g = self.inner.lock();
            self.check_request_locked(&g, run_id)?;
            g.tabs
                .get(&tab_id)
                .and_then(|rec| rec.current_observation.as_ref())
                .map(|obs| obs.snapshot_id.clone())
                .unwrap_or_default()
        };
        self.act_managed_tab(
            run_id,
            &tab_id,
            ManagedTabAction {
                page_generation,
                snapshot_id: &snapshot_id,
                action_id,
                kind,
                locator,
                params,
            },
        )?;
        Ok(())
    }

    pub fn act_managed_tab(
        &self,
        run_id: &str,
        tab_id: &str,
        action: ManagedTabAction<'_>,
    ) -> Result<ManagedPage, BrokerError> {
        let ManagedTabAction {
            page_generation,
            snapshot_id,
            action_id,
            kind,
            locator,
            params,
        } = action;
        validate_browser_action_id(action_id)?;
        if let Some(msg) = forbidden_browser_act(kind, &params) {
            return Err(BrokerError::Schema(msg.into()));
        }
        if kind.eq_ignore_ascii_case("key") {
            let key = params
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| BrokerError::Schema("key required".into()))?;
            crate::protocol::normalize_key(key).map_err(BrokerError::Schema)?;
        }
        if page_generation == 0 {
            return Err(BrokerError::IdentityMismatch("managed page generation"));
        }
        let (profile, page_id, current_generation, dispatched, ledger_outcome) = {
            let mut g = self.inner.lock();
            self.check_request_locked(&g, run_id)?;
            let rec = g.tabs.get(tab_id).ok_or(BrokerError::TargetUnauthorized)?;
            Self::require_live_grant(rec, run_id)?;
            if rec.info.closed || rec.disconnect.is_some() {
                return Err(BrokerError::DeadTarget);
            }
            let profile = rec.profile.clone().ok_or(BrokerError::TargetUnauthorized)?;
            let page_id = rec
                .worker_page_id
                .clone()
                .ok_or(BrokerError::IdentityMismatch("managed pageId"))?;
            let current_generation = rec.info.document_generation.max(1);
            let snapshot = snapshot_id.to_string();
            host_allowlist::preflight_typed_action(
                kind,
                rec.current_observation.as_ref(),
                &page_id,
                page_generation,
                &snapshot,
                &locator,
                &params,
            )?;
            if locator.element_ref.is_none()
                && params.get("x").is_some()
                && params.get("y").is_some()
            {
                host_allowlist::preflight_coordinate(rec.current_observation.as_ref())?;
            }
            let dispatched = rec.current_observation.clone();
            let fingerprint = action_ledger::fingerprint(&action_ledger::canonical_semantics(
                kind,
                &page_id,
                page_generation,
                &snapshot,
                locator.element_ref.as_deref(),
                &params,
            ));
            let ledger_outcome = rec_ledger_begin(&mut g.ledgers, run_id, action_id, fingerprint);
            (
                profile,
                page_id,
                current_generation,
                dispatched,
                ledger_outcome,
            )
        };
        let _ = current_generation;
        match ledger_outcome {
            action_ledger::LedgerOutcome::Execute => {}
            action_ledger::LedgerOutcome::ReplayInFlight | action_ledger::LedgerOutcome::Busy => {
                return Err(BrokerError::LeaseHeld {
                    run_id: run_id.to_string(),
                });
            }
            action_ledger::LedgerOutcome::Conflict => {
                return Err(BrokerError::Schema("actionId fingerprint conflict".into()));
            }
            action_ledger::LedgerOutcome::Full => {
                return Err(BrokerError::Schema("action ledger is full".into()));
            }
            action_ledger::LedgerOutcome::ReplayTerminal(terminal) => {
                return match terminal {
                    action_ledger::LedgerTerminal::Done {
                        page_id,
                        page_generation,
                        url,
                        popup,
                    } => Ok(ManagedPage {
                        page_id,
                        page_generation,
                        url,
                        popup,
                    }),
                    action_ledger::LedgerTerminal::Rejected => {
                        Err(BrokerError::Schema("action was rejected".into()))
                    }
                    action_ledger::LedgerTerminal::Unknown => Err(BrokerError::Adapter(
                        "previous action outcome is unknown".into(),
                    )),
                };
            }
        }
        let snapshot_owned = dispatched
            .as_ref()
            .map(|obs| obs.snapshot_id.clone())
            .unwrap_or_default();
        let page = self
            .worker()?
            .act_page(ManagedWorkerAction {
                owner: run_id,
                profile: &profile,
                page: ManagedPageRef {
                    page_id: &page_id,
                    page_generation,
                },
                snapshot_id: &snapshot_owned,
                action_id,
                kind,
                locator: &locator,
                params: &params,
            })
            .map_err(|error| {
                let terminal = if error.completion == WorkerCompletion::NotStarted {
                    action_ledger::LedgerTerminal::Rejected
                } else {
                    action_ledger::LedgerTerminal::Unknown
                };
                self.finish_action(run_id, action_id, terminal);
                BrokerError::BrowserWorker(error)
            })?;
        if page.page_id != page_id {
            return Err(BrokerError::IdentityMismatch("managed pageId"));
        }
        if kind.eq_ignore_ascii_case("wait") && page.page_generation != page_generation {
            self.finish_action(run_id, action_id, action_ledger::LedgerTerminal::Rejected);
            return Err(BrokerError::IdentityMismatch("wait pageGeneration"));
        }
        {
            let g = self.inner.lock();
            self.check_request_locked(&g, run_id)?;
            let rec = g.tabs.get(tab_id).ok_or(BrokerError::TargetUnauthorized)?;
            if let Some(dispatched) = dispatched.as_ref() {
                if let Err(error) = host_allowlist::cas_identity_unchanged(
                    rec.current_observation.as_ref(),
                    dispatched,
                ) {
                    drop(g);
                    self.finish_action(run_id, action_id, action_ledger::LedgerTerminal::Unknown);
                    return Err(error);
                }
            }
        }
        if !kind.eq_ignore_ascii_case("wait") {
            self.apply_managed_tab_page(run_id, tab_id, &page, true)?;
            let mut g = self.inner.lock();
            self.check_request_locked(&g, run_id)?;
            if let Some(rec) = g.tabs.get_mut(tab_id) {
                rec.current_observation = None;
            }
        }
        self.finish_action(
            run_id,
            action_id,
            action_ledger::LedgerTerminal::Done {
                page_id: page.page_id.clone(),
                page_generation: page.page_generation,
                url: page.url.clone(),
                popup: page.popup,
            },
        );
        Ok(page)
    }

    fn finish_action(
        &self,
        run_id: &str,
        action_id: &str,
        terminal: action_ledger::LedgerTerminal,
    ) {
        let mut g = self.inner.lock();
        if let Some(ledger) = g.ledgers.get_mut(run_id) {
            ledger.finish(action_id, terminal);
        }
    }

    pub(crate) fn managed_primary_tab_identity(
        &self,
        run_id: &str,
        profile: &str,
    ) -> Result<(String, u64), BrokerError> {
        let g = self.inner.lock();
        self.check_request_locked(&g, run_id)?;
        let tab_id = g
            .profile_primary_tabs
            .get(profile)
            .ok_or(BrokerError::TargetUnauthorized)?;
        let rec = g
            .tabs
            .get(tab_id)
            .filter(|t| t.info.run_id == run_id && !t.info.closed)
            .ok_or(BrokerError::TargetUnauthorized)?;
        Ok((rec.info.tab_id.clone(), rec.info.document_generation.max(1)))
    }

    fn managed_page_identity(
        &self,
        run_id: &str,
        profile: &str,
    ) -> Result<(String, u64), BrokerError> {
        let (tab_id, generation) = self.managed_primary_tab_identity(run_id, profile)?;
        let (_, page_id) = self.managed_tab_identity(run_id, &tab_id, generation)?;
        Ok((page_id, generation))
    }

    fn managed_tab_identity(
        &self,
        run_id: &str,
        tab_id: &str,
        page_generation: u64,
    ) -> Result<(String, String), BrokerError> {
        let (profile, page_id, current_generation) = self.managed_tab_route(run_id, tab_id)?;
        if page_generation == 0 || current_generation != page_generation {
            return Err(BrokerError::IdentityMismatch("managed page generation"));
        }
        Ok((profile, page_id))
    }

    fn managed_tab_route(
        &self,
        run_id: &str,
        tab_id: &str,
    ) -> Result<(String, String, u64), BrokerError> {
        let g = self.inner.lock();
        self.check_request_locked(&g, run_id)?;
        let rec = g.tabs.get(tab_id).ok_or(BrokerError::TargetUnauthorized)?;
        Self::require_live_grant(rec, run_id)?;
        if rec.info.user_owned {
            return Err(BrokerError::Schema(
                "existing tab operations require the extension transport".into(),
            ));
        }
        if rec.profile.is_none() {
            return Err(BrokerError::Schema(
                "tab is not a managed browser page".into(),
            ));
        }
        Ok((
            rec.profile.clone().ok_or(BrokerError::TargetUnauthorized)?,
            rec.worker_page_id
                .clone()
                .ok_or(BrokerError::IdentityMismatch("managed pageId"))?,
            rec.info.document_generation.max(1),
        ))
    }

    fn apply_managed_tab_page(
        &self,
        run_id: &str,
        tab_id: &str,
        page: &ManagedPage,
        apply_url: bool,
    ) -> Result<(), BrokerError> {
        validate_managed_page(page)?;
        let mut g = self.inner.lock();
        self.check_request_locked(&g, run_id)?;
        if g.stopped_runs.contains(run_id) {
            return Err(BrokerError::StopRequested);
        }
        let rec = g
            .tabs
            .get_mut(tab_id)
            .ok_or(BrokerError::TargetUnauthorized)?;
        Self::require_live_grant(rec, run_id)?;
        if rec.info.user_owned || rec.worker_page_id.as_deref() != Some(page.page_id.as_str()) {
            return Err(BrokerError::IdentityMismatch("managed pageId"));
        }
        apply_managed_worker_page(rec, page, apply_url);
        Ok(())
    }

    fn register_managed_page(
        &self,
        run_id: &str,
        profile: &str,
        page: &ManagedPage,
    ) -> Result<TabInfo, BrokerError> {
        validate_managed_page(page)?;
        let mut g = self.inner.lock();
        self.check_request_locked(&g, run_id)?;
        if g.stopped_runs.contains(run_id) {
            return Err(BrokerError::StopRequested);
        }
        if g.clearing_profiles.contains_key(profile) {
            return Err(BrokerError::Schema(
                "managed profile cleanup is pending".into(),
            ));
        }
        match g.profiles.get(profile) {
            Some(owner) if owner == run_id => {}
            Some(_) => return Err(BrokerError::Schema("profile owned by another run".into())),
            None => return Err(BrokerError::TargetUnauthorized),
        }
        let session = g
            .tabs
            .values()
            .find(|record| {
                record.info.run_id == run_id && record.profile.as_deref() == Some(profile)
            })
            .map(|record| record.info.session.clone())
            .ok_or(BrokerError::TargetUnauthorized)?;
        let tab_id = managed_tab_id(profile, &page.page_id);
        if let Some(record) = g.tabs.get_mut(&tab_id) {
            if record.info.run_id != run_id || record.info.user_owned {
                return Err(BrokerError::IdentityMismatch("managed page ownership"));
            }
            record.info.closed = false;
            record.disconnect = None;
            record.info.disconnect_reason = None;
            apply_managed_worker_page(record, page, true);
            return Ok(record.info.clone());
        }
        let record = TabRecord {
            info: TabInfo {
                tab_id: tab_id.clone(),
                run_id: run_id.to_string(),
                session,
                title: if page.popup {
                    "managed popup"
                } else {
                    "managed"
                }
                .into(),
                url: page.url.clone(),
                user_owned: false,
                borrowed: false,
                closed: false,
                generation: 1,
                home_index: None,
                document_generation: page.page_generation,
                connection_generation: 1,
                focused: false,
                disconnect_reason: None,
                preview_generation: 0,
            },
            origin: "https://managed.local".into(),
            extension_identity: None,
            pairing_token: None,
            profile: Some(profile.to_string()),
            worker_page_id: Some(page.page_id.clone()),
            home: None,
            user_navigated: false,
            disconnect: None,
            current_observation: None,
        };
        let info = record.info.clone();
        g.tabs.insert(tab_id, record);
        Ok(info)
    }

    fn sync_managed_pages(
        &self,
        run_id: &str,
        profile: &str,
        pages: &[ManagedPage],
    ) -> Result<(), BrokerError> {
        let live = pages
            .iter()
            .map(|page| {
                self.register_managed_page(run_id, profile, page)?;
                Ok(page.page_id.as_str())
            })
            .collect::<Result<std::collections::HashSet<_>, BrokerError>>()?;
        let mut g = self.inner.lock();
        self.check_request_locked(&g, run_id)?;
        for record in g.tabs.values_mut().filter(|record| {
            record.info.run_id == run_id
                && record.profile.as_deref() == Some(profile)
                && !record.info.user_owned
        }) {
            if record
                .worker_page_id
                .as_deref()
                .is_some_and(|page_id| !live.contains(page_id))
            {
                record.info.closed = true;
                record.disconnect = Some(TabDisconnect::TabClose);
                record.info.disconnect_reason = Some(TabDisconnect::TabClose.as_str().into());
                record.info.generation = record.info.generation.saturating_add(1);
            }
        }
        Ok(())
    }

    fn require_profile_owner(&self, run_id: &str, profile: &str) -> Result<(), BrokerError> {
        let g = self.inner.lock();
        self.check_request_locked(&g, run_id)?;
        if g.stopped_runs.contains(run_id) {
            return Err(BrokerError::StopRequested);
        }
        if g.clearing_profiles.contains_key(profile) {
            return Err(BrokerError::Schema(
                "managed profile cleanup is pending".into(),
            ));
        }
        match g.profiles.get(profile) {
            Some(owner) if owner == run_id => Ok(()),
            Some(_) => Err(BrokerError::Schema("profile owned by another run".into())),
            None => Err(BrokerError::TargetUnauthorized),
        }
    }

    pub fn list_managed_profiles(&self) -> Vec<String> {
        let root = self.inner.lock().profile_root.clone();
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        if validate_profile_name(name).is_ok() {
                            names.push(name.to_string());
                        }
                    }
                }
            }
        }
        names.sort();
        names
    }

    pub fn navigate(
        &self,
        run_id: &str,
        tab_id: &str,
        url: &str,
        action_id: &str,
    ) -> Result<TabInfo, BrokerError> {
        let page_generation = {
            let g = self.inner.lock();
            self.check_request_locked(&g, run_id)?;
            let rec = g.tabs.get(tab_id).ok_or(BrokerError::TargetUnauthorized)?;
            Self::require_live_grant(rec, run_id)?;
            rec.info.document_generation.max(1)
        };
        self.navigate_with_action(run_id, tab_id, page_generation, action_id, url)
    }

    pub fn navigate_with_action(
        &self,
        run_id: &str,
        tab_id: &str,
        page_generation: u64,
        action_id: &str,
        url: &str,
    ) -> Result<TabInfo, BrokerError> {
        self.navigate_with_action_commit(run_id, tab_id, page_generation, action_id, url, || true)
    }

    pub fn navigate_with_action_commit(
        &self,
        run_id: &str,
        tab_id: &str,
        page_generation: u64,
        action_id: &str,
        url: &str,
        commit: impl FnOnce() -> bool,
    ) -> Result<TabInfo, BrokerError> {
        is_allowed_navigate_url(url)?;
        validate_browser_action_id(action_id)?;
        if page_generation == 0 {
            return Err(BrokerError::IdentityMismatch("managed page generation"));
        }
        let worker = self.worker()?;
        let (profile, page_id, _) = self.managed_tab_route(run_id, tab_id)?;
        let landed = worker
            .goto(run_id, &profile, &page_id, page_generation, action_id, url)
            .map_err(BrokerError::BrowserWorker)?;
        if landed.page_id != page_id || landed.page_generation < page_generation {
            return Err(BrokerError::IdentityMismatch("managed page identity"));
        }
        navigation_result_allowed(url, &landed.url)?;
        if !commit() {
            return Err(BrokerError::IdentityMismatch("observation generation"));
        }
        let mut g = self.inner.lock();
        self.check_request_locked(&g, run_id)?;
        let rec = g
            .tabs
            .get_mut(tab_id)
            .ok_or(BrokerError::TargetUnauthorized)?;
        if rec.info.run_id != run_id || rec.info.closed {
            return Err(BrokerError::DeadTarget);
        }
        if rec.info.user_owned {
            return Err(BrokerError::Schema(
                "existing tab navigation requires the extension transport".into(),
            ));
        }
        rec.info.generation = rec.info.generation.saturating_add(1);
        apply_managed_worker_page(rec, &landed, true);
        Ok(rec.info.clone())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stage_download_with_action(
        &self,
        run_id: &str,
        tab_id: &str,
        page_generation: u64,
        action_id: &str,
        snapshot_id: &str,
        element_ref: Option<&str>,
        filename: &str,
    ) -> Result<std::path::PathBuf, BrokerError> {
        let name = crate::staging::sanitize_filename(filename)?;
        validate_browser_action_id(action_id)?;
        if page_generation == 0 {
            return Err(BrokerError::IdentityMismatch("managed page generation"));
        }
        if snapshot_id.trim().is_empty() {
            return Err(BrokerError::Schema("snapshotId required".into()));
        }
        let element_ref = element_ref
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| BrokerError::Schema("elementRef required".into()))?;
        let worker = self.worker()?;
        let (profile, page_id, _) = self.managed_tab_route(run_id, tab_id)?;
        let downloaded = worker
            .download(
                run_id,
                &profile,
                &page_id,
                page_generation,
                snapshot_id,
                action_id,
                &name,
                Some(element_ref),
            )
            .map_err(BrokerError::BrowserWorker)?;
        if downloaded.page.page_id != page_id || downloaded.page.page_generation < page_generation {
            return Err(BrokerError::IdentityMismatch("managed page identity"));
        }
        {
            let mut g = self.inner.lock();
            self.check_request_locked(&g, run_id)?;
            if let Some(rec) = g.tabs.get_mut(tab_id) {
                if !rec.info.user_owned {
                    apply_managed_worker_page(rec, &downloaded.page, false);
                }
            }
        }
        let dest = downloaded.path;
        let staging_root = self.inner.lock().staging_root.clone();
        crate::staging::accept_staged_file(&dest, &staging_root).or_else(|_| {
            crate::staging::accept_staged_file(
                &dest,
                &staging_root.join("computer-use-staging").join(run_id),
            )
        })?;
        let written = dest
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if written != name {
            let _ = std::fs::remove_file(&dest);
            return Err(BrokerError::Schema(
                "download filename does not match staging name".into(),
            ));
        }
        Ok(dest)
    }

    /// Host/user only. `source` must already live under the run staging root.
    pub fn upload_managed(
        &self,
        run_id: &str,
        tab_id: &str,
        action_id: &str,
        element_ref: &str,
        source: &std::path::Path,
    ) -> Result<(), BrokerError> {
        validate_browser_action_id(action_id)?;
        let staging_root = self.inner.lock().staging_root.clone();
        crate::staging::accept_staged_file(source, &staging_root).or_else(|_| {
            crate::staging::accept_staged_file(
                source,
                &staging_root.join("computer-use-staging").join(run_id),
            )
        })?;
        if element_ref.trim().is_empty()
            || element_ref.contains("javascript:")
            || element_ref.contains("eval(")
        {
            return Err(BrokerError::Schema("illegal upload elementRef".into()));
        }
        let worker = self.worker()?;
        let (profile, page_id, page_generation, snapshot_id) = {
            let g = self.inner.lock();
            self.check_request_locked(&g, run_id)?;
            let rec = g.tabs.get(tab_id).ok_or(BrokerError::TargetUnauthorized)?;
            Self::require_live_grant(rec, run_id)?;
            (
                rec.profile
                    .clone()
                    .unwrap_or_else(|| rec.info.tab_id.clone()),
                rec.worker_page_id
                    .clone()
                    .unwrap_or_else(|| rec.info.tab_id.clone()),
                rec.info.document_generation.max(1),
                rec.current_observation
                    .as_ref()
                    .map(|obs| obs.snapshot_id.clone())
                    .unwrap_or_default(),
            )
        };
        worker
            .upload_file(ManagedWorkerUpload {
                owner: run_id,
                profile: &profile,
                page: ManagedPageRef {
                    page_id: &page_id,
                    page_generation,
                },
                snapshot_id: &snapshot_id,
                action_id,
                element_ref,
                source,
            })
            .map_err(BrokerError::BrowserWorker)
    }
}
