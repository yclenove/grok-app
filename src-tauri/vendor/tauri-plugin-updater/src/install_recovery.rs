//! Process-owned recovery, independent of Webview resource lifetime. Locks only
//! protect metadata; never hold one while staging, draining or launching.
#[cfg(any(windows, test))]
use crate::{Error, Result};
use serde::Serialize;
#[cfg(any(windows, test))]
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingInstall {
    pub candidate_id: String,
    pub version: String,
    pub state: RecoveryState,
    pub phase: &'static str,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
#[serde(rename_all = "lowercase")]
pub(crate) enum RecoveryState {
    Running,
    Retryable,
    Blocked,
    Failed,
    #[cfg_attr(not(windows), allow(dead_code))]
    Completed,
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct CandidateKey {
    pub transaction: usize,
    pub digest: [u8; 32],
}

#[cfg(any(windows, test))]
struct Candidate<T> {
    key: CandidateKey,
    value: Arc<T>,
    metadata: PendingInstall,
}

#[cfg(any(windows, test))]
pub(crate) struct InstallRecovery<T> {
    slot: Mutex<Option<Candidate<T>>>,
}

#[cfg(any(windows, test))]
impl<T> InstallRecovery<T> {
    pub(crate) const fn new() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<Candidate<T>>>> {
        self.slot.lock().map_err(|_| Error::WindowsInstallPending {
            phase: "task",
            message: "Install recovery metadata is poisoned; replay is blocked".into(),
        })
    }

    pub(crate) fn snapshot(&self) -> Result<Option<PendingInstall>> {
        Ok(self.lock()?.as_ref().map(|c| c.metadata.clone()))
    }

    #[cfg(windows)]
    pub(crate) fn retire_verified(
        &self,
        id: &str,
        publish: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let mut slot = self.lock()?;
        let candidate = slot
            .as_mut()
            .filter(|c| c.metadata.candidate_id == id && c.metadata.state == RecoveryState::Blocked)
            .ok_or(Error::WindowsInstallRecoveryMissing)?;
        // Serialize the short durable terminal CAS with in-process release.
        // No cleanup, installation, network or process launch occurs here.
        publish()?;
        candidate.metadata.state = RecoveryState::Completed;
        candidate.metadata.phase = "completed";
        candidate.metadata.message = None;
        Ok(())
    }

    #[cfg(windows)]
    pub(crate) fn restore(
        &self,
        key: CandidateKey,
        metadata: PendingInstall,
        value: T,
    ) -> Result<()> {
        let mut slot = self.lock()?;
        if slot.is_some() {
            return Err(Error::WindowsInstallBusy);
        }
        *slot = Some(Candidate {
            key,
            value: Arc::new(value),
            metadata,
        });
        Ok(())
    }

    pub(crate) fn begin(
        &self,
        key: CandidateKey,
        version: String,
        value: T,
    ) -> Result<InstallAttempt<'_, T>> {
        let mut slot = self.lock()?;
        if slot.as_ref().is_some_and(|c| {
            matches!(
                c.metadata.state,
                RecoveryState::Failed | RecoveryState::Completed
            )
        }) {
            // An explicit terminal outcome releases the original transaction.
            *slot = None;
        }
        if let Some(candidate) = slot.as_mut() {
            if candidate.key != key {
                return Err(Error::WindowsInstallBusy);
            }
            return self.claim(candidate);
        }
        let candidate = Candidate {
            key,
            value: Arc::new(value),
            metadata: PendingInstall {
                candidate_id: uuid::Uuid::new_v4().to_string(),
                version,
                state: RecoveryState::Retryable,
                phase: "staging",
                message: None,
            },
        };
        *slot = Some(candidate);
        self.claim(slot.as_mut().unwrap())
    }

    /// The journal has proven explicit OS refusal, not worker disappearance.
    /// Caller must reverify the original signed candidate before replacing the
    /// blocked in-memory transaction. Running attempts can never be replaced.
    #[cfg(windows)]
    pub(crate) fn restore_refused(
        &self,
        key: CandidateKey,
        metadata: PendingInstall,
        value: T,
    ) -> Result<()> {
        let mut slot = self.lock()?;
        let candidate = slot.as_ref().ok_or(Error::WindowsInstallRecoveryMissing)?;
        if candidate.metadata.state != RecoveryState::Blocked
            || candidate.metadata.candidate_id != metadata.candidate_id
            || candidate.metadata.version != metadata.version
            || candidate.key.digest != key.digest
            || metadata.state != RecoveryState::Retryable
            || metadata.phase != "cleanup"
        {
            return Err(Error::WindowsInstallBusy);
        }
        *slot = Some(Candidate {
            key,
            metadata,
            value: Arc::new(value),
        });
        Ok(())
    }

    pub(crate) fn resume(&self, id: &str) -> Result<InstallAttempt<'_, T>> {
        let mut slot = self.lock()?;
        let candidate = slot
            .as_mut()
            .filter(|c| c.metadata.candidate_id == id)
            .ok_or(Error::WindowsInstallRecoveryMissing)?;
        self.claim(candidate)
    }

    fn claim<'a>(&'a self, candidate: &mut Candidate<T>) -> Result<InstallAttempt<'a, T>> {
        match candidate.metadata.state {
            RecoveryState::Running => return Err(Error::WindowsInstallBusy),
            RecoveryState::Failed | RecoveryState::Completed => {
                return Err(Error::WindowsInstallRecoveryMissing)
            }
            RecoveryState::Blocked => {
                return Err(Error::WindowsInstallPending {
                    phase: candidate.metadata.phase,
                    message: candidate.metadata.message.clone().unwrap_or_default(),
                })
            }
            RecoveryState::Retryable => {}
        }
        candidate.metadata.state = RecoveryState::Running;
        candidate.metadata.message = None;
        Ok(InstallAttempt {
            recovery: self,
            id: candidate.metadata.candidate_id.clone(),
            value: candidate.value.clone(),
            finished: false,
        })
    }
}

#[cfg(any(windows, test))]
pub(crate) struct InstallAttempt<'a, T> {
    recovery: &'a InstallRecovery<T>,
    id: String,
    pub value: Arc<T>,
    finished: bool,
}

#[cfg(any(windows, test))]
impl<T> InstallAttempt<'_, T> {
    #[cfg(windows)]
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn phase(&self, phase: &'static str) -> std::result::Result<(), String> {
        let mut slot = self.recovery.lock().map_err(|e| e.to_string())?;
        let candidate = slot
            .as_mut()
            .filter(|c| c.metadata.candidate_id == self.id)
            .ok_or_else(|| "Original install recovery owner is missing".to_string())?;
        candidate.metadata.phase = phase;
        Ok(())
    }

    pub(crate) fn finish(mut self, result: Result<()>, prepared: bool) -> Result<()> {
        // The production Windows exit never returns. A returning path cannot
        // attest installation and must not release the original candidate.
        let error = result
            .err()
            .unwrap_or_else(|| Error::WindowsInstallPending {
                phase: "exit",
                message: "Installer handoff returned without process exit".into(),
            });
        let mut slot = self.recovery.lock()?;
        let candidate = slot
            .as_mut()
            .filter(|c| c.metadata.candidate_id == self.id)
            .ok_or(Error::WindowsInstallRecoveryMissing)?;
        if !prepared && !matches!(error, Error::WindowsInstallPending { .. }) {
            // Keep an explicit terminal receipt for UI waiters. Unlike missing
            // metadata, this proves cleanup/launch never began and a new check
            // is safe. begin() may replace only this terminal failed candidate.
            candidate.metadata.state = RecoveryState::Failed;
            candidate.metadata.phase = "staging";
            candidate.metadata.message = Some(error.to_string());
        } else {
            let phase = match &error {
                Error::WindowsInstallPending { phase, .. } => *phase,
                _ => "task",
            };
            candidate.metadata.state = match phase {
                "cleanup" | "launch" | "exit" if prepared => RecoveryState::Retryable,
                _ => RecoveryState::Blocked,
            };
            candidate.metadata.phase = phase;
            candidate.metadata.message = Some(error.to_string());
        }
        self.finished = true;
        Err(error)
    }
}

#[cfg(any(windows, test))]
impl<T> Drop for InstallAttempt<'_, T> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Ok(mut slot) = self.recovery.lock() {
            if let Some(candidate) = slot.as_mut().filter(|c| c.metadata.candidate_id == self.id) {
                candidate.metadata.state = RecoveryState::Blocked;
                candidate.metadata.phase = "task";
                candidate.metadata.message = Some(
                    "Install owner stopped without a verified outcome; replay is blocked".into(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> CandidateKey {
        CandidateKey {
            transaction: 12,
            digest: [7; 32],
        }
    }
    fn pending(phase: &'static str) -> Result<()> {
        Err(Error::WindowsInstallPending {
            phase,
            message: "test refusal".into(),
        })
    }

    #[test]
    #[cfg(windows)]
    fn completion_catalog_cas_preserves_owner_until_durable_publication() {
        let catalog = InstallRecovery::new();
        let attempt = catalog.begin(key(), "1".into(), ()).unwrap();
        let id = catalog.snapshot().unwrap().unwrap().candidate_id;
        assert!(catalog
            .retire_verified(&id, || panic!("running owner cannot publish"))
            .is_err());
        drop(attempt);
        assert!(catalog
            .retire_verified("stale-id", || panic!("stale owner cannot publish"))
            .is_err());
        assert!(catalog
            .retire_verified(&id, || pending("durable-publication"))
            .is_err());
        let original = catalog.snapshot().unwrap().unwrap();
        assert_eq!(original.candidate_id, id);
        assert_eq!(original.state, RecoveryState::Blocked);
        assert!(catalog.begin(key(), "2".into(), ()).is_err());
        catalog.retire_verified(&id, || Ok(())).unwrap();
        assert_eq!(
            catalog.snapshot().unwrap().unwrap().state,
            RecoveryState::Completed
        );
        assert!(catalog
            .retire_verified(&id, || panic!("no duplicate publication"))
            .is_err());
        let next = catalog.begin(key(), "2".into(), ()).unwrap();
        assert_ne!(next.id, id);
    }

    #[test]
    fn resource_drop_retains_exact_payload_and_nonce() {
        let catalog = InstallRecovery::new();
        let payload = Arc::new("original signed candidate");
        let attempt = catalog
            .begin(key(), "1.2.3".into(), payload.clone())
            .unwrap();
        drop(payload);
        let original = catalog.snapshot().unwrap().unwrap();
        attempt.finish(pending("cleanup"), true).unwrap_err();
        let resumed = catalog.resume(&original.candidate_id).unwrap();
        assert_eq!(**resumed.value, "original signed candidate");
        assert_eq!(
            catalog.snapshot().unwrap().unwrap().candidate_id,
            original.candidate_id
        );
        resumed.finish(pending("launch"), true).unwrap_err();
    }

    #[test]
    fn snapshots_remain_available_while_work_is_blocked_and_replay_is_rejected() {
        let catalog = Arc::new(InstallRecovery::new());
        let (started, waiting) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let owner = catalog.clone();
        let worker = std::thread::spawn(move || {
            let attempt = owner.begin(key(), "1".into(), ()).unwrap();
            attempt.phase("cleanup").unwrap();
            started.send(()).unwrap();
            released.recv().unwrap();
            attempt.finish(pending("cleanup"), true).unwrap_err();
        });
        waiting
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let snapshot = catalog.snapshot().unwrap().unwrap();
        assert_eq!(snapshot.state, RecoveryState::Running);
        assert_eq!(snapshot.phase, "cleanup");
        assert!(matches!(
            catalog.resume(&snapshot.candidate_id),
            Err(Error::WindowsInstallBusy)
        ));
        assert!(matches!(
            catalog.begin(key(), "2".into(), ()),
            Err(Error::WindowsInstallBusy)
        ));
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(
            catalog.snapshot().unwrap().unwrap().state,
            RecoveryState::Retryable
        );
    }

    #[test]
    fn stale_identity_and_new_candidate_cannot_replace_prepared_owner() {
        let catalog = InstallRecovery::new();
        catalog
            .begin(key(), "1".into(), ())
            .unwrap()
            .finish(pending("cleanup"), true)
            .unwrap_err();
        assert!(matches!(
            catalog.resume("wrong"),
            Err(Error::WindowsInstallRecoveryMissing)
        ));
        let mut other = key();
        other.transaction += 1;
        assert!(matches!(
            catalog.begin(other, "2".into(), ()),
            Err(Error::WindowsInstallBusy)
        ));
        other = key();
        other.digest = [8; 32];
        assert!(matches!(
            catalog.begin(other, "1".into(), ()),
            Err(Error::WindowsInstallBusy)
        ));
    }

    #[test]
    fn unknown_outcome_and_abandoned_owner_are_quarantined() {
        for phase in ["launch_unknown", "task"] {
            let catalog = InstallRecovery::new();
            catalog
                .begin(key(), "1".into(), ())
                .unwrap()
                .finish(pending(phase), true)
                .unwrap_err();
            let snapshot = catalog.snapshot().unwrap().unwrap();
            assert_eq!(snapshot.state, RecoveryState::Blocked);
            assert!(catalog.resume(&snapshot.candidate_id).is_err());
            assert!(catalog.begin(key(), "1".into(), ()).is_err());
        }
        let catalog = InstallRecovery::new();
        drop(catalog.begin(key(), "1".into(), ()).unwrap());
        let snapshot = catalog.snapshot().unwrap().unwrap();
        assert_eq!(snapshot.state, RecoveryState::Blocked);
        assert!(catalog.resume(&snapshot.candidate_id).is_err());
    }

    #[test]
    fn unprepared_format_failure_releases_but_returning_exit_does_not() {
        let catalog = InstallRecovery::new();
        catalog
            .begin(key(), "1".into(), ())
            .unwrap()
            .finish(Err(Error::InvalidUpdaterFormat), false)
            .unwrap_err();
        let failed = catalog.snapshot().unwrap().unwrap();
        assert_eq!(failed.state, RecoveryState::Failed);
        assert!(matches!(
            catalog.resume(&failed.candidate_id),
            Err(Error::WindowsInstallRecoveryMissing)
        ));
        catalog
            .begin(key(), "1".into(), ())
            .unwrap()
            .finish(Ok(()), true)
            .unwrap_err();
        let snapshot = catalog.snapshot().unwrap().unwrap();
        assert_eq!(snapshot.state, RecoveryState::Retryable);
        assert_eq!(snapshot.phase, "exit");
    }
}
