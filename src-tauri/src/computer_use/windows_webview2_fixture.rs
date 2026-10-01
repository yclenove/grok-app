//! Host-owned WebView2 window used as an S4.5 native fixture.

#![cfg(all(target_os = "windows", feature = "computer-use-probe"))]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use uuid::Uuid;
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::{CreateCoreWebView2Environment, ICoreWebView2Controller},
    NavigationCompletedEventHandler, WebMessageReceivedEventHandler,
};
use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, PostMessageW,
    PostQuitMessage, RegisterClassW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, MSG, SW_SHOW, WM_DESTROY, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use super::adapter::ComputerUseAdapter;
use super::broker::ComputerUseBroker;
use super::protocol::{ActionKind, ActionRequest, ActionTarget, PROTOCOL_VERSION};
use super::windows_adapter::WindowsAdapter;
use super::windows_fixture::force_foreground;

pub fn run() -> Result<(), String> {
    let title = format!("GrokCuWebView2-{}", std::process::id());
    let oracle = std::env::temp_dir().join(format!(
        "grok-cu-wv2-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&oracle).map_err(|e| e.to_string())?;
    let html = oracle.join("index.html");
    std::fs::write(&html, INDEX_HTML).map_err(|e| e.to_string())?;
    let _ = std::fs::write(oracle.join("clicks.txt"), "0");
    let _ = std::fs::write(oracle.join("edit.txt"), "");

    let (ready_tx, ready_rx) = mpsc::channel::<Result<isize, String>>();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let title_t = title.clone();
    let oracle_t = oracle.clone();
    let html_t = html.clone();
    let thread = thread::spawn(move || sta_thread(title_t, html_t, oracle_t, ready_tx, stop_t));
    let hwnd_val = ready_rx
        .recv_timeout(Duration::from_secs(25))
        .map_err(|_| "webview2 host did not start".to_string())??;
    let hwnd = HWND(hwnd_val as *mut _);
    force_foreground(hwnd);

    let adapter = Arc::new(WindowsAdapter::new());
    let listed = wait_count_node(&adapter, &title)?;
    let broker = ComputerUseBroker::new(adapter.clone(), windows_s45_opts());
    broker
        .open_run("s45-wv2", "s45-wv2-run")
        .map_err(|e| e.to_string())?;
    let gen = broker
        .authorize_target("s45-wv2-run", &listed.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("s45-wv2-run").map_err(|e| e.to_string())?;
    let count_ref = obs
        .nodes
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case("count"))
        .map(|n| n.node_ref.clone())
        .ok_or_else(|| {
            format!(
                "webview2 Count missing names={:?}",
                obs.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
            )
        })?;
    let click = broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "wv2-click".into(),
        run_id: "s45-wv2-run".into(),
        target_id: listed.target_id.clone(),
        target_generation: gen,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: count_ref,
        },
        parameters: serde_json::json!({}),
    });
    if !click.executed {
        stop.store(true, Ordering::SeqCst);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_DESTROY, WPARAM(0), LPARAM(0));
        }
        let _ = thread.join();
        return Err(format!("webview2 click not executed: {click:?}"));
    }
    let start = Instant::now();
    while read(&oracle.join("clicks.txt")) != "1" {
        if start.elapsed() > Duration::from_secs(4) {
            stop.store(true, Ordering::SeqCst);
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_DESTROY, WPARAM(0), LPARAM(0));
            }
            let _ = thread.join();
            return Err(format!(
                "webview2 click file={}",
                read(&oracle.join("clicks.txt"))
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if let Some(edit_ref) = obs
        .nodes
        .iter()
        .find(|n| n.role == "edit")
        .map(|n| n.node_ref.clone())
    {
        let _ = broker.act(ActionRequest {
            version: PROTOCOL_VERSION,
            action_id: "wv2-cjk".into(),
            run_id: "s45-wv2-run".into(),
            target_id: listed.target_id,
            target_generation: gen,
            snapshot_id: obs.snapshot_id,
            geometry_revision: obs.geometry_revision,
            action: ActionKind::SetValue,
            target: ActionTarget::Element {
                element_ref: edit_ref,
            },
            parameters: serde_json::json!({ "text": "你好世界" }),
        });
        std::thread::sleep(Duration::from_millis(250));
        let edit = read(&oracle.join("edit.txt"));
        if !edit.contains("你好") {
            println!("s45 webview2 edit file={edit:?} (UIA value may not reach input)");
        }
    }

    stop.store(true, Ordering::SeqCst);
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_DESTROY, WPARAM(0), LPARAM(0));
    }
    let _ = thread.join();
    println!("gate: windows_s45_webview2");
    Ok(())
}

fn windows_s45_opts() -> super::broker::BrokerOptions {
    super::broker::BrokerOptions {
        feature_enabled: true,
        lease_path: std::env::temp_dir().join(format!(
            "grok-cu-s45-wv2-{}-{}.lease",
            std::process::id(),
            Uuid::new_v4()
        )),
        ..super::broker::BrokerOptions::default()
    }
}

fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn wait_count_node(
    adapter: &WindowsAdapter,
    title: &str,
) -> Result<super::adapter::TargetInfo, String> {
    let start = Instant::now();
    let mut last = "no observe".to_string();
    while start.elapsed() < Duration::from_secs(12) {
        if let Ok(list) = adapter.list_targets() {
            if let Some(t) = list.iter().find(|x| x.title.contains(title)).cloned() {
                match adapter.observe(&t.target_id) {
                    Ok(obs) => {
                        if obs
                            .nodes
                            .iter()
                            .any(|n| n.name.eq_ignore_ascii_case("count"))
                        {
                            return Ok(t);
                        }
                        last = format!(
                            "names={:?}",
                            obs.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
                        );
                    }
                    Err(e) => last = e,
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(format!("webview2 Count UIA not ready: {last}"))
}

fn sta_thread(
    title: String,
    html: PathBuf,
    oracle: PathBuf,
    ready_tx: mpsc::Sender<Result<isize, String>>,
    stop: Arc<AtomicBool>,
) {
    let result = sta_thread_inner(title, html, oracle, &ready_tx, stop);
    if let Err(e) = result {
        let _ = ready_tx.send(Err(e));
    }
}

fn sta_thread_inner(
    title: String,
    html: PathBuf,
    oracle: PathBuf,
    ready_tx: &mpsc::Sender<Result<isize, String>>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
    }
    let hwnd = create_host_window(&title)?;

    let environment = {
        let (tx, rx) = mpsc::channel();
        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                CreateCoreWebView2Environment(&handler).map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, environment| {
                error_code?;
                tx.send(environment.ok_or_else(|| {
                    windows::core::Error::from(windows::Win32::Foundation::E_POINTER)
                }))
                .ok();
                Ok(())
            }),
        )
        .map_err(|e| format!("webview2 env: {e:?}"))?;
        rx.recv()
            .map_err(|_| "webview2 env recv".to_string())?
            .map_err(|e| e.to_string())?
    };

    let controller: ICoreWebView2Controller = {
        let (tx, rx) = mpsc::channel();
        CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                environment
                    .CreateCoreWebView2Controller(hwnd, &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, controller| {
                error_code?;
                tx.send(controller.ok_or_else(|| {
                    windows::core::Error::from(windows::Win32::Foundation::E_POINTER)
                }))
                .ok();
                Ok(())
            }),
        )
        .map_err(|e| format!("webview2 controller: {e:?}"))?;
        rx.recv()
            .map_err(|_| "webview2 controller recv".to_string())?
            .map_err(|e| e.to_string())?
    };

    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        controller.SetBounds(rc).map_err(|e| e.to_string())?;
        controller.SetIsVisible(true).map_err(|e| e.to_string())?;
    }
    let webview = unsafe { controller.CoreWebView2().map_err(|e| e.to_string())? };

    let oracle_msg = oracle.clone();
    let mut token = 0i64;
    unsafe {
        webview
            .add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |_wv, args| {
                    if let Some(args) = args {
                        let mut raw = PWSTR::null();
                        if args.TryGetWebMessageAsString(&mut raw).is_ok() {
                            let text = take_pwstr(raw);
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                                match v.get("k").and_then(|x| x.as_str()) {
                                    Some("click") => {
                                        if let Some(n) = v.get("n").and_then(|x| x.as_u64()) {
                                            let _ = std::fs::write(
                                                oracle_msg.join("clicks.txt"),
                                                n.to_string(),
                                            );
                                        }
                                    }
                                    Some("edit") => {
                                        if let Some(s) = v.get("v").and_then(|x| x.as_str()) {
                                            let _ = std::fs::write(oracle_msg.join("edit.txt"), s);
                                        }
                                    }
                                    Some("scroll") => {
                                        if let Some(y) = v.get("y") {
                                            let _ = std::fs::write(
                                                oracle_msg.join("scroll.txt"),
                                                y.to_string(),
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|e| e.to_string())?;
    }

    let navigated = Arc::new(AtomicBool::new(false));
    let nav_flag = navigated.clone();
    let mut nav_token = 0i64;
    unsafe {
        webview
            .add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_wv, _args| {
                    nav_flag.store(true, Ordering::SeqCst);
                    Ok(())
                })),
                &mut nav_token,
            )
            .map_err(|e| e.to_string())?;
    }

    let url = format!("file:///{}", html.display().to_string().replace('\\', "/"));
    let url_w: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        webview
            .Navigate(PCWSTR(url_w.as_ptr()))
            .map_err(|e| e.to_string())?;
    }

    let mut msg = MSG::default();
    let start = Instant::now();
    while !navigated.load(Ordering::SeqCst) {
        if start.elapsed() > Duration::from_secs(15) {
            return Err("webview2 navigation timeout".into());
        }
        unsafe {
            if GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    let _ = ready_tx.send(Ok(hwnd.0 as isize));

    while !stop.load(Ordering::SeqCst) {
        unsafe {
            if GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_DESTROY {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                break;
            }
        }
    }
    unsafe {
        let _ = controller.Close();
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
        let class = windows::core::w!("GrokCuWebView2Host");
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
            640,
            480,
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
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="zh-CN"><meta charset="utf-8">
<title>GrokCuWebView2</title>
<body>
<button id="count">Count</button>
<p id="status" role="status">clicks=0</p>
<input id="edit" aria-label="edit">
<div id="scroller" style="height:140px;overflow:auto;border:1px solid #888"></div>
<script>
const scroller = document.getElementById('scroller');
for (let i = 0; i < 30; i++) {
  const d = document.createElement('div');
  d.textContent = 'item-' + String(i).padStart(2, '0');
  scroller.appendChild(d);
}
const post = (o) => {
  if (window.chrome && chrome.webview) chrome.webview.postMessage(JSON.stringify(o));
};
let n = 0;
document.getElementById('count').onclick = () => {
  n += 1;
  document.getElementById('status').textContent = 'clicks=' + n;
  post({k:'click', n});
};
document.getElementById('edit').addEventListener('input', (e) => post({k:'edit', v:e.target.value}));
scroller.addEventListener('scroll', (e) => post({k:'scroll', y:e.target.scrollTop}));
</script>
</body></html>
"#;
