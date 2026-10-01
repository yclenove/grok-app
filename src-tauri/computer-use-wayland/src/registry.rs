//! Host-only run registry. A pending consent dialog already owns occupancy.
//! Each entry retains its granting reactor until the exact native and adapter
//! owners join. Neither a UI timeout nor a portal Closed signal frees the run.
use crate::{
    PortalAdapter, PortalHostSession, PortalInputPolicy, PortalOptions, PortalSession, SourceKind,
};
use grok_computer_use_core::native_parent::NativeParent;
use grok_computer_use_core::session_grants::AuthorizationTicket;
use grok_computer_use_core::{adapter::ComputerUseAdapter, protocol::JS_MAX_SAFE_INTEGER};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};
use tokio::sync::watch;

#[path = "registry_policy.rs"]
mod native_policy;
#[path = "registry_parent.rs"]
mod parent;
use parent::ParentPreparation;
pub(super) use parent::SelectionOwner;

/// An in-process Host capability, not a model-supplied run string. It is not
/// deserializable, and a previous selection cannot cancel a replacement run.
#[derive(Clone, Debug)]
pub struct PortalSelection {
    run_id: String,
    id: uuid::Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalSelectionState {
    Pending,
    Ready {
        target_id: String,
    },
    Closing,
    /// All local owners joined. A reason may report denial/remote cleanup loss;
    /// this does not claim compositor acknowledgement or application effects.
    Closed {
        reason: Option<String>,
    },
    /// Completion unproven; stays occupied, including after a worker panic.
    Failed {
        error: String,
    },
}

struct State {
    phase: PortalSelectionState,
    adapter: Option<Arc<PortalAdapter>>,
}

enum Owner {
    Starting,
    Running(JoinHandle<()>),
    Joined,
    Failed,
}

pub(super) struct Entry {
    selection: PortalSelection,
    initial_generation: u64,
    state: Mutex<State>,
    stop: watch::Sender<bool>,
    owner: Mutex<Owner>,
    preparation: Option<AuthorizationTicket>,
    parent: Mutex<Option<Arc<dyn NativeParent>>>,
    policy: Arc<crate::policy::PolicyLease>,
    monitored_policy: Mutex<Option<Arc<dyn PortalInputPolicy>>>,
}

impl Entry {
    fn preparation_cancelled(&self) -> bool {
        self.preparation
            .as_ref()
            .is_some_and(AuthorizationTicket::is_preparation_cancelled)
    }

    fn revoked(&self) -> bool {
        let monitored = self.monitored_policy.lock().map(|policy| policy.clone());
        self.policy.is_revoked()
            || self.preparation_cancelled()
            || self.parent_revoked()
            || match monitored {
                Ok(policy) => policy.is_some_and(|p| !p.input_available() || p.user_input_active()),
                Err(_) => true,
            }
    }

    fn parent_revoked(&self) -> bool {
        // Clone before invoking an owner: native/Host callbacks never run
        // under the registry's mutexes. Poison cannot certify authority.
        match self.parent.lock().map(|parent| parent.clone()) {
            Ok(parent) => parent.is_some_and(|parent| parent.is_revoked()),
            Err(_) => true,
        }
    }

    async fn parent_cancelled(&self) {
        while !self.parent_revoked() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn close_parent(&self) {
        if let Ok(Some(parent)) = self.parent.lock().map(|parent| parent.clone()) {
            parent.request_close();
        }
    }

    async fn cancellation(&self) {
        match &self.preparation {
            Some(ticket) => ticket.preparation_cancelled().await,
            None => std::future::pending().await,
        }
    }

    pub(super) fn adapter(&self) -> Option<Arc<PortalAdapter>> {
        if self.revoked() {
            return None;
        }
        let state = self.state.lock().ok()?;
        matches!(state.phase, PortalSelectionState::Ready { .. })
            .then(|| state.adapter.clone())
            .flatten()
    }

    pub(super) fn close(&self) {
        self.close_parent();
        let adapter = {
            let Ok(mut state) = self.state.lock() else {
                self.stop.send_replace(true);
                return;
            };
            if !matches!(
                state.phase,
                PortalSelectionState::Closed { .. } | PortalSelectionState::Failed { .. }
            ) {
                state.phase = PortalSelectionState::Closing;
            }
            self.stop.send_replace(true);
            state.adapter.clone()
        };
        // No native call or Host policy callback runs under the registry lock.
        if let Some(adapter) = adapter {
            adapter.fence(u64::MAX);
        }
    }

    pub(super) fn abort(&self, generation: u64) {
        let (pending, adapter) = {
            let Ok(state) = self.state.lock() else {
                self.stop.send_replace(true);
                return;
            };
            match state.phase {
                PortalSelectionState::Pending if generation > self.initial_generation => {
                    (true, None)
                }
                PortalSelectionState::Ready { .. } => (false, state.adapter.clone()),
                _ => (false, None),
            }
        };
        if pending || adapter.is_some_and(|adapter| adapter.fence(generation)) {
            self.close();
        }
    }

    fn fail(&self, error: String) {
        self.close_parent();
        if let Ok(mut state) = self.state.lock() {
            state.phase = PortalSelectionState::Failed { error };
        }
        self.stop.send_replace(true);
    }

    fn joined(&self) -> bool {
        let Ok(mut owner) = self.owner.lock() else {
            return false;
        };
        match &*owner {
            Owner::Joined => return true,
            Owner::Running(thread) if thread.is_finished() => {}
            _ => return false,
        }
        let Owner::Running(thread) = std::mem::replace(&mut *owner, Owner::Failed) else {
            return false;
        };
        if thread.join().is_err() {
            self.fail("portal registry thread panicked; completion unproven".into());
            return false;
        }
        *owner = Owner::Joined;
        true
    }

    fn closed(&self) -> bool {
        let closed = self
            .state
            .lock()
            .map(|s| matches!(s.phase, PortalSelectionState::Closed { .. }))
            .unwrap_or(false);
        closed && self.joined()
    }

    pub(super) fn idle(&self) -> bool {
        if let Some(adapter) = self.adapter() {
            if !adapter.target_alive(adapter.target_id())
                || !adapter.is_idle(&self.selection.run_id)
            {
                return false;
            }
            // Revalidate after the callback. A concurrent Stop must not let an
            // active-idle observation certify the new retirement phase.
            return self.adapter().is_some_and(|a| Arc::ptr_eq(&a, &adapter))
                && adapter.target_alive(adapter.target_id());
        }
        self.closed()
    }
}

/// Multiple independent grants; no process-wide "current run". Desktop input
/// serialization remains the Broker/HostOwnedAdapter's job. The Host must still
/// validate chat/ticket ownership, export a native parent, monitor permission /
/// lock / takeover and call abort. This registry does not enable App Wayland.
pub struct PortalRegistry {
    entries: Mutex<HashMap<String, Arc<Entry>>>,
    pub(super) policy: Arc<dyn PortalInputPolicy>,
}

impl PortalRegistry {
    pub fn new(policy: Arc<dyn PortalInputPolicy>) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            policy,
        }
    }

    /// Start explicit Host consent, never invoked by model target enumeration.
    /// Monitor-only input remains session-wide, not directed window authority.
    pub fn select(
        &self,
        options: PortalOptions,
        generation: u64,
        target_generation: u64,
    ) -> Result<PortalSelection, String> {
        self.select_with(options, generation, target_generation, PortalSession::start)
    }

    /// Bind native preparation to the actual App SessionGrants attempt. The
    /// sticky signal closes cancel-before-register and late-consent races, even
    /// before the Broker has an authorized surface to clean up. A committed
    /// attempt keeps its native grant; ordinary Broker Stop owns it thereafter.
    pub fn select_for_authorization(
        &self,
        options: PortalOptions,
        generation: u64,
        target_generation: u64,
        ticket: &AuthorizationTicket,
    ) -> Result<PortalSelection, String> {
        self.select_guarded(
            options,
            generation,
            target_generation,
            Some(ticket.clone()),
            PortalSession::start,
        )
    }

    /// Register occupancy BEFORE dispatching a GTK export. The retained owner
    /// awaits the original UI operation even after cancellation or a deadline;
    /// a lost waiter is not proof that a late native resource was released.
    pub fn select_parented_for_authorization<P, F>(
        &self,
        options: PortalOptions,
        generation: u64,
        target_generation: u64,
        ticket: &AuthorizationTicket,
        prepare: P,
    ) -> Result<PortalSelection, String>
    where
        P: FnOnce() -> F + Send + 'static,
        F: std::future::Future<Output = Result<Box<dyn NativeParent>, String>> + Send + 'static,
    {
        self.select_owned(
            options,
            SelectionOwner {
                generation,
                target_generation,
                preparation: Some(ticket.clone()),
                parent: Some(Box::new(move || Box::pin(prepare()))),
                policy: None,
            },
            PortalSession::start,
        )
    }

    pub(super) fn select_with(
        &self,
        options: PortalOptions,
        generation: u64,
        target_generation: u64,
        start: impl FnOnce(PortalOptions) -> Result<PortalSession, String> + Send + 'static,
    ) -> Result<PortalSelection, String> {
        self.select_guarded(options, generation, target_generation, None, start)
    }

    pub(super) fn select_guarded(
        &self,
        options: PortalOptions,
        generation: u64,
        target_generation: u64,
        preparation: Option<AuthorizationTicket>,
        start: impl FnOnce(PortalOptions) -> Result<PortalSession, String> + Send + 'static,
    ) -> Result<PortalSelection, String> {
        self.select_owned(
            options,
            SelectionOwner {
                generation,
                target_generation,
                preparation,
                parent: None,
                policy: None,
            },
            start,
        )
    }

    pub(super) fn select_owned(
        &self,
        options: PortalOptions,
        owner: SelectionOwner,
        start: impl FnOnce(PortalOptions) -> Result<PortalSession, String> + Send + 'static,
    ) -> Result<PortalSelection, String> {
        let SelectionOwner {
            generation,
            target_generation,
            preparation,
            parent,
            policy: prepare_policy,
        } = owner;
        options.validate()?;
        if let Some(ticket) = &preparation {
            ticket.check_preparation()?;
            if ticket.run_id != options.run_id {
                return Err("portal selection does not belong to the authorization run".into());
            }
        }
        if parent.is_some() && (preparation.is_none() || !options.parent_window.is_empty()) {
            return Err("owned parent requires an authorization and no supplied handle".into());
        }
        if parent.is_none() && options.parent_window.is_empty() {
            return Err("Host selection requires an exported Wayland parent".into());
        }
        if options.source != SourceKind::Monitor {
            return Err("window-scoped Host input is not implemented".into());
        }
        if !(1..=JS_MAX_SAFE_INTEGER).contains(&generation)
            || !(1..=JS_MAX_SAFE_INTEGER).contains(&target_generation)
        {
            return Err("Host generations must be positive safe integers".into());
        }
        let policy = crate::policy::PolicyLease::new(self.policy.clone());
        if !policy.input_available() {
            return Err("Host policy denied portal selection".into());
        }
        let selection = PortalSelection {
            run_id: options.run_id.clone(),
            id: uuid::Uuid::new_v4(),
        };
        let (stop, stopping) = watch::channel(false);
        let entry = Arc::new(Entry {
            selection: selection.clone(),
            initial_generation: generation,
            state: Mutex::new(State {
                phase: PortalSelectionState::Pending,
                adapter: None,
            }),
            stop,
            owner: Mutex::new(Owner::Starting),
            preparation,
            parent: Mutex::new(None),
            policy: policy.clone(),
            monitored_policy: Mutex::new(None),
        });
        {
            let mut entries = self
                .entries
                .lock()
                .map_err(|_| "portal registry poisoned")?;
            if entries.contains_key(&selection.run_id) {
                return Err(
                    "portal run already retained; join and forget its exact selection first".into(),
                );
            }
            if let Some(ticket) = &entry.preparation {
                ticket.check_preparation()?;
            }
            if !self.policy.input_available() {
                return Err("Host policy denied portal registration".into());
            }
            entries.insert(selection.run_id.clone(), entry.clone());
        }
        let retained = entry.clone();
        let worker = std::thread::Builder::new()
            .name("cu-wayland-registry".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let runtime = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime,
                        Err(error) => return Ok(Some(format!("portal registry runtime: {error}"))),
                    };
                    runtime.block_on(async {
                        let mut monitor = None;
                        let result = own_selection(
                            &retained,
                            options,
                            (generation, target_generation),
                            policy,
                            stopping,
                            (parent, prepare_policy, &mut monitor),
                            start,
                        )
                        .await;
                        // Retain the original OS monitor until native input /
                        // capture have joined, then join it before releasing UI.
                        let monitor_result = native_policy::retire(monitor).await;
                        // Both domains must retire: Portal/adapter on this
                        // reactor, then the original export on the GTK thread.
                        parent::retire(&retained).await?;
                        monitor_result?;
                        result
                    })
                }));
                match result {
                    Ok(Ok(reason)) => {
                        if let Ok(mut state) = retained.state.lock() {
                            state.phase = PortalSelectionState::Closed { reason };
                        }
                    }
                    Ok(Err(error)) => retained.fail(error),
                    Err(_) => {
                        retained.fail("portal selection owner panicked; completion unproven".into())
                    }
                }
            });
        match worker {
            Ok(worker) => {
                *entry.owner.lock().map_err(|_| "portal owner poisoned")? = Owner::Running(worker)
            }
            Err(error) => {
                // No native owner was created. Keep a queryable exact ticket;
                // callers can explicitly forget this fully joined failure.
                *entry.owner.lock().map_err(|_| "portal owner poisoned")? = Owner::Joined;
                entry
                    .state
                    .lock()
                    .map_err(|_| "portal state poisoned")?
                    .phase = PortalSelectionState::Closed {
                    reason: Some(format!("portal registry thread: {error}")),
                };
            }
        }
        Ok(selection)
    }

    pub(super) fn entry(&self, run: &str) -> Option<Arc<Entry>> {
        self.entries.lock().ok()?.get(run).cloned()
    }

    fn owned(&self, selection: &PortalSelection) -> Result<Arc<Entry>, String> {
        self.entry(&selection.run_id)
            .filter(|e| e.selection.id == selection.id)
            .ok_or_else(|| "portal selection is foreign or retired".into())
    }

    pub fn state(&self, selection: &PortalSelection) -> Result<PortalSelectionState, String> {
        let entry = self.owned(selection)?;
        let phase = entry
            .state
            .lock()
            .map_err(|_| "portal state poisoned")?
            .phase
            .clone();
        if entry.revoked()
            && matches!(
                phase,
                PortalSelectionState::Pending | PortalSelectionState::Ready { .. }
            )
        {
            return Ok(PortalSelectionState::Closing);
        }
        Ok(
            if matches!(phase, PortalSelectionState::Closed { .. }) && !entry.joined() {
                PortalSelectionState::Closing
            } else {
                phase
            },
        )
    }

    /// Synchronous revocation, not a promise of synchronous native completion.
    pub fn cancel(&self, selection: &PortalSelection) -> Result<(), String> {
        self.owned(selection)?.close();
        Ok(())
    }

    /// Host maintenance only, AFTER its policy has fenced new selections.
    /// Closes every retained owner; reports true only after actual local join.
    /// Failed/poisoned owners remain occupied, never counted as quiescent.
    pub fn quiesce_for_maintenance(&self) -> Result<bool, String> {
        if self.policy.input_available() {
            return Err("Host must fence selection before maintenance".into());
        }
        let entries = self
            .entries
            .lock()
            .map_err(|_| "portal registry poisoned")?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for entry in &entries {
            entry.close();
        }
        Ok(entries.iter().all(|entry| entry.closed()))
    }

    /// No automatic replacement and no timeout-based pruning. Old UI tickets
    /// cannot affect a newly selected run, even if its run string is reused.
    pub fn forget(&self, selection: &PortalSelection) -> Result<(), String> {
        let entry = self.owned(selection)?;
        if !entry.closed() {
            return Err("portal selection owners have not joined".into());
        }
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "portal registry poisoned")?;
        if entries
            .get(&selection.run_id)
            .is_some_and(|current| Arc::ptr_eq(current, &entry))
        {
            entries.remove(&selection.run_id);
            Ok(())
        } else {
            Err("portal selection changed during retirement".into())
        }
    }

    pub(super) fn all_entries(&self) -> Vec<Arc<Entry>> {
        self.entries
            .lock()
            .map(|e| e.values().cloned().collect())
            .unwrap_or_default()
    }

    pub(super) fn idle_for_run(&self, run: &str) -> bool {
        let entry = match self.entries.lock() {
            Ok(entries) => match entries.get(run) {
                Some(entry) => entry.clone(),
                None => return true, // Verified absence, not a poisoned lookup.
            },
            Err(_) => return false,
        };
        entry.idle()
            && self
                .entry(run)
                .is_some_and(|current| Arc::ptr_eq(&current, &entry))
    }
}

impl Drop for PortalRegistry {
    fn drop(&mut self) {
        for entry in self.all_entries() {
            entry.close();
        }
        // Entry-owning threads retain their runtimes and join the original
        // tasks. Dropping a registry never aborts them or claims idle.
    }
}

async fn own_selection(
    entry: &Entry,
    mut options: PortalOptions,
    generations: (u64, u64),
    mut policy: Arc<dyn PortalInputPolicy>,
    mut stop: watch::Receiver<bool>,
    preparation: (
        Option<ParentPreparation>,
        Option<native_policy::PolicyPreparation>,
        &mut Option<native_policy::Monitor>,
    ),
    start: impl FnOnce(PortalOptions) -> Result<PortalSession, String>,
) -> Result<Option<String>, String> {
    let (generation, target_generation) = generations;
    let (prepare, prepare_policy, monitor) = preparation;
    if *stop.borrow() || entry.revoked() {
        return Ok(None);
    }
    if let Some(prepare_policy) = prepare_policy {
        match native_policy::prepare(
            entry,
            &mut options,
            &mut stop,
            prepare_policy,
            policy.clone(),
            monitor,
        )
        .await
        {
            Ok(guarded) => policy = guarded,
            Err(error) => return Ok(Some(error)),
        }
    }
    if let Some(prepare) = prepare {
        match parent::prepare(entry, &mut options, &mut stop, prepare, policy.clone()).await {
            Ok(guarded) => policy = guarded,
            Err(error) => return Ok(Some(error)),
        }
    }
    if *stop.borrow() || entry.revoked() {
        return Ok(None);
    }
    let mut session = match start(options) {
        Ok(session) => Some(session),
        Err(error) => return Ok(Some(error)),
    };
    let ready = tokio::select! {
        biased;
        _ = stop.changed() => Err("portal selection cancelled".to_owned()),
        _ = entry.cancellation() => Err("target authorization was cancelled".to_owned()),
        _ = entry.parent_cancelled() => Err("native parent was revoked".to_owned()),
        _ = crate::policy::revoked(policy.as_ref()) => Err("Host policy revoked during portal consent".to_owned()),
        ready = session.as_mut().expect("retained session").ready() => ready.map(|_| ()).map_err(|e| format!("{e:?}")),
    };
    if let Err(error) = ready {
        entry.close();
        session.as_mut().expect("retained session").stop().await?;
        return Ok(Some(error));
    }
    if let Some(monitor) = monitor.as_mut() {
        let armed = tokio::select! {
            biased;
            _ = stop.changed() => Err("portal selection cancelled during policy activation".into()),
            _ = entry.cancellation() => Err("authorization cancelled during policy activation".into()),
            _ = entry.parent_cancelled() => Err("native parent revoked during policy activation".into()),
            _ = crate::policy::revoked(policy.as_ref()) => Err("policy revoked during activation".into()),
            result = monitor.activate(policy.clone()) => result,
        };
        match armed {
            Ok(active) => policy = active,
            Err(error) => {
                entry.close();
                session.as_mut().expect("retained session").stop().await?;
                return Ok(Some(error));
            }
        }
        *entry
            .monitored_policy
            .lock()
            .map_err(|_| "native policy slot poisoned")? = Some(policy.clone());
    }
    let native_state = session.as_ref().expect("retained session").control();
    let mut host =
        match PortalHostSession::from_session_slot(&mut session, generation, target_generation) {
            Ok(host) => Some(host),
            Err(error) => {
                entry.close();
                session
                    .as_mut()
                    .expect("failed handoff retains session")
                    .stop()
                    .await?;
                return Ok(Some(error));
            }
        };
    let adapter = match PortalAdapter::from_granted_slot(&mut host, policy.clone()) {
        Ok(adapter) => Arc::new(adapter),
        Err(error) => {
            entry.close();
            host.as_mut()
                .expect("failed handoff retains Host")
                .stop()
                .await?;
            return Ok(Some(error));
        }
    };
    let allowed = policy.input_available() && !policy.user_input_active();
    let revoked = entry.revoked();
    {
        let mut state = entry.state.lock().map_err(|_| "portal state poisoned")?;
        state.adapter = Some(adapter.clone());
        if allowed
            && !*stop.borrow()
            && !revoked
            && !entry.preparation_cancelled()
            && matches!(state.phase, PortalSelectionState::Pending)
        {
            state.phase = PortalSelectionState::Ready {
                target_id: adapter.target_id().into(),
            };
        } else {
            state.phase = PortalSelectionState::Closing;
            entry.stop.send_replace(true);
        }
    }
    while !*stop.borrow() && !entry.revoked() && adapter.target_alive(adapter.target_id()) {
        tokio::select! {
            _ = stop.changed() => {},
            _ = entry.cancellation() => {},
            _ = crate::policy::revoked(policy.as_ref()) => break,
            _ = tokio::time::sleep(Duration::from_millis(10)) => {},
        }
    }
    let policy_revoked = !policy.input_available() || policy.user_input_active();
    entry.close();
    while !adapter.is_idle(&entry.selection.run_id) {
        if let Some(error) = adapter.cleanup_error() {
            entry.fail(error);
            // Keep the granting reactor alive even when owner completion is
            // unproven. A failed callback cannot silently cancel native cleanup.
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok(if !allowed || policy_revoked {
        Some("Host policy revoked during portal selection".into())
    } else {
        match native_state.state() {
            crate::SessionState::Closed(crate::CloseReason::Stopped) => None,
            state => Some(format!("{state:?}")),
        }
    })
}
