//! Windows WebView2 download coordinator for App-owned side-browser views.
//!
//! The model never sees the resolved URL or destination. Wry keeps its ordinary
//! user-download flow; only an exact native-instance expectation is intercepted.

use super::*;
use ::windows::core::{Interface, HSTRING, PWSTR};
use ::windows::Win32::System::Com::CoTaskMemFree;
use parking_lot::{Condvar, Mutex};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2DownloadOperation, ICoreWebView2PermissionRequestedEventArgs3,
    ICoreWebView2_4, COREWEBVIEW2_DOWNLOAD_STATE, COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
    COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS,
    COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
};
use webview2_com::{
    DownloadStartingEventHandler, PermissionRequestedEventHandler, StateChangedEventHandler,
};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
const CANCEL_GRACE: Duration = Duration::from_secs(8);

mod admission;
mod close;
mod destination;
mod exit;
mod journal;
use admission::register;
#[cfg(feature = "computer-use-probe")]
pub(crate) mod probe;
#[cfg(feature = "computer-use-probe")]
pub(crate) use close::{probe_resources_retired, retire_instance_now, uninstall_instance_now};
pub(crate) use close::{retire_instance, uninstall_instance, wait_retired};
pub(crate) use journal::maintain as maintain_download_buffers;

static NEXT_EXPECTATION: AtomicU64 = AtomicU64::new(1);
static INSTALLS: LazyLock<Mutex<HashMap<u64, Arc<InstallRecord>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static EXPECTATIONS: LazyLock<Mutex<HashMap<u64, Arc<Expectation>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

thread_local! {
    /// WebView2 download COM interfaces remain on the native UI thread. Worker
    /// cancellation is posted back through `with_webview` and resolves here.
    static NATIVE_OPERATIONS: RefCell<HashMap<u64, NativeDownload>> =
        RefCell::new(HashMap::new());
}

struct NativeDownload {
    instance: u64,
    operation: ICoreWebView2DownloadOperation,
}

struct InstallRecord {
    state: Mutex<InstallState>,
    owners: Mutex<Option<journal::Owners>>,
    changed: Condvar,
}

enum InstallState {
    Pending,
    Ready,
    Failed,
    Closed,
}

impl InstallRecord {
    fn pending() -> Self {
        Self {
            state: Mutex::new(InstallState::Pending),
            owners: Mutex::new(None),
            changed: Condvar::new(),
        }
    }

    fn finish(&self, ready: bool) {
        let mut state = self.state.lock();
        if matches!(*state, InstallState::Closed) {
            return;
        }
        *state = if ready {
            InstallState::Ready
        } else {
            InstallState::Failed
        };
        self.changed.notify_all();
    }
}

struct Expectation {
    id: u64,
    instance: u64,
    url: String,
    destination: PathBuf,
    native_destination: destination::NativeDestination,
    operation: Mutex<Option<ScriptOperation>>,
    state: Mutex<ExpectationState>,
    changed: Condvar,
}

#[derive(Default)]
struct ExpectationState {
    wry_requested: bool,
    wry_finished: bool,
    native_started: bool,
    cancel_requested: bool,
    caller_done: bool,
    abandoned: bool,
    native_exited: bool,
    terminal: Option<NativeTerminal>,
}

#[derive(Clone, Copy)]
enum NativeTerminal {
    Completed,
    Interrupted,
    InvalidDestination,
    OwnerExited,
}

pub(crate) fn schedule_handler(webview: &tauri::Webview, instance: u64) -> Result<(), String> {
    let record = {
        let mut installs = INSTALLS.lock();
        if installs.contains_key(&instance) {
            return Ok(());
        }
        let record = Arc::new(InstallRecord::pending());
        installs.insert(instance, record.clone());
        record
    };
    let scheduled_record = record.clone();
    let scheduled = webview.with_webview(move |platform| {
        if matches!(*scheduled_record.state.lock(), InstallState::Closed) {
            return;
        }
        let ready = match install_handler_on_platform(platform, instance) {
            Ok(owners) => {
                *scheduled_record.owners.lock() = Some(owners);
                true
            }
            Err(_) => false,
        };
        scheduled_record.finish(ready);
        if !ready
            || (matches!(*scheduled_record.state.lock(), InstallState::Closed)
                && !close::physical_pending(instance))
        {
            exit::detach(instance);
        }
    });
    if scheduled.is_err() {
        record.finish(false);
        return Err("WebView native download handler unavailable".into());
    }
    Ok(())
}

fn ensure_handler(webview: &tauri::Webview, instance: u64) -> Result<(), String> {
    schedule_handler(webview, instance)?;
    let record = INSTALLS
        .lock()
        .get(&instance)
        .cloned()
        .ok_or_else(|| "WebView native download handler unavailable".to_string())?;
    let deadline = Instant::now() + INSTALL_TIMEOUT;
    let mut state = record.state.lock();
    loop {
        match *state {
            InstallState::Ready => return Ok(()),
            InstallState::Failed | InstallState::Closed => {
                return Err("WebView native download handler unavailable".into())
            }
            InstallState::Pending => {}
        }
        let now = Instant::now();
        if now >= deadline {
            return Err("WebView native download handler unavailable".into());
        }
        record.changed.wait_for(&mut state, deadline - now);
    }
}

fn install_handler_on_platform(
    platform: tauri::webview::PlatformWebview,
    instance: u64,
) -> Result<journal::Owners, String> {
    let core = unsafe { platform.controller().CoreWebView2() }
        .map_err(|_| "WebView native controller unavailable")?;
    install_handler_on_core(&core, instance)
}

fn install_handler_on_core(core: &ICoreWebView2, instance: u64) -> Result<journal::Owners, String> {
    let permission_handler = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut kind = Default::default();
        unsafe { args.PermissionKind(&mut kind)? };
        if kind == COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS {
            let mut raw_uri = PWSTR::null();
            unsafe { args.Uri(&mut raw_uri)? };
            let permission_uri = take_pwstr(raw_uri);
            if !pending_expectation_matches_origin(instance, &permission_uri) {
                return Ok(());
            }
            if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                let _ = unsafe { args3.SetSavesInProfile(false) };
            }
            unsafe { args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)? };
        }
        Ok(())
    }));
    let mut permission_token = 0;
    unsafe { core.add_PermissionRequested(&permission_handler, &mut permission_token) }
        .map_err(|_| "WebView native download permission handler unavailable".to_string())?;

    let webview4: ICoreWebView2_4 = core
        .cast()
        .map_err(|_| "WebView native download handler unavailable")?;
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let operation = unsafe { args.DownloadOperation()? };
        let uri = read_download_uri(&operation)?;
        let Some(expectation) = claim_native_start(instance, &uri) else {
            if has_pending_expectation(instance) {
                let _ = unsafe { args.SetCancel(true) };
                let _ = unsafe { args.SetHandled(true) };
            }
            return Ok(());
        };
        #[cfg(feature = "computer-use-probe")]
        probe::record(instance, probe::Event::NativeStarted);

        let path = windows_path(expectation.native_destination.path());
        let path = HSTRING::from(path.as_str());
        if unsafe { args.SetResultFilePath(&path) }.is_err()
            || unsafe { args.SetHandled(true) }.is_err()
        {
            let _ = unsafe { args.SetCancel(true) };
            complete(&expectation, NativeTerminal::InvalidDestination);
            return Ok(());
        }

        NATIVE_OPERATIONS.with(|operations| {
            operations.borrow_mut().insert(
                expectation.id,
                NativeDownload {
                    instance,
                    operation: operation.clone(),
                },
            );
        });
        let expectation_id = expectation.id;
        let state_handler = StateChangedEventHandler::create(Box::new(move |operation, _| {
            let Some(operation) = operation else {
                return Ok(());
            };
            let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
            unsafe { operation.State(&mut state)? };
            if state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
                return Ok(());
            }
            #[cfg(feature = "computer-use-probe")]
            {
                probe::record(instance, probe::Event::NativeTerminal);
                probe::native_state(&operation);
                probe::record_stage(
                    instance,
                    if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
                        "native-completed"
                    } else {
                        "native-interrupted"
                    },
                );
                eprintln!("gate: webview_download_terminal instance={instance}");
            }
            let terminal = if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
                match read_result_path(&operation) {
                    Ok(path) if result_path_matches(expectation_id, &path) => {
                        NativeTerminal::Completed
                    }
                    _ => NativeTerminal::InvalidDestination,
                }
            } else {
                NativeTerminal::Interrupted
            };
            NATIVE_OPERATIONS.with(|operations| {
                operations.borrow_mut().remove(&expectation_id);
            });
            complete_by_id(expectation_id, terminal);
            Ok(())
        }));
        let mut token = 0;
        if unsafe { operation.add_StateChanged(&state_handler, &mut token) }.is_err() {
            let _ = unsafe { operation.Cancel() };
            complete(&expectation, NativeTerminal::Interrupted);
            NATIVE_OPERATIONS.with(|operations| {
                operations.borrow_mut().remove(&expectation.id);
            });
            return Ok(());
        }
        if expectation.state.lock().cancel_requested {
            let _ = unsafe { operation.Cancel() };
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview4.add_DownloadStarting(&handler, &mut token) }
        .map_err(|_| "WebView native download handler unavailable".to_string())?;
    exit::install(core, instance)
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn install_probe_handlers(
    core: &ICoreWebView2,
    instance: u64,
    requested: Arc<AtomicU64>,
    claimed: Arc<AtomicU64>,
    finished: Arc<AtomicU64>,
) -> Result<(), String> {
    let owners = install_handler_on_core(core, instance)?;
    let record = Arc::new(InstallRecord::pending());
    *record.owners.lock() = Some(owners);
    record.finish(true);
    INSTALLS.lock().insert(instance, record);
    let webview4: ICoreWebView2_4 = core
        .cast()
        .map_err(|_| "probe WebView native download bridge unavailable")?;
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
        requested.fetch_add(1, Ordering::SeqCst);
        let Some(args) = args else {
            return Ok(());
        };
        let operation = unsafe { args.DownloadOperation()? };
        let uri = read_download_uri(&operation)?;
        let mut destination = PathBuf::new();
        if !claim_wry_requested(instance, &uri, &mut destination) {
            return Ok(());
        }
        claimed.fetch_add(1, Ordering::SeqCst);
        let completed_uri = uri.clone();
        let completed_count = finished.clone();
        let settled = Arc::new(AtomicBool::new(false));
        let state_handler = StateChangedEventHandler::create(Box::new(move |operation, _| {
            let Some(operation) = operation else {
                return Ok(());
            };
            let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
            unsafe { operation.State(&mut state)? };
            if state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS
                || settled.swap(true, Ordering::SeqCst)
            {
                return Ok(());
            }
            if state != COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
                let mut reason = Default::default();
                let _ = unsafe { operation.InterruptReason(&mut reason) };
                eprintln!(
                    "gate: webview_download_bridge_terminal state={} reason={}",
                    state.0, reason.0
                );
            }
            let path = read_result_path(&operation).ok();
            // Record receipt of this claimed operation's native terminal
            // before notifying its waiting caller through the Wry bridge.
            completed_count.fetch_add(1, Ordering::SeqCst);
            finish_wry(
                instance,
                &completed_uri,
                path.as_deref(),
                state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
            );
            Ok(())
        }));
        let mut token = 0;
        unsafe { operation.add_StateChanged(&state_handler, &mut token)? };
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview4.add_DownloadStarting(&handler, &mut token) }
        .map_err(|_| "probe WebView native download bridge unavailable".to_string())
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn cancel_probe_download_now(id: u64) {
    cancel_on_native_thread(id);
}

pub(crate) fn claim_wry_requested(instance: u64, url: &str, destination: &mut PathBuf) -> bool {
    let Some(expectation) = matching_expectation(instance, url) else {
        return false;
    };
    let mut state = expectation.state.lock();
    if state.wry_requested || state.terminal.is_some() {
        return false;
    }
    state.wry_requested = true;
    *destination = expectation.native_destination.path().to_path_buf();
    #[cfg(feature = "computer-use-probe")]
    probe::record(instance, probe::Event::WryRequested);
    true
}

pub(crate) fn finish_wry(instance: u64, url: &str, path: Option<&Path>, success: bool) -> bool {
    let Some(expectation) = matching_expectation(instance, url) else {
        return false;
    };
    #[cfg(feature = "computer-use-probe")]
    {
        probe::record(instance, probe::Event::WryFinished);
        if probe::hold_finish(&expectation, path, success) {
            return true;
        }
    }
    finish_wry_expectation(&expectation, path, success)
}

fn finish_wry_expectation(
    expectation: &Arc<Expectation>,
    path: Option<&Path>,
    success: bool,
) -> bool {
    {
        let mut state = expectation.state.lock();
        if !state.wry_requested && !state.native_started {
            return false;
        }
        state.wry_finished = true;
    }
    let terminal = if success {
        match path {
            Some(path) if same_path(path, expectation.native_destination.path()) => {
                NativeTerminal::Completed
            }
            _ => NativeTerminal::InvalidDestination,
        }
    } else {
        NativeTerminal::Interrupted
    };
    #[cfg(feature = "computer-use-probe")]
    probe::record(expectation.instance, probe::Event::WryProcessed);
    complete(expectation, terminal);
    true
}

pub(crate) fn cancel_instance(webview: &tauri::Webview, instance: u64) {
    mark_instance_cancelled(instance);
    let _ = webview.with_webview(move |_| cancel_instance_on_native_thread(instance));
}

pub(crate) fn cancel_instance_now(instance: u64) {
    mark_instance_cancelled(instance);
    cancel_instance_on_native_thread(instance);
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn probe_download_pending(instance: u64) -> bool {
    close::physical_pending(instance)
}

fn mark_instance_cancelled(instance: u64) {
    let expectations = EXPECTATIONS
        .lock()
        .values()
        .filter(|expectation| expectation.instance == instance)
        .cloned()
        .collect::<Vec<_>>();
    for expectation in &expectations {
        expectation.state.lock().cancel_requested = true;
    }
}

fn cancel_instance_on_native_thread(instance: u64) {
    let ids = EXPECTATIONS
        .lock()
        .values()
        .filter(|expectation| expectation.instance == instance)
        .map(|expectation| expectation.id)
        .collect::<Vec<_>>();
    for id in ids {
        cancel_on_native_thread(id);
    }
}

pub(crate) fn download_live(
    view: &SideBrowserLiveView,
    request: &DownloadRequest,
    expected_generation: u64,
    prepared_url: &str,
    dispatch_script: &str,
    operation: ScriptOperation,
) -> Result<DownloadResult, DownloadError> {
    download_live_with(
        request,
        prepared_url,
        operation,
        view.instance,
        || view.document_identity(expected_generation).is_ok(),
        || ensure_handler(&view.webview, view.instance),
        |child| side_script::run(view, dispatch_script, expected_generation, child),
        |id| {
            let _ = view
                .webview
                .with_webview(move |_| cancel_on_native_thread(id));
        },
    )
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn download_live_host(
    view: &crate::computer_use::webview_host::HostOwnedWebView,
    request: &DownloadRequest,
    expected_generation: u64,
    prepared_url: &str,
    dispatch_script: &str,
    operation: ScriptOperation,
) -> Result<DownloadResult, DownloadError> {
    download_live_with(
        request,
        prepared_url,
        operation,
        view.download_instance(),
        || view.download_document_matches(expected_generation),
        || view.ensure_download_handler(),
        |child| view.host_script_owned(dispatch_script, expected_generation, child),
        |id| view.cancel_download(id),
    )
}

#[allow(clippy::too_many_arguments)]
fn download_live_with<Current, Ensure, Dispatch, Cancel>(
    request: &DownloadRequest,
    prepared_url: &str,
    operation: ScriptOperation,
    instance: u64,
    is_current: Current,
    ensure_handler: Ensure,
    dispatch: Dispatch,
    cancel: Cancel,
) -> Result<DownloadResult, DownloadError>
where
    Current: Fn() -> bool,
    Ensure: FnOnce() -> Result<(), String>,
    Dispatch: FnOnce(ScriptOperation) -> Result<String, String>,
    Cancel: Fn(u64),
{
    if !is_current() {
        operation.finished();
        return Err(DownloadError::not_dispatched(
            "WebView native document retired",
        ));
    }
    if let Err(error) = ensure_handler() {
        operation.finished();
        return Err(DownloadError::not_dispatched(error));
    }
    if let Err(error) = operation.check() {
        operation.finished();
        return Err(DownloadError::not_dispatched(error));
    }

    let control = operation.clone();
    let child = operation.child();
    let expectation = match register(
        instance,
        prepared_url,
        request.destination.clone(),
        operation,
    ) {
        Ok(expectation) => expectation,
        Err(error) => return Err(DownloadError::not_dispatched(error)),
    };

    #[cfg(feature = "computer-use-probe")]
    probe::record_stage(instance, "dispatch-started");
    let dispatched = dispatch(child);
    #[cfg(feature = "computer-use-probe")]
    probe::record_stage(instance, "dispatch-replied");
    let raw = match dispatched {
        Ok(raw) => raw,
        Err(_) => {
            request_cancel(&expectation, &cancel);
            abandon(&expectation);
            return Err(DownloadError::dispatched_unknown(
                "WebView download dispatch outcome unknown",
            ));
        }
    };
    let reply = match parse_script_json(&raw) {
        Ok(reply) => reply,
        Err(_) => {
            request_cancel(&expectation, &cancel);
            abandon(&expectation);
            return Err(DownloadError::dispatched_unknown(
                "WebView download dispatch outcome unknown",
            ));
        }
    };
    if dom::acknowledge(&reply).is_err() {
        retire_before_dispatch(&expectation);
        return Err(DownloadError::not_dispatched(
            "WebView download element changed before dispatch",
        ));
    }
    #[cfg(feature = "computer-use-probe")]
    probe::record(instance, probe::Event::DispatchAcknowledged);

    let started = Instant::now();
    let mut cancel_started: Option<Instant> = None;
    loop {
        let terminal = {
            let state = expectation.state.lock();
            // Both native and Wry completion handlers belong to this download.
            // A first terminal must not publish success or release occupancy
            // while the other original callback is still outstanding.
            // Exact browser exit may retire lost callbacks with unknown outcome.
            (state.wry_finished || state.native_exited)
                .then_some(state.terminal)
                .flatten()
        };
        if let Some(terminal) = terminal {
            let cancelled = cancel_started.is_some() || control.is_cancelled();
            return finish_caller(&expectation, terminal, cancelled);
        }

        let document_retired = !is_current();
        let cancelled = control.check().is_err() || document_retired;
        if cancel_started.is_none() && (cancelled || started.elapsed() >= DOWNLOAD_TIMEOUT) {
            cancel_started = Some(Instant::now());
            #[cfg(feature = "computer-use-probe")]
            probe::record_stage(instance, "native-cancel-wait-started");
            request_cancel(&expectation, &cancel);
        }

        if cancel_started.is_some_and(|when| when.elapsed() >= CANCEL_GRACE) {
            #[cfg(feature = "computer-use-probe")]
            {
                probe::record_stage(instance, "native-cancel-wait-expired");
                probe::record_cancel_expiry(
                    instance,
                    cancel_started.expect("cancel deadline expired").elapsed(),
                );
            }
            abandon(&expectation);
            return Err(DownloadError::dispatched_unknown(
                "WebView download did not reach a native terminal state",
            ));
        }
        let mut state = expectation.state.lock();
        // Do not deliberately sleep beyond the original cancellation deadline.
        // Native callbacks may still be outstanding when the caller returns
        // unknown; the retained expectation continues to own that occupancy.
        let wait = cancel_started.map_or(Duration::from_millis(50), |when| {
            Duration::from_millis(50).min(CANCEL_GRACE.saturating_sub(when.elapsed()))
        });
        expectation.changed.wait_for(&mut state, wait);
    }
}

fn matching_expectation(instance: u64, url: &str) -> Option<Arc<Expectation>> {
    EXPECTATIONS
        .lock()
        .values()
        .find(|expectation| expectation.instance == instance && expectation.url == url)
        .cloned()
}

fn has_pending_expectation(instance: u64) -> bool {
    EXPECTATIONS
        .lock()
        .values()
        .any(|expectation| expectation.instance == instance)
}

fn pending_expectation_matches_origin(instance: u64, permission_uri: &str) -> bool {
    let urls = EXPECTATIONS
        .lock()
        .values()
        .filter(|expectation| expectation.instance == instance)
        .map(|expectation| expectation.url.clone())
        .collect::<Vec<_>>();
    let Ok(permission) = url::Url::parse(permission_uri) else {
        return false;
    };
    urls.into_iter().any(|expected| {
        url::Url::parse(&expected).is_ok_and(|expected| {
            permission.scheme() == expected.scheme()
                && permission.host_str() == expected.host_str()
                && permission.port_or_known_default() == expected.port_or_known_default()
        })
    })
}

fn claim_native_start(instance: u64, url: &str) -> Option<Arc<Expectation>> {
    let expectation = matching_expectation(instance, url)?;
    let mut state = expectation.state.lock();
    if state.native_started || state.terminal.is_some() {
        return None;
    }
    state.native_started = true;
    drop(state);
    Some(expectation)
}

fn complete_by_id(id: u64, terminal: NativeTerminal) {
    let expectation = { EXPECTATIONS.lock().get(&id).cloned() };
    if let Some(expectation) = expectation {
        complete(&expectation, terminal);
    }
}

fn complete(expectation: &Arc<Expectation>, terminal: NativeTerminal) {
    {
        let mut state = expectation.state.lock();
        if state.terminal.is_none() {
            state.terminal = Some(terminal);
        }
    }
    // Native WebView2 and Wry can report in either order. A first terminal
    // or an abandoned response receiver must not delete the original files.
    expectation.changed.notify_all();
    remove_if_finished(expectation);
}

fn finish_caller(
    expectation: &Arc<Expectation>,
    terminal: NativeTerminal,
    cancelled: bool,
) -> Result<DownloadResult, DownloadError> {
    let result = match terminal {
        NativeTerminal::Completed if !cancelled => {
            let copied = expectation
                .native_destination
                .copy_into(&expectation.destination, || {
                    expectation
                        .operation
                        .lock()
                        .as_ref()
                        .ok_or_else(|| "WebView download operation retired".to_string())?
                        .check()
                });
            match copied {
                Ok(()) => Ok(DownloadResult {
                    path: expectation.destination.clone(),
                }),
                Err(error) => {
                    abandon(expectation);
                    Err(DownloadError::dispatched_rejected(error))
                }
            }
        }
        NativeTerminal::Completed | NativeTerminal::Interrupted => {
            abandon(expectation);
            Err(DownloadError::dispatched_rejected(
                "WebView download was cancelled or interrupted",
            ))
        }
        NativeTerminal::InvalidDestination => {
            abandon(expectation);
            Err(DownloadError::dispatched_rejected(
                "WebView download did not use the Broker staging destination",
            ))
        }
        NativeTerminal::OwnerExited => {
            abandon(expectation);
            Err(DownloadError::dispatched_unknown(
                "WebView native owner exited before download completion; outcome unknown",
            ))
        }
    };
    expectation.state.lock().caller_done = true;
    remove_if_finished(expectation);
    result
}

fn retire_before_dispatch(expectation: &Arc<Expectation>) {
    expectation.state.lock().caller_done = true;
    if let Some(operation) = expectation.operation.lock().take() {
        operation.finished();
    }
    EXPECTATIONS.lock().remove(&expectation.id);
    cleanup_expectation(expectation);
}

fn abandon(expectation: &Arc<Expectation>) {
    {
        let mut state = expectation.state.lock();
        state.caller_done = true;
        state.abandoned = true;
    }
    remove_if_finished(expectation);
}

fn remove_if_finished(expectation: &Arc<Expectation>) {
    let remove = {
        let state = expectation.state.lock();
        state.caller_done && (state.wry_finished || state.native_exited) && state.terminal.is_some()
    };
    if remove {
        let retired = {
            let mut expectations = EXPECTATIONS.lock();
            if expectations
                .get(&expectation.id)
                .is_some_and(|current| Arc::ptr_eq(current, expectation))
            {
                expectations.remove(&expectation.id).is_some()
            } else {
                false
            }
        };
        if !retired {
            return; // Duplicate old callbacks cannot clean a later occupant.
        }
        if expectation.state.lock().abandoned {
            cleanup_expectation(expectation);
        }
        if let Some(operation) = expectation.operation.lock().take() {
            operation.finished();
        }
    }
}

fn request_cancel(expectation: &Arc<Expectation>, cancel: &impl Fn(u64)) {
    expectation.state.lock().cancel_requested = true;
    cancel(expectation.id);
}

fn cancel_on_native_thread(id: u64) {
    // Cancel may synchronously raise StateChanged, whose handler mutably
    // borrows this registry. Release our RefCell borrow before calling COM.
    let operation = NATIVE_OPERATIONS.with(|operations| {
        operations
            .borrow()
            .get(&id)
            .map(|native| native.operation.clone())
    });
    if let Some(operation) = operation {
        #[cfg(feature = "computer-use-probe")]
        probe::native_state(&operation);
        let _ = unsafe { operation.Cancel() };
    }
}

fn result_path_matches(id: u64, path: &Path) -> bool {
    EXPECTATIONS
        .lock()
        .get(&id)
        .is_some_and(|expectation| same_path(path, expectation.native_destination.path()))
}

fn cleanup_expectation(expectation: &Expectation) {
    cleanup_exact_download(&expectation.destination);
    expectation.native_destination.cleanup();
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn windows_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
    }
}

fn read_download_uri(
    operation: &ICoreWebView2DownloadOperation,
) -> ::windows::core::Result<String> {
    unsafe {
        let mut raw = PWSTR::null();
        operation.Uri(&mut raw)?;
        Ok(take_pwstr(raw))
    }
}

fn read_result_path(
    operation: &ICoreWebView2DownloadOperation,
) -> ::windows::core::Result<PathBuf> {
    unsafe {
        let mut raw = PWSTR::null();
        operation.ResultFilePath(&mut raw)?;
        Ok(PathBuf::from(take_pwstr(raw)))
    }
}

fn take_pwstr(raw: PWSTR) -> String {
    if raw.0.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0usize;
        while *raw.0.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(raw.0, len));
        CoTaskMemFree(Some(raw.0 as *const _));
        value
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_buffer_retirement;
