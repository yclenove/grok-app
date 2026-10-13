//! Picker references remain pinned to the original request transport.
use super::*;
use crate::computer_use::SurfaceKind;
use grok_computer_use_core::broker::{
    AccessApproval, AccessApprovalReady, HostScopeCandidateView, HostScopePicker,
    HostScopePickerCall, HostScopePickerCancel, HostScopeSelection,
};
use grok_computer_use_core::task_consent::{CommitOutcome, HostRequestKey, MAX_SCOPE_MEMBERS};
#[cfg(all(test, windows))]
mod native_fixture;
mod preparation;
pub(crate) use preparation::TaskPickerPreparation;

pub(super) struct PickerSlot {
    id: String,
    key: HostRequestKey,
    surface: SurfaceKind,
    app_process_id: String,
    call: Option<HostScopePickerCall>,
    ready: Option<Arc<HostScopePicker>>,
    cancel: HostScopePickerCancel,
    approving: bool,
}

impl Drop for PickerSlot {
    fn drop(&mut self) {
        self.cancel.fence();
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskPickerStarted {
    session_id: String,
    picker_id: String,
    key: HostRequestKey,
    max_selection: usize,
    surface: &'static str,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskPickerView {
    session_id: String,
    picker_id: String,
    key: HostRequestKey,
    max_selection: usize,
    surface: &'static str,
    candidates: Vec<HostScopeCandidateView>,
    truncated: bool,
}

struct PickerCallGuard {
    cancel: HostScopePickerCancel,
    completed: bool,
}
impl Drop for PickerCallGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel.fence();
        }
    }
}

struct PickerApprovalGuard {
    transport: Arc<RequestTransport>,
    picker_id: String,
    cancel: HostScopePickerCancel,
    completed: bool,
}

impl Drop for PickerApprovalGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel.fence();
        }
        let mut picker = self.transport.picker.lock();
        if let Some(slot) = picker.as_mut().filter(|slot| slot.id == self.picker_id) {
            slot.approving = false;
        }
    }
}

impl AcpClient {
    pub(crate) fn begin_computer_use_task_picker(
        &self,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        surface: &str,
        app_process_id: &str,
    ) -> Result<TaskPickerStarted, String> {
        if app_process_id.is_empty() {
            return Err("Computer Use original App process unavailable".into());
        }
        let surface = match surface {
            "desktop" => SurfaceKind::Desktop,
            "existing-tabs" => SurfaceKind::ExistingTab,
            "app-webview" => SurfaceKind::WebView,
            _ => {
                return Err("Computer Use task picker producer unavailable for this surface".into())
            }
        };
        let slots = self.computer_use_connections.lock();
        let transport = slots
            .get(session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?;
        if surface == SurfaceKind::ExistingTab {
            transport
                .broker
                .host_task_view(&transport.token)
                .map_err(|_| "Computer Use request retired or changed")?
                .request
                .filter(|request| {
                    request.key == *key
                        && request
                            .surfaces
                            .iter()
                            .any(|value| value == surface.as_wire())
                        && matches!(
                            request.phase,
                            grok_computer_use_core::task_consent::HostRequestPhase::Pending
                                | grok_computer_use_core::task_consent::HostRequestPhase::Preparing
                        )
                })
                .ok_or("Computer Use request retired or changed")?;
            crate::computer_use::task_picker::prepare(&transport.broker, surface)?;
        }
        let call = transport
            .broker
            .begin_task_scope_picker(&transport.token, key, surface)
            .map_err(|_| "Computer Use request retired or changed")?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let cancel = call.cancel_handle();
        let original = transport.picker.lock().replace(PickerSlot {
            id: id.clone(),
            key: key.clone(),
            surface,
            app_process_id: app_process_id.into(),
            call: Some(call),
            ready: None,
            cancel,
            approving: false,
        });
        drop(original); // Only the prior selection generation is fenced.
        Ok(TaskPickerStarted {
            session_id: session.into(),
            picker_id: id,
            key: key.clone(),
            max_selection: MAX_SCOPE_MEMBERS,
            surface: surface.as_wire(),
        })
    }

    pub(crate) fn cancel_computer_use_task_picker(
        &self,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        picker_id: &str,
    ) -> Result<bool, String> {
        let slots = self.computer_use_connections.lock();
        let transport = slots
            .get(session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?;
        let mut picker = transport.picker.lock();
        if picker
            .as_ref()
            .is_none_or(|slot| slot.key != *key || slot.id != picker_id)
        {
            return Ok(false);
        }
        drop(picker.take());
        Ok(true)
    }

    pub(crate) async fn load_computer_use_task_picker(
        &self,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        picker_id: &str,
    ) -> Result<TaskPickerView, String> {
        self.load_picker_with(
            session,
            agent,
            key,
            picker_id,
            crate::computer_use::task_picker::discover,
        )
        .await
    }

    async fn load_picker_with<D, DF>(
        &self,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        picker_id: &str,
        discover: D,
    ) -> Result<TaskPickerView, String>
    where
        D: FnOnce(Arc<crate::computer_use::ComputerUseBroker>, HostScopePickerCall) -> DF,
        DF: std::future::Future<Output = Result<HostScopePicker, String>>,
    {
        let (transport, call, mut guard) = {
            let slots = self.computer_use_connections.lock();
            let transport = slots
                .get(session)
                .and_then(|slot| slot.transport.as_ref())
                .filter(|transport| self.is_alive() && transport.bound_to(agent))
                .ok_or("Computer Use request owner unavailable")?
                .clone();
            let mut picker = transport.picker.lock();
            let slot = picker
                .as_mut()
                .filter(|slot| slot.key == *key && slot.id == picker_id)
                .ok_or("Computer Use picker retired")?;
            slot.retry_empty_discovery()?;
            if let Some(ready) = &slot.ready {
                return picker_view(session, picker_id, ready);
            }
            if slot.surface == SurfaceKind::ExistingTab {
                let preparation = slot
                    .call
                    .as_ref()
                    .ok_or("Computer Use picker discovery already in progress")?
                    .existing_share_preparation()
                    .map_err(|_| "Computer Use picker retired")?;
                if preparation.challenge.is_some() || preparation.available_tabs == 0 {
                    return Err(
                        "Computer Use picker is waiting for pairing or a compatible Share".into(),
                    );
                }
            }
            let call = slot
                .call
                .take()
                .ok_or("Computer Use picker discovery already in progress")?;
            let guard = PickerCallGuard {
                cancel: call.cancel_handle(),
                completed: false,
            };
            drop(picker);
            (transport, call, guard)
        };
        let receipt = discover(transport.broker.clone(), call).await?;
        if !self.scope_owner_is_current(session, agent, &transport) {
            return Err("Computer Use request owner retired".into());
        }
        let view = picker_view(session, picker_id, &receipt)?;
        {
            let mut picker = transport.picker.lock();
            let slot = picker
                .as_mut()
                .filter(|slot| slot.key == *key && slot.id == picker_id)
                .ok_or("Computer Use picker retired")?;
            if !receipt.is_current() {
                return Err("Computer Use picker retired".into());
            }
            slot.ready = Some(Arc::new(receipt));
            guard.completed = true;
        }
        Ok(view)
    }

    pub(crate) async fn approve_computer_use_task_picker(
        self: &Arc<Self>,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        picker_id: &str,
        candidate_ids: Vec<String>,
        pin: TaskApprovalPin,
    ) -> Result<CommitOutcome, String> {
        self.approve_picker_with(
            session,
            agent,
            (key, picker_id),
            candidate_ids,
            pin,
            crate::computer_use::task_access::approve,
        )
        .await
    }

    async fn approve_picker_with<A, AF>(
        self: &Arc<Self>,
        session: &str,
        agent: &str,
        selection: (&HostRequestKey, &str),
        candidate_ids: Vec<String>,
        pin: TaskApprovalPin,
        approve: A,
    ) -> Result<CommitOutcome, String>
    where
        A: FnOnce(
            Arc<crate::computer_use::ComputerUseBroker>,
            AccessApproval,
            crate::computer_use::task_access::ApprovalPublication,
        ) -> AF,
        AF: std::future::Future<Output = Result<CommitOutcome, String>>,
    {
        let (key, picker_id) = selection;
        let start = self.start_picker_approval(session, agent, key, picker_id, &candidate_ids)?;
        let publication = match &start {
            PickerApprovalStart::Already(_) => None,
            PickerApprovalStart::Select(_, guard) => {
                Some(self.approval_publication(session, agent, guard.transport.clone(), pin))
            }
        };
        self.finish_picker_approval(
            session,
            agent,
            start,
            crate::computer_use::task_picker::reserve,
            move |broker, approval| async move {
                approve(
                    broker,
                    approval,
                    publication.ok_or("Computer Use approval owner unavailable")?,
                )
                .await
            },
        )
        .await
    }

    fn start_picker_approval(
        &self,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        picker_id: &str,
        candidate_ids: &[String],
    ) -> Result<PickerApprovalStart, String> {
        let slots = self.computer_use_connections.lock();
        let transport = slots
            .get(session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?
            .clone();
        let mut picker = transport.picker.lock();
        let slot = picker
            .as_mut()
            .filter(|slot| slot.key == *key && slot.id == picker_id)
            .ok_or("Computer Use picker not ready or retired")?;
        let receipt = slot
            .ready
            .as_ref()
            .ok_or("Computer Use picker not ready or retired")?;
        if let Some(outcome) = receipt
            .committed_outcome(&transport.token, candidate_ids)
            .map_err(|_| "Computer Use selection differs or owner retired")?
        {
            return Ok(PickerApprovalStart::Already(outcome));
        }
        if slot.approving {
            return Err("Computer Use picker approval already in progress".into());
        }
        let selection = receipt
            .select(candidate_ids)
            .map_err(|_| "Computer Use picker selection unavailable or retired")?;
        let cancel = receipt.cancel_handle();
        slot.approving = true;
        drop(picker);
        let guard = PickerApprovalGuard {
            transport,
            picker_id: picker_id.into(),
            cancel,
            completed: false,
        };
        Ok(PickerApprovalStart::Select(selection, guard))
    }

    async fn finish_picker_approval<R, A, RF, AF>(
        &self,
        session: &str,
        agent: &str,
        start: PickerApprovalStart,
        reserve: R,
        approve: A,
    ) -> Result<CommitOutcome, String>
    where
        R: FnOnce(
            Arc<crate::computer_use::ComputerUseBroker>,
            String,
            HostScopeSelection,
            HostScopePickerCancel,
        ) -> RF,
        A: FnOnce(Arc<crate::computer_use::ComputerUseBroker>, AccessApproval) -> AF,
        RF: std::future::Future<Output = Result<AccessApprovalReady, String>>,
        AF: std::future::Future<Output = Result<CommitOutcome, String>>,
    {
        let (selection, mut guard) = match start {
            PickerApprovalStart::Already(outcome) => return Ok(outcome),
            PickerApprovalStart::Select(selection, guard) => (selection, guard),
        };
        let transport = guard.transport.clone();
        let cancel = {
            let slots = self.computer_use_connections.lock();
            if !slots
                .get(session)
                .and_then(|slot| slot.transport.as_ref())
                .is_some_and(|current| {
                    Arc::ptr_eq(current, &transport) && self.is_alive() && current.bound_to(agent)
                })
            {
                return Err("Computer Use request owner retired".into());
            }
            transport
                .picker
                .lock()
                .as_ref()
                .filter(|slot| slot.id == guard.picker_id)
                .map(|slot| slot.cancel.clone())
                .ok_or("Computer Use picker retired")?
        };
        let reservation = reserve(
            transport.broker.clone(),
            transport.token.clone(),
            selection,
            cancel,
        )
        .await?;
        if !self.scope_owner_is_current(session, agent, &transport) {
            return Err("Computer Use request owner retired".into());
        }
        let outcome = match reservation {
            AccessApprovalReady::Already(outcome) => outcome,
            AccessApprovalReady::Prepare(approval) => {
                approve(transport.broker.clone(), approval).await?
            }
        };
        if !self.scope_owner_is_current(session, agent, &transport) {
            return Err("Computer Use request owner retired".into());
        }
        guard.completed = true;
        Ok(outcome)
    }
}

enum PickerApprovalStart {
    Already(CommitOutcome),
    Select(HostScopeSelection, PickerApprovalGuard),
}

fn picker_view(
    session: &str,
    id: &str,
    receipt: &HostScopePicker,
) -> Result<TaskPickerView, String> {
    let candidates = receipt.views().map_err(|_| "Computer Use picker retired")?;
    Ok(TaskPickerView {
        session_id: session.into(),
        picker_id: id.into(),
        key: receipt.key().clone(),
        max_selection: MAX_SCOPE_MEMBERS,
        surface: receipt.surface().as_wire(),
        candidates,
        truncated: receipt.truncated(),
    })
}

#[cfg(test)]
mod tests;
