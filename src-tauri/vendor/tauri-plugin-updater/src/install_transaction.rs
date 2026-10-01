//! Prepared identity survives a refused cleanup/launch. Never treat elapsed
//! time, cancellation of an IPC waiter, or a previous cleanup as permission.
use crate::{Error, Result};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

struct Prepared<T> {
    identity: [u8; 32],
    value: T,
    launched: bool,
    launch_unknown: bool,
}

pub(crate) struct InstallTransaction<T> {
    prepared: Mutex<Option<Prepared<T>>>,
}

impl<T> Default for InstallTransaction<T> {
    fn default() -> Self {
        Self {
            prepared: Mutex::new(None),
        }
    }
}

impl<T> InstallTransaction<T> {
    #[cfg(windows)]
    pub(crate) fn restored(identity: [u8; 32], value: T) -> Self {
        Self {
            prepared: Mutex::new(Some(Prepared {
                identity,
                value,
                launched: false,
                launch_unknown: false,
            })),
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn inspect_prepared<R>(&self, inspect: impl FnOnce(&T) -> R) -> Option<R> {
        self.prepared
            .lock()
            .unwrap()
            .as_ref()
            .map(|prepared| inspect(&prepared.value))
    }

    pub(crate) fn has_prepared(&self) -> bool {
        self.prepared.lock().map_or(true, |state| state.is_some())
    }

    pub(crate) fn run(
        &self,
        identity: [u8; 32],
        stage: impl FnOnce() -> Result<T>,
        cleanup: impl FnOnce() -> std::result::Result<(), String>,
        launch: impl FnOnce(&T) -> std::result::Result<(), String>,
        exit: impl FnOnce() -> std::result::Result<(), String>,
    ) -> Result<()> {
        let mut slot = self
            .prepared
            .try_lock()
            .map_err(|_| Error::WindowsInstallBusy)?;
        if slot.is_none() {
            *slot = Some(Prepared {
                identity,
                value: stage()?,
                launched: false,
                launch_unknown: false,
            });
        }
        let prepared = slot.as_mut().unwrap();
        if prepared.identity != identity {
            return Err(Error::WindowsInstallBusy);
        }
        if prepared.launch_unknown {
            return Err(Error::WindowsInstallPending {
                phase: "launch_unknown",
                message:
                    "Installer launch outcome is unknown; automatic replay and exit are blocked"
                        .into(),
            });
        }
        checked("cleanup", cleanup)?;
        if !prepared.launched {
            prepared.launch_unknown = true;
            match catch_unwind(AssertUnwindSafe(|| launch(&prepared.value))) {
                Ok(Ok(())) => prepared.launch_unknown = false,
                Ok(Err(message)) => {
                    prepared.launch_unknown = false;
                    return Err(Error::WindowsInstallPending {
                        phase: "launch",
                        message,
                    });
                }
                Err(_) => {
                    return Err(Error::WindowsInstallPending {
                        phase: "launch_unknown",
                        message: "Installer launch task panicked; outcome unknown".into(),
                    })
                }
            }
            // If exit cleanup fails, retry must NOT launch the installer twice.
            prepared.launched = true;
        }
        checked("exit", exit)
    }
}

fn checked(
    phase: &'static str,
    work: impl FnOnce() -> std::result::Result<(), String>,
) -> Result<()> {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(message)) => Err(Error::WindowsInstallPending { phase, message }),
        Err(_) => Err(Error::WindowsInstallPending {
            phase,
            message: "owned update task panicked".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};

    #[test]
    fn dropping_ipc_waiter_cannot_cancel_the_owned_install_or_start_a_second() {
        let state = Arc::new(InstallTransaction::default());
        let owner = state.clone();
        let (started, waiting) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        let task = tauri::async_runtime::spawn_blocking(move || {
            let outcome = owner.run(
                [7; 32],
                || Ok(()),
                || {
                    started.send(()).unwrap();
                    released.recv().unwrap();
                    Err("owned cleanup refused".into())
                },
                |_| panic!("launch without cleanup"),
                || panic!("exit without cleanup"),
            );
            finished.send(outcome).unwrap();
        });
        waiting
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        drop(task);
        assert!(matches!(
            state.run(
                [7; 32],
                || panic!("restage"),
                || panic!("second cleanup"),
                |_| panic!("second launch"),
                || panic!("second exit")
            ),
            Err(Error::WindowsInstallBusy)
        ));
        release.send(()).unwrap();
        assert!(matches!(
            result
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap(),
            Err(Error::WindowsInstallPending {
                phase: "cleanup",
                ..
            })
        ));
        assert!(state.has_prepared());
        let calls = Mutex::new(Vec::new());
        state
            .run(
                [7; 32],
                || panic!("lost prepared state"),
                || {
                    calls.lock().unwrap().push("cleanup");
                    Ok(())
                },
                |_| {
                    calls.lock().unwrap().push("launch");
                    Ok(())
                },
                || {
                    calls.lock().unwrap().push("exit");
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(*calls.lock().unwrap(), ["cleanup", "launch", "exit"]);
    }

    #[test]
    fn preparation_failure_never_stops_services_or_launches() {
        let state = InstallTransaction::<()>::default();
        assert!(state
            .run(
                [1; 32],
                || Err(Error::InvalidUpdaterFormat),
                || panic!("cleanup"),
                |_| panic!("launch"),
                || panic!("exit")
            )
            .is_err());
        assert!(!state.has_prepared());
    }

    #[test]
    fn failed_cleanup_retains_same_prepared_package_for_retry() {
        let state = InstallTransaction::default();
        assert!(state
            .run(
                [1; 32],
                || Ok(42),
                || Err("native cleanup pending".into()),
                |_| panic!("launch"),
                || panic!("exit")
            )
            .is_err());
        assert!(state.has_prepared());
        state
            .run(
                [1; 32],
                || panic!("must not restage"),
                || Ok(()),
                |value| {
                    assert_eq!(*value, 42);
                    Ok(())
                },
                || Ok(()),
            )
            .unwrap();
    }

    #[test]
    fn launch_failure_cannot_exit_and_retry_rechecks_cleanup() {
        let state = InstallTransaction::default();
        let cleanup = AtomicUsize::new(0);
        let guard = || {
            cleanup.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        assert!(state
            .run(
                [2; 32],
                || Ok(()),
                guard,
                |_| Err("launch refused".into()),
                || panic!("exit")
            )
            .is_err());
        state
            .run([2; 32], || panic!("restage"), guard, |_| Ok(()), || Ok(()))
            .unwrap();
        assert_eq!(cleanup.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn changed_package_or_concurrent_attempt_cannot_bypass_original_cleanup() {
        let state = Arc::new(InstallTransaction::default());
        let other = state.clone();
        let (started, waiting) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            other.run(
                [3; 32],
                || Ok(()),
                || {
                    started.send(()).unwrap();
                    released.recv().unwrap();
                    Err("still pending".into())
                },
                |_| panic!("launch"),
                || panic!("exit"),
            )
        });
        waiting.recv().unwrap();
        assert!(state
            .run(
                [3; 32],
                || panic!("stage"),
                || panic!("cleanup"),
                |_| panic!("launch"),
                || panic!("exit")
            )
            .is_err());
        release.send(()).unwrap();
        assert!(thread.join().unwrap().is_err());
        assert!(state
            .run(
                [4; 32],
                || panic!("stage"),
                || panic!("cleanup"),
                |_| panic!("launch"),
                || panic!("exit")
            )
            .is_err());
    }

    #[test]
    fn panic_is_retryable_and_accepted_launch_is_not_replayed() {
        let state = InstallTransaction::default();
        let error = state
            .run(
                [5; 32],
                || Ok(()),
                || panic!("cleanup panic"),
                |_| panic!("launch"),
                || panic!("exit"),
            )
            .unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap()["code"],
            "update_install_pending"
        );
        assert!(state
            .run(
                [5; 32],
                || panic!("stage"),
                || Ok(()),
                |_| Ok(()),
                || Err("exit refused".into())
            )
            .is_err());
        state
            .run(
                [5; 32],
                || panic!("stage"),
                || Ok(()),
                |_| panic!("duplicate installer"),
                || Ok(()),
            )
            .unwrap();
    }

    #[test]
    fn ambiguous_launch_panic_never_grants_replay_or_exit() {
        let state = InstallTransaction::default();
        assert!(state
            .run(
                [6; 32],
                || Ok(()),
                || Ok(()),
                |_| panic!("unknown OS outcome"),
                || panic!("exit")
            )
            .is_err());
        assert!(state
            .run(
                [6; 32],
                || panic!("stage"),
                || panic!("cleanup"),
                |_| panic!("replay"),
                || panic!("exit")
            )
            .is_err());
    }
}
