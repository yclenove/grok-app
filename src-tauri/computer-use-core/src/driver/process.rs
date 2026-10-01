//! Process ownership is separate from pipe I/O, so stop never waits for a read.
use super::wire::MAX_STDERR;
use parking_lot::Mutex;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
const COOPERATIVE_WAIT: Duration = Duration::from_millis(200);

pub(super) struct Process {
    child: Mutex<Child>,
    exited: AtomicBool,
    stderr: Arc<Mutex<BoundedLog>>,
    #[cfg(windows)]
    job: windows::Win32::Foundation::HANDLE,
    #[cfg(unix)]
    group: i32,
}

struct BoundedLog {
    data: Vec<u8>,
    cap: usize,
}

impl BoundedLog {
    fn new(cap: usize) -> Self {
        Self {
            data: Vec::new(),
            cap,
        }
    }

    fn append(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
        if self.data.len() > self.cap {
            let drop = self.data.len() - self.cap;
            self.data.drain(..drop);
        }
    }

    fn preview(&self) -> String {
        redact(&String::from_utf8_lossy(&self.data))
    }
}

fn redact(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let markers = [
        "bearer ",
        "token=",
        "token: ",
        "secret=",
        "password=",
        "api_key=",
        "api-key=",
    ];
    let mut ranges = Vec::new();
    for marker in markers {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(marker) {
            let start = from + pos + marker.len();
            let end = input[start..]
                .find(|c: char| c.is_whitespace())
                .map(|i| start + i)
                .unwrap_or(input.len());
            ranges.push((start, end));
            from = start;
        }
    }
    ranges.sort_by_key(|r| std::cmp::Reverse(r.0));
    let mut out = input.to_string();
    for (start, end) in ranges {
        if start < end && end <= out.len() {
            out.replace_range(start..end, "[redacted]");
        }
    }
    out
}

// The job is an owned kernel handle. Access is independent of the serialized child.
#[cfg(windows)]
unsafe impl Send for Process {}
#[cfg(windows)]
unsafe impl Sync for Process {}

pub(super) fn command(binary: &std::path::Path, generation: &str) -> Command {
    let mut command = Command::new(binary);
    command
        .args(["__private-worker", "--generation", generation])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    // Preserve only OS/session discovery. Never inherit API keys or CUA permission overrides.
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "TEMP",
        "TMP",
        "HOME",
        "LANG",
        "LC_ALL",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
        "XAUTHORITY",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW, no flashing console.
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        #[cfg(target_os = "linux")]
        {
            // SAFETY: runs in the child after fork, before exec.
            unsafe {
                command.pre_exec(|| {
                    if libc::prctl(
                        libc::PR_SET_PDEATHSIG,
                        libc::SIGKILL as libc::c_ulong,
                        0,
                        0,
                        0,
                    ) != 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
    }
    command
}

impl Process {
    pub(super) fn own(mut child: Child) -> Result<Self, String> {
        let stderr_log = Arc::new(Mutex::new(BoundedLog::new(MAX_STDERR)));
        if let Some(pipe) = child.stderr.take() {
            let log = stderr_log.clone();
            let _ = std::thread::Builder::new()
                .name("computer-use-driver-stderr".into())
                .spawn(move || drain_stderr(pipe, log));
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::Foundation::{CloseHandle, HANDLE};
            use windows::Win32::System::JobObjects::*;
            let result = unsafe {
                (|| {
                    let job = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
                    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                    let assigned = SetInformationJobObject(
                        job,
                        JobObjectExtendedLimitInformation,
                        &info as *const _ as *const _,
                        std::mem::size_of_val(&info) as u32,
                    )
                    .and_then(|()| AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())));
                    if let Err(error) = assigned {
                        let _ = CloseHandle(job);
                        return Err(error.to_string());
                    }
                    Ok(job)
                })()
            };
            match result {
                Ok(job) => Ok(Self {
                    child: Mutex::new(child),
                    exited: AtomicBool::new(false),
                    stderr: stderr_log,
                    job,
                }),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    Err(format!("cannot supervise driver process: {error}"))
                }
            }
        }
        #[cfg(unix)]
        {
            let group = child.id() as i32;
            // Keep identical ownership behavior without mutability on Unix.
            let _ = &mut child;
            Ok(Self {
                child: Mutex::new(child),
                exited: AtomicBool::new(false),
                stderr: stderr_log,
                group,
            })
        }
    }

    pub(super) fn stderr_preview(&self) -> String {
        self.stderr.lock().preview()
    }

    pub(super) fn request_stop(&self) -> Result<(), String> {
        if self.exited.load(Ordering::Acquire) {
            return Ok(());
        }
        #[cfg(windows)]
        unsafe {
            windows::Win32::System::JobObjects::TerminateJobObject(self.job, 1)
                .map_err(|e| e.to_string())?;
        }
        #[cfg(unix)]
        {
            // Cooperative: SIGTERM the group, then SIGKILL if it has not exited.
            unsafe {
                if libc::kill(-self.group, libc::SIGTERM) != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error.to_string());
                    }
                }
            }
            let start = Instant::now();
            while start.elapsed() < COOPERATIVE_WAIT {
                if self.stopped() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            unsafe {
                if libc::kill(-self.group, libc::SIGKILL) != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error.to_string());
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn stopped(&self) -> bool {
        let mut child = self.child.lock();
        if self.exited.load(Ordering::Acquire) {
            return true;
        }
        if !matches!(child.try_wait(), Ok(Some(_))) {
            return false;
        }
        #[cfg(windows)]
        let stopped = unsafe {
            use windows::Win32::System::JobObjects::*;
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            QueryInformationJobObject(
                Some(self.job),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as u32,
                None,
            )
            .is_ok()
                && info.ActiveProcesses == 0
        };
        #[cfg(unix)]
        let stopped = unsafe {
            libc::kill(-self.group, 0) != 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        };
        if stopped {
            self.exited.store(true, Ordering::Release);
        }
        stopped
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.request_stop();
        // Reap only our own child. Never act on a target application PID.
        let _ = self.child.get_mut().wait();
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

fn drain_stderr(mut pipe: impl Read, log: Arc<Mutex<BoundedLog>>) {
    let mut buf = [0u8; 4096];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => log.lock().append(&buf[..n]),
            Err(_) => break,
        }
    }
}

#[cfg(feature = "test-support")]
pub fn pid_is_running(pid: u32) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code).is_ok();
        let _ = CloseHandle(handle);
        ok && code == 259
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, 0) == 0
    }
}
