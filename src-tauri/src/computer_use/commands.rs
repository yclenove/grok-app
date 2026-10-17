//! Desktop commands for an approval decision and one redacted screenshot.

use serde::Deserialize;
use tauri::{State, WebviewWindow};

use super::approval::{
    claim_from_body, confirm_from_body, fault_code, handoff_from_body, ConfirmBody, DecisionBody,
    HandoffBody,
};
use super::events::{tool_shot, ShotDto};
use super::ComputerUseHost;
use gcu_host_protocol::approval::{ConfirmEnd, HandoffEnd};

/// Body of `cu_tool_shot`. The id is the screenshot id from tool JSON, not a path.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShotQuery {
    session_id: String,
    id: String,
}

/// Consume the card the user is looking at. Errors are `absent`, `identity`,
/// `replay`, or `failed` — no paths or reasons.
#[tauri::command]
pub async fn cu_approval_decide(
    host: State<'_, ComputerUseHost>,
    body: DecisionBody,
) -> Result<(), String> {
    let office = host.office();
    let claim = claim_from_body(body)?;
    office.decide(claim).await.map_err(fault_code)
}

/// Consume one sensitive confirmation. The desk returns whether it still matches.
#[tauri::command]
pub async fn cu_confirm_decide(
    host: State<'_, ComputerUseHost>,
    body: ConfirmBody,
) -> Result<ConfirmEnd, String> {
    let claim = confirm_from_body(body)?;
    host.office().decide_confirm(&claim).map_err(fault_code)
}

/// Mark a handoff handled. This does not grant and does not clear Stop.
#[tauri::command]
pub async fn cu_handoff_decide(
    host: State<'_, ComputerUseHost>,
    body: HandoffBody,
) -> Result<HandoffEnd, String> {
    let claim = handoff_from_body(body)?;
    host.office().decide_handoff(&claim).map_err(fault_code)
}

/// Redacted thumbnail and full image for the chat tool card.
///
/// Errors are `foreign`, `missing`, or `rejected` — no paths.
#[tauri::command]
pub async fn cu_tool_shot(
    host: State<'_, ComputerUseHost>,
    body: ShotQuery,
) -> Result<ShotDto, String> {
    tool_shot(&host, &body.session_id, &body.id).await
}

/// Global Stop uses the dedicated control connection and persists its latch.
#[tauri::command]
pub async fn cu_stop(
    window: WebviewWindow,
    host: State<'_, ComputerUseHost>,
) -> Result<(), String> {
    trusted_control(&window)?;
    host.user_stop().await.map_err(|_| "failed".into())
}

/// Resume is not a model tool or ordinary Host method.
#[tauri::command]
pub async fn cu_resume(
    window: WebviewWindow,
    host: State<'_, ComputerUseHost>,
) -> Result<(), String> {
    trusted_control(&window)?;
    host.user_resume().await.map_err(|_| "failed".into())
}
fn trusted_control(window: &WebviewWindow) -> Result<(), String> {
    if matches!(window.label(), "main" | super::overlay::CONTROLS) {
        Ok(())
    } else {
        Err("untrusted".into())
    }
}

/// User-editable settings; grants have a separate read/revoke surface.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    enabled: bool,
    confirmation: gcu_host_protocol::messages::ConfirmationLevel,
    foreground: gcu_host_protocol::messages::ForegroundPolicy,
    hotkey: String,
}

/// Read the current switch and policy. No native probe or file IO.
#[tauri::command]
pub async fn cu_settings_get(host: State<'_, ComputerUseHost>) -> Result<SettingsView, String> {
    let policy = host.office().policy();
    Ok(SettingsView {
        enabled: host.status().await.enabled,
        confirmation: policy.sensitive_action_confirmation,
        foreground: policy.allow_foreground,
        hotkey: policy.stop_hotkey,
    })
}

/// Apply the switch, then persist on a blocking worker. Never clears Stop.
#[tauri::command]
pub async fn cu_settings_enabled(
    host: State<'_, ComputerUseHost>,
    enabled: bool,
) -> Result<(), String> {
    let previous = host.office().enabled();
    if !enabled {
        host.user_stop().await.map_err(|_| "failed".to_owned())?;
    }
    host.set_enabled(enabled)
        .await
        .map_err(|_| "failed".to_owned())?;
    let office = host.office();
    let saved = tauri::async_runtime::spawn_blocking(move || office.save_enabled(enabled))
        .await
        .unwrap_or(Err(gcu_host_protocol::approval::Fault::Failed));
    if saved.is_err() {
        let _ = host.set_enabled(previous).await;
    }
    saved.map_err(fault_code)
}

/// Persist only the supported policy choices and await the engine reload.
#[tauri::command]
pub async fn cu_settings_policy(
    host: State<'_, ComputerUseHost>,
    confirmation: gcu_host_protocol::messages::ConfirmationLevel,
    foreground: gcu_host_protocol::messages::ForegroundPolicy,
) -> Result<(), String> {
    host.office()
        .set_preferences(confirmation, foreground)
        .await
        .map_err(fault_code)
}

/// List persistent approvals without issuing a grant or probing native apps.
#[tauri::command]
pub fn cu_approvals_list(
    host: State<'_, ComputerUseHost>,
) -> Vec<gcu_host_protocol::approval::Approval> {
    host.office().approvals()
}

/// Remove one exact stored image and await the engine reload.
#[tauri::command]
pub async fn cu_approvals_revoke(
    host: State<'_, ComputerUseHost>,
    id: String,
) -> Result<(), String> {
    host.office().revoke(&id).await.map_err(fault_code)
}

/// Probe an enabled engine over the authenticated Host protocol (five seconds).
#[tauri::command]
pub async fn cu_doctor(
    host: State<'_, ComputerUseHost>,
) -> Result<gcu_host_protocol::messages::DoctorReport, String> {
    // Wake an idle engine without opening a model session or issuing a token.
    if !host.status().await.enabled {
        return Err("disabled".into());
    }
    host.set_sessions(1)
        .await
        .map_err(|_| "failed".to_owned())?;
    let report = host.office().doctor().await;
    let count = host.lifecycle.live_count();
    let _ = host.set_sessions(count).await;
    report.map_err(fault_code)
}

/// Only fixed OS settings destinations can be opened; never accepts a URL/path.
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemPermission {
    /// macOS Accessibility.
    Accessibility,
    /// macOS Screen Recording.
    ScreenRecording,
    /// Optional macOS Input Monitoring.
    InputMonitoring,
}

/// Open a fixed macOS permission pane. Five-second wait; cancellation kills open.
#[tauri::command]
pub async fn cu_open_system_settings(kind: SystemPermission) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let pane = match kind {
            SystemPermission::Accessibility => "Privacy_Accessibility",
            SystemPermission::ScreenRecording => "Privacy_ScreenCapture",
            SystemPermission::InputMonitoring => "Privacy_ListenEvent",
        };
        let mut child = tokio::process::Command::new("/usr/bin/open")
            .arg(format!(
                "x-apple.systempreferences:com.apple.preference.security?{pane}"
            ))
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "failed".to_owned())?;
        let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .map_err(|_| "failed".to_owned())?
            .map_err(|_| "failed".to_owned())?;
        if status.success() {
            Ok(())
        } else {
            Err("failed".into())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
        Err("unavailable".into())
    }
}
