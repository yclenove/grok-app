//! Windows process lifetime containment. All Job Object FFI lives here.
use gcu_core::driver::{DriverError, Result};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::*,
};

pub(super) struct Job(HANDLE);
// SAFETY: the handle is exclusively owned, never inherited, and only closed on Drop.
unsafe impl Send for Job {}
impl Job {
    pub(super) fn attach(child: &tokio::process::Child) -> Result<Self> {
        let error = || DriverError::Crashed {
            maybe_executed: false,
        };
        // SAFETY: null security attributes create a noninheritable unnamed job.
        let job = Self(unsafe { CreateJobObjectW(None, None) }.map_err(|_| error())?);
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let process = child.raw_handle().ok_or_else(error)?;
        // SAFETY: job and child handles remain live. Info is correctly sized and
        // initialized. No breakaway flag: descendants stay in the lifetime job.
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(|_| error())?;
            AssignProcessToJobObject(job.0, HANDLE(process)).map_err(|_| error())?;
        }
        Ok(job)
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: this is the only owner; closing kills any remaining descendants.
        if unsafe { CloseHandle(self.0) }.is_err() {
            tracing::warn!("worker Job close failed");
        }
    }
}

/// OS-verified parent process instance, independent of worker incarnations.
pub(super) fn owner_instance() -> Result<String> {
    use windows::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: querying our own live pseudo-handle; all FILETIME outputs are valid.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    }
    .map_err(|_| DriverError::Crashed {
        maybe_executed: false,
    })?;
    let created = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    Ok(format!("{}.{created}", std::process::id()))
}
