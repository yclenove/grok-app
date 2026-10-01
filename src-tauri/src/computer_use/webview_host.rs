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
use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, PeekMessageW, RegisterClassW,
    ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, MSG, PM_REMOVE, SW_SHOW,
    WM_DESTROY, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

type PendingNavigation = Arc<Mutex<Option<Sender<Result<u64, String>>>>>;

use super::webview::{LiveWebView, QueuedScript, ScriptOperation};
mod lifetime;
mod readiness;
mod shutdown;
mod startup;

enum Cmd {
    Url(Sender<Result<String, String>>),
    Title(Sender<Result<String, String>>),
    CrashDisposableRenderer(Sender<Result<(), String>>),
    Ready(Sender<Result<(), String>>),
    Navigate {
        url: String,
        tx: Sender<Result<u64, String>>,
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
    gen: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
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

    pub fn spawn(label: &str, tab_id: &str, url: &str) -> Result<Arc<Self>, String> {
        Self::spawn_with_profile(label, tab_id, url, None)
    }

    pub(crate) fn spawn_disposable(
        label: &str,
        tab_id: &str,
        url: &str,
        profile: &std::path::Path,
    ) -> Result<Arc<Self>, String> {
        // Refuse existing profiles. Crash probes never attach to an ordinary
        // App/browser profile or a previous probe's still-live environment.
        std::fs::create_dir(profile).map_err(|_| "disposable WebView profile must be new")?;
        Self::spawn_with_profile(label, tab_id, url, Some(profile.to_owned()))
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

    fn spawn_with_profile(
        label: &str,
        tab_id: &str,
        url: &str,
        profile: Option<std::path::PathBuf>,
    ) -> Result<Arc<Self>, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let gen = Arc::new(AtomicU64::new(0));
        let alive = Arc::new(AtomicBool::new(true));
        let gen_t = gen.clone();
        let alive_t = alive.clone();
        let startup = startup::Startup::new(alive.clone());
        let startup_t = startup.clone();
        let url = url.to_string();
        let join = thread::spawn(move || {
            sta_thread(url, cmd_rx, ready_tx, gen_t, alive_t, profile, startup_t)
        });
        match ready_rx.recv_timeout(Duration::from_secs(25)) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                startup.stop();
                return Err(error);
            }
            Err(_) => return Err(startup.stop()),
        }
        Ok(Arc::new(Self {
            label: label.into(),
            tab_id: tab_id.into(),
            cmds: cmd_tx,
            gen,
            alive,
            join: shutdown::NativeThread::new(join),
        }))
    }

    pub fn shutdown(&self) -> Result<(), String> {
        self.alive.store(false, Ordering::SeqCst);
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
        self.alive.load(Ordering::SeqCst)
    }
    fn navigation_generation(&self) -> u64 {
        self.gen.load(Ordering::SeqCst)
    }
    fn navigate(&self, url: &str) -> Result<u64, String> {
        if !self.is_alive() {
            return Err("webview closed".into());
        }
        fence_generation(&self.gen, &self.alive)?;
        let (tx, rx) = mpsc::channel();
        self.cmds
            .send(Cmd::Navigate {
                url: url.into(),
                tx,
            })
            .map_err(|_| "webview host gone".to_string())?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "navigate timeout".to_string())?
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
}

fn fence_generation(gen: &AtomicU64, alive: &AtomicBool) -> Result<u64, String> {
    gen.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
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
    gen: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    profile: Option<std::path::PathBuf>,
    startup: startup::Startup,
) -> Result<(), String> {
    let result = sta_thread_inner(url, cmds, &ready, gen, alive.clone(), profile, &startup);
    alive.store(false, Ordering::SeqCst);
    if let Err(error) = &result {
        let _ = ready.send(Err(error.clone()));
    }
    result
}

fn sta_thread_inner(
    url: String,
    cmds: Receiver<Cmd>,
    ready: &Sender<Result<(), String>>,
    gen: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    profile: Option<std::path::PathBuf>,
    startup: &startup::Startup,
) -> Result<(), String> {
    startup.stage("COM apartment")?;
    let _apartment = lifetime::Apartment::enter()?;
    let title = format!("GrokCuWebViewBind-{}", std::process::id());
    let window = lifetime::Window(create_host_window(&title)?);
    let hwnd = window.0;
    let disposable = profile.is_some();
    startup.stage("environment")?;
    let environment = {
        let (tx, rx) = mpsc::channel();
        let profile = profile.map(|path| {
            use std::os::windows::ffi::OsStrExt;
            path.as_os_str()
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
    let browser_exit = if disposable {
        Some(lifetime::BrowserExit::observe(&environment, &webview)?)
    } else {
        None // An ordinary/shared environment need not exit with this view.
    };

    let nav_done = Arc::new(AtomicBool::new(false));
    let pending: PendingNavigation = Arc::new(Mutex::new(None));
    let nav_flag = nav_done.clone();
    let pending_t = pending.clone();
    let gen_h = gen.clone();
    let gen_start = gen.clone();
    let alive_start = alive.clone();
    let mut nav_start_token = 0i64;
    let mut nav_token = 0i64;
    unsafe {
        webview
            .add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_wv, _args| {
                    // Includes link/user navigation, not only Cmd::Navigate.
                    let _ = fence_generation(&gen_start, &alive_start);
                    Ok(())
                })),
                &mut nav_start_token,
            )
            .map_err(|e| e.to_string())?;
        webview
            .add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_wv, _args| {
                    nav_flag.store(true, Ordering::SeqCst);
                    if let Some(tx) = pending_t.lock().take() {
                        let g = gen_h.load(Ordering::SeqCst);
                        let _ = tx.send(Ok(g));
                    }
                    Ok(())
                })),
                &mut nav_token,
            )
            .map_err(|e| e.to_string())?;
    }
    navigate_now(&webview, &url)?;
    startup.wait_flag(&nav_done, Duration::from_secs(15))?;
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
    super::webview::isolated_windows::process::install(&webview, &world, move |failure| {
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
        alive.store(false, Ordering::SeqCst);
    }
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
                Cmd::Navigate { url, tx } => {
                    *pending.lock() = Some(tx);
                    if let Err(e) = navigate_now(&webview, &url) {
                        if let Some(tx) = pending.lock().take() {
                            let _ = tx.send(Err(e));
                        }
                    }
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
    if let Some(browser_exit) = browser_exit {
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

fn navigate_now(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    url: &str,
) -> Result<(), String> {
    let url_w: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        webview
            .Navigate(PCWSTR(url_w.as_ptr()))
            .map_err(|e| e.to_string())
    }
}

fn read_source(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
) -> Result<String, String> {
    unsafe {
        let mut raw = PWSTR::null();
        webview.Source(&mut raw).map_err(|e| e.to_string())?;
        Ok(take_pwstr(raw))
    }
}

fn run_script(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    js: &str,
    operation: Option<ScriptOperation>,
) -> Result<String, String> {
    run_script_current(webview, js, operation, || Ok(()))
}

fn run_script_current(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    js: &str,
    operation: Option<ScriptOperation>,
    current: impl Fn() -> Result<(), String>,
) -> Result<String, String> {
    if let Err(error) = current() {
        if let Some(operation) = &operation {
            operation.finished();
        }
        return Err(error);
    }
    let js_w: Vec<u16> = js.encode_utf16().chain(std::iter::once(0)).collect();
    let (tx, rx) = mpsc::channel();
    let dispatch = operation.clone();
    let completed = operation;
    let handler = ExecuteScriptCompletedHandler::create(Box::new(move |error_code, result| {
        if let Some(operation) = &completed {
            operation.finished();
        }
        let _ = tx.send(
            error_code
                .map(|()| result)
                .map_err(|error| error.to_string()),
        );
        Ok(())
    }));
    if let Err(error) = unsafe { webview.ExecuteScript(PCWSTR(js_w.as_ptr()), &handler) } {
        if let Some(operation) = &dispatch {
            operation.finished();
        }
        return Err(format!("execute script: {error:?}"));
    }
    // A missing callback cannot hold this fixture STA forever. Timeout does not
    // settle a dispatched operation; its callback/physical witness still owns it.
    startup::receive(rx, Instant::now() + Duration::from_secs(8), current)?
}

fn read_title(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
) -> Result<String, String> {
    unsafe {
        let mut raw = PWSTR::null();
        webview.DocumentTitle(&mut raw).map_err(|e| e.to_string())?;
        Ok(take_pwstr(raw))
    }
}

fn wait_flag(flag: &AtomicBool, timeout: Duration, what: &str) -> Result<(), String> {
    let start = Instant::now();
    let mut msg = MSG::default();
    while !flag.load(Ordering::SeqCst) {
        if start.elapsed() > timeout {
            return Err(format!("{what} timeout"));
        }
        unsafe {
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                thread::sleep(Duration::from_millis(8));
            }
        }
    }
    Ok(())
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
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(raw.0, len));
        CoTaskMemFree(Some(raw.0 as *const _));
        s
    }
}

fn create_host_window(title: &str) -> Result<HWND, String> {
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = windows::core::w!("GrokCuWebViewBindHost");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(host_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        CreateWindowExW(
            Default::default(),
            class,
            PCWSTR(title_w.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            480,
            320,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .map_err(|e| e.to_string())
        .inspect(|&hwnd| {
            let _ = ShowWindow(hwnd, SW_SHOW);
        })
    }
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_DESTROY {
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
