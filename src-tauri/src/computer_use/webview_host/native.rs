//! STA navigation, script replies, strings and fixture window primitives.

use super::*;

pub(super) fn navigate_now(
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

pub(super) fn history_available(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    kind: NavigationKind,
) -> Result<bool, String> {
    let mut available = BOOL(0);
    let result = unsafe {
        match kind {
            NavigationKind::Back => webview.CanGoBack(&mut available),
            NavigationKind::Forward => webview.CanGoForward(&mut available),
            _ => return Err("history check requires back or forward".into()),
        }
    };
    result.map_err(|error| error.to_string())?;
    Ok(available.as_bool())
}

pub(super) fn dispatch_navigation_now(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    kind: NavigationKind,
    url: Option<&str>,
) -> Result<(), String> {
    match kind {
        NavigationKind::Url => navigate_now(
            webview,
            url.ok_or_else(|| "url navigation requires url".to_string())?,
        ),
        NavigationKind::Reload => unsafe { webview.Reload().map_err(|error| error.to_string()) },
        NavigationKind::Back => unsafe { webview.GoBack().map_err(|error| error.to_string()) },
        NavigationKind::Forward => unsafe {
            webview.GoForward().map_err(|error| error.to_string())
        },
    }
}

pub(super) fn read_source(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
) -> Result<String, String> {
    unsafe {
        let mut raw = PWSTR::null();
        webview.Source(&mut raw).map_err(|e| e.to_string())?;
        Ok(take_pwstr(raw))
    }
}

pub(super) fn run_script(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    js: &str,
    operation: Option<ScriptOperation>,
) -> Result<String, String> {
    run_script_current(webview, js, operation, || Ok(()))
}

pub(super) fn run_script_current(
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

pub(super) fn read_title(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
) -> Result<String, String> {
    unsafe {
        let mut raw = PWSTR::null();
        webview.DocumentTitle(&mut raw).map_err(|e| e.to_string())?;
        Ok(take_pwstr(raw))
    }
}

pub(super) fn wait_flag(flag: &AtomicBool, timeout: Duration, what: &str) -> Result<(), String> {
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

pub(super) fn wait_download_terminal(instance: u64) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut msg = MSG::default();
    while super::super::webview::probe_download_pending(instance) {
        if Instant::now() >= deadline {
            return Err("shared view download lacked a physical terminal callback".into());
        }
        unsafe {
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                // Wake on real STA messages instead of spinning or adding a
                // fixed delay to the lifecycle assertion.
                windows::Win32::UI::WindowsAndMessaging::MsgWaitForMultipleObjectsEx(
                    None,
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .min(50) as u32,
                    windows::Win32::UI::WindowsAndMessaging::QS_ALLINPUT,
                    windows::Win32::UI::WindowsAndMessaging::MWMO_INPUTAVAILABLE,
                );
            }
        }
    }
    Ok(())
}

pub(super) fn take_pwstr(raw: PWSTR) -> String {
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

pub(super) fn create_host_window(title: &str) -> Result<HWND, String> {
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
