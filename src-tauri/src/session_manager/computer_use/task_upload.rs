//! File choice uses the exact App session and ACP-owned execution, never active-run defaults.
use super::*;
use grok_computer_use_core::broker::{HostExecutionAddress, WebViewUploadCompletion};

impl SessionManager {
    pub(crate) fn cancel_computer_use_upload(
        &self,
        session: &str,
        address: &HostExecutionAddress,
        request: &str,
    ) -> Result<WebViewUploadCompletion, String> {
        let endpoint = self.task_picker_endpoint(session)?;
        endpoint
            .acp
            .computer_use_execution_call(session, &endpoint.agent_session_id, address)?
            .cancel_upload(request)
    }

    pub(crate) async fn choose_computer_use_upload(
        &self,
        session: &str,
        address: &HostExecutionAddress,
        request: &str,
        dialog: rfd::FileDialog,
    ) -> Result<WebViewUploadCompletion, String> {
        let endpoint = self.task_picker_endpoint(session)?;
        let call = endpoint.acp.computer_use_execution_call(
            session,
            &endpoint.agent_session_id,
            address,
        )?;
        let selection = call.select_upload(request)?;
        let completion = crate::computer_use::task_upload::choose(selection, dialog).await?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use original upload owner retired".into());
        }
        Ok(completion)
    }
}
