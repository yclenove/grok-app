//! Profile opening owns a reservation until validation/publication finishes.
//! Ownership survives uncertain replies so Stop can still close remote resources.
use super::super::types::*;
use super::super::validation::{validate_managed_page, validate_profile_name};
use super::{managed_tab_id, ExistingTabHost, TabRecord};
use crate::error::BrokerError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ProfileClearState {
    InFlight,
    Pending,
}

struct ProfileClearing<'a> {
    host: &'a ExistingTabHost,
    profile: &'a str,
    completed: bool,
}

impl Drop for ProfileClearing<'_> {
    fn drop(&mut self) {
        let mut g = self.host.inner.lock();
        if self.completed {
            g.clearing_profiles.remove(self.profile);
        } else {
            g.clearing_profiles
                .insert(self.profile.to_string(), ProfileClearState::Pending);
        }
    }
}

struct ProfileOpening<'a> {
    host: &'a ExistingTabHost,
    profile: &'a str,
    run: &'a str,
    new_owner: bool,
    dispatched: bool,
}

impl Drop for ProfileOpening<'_> {
    fn drop(&mut self) {
        let mut g = self.host.inner.lock();
        if g.opening_profiles
            .get(self.profile)
            .is_some_and(|owner| owner == self.run)
        {
            g.opening_profiles.remove(self.profile);
        }
        // Local setup failed before a worker request. After dispatch, even an
        // invalid response may have left a real browser that needs cleanup.
        if self.new_owner
            && !self.dispatched
            && g.profiles
                .get(self.profile)
                .is_some_and(|owner| owner == self.run)
        {
            g.profiles.remove(self.profile);
        }
    }
}

impl ExistingTabHost {
    pub fn open_managed_profile(
        &self,
        session: &str,
        run_id: &str,
        profile: &str,
    ) -> Result<TabInfo, BrokerError> {
        let profile = validate_profile_name(profile)?;
        let worker = self.worker()?;
        let (profile_root, new_owner) = {
            let mut g = self.inner.lock();
            self.check_request_locked(&g, run_id)?;
            if g.stopped_runs.contains(run_id) {
                return Err(BrokerError::StopRequested);
            }
            if g.profiles
                .get(&profile)
                .is_some_and(|owner| owner != run_id)
            {
                return Err(BrokerError::Schema("profile owned by another run".into()));
            }
            if g.opening_profiles.contains_key(&profile) {
                return Err(BrokerError::Schema("profile is currently opening".into()));
            }
            if g.clearing_profiles.contains_key(&profile) {
                return Err(BrokerError::Schema(
                    "managed profile cleanup is pending".into(),
                ));
            }
            let new_owner = g
                .profiles
                .insert(profile.clone(), run_id.to_string())
                .is_none();
            g.opening_profiles
                .insert(profile.clone(), run_id.to_string());
            (g.profile_root.clone(), new_owner)
        };
        let mut opening = ProfileOpening {
            host: self,
            profile: &profile,
            run: run_id,
            new_owner,
            dispatched: false,
        };
        std::fs::create_dir_all(&profile_root).map_err(|e| BrokerError::Adapter(e.to_string()))?;
        if self.inner.lock().stopped_runs.contains(run_id) {
            return Err(BrokerError::StopRequested);
        }
        opening.dispatched = true;
        let opened = worker
            .open_profile(run_id, &profile)
            .map_err(BrokerError::BrowserWorker)?;
        if !opened.dir.is_dir() {
            return Err(BrokerError::Adapter(
                "managed worker did not create a profile directory".into(),
            ));
        }
        let root = std::fs::canonicalize(&profile_root)
            .map_err(|e| BrokerError::Adapter(format!("profile root unavailable: {e}")))?;
        let actual = std::fs::canonicalize(&opened.dir)
            .map_err(|e| BrokerError::Adapter(format!("managed profile unavailable: {e}")))?;
        let expected = std::fs::canonicalize(profile_root.join(&profile))
            .map_err(|e| BrokerError::Adapter(format!("expected profile unavailable: {e}")))?;
        if !crate::staging::path_is_under(&actual, &root)
            || actual.file_name().and_then(|s| s.to_str()) != Some(profile.as_str())
            || actual != expected
        {
            return Err(BrokerError::Adapter(
                "managed worker returned a profile outside the Host root".into(),
            ));
        }
        validate_managed_page(&opened.page)?;
        let mut g = self.inner.lock();
        self.check_request_locked(&g, run_id)?;
        if g.stopped_runs.contains(run_id) {
            return Err(BrokerError::StopRequested);
        }
        if g.opening_profiles
            .get(&profile)
            .is_none_or(|owner| owner != run_id)
            || g.profiles.get(&profile).is_none_or(|owner| owner != run_id)
        {
            return Err(BrokerError::IdentityMismatch("managed profile reservation"));
        }
        let tab_id = managed_tab_id(&profile, &opened.page.page_id);
        let rec = TabRecord {
            info: TabInfo {
                tab_id: tab_id.clone(),
                run_id: run_id.to_string(),
                session: session.to_string(),
                title: "managed".into(),
                url: opened.page.url.clone(),
                user_owned: false,
                borrowed: false,
                closed: false,
                generation: 1,
                home_index: None,
                document_generation: opened.page.page_generation,
                connection_generation: 1,
                focused: false,
                disconnect_reason: None,
                preview_generation: 0,
            },
            origin: "https://managed.local".into(),
            extension_identity: None,
            pairing_token: None,
            profile: Some(profile.clone()),
            worker_page_id: Some(opened.page.page_id),
            home: None,
            user_navigated: false,
            disconnect: None,
            current_observation: None,
        };
        let info = rec.info.clone();
        g.profile_primary_tabs
            .insert(profile.clone(), tab_id.clone());
        g.tabs.insert(tab_id, rec);
        Ok(info)
    }

    /// Host/user only. Keep the name reserved through remote close and local
    /// removal. A failed clear needs an explicit retry, not a new open request.
    pub fn clear_managed_profile(&self, profile: &str) -> Result<(), BrokerError> {
        let profile = validate_profile_name(profile)?;
        let fallback_worker = self.worker()?;
        let (profile_root, worker) = {
            let mut g = self.inner.lock();
            if g.opening_profiles.contains_key(&profile)
                || g.clearing_profiles.get(&profile) == Some(&ProfileClearState::InFlight)
            {
                return Err(BrokerError::Schema(
                    "managed profile transition is in progress".into(),
                ));
            }
            let worker = g
                .profiles
                .get(&profile)
                .and_then(|owner| Self::managed_worker_locked(&g, owner))
                .unwrap_or(fallback_worker);
            g.clearing_profiles
                .insert(profile.clone(), ProfileClearState::InFlight);
            for rec in g
                .tabs
                .values_mut()
                .filter(|rec| rec.profile.as_deref() == Some(&profile))
            {
                rec.info.generation = rec.info.generation.saturating_add(1);
                rec.disconnect = Some(TabDisconnect::TabClose);
                rec.info.disconnect_reason = Some(TabDisconnect::TabClose.as_str().into());
                rec.current_observation = None;
                rec.info.preview_generation = 0;
            }
            (g.profile_root.clone(), worker)
        };
        let mut clearing = ProfileClearing {
            host: self,
            profile: &profile,
            completed: false,
        };
        worker
            .clear_profile(&profile)
            .map_err(BrokerError::BrowserWorker)?;
        let dir = profile_root.join(&profile);
        match std::fs::symlink_metadata(&dir) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(BrokerError::Adapter(error.to_string())),
            Ok(_) => {
                // Never delete outside the exact Host-owned profile root, even
                // if a stale path or filesystem link appeared during shutdown.
                let root = std::fs::canonicalize(&profile_root)
                    .map_err(|e| BrokerError::Adapter(e.to_string()))?;
                let actual =
                    std::fs::canonicalize(&dir).map_err(|e| BrokerError::Adapter(e.to_string()))?;
                if actual.parent() != Some(root.as_path())
                    || actual.file_name().and_then(|s| s.to_str()) != Some(profile.as_str())
                {
                    return Err(BrokerError::Adapter("profile cleanup path changed".into()));
                }
                std::fs::remove_dir_all(&dir).map_err(|e| BrokerError::Adapter(e.to_string()))?;
            }
        }
        let mut g = self.inner.lock();
        for rec in g
            .tabs
            .values_mut()
            .filter(|rec| rec.profile.as_deref() == Some(&profile))
        {
            rec.info.closed = true;
        }
        g.profiles.remove(&profile);
        g.profile_primary_tabs.remove(&profile);
        clearing.completed = true;
        Ok(())
    }
}
