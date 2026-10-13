//! First-use metadata remains owned by the original picker and ACP transport.
use super::*;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskPickerPairing {
    nonce: String,
    instance_id: String,
    verification_code: String,
    endpoint: String,
    installed_extension_id: String,
    expires_at_ms: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskPickerPreparation {
    #[serde(flatten)]
    started: TaskPickerStarted,
    pairing: Option<TaskPickerPairing>,
    pairing_confirmed: bool,
    ready: bool,
    available_tabs: usize,
    incompatible_tabs: usize,
    borrowed_tabs: usize,
}

impl PickerSlot {
    /// A readiness/discovery race may produce an empty receipt. Transfer only
    /// that receipt back into its original call; no new Pair challenge, permit,
    /// App owner or request revision is created, and no nonempty ID is rebound.
    pub(super) fn retry_empty_discovery(&mut self) -> Result<(), String> {
        let Some(receipt) = self.ready.as_ref() else {
            return Ok(());
        };
        if self.surface != SurfaceKind::ExistingTab
            || !receipt
                .views()
                .map_err(|_| "Computer Use picker retired")?
                .is_empty()
        {
            return Ok(());
        }
        if self.approving {
            return Err("Computer Use picker approval already in progress".into());
        }
        let prep = receipt
            .existing_share_preparation()
            .map_err(|_| "Computer Use picker preparation retired")?;
        if prep.challenge.is_some() || prep.available_tabs == 0 {
            return Ok(());
        }
        let receipt = self.ready.take().expect("inspected original receipt");
        let receipt = match Arc::try_unwrap(receipt) {
            Ok(receipt) => receipt,
            Err(receipt) => {
                self.ready = Some(receipt);
                return Err("Computer Use picker receipt still in use".into());
            }
        };
        self.call = Some(
            receipt
                .into_empty_retry()
                .map_err(|_| "Computer Use picker preparation retired")?,
        );
        Ok(())
    }
}

impl AcpClient {
    /// App-only identity, retained at the original begin gesture. The main
    /// command must check it under the original SessionManager slot boundary.
    pub(crate) fn computer_use_task_picker_owner_matches(
        &self,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        picker_id: &str,
        process_id: &str,
    ) -> Result<bool, String> {
        let slots = self.computer_use_connections.lock();
        let transport = slots
            .get(session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?;
        let picker = transport.picker.lock();
        Ok(picker.as_ref().is_some_and(|slot| {
            slot.key == *key && slot.id == picker_id && slot.app_process_id == process_id
        }))
    }

    /// Only the trusted main WebView receives the one-time code. Neither the
    /// transport bearer nor native/share identity is part of this DTO.
    pub(crate) fn computer_use_task_picker_preparation(
        &self,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        picker_id: &str,
    ) -> Result<TaskPickerPreparation, String> {
        let slots = self.computer_use_connections.lock();
        let transport = slots
            .get(session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?;
        let picker = transport.picker.lock();
        let slot = picker
            .as_ref()
            .filter(|slot| {
                slot.key == *key
                    && slot.id == picker_id
                    && slot.surface == SurfaceKind::ExistingTab
                    && !slot.approving
            })
            .ok_or("Computer Use picker preparation retired or busy")?;
        let view = if let Some(call) = slot.call.as_ref() {
            call.existing_share_preparation()
        } else if let Some(receipt) = slot.ready.as_ref() {
            receipt.existing_share_preparation()
        } else {
            return Err("Computer Use picker discovery already in progress".into());
        }
        .map_err(|_| "Computer Use picker preparation retired")?;
        let ready = view.challenge.is_none() && view.available_tabs > 0;
        let pairing = view
            .challenge
            .map(|challenge| {
                Ok::<_, String>(TaskPickerPairing {
                    endpoint: pinned_pairing_endpoint(&transport.entry)?,
                    nonce: challenge.nonce,
                    instance_id: challenge.instance_id,
                    verification_code: challenge.verification_code,
                    installed_extension_id: challenge.installed_extension_id,
                    expires_at_ms: challenge.expires_at_ms,
                })
            })
            .transpose()?;
        Ok(TaskPickerPreparation {
            started: TaskPickerStarted {
                session_id: session.into(),
                picker_id: picker_id.into(),
                key: key.clone(),
                max_selection: MAX_SCOPE_MEMBERS,
                surface: slot.surface.as_wire(),
            },
            pairing,
            pairing_confirmed: view.pairing_confirmed,
            ready,
            available_tabs: view.available_tabs,
            incompatible_tabs: view.incompatible_tabs,
            borrowed_tabs: view.borrowed_tabs,
        })
    }

    pub(crate) fn confirm_computer_use_task_picker_pairing(
        &self,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        picker_id: &str,
        nonce: &str,
    ) -> Result<(), String> {
        let slots = self.computer_use_connections.lock();
        let transport = slots
            .get(session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?;
        let picker = transport.picker.lock();
        let slot = picker
            .as_ref()
            .filter(|slot| {
                slot.key == *key
                    && slot.id == picker_id
                    && slot.surface == SurfaceKind::ExistingTab
                    && !slot.approving
            })
            .ok_or("Computer Use picker preparation retired or busy")?;
        if let Some(call) = slot.call.as_ref() {
            call.confirm_existing_pairing(nonce)
        } else if let Some(receipt) = slot.ready.as_ref() {
            receipt.confirm_existing_pairing(nonce)
        } else {
            return Err("Computer Use picker discovery already in progress".into());
        }
        .map_err(|_| "Computer Use picker pairing retired or changed".into())
    }
}

/// The endpoint comes from this original Host-generated MCP entry, not a new
/// global bootstrap or a renderer-supplied address. Never clone the env/token.
fn pinned_pairing_endpoint(entry: &Value) -> Result<String, String> {
    let env = entry
        .get("env")
        .and_then(Value::as_array)
        .ok_or("Computer Use original pairing endpoint unavailable")?;
    let mut endpoints = env
        .iter()
        .filter(|row| row.get("name").and_then(Value::as_str) == Some("GROK_APP_CU_IPC"));
    let endpoint = endpoints
        .next()
        .and_then(|row| row.get("value"))
        .and_then(Value::as_str)
        .ok_or("Computer Use original pairing endpoint unavailable")?;
    if endpoints.next().is_some() {
        return Err("Computer Use original pairing endpoint changed".into());
    }
    grok_computer_use_core::browser::require_loopback_base(endpoint)
        .map_err(|_| "Computer Use original pairing endpoint unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_endpoint_is_original_unique_loopback_metadata_only() {
        for endpoint in ["http://127.0.0.1:9", "http://localhost:9", "http://[::1]:9"] {
            let entry = json!({"env":[{"name":"GROK_APP_CU_IPC","value":endpoint}]});
            assert!(pinned_pairing_endpoint(&entry).is_ok());
        }
        for entry in [
            Value::Null,
            json!({"env":[]}),
            json!({"env":[{"name":"GROK_APP_CU_IPC","value":"https://remote.invalid"}]}),
            json!({"env":[{"name":"GROK_APP_CU_IPC","value":"http://127.0.0.1:9"},
                {"name":"GROK_APP_CU_IPC","value":"http://127.0.0.1:10"}]}),
        ] {
            assert!(pinned_pairing_endpoint(&entry).is_err());
        }
    }
}
