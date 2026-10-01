//! Platform process-tree helpers. Windows Job Object + ToolHelp snapshot.

#[cfg(windows)]
use std::process::Child;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
pub(crate) fn assign_job(child: &Child) -> Result<windows::Win32::Foundation::HANDLE, String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    unsafe {
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
    }
}

#[cfg(windows)]
pub(super) fn terminate_owned_job(
    job: windows::Win32::Foundation::HANDLE,
    timeout: Duration,
) -> Result<(), String> {
    use windows::Win32::System::JobObjects::TerminateJobObject;
    unsafe { TerminateJobObject(job, 1) }
        .map_err(|e| format!("terminate owned worker job: {e}"))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if active_job_processes(job)? == 0 {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err("owned browser job still contains active processes".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
pub(super) fn active_job_processes(job: windows::Win32::Foundation::HANDLE) -> Result<u32, String> {
    use windows::Win32::System::JobObjects::{
        JobObjectBasicAccountingInformation, QueryInformationJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            Some(job),
            JobObjectBasicAccountingInformation,
            &mut info as *mut _ as *mut _,
            std::mem::size_of_val(&info) as u32,
            None,
        )
    }
    .map_err(|e| format!("query owned worker job: {e}"))?;
    Ok(info.ActiveProcesses)
}

#[cfg(feature = "computer-use-probe")]
pub(super) fn descendants_of(root: u32) -> Vec<u32> {
    #[cfg(windows)]
    {
        let snap = process_snapshot();
        let mut kids = Vec::new();
        let mut stack = vec![root];
        while let Some(parent) = stack.pop() {
            for (pid, ppid, _) in &snap {
                if *ppid == parent && *pid != root && !kids.contains(pid) {
                    kids.push(*pid);
                    stack.push(*pid);
                }
            }
        }
        kids
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Vec::new()
    }
}

#[cfg(all(windows, feature = "computer-use-probe"))]
fn process_snapshot() -> Vec<(u32, u32, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Vec::new();
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut out = Vec::new();
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                out.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        out
    }
}

#[cfg(all(windows, feature = "computer-use-probe"))]
fn is_browser_exe(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "chrome.exe"
        || n == "msedge.exe"
        || n == "chromium.exe"
        || n == "chrome_crashpad_handler.exe"
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn live_browser_descendant_pids(root: u32) -> Vec<u32> {
    live_browser_descendants(root)
        .into_iter()
        .map(|(pid, _)| pid)
        .collect()
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn live_browser_descendants(root: u32) -> Vec<(u32, String)> {
    #[cfg(windows)]
    {
        let kids = descendants_of(root);
        process_snapshot()
            .into_iter()
            .filter(|(pid, _, name)| kids.contains(pid) && is_browser_exe(name))
            .map(|(pid, _, name)| (pid, name))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Vec::new()
    }
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn tracked_process_tree(seeds: &[u32]) -> Vec<u32> {
    let mut ids: Vec<u32> = seeds.iter().copied().filter(|p| *p != 0).collect();
    for pid in seeds {
        ids.extend(descendants_of(*pid));
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn leftover_browser_pids(tracked: &[u32]) -> Vec<(u32, String)> {
    #[cfg(windows)]
    {
        process_snapshot()
            .into_iter()
            .filter(|(pid, _, name)| tracked.contains(pid) && is_browser_exe(name))
            .map(|(pid, _, name)| (pid, name))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = tracked;
        Vec::new()
    }
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        const STILL_ACTIVE: u32 = 259;
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code).is_ok();
            let _ = CloseHandle(h);
            ok && code == STILL_ACTIVE
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}
