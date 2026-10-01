//! Exclusive desktop input lease: in-process + cross-instance file lock.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::LeaseError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseInfo {
    pub run_id: String,
    pub instance_id: String,
    pub pid: u32,
}

struct HeldLease {
    info: LeaseInfo,
    file: File,
}

pub struct DesktopLease {
    path: PathBuf,
    held: Mutex<Option<HeldLease>>,
}

impl DesktopLease {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            held: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn holder(&self) -> Option<LeaseInfo> {
        self.held.lock().as_ref().map(|h| h.info.clone())
    }

    pub fn try_acquire(&self, run_id: &str, instance_id: &str) -> Result<(), LeaseError> {
        let mut held = self.held.lock();
        if let Some(h) = held.as_ref() {
            if h.info.run_id == run_id && h.info.instance_id == instance_id {
                return Ok(());
            }
            return Err(LeaseError::Held {
                run_id: h.info.run_id.clone(),
                instance_id: h.info.instance_id.clone(),
            });
        }

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| LeaseError::Io(e.to_string()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&self.path)
            .map_err(|e| LeaseError::Io(e.to_string()))?;
        file.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.raw_os_error() == fs2::lock_contended_error().raw_os_error()
            {
                LeaseError::Held {
                    run_id: "other-instance".into(),
                    instance_id: "file-lock".into(),
                }
            } else {
                LeaseError::Io(e.to_string())
            }
        })?;
        {
            let mut existing = Vec::new();
            file.seek(SeekFrom::Start(0))
                .map_err(|e| LeaseError::Io(e.to_string()))?;
            file.read_to_end(&mut existing)
                .map_err(|e| LeaseError::Io(e.to_string()))?;
            if let Ok(info) = serde_json::from_slice::<LeaseInfo>(&existing) {
                if info.pid != std::process::id() && pid_is_alive(info.pid) {
                    let _ = file.unlock();
                    return Err(LeaseError::Held {
                        run_id: info.run_id,
                        instance_id: info.instance_id,
                    });
                }
            }
        }
        file.set_len(0).map_err(|e| LeaseError::Io(e.to_string()))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|e| LeaseError::Io(e.to_string()))?;
        let info = LeaseInfo {
            run_id: run_id.to_string(),
            instance_id: instance_id.to_string(),
            pid: std::process::id(),
        };
        let encoded = serde_json::to_vec(&info).map_err(|e| LeaseError::Io(e.to_string()))?;
        file.write_all(&encoded)
            .map_err(|e| LeaseError::Io(e.to_string()))?;
        file.sync_all().map_err(|e| LeaseError::Io(e.to_string()))?;

        *held = Some(HeldLease { info, file });
        Ok(())
    }

    pub fn release(&self, run_id: &str) -> Result<(), LeaseError> {
        let mut held = self.held.lock();
        let Some(h) = held.as_ref() else {
            return Ok(());
        };
        if h.info.run_id != run_id {
            return Err(LeaseError::Held {
                run_id: h.info.run_id.clone(),
                instance_id: h.info.instance_id.clone(),
            });
        }
        let taken = held.take();
        if let Some(h) = taken {
            let _ = h.file.unlock();
        }
        Ok(())
    }
}

pub(crate) fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code).is_ok();
            let _ = CloseHandle(handle);
            ok && code == STILL_ACTIVE.0 as u32
        }
    }
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(any(windows, unix)))]
    {
        pid == std::process::id()
    }
}

impl Drop for DesktopLease {
    fn drop(&mut self) {
        if let Some(h) = self.held.lock().take() {
            let _ = h.file.unlock();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_lease() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("grok-cu-lease-{n}.lock"))
    }

    #[test]
    fn second_run_cannot_steal_lease() {
        let path = temp_lease();
        let a = DesktopLease::new(path.clone());
        let b = DesktopLease::new(path);
        a.try_acquire("run-a", "inst-a").expect("a");
        let err = b.try_acquire("run-b", "inst-b").unwrap_err();
        match err {
            LeaseError::Held { .. } => {}
            other => panic!("{other:?}"),
        }
        a.release("run-a").unwrap();
        b.try_acquire("run-b", "inst-b").expect("b after release");
        b.release("run-b").unwrap();
    }

    #[test]
    fn leftover_dead_pid_lease_is_not_in_process_holder() {
        let path = temp_lease();
        let leftover = LeaseInfo {
            run_id: "ghost".into(),
            instance_id: "dead-inst".into(),
            pid: 1,
        };
        std::fs::write(&path, serde_json::to_vec(&leftover).unwrap()).unwrap();
        let lease = DesktopLease::new(path);
        assert!(lease.holder().is_none());
        lease
            .try_acquire("fresh", "me")
            .expect("dead pid leftover must be stealable, not inherited");
        assert_eq!(lease.holder().unwrap().run_id, "fresh");
        lease.release("fresh").unwrap();
    }
}
