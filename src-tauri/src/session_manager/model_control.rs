//! Apply persisted model intent to its explicit App session, never the focus slot.
use super::*;
use crate::{acp_client::AcpClient, store};

type ModelWire = (Arc<AcpClient>, String);

fn live_model_target(session: &mut LiveSession, model: &str) -> Option<ModelWire> {
    session.model_id = Some(model.to_owned());
    session.meta.model_id = Some(model.to_owned());
    if !matches!(
        session.fsm.state(),
        SessionState::Ready | SessionState::Streaming | SessionState::AwaitingPermission
    ) {
        return None;
    }
    model_wire(
        session.acp.as_ref(),
        session.meta.agent_session_id.as_deref(),
    )
}

fn model_wire(acp: Option<&Arc<AcpClient>>, agent_id: Option<&str>) -> Option<ModelWire> {
    let client = acp.filter(|client| client.is_alive())?;
    let agent_id = agent_id.map(str::trim).filter(|id| !id.is_empty())?;
    Some((client.clone(), agent_id.to_owned()))
}

impl SessionManager {
    /// Call only after saving the preference. Queued calls re-read the latest
    /// intent after connect/unpark completes instead of replaying old arguments.
    pub async fn set_model(
        &self,
        requested_model: String,
        session_id: Option<&str>,
    ) -> Result<(), String> {
        if requested_model.trim().is_empty() {
            return Err("model id empty".into());
        }
        let Some(session_id) = session_id.map(str::trim).filter(|id| !id.is_empty()) else {
            // Draft preference applies on its first send, not to a foreign live chat.
            return Ok(());
        };
        let _connect = self.connect_lock.lock().await;
        let Some(meta) = store::load_sessions_snapshot()?
            .into_iter()
            .find(|row| row.id == session_id)
        else {
            return Ok(());
        };
        let prefs = store::resolve_composer_prefs(meta.project_id.as_deref(), Some(session_id));
        self.queue_main_route_rebind(&meta, &prefs).await?;
        if self
            .pending_soft_respawn
            .lock()
            .get(session_id)
            .is_some_and(|reason| reason == "session_channel")
        {
            // Pinned generic models and native capability changes need a new
            // projected config/hooks; an ACP alias switch cannot update them.
            return Ok(());
        }
        let model = prefs.model_id;
        let live = {
            let mut guard = self.inner.lock();
            guard
                .as_mut()
                .filter(|session| session.app_session_id == session_id)
                .map(|session| live_model_target(session, &model))
        };
        // Outer Some means the owning shell was found, even if no wire is ready.
        let target = live
            .or_else(|| {
                self.background
                    .lock()
                    .get_mut(session_id)
                    .map(|session| live_model_target(session, &model))
            })
            .or_else(|| {
                self.parked.lock().get_mut(session_id).map(|session| {
                    session.model_id = Some(model.clone());
                    session.meta.model_id = Some(model.clone());
                    model_wire(Some(&session.acp), session.meta.agent_session_id.as_deref())
                })
            })
            .flatten();
        if let Some((client, agent_id)) = target {
            let agent_model = client.resolve_agent_model(&model);
            client.set_model_for(&agent_id, &agent_model).await?;
        }
        Ok(())
    }
}
