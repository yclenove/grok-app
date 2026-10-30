//! Live `OsFacts`. Process identity comes from `libproc` and `sysctl`.
//! Window rows come from `CGWindowList`. Continuity tokens come from the
//! accessibility thread. A failed read is `None`.

use std::sync::Arc;

use super::facts::{ElementFact, OsFacts, OsWindow, TokenBook};
use crate::real::{copy_windows, element_fact, AxThread, AX_CALL_TIMEOUT};

/// OS reads for the production driver.
pub struct LiveOsFacts {
    thread: Option<Arc<AxThread>>,
    book: Arc<TokenBook>,
}

impl LiveOsFacts {
    /// Facts with no accessibility thread. Process and boot reads still work.
    ///
    /// The own-process probe is the only caller. Production evidence always
    /// has the accessibility thread.
    #[cfg(test)]
    pub(crate) fn process_only() -> Self {
        Self {
            thread: None,
            book: TokenBook::new(),
        }
    }

    fn with_thread(thread: Arc<AxThread>) -> Arc<Self> {
        Arc::new(Self {
            thread: Some(thread),
            book: TokenBook::new(),
        })
    }
}

impl Drop for LiveOsFacts {
    fn drop(&mut self) {
        let Some(thread) = self.thread.clone() else {
            return;
        };
        let book = Arc::clone(&self.book);
        let _ = thread.call_blocking(AX_CALL_TIMEOUT, false, move |_world| {
            crate::real::release_book(&book);
            Ok(())
        });
    }
}

impl OsFacts for LiveOsFacts {
    fn boot_identity(&self) -> Option<String> {
        boot_identity()
    }

    fn process_start_us(&self, pid: u32) -> Option<u64> {
        process_start_us(pid)
    }

    fn executable_path(&self, pid: u32) -> Option<String> {
        executable_path(pid)
    }

    fn windows(&self) -> Option<Vec<OsWindow>> {
        let rows = copy_windows()?;
        let tokens = self.book.published();
        Some(
            rows.into_iter()
                .map(|row| OsWindow {
                    cg_id: row.window_id,
                    owner_pid: row.pid,
                    layer: row.layer,
                    alpha: row.alpha,
                    frame: row.frame,
                    on_screen: true,
                    token: tokens.get(&row.window_id).copied(),
                })
                .collect(),
        )
    }

    fn element(&self, pid: u32, id: u64) -> Option<ElementFact> {
        let thread = self.thread.as_ref()?;
        let fact = thread
            .call_blocking(AX_CALL_TIMEOUT, false, move |world| {
                Ok(element_fact(world, id))
            })
            .ok()??;
        if fact.pid != pid {
            None
        } else {
            Some(fact)
        }
    }

    fn snapshot(&self) -> Option<super::facts::Snapshot> {
        if std::thread::current().name() == Some("gcu-macos-ax") {
            return None;
        }
        let thread = self.thread.as_ref()?;
        let book = Arc::clone(&self.book);
        thread
            .call_blocking(AX_CALL_TIMEOUT, false, move |world| {
                Ok(crate::real::observe_snapshot(world, &book))
            })
            .ok()
            .flatten()
    }

    fn token_book(&self) -> Option<Arc<TokenBook>> {
        Some(Arc::clone(&self.book))
    }
}

/// Live evidence. Scoped checks start only after a scope pins a binding.
pub(crate) fn live_evidence(thread: Arc<AxThread>) -> Arc<super::Evidence> {
    super::Evidence::live_adapter(LiveOsFacts::with_thread(thread))
}

/// Boot session UUID. `kern.boottime` is read and then ignored.
pub(crate) fn boot_identity() -> Option<String> {
    let uuid = sysctl_string(c"kern.bootsessionuuid");
    let boot_time = sysctl_boottime().and_then(|(sec, usec)| {
        if sec == 0 && usec == 0 {
            None
        } else {
            Some(format!("{sec}.{usec}"))
        }
    });
    super::facts::boot_identity_from(uuid.as_deref(), boot_time.as_deref())
}

/// One snapshot from rows already observed on this thread.
pub(crate) fn snapshot_from_rows(rows: Vec<super::facts::OsWindow>) -> super::facts::Snapshot {
    use std::collections::HashMap;
    let boot = boot_identity();
    let mut starts = HashMap::new();
    for row in &rows {
        starts.insert(row.owner_pid, process_start_us(row.owner_pid).unwrap_or(0));
    }
    super::facts::Snapshot { rows, boot, starts }
}

/// Process start in microseconds, or `None` when the pid is gone or the time is zero.
pub(crate) fn process_start_us(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    // SAFETY: `info` is a writable `proc_bsdinfo`. The flavor is
    // `PROC_PIDTBSDINFO` (3). A non-positive return means the pid is gone.
    // The buffer is read only when the kernel wrote the whole struct.
    let wrote = unsafe {
        proc_pidinfo(
            pid as i32,
            PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    if wrote <= 0 || wrote as usize != std::mem::size_of::<libc::proc_bsdinfo>() {
        return None;
    }
    // SAFETY: `proc_pidinfo` reported a full write into `info`.
    let info = unsafe { info.assume_init() };
    if info.pbi_start_tvsec == 0 && info.pbi_start_tvusec == 0 {
        return None;
    }
    info.pbi_start_tvsec
        .checked_mul(1_000_000)?
        .checked_add(info.pbi_start_tvusec)
}

/// `proc_pidpath`. Evidence only. Empty and relative paths are `None`.
#[allow(dead_code)]
pub(crate) fn executable_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let mut buf = [0u8; 4096];
    // SAFETY: `buf` is a writable 4096-byte buffer. A non-positive return means
    // the path is unavailable. The bytes up to the returned length are read.
    let wrote = unsafe { proc_pidpath(pid as i32, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if wrote <= 0 {
        return None;
    }
    let wrote = (wrote as usize).min(buf.len());
    let end = buf[..wrote]
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(wrote);
    let path = std::str::from_utf8(&buf[..end]).ok()?.trim();
    if path.is_empty() || !path.starts_with('/') {
        None
    } else {
        Some(path.to_owned())
    }
}

fn sysctl_string(name: &std::ffi::CStr) -> Option<String> {
    let mut len = 0usize;
    // SAFETY: `name` is NUL-terminated. A null oldp queries the size only.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len == 0 || len > 256 {
        return None;
    }
    let mut buf = vec![0u8; len];
    // SAFETY: `buf` has `len` bytes. The kernel writes at most `len`.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len == 0 || len > buf.len() {
        return None;
    }
    let text = String::from_utf8_lossy(&buf[..len])
        .trim_end_matches('\0')
        .trim()
        .to_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn sysctl_boottime() -> Option<(i64, i32)> {
    let mut value = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let mut len = std::mem::size_of::<libc::timeval>();
    // SAFETY: `value` is a writable `timeval` and `len` is its size.
    // `kern.boottime` is a timeval. A non-zero return is a failed read.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.boottime".as_ptr(),
            (&mut value as *mut libc::timeval).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len < std::mem::size_of::<i64>() + std::mem::size_of::<i32>() {
        None
    } else {
        Some((value.tv_sec, value.tv_usec))
    }
}

const PROC_PIDTBSDINFO: i32 = 3;

// `pbi_nice` ends on an 8-byte boundary, so the following `u64` fields have no
// extra padding. A hand-rolled pad would read the start time from the wrong offset.
const _: () = assert!(std::mem::size_of::<libc::proc_bsdinfo>() == 136);
const _: () = assert!(std::mem::offset_of!(libc::proc_bsdinfo, pbi_start_tvsec) == 120);
const _: () = assert!(std::mem::offset_of!(libc::proc_bsdinfo, pbi_start_tvusec) == 128);

#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut std::ffi::c_void,
        buffersize: i32,
    ) -> i32;
    fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, buffersize: u32) -> i32;
}
