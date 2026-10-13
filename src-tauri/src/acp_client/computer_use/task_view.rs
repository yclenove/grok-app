//! Local Host readback/decisions use the original ACP-owned request transport.
use super::*;
use grok_computer_use_core::broker::{HostTaskDecision, HostTaskDecisionReceipt, HostTaskView};
use grok_computer_use_core::task_consent::HostRequestKey;
mod recovery;
pub(crate) use recovery::{TaskResumeCall, TaskResumeStarted};

fn present_view(mut view: HostTaskView) -> HostTaskView {
    // This is a compiled producer descriptor, not native readiness or consent.
    // Reading it does not instantiate an adapter or enumerate a target.
    if cfg!(target_os = "windows") {
        view.picker_surfaces.push("desktop".into());
    }
    if cfg!(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux"
    )) {
        view.picker_surfaces.push("managed-browser".into());
    }
    #[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
    view.picker_surfaces.push("app-webview".into());
    view
}

/// Pins the original transport across native work without holding ACP locks.
/// The core rechecks its bearer and full address at admission/publication.
pub(crate) struct TaskExecutionCall {
    client: std::sync::Weak<AcpClient>,
    app_session: String,
    agent: String,
    original: Arc<RequestTransport>,
    address: grok_computer_use_core::broker::HostExecutionAddress,
}

impl TaskExecutionCall {
    fn check_original(&self) -> Result<(), String> {
        let client = self
            .client
            .upgrade()
            .ok_or("Computer Use request owner retired")?;
        let slots = client.computer_use_connections.lock();
        if !slots
            .get(&self.app_session)
            .and_then(|slot| slot.transport.as_ref())
            .is_some_and(|current| {
                Arc::ptr_eq(current, &self.original)
                    && client.is_alive()
                    && current.bound_to(&self.agent)
            })
        {
            return Err("Computer Use request owner retired".into());
        }
        Ok(())
    }

    pub(crate) fn pause(self) -> Result<(), String> {
        self.check_original()?;
        self.original
            .broker
            .pause_host_execution(&self.original.token, &self.address)
            .map_err(|_| "Computer Use pause unavailable or execution changed".to_string())?;
        self.check_original()
    }

    pub(crate) fn preview(self) -> Result<grok_computer_use_core::protocol::Observation, String> {
        self.check_original()?;
        let observation = self
            .original
            .broker
            .preview_host_execution(&self.original.token, &self.address)
            .map_err(|_| "Computer Use preview unavailable or execution changed".to_string())?;
        self.check_original()?;
        Ok(observation)
    }

    pub(crate) fn select_upload(
        self,
        request: &str,
    ) -> Result<grok_computer_use_core::broker::HostUploadSelection, String> {
        self.check_original()?;
        let selection = self
            .original
            .broker
            .begin_host_upload_selection(&self.original.token, &self.address, request)
            .map_err(|_| "Computer Use original upload request retired or changed".to_string())?;
        self.check_original()?;
        Ok(selection)
    }

    pub(crate) fn cancel_upload(
        self,
        request: &str,
    ) -> Result<grok_computer_use_core::broker::WebViewUploadCompletion, String> {
        self.check_original()?;
        let completion = self
            .original
            .broker
            .cancel_host_upload(&self.original.token, &self.address, request)
            .map_err(|_| "Computer Use original upload request retired or changed".to_string())?;
        self.check_original()?;
        Ok(completion)
    }
}

/// Keeps the original transport alive for observation only. This is not a
/// bearer DTO and cannot be sent to a renderer or a model.
pub(crate) struct TaskTransportWatch {
    original: Arc<RequestTransport>,
    pub(crate) changes: grok_computer_use_core::broker::HostTaskChanges,
    pub(crate) host_instance_id: String,
}

impl AcpClient {
    pub(crate) fn computer_use_task_watch(
        &self,
        app_session: &str,
        agent: &str,
    ) -> Result<TaskTransportWatch, String> {
        let slots = self.computer_use_connections.lock();
        let original = slots
            .get(app_session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?
            .clone();
        Ok(TaskTransportWatch {
            changes: original.broker.host_task_changes(),
            host_instance_id: original.broker.host_instance_id().into(),
            original,
        })
    }

    pub(crate) fn read_computer_use_task_watch(
        &self,
        app_session: &str,
        agent: &str,
        watch: &TaskTransportWatch,
    ) -> Result<HostTaskView, String> {
        self.with_task_transport(app_session, agent, |transport| {
            if !std::ptr::eq(transport, watch.original.as_ref()) {
                return Err(
                    grok_computer_use_core::error::BrokerError::IdentityMismatch(
                        "originalTransport",
                    ),
                );
            }
            transport
                .broker
                .host_task_view(&transport.token)
                .map(present_view)
        })
    }

    pub(crate) fn computer_use_task_view(
        &self,
        app_session: &str,
        agent: &str,
    ) -> Result<HostTaskView, String> {
        self.with_task_transport(app_session, agent, |transport| {
            transport
                .broker
                .host_task_view(&transport.token)
                .map(present_view)
        })
    }

    pub(crate) fn decide_computer_use_task(
        &self,
        app_session: &str,
        agent: &str,
        key: &HostRequestKey,
        decision: HostTaskDecision,
    ) -> Result<HostTaskDecisionReceipt, String> {
        self.with_task_transport(app_session, agent, |transport| {
            transport
                .broker
                .decide_host_task(&transport.token, key, decision)
        })
    }

    pub(crate) fn computer_use_execution_call(
        self: &Arc<Self>,
        app_session: &str,
        agent: &str,
        address: &grok_computer_use_core::broker::HostExecutionAddress,
    ) -> Result<TaskExecutionCall, String> {
        let slots = self.computer_use_connections.lock();
        let original = slots
            .get(app_session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?
            .clone();
        // No native query or bootstrap; the execution entry will check this
        // against authority again, including replacements during native work.
        if original
            .broker
            .host_task_view(&original.token)
            .map_err(|_| "Computer Use request owner retired")?
            .connection_id
            != address.key.connection_id
        {
            return Err("Computer Use execution owner changed".into());
        }
        Ok(TaskExecutionCall {
            client: Arc::downgrade(self),
            app_session: app_session.into(),
            agent: agent.into(),
            original,
            address: address.clone(),
        })
    }

    fn with_task_transport<T>(
        &self,
        app_session: &str,
        agent: &str,
        operation: impl FnOnce(
            &RequestTransport,
        ) -> Result<T, grok_computer_use_core::error::BrokerError>,
    ) -> Result<T, String> {
        // Synchronous metadata/fence only; no await or native query under this
        // mutex. Replacement/kill cannot swap the transport mid-decision.
        let slots = self.computer_use_connections.lock();
        let transport = slots
            .get(app_session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?;
        operation(transport).map_err(|_| "Computer Use request retired or changed".into())
    }
}

#[cfg(test)]
pub(crate) mod tests;
