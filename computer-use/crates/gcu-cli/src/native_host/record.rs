//! Record the current process as a native-host launch snapshot.
//!
//! Linux reads `/proc/self` and the parent executable name. Other targets fail
//! closed: this build host cannot observe their process tokens. The snapshot
//! never includes page text, cookies, or browser storage.
//!
//! Blocking: reads procfs. Timeout: none; the reads are local and bounded by
//! the files' small size. Cancellation: none.

#[cfg(target_os = "linux")]
use super::proof::build_id;
use super::proof::LaunchSnapshot;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::path::Path;

/// Why the current process could not be recorded. No path is included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordFault {
    /// This target has no reviewed process-token reader.
    Unavailable,
    /// Procfs or the parent image could not be read. Treat as unverified.
    Unreadable,
}

/// Record pid, parent image, exe, build id, and this process's argv.
///
/// Blocking: procfs reads on Linux. Timeout: none. Cancellation: none.
pub fn record_current() -> Result<LaunchSnapshot, RecordFault> {
    #[cfg(target_os = "linux")]
    {
        record_linux()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(RecordFault::Unavailable)
    }
}

#[cfg(target_os = "linux")]
fn record_linux() -> Result<LaunchSnapshot, RecordFault> {
    let host_exe = link_path("/proc/self/exe")?;
    let host_instance = start_time("/proc/self/stat")?;
    let parent_pid = parent_pid("/proc/self/status")?;
    let parent_link = format!("/proc/{parent_pid}/exe");
    let parent_exe = link_path(&parent_link)?;
    let parent_exe_name = Path::new(&parent_exe)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && !name.contains("(deleted)"))
        .ok_or(RecordFault::Unreadable)?
        .to_owned();
    let parent_stat = format!("/proc/{parent_pid}/stat");
    Ok(LaunchSnapshot {
        host_pid: std::process::id(),
        host_instance,
        host_exe,
        host_build_id: build_id(),
        parent_pid,
        parent_instance: start_time(&parent_stat)?,
        parent_exe_name,
        argv: command_args()?,
    })
}

#[cfg(target_os = "linux")]
fn link_path(path: &str) -> Result<String, RecordFault> {
    let target = fs::read_link(path).map_err(|_| RecordFault::Unreadable)?;
    let text = target.to_str().ok_or(RecordFault::Unreadable)?;
    if text.is_empty() || text.contains('\0') || text.contains("(deleted)") {
        return Err(RecordFault::Unreadable);
    }
    Ok(text.to_owned())
}

#[cfg(target_os = "linux")]
fn parent_pid(path: &str) -> Result<u32, RecordFault> {
    let status = read_bounded(path)?;
    for line in status.split('\n') {
        let Some(value) = line.strip_prefix("PPid:") else {
            continue;
        };
        return value
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or(RecordFault::Unreadable);
    }
    Err(RecordFault::Unreadable)
}

#[cfg(target_os = "linux")]
fn command_args() -> Result<Vec<String>, RecordFault> {
    let bytes = read_bytes("/proc/self/cmdline")?;
    let mut parts = bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned());
    if parts.next().is_none() {
        return Err(RecordFault::Unreadable);
    }
    Ok(parts.collect())
}

#[cfg(target_os = "linux")]
fn read_bounded(path: &str) -> Result<String, RecordFault> {
    let bytes = read_bytes(path)?;
    String::from_utf8(bytes).map_err(|_| RecordFault::Unreadable)
}

#[cfg(target_os = "linux")]
fn read_bytes(path: &str) -> Result<Vec<u8>, RecordFault> {
    let mut file = fs::File::open(path).map_err(|_| RecordFault::Unreadable)?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = file.read(&mut chunk).map_err(|_| RecordFault::Unreadable)?;
        if read == 0 {
            break;
        }
        if bytes.len() + read > 65_536 {
            return Err(RecordFault::Unreadable);
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(bytes)
}

/// Field 22 of `/proc/pid/stat`, after a comm that may contain spaces.
#[cfg(target_os = "linux")]
fn start_time(path: &str) -> Result<u64, RecordFault> {
    let stat = read_bounded(path)?;
    parse_start_time(&stat).ok_or(RecordFault::Unreadable)
}

#[cfg(target_os = "linux")]
fn parse_start_time(stat: &str) -> Option<u64> {
    let close = stat.rfind(')')?;
    let rest = stat.get(close + 1..)?.trim_start();
    let mut fields = rest.split_whitespace();
    // Field 3 is the first token after comm. starttime is field 22.
    let raw = fields.nth(19)?;
    raw.parse::<u64>().ok().filter(|value| *value > 0)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::parse_start_time;

    #[test]
    fn start_time_ignores_spaces_inside_comm() {
        let mut fields = vec!["S".to_owned()];
        fields.extend((0..18).map(|index| index.to_string()));
        fields.push("4242".to_owned());
        let stat = format!("9 (chrome (beta) one) {}", fields.join(" "));
        assert_eq!(parse_start_time(&stat), Some(4242));
        assert_eq!(parse_start_time("9 (chrome) S 0"), None);
    }
}
