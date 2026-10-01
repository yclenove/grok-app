//! Public libproc process-birth binding. No shell, names, or PID-only fallback.
//! A birth tuple fences PID reuse; it is NOT an AX/window-instance witness or
//! an atomic process handle for CGEventPostToPid. Those gates remain separate.

use super::{c_void, WindowKey};

const PROC_PIDTBSDINFO: i32 = 3;

// Apple's public <sys/proc_info.h>, MAXCOMLEN = 16 (<sys/param.h>).
// Keep all fields rather than reading undocumented offsets from a byte buffer.
#[repr(C)]
#[derive(Default)]
struct ProcBsdInfo {
    flags: u32,
    status: u32,
    xstatus: u32,
    pid: u32,
    ppid: u32,
    uid: u32,
    gid: u32,
    ruid: u32,
    rgid: u32,
    svuid: u32,
    svgid: u32,
    reserved: u32,
    comm: [u8; 16],
    name: [u8; 32],
    nfiles: u32,
    pgid: u32,
    pjobc: u32,
    tdev: u32,
    tpgid: u32,
    nice: i32,
    start_seconds: u64,
    start_microseconds: u64,
}

const _: () = assert!(std::mem::size_of::<ProcBsdInfo>() == 136);

#[cfg_attr(target_os = "macos", link(name = "proc"))]
extern "C" {
    fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut c_void, size: i32) -> i32;
}

fn birth(pid: u32) -> Result<(u64, u64), String> {
    if pid == 0 || pid > i32::MAX as u32 || pid == std::process::id() {
        return Err("macOS process identity unavailable or belongs to this Host".into());
    }
    let mut info = ProcBsdInfo::default();
    let expected = std::mem::size_of::<ProcBsdInfo>() as i32;
    let bytes = unsafe {
        proc_pidinfo(
            pid as i32,
            PROC_PIDTBSDINFO,
            0,
            std::ptr::from_mut(&mut info).cast(),
            expected,
        )
    };
    if bytes != expected
        || info.pid != pid
        || info.status == 0
        || info.status == 5 // SZOMB: an exited process is not an input owner.
        || info.start_seconds == 0
        || info.start_microseconds >= 1_000_000
    {
        return Err("cannot establish live macOS process birth identity".into());
    }
    Ok((info.start_seconds, info.start_microseconds))
}

pub(super) fn discover(pid: u32, wid: u32) -> Result<WindowKey, String> {
    let (seconds, microseconds) = birth(pid)?;
    WindowKey::new(pid, wid, seconds, microseconds)
}

pub(super) fn validate(target: WindowKey) -> Result<(), String> {
    if birth(target.pid)? != (target.birth_seconds, target.birth_microseconds) {
        return Err(
            "macOS target process was replaced; select and authorize a fresh target".into(),
        );
    }
    Ok(())
}
