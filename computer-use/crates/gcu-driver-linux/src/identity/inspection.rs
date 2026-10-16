//! Open-image/process checks shared by P3 and Wayland. Never trust a path-only digest.
#[cfg(unix)]
use super::hash::*;
use super::AppIdentity;
#[cfg(unix)]
use gcu_core::driver::Cancel;
#[cfg(unix)]
use std::path::PathBuf;
use std::{io, time::Duration};

// The independent cold-image budget allows large debug images, with 5s for
// metadata/queue overhead. No input is dispatched by either worker.
pub(crate) const INSPECTION_TIMEOUT: Duration = Duration::from_secs(185);

#[cfg(unix)]
pub(super) struct Stop(pub Cancel);
#[cfg(unix)]
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(crate) async fn inspect_async(pid: u32) -> io::Result<AppIdentity> {
    #[cfg(unix)]
    {
        run_async(move |control| inspect(pid, control)).await
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(unix)]
async fn run_async<T: Send + 'static>(
    work: impl FnOnce(&Control) -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    let stop = Stop(Cancel::default());
    let control = Control::native(stop.0.clone());
    tokio::time::timeout(
        INSPECTION_TIMEOUT,
        tokio::task::spawn_blocking(move || work(&control)),
    )
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?
}

// The legacy sync APIs are explicitly blocking, but hashing runs on a dedicated
// worker with a bounded wait. Dropping the waiter cooperatively stops its worker.
#[cfg(unix)]
pub(super) fn blocking<T: Send + 'static>(
    work: impl FnOnce(&Control) -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    let stop = Stop(Cancel::default());
    let control = Control::native(stop.0.clone());
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("gcu-image-inspect".into())
        .spawn(move || {
            let _ = sender.send(work(&control));
        })?;
    receiver
        .recv_timeout(INSPECTION_TIMEOUT)
        .map_err(|error| match error {
            std::sync::mpsc::RecvTimeoutError::Timeout => io::Error::from(io::ErrorKind::TimedOut),
            std::sync::mpsc::RecvTimeoutError::Disconnected => {
                io::Error::from(io::ErrorKind::InvalidData)
            }
        })?
}

#[cfg(unix)]
pub(super) fn digest_path(path: PathBuf, control: &Control) -> io::Result<String> {
    let mut file = std::fs::File::open(&path)?;
    shared_cache().digest(&mut file, control)
}

#[cfg(unix)]
pub(super) fn inspect(pid: u32, control: &Control) -> io::Result<AppIdentity> {
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        control.check()?;
        let start_time = super::get_process_start_time(pid)?;
        let link = format!("/proc/{pid}/exe");
        let exe_path = fs::canonicalize(&link)?;
        let mut file = fs::File::open(&link)?;
        let before = file.stamp()?;
        let path_stamp =
            |path: &std::path::Path| -> io::Result<Stamp> { Ok(Stamp::from(&fs::metadata(path)?)) };
        if before != path_stamp(&exe_path)? {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let sha256 = shared_cache().digest(&mut file, control)?;
        if before != file.stamp()?
            || before != path_stamp(&exe_path)?
            || before != path_stamp(std::path::Path::new(&link))?
            || exe_path != fs::canonicalize(&link)?
            || start_time != super::get_process_start_time(pid)?
        {
            return Err(io::ErrorKind::InvalidData.into());
        }
        control.check()?;
        Ok(AppIdentity {
            exe_path,
            dev: before.dev,
            inode: before.inode,
            sha256,
            pid,
            start_time,
            desktop_id: None,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pid, control);
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(all(test, unix))]
#[path = "inspection_tests.rs"]
mod tests;
