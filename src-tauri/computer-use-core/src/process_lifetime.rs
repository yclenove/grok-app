//! Kernel facts for a witness captured by the original authenticated Host.
//! This is not proof authentication and never mutates a completion registry.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HostLifetime {
    pub instance_id: String,
    pub platform: String,
    pub scope: String,
    pub pid: u32,
    pub birth: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRetirement {
    Live,
    Retired,
    Unavailable,
}

enum KernelProcess {
    Live(String),
    Gone,
}

impl HostLifetime {
    pub fn capture(instance_id: &str) -> Result<Self, String> {
        let pid = std::process::id();
        let KernelProcess::Live(birth) = kernel_process(pid)? else {
            return Err("Host process identity unavailable".into());
        };
        let value = Self {
            instance_id: instance_id.into(),
            platform: std::env::consts::OS.into(),
            scope: kernel_scope()?,
            pid,
            birth,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if uuid::Uuid::parse_str(&self.instance_id).is_err()
            || self.instance_id.len() != 36
            || !matches!(self.platform.as_str(), "windows" | "linux" | "macos")
            || self.pid == 0
            || self.pid > i32::MAX as u32
            || !hex_stamp(&self.scope)
            || !hex_stamp(&self.birth)
        {
            return Err("invalid Host process identity".into());
        }
        Ok(())
    }

    pub fn retirement(&self) -> ProcessRetirement {
        use ProcessRetirement::*;
        if self.validate().is_err()
            || self.platform != std::env::consts::OS
            || kernel_scope().ok().as_ref() != Some(&self.scope)
        {
            return Unavailable;
        }
        match kernel_process(self.pid) {
            Ok(KernelProcess::Live(birth)) if birth == self.birth => Live,
            Ok(KernelProcess::Live(_) | KernelProcess::Gone) => Retired,
            Err(_) => Unavailable,
        }
    }
}

fn hex_stamp(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(windows)]
fn kernel_scope() -> Result<String, String> {
    Ok("0".into())
}

#[cfg(windows)]
fn kernel_process(pid: u32) -> Result<KernelProcess, String> {
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE,
    };
    unsafe {
        let handle = match OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        ) {
            Ok(handle) => handle,
            Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => {
                return Ok(KernelProcess::Gone)
            }
            Err(_) => return Err("Host process query unavailable".into()),
        };
        let result = (|| {
            match WaitForSingleObject(handle, 0) {
                WAIT_OBJECT_0 => return Ok(KernelProcess::Gone),
                WAIT_TIMEOUT => {}
                _ => return Err("Host process wait unavailable".into()),
            }
            let mut created = FILETIME::default();
            let mut exited = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user)
                .map_err(|_| "Host process time unavailable")?;
            Ok(KernelProcess::Live(format!(
                "{:08x}{:08x}",
                created.dwHighDateTime, created.dwLowDateTime
            )))
        })();
        let _ = CloseHandle(handle);
        result
    }
}

#[cfg(target_os = "linux")]
fn kernel_scope() -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|_| "boot identity unavailable")?;
    uuid::Uuid::parse_str(boot.trim()).map_err(|_| "boot identity invalid")?;
    let ns = std::fs::metadata("/proc/self/ns/pid").map_err(|_| "process namespace unavailable")?;
    Ok(hex::encode(Sha256::digest(
        format!("{}:{}:{}", boot.trim(), ns.dev(), ns.ino()).as_bytes(),
    )))
}

#[cfg(target_os = "linux")]
fn kernel_process(pid: u32) -> Result<KernelProcess, String> {
    use std::io::Read;
    match std::fs::File::open(format!("/proc/{pid}/stat")) {
        Ok(file) => {
            let mut bytes = String::new();
            file.take(8193)
                .read_to_string(&mut bytes)
                .map_err(|_| "process stat unavailable")?;
            if bytes.len() > 8192 {
                return Err("process stat limit".into());
            }
            let (_, fields) = bytes.rsplit_once(") ").ok_or("process stat invalid")?;
            let start = fields
                .split_whitespace()
                .nth(19)
                .ok_or("process start missing")?
                .parse::<u64>()
                .map_err(|_| "process start invalid")?;
            Ok(KernelProcess::Live(format!("{start:x}")))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // hidepid/permissions or a missing proc mount must not become death evidence.
            if unsafe { libc::kill(pid as i32, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                Ok(KernelProcess::Gone)
            } else {
                Err("process existence uncertain".into())
            }
        }
        Err(_) => Err("process stat unavailable".into()),
    }
}

#[cfg(target_os = "macos")]
fn kernel_scope() -> Result<String, String> {
    Ok("0".into())
}

#[cfg(target_os = "macos")]
fn kernel_process(pid: u32) -> Result<KernelProcess, String> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if count == size {
        Ok(KernelProcess::Live(format!(
            "{:016x}{:016x}",
            info.pbi_start_tvsec, info.pbi_start_tvusec
        )))
    } else if count == 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(KernelProcess::Gone)
    } else {
        Err("process info unavailable".into())
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn kernel_scope() -> Result<String, String> {
    Err("unsupported process identity".into())
}
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn kernel_process(_pid: u32) -> Result<KernelProcess, String> {
    Err("unsupported process identity".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kernel_self_is_live_but_a_reused_pid_does_not_match_the_original_birth() {
        let mut witness = HostLifetime::capture(&uuid::Uuid::new_v4().to_string()).unwrap();
        assert_eq!(witness.retirement(), ProcessRetirement::Live);
        witness.birth.push('0');
        assert_eq!(witness.retirement(), ProcessRetirement::Retired);
        witness.scope.push('0');
        assert_eq!(witness.retirement(), ProcessRetirement::Unavailable);
        witness.pid = 0;
        assert!(witness.validate().is_err());
    }
}
