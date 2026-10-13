//! Host-owned WebView2 used as the S9.1 live bind target when cu_probe has no AppHandle.

#![cfg(all(target_os = "windows", feature = "computer-use-probe"))]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    ExecuteScriptCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        CreateCoreWebView2Environment, CreateCoreWebView2EnvironmentWithOptions,
        ICoreWebView2Controller,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
};
use windows::core::{Interface, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, PeekMessageW, RegisterClassW,
    ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, MSG, PM_REMOVE, SW_SHOW,
    WM_DESTROY, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use super::adapter::{
    DownloadError, DownloadRequest, DownloadResult, NavigationError, NavigationKind,
    NavigationRequest,
};
use super::webview::{LiveNavigationDocument, LiveWebView, QueuedScript, ScriptOperation};

type PendingNavigation = Arc<Mutex<Option<Sender<Result<u64, NavigationError>>>>>;
static NEXT_HOST_DOWNLOAD_INSTANCE: AtomicU64 = AtomicU64::new(1 << 63);

#[derive(Clone)]
struct HostDocumentState {
    download_instance: u64,
    download_requested: Arc<AtomicU64>,
    download_claimed: Arc<AtomicU64>,
    download_finished: Arc<AtomicU64>,
    browser_pid: Arc<AtomicU64>,
    browser_exited: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    authority: Arc<Mutex<()>>,
    navigation_pending: Arc<AtomicBool>,
    navigation_starts: Arc<AtomicU64>,
    navigation_completions: Arc<AtomicU64>,
    navigation_unknown_once: Arc<AtomicBool>,
}

impl HostDocumentState {
    fn new() -> Self {
        Self {
            download_instance: NEXT_HOST_DOWNLOAD_INSTANCE.fetch_add(1, Ordering::SeqCst),
            download_requested: Arc::new(AtomicU64::new(0)),
            download_claimed: Arc::new(AtomicU64::new(0)),
            download_finished: Arc::new(AtomicU64::new(0)),
            browser_pid: Arc::new(AtomicU64::new(0)),
            browser_exited: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            alive: Arc::new(AtomicBool::new(true)),
            authority: Arc::new(Mutex::new(())),
            navigation_pending: Arc::new(AtomicBool::new(false)),
            navigation_starts: Arc::new(AtomicU64::new(0)),
            navigation_completions: Arc::new(AtomicU64::new(0)),
            navigation_unknown_once: Arc::new(AtomicBool::new(false)),
        }
    }
}
mod lifetime;
mod profile;
pub(crate) use profile::SharedBrowser;
mod readiness;
mod shutdown;
mod startup;

enum Cmd {
    Url(Sender<Result<String, String>>),
    Title(Sender<Result<String, String>>),
    CrashDisposableRenderer(Sender<Result<(), String>>),
    Ready(Sender<Result<(), String>>),
    Navigate {
        kind: NavigationKind,
        url: Option<String>,
        expected_generation: u64,
        cancellation: grok_computer_use_core::execution::ActionCancellation,
        tx: Sender<Result<u64, NavigationError>>,
    },
    InstallDownloadHandler(Sender<Result<(), String>>),
    CancelDownload {
        id: u64,
        tx: Sender<()>,
    },
    Script {
        js: String,
        tx: Sender<Result<String, String>>,
        operation: Option<QueuedScript>,
        generation: u64,
    },
    SetupFault {
        tx: Sender<Result<String, String>>,
        operation: QueuedScript,
        generation: u64,
        fault: super::webview::isolated_windows::probe_fault::Fault,
    },
    Shutdown,
}

pub struct HostOwnedWebView {
    label: String,
    tab_id: String,
    cmds: Sender<Cmd>,
    document: HostDocumentState,
    join: shutdown::NativeThread,
}

impl HostOwnedWebView {
    pub(crate) fn setup_fault(
        &self,
        operation: ScriptOperation,
        fault: super::webview::isolated_windows::probe_fault::Fault,
    ) -> Result<Receiver<Result<String, String>>, String> {
        let (tx, rx) = mpsc::channel();
        if self
            .cmds
            .send(Cmd::SetupFault {
                operation: QueuedScript::new(operation, tx.clone()),
                tx,
                generation: self.navigation_generation(),
                fault,
            })
            .is_err()
        {
            return Err("owned setup fault host unavailable".into());
        }
        Ok(rx)
    }

    pub(crate) fn crash_disposable_renderer(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.cmds
            .send(Cmd::CrashDisposableRenderer(tx))
            .map_err(|_| "owned renderer host unavailable")?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| "owned renderer crash dispatch timed out")?
    }

    pub(crate) fn fixture_ready(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.cmds
            .send(Cmd::Ready(tx))
            .map_err(|_| "owned readiness host unavailable")?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "owned readiness timed out")?
    }

    pub(crate) fn navigation_counts(&self) -> (u64, u64) {
        (
            self.document.navigation_starts.load(Ordering::SeqCst),
            self.document.navigation_completions.load(Ordering::SeqCst),
        )
    }

    pub(crate) fn fixture_generation(&self) -> u64 {
        self.navigation_generation()
    }

    pub(crate) fn download_counts(&self) -> (u64, u64, u64) {
        (
            self.document.download_requested.load(Ordering::SeqCst),
            self.document.download_claimed.load(Ordering::SeqCst),
            self.document.download_finished.load(Ordering::SeqCst),
        )
    }

    pub(crate) fn download_instance(&self) -> u64 {
        self.document.download_instance
    }

    pub(crate) fn browser_pid(&self) -> u64 {
        self.document.browser_pid.load(Ordering::SeqCst)
    }

    pub(crate) fn browser_exited(&self) -> bool {
        self.document.browser_exited.load(Ordering::SeqCst)
    }

    pub(crate) fn download_document_matches(&self, generation: u64) -> bool {
        self.is_alive() && self.navigation_generation() == generation
    }

    pub(crate) fn ensure_download_handler(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.cmds
            .send(Cmd::InstallDownloadHandler(tx))
            .map_err(|_| "owned download host unavailable".to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "owned download handler setup timed out".to_string())?
    }

    pub(crate) fn cancel_download(&self, id: u64) {
        let (tx, rx) = mpsc::channel();
        if self.cmds.send(Cmd::CancelDownload { id, tx }).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(3));
        }
    }

    pub(crate) fn make_next_navigation_unknown(&self) {
        self.document
            .navigation_unknown_once
            .store(true, Ordering::SeqCst);
    }

    fn dispatch_navigation(
        &self,
        kind: NavigationKind,
        url: Option<String>,
        expected_generation: u64,
        cancellation: grok_computer_use_core::execution::ActionCancellation,
    ) -> Result<u64, NavigationError> {
        if !self.is_alive() {
            return Err(NavigationError::not_dispatched("webview closed"));
        }
        cancellation
            .check()
            .map_err(NavigationError::not_dispatched)?;
        let (tx, rx) = mpsc::channel();
        self.cmds
            .send(Cmd::Navigate {
                kind,
                url,
                expected_generation,
                cancellation,
                tx,
            })
            .map_err(|_| NavigationError::not_dispatched("webview host gone"))?;
        rx.recv_timeout(Duration::from_secs(15)).map_err(|_| {
            NavigationError::dispatched_unknown("WebView navigation completion timed out")
        })?
    }

    pub fn shutdown(&self) -> Result<(), String> {
        {
            let _authority = self.document.authority.lock();
            self.document.alive.store(false, Ordering::SeqCst);
        }
        let _ = self.cmds.send(Cmd::Shutdown);
        self.join.join()
    }

    pub(crate) fn finish_probe<T>(&self, primary: Result<T, String>) -> Result<T, String> {
        shutdown::combine(primary, self.shutdown())
    }
}

impl Drop for HostOwnedWebView {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            tracing::warn!(%error, "owned WebView cleanup was not confirmed");
        }
    }
}

impl LiveWebView for HostOwnedWebView {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn tab_id(&self) -> String {
        self.tab_id.clone()
    }
    fn url(&self) -> Result<String, String> {
        if !self.is_alive() {
            return Err("webview closed".into());
        }
        let (tx, rx) = mpsc::channel();
        self.cmds
            .send(Cmd::Url(tx))
            .map_err(|_| "webview host gone".to_string())?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| "url timeout".to_string())?
    }
    fn title(&self) -> Result<String, String> {
        if !self.is_alive() {
            return Err("webview closed".into());
        }
        let (tx, rx) = mpsc::channel();
        self.cmds
            .send(Cmd::Title(tx))
            .map_err(|_| "webview host gone".to_string())?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| "title timeout".to_string())?
    }
    fn is_alive(&self) -> bool {
        self.document.alive.load(Ordering::SeqCst)
    }
    fn navigation_generation(&self) -> u64 {
        self.document.generation.load(Ordering::SeqCst)
    }
    fn navigate(&self, url: &str) -> Result<u64, String> {
        self.dispatch_navigation(
            NavigationKind::Url,
            Some(url.to_string()),
            self.navigation_generation(),
            Default::default(),
        )
        .map_err(|error| error.detail)
    }
    fn with_document_authority(
        &self,
        generation: u64,
        commit: &mut dyn FnMut() -> Result<(), super::BrokerError>,
    ) -> Result<(), super::BrokerError> {
        let _authority = self.document.authority.lock();
        if !self.document.alive.load(Ordering::SeqCst)
            || self.document.generation.load(Ordering::SeqCst) != generation
        {
            return Err(super::BrokerError::DeadTarget);
        }
        commit()
    }
    fn host_script(&self, js: &str) -> Result<String, String> {
        if !self.is_alive() {
            return Err("webview closed".into());
        }
        let (tx, rx) = mpsc::channel();
        self.cmds
            .send(Cmd::Script {
                js: js.into(),
                tx,
                operation: None,
                generation: self.navigation_generation(),
            })
            .map_err(|_| "webview host gone".to_string())?;
        rx.recv_timeout(Duration::from_secs(8))
            .map_err(|_| "script timeout".to_string())?
    }

    fn host_script_owned(
        &self,
        js: &str,
        generation: u64,
        operation: ScriptOperation,
    ) -> Result<String, String> {
        if !self.is_alive()
            || generation != self.navigation_generation()
            || operation.check().is_err()
        {
            operation.finished();
            return Err("WebView native dispatch unavailable".into());
        }
        let (tx, rx) = mpsc::channel();
        if self
            .cmds
            .send(Cmd::Script {
                js: js.into(),
                operation: Some(QueuedScript::new(operation, tx.clone())),
                tx,
                generation,
            })
            .is_err()
        {
            return Err("WebView native dispatch unavailable".into());
        }
        rx.recv_timeout(Duration::from_secs(8))
            .map_err(|_| "WebView typed script outcome unknown".to_string())?
    }

    fn navigate_for_run(
        &self,
        request: &NavigationRequest,
        expected_generation: u64,
    ) -> Result<LiveNavigationDocument, NavigationError> {
        if self.navigation_generation() != expected_generation {
            return Err(NavigationError::not_dispatched(
                "WebView document changed before navigation",
            ));
        }
        match request.kind {
            NavigationKind::Url => {
                let url = request.url.as_deref().ok_or_else(|| {
                    NavigationError::not_dispatched("url navigation requires url")
                })?;
                super::browser::is_allowed_navigate_url(url)
                    .map_err(|error| NavigationError::not_dispatched(error.to_string()))?;
            }
            _ if request.url.is_some() => {
                return Err(NavigationError::not_dispatched(
                    "url is only valid for kind=url",
                ));
            }
            _ => {}
        }
        let generation = self.dispatch_navigation(
            request.kind,
            request.url.clone(),
            expected_generation,
            request.cancellation.clone(),
        )?;
        request
            .cancellation
            .check()
            .map_err(NavigationError::dispatched_unknown)?;
        if self.navigation_generation() != generation {
            return Err(NavigationError::dispatched_unknown(
                "WebView successor document changed before readback",
            ));
        }
        let url = self.url().map_err(NavigationError::dispatched_unknown)?;
        let title = self.title().unwrap_or_else(|_| url.clone());
        if self.navigation_generation() != generation {
            return Err(NavigationError::dispatched_unknown(
                "WebView successor document changed during readback",
            ));
        }
        Ok(LiveNavigationDocument {
            generation,
            url,
            title,
        })
    }

    fn download_for_run(
        &self,
        request: &DownloadRequest,
        expected_generation: u64,
        prepared_url: &str,
        dispatch_script: &str,
        operation: ScriptOperation,
    ) -> Result<DownloadResult, DownloadError> {
        super::webview::download_live_probe(
            self,
            request,
            expected_generation,
            prepared_url,
            dispatch_script,
            operation,
        )
    }
}

fn fence_generation(gen: &AtomicU64, alive: &AtomicBool) -> Result<u64, String> {
    gen.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
        .map(|previous| previous + 1)
        .map_err(|_| {
            alive.store(false, Ordering::SeqCst);
            "webview document identity exhausted".into()
        })
}

fn sta_thread(
    url: String,
    cmds: Receiver<Cmd>,
    ready: Sender<Result<(), String>>,
    document: HostDocumentState,
    profile: Option<profile::Profile>,
    startup: startup::Startup,
) -> Result<(), String> {
    let result = sta_thread_inner(url, cmds, &ready, document.clone(), profile, &startup);
    {
        let _authority = document.authority.lock();
        document.alive.store(false, Ordering::SeqCst);
    }
    if let Err(error) = &result {
        let _ = ready.send(Err(error.clone()));
    }
    result
}

fn sta_thread_inner(
    url: String,
    cmds: Receiver<Cmd>,
    ready: &Sender<Result<(), String>>,
    document: HostDocumentState,
    profile: Option<profile::Profile>,
    startup: &startup::Startup,
) -> Result<(), String> {
    let gen = document.generation.clone();
    let alive = document.alive.clone();
    let document_authority = document.authority.clone();
    let download_instance = document.download_instance;
    startup.stage("COM apartment")?;
    let _apartment = lifetime::Apartment::enter()?;
    let title = format!("GrokCuWebViewBind-{}", std::process::id());
    let window = lifetime::Window(create_host_window(&title)?);
    let hwnd = window.0;
    let disposable = profile.is_some();
    let wait_for_browser_exit = profile
        .as_ref()
        .is_some_and(|profile| profile.wait_for_exit);
    startup.stage("environment")?;
    let environment = {
        let (tx, rx) = mpsc::channel();
        let profile = profile.map(|path| {
            use std::os::windows::ffi::OsStrExt;
            path.path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>()
        });
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |error_code, environment| {
                let result = error_code.and_then(|()| {
                    environment.ok_or_else(|| {
                        windows::core::Error::from(windows::Win32::Foundation::E_POINTER)
                    })
                });
                let _ = tx.send(result);
                Ok(())
            },
        ));
        unsafe {
            if let Some(profile) = &profile {
                CreateCoreWebView2EnvironmentWithOptions(
                    PCWSTR::null(),
                    PCWSTR(profile.as_ptr()),
                    None,
                    &handler,
                )
            } else {
                CreateCoreWebView2Environment(&handler)
            }
        }
        .map_err(|e| format!("webview2 env: {e:?}"))?;
        startup.receive(rx)?.map_err(|e| e.to_string())?
    };
    startup.stage("controller")?;
    let controller: ICoreWebView2Controller = {
        let (tx, rx) = mpsc::channel();
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
            move |error_code, controller| {
                let result = error_code.and_then(|()| {
                    controller.ok_or_else(|| {
                        windows::core::Error::from(windows::Win32::Foundation::E_POINTER)
                    })
                });
                let _ = tx.send(result);
                Ok(())
            },
        ));
        unsafe { environment.CreateCoreWebView2Controller(hwnd, &handler) }
            .map_err(|e| format!("webview2 controller: {e:?}"))?;
        startup.receive(rx)?.map_err(|e| e.to_string())?
    };
    startup.stage("navigation")?;
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        controller.SetBounds(rc).map_err(|e| e.to_string())?;
        controller.SetIsVisible(true).map_err(|e| e.to_string())?;
    }
    let webview = unsafe { controller.CoreWebView2().map_err(|e| e.to_string())? };
    let mut browser_pid = 0;
    unsafe { webview.BrowserProcessId(&mut browser_pid) }
        .map_err(|_| "owned browser identity unavailable")?;
    document
        .browser_pid
        .store(u64::from(browser_pid), Ordering::SeqCst);
    let browser_exit = if disposable {
        Some(lifetime::BrowserExit::observe(
            &environment,
            &webview,
            document.browser_exited.clone(),
        )?)
    } else {
        None // An ordinary/shared environment need not exit with this view.
    };

    let nav_done = Arc::new(AtomicBool::new(false));
    let initial_navigation_seen = Arc::new(AtomicBool::new(false));
    let initial_navigation_success = Arc::new(AtomicBool::new(false));
    let initial_navigation_error = Arc::new(Mutex::new(None));
    let pending: PendingNavigation = Arc::new(Mutex::new(None));
    let nav_flag = nav_done.clone();
    let initial_seen = initial_navigation_seen.clone();
    let initial_success = initial_navigation_success.clone();
    let initial_error = initial_navigation_error.clone();
    let pending_t = pending.clone();
    let gen_h = gen.clone();
    let gen_start = gen.clone();
    let alive_start = alive.clone();
    let authority_start = document_authority.clone();
    let navigation_pending_start = document.navigation_pending.clone();
    let navigation_starts = document.navigation_starts.clone();
    let navigation_pending_complete = document.navigation_pending.clone();
    let navigation_completions = document.navigation_completions.clone();
    let navigation_unknown_once = document.navigation_unknown_once.clone();
    let navigation_download_instance = download_instance;
    let mut nav_start_token = 0i64;
    let mut nav_token = 0i64;
    unsafe {
        webview
            .add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_wv, _args| {
                    // Includes link/user navigation, not only Cmd::Navigate.
                    super::webview::cancel_side_browser_downloads_now(navigation_download_instance);
                    let _authority = authority_start.lock();
                    navigation_starts.fetch_add(1, Ordering::SeqCst);
                    if !navigation_pending_start.swap(true, Ordering::SeqCst) {
                        let _ = fence_generation(&gen_start, &alive_start);
                    }
                    Ok(())
                })),
                &mut nav_start_token,
            )
            .map_err(|e| e.to_string())?;
        webview
            .add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_wv, args| {
                    let mut success = BOOL(0);
                    let completed = args
                        .as_ref()
                        .is_some_and(|args| args.IsSuccess(&mut success).is_ok())
                        && success.as_bool();
                    if !initial_seen.swap(true, Ordering::SeqCst) {
                        initial_success.store(completed, Ordering::SeqCst);
                        let mut status = Default::default();
                        if let Some(args) = args.as_ref() {
                            if args.WebErrorStatus(&mut status).is_ok() {
                                *initial_error.lock() = Some(status.0);
                            }
                        }
                    }
                    nav_flag.store(true, Ordering::SeqCst);
                    navigation_completions.fetch_add(1, Ordering::SeqCst);
                    navigation_pending_complete.store(false, Ordering::SeqCst);
                    if let Some(tx) = pending_t.lock().take() {
                        let g = gen_h.load(Ordering::SeqCst);
                        let result = if navigation_unknown_once.swap(false, Ordering::SeqCst) {
                            Err(NavigationError::dispatched_unknown(
                                "fixture navigation completion outcome unknown",
                            ))
                        } else if completed {
                            Ok(g)
                        } else {
                            Err(NavigationError::dispatched_unknown(
                                "WebView navigation completed unsuccessfully",
                            ))
                        };
                        let _ = tx.send(result);
                    }
                    Ok(())
                })),
                &mut nav_token,
            )
            .map_err(|e| e.to_string())?;
    }
    navigate_now(&webview, &url)?;
    startup.wait_flag(&nav_done, Duration::from_secs(15))?;
    if !initial_navigation_success.load(Ordering::SeqCst) {
        return Err(format!(
            "initial WebView navigation completed unsuccessfully; web_error_status={:?}",
            *initial_navigation_error.lock(),
        ));
    }
    if gen.load(Ordering::SeqCst) == 0 {
        return Err("initial WebView navigation produced no native identity".into());
    }
    let mut bounds = RECT::default();
    unsafe { controller.Bounds(&mut bounds).map_err(|e| e.to_string())? };
    startup.stage("viewport readiness")?;
    readiness::wait(
        &webview,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        Some(startup),
    )?;
    startup.check()?;
    let world = Arc::new(super::webview::isolated_windows::NativeWorld::default());
    let failed_gen = gen.clone();
    let failed_alive = alive.clone();
    let failed_authority = document_authority.clone();
    let failed_download_instance = download_instance;
    super::webview::isolated_windows::process::install(&webview, &world, move |failure| {
        super::webview::cancel_side_browser_downloads_now(failed_download_instance);
        let _authority = failed_authority.lock();
        match failure {
            super::webview::isolated_windows::process::Failure::MainRendererExited => {
                let _ = fence_generation(&failed_gen, &failed_alive);
            }
            super::webview::isolated_windows::process::Failure::BrowserExited
            | super::webview::isolated_windows::process::Failure::NativeViewClosed => {
                failed_alive.store(false, Ordering::SeqCst);
            }
        }
    })?;
    if ready.send(Ok(())).is_err() {
        let _authority = document_authority.lock();
        alive.store(false, Ordering::SeqCst);
    }
    let mut download_handler: Option<Result<(), String>> = None;
    let mut msg = MSG::default();
    while alive.load(Ordering::SeqCst) {
        while let Ok(cmd) = cmds.try_recv() {
            match cmd {
                Cmd::Url(tx) => {
                    let _ = tx.send(read_source(&webview));
                }
                Cmd::Title(tx) => {
                    let _ = tx.send(read_title(&webview));
                }
                Cmd::CrashDisposableRenderer(tx) => {
                    if !disposable {
                        let _ = tx.send(Err(
                            "renderer crash requires a new disposable profile".into()
                        ));
                        continue;
                    }
                    let method: Vec<u16> = "Page.crash"
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                    let parameters: Vec<u16> =
                        "{}".encode_utf16().chain(std::iter::once(0)).collect();
                    let handler = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(
                        Box::new(|_, _| Ok(())),
                    );
                    let dispatched = unsafe {
                        webview.CallDevToolsProtocolMethod(
                            PCWSTR(method.as_ptr()),
                            PCWSTR(parameters.as_ptr()),
                            &handler,
                        )
                    }
                    .map_err(|_| "disposable renderer crash dispatch failed".into());
                    let _ = tx.send(dispatched);
                }
                Cmd::Ready(tx) => {
                    let mut bounds = RECT::default();
                    let result = unsafe { controller.Bounds(&mut bounds) }
                        .map_err(|_| "owned fixture bounds unavailable".to_string())
                        .and_then(|()| {
                            readiness::wait(
                                &webview,
                                bounds.right - bounds.left,
                                bounds.bottom - bounds.top,
                                None,
                            )
                        });
                    let _ = tx.send(result);
                }
                Cmd::Navigate {
                    kind,
                    url,
                    expected_generation,
                    cancellation,
                    tx,
                } => {
                    if pending.lock().is_some() {
                        let _ = tx.send(Err(NavigationError::not_dispatched(
                            "WebView navigation is already pending",
                        )));
                        continue;
                    }
                    if let Err(error) = cancellation.check() {
                        let _ = tx.send(Err(NavigationError::not_dispatched(error)));
                        continue;
                    }
                    if matches!(kind, NavigationKind::Back | NavigationKind::Forward) {
                        match history_available(&webview, kind) {
                            Ok(true) => {}
                            Ok(false) => {
                                let _ = tx.send(Err(NavigationError::not_dispatched(
                                    "WebView has no history entry",
                                )));
                                continue;
                            }
                            Err(error) => {
                                let _ = tx.send(Err(NavigationError::not_dispatched(error)));
                                continue;
                            }
                        }
                    }
                    let admitted = {
                        let _authority = document_authority.lock();
                        if !alive.load(Ordering::SeqCst)
                            || gen.load(Ordering::SeqCst) != expected_generation
                        {
                            Err(NavigationError::not_dispatched(
                                "WebView document changed before navigation",
                            ))
                        } else if document.navigation_pending.swap(true, Ordering::SeqCst) {
                            Err(NavigationError::not_dispatched(
                                "WebView navigation is already pending",
                            ))
                        } else {
                            match fence_generation(&gen, &alive) {
                                Ok(generation) => Ok(generation),
                                Err(error) => {
                                    document.navigation_pending.store(false, Ordering::SeqCst);
                                    Err(NavigationError::not_dispatched(error))
                                }
                            }
                        }
                    };
                    if let Err(error) = admitted {
                        let _ = tx.send(Err(error));
                        continue;
                    }
                    *pending.lock() = Some(tx);
                    if let Err(error) = dispatch_navigation_now(&webview, kind, url.as_deref()) {
                        document.navigation_pending.store(false, Ordering::SeqCst);
                        if let Some(tx) = pending.lock().take() {
                            let _ = tx.send(Err(NavigationError::dispatched_unknown(format!(
                                "WebView navigation dispatch failed: {error}"
                            ))));
                        }
                    }
                }
                Cmd::InstallDownloadHandler(tx) => {
                    let result = download_handler
                        .get_or_insert_with(|| {
                            super::webview::install_probe_download_handlers(
                                &webview,
                                download_instance,
                                document.download_requested.clone(),
                                document.download_claimed.clone(),
                                document.download_finished.clone(),
                            )
                        })
                        .clone();
                    let _ = tx.send(result);
                }
                Cmd::CancelDownload { id, tx } => {
                    super::webview::cancel_probe_download_now(id);
                    let _ = tx.send(());
                }
                Cmd::Script {
                    js,
                    tx,
                    operation,
                    generation,
                } => {
                    if generation != gen.load(Ordering::SeqCst)
                        || operation
                            .as_ref()
                            .is_some_and(|operation| operation.check().is_err())
                        || !alive.load(Ordering::SeqCst)
                    {
                        if let Some(operation) = operation {
                            operation.reject("WebView native dispatch retired before execution");
                        } else {
                            let _ = tx.send(Err(
                                "WebView native dispatch retired before execution".into(),
                            ));
                        }
                        continue;
                    }
                    if let Some(operation) = operation {
                        let current_gen = gen.clone();
                        let current_alive = alive.clone();
                        super::webview::isolated_windows::dispatch(
                            webview.clone(),
                            world.clone(),
                            generation,
                            operation.handoff(),
                            move || {
                                current_alive.load(Ordering::SeqCst)
                                    && current_gen.load(Ordering::SeqCst) == generation
                            },
                            js,
                            tx,
                        );
                    } else {
                        // Independent fixture inspection deliberately uses the page world.
                        let _ = tx.send(run_script(&webview, &js, None));
                    }
                }
                Cmd::SetupFault {
                    tx,
                    operation,
                    generation,
                    fault,
                } => {
                    let current_gen = gen.clone();
                    let current_alive = alive.clone();
                    super::webview::isolated_windows::probe_fault::dispatch(
                        webview.clone(),
                        world.clone(),
                        generation,
                        operation.handoff(),
                        move || {
                            current_alive.load(Ordering::SeqCst)
                                && current_gen.load(Ordering::SeqCst) == generation
                        },
                        tx,
                        fault,
                    );
                }
                Cmd::Shutdown => {
                    let _authority = document_authority.lock();
                    alive.store(false, Ordering::SeqCst);
                }
            }
        }
        unsafe {
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_DESTROY {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                thread::sleep(Duration::from_millis(8));
            }
        }
    }
    // shutdown() fences alive before queuing Shutdown. The loop can therefore
    // exit without consuming that command; cancellation must be unconditional
    // on the owning STA, before the exact controller is closed.
    super::webview::retire_probe_downloads_now(download_instance);
    if !wait_for_browser_exit {
        // A shared browser keeps the original download alive independently of
        // this controller. Observe cancellation before destroying its callbacks.
        native::wait_download_terminal(download_instance)?;
        super::webview::uninstall_probe_downloads_now(download_instance);
        if !super::webview::probe_download_resources_retired(download_instance) {
            return Err(
                "shared view native download resources were retained after physical terminal"
                    .into(),
            );
        }
    }
    // Dropping unconsumed commands proves they never entered native dispatch.
    // Do this before potentially slow native teardown, not at the end of it.
    drop(cmds);
    super::webview::isolated_windows::process::closing(&world);
    unsafe {
        controller
            .Close()
            .map_err(|_| "owned native controller close failed")?;
    }
    drop(webview);
    drop(controller);
    if wait_for_browser_exit {
        let browser_exit = browser_exit
            .as_ref()
            .ok_or("owned browser exit observer missing")?;
        let started = Instant::now();
        let result = wait_flag(
            &browser_exit.observed,
            Duration::from_secs(20),
            "owned browser exit",
        );
        eprintln!(
            "owned-browser-close: exit_event={} pump_ms={} result={result:?}",
            browser_exit.observed.load(Ordering::SeqCst),
            started.elapsed().as_millis()
        );
        result?;
    }
    drop(window);
    Ok(())
}

mod native;
use native::{
    create_host_window, dispatch_navigation_now, history_available, navigate_now, read_source,
    read_title, run_script, run_script_current, wait_flag,
};
