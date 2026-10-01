//! ExistingTabHost: owned managed pages and borrowed existing tabs share one registry.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::error::BrokerError;
use crate::pairing::ExtensionPairing;

use super::action_ledger;
use super::host_allowlist;
use super::types::*;
use super::worker::ManagedBrowserWorker;

pub(crate) struct TabWriteIdentity {
    pub page_id: String,
    pub page_generation: u64,
    pub snapshot_id: Option<String>,
    pub refs: HashSet<String>,
}

struct HomeSnapshot {
    index: Option<u32>,
    url: String,
    focused: bool,
}

struct TabRecord {
    info: TabInfo,
    origin: String,
    extension_identity: Option<String>,
    pairing_token: Option<String>,
    profile: Option<String>,
    worker_page_id: Option<String>,
    home: Option<HomeSnapshot>,
    user_navigated: bool,
    disconnect: Option<TabDisconnect>,
    current_observation: Option<host_allowlist::HostObservation>,
}

#[allow(dead_code)]
#[derive(Clone)]
struct SharedOffer {
    picker_token: String,
    title: String,
    url: String,
    browser_id: String,
    profile_id: String,
    origin: String,
    extension_id: Option<String>,
    pairing_token: String,
    document_generation: u64,
    connection_generation: u64,
    focused: bool,
}

struct Inner {
    tabs: HashMap<String, TabRecord>,
    profiles: HashMap<String, String>,
    profile_primary_tabs: HashMap<String, String>,
    opening_profiles: HashMap<String, String>,
    stopped_runs: HashSet<String>,
    clearing_profiles: HashMap<String, ProfileClearState>,
    managed_runs: HashMap<String, managed_lifecycle::ManagedRun>,
    installed_extension_id: String,
    staging_root: std::path::PathBuf,
    profile_root: std::path::PathBuf,
    pairing: ExtensionPairing,
    shared: HashMap<String, SharedOffer>,
    shared_sequence: u64,
    extension_requests: std::collections::VecDeque<transport::PendingExtension>,
    extension_sequence: u64,
    extension_completions: super::extension_completion::CompletionRegistry,
    browser_session_id: Option<String>,
    extension_actions: std::collections::VecDeque<actions::PendingAction>,
    action_negotiation: Option<super::extension_action::ActionNegotiation>,
    ledgers: HashMap<String, action_ledger::ActionLedger>,
}

pub struct ExistingTabHost {
    inner: Arc<Mutex<Inner>>,
    worker: Mutex<Option<std::sync::Arc<dyn ManagedBrowserWorker>>>,
    request: Option<super::ManagedRequest>,
}

#[derive(Debug, Clone, Copy)]
pub struct TabAttachment<'a> {
    pub session: &'a str,
    pub run_id: &'a str,
    pub tab_id: &'a str,
    pub title: &'a str,
    pub url: &'a str,
    pub origin: &'a str,
    pub extension_id: Option<&'a str>,
    pub pairing_token: Option<&'a str>,
    pub home_index: Option<u32>,
    pub document_generation: Option<u64>,
    pub connection_generation: Option<u64>,
    pub focused: bool,
}

impl Default for ExistingTabHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ExistingTabHost {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                tabs: HashMap::new(),
                profiles: HashMap::new(),
                profile_primary_tabs: HashMap::new(),
                opening_profiles: HashMap::new(),
                stopped_runs: HashSet::new(),
                clearing_profiles: HashMap::new(),
                managed_runs: HashMap::new(),
                installed_extension_id: crate::pairing::EXTENSION_ID.into(),
                staging_root: std::env::temp_dir(),
                profile_root: std::env::temp_dir().join("grok-cu-browser-profiles"),
                pairing: ExtensionPairing::new(
                    uuid::Uuid::new_v4().to_string(),
                    crate::pairing::EXTENSION_ID,
                ),
                shared: HashMap::new(),
                shared_sequence: 0,
                extension_requests: Default::default(),
                extension_sequence: 0,
                extension_completions: Default::default(),
                browser_session_id: None,
                extension_actions: Default::default(),
                action_negotiation: None,
                ledgers: HashMap::new(),
            })),
            worker: Mutex::new(None),
            request: None,
        }
    }

    pub fn set_worker(&self, worker: std::sync::Arc<dyn ManagedBrowserWorker>) {
        *self.worker.lock() = Some(worker);
    }

    pub fn worker_attached(&self) -> bool {
        self.worker.lock().is_some()
    }

    pub(super) fn worker(&self) -> Result<std::sync::Arc<dyn ManagedBrowserWorker>, BrokerError> {
        if let Some(request) = &self.request {
            self.check_request_locked(&self.inner.lock(), request.identity.owner())?;
        }
        self.worker
            .lock()
            .clone()
            .ok_or_else(|| BrokerError::Schema("managed browser worker is unavailable".into()))
    }

    pub fn set_staging_root(&self, root: std::path::PathBuf) {
        self.inner.lock().staging_root = root;
    }

    pub fn set_profile_root(&self, root: std::path::PathBuf) {
        self.inner.lock().profile_root = root;
    }

    pub fn profile_root(&self) -> std::path::PathBuf {
        self.inner.lock().profile_root.clone()
    }
}

fn rec_ledger_begin(
    ledgers: &mut HashMap<String, action_ledger::ActionLedger>,
    run_id: &str,
    action_id: &str,
    fingerprint: [u8; 32],
) -> action_ledger::LedgerOutcome {
    ledgers
        .entry(run_id.to_string())
        .or_default()
        .begin(action_id, fingerprint)
}

fn managed_tab_id(profile: &str, page_id: &str) -> String {
    format!("managed:{profile}:{page_id}")
}

fn apply_managed_worker_page(rec: &mut TabRecord, page: &ManagedPage, apply_url: bool) {
    if !page.page_id.is_empty() {
        rec.worker_page_id = Some(page.page_id.clone());
    }
    if page.page_generation > 0 && page.page_generation != rec.info.document_generation {
        rec.current_observation = None;
        rec.info.document_generation = page.page_generation;
    } else if page.page_generation > 0 {
        rec.info.document_generation = page.page_generation;
    }
    if apply_url && !page.url.is_empty() {
        rec.info.url = page.url.clone();
    }
}

mod actions;
mod completion;
mod existing;
mod extension_state;
mod managed;
mod managed_lifecycle;
mod managed_profiles;
use managed_profiles::ProfileClearState;
mod shared;
mod transport;
