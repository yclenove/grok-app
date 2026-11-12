use crate::io::{self, Dir};
#[cfg(test)]
use std::cell::{Cell, RefCell};
use std::io::{self as stdio, ErrorKind};
use std::time::Instant;

pub const DATA_FILES: u64 = 80;
pub const DATA_CHUNK: u64 = 1024 * 1024;
pub const DATA_BATCH: u64 = 8;
pub const SMALL_FILES: u64 = 8192;
pub const SMALL_CHUNK: u64 = 1024;
pub const SMALL_BATCH: u64 = 512;
/// Logical size and allocated bytes of the live image must stay under this.
pub const IMAGE_CAP: u64 = 80 * 1024 * 1024;

#[derive(Clone, Copy)]
pub struct Plan {
    pub files: u64,
    pub chunk: u64,
    pub batch: u64,
    /// Another chunk that would pass this many bytes is reported as ENOSPC.
    /// Production uses `u64::MAX`; tests pass a small quota.
    pub quota: u64,
    pub tag: char,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub bytes: u64,
    pub files: u64,
    pub batches: u64,
    pub enospc: bool,
}
impl Report {
    fn empty() -> Self {
        Self { bytes: 0, files: 0, batches: 0, enospc: false }
    }
}
pub fn plan_for(mode: &str) -> Option<Plan> {
    match mode {
        "data" => Some(Plan { files: DATA_FILES, chunk: DATA_CHUNK, batch: DATA_BATCH, quota: IMAGE_CAP, tag: 'd' }),
        "small" => Some(Plan { files: SMALL_FILES, chunk: SMALL_CHUNK, batch: SMALL_BATCH, quota: u64::MAX, tag: 's' }),
        _ => None,
    }
}
pub fn is_enospc(err: &stdio::Error) -> bool {
    err.kind() == ErrorKind::StorageFull || err.raw_os_error() == Some(28)
}
#[cfg(test)]
thread_local! {
    static PLAN_OVERRIDE: RefCell<Option<Plan>> = const { RefCell::new(None) };
    static UNLINK_FAIL: Cell<bool> = const { Cell::new(false) };
}
#[cfg(test)]
pub fn test_set_plan(plan: Plan) {
    PLAN_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(plan));
}
#[cfg(test)]
pub fn test_clear_plan() {
    PLAN_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
}
#[cfg(test)]
pub fn test_fail_unlink(fail: bool) {
    UNLINK_FAIL.with(|slot| slot.set(fail));
}
pub fn resolve(mode: &str) -> Option<Plan> {
    #[cfg(test)]
    {
        if let Some(plan) = PLAN_OVERRIDE.with(|slot| *slot.borrow()) {
            return Some(plan);
        }
    }
    plan_for(mode)
}
fn progress(seq: u64, report: &Report) -> String {
    format!("v=1 seq={seq} bytes={} files={} enospc={}\n", report.bytes, report.files, u8::from(report.enospc))
}
fn publish_progress(mailbox: &Dir, seq: u64, report: &Report, end: Instant) {
    let line = progress(seq, report);
    if io::past(end) {
        let _ = mailbox.publish_diag("progress.tmp", "progress", line.as_bytes(), false);
    } else {
        let _ = mailbox.publish("progress.tmp", "progress", line.as_bytes(), false, end);
    }
}
/// Write at most `plan.files` chunks. Stop on ENOSPC (real or quota) and on `end`.
pub fn run(payload: &Dir, mailbox: &Dir, plan: Plan, end: Instant) -> Report {
    if plan.chunk == 0 || plan.chunk > DATA_CHUNK || plan.files > SMALL_FILES || plan.batch == 0 || !plan.tag.is_ascii_alphanumeric() {
        return Report::empty();
    }
    let buf = vec![0u8; plan.chunk as usize];
    let mut report = Report::empty();
    let mut seq = 0u64;
    let mut published_files = 0u64;
    while report.files < plan.files {
        if io::past(end) {
            break;
        }
        if report.bytes.saturating_add(plan.chunk) > plan.quota {
            report.enospc = true;
            break;
        }
        let name = format!("{}-{:04}", plan.tag, report.files);
        match payload.create_exclusive(&name, &buf) {
            Ok(()) => {
                report.files += 1;
                report.bytes = report.bytes.saturating_add(plan.chunk);
            }
            Err(err) if is_enospc(&err) => {
                report.enospc = true;
                break;
            }
            Err(_) => break,
        }
        if report.files.is_multiple_of(plan.batch) {
            seq += 1;
            publish_progress(mailbox, seq, &report, end);
            published_files = report.files;
        }
    }
    if published_files != report.files || report.enospc || seq == 0 {
        seq += 1;
        publish_progress(mailbox, seq, &report, end);
    }
    report.batches = seq;
    report
}
pub fn remove_last(payload: &Dir, plan: Plan, files: u64) -> bool {
    if files == 0 {
        return false;
    }
    #[cfg(test)]
    if UNLINK_FAIL.with(|slot| slot.replace(false)) {
        return false;
    }
    let name = format!("{}-{:04}", plan.tag, files - 1);
    payload.unlink_file(&name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    fn scratch() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("s11m-fillmod-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("payload")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        fs::canonicalize(&p).unwrap()
    }
    fn dirs(root: &std::path::Path) -> (Dir, Dir) {
        let mailbox = Dir::open_path(root.to_str().unwrap()).unwrap();
        let payload = mailbox.open_child_dir("payload").unwrap();
        (mailbox, payload)
    }

    #[test]
    fn plans_match_the_caps() {
        let data = plan_for("data").unwrap();
        let small = plan_for("small").unwrap();
        assert_eq!(data.files, 80);
        assert_eq!(data.chunk, 1024 * 1024);
        assert_eq!(data.files * data.chunk, IMAGE_CAP);
        assert_eq!(small.files, 8192);
        assert_eq!(small.chunk, 1024);
        assert!(small.files * small.chunk <= IMAGE_CAP);
        assert!(plan_for("sparse").is_none());
        let err = stdio::Error::from_raw_os_error(28);
        assert!(is_enospc(&err));
        assert!(is_enospc(&stdio::Error::new(ErrorKind::StorageFull, "full")));
        assert!(!is_enospc(&stdio::Error::new(ErrorKind::AlreadyExists, "x")));
    }

    #[test]
    fn quota_stops_like_enospc() {
        let root = scratch();
        let (mailbox, payload) = dirs(&root);
        let plan = Plan { files: 10, chunk: 1000, batch: 4, quota: 2500, tag: 's' };
        let end = Instant::now() + Duration::from_secs(5);
        let report = run(&payload, &mailbox, plan, end);
        assert_eq!(report, Report { bytes: 2000, files: 2, batches: 1, enospc: true });
        assert!(root.join("payload/s-0000").is_file());
        assert!(root.join("payload/s-0001").is_file());
        assert!(!root.join("payload/s-0002").exists());
        let progress = fs::read_to_string(root.join("progress")).unwrap();
        assert!(progress.contains("enospc=1"), "{progress}");
        assert!(progress.contains("bytes=2000"), "{progress}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_cap_without_enospc() {
        let root = scratch();
        let (mailbox, payload) = dirs(&root);
        let plan = Plan { files: 3, chunk: 10, batch: 2, quota: u64::MAX, tag: 'd' };
        let end = Instant::now() + Duration::from_secs(5);
        let report = run(&payload, &mailbox, plan, end);
        assert_eq!(report.files, 3);
        assert_eq!(report.bytes, 30);
        assert!(!report.enospc);
        assert!(report.batches >= 2);
        assert!(!root.join("payload/d-0003").exists());
        let _ = fs::remove_dir_all(&root);
    }
}
