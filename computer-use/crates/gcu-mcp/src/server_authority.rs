//! A single boundary for every rmcp request, including SDK default handlers.
use crate::server::Server;
use gcu_core::scope::SCOPE_DENIED_MESSAGE;
use rmcp::{
    model::{ClientNotification, ClientRequest, ServerResult},
    service::{NotificationContext, RequestContext, Service},
    ErrorData, RoleServer, ServerHandler,
};
use serde_json::json;

#[derive(Clone)]
pub(crate) struct Authorized(pub(crate) Server);

impl Service<RoleServer> for Authorized {
    async fn handle_request(
        &self,
        request: ClientRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<ServerResult, ErrorData> {
        let tool = matches!(&request, ClientRequest::CallToolRequest(_));
        if !self.0.usable() {
            return self.denied(tool);
        }
        let work = Service::handle_request(&self.0, request, context);
        let result = if let Some(authority) = self.0.authority() {
            authority.enter(work).await
        } else {
            work.await
        };
        // Tool replies use Results' typed publication/commit boundary, which
        // also retains trusted native unknown delivery and Stop priority.
        if !tool && !self.0.usable() {
            return self.denied(false);
        }
        result
    }

    async fn handle_notification(
        &self,
        notification: ClientNotification,
        context: NotificationContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        if !self.0.usable() {
            return Err(denied());
        }
        Service::handle_notification(&self.0, notification, context).await
    }

    fn get_info(&self) -> rmcp::model::ServerConfig {
        ServerHandler::get_info(&self.0)
    }

    fn supported_protocol_versions(
        &self,
    ) -> std::borrow::Cow<'static, [rmcp::model::ProtocolVersion]> {
        ServerHandler::supported_protocol_versions(&self.0)
    }
}

impl Authorized {
    fn denied(&self, tool: bool) -> Result<ServerResult, ErrorData> {
        if tool {
            Ok(ServerResult::CallToolResult(self.0.scope_refusal()))
        } else {
            Err(denied())
        }
    }
}

fn denied() -> ErrorData {
    ErrorData::invalid_request(
        SCOPE_DENIED_MESSAGE,
        Some(json!({"code":"app_denied","message":SCOPE_DENIED_MESSAGE})),
    )
}
