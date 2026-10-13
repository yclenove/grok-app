//! Read-only, bounded process diagnostics for the original test-owned Job.
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{JobObjectBasicProcessIdList, QueryInformationJobObject};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

pub(super) fn members(job: HANDLE) -> Result<serde_json::Value, String> {
    #[repr(C)]
    struct ProcessList {
        assigned: u32,
        returned: u32,
        ids: [usize; 128],
    }
    let mut list = ProcessList {
        assigned: 0,
        returned: 0,
        ids: [0; 128],
    };
    unsafe {
        QueryInformationJobObject(
            Some(job),
            JobObjectBasicProcessIdList,
            &mut list as *mut _ as *mut _,
            std::mem::size_of_val(&list) as u32,
            None,
        )
        .map_err(|_| "original fixture Job member witness unavailable")?;
    }
    if list.returned > 128 || list.assigned > 128 {
        return Err("original fixture Job member witness exceeded fixed bound".into());
    }
    let rows: Vec<_> = list.ids[..list.returned as usize]
        .iter()
        .map(|pid| {
            let mut family = "unavailable";
            let mut active = None;
            unsafe {
                if let Ok(handle) =
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, *pid as u32)
                {
                    let mut code = 0;
                    if GetExitCodeProcess(handle, &mut code).is_ok() {
                        active = Some(code == 259);
                    }
                    let mut buffer = [0u16; 1024];
                    let mut length = buffer.len() as u32;
                    if QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        PWSTR(buffer.as_mut_ptr()),
                        &mut length,
                    )
                    .is_ok()
                    {
                        let image = String::from_utf16_lossy(&buffer[..length as usize]);
                        family = match std::path::Path::new(&image)
                            .file_name()
                            .and_then(|value| value.to_str())
                        {
                            Some("node.exe") => "node",
                            Some("chrome.exe") => "chrome",
                            Some("chrome_crashpad_handler.exe") => "crashpad",
                            Some("conhost.exe") => "console-host",
                            _ => "other",
                        };
                    }
                    let _ = CloseHandle(handle);
                }
            }
            // No paths, command lines, browser metadata or transport tokens.
            serde_json::json!({"pid":pid,"family":family,"active":active})
        })
        .collect();
    Ok(serde_json::json!(rows))
}
