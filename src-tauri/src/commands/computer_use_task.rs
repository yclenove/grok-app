// Task-level Host APIs. Reading a request does not boot an execution runtime.

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ComputerTaskPreviewDto {
    address: grok_computer_use_core::broker::HostExecutionAddress,
    preview_data_url: Option<String>,
    truncated: bool,
}

#[tauri::command]
pub(crate) async fn computer_use_task_pause(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    address: grok_computer_use_core::broker::HostExecutionAddress,
) -> Result<(), String> {
    computer_task_main_webview(&webview)?;
    mgr.pause_computer_use_task_execution(&session_id, &address)
        .await
}

#[tauri::command]
pub(crate) async fn computer_use_task_resume(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    address: grok_computer_use_core::broker::HostExecutionAddress,
) -> Result<crate::session_manager::TaskResumeOutcome, String> {
    computer_task_main_webview(&webview)?;
    mgr.inner().resume_computer_use_task_execution(&session_id, &address).await
}

#[tauri::command]
pub(crate) async fn computer_use_task_preview(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    address: grok_computer_use_core::broker::HostExecutionAddress,
) -> Result<ComputerTaskPreviewDto, String> {
    computer_task_main_webview(&webview)?;
    let observation = mgr
        .preview_computer_use_task_execution(&session_id, &address)
        .await?;
    Ok(ComputerTaskPreviewDto {
        address,
        preview_data_url: observation
            .image
            .png_base64
            .map(|png| format!("data:image/png;base64,{png}")),
        truncated: observation.truncated,
    })
}

fn computer_task_main_webview(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Computer Use task observers require the main WebView".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn computer_use_task_choose_upload(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    address: grok_computer_use_core::broker::HostExecutionAddress,
    upload_request_id: String,
    dialog_title: String,
) -> Result<grok_computer_use_core::broker::WebViewUploadCompletion, String> {
    computer_task_main_webview(&webview)?;
    // Localized title is supplied by the trusted task card, never the model.
    if dialog_title.is_empty() || dialog_title.len() > 512 || dialog_title.chars().any(char::is_control) {
        return Err("Computer Use file dialog title invalid".into());
    }
    let dialog = rfd::FileDialog::new().set_title(dialog_title).set_parent(&webview.window());
    mgr.choose_computer_use_upload(&session_id, &address, &upload_request_id, dialog).await
}

#[tauri::command]
pub(crate) fn computer_use_task_cancel_upload(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    address: grok_computer_use_core::broker::HostExecutionAddress,
    upload_request_id: String,
) -> Result<grok_computer_use_core::broker::WebViewUploadCompletion, String> {
    computer_task_main_webview(&webview)?;
    mgr.cancel_computer_use_upload(&session_id, &address, &upload_request_id)
}

#[tauri::command]
pub(crate) async fn computer_use_task_subscribe(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    events: tauri::ipc::Channel<crate::computer_use::task_events::TaskEvent>,
) -> Result<crate::computer_use::task_events::TaskSubscriptionStarted, String> {
    computer_task_main_webview(&webview)?;
    let source = mgr.inner().computer_use_task_watch(&session_id)?;
    crate::computer_use::task_events::subscribe(
        &session_id,
        source,
        Arc::new(move |event| {
            events
                .send(event)
                .map_err(|_| "Computer Use observer closed".into())
        }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn computer_use_task_unsubscribe(
    webview: tauri::Webview,
    session_id: String,
    subscription_id: String,
) -> Result<bool, String> {
    computer_task_main_webview(&webview)?;
    crate::computer_use::task_events::unsubscribe(&session_id, &subscription_id).await
}

#[tauri::command]
pub(crate) fn computer_use_task_acknowledge(
    webview: tauri::Webview,
    session_id: String,
    subscription_id: String,
    sequence: String,
) -> Result<(), String> {
    computer_task_main_webview(&webview)?;
    crate::computer_use::task_events::acknowledge(&session_id, &subscription_id, &sequence)
}

#[tauri::command]
pub fn computer_use_task_readback(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
) -> Result<grok_computer_use_core::broker::HostTaskView, String> {
    computer_task_main_webview(&webview)?;
    mgr.computer_use_task_view(&session_id)
}

#[tauri::command]
pub fn computer_use_task_decide(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    key: grok_computer_use_core::task_consent::HostRequestKey,
    decision: grok_computer_use_core::broker::HostTaskDecision,
) -> Result<grok_computer_use_core::broker::HostTaskDecisionReceipt, String> {
    computer_task_main_webview(&webview)?;
    mgr.decide_computer_use_task(&session_id, &key, decision)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerTaskApprovalDto {
    request_id: String,
    revision: u64,
    run_id: String,
    decision_class: &'static str,
}

impl From<grok_computer_use_core::task_consent::CommitOutcome> for ComputerTaskApprovalDto {
    fn from(outcome: grok_computer_use_core::task_consent::CommitOutcome) -> Self {
        Self {
            request_id: outcome.request_id,
            revision: outcome.revision,
            run_id: outcome.run_id,
            decision_class: outcome.decision_class,
        }
    }
}

#[tauri::command]
pub(crate) fn computer_use_task_picker_begin(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    key: grok_computer_use_core::task_consent::HostRequestKey,
    surface: String,
) -> Result<crate::acp_client::computer_use::TaskPickerStarted, String> {
    computer_task_main_webview(&webview)?;
    mgr.begin_computer_use_task_picker(&session_id, &key, &surface)
}

#[tauri::command]
pub(crate) fn computer_use_task_picker_cancel(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    key: grok_computer_use_core::task_consent::HostRequestKey,
    picker_id: String,
) -> Result<bool, String> {
    computer_task_main_webview(&webview)?;
    mgr.cancel_computer_use_task_picker(&session_id, &key, &picker_id)
}

#[tauri::command]
pub(crate) async fn computer_use_task_picker_load(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    key: grok_computer_use_core::task_consent::HostRequestKey,
    picker_id: String,
) -> Result<crate::acp_client::computer_use::TaskPickerView, String> {
    computer_task_main_webview(&webview)?;
    mgr.load_computer_use_task_picker(&session_id, &key, &picker_id)
        .await
}

#[tauri::command]
pub(crate) fn computer_use_task_picker_preparation(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    key: grok_computer_use_core::task_consent::HostRequestKey,
    picker_id: String,
) -> Result<crate::acp_client::computer_use::TaskPickerPreparation, String> {
    computer_task_main_webview(&webview)?;
    mgr.computer_use_task_picker_preparation(&session_id, &key, &picker_id)
}

#[tauri::command]
pub(crate) fn computer_use_task_picker_pairing_confirm(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    key: grok_computer_use_core::task_consent::HostRequestKey,
    picker_id: String,
    nonce: String,
) -> Result<(), String> {
    computer_task_main_webview(&webview)?;
    mgr.confirm_computer_use_task_picker_pairing(&session_id, &key, &picker_id, &nonce)
}

#[tauri::command]
pub async fn computer_use_task_picker_approve(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    key: grok_computer_use_core::task_consent::HostRequestKey,
    picker_id: String,
    candidate_ids: Vec<String>,
) -> Result<ComputerTaskApprovalDto, String> {
    tracing::info!(
        session_id = %session_id,
        request_id = %key.request_id,
        revision = key.revision,
        picker_id = %picker_id,
        selected_count = candidate_ids.len(),
        webview_label = %webview.label(),
        "computer-use task picker approval received"
    );
    if let Err(error) = computer_task_main_webview(&webview) {
        tracing::warn!(
            session_id = %session_id,
            request_id = %key.request_id,
            revision = key.revision,
            picker_id = %picker_id,
            selected_count = candidate_ids.len(),
            webview_label = %webview.label(),
            stage = "main-webview",
            error = %error,
            "computer-use task picker approval failed"
        );
        return Err(error);
    }
    let selected_count = candidate_ids.len();
    match mgr
        .approve_computer_use_task_picker(&session_id, &key, &picker_id, candidate_ids)
        .await
    {
        Ok(outcome) => {
            let receipt: ComputerTaskApprovalDto = outcome.into();
            tracing::info!(
                session_id = %session_id,
                request_id = %receipt.request_id,
                revision = receipt.revision,
                picker_id = %picker_id,
                selected_count,
                run_id = %receipt.run_id,
                decision_class = receipt.decision_class,
                "computer-use task picker approval committed"
            );
            Ok(receipt)
        }
        Err(error) => {
            tracing::warn!(
                session_id = %session_id,
                request_id = %key.request_id,
                revision = key.revision,
                picker_id = %picker_id,
                selected_count,
                error = %error,
                "computer-use task picker approval failed"
            );
            Err(error)
        }
    }
}

#[tauri::command]
pub(crate) async fn computer_use_task_managed_approve(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    key: grok_computer_use_core::task_consent::HostRequestKey,
) -> Result<ComputerTaskApprovalDto, String> {
    computer_task_main_webview(&webview)?;
    mgr.approve_computer_use_managed_task(&session_id, &key)
        .await
        .map(Into::into)
}

#[tauri::command]
pub async fn computer_use_task_approve(
    webview: tauri::Webview,
    mgr: State<'_, Arc<SessionManager>>,
    session_id: String,
    request_id: String,
    revision: u64,
    selected_targets: Vec<crate::computer_use::task_scope::TaskScopeTarget>,
) -> Result<ComputerTaskApprovalDto, String> {
    computer_task_main_webview(&webview)?;
    let outcome = mgr
        .approve_computer_use_task(&session_id, &request_id, revision, selected_targets)
        .await?;
    Ok(outcome.into())
}
