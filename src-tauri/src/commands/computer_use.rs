// Computer Use Host facade. Native work never blocks the WebView/main thread.
use crate::computer_use::ComputerUseBroker;

fn broker() -> Arc<ComputerUseBroker> {
    crate::computer_use::ensure_host_runtime()
}

async fn cu_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

fn cu_run(
    b: &ComputerUseBroker,
    session: Option<&str>,
    requested: Option<&str>,
) -> Result<String, String> {
    let run = crate::computer_use::sessions::resolve(session, requested)?;
    b.require_owner(session.unwrap_or_default(), &run)
        .map_err(|e| e.to_string())?;
    Ok(run)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerTraceDto {
    pub kind: String,
    pub run_id: String,
    pub detail: String,
    pub ms: u64,
    pub audience: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerTimingsDto {
    pub observe_ms: u64,
    pub act_ms: u64,
    pub verify_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerStatusDto {
    pub run_id: Option<String>,
    pub target_id: Option<String>,
    pub target_name: Option<String>,
    pub paused: bool,
    pub feature_enabled: bool,
    pub enabled: bool,
    pub stop_state: String,
    pub backend: String,
    pub desktop_selection: &'static str,
    pub notes: Vec<String>,
    pub traces: Vec<ComputerTraceDto>,
    pub recovery: Option<String>,
    pub timings: Option<ComputerTimingsDto>,
    pub target_alive: bool,
    pub mcp_catalog: Option<ComputerMcpCatalogStatusDto>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerMcpCatalogStatusDto {
    pub desired_generation: u64,
    pub applied_generation: Option<u64>,
    pub desired_present: bool,
    pub pending: bool,
    pub cleanup_pending: bool,
    pub last_error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerTargetDto {
    pub target_id: String,
    pub title: String,
    pub app_name: String,
    pub kind: String,
    pub surface: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerAuthorizationDto {
    pub attempt_id: String,
    pub selector_revision: u64,
    pub run_id: String,
    pub target: ComputerTargetDto,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerAuthorizationRequest {
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub attempt_id: String,
    pub selector_revision: u64,
    pub surface: String,
    pub target_id: String,
}

fn target_dto(t: grok_computer_use_core::adapter::TargetInfo) -> ComputerTargetDto {
    let surface = grok_computer_use_core::broker::SurfaceRouter::classify_target_id(&t.target_id)
        .as_wire()
        .to_string();
    ComputerTargetDto {
        target_id: t.target_id,
        title: t.title,
        app_name: t.app_name,
        kind: t.kind,
        surface,
    }
}

fn validate_authorization_attempt(
    attempt_id: &str,
    selector_revision: u64,
    target_id: &str,
) -> Result<(), String> {
    validate_authorization_identity(attempt_id, selector_revision)?;
    if target_id.trim().is_empty() {
        return Err("select a Computer Use target first".into());
    }
    Ok(())
}

fn validate_authorization_identity(attempt_id: &str, selector_revision: u64) -> Result<(), String> {
    let attempt_id = attempt_id.trim();
    if attempt_id.is_empty()
        || attempt_id.len() > 128
        || !attempt_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':'))
    {
        return Err("invalid Computer Use authorization attempt".into());
    }
    if selector_revision == 0 {
        return Err("invalid Computer Use selector revision".into());
    }
    Ok(())
}

fn uses_system_picker(surface: grok_computer_use_core::adapter::SurfaceKind, mode: &str) -> bool {
    surface == grok_computer_use_core::adapter::SurfaceKind::Desktop && mode == "portal"
}

fn validate_authorization_selection(
    attempt_id: &str,
    selector_revision: u64,
    target_id: &str,
    system_picker: bool,
) -> Result<(), String> {
    if !system_picker {
        return validate_authorization_attempt(attempt_id, selector_revision, target_id);
    }
    validate_authorization_identity(attempt_id, selector_revision)?;
    if !target_id.trim().is_empty() {
        return Err("native Wayland targets require a new system selection".into());
    }
    Ok(())
}

fn authorized_target_dto(
    b: &ComputerUseBroker,
    run_id: &str,
    surface: grok_computer_use_core::adapter::SurfaceKind,
) -> Result<ComputerTargetDto, String> {
    b.authorized_target_info(run_id, surface)
        .map(target_dto)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerObservationDto {
    pub snapshot_id: String,
    pub geometry_revision: u64,
    pub preview_data_url: Option<String>,
    pub truncated: bool,
}

#[tauri::command]
pub async fn computer_use_status(
    mgr: State<'_, Arc<SessionManager>>,
    session_id: Option<String>,
    run_id: Option<String>,
) -> Result<ComputerStatusDto, String> {
    let manager = mgr.inner().clone();
    cu_blocking(move || {
        let b = broker();
        let rid = cu_run(&b, session_id.as_deref(), run_id.as_deref()).ok();
        let stop = rid
            .as_deref()
            .and_then(|r| b.stop_state(r).ok())
            .map(|s| match s {
                crate::computer_use::StopState::Running => "running",
                crate::computer_use::StopState::StopRequested => "stop_requested",
                crate::computer_use::StopState::Stopped => "stopped",
            })
            .unwrap_or("stopped");
        let cap = rid
            .as_deref()
            .and_then(|run| b.capabilities_for_run(run).ok())
            .unwrap_or_else(|| b.capabilities());
        let traces: Vec<ComputerTraceDto> = rid
            .as_deref()
            .map(|r| b.traces(Some(r), false))
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.audience == crate::computer_use::TraceAudience::Ui)
            .map(|t| ComputerTraceDto {
                kind: t.kind,
                run_id: t.run_id,
                detail: t.detail,
                ms: t.ms,
                audience: "ui".into(),
            })
            .collect();
        let traces = traces.into_iter().rev().take(24).rev().collect();
        let recovery = rid
            .as_deref()
            .and_then(|r| b.recovery_source(r))
            .map(|r| match r {
                crate::computer_use::RecoverySource::User => "user".into(),
                crate::computer_use::RecoverySource::System => "system".into(),
            });
        let timings = rid
            .as_deref()
            .and_then(|r| b.timings(r).ok())
            .map(|t| ComputerTimingsDto {
                observe_ms: t.observe_ms,
                act_ms: t.act_ms,
                verify_ms: t.verify_ms,
            });
        let target_alive = rid.as_deref().is_some_and(|r| b.authorized_target_alive(r));
        let mcp_catalog = session_id
            .as_deref()
            .and_then(|sid| manager.mcp_catalog_status(sid))
            .map(|status| ComputerMcpCatalogStatusDto {
                desired_generation: status.desired_generation,
                applied_generation: status.applied_generation,
                desired_present: status.desired_present,
                pending: status.pending,
                cleanup_pending: !status.desired_present
                    && (status.pending || status.resource_cleanup_pending),
                last_error: status.last_error,
            });
        Ok(ComputerStatusDto {
            target_id: rid
                .as_deref()
                .and_then(|r| b.authorized_target(r).ok())
                .map(|t| t.0),
            target_name: rid.as_deref().and_then(|r| b.target_title(r)),
            paused: rid
                .as_deref()
                .and_then(|r| b.is_paused(r).ok())
                .unwrap_or(false),
            run_id: rid,
            feature_enabled: b.feature_enabled(),
            enabled: session_id
                .as_deref()
                .is_some_and(crate::computer_use::is_session_enabled),
            stop_state: stop.into(),
            backend: cap.backend_id,
            desktop_selection: crate::computer_use::desktop_selection_mode(),
            notes: cap.notes,
            traces,
            recovery,
            timings,
            target_alive,
            mcp_catalog,
        })
    })
    .await
}

#[tauri::command]
pub async fn computer_use_set_feature(
    mgr: State<'_, Arc<SessionManager>>,
    enabled: bool,
) -> Result<(), String> {
    let manager = mgr.inner().clone();
    crate::computer_use::persist_feature_transition(manager, enabled, move || {
        let mut settings = store::load_settings();
        settings.computer_use_enabled = enabled;
        store::save_settings(&settings)
    })
    .await
}

#[tauri::command]
pub async fn computer_use_list_targets(
    session_id: Option<String>,
    run_id: Option<String>,
    surface: Option<String>,
) -> Result<Vec<ComputerTargetDto>, String> {
    cu_blocking(move || {
        computer_use_picker_targets(
            &broker(),
            session_id.as_deref(),
            run_id.as_deref(),
            surface.as_deref(),
        )
    })
    .await
}

fn computer_use_picker_targets(
    broker: &ComputerUseBroker,
    session: Option<&str>,
    requested_run: Option<&str>,
    surface: Option<&str>,
) -> Result<Vec<ComputerTargetDto>, String> {
    computer_use_picker_targets_with_mode(
        broker,
        session,
        requested_run,
        surface,
        crate::computer_use::desktop_selection_mode(),
    )
}

fn computer_use_picker_targets_with_mode(
    broker: &ComputerUseBroker,
    session: Option<&str>,
    requested_run: Option<&str>,
    surface: Option<&str>,
    selection_mode: &str,
) -> Result<Vec<ComputerTargetDto>, String> {
    if !broker.feature_enabled() {
        return Ok(Vec::new());
    }
    let sid = session
        .filter(|s| !s.trim().is_empty())
        .ok_or("select a local chat first")?;
    let owned_run = requested_run
        .map(|requested| cu_run(broker, Some(sid), Some(requested)))
        .transpose()?;
    let kind = grok_computer_use_core::adapter::SurfaceKind::from_wire(surface.unwrap_or(""))?;
    if owned_run.is_none() && uses_system_picker(kind, selection_mode) {
        // Discovery is read-only. No synthetic monitor and no system consent.
        return Ok(Vec::new());
    }
    // A validated run is not just an admission check: run-owned portal and
    // WebView adapters require that exact identity during enumeration.
    // Before the first authorization, preserve the Host discovery picker.
    match owned_run.as_deref() {
        Some(run) => broker.list_targets_for_surface_in_run(kind, run),
        None => broker.list_targets_for_surface(kind),
    }
    .map(|rows| rows.into_iter().map(target_dto).collect())
    .map_err(|e| e.to_string())
}

fn computer_use_prepare_target(
    ticket: &crate::computer_use::sessions::AuthorizationTicket,
    prepare: impl FnOnce() -> Result<String, String>,
    discard: impl FnOnce(),
) -> Result<String, String> {
    ticket.check_preparation()?;
    let target = prepare()?;
    if let Err(error) = ticket.check_preparation() {
        discard();
        return Err(error);
    }
    Ok(target)
}

#[tauri::command]
pub async fn computer_use_authorize_surface(
    app: tauri::AppHandle,
    window: tauri::Window,
    mgr: State<'_, Arc<SessionManager>>,
    request: ComputerAuthorizationRequest,
) -> Result<ComputerAuthorizationDto, String> {
    let ComputerAuthorizationRequest {
        session_id,
        run_id,
        attempt_id,
        selector_revision,
        surface,
        target_id,
    } = request;
    let sid = session_id
        .filter(|session| !session.trim().is_empty())
        .ok_or("select a local chat first")?;
    crate::computer_use::refuse_if_not_local(&sid)?;
    let surface = grok_computer_use_core::broker::SurfaceRouter::parse_wire(&surface)
        .map_err(|e| e.to_string())?;
    let system_picker = uses_system_picker(surface, crate::computer_use::desktop_selection_mode());
    validate_authorization_selection(&attempt_id, selector_revision, &target_id, system_picker)?;
    #[cfg(not(target_os = "linux"))]
    let _ = window;
    let b = broker();
    let manager = mgr.inner().clone();
    let ticket = crate::computer_use::sessions::begin(
        &b,
        &sid,
        run_id.as_deref(),
        &attempt_id,
        selector_revision,
    )?;
    let work_broker = b.clone();
    let work_ticket = ticket.clone();
    let work_sid = sid.clone();
    let requested_target = target_id.trim().to_string();
    let transaction = async {
        // Only this explicit local UI command can open a system picker. Its
        // parent comes from Tauri's invoking window, never a renderer label.
        #[cfg(target_os = "linux")]
        let mut portal = if system_picker {
            Some(crate::computer_use::linux_portal::prepare(window, &ticket).await?)
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        let requested_target = portal
            .as_ref()
            .map(|p| p.target().to_string())
            .unwrap_or(requested_target);
        let target = cu_blocking(move || {
            let target_id = computer_use_prepare_target(
                &work_ticket,
                || {
                    if surface == grok_computer_use_core::adapter::SurfaceKind::WebView
                        && !requested_target.starts_with("wv|")
                        && !requested_target.starts_with("wv:")
                        && !requested_target.starts_with("webview:")
                    {
                        Ok(crate::computer_use::product_webview()
                            .bind_side_browser(
                                app,
                                &requested_target,
                                &work_sid,
                                &work_ticket.run_id,
                            )
                            .map_err(|e| e.to_string())?
                            .target_id)
                    } else {
                        Ok(requested_target)
                    }
                },
                || {
                    if surface == grok_computer_use_core::adapter::SurfaceKind::WebView {
                        crate::computer_use::product_webview()
                            .unbind_for_run(&work_sid, &work_ticket.run_id);
                    }
                },
            )?;
            if let Err(error) = work_broker.authorize_on_surface(
                &work_sid,
                &work_ticket.run_id,
                surface,
                &target_id,
            ) {
                if surface == grok_computer_use_core::adapter::SurfaceKind::WebView {
                    crate::computer_use::product_webview()
                        .unbind_for_run(&work_sid, &work_ticket.run_id);
                }
                return Err(error.to_string());
            }
            crate::computer_use::sessions::activate(&work_broker, &work_ticket)?;
            authorized_target_dto(&work_broker, &work_ticket.run_id, surface)
        })
        .await?;
        manager.attach_computer_use(&sid, &ticket.run_id).await?;
        let complete_broker = b.clone();
        let complete_ticket = ticket.clone();
        cu_blocking(move || {
            crate::computer_use::sessions::complete(&complete_broker, &complete_ticket)
        })
        .await?;
        #[cfg(target_os = "linux")]
        if let Some(portal) = portal.as_mut() {
            portal.commit();
        }
        Ok::<_, String>(ComputerAuthorizationDto {
            attempt_id: ticket.attempt_id.clone(),
            selector_revision: ticket.selector_revision,
            run_id: ticket.run_id.clone(),
            target,
        })
    }
    .await;

    match transaction {
        Ok(result) => Ok(result),
        Err(error) => {
            let cleanup = manager.fence_failed_computer_use_authorization(b.clone(), &ticket);
            manager.spawn_fenced_computer_use_stop(sid, cleanup, None);
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn computer_use_cancel_authorization(
    mgr: State<'_, Arc<SessionManager>>,
    session_id: Option<String>,
    attempt_id: String,
    selector_revision: u64,
) -> Result<bool, String> {
    validate_authorization_identity(&attempt_id, selector_revision)?;
    let sid = session_id
        .filter(|session| !session.trim().is_empty())
        .ok_or("select a local chat first")?;
    crate::computer_use::refuse_if_not_local(&sid)?;
    let manager = mgr.inner().clone();
    let (cancelled, cleanup) = manager.fence_cancelled_computer_use_authorization(
        broker(),
        &sid,
        &attempt_id,
        selector_revision,
    )?;
    if let Some(cleanup) = cleanup {
        manager.spawn_fenced_computer_use_stop(sid, cleanup, None);
    }
    Ok(cancelled)
}

#[tauri::command]
pub async fn computer_use_observe(
    session_id: Option<String>,
    run_id: Option<String>,
) -> Result<Option<ComputerObservationDto>, String> {
    cu_blocking(move || {
        let b = broker();
        let rid = cu_run(&b, session_id.as_deref(), run_id.as_deref())?;
        // Busy is a skipped UI frame, not an adapter failure. The UI retains
        // its previous frame as stale and retries on its normal poll interval.
        let obs = match b.observe_preview(&rid) {
            Ok(obs) => obs,
            Err(crate::computer_use::BrokerError::LeaseHeld { .. }) => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        Ok(Some(ComputerObservationDto {
            snapshot_id: obs.snapshot_id,
            geometry_revision: obs.geometry_revision,
            preview_data_url: obs
                .image
                .png_base64
                .map(|d| format!("data:image/png;base64,{d}")),
            truncated: obs.truncated,
        }))
    })
    .await
}

#[tauri::command]
pub async fn computer_use_pause(
    session_id: Option<String>,
    run_id: Option<String>,
) -> Result<(), String> {
    cu_blocking(move || {
        let b = broker();
        b.pause(&cu_run(&b, session_id.as_deref(), run_id.as_deref())?)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn computer_use_resume(
    session_id: Option<String>,
    run_id: Option<String>,
) -> Result<(), String> {
    cu_blocking(move || {
        let b = broker();
        b.resume(&cu_run(&b, session_id.as_deref(), run_id.as_deref())?)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn computer_use_takeover(
    session_id: Option<String>,
    run_id: Option<String>,
) -> Result<(), String> {
    cu_blocking(move || {
        let b = broker();
        b.takeover(&cu_run(&b, session_id.as_deref(), run_id.as_deref())?)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn computer_use_stop(
    mgr: State<'_, Arc<SessionManager>>,
    session_id: Option<String>,
    run_id: Option<String>,
) -> Result<(), String> {
    let sid = session_id
        .as_deref()
        .ok_or("select a local chat first")?
        .to_string();
    let b = broker();
    // Validate an explicitly supplied run before fencing.  This lookup is
    // in-memory; it must not be coupled to the slow cleanup worker.
    if run_id.is_some() {
        cu_run(&b, Some(&sid), run_id.as_deref())?;
    }
    let plan = mgr.inner().fence_computer_use_stop_with_broker(b, &sid);
    // The command acknowledges the local Stop immediately.  ACP catalog and
    // adapter/browser cleanup are generation-bound and continue asynchronously
    // with an explicit retry ledger on failure.
    mgr.inner().spawn_fenced_computer_use_stop(sid, plan, None);
    Ok(())
}

#[tauri::command]
pub async fn computer_use_retry_cleanup(
    mgr: State<'_, Arc<SessionManager>>,
    session_id: Option<String>,
) -> Result<ComputerMcpCatalogStatusDto, String> {
    computer_use_retry_cleanup_impl(mgr.inner().as_ref(), session_id).await
}

async fn computer_use_retry_cleanup_impl(
    mgr: &SessionManager,
    session_id: Option<String>,
) -> Result<ComputerMcpCatalogStatusDto, String> {
    let sid = session_id
        .filter(|session| !session.trim().is_empty())
        .ok_or("select a local chat first")?;
    mgr.retry_computer_use_cleanup(&sid).await?;
    let status = mgr
        .mcp_catalog_status(&sid)
        .ok_or("Computer Use cleanup status is unavailable")?;
    Ok(ComputerMcpCatalogStatusDto {
        desired_generation: status.desired_generation,
        applied_generation: status.applied_generation,
        desired_present: status.desired_present,
        pending: status.pending,
        cleanup_pending: !status.desired_present
            && (status.pending || status.resource_cleanup_pending),
        last_error: status.last_error,
    })
}

#[tauri::command]
pub async fn computer_use_set_preview(
    session_id: Option<String>,
    run_id: Option<String>,
    visible: bool,
) -> Result<(), String> {
    cu_blocking(move || {
        let b = broker();
        let rid = cu_run(&b, session_id.as_deref(), run_id.as_deref())?;
        b.set_preview_visible(&rid, visible)
            .map_err(|e| e.to_string())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerRuntimeIssueDto {
    pub code: String,
    pub component: String,
    pub action: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerRuntimeStatusDto {
    pub issues: Vec<ComputerRuntimeIssueDto>,
    pub can_repair: bool,
    pub can_rollback: bool,
}

fn runtime_issue_dto(
    issue: grok_computer_use_core::runtime::RuntimeIssue,
) -> ComputerRuntimeIssueDto {
    ComputerRuntimeIssueDto {
        code: issue.code,
        component: issue.component,
        action: issue.action,
    }
}

#[tauri::command]
pub async fn computer_use_runtime_status() -> Result<ComputerRuntimeStatusDto, String> {
    cu_blocking(|| {
        let issues = crate::computer_use::runtime::diagnose();
        Ok(ComputerRuntimeStatusDto {
            can_repair: true,
            can_rollback: crate::computer_use::runtime::can_rollback(),
            issues: issues.into_iter().map(runtime_issue_dto).collect(),
        })
    })
    .await
}

#[tauri::command]
pub async fn computer_use_runtime_repair(app: tauri::AppHandle) -> Result<(), String> {
    cu_blocking(move || {
        use tauri::Manager;
        let resource = app
            .path()
            .resource_dir()
            .unwrap_or_else(|_| std::path::PathBuf::new());
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default();
        crate::computer_use::runtime::repair_from_install(&resource, &exe_dir).map(|_| ())
    })
    .await
}

#[tauri::command]
pub async fn computer_use_runtime_rollback() -> Result<(), String> {
    cu_blocking(|| crate::computer_use::runtime::rollback().map(|_| ())).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerPairingChallengeDto {
    pub nonce: String,
    pub instance_id: String,
    pub verification_code: String,
    pub endpoint: String,
    pub installed_extension_id: String,
    pub expires_at_ms: u64,
}

#[tauri::command]
pub async fn computer_use_begin_pairing() -> Result<ComputerPairingChallengeDto, String> {
    cu_blocking(|| {
        let broker = broker();
        let endpoint = crate::computer_use::ipc::ensure(broker.clone())?
            .url
            .clone();
        let ch = broker.begin_pairing()?;
        Ok(ComputerPairingChallengeDto {
            nonce: ch.nonce,
            instance_id: ch.instance_id,
            verification_code: ch.verification_code,
            endpoint,
            installed_extension_id: ch.installed_extension_id,
            expires_at_ms: ch.expires_at_ms,
        })
    })
    .await
}

#[tauri::command]
pub async fn computer_use_confirm_pairing_app(nonce: String) -> Result<(), String> {
    cu_blocking(move || broker().confirm_pairing(&nonce)).await
}

#[tauri::command]
pub async fn computer_use_revoke_pairing() -> Result<(), String> {
    cu_blocking(|| {
        broker().tabs().revoke_pairing();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn computer_use_list_shared_tabs() -> Result<Vec<ComputerTargetDto>, String> {
    cu_blocking(|| {
        Ok(broker()
            .list_targets_for_surface(grok_computer_use_core::adapter::SurfaceKind::ExistingTab)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(target_dto)
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn computer_use_unbind_webview(
    mgr: State<'_, Arc<SessionManager>>,
    session_id: Option<String>,
    run_id: Option<String>,
) -> Result<(), String> {
    let sid = session_id
        .filter(|session| !session.trim().is_empty())
        .ok_or("select a local chat first")?;
    let b = broker();
    let rid = cu_run(&b, Some(&sid), run_id.as_deref())?;
    let (target_id, _) = b
        .authorized_target(&rid)
        .map_err(|error| error.to_string())?;
    if grok_computer_use_core::broker::SurfaceRouter::classify_target_id(&target_id)
        != grok_computer_use_core::adapter::SurfaceKind::WebView
    {
        return Err("the authorized Computer Use target is not an App WebView".into());
    }
    let manager = mgr.inner().clone();
    let plan = manager.fence_computer_use_stop_with_broker(b, &sid);
    manager.spawn_fenced_computer_use_stop(sid, plan, None);
    Ok(())
}

#[tauri::command]
pub async fn computer_use_export_bundle() -> Result<String, String> {
    cu_blocking(|| {
        crate::computer_use::diagnostics::export_bundle().map(|p| p.display().to_string())
    })
    .await
}

#[tauri::command]
pub async fn computer_use_clear_traces() -> Result<(), String> {
    cu_blocking(|| {
        crate::computer_use::diagnostics::clear(
            grok_computer_use_core::privacy::CleanupKind::Traces,
        )
    })
    .await
}

#[tauri::command]
pub async fn computer_use_clear_staging() -> Result<(), String> {
    cu_blocking(|| {
        crate::computer_use::diagnostics::clear(
            grok_computer_use_core::privacy::CleanupKind::Staging,
        )
    })
    .await
}

#[tauri::command]
pub async fn computer_use_clear_managed_profiles() -> Result<(), String> {
    cu_blocking(|| {
        crate::computer_use::diagnostics::clear(
            grok_computer_use_core::privacy::CleanupKind::ManagedProfiles,
        )?;
        broker().tabs().revoke_pairing();
        Ok(())
    })
    .await
}

#[cfg(test)]
#[path = "computer_use_command_tests.rs"]
mod computer_use_command_tests;
