//! App-owned request transport. Generic/auxiliary ACP callers receive no token.
use super::*;
use grok_computer_use_core::task_consent::BearerClass;
mod approval_publication;
mod delivery;
mod managed_approval;
pub(crate) use approval_publication::{TaskApprovalOwner, TaskApprovalPin};
#[cfg(feature = "computer-use-probe")]
mod normal_cli;
mod permission;
mod task_approval;
mod task_picker;
pub(crate) use task_picker::{TaskPickerPreparation, TaskPickerStarted, TaskPickerView};
mod task_view;
pub(crate) use delivery::{DecisionDelivery, DeliveryAttempt, PromptCallGuard, PromptWaitGuard};
#[cfg(feature = "computer-use-probe")]
pub(crate) use normal_cli::run as normal_cli_probe;
#[cfg(test)]
pub(crate) use task_view::tests::Fixture as TaskViewFixture;
#[cfg(all(test, windows))]
pub(crate) use task_view::tests::TransportFixture as TaskTransportFixture;
pub(crate) use task_view::{TaskResumeCall, TaskResumeStarted};

const MAX_CONNECTIONS: usize = 32;

pub(super) struct RequestConnection {
    transport: Option<Arc<RequestTransport>>,
    error: Option<&'static str>,
}

struct RequestTransport {
    broker: Arc<crate::computer_use::ComputerUseBroker>,
    token: String,
    entry: Value,
    deliveries: ParkingMutex<delivery::DeliveryLedger>,
    history_path: Option<PathBuf>,
    picker: ParkingMutex<Option<task_picker::PickerSlot>>,
}

impl RequestTransport {
    fn reserve(
        broker: Arc<crate::computer_use::ComputerUseBroker>,
        session: &str,
    ) -> Result<Self, String> {
        let history_path = crate::computer_use::host_history::path(session)?;
        let issued = broker
            .reserve_transport_connection(session)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            broker,
            token: issued.token().into(),
            entry: Value::Null,
            deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
            history_path: Some(history_path),
            picker: ParkingMutex::new(None),
        })
    }

    fn build(mut transport: Self, session: &str) -> Result<Arc<Self>, String> {
        let endpoint =
            crate::computer_use::ipc::endpoint().ok_or("request endpoint unavailable")?;
        transport.entry =
            crate::computer_use::request_mcp_entry(session, &transport.token, endpoint)?;
        Ok(Arc::new(transport))
    }

    fn retire(&self) {
        self.broker.retire_transport_connection(&self.token);
        self.picker.lock().take();
    }

    fn bound_to(&self, agent: &str) -> bool {
        matches!(self.broker.classify_bearer(&self.token), BearerClass::Live(context) if context.agent_session_id.as_deref() == Some(agent))
    }
}

impl Drop for RequestTransport {
    fn drop(&mut self) {
        self.retire();
    }
}

/// Covers cancellation of the actual open RPC, not just an error return.
struct PendingHandshake<'a> {
    client: &'a AcpClient,
    session: &'a str,
    transport: Arc<RequestTransport>,
    committed: bool,
}

impl Drop for PendingHandshake<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.transport.retire();
        let mut slots = self.client.computer_use_connections.lock();
        if slots
            .get(self.session)
            .and_then(|slot| slot.transport.as_ref())
            .is_some_and(|transport| Arc::ptr_eq(transport, &self.transport))
        {
            slots.remove(self.session);
        }
    }
}

pub(super) fn local_transport_allowed(
    owned_native: bool,
    auxiliary: bool,
    ssh: bool,
    has_child: bool,
) -> bool {
    owned_native && !auxiliary && !ssh && has_child
}

pub(super) fn merge_request_entry(mut base: Value, request: Option<&Value>) -> Value {
    if !base.is_array() {
        base = json!([]);
    }
    let entries = base.as_array_mut().expect("array normalized");
    entries.retain(|entry| {
        entry.get("name").and_then(Value::as_str) != Some(crate::computer_use::MCP_SERVER_NAME)
    });
    if let Some(request) = request {
        entries.push(request.clone());
    }
    base
}

impl AcpClient {
    pub(crate) async fn open_app_session_at(
        &self,
        app_session: &str,
        resume: Option<&str>,
        fork: bool,
        cwd: &str,
    ) -> Result<(String, bool), AgentError> {
        if !self.supports_local_computer_use().await
            || crate::computer_use::refuse_if_not_local(app_session).is_err()
        {
            return self.open_session_at(resume, fork, cwd).await;
        }
        {
            let slots = self.computer_use_connections.lock();
            if slots.len() >= MAX_CONNECTIONS && !slots.contains_key(app_session) {
                return Err(AgentError::new(
                    AgentErrorCode::AgentCrashed,
                    "Computer Use connection capacity exhausted",
                ));
            }
        }
        let bootstrapped =
            tauri::async_runtime::spawn_blocking(crate::computer_use::ensure_request_endpoint)
                .await;
        let reservation = match bootstrapped {
            Ok(Ok(broker)) => {
                // Reserve synchronously while the actual process is still live.
                // A cancelled old bootstrap may finish, but cannot create an
                // owner after kill and fence a newer process's connection.
                let _slots = self.computer_use_connections.lock();
                if !self.is_alive() {
                    return Err(AgentError::new(
                        AgentErrorCode::AgentCrashed,
                        "agent stopped",
                    ));
                }
                RequestTransport::reserve(broker, app_session)
            }
            _ => Err("request endpoint unavailable".into()),
        };
        let session = app_session.to_string();
        let prepared = match reservation {
            Ok(reservation) => {
                tauri::async_runtime::spawn_blocking(move || {
                    RequestTransport::build(reservation, &session)
                })
                .await
            }
            Err(_) => Ok(Err("request transport unavailable".into())),
        };
        let transport = match prepared {
            Ok(Ok(transport)) => transport,
            _ => {
                // The rest of chat remains available. Expose a stable diagnostic
                // and retry on reconnect; never log token-bearing builder data.
                self.computer_use_connections.lock().insert(app_session.into(), RequestConnection { transport: None, error: Some("Computer Use request runtime unavailable; repair the private runtime and reconnect") });
                return self.open_session_at(resume, fork, cwd).await;
            }
        };
        let mut handshake = PendingHandshake {
            client: self,
            session: app_session,
            transport: transport.clone(),
            committed: false,
        };
        {
            let mut slots = self.computer_use_connections.lock();
            if !self.is_alive() {
                return Err(AgentError::new(
                    AgentErrorCode::AgentCrashed,
                    "agent stopped",
                ));
            }
            slots.insert(
                app_session.into(),
                RequestConnection {
                    transport: Some(transport.clone()),
                    error: None,
                },
            );
        }
        let opened = self
            .open_session_at_with_entry(resume, fork, cwd, Some(&transport.entry))
            .await?;
        {
            let slots = self.computer_use_connections.lock();
            let current = slots
                .get(app_session)
                .and_then(|slot| slot.transport.as_ref())
                .is_some_and(|registered| Arc::ptr_eq(registered, &transport));
            if !self.is_alive() || !current {
                return Err(AgentError::new(
                    AgentErrorCode::AgentCrashed,
                    "Computer Use handshake superseded",
                ));
            }
            transport
                .broker
                .bind_transport_agent(&transport.token, &opened.0)
                .map_err(|_| {
                    AgentError::new(
                        AgentErrorCode::AgentCrashed,
                        "Computer Use handshake owner retired",
                    )
                })?;
            handshake.committed = true;
        }
        Ok(opened)
    }

    pub(crate) fn computer_use_request_entry(
        &self,
        app_session: &str,
        agent: &str,
    ) -> Option<Value> {
        if !self.is_alive() {
            return None;
        }
        let slots = self.computer_use_connections.lock();
        let transport = slots.get(app_session)?.transport.as_ref()?;
        transport.bound_to(agent).then(|| transport.entry.clone())
    }

    pub(crate) fn computer_use_request_error(&self, app_session: &str) -> Option<&'static str> {
        let slots = self.computer_use_connections.lock();
        let slot = slots.get(app_session)?;
        slot.error.or_else(|| {
            slot.transport
                .as_ref()
                .and_then(|transport| transport.deliveries.lock().last_error)
        })
    }

    pub(super) fn retire_computer_use_transports(&self) {
        let slots = std::mem::take(&mut *self.computer_use_connections.lock());
        for slot in slots.into_values() {
            if let Some(transport) = slot.transport {
                transport.retire();
            }
        }
    }

    pub(super) async fn computer_use_base_catalog(&self, cwd: String) -> Value {
        if self.empty_mcp_servers || self.ssh_alias.is_some() {
            return json!([]);
        }
        let budget = crate::extensions::MCP_CONNECT_BUDGET;
        match tokio::time::timeout(
            budget,
            tauri::async_runtime::spawn_blocking(move || {
                crate::extensions::build_session_mcp_servers_for_connect(Some(&cwd))
            }),
        )
        .await
        {
            Ok(Ok(servers)) => servers,
            _ => {
                warn!("acp base MCP catalog unavailable within connect budget");
                json!([])
            }
        }
    }
}

#[cfg(test)]
#[path = "computer_use_tests.rs"]
mod tests;
