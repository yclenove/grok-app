//! Owned child process only; both ends verify the explicitly bound private station.
use super::{text, Edit};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::StationsAndDesktops::*;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, GetWindowTextW, GetWindowThreadProcessId, PostThreadMessageW,
    TranslateMessage, MSG, WM_QUIT,
};

const STATION_ENV: &str = "GROK_CU_TEXT_PROBE_STATION";
fn initial() -> String {
    "fixture界🙂".repeat(200)
}
const TYPED: &str = "原生追加🙂";
const PASTED: &str = "粘贴追加🙂";

struct StationAttachment {
    original: HWINSTA,
    original_desktop: HDESK,
    station: HWINSTA,
    desktop: Option<HDESK>,
}
impl StationAttachment {
    fn open(name: &str) -> Result<Self, String> {
        if !name.starts_with("GrokCuClipboard-") || name.contains('\0') {
            return Err("text peer refuses a non-probe station binding".into());
        }
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let original = GetProcessWindowStation().map_err(|e| e.to_string())?;
            let original_desktop =
                GetThreadDesktop(GetCurrentThreadId()).map_err(|e| e.to_string())?;
            let station = OpenWindowStationW(PCWSTR(wide.as_ptr()), false, 0x000f037f)
                .map_err(|e| format!("owned text station open: {e}"))?;
            let mut guard = Self {
                original,
                original_desktop,
                station,
                desktop: None,
            };
            let mut flags = USEROBJECTFLAGS::default();
            GetUserObjectInformationW(
                HANDLE(station.0),
                UOI_FLAGS,
                Some((&mut flags as *mut USEROBJECTFLAGS).cast()),
                std::mem::size_of_val(&flags) as u32,
                None,
            )
            .map_err(|e| e.to_string())?;
            if flags.dwFlags & 1 != 0 {
                // WSF_VISIBLE
                return Err("text peer refuses an interactive window station".into());
            }
            SetProcessWindowStation(station).map_err(|e| e.to_string())?;
            let desktop = OpenDesktopW(w!("CuClipboard"), Default::default(), false, 0x01ff)
                .map_err(|e| format!("owned text desktop open: {e}"))?;
            guard.desktop = Some(desktop);
            SetThreadDesktop(desktop).map_err(|e| e.to_string())?;
            if station_name()? != name {
                return Err("text peer station binding mismatch".into());
            }
            Ok(guard)
        }
    }
}
impl Drop for StationAttachment {
    fn drop(&mut self) {
        unsafe {
            let _ = SetThreadDesktop(self.original_desktop);
            let _ = SetProcessWindowStation(self.original);
            if let Some(desktop) = self.desktop {
                let _ = CloseDesktop(desktop);
            }
            let _ = CloseWindowStation(self.station);
        }
    }
}

fn station_name() -> Result<String, String> {
    let mut buf = [0u16; 256];
    unsafe {
        let station = GetProcessWindowStation().map_err(|e| e.to_string())?;
        GetUserObjectInformationW(
            HANDLE(station.0),
            UOI_NAME,
            Some(buf.as_mut_ptr().cast()),
            std::mem::size_of_val(&buf) as u32,
            None,
        )
        .map_err(|e| e.to_string())?;
    }
    let end = buf
        .iter()
        .position(|c| *c == 0)
        .ok_or("unterminated station name")?;
    let name = String::from_utf16(&buf[..end]).map_err(|_| "invalid station name")?;
    if !name.starts_with("GrokCuClipboard-") {
        return Err("text peer refuses a non-probe station".into());
    }
    Ok(name)
}

struct Peer {
    child: Child,
    output: Receiver<String>,
    reader: Option<JoinHandle<()>>,
}
impl Peer {
    fn line(&self) -> Result<String, String> {
        self.output
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "owned text peer did not respond before deadline".into())
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        // Only the exact child we spawned, including on assertion/deadline failure.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(super) fn run_parent() -> Result<(), String> {
    let station = station_name()?;
    let child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("windows-text-peer")
        .env(STATION_ENV, station)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::channel();
    let mut peer = Peer {
        child,
        output: rx,
        reader: None,
    };
    let output = peer
        .child
        .stdout
        .take()
        .ok_or("owned peer stdout missing")?;
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    peer.reader = Some(reader);
    let ready: serde_json::Value =
        serde_json::from_str(&peer.line()?).map_err(|_| "invalid owned peer greeting")?;
    let raw = ready
        .get("hwnd")
        .and_then(|v| v.as_u64())
        .ok_or("owned peer HWND missing")?;
    let hwnd = HWND(raw as usize as *mut _);
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    if pid != peer.child.id() || ready.get("pid").and_then(|v| v.as_u64()) != Some(pid as u64) {
        return Err("native text peer identity mismatch".into());
    }
    if text::read(hwnd)? != initial() {
        return Err("cross-process initial readback mismatch".into());
    }
    let target = crate::computer_use::windows_adapter::native_target::NativeTarget::capture(hwnd)?;
    text::type_text(&target, TYPED, false, &|| Ok(()))?;
    text::type_text(&target, PASTED, true, &|| Ok(()))?;
    writeln!(
        peer.child
            .stdin
            .as_mut()
            .ok_or("owned peer stdin missing")?,
        "verify"
    )
    .map_err(|e| e.to_string())?;
    if peer.line()? != "native-peer-oracle:PASS" {
        return Err("owned peer oracle failed".into());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = peer.child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err("owned text peer exited unsuccessfully".into())
            };
        }
        if Instant::now() >= deadline {
            return Err("owned text peer did not exit".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(in crate::computer_use) fn run_peer() -> Result<(), String> {
    let expected_station =
        std::env::var(STATION_ENV).map_err(|_| "text peer lacks its station binding")?;
    let _station = StationAttachment::open(&expected_station)?;
    if station_name()? != expected_station {
        return Err("text peer station binding mismatch".into());
    }
    let edit = Edit::new(&initial())?;
    let thread = unsafe { GetCurrentThreadId() };
    let input = std::thread::spawn(move || {
        let mut command = String::new();
        let received =
            std::io::stdin().read_line(&mut command).is_ok() && command.trim() == "verify";
        // EOF (parent exit) also retires the peer. No orphan window/process.
        unsafe {
            let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        received
    });
    println!(
        "{}",
        serde_json::json!({"pid": std::process::id(), "hwnd": edit.hwnd.0 as usize})
    );
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut msg = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
        if result == 0 {
            break;
        }
        if result < 0 {
            return Err("owned text peer message pump failed".into());
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    if !input
        .join()
        .map_err(|_| "owned text peer input thread failed")?
    {
        return Err("parent exited before verification".into());
    }
    // Independent local API, not the product's cross-process WM_GETTEXT reader.
    let mut buf = vec![0u16; 20_000];
    let n = unsafe { GetWindowTextW(edit.hwnd, &mut buf) };
    let actual = if n > 0 {
        String::from_utf16(&buf[..n as usize]).ok()
    } else {
        None
    };
    if actual.as_deref() != Some(&format!("{}{TYPED}{PASTED}", initial())) {
        return Err("owned peer independent text oracle mismatch".into());
    }
    println!("native-peer-oracle:PASS");
    Ok(())
}
