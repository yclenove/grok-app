//! Actual App child/isolated kernel; file consent and Broker upload are not claimed.

use super::probe_download::lifecycle::app::io;
use super::*;
use grok_computer_use_core::adapter::HostSelectedUploadFile;
use sha2::{Digest, Sha256};
use tauri::Manager;

mod mcp;
mod server;
mod task;
use server::Server;
const TARGET: &str = "resource-browser-upload-target";

pub(crate) fn run_mcp(app: tauri::AppHandle) -> serde_json::Value {
    let settings = crate::store::load_settings();
    let result = if crate::paths::isolated_app_instance_id().is_some()
        && settings.session_data_mode == "independent"
        && !settings.close_to_tray
        && !settings.keep_tray_for_schedules
    {
        io::ready(&app, "main", true).and_then(|()| mcp::check(&app))
    } else {
        Err("upload gate requires an isolated independent App".into())
    };
    let closed = crate::side_browser_host::close(&app, TARGET.into());
    serde_json::json!({"ok":result.is_ok() && closed.is_ok(),"mode":"webview-upload-mcp",
        "nativeProductSideBrowser":true,"nativeFixedKernel":true,"taskGrantPath":true,
        "hostApprovalSynthetic":true,"fileChoiceSynthetic":true,"realMainIpc":true,
        "realWryDownloadCallbacks":false,"realAcp":false,"realMcp":true,"realModel":false,
        "realNativeFilePicker":false,"loginDataImported":false,"result":result.as_ref().ok(),
        "error":result.err(),"closeError":closed.err()})
}

pub(crate) fn run_task(app: tauri::AppHandle) -> serde_json::Value {
    let settings = crate::store::load_settings();
    let result = if crate::paths::isolated_app_instance_id().is_some()
        && settings.session_data_mode == "independent"
        && !settings.close_to_tray
        && !settings.keep_tray_for_schedules
    {
        io::ready(&app, "main", true).and_then(|()| task::check(&app))
    } else {
        Err("upload gate requires an isolated independent App".into())
    };
    let closed = crate::side_browser_host::close(&app, TARGET.into());
    serde_json::json!({"ok":result.is_ok() && closed.is_ok(),"mode":"webview-upload-task",
        "nativeProductSideBrowser":true,"nativeFixedKernel":true,"taskGrantPath":true,
        "hostApprovalSynthetic":true,"fileChoiceSynthetic":true,"realMainIpc":true,
        "realWryDownloadCallbacks":false,"realAcp":false,"realMcp":false,"realModel":false,
        "realNativeFilePicker":false,"loginDataImported":false,"result":result.as_ref().ok(),
        "error":result.err(),"closeError":closed.err()})
}

pub(crate) fn run(app: tauri::AppHandle) -> serde_json::Value {
    let settings = crate::store::load_settings();
    let result = if crate::paths::isolated_app_instance_id().is_some()
        && settings.session_data_mode == "independent"
        && !settings.close_to_tray
        && !settings.keep_tray_for_schedules
    {
        io::ready(&app, "main", true).and_then(|()| check(&app))
    } else {
        Err("upload gate requires an isolated independent App".into())
    };
    let closed = crate::side_browser_host::close(&app, TARGET.into());
    serde_json::json!({"ok":result.is_ok() && closed.is_ok(),"mode":"webview-upload",
        "nativeProductSideBrowser":true,"nativeFixedKernel":true,"taskGrantPath":false,
        "hostApprovalSynthetic":true,"realMainIpc":false,"realWryDownloadCallbacks":false,
        "realAcp":false,"realMcp":false,"realModel":false,"loginDataImported":false,
        "result":result.as_ref().ok(),"error":result.err(),"closeError":closed.err()})
}

fn ready(app: &tauri::AppHandle, predecessor: Option<&str>) -> Result<(), String> {
    let predecessor =
        serde_json::to_string(&predecessor).map_err(|_| "fixture page token invalid")?;
    let script = format!("document.readyState==='complete' && document.querySelector('input')?.type==='file' && typeof uploadPage==='string' && uploadPage!=={predecessor}");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if io::eval(app, TARGET, &script)?.as_bool() == Some(true) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("App upload fixture page not ready".into());
        }
    }
}

fn request(
    adapter: &WebViewAdapter,
    target: &TargetInfo,
    name: &str,
    bytes: Vec<u8>,
) -> Result<UploadRequest, String> {
    let observation = adapter.observe_for_run(
        &parse_id(&target.target_id)
            .ok_or("invalid upload target")?
            .4,
        &target.target_id,
    )?;
    let field = observation
        .nodes
        .iter()
        .find(|node| {
            node.name == "上传文件" && node.actions.iter().any(|action| action == "upload")
        })
        .ok_or("native file input observation missing")?;
    if field
        .actions
        .iter()
        .any(|action| matches!(action.as_str(), "click" | "set_value" | "type_text"))
    {
        return Err(
            "file field advertised native chooser/text input instead of typed upload".into(),
        );
    }
    Ok(UploadRequest {
        run_id: observation.run_id,
        target_id: observation.target_id,
        target_generation: observation.target_generation,
        action_id: uuid::Uuid::new_v4().to_string(),
        snapshot_id: observation.snapshot_id,
        element_ref: field.node_ref.clone(),
        file: HostSelectedUploadFile::from_host_selection(
            name.into(),
            "application/octet-stream".into(),
            bytes,
        )?,
        cancellation: Default::default(),
    })
}

fn receipt(
    app: &tauri::AppHandle,
    server: &Server,
    request: &UploadRequest,
    count: usize,
) -> Result<serde_json::Value, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let events = io::eval(app, TARGET, "({...uploadEvents})")?;
        if events["error"].is_string() {
            return Err("page upload failed".into());
        }
        if events["settled"].as_u64() == Some(count as u64) {
            let actual = server.received();
            let file = actual.last().ok_or("independent receiver file missing")?;
            if actual.len() != count
                || file.bytes != request.file.bytes()
                || file.name != request.file.filename()
                || file.mime != request.file.mime()
                || events["input"].as_u64() != Some(count as u64)
                || events["change"].as_u64() != Some(count as u64)
            {
                return Err("independent upload bytes/name/type/event postcondition failed".into());
            }
            return Ok(
                serde_json::json!({"bytes":file.bytes.len(),"sha256":hex::encode(Sha256::digest(&file.bytes)),
                "inputEvents":events["input"],"changeEvents":events["change"],"receiverRequests":count}),
            );
        }
        if Instant::now() >= deadline {
            return Err("independent upload receiver missed deadline".into());
        }
    }
}

fn assign(adapter: &WebViewAdapter, request: &UploadRequest) -> Result<(), String> {
    let staged = adapter
        .stage_upload(request)
        .map_err(|error| format!("native upload failed: {error:?}"))?;
    if !staged.result().applied || !staged.result().postcondition_ok || !staged.result().verifiable
    {
        return Err("native upload field was not verified".into());
    }
    staged
        .commit(&mut || Ok(()))
        .map_err(|error| error.to_string())
}

fn check(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut server = Server::spawn()?;
    let checked = check_with_server(app, &server);
    let closed = crate::side_browser_host::close(app, TARGET.into());
    let joined = server.finish();
    let mut result = checked?;
    closed?;
    joined?;
    result["emptyConnectionsWithoutHttp"] = serde_json::json!(server.empty_connections());
    Ok(result)
}

fn check_with_server(app: &tauri::AppHandle, server: &Server) -> Result<serde_json::Value, String> {
    io::create(app, TARGET, &server.url(), 40.0)?;
    ready(app, None)?;
    let live = Arc::new(native_view::SideBrowserLiveView::open(
        app.clone(),
        TARGET.into(),
    )?);
    let adapter = WebViewAdapter::new();
    let target = adapter.bind(
        live.clone(),
        "upload-app",
        "upload-run",
        &serde_json::json!({}),
    )?;
    let original = request(
        &adapter,
        &target,
        "示例文档.bin",
        b"Chinese filename, exact bytes: \0\xff\x80".to_vec(),
    )?;
    let mut wrong_generation = original.clone();
    wrong_generation.target_generation += 1;
    if !matches!(
        adapter.stage_upload(&wrong_generation),
        Err(UploadError::NotDispatched(_))
    ) || !server.received().is_empty()
    {
        return Err("mismatched document generation queued file assignment".into());
    }
    assign(&adapter, &original)?;
    let normal = receipt(app, server, &original, 1)?;
    if adapter.stage_upload(&original).is_ok() || server.received().len() != 1 {
        return Err("old upload snapshot replayed".into());
    }
    let stale = request(&adapter, &target, "stale.bin", vec![1])?;
    io::eval(
        app,
        TARGET,
        "document.querySelector('input').setAttribute('accept','image/*'); true",
    )?;
    if adapter.stage_upload(&stale).is_ok() || server.received().len() != 1 {
        return Err("mutated file field accepted a stale reference".into());
    }
    let empty = request(&adapter, &target, "空文件.bin", vec![])?;
    assign(&adapter, &empty)?;
    let empty_receipt = receipt(app, server, &empty, 2)?;
    let paused = request(&adapter, &target, "paused.bin", vec![2])?;
    adapter.abort("upload-run", 2)?;
    if adapter.stage_upload(&paused).is_ok() || server.received().len() != 2 {
        return Err("retired upload survived adapter abort".into());
    }
    let held = request(&adapter, &target, "retired-result.bin", vec![0, 127, 255])?;
    let staged = adapter
        .stage_upload(&held)
        .map_err(|error| format!("upload stage failed: {error:?}"))?;
    adapter.abort("upload-run", 3)?;
    let mut publications = 0;
    if staged
        .commit(&mut || {
            publications += 1;
            Ok(())
        })
        .is_ok()
        || publications != 0
    {
        return Err("retired upload incorrectly published success".into());
    }
    let retired_receipt = receipt(app, server, &held, 3)?;
    if adapter.stage_upload(&held).is_ok() {
        return Err("retired upload repeated its assignment".into());
    }
    let old_page = request(&adapter, &target, "old-page.bin", vec![3])?;
    let predecessor = io::eval(app, TARGET, "uploadPage")?;
    live.navigate(&server.url())?;
    ready(
        app,
        Some(
            predecessor
                .as_str()
                .ok_or("original upload page token missing")?,
        ),
    )?;
    if adapter.stage_upload(&old_page).is_ok() || server.received().len() != 3 {
        return Err("old document upload revived after native navigation".into());
    }
    let next = adapter.bind(
        live,
        "upload-next-app",
        "upload-next-run",
        &serde_json::json!({}),
    )?;
    let full = request(
        &adapter,
        &next,
        "完整边界.bin",
        vec![0xa5; grok_computer_use_core::adapter::MAX_UPLOAD_BYTES],
    )?;
    let started = Instant::now();
    assign(&adapter, &full)?;
    // Navigation reset page events, while the independent receiver retained history.
    let deadline = Instant::now() + Duration::from_secs(5);
    let events = loop {
        let events = io::eval(app, TARGET, "({...uploadEvents})")?;
        if events["settled"] == 1 {
            break events;
        }
        if events["error"].is_string() || Instant::now() >= deadline {
            return Err("full-size native upload did not settle".into());
        }
    };
    let actual = server.received();
    let last = actual.last().ok_or("full-size upload missing")?;
    if actual.len() != 4
        || last.bytes != full.file.bytes()
        || last.name != full.file.filename()
        || last.mime != full.file.mime()
        || events["input"] != 1
        || events["change"] != 1
        || !adapter.is_idle("upload-next-run")
        || app.get_webview(TARGET).is_none()
    {
        return Err("full-size new owner upload postcondition failed".into());
    }
    Ok(
        serde_json::json!({"scenariosPassed":8,"normal":normal,"empty":empty_receipt,
        "wrongGenerationRejected":true,
        "staleFieldRejected":true,"abortBeforeAssignmentRejected":true,"originalReplayRejected":true,
        "retiredPublication":{"published":false,"assigned":true,"replayed":false,"receipt":retired_receipt},
        "oldDocumentRejected":true,"freshOwnerFullSize":{"bytes":last.bytes.len(),
            "sha256":hex::encode(Sha256::digest(&last.bytes)),"elapsedMs":started.elapsed().as_millis()},
        "totalReceiverRequests":actual.len(),"nativeIdle":true}),
    )
}
