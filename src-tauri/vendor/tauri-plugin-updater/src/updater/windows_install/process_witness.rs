//! Read-only observation of the exact accepted OS process. A held handle avoids
//! PID ABA after attach; missing/inaccessible/reused PIDs never prove an outcome.
use super::super::windows_journal::{ProcessExit, ProcessIdentity};
use super::durable::process_identity;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_PARAMETER, FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Observation {
    Running,
    Exited(ProcessExit),
    Missing,
    IdentityMismatch,
    Unavailable(String),
}

pub(super) struct ProcessWitness {
    identity: ProcessIdentity,
    handle: OwnedHandle,
}

impl ProcessWitness {
    pub fn attach(identity: &ProcessIdentity) -> Result<Self, Observation> {
        // No VM access, termination, injection or inherited handle rights.
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                identity.pid,
            )
        };
        if raw.is_null() {
            let error = std::io::Error::last_os_error();
            return Err(
                if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                    Observation::Missing
                } else {
                    Observation::Unavailable(error.to_string())
                },
            );
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        Self::from_handle(handle, identity)
    }

    pub fn from_handle(
        handle: OwnedHandle,
        identity: &ProcessIdentity,
    ) -> Result<Self, Observation> {
        if process_identity(&handle).map_err(Observation::Unavailable)? != *identity {
            return Err(Observation::IdentityMismatch);
        }
        Ok(Self {
            identity: identity.clone(),
            handle,
        })
    }

    pub fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }

    pub fn observe(&self) -> Observation {
        let handle = self.handle.as_raw_handle();
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_TIMEOUT => Observation::Running,
            WAIT_OBJECT_0 => {
                let mut code = 0;
                let mut created: FILETIME = unsafe { std::mem::zeroed() };
                let mut exited = created;
                let mut kernel = created;
                let mut user = created;
                if unsafe { GetExitCodeProcess(handle, &mut code) } == 0
                    || unsafe {
                        GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user)
                    } == 0
                {
                    return Observation::Unavailable(std::io::Error::last_os_error().to_string());
                }
                let created =
                    ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
                let exited = ((exited.dwHighDateTime as u64) << 32) | exited.dwLowDateTime as u64;
                if created != self.identity.created || exited < created || exited == 0 {
                    return Observation::IdentityMismatch;
                }
                // 259/STILL_ACTIVE is a legal exit code. Only the signaled
                // handle, not the numeric exit code, determines termination.
                Observation::Exited(ProcessExit {
                    identity: self.identity.clone(),
                    exited,
                    code,
                })
            }
            _ => Observation::Unavailable(std::io::Error::last_os_error().to_string()),
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::os::windows::io::AsHandle;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    pub(crate) struct OwnedChild {
        root: tempfile::TempDir,
        child: Option<Child>,
        code: i32,
    }

    impl OwnedChild {
        pub(crate) fn spawn(code: i32) -> Self {
            let root = tempfile::tempdir().unwrap();
            let exe = root.path().join("owned-process-witness-fixture.exe");
            std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
            std::fs::write(
                root.path().join("fixture-purpose"),
                b"owned process witness only",
            )
            .unwrap();
            let child = Command::new(exe)
                .args([
                    "--exact",
                    "updater::windows_install::process_witness::tests::owned_process_witness_child",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env("GROK_UPDATER_WITNESS_TEST_ROOT", root.path())
                .env("GROK_UPDATER_WITNESS_TEST_EXIT", code.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut owned = Self {
                root,
                child: Some(child),
                code,
            };
            let deadline = Instant::now() + Duration::from_secs(10);
            while !owned.root.path().join("ready").exists() {
                assert!(
                    owned.child.as_mut().unwrap().try_wait().unwrap().is_none(),
                    "owned fixture exited before readiness"
                );
                assert!(
                    Instant::now() < deadline,
                    "owned fixture readiness timed out"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            owned
        }

        pub(crate) fn identity(&self) -> ProcessIdentity {
            let handle = self
                .child
                .as_ref()
                .unwrap()
                .as_handle()
                .try_clone_to_owned()
                .unwrap();
            process_identity(&handle).unwrap()
        }

        pub(crate) fn handle(&self) -> OwnedHandle {
            self.child
                .as_ref()
                .unwrap()
                .as_handle()
                .try_clone_to_owned()
                .unwrap()
        }

        pub(crate) fn release_path(&self) -> std::path::PathBuf {
            self.root.path().join("release")
        }

        pub(crate) fn finish(&mut self) {
            std::fs::write(self.root.path().join("release"), b"release only this child").unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                    assert_eq!(status.code(), Some(self.code));
                    drop(self.child.take());
                    return;
                }
                assert!(Instant::now() < deadline, "owned fixture exit timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if let Some(child) = self.child.as_mut() {
                if !matches!(child.try_wait(), Ok(Some(_))) {
                    let _ = child.kill();
                }
                let _ = child.wait();
            }
        }
    }

    #[test]
    #[ignore = "parent launches this exact private test executable and checks identity/exit"]
    fn owned_process_witness_child() {
        let root = std::path::PathBuf::from(
            std::env::var_os("GROK_UPDATER_WITNESS_TEST_ROOT").expect("owned parent only"),
        );
        assert_eq!(
            std::env::current_exe().unwrap().parent(),
            Some(root.as_path())
        );
        assert_eq!(
            std::fs::read(root.join("fixture-purpose")).unwrap(),
            b"owned process witness only"
        );
        let code: i32 = std::env::var("GROK_UPDATER_WITNESS_TEST_EXIT")
            .unwrap()
            .parse()
            .unwrap();
        std::fs::write(root.join("ready"), b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !root.join("release").exists() {
            assert!(
                Instant::now() < deadline,
                "owned parent did not release child"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        std::process::exit(code);
    }

    #[test]
    fn exact_signaled_handle_retains_real_exit_259_after_original_handle_drop() {
        let mut child = OwnedChild::spawn(259);
        let identity = child.identity();
        let witness = ProcessWitness::attach(&identity).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(witness.observe(), Observation::Running);
        child.finish();
        let Observation::Exited(exit) = witness.observe() else {
            panic!("exit code 259 must not mean still running")
        };
        assert_eq!(exit.identity, identity);
        assert_eq!(exit.code, 259);
        assert!(exit.exited >= identity.created);
        assert_eq!(witness.observe(), Observation::Exited(exit));
    }

    #[test]
    fn mismatched_creation_identity_and_missing_pid_never_prove_completion() {
        let mut child = OwnedChild::spawn(0);
        let identity = child.identity();
        let changed = ProcessIdentity {
            created: identity.created + 1,
            ..identity
        };
        assert!(matches!(
            ProcessWitness::attach(&changed),
            Err(Observation::IdentityMismatch)
        ));
        assert!(matches!(
            ProcessWitness::attach(&ProcessIdentity { pid: 0, created: 1 }),
            Err(Observation::Missing)
        ));
        assert!(
            child.child.as_mut().unwrap().try_wait().unwrap().is_none(),
            "observation must never terminate the process"
        );
        child.finish();
    }
}
