use super::super::windows_journal::{self as journal, Binding, ProcessIdentity, Record};
use super::process_witness::{Observation, ProcessWitness};
use super::verified_artifact::{installer_bytes, windows_installer};
use super::*;
use crate::install_recovery::RecoveryState;
use crate::install_transaction::InstallTransaction;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::AsRawHandle;
use std::sync::Mutex;

pub(crate) struct WindowsRecovery {
    root: PathBuf,
    // Only initialization does I/O under this lock. Catalog polling never
    // holds the cleanup/launch lock or grants recovery based on elapsed time.
    loaded: Mutex<Option<Arc<Journal>>>,
    witness: Mutex<Option<ProcessWitness>>,
    pub(super) catalog: InstallRecovery<RetainedInstall>,
}

impl WindowsRecovery {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            loaded: Mutex::new(None),
            witness: Mutex::new(None),
            catalog: InstallRecovery::new(),
        }
    }

    fn observe_process(&self, identity: &ProcessIdentity) -> Result<Observation> {
        let mut witness = self.witness.lock().map_err(journal::pending)?;
        // The authenticated active journal, not this optional handle cache,
        // owns process identity. A retired candidate must not poison the next
        // update's observation. Every new handle still verifies creation time.
        if witness.as_ref().is_some_and(|w| w.identity() != identity) {
            *witness = None;
        }
        if let Some(witness) = witness.as_ref() {
            return Ok(witness.observe());
        }
        match ProcessWitness::attach(identity) {
            Ok(attached) => {
                let observation = attached.observe();
                *witness = Some(attached);
                Ok(observation)
            }
            Err(observation) => Ok(observation),
        }
    }

    fn terminal_snapshot(&self, original: &PendingInstall) -> Result<Option<PendingInstall>> {
        match self.catalog.snapshot()? {
            Some(done)
                if done.candidate_id == original.candidate_id
                    && done.state == RecoveryState::Completed =>
            {
                Ok(Some(done))
            }
            _ => Err(journal::pending("Original installer journal is missing")),
        }
    }

    pub(super) fn initialize(&self, trusted: &Update) -> Result<Arc<Journal>> {
        let mut loaded = self.loaded.lock().map_err(journal::pending)?;
        if let Some(journal) = loaded.as_ref() {
            return Ok(journal.clone());
        }
        let journal = Arc::new(Journal::open(&self.root)?);
        if let Some(record) = journal.reconcile_dispatch()? {
            self.restore_record(trusted, &journal, record, false)?;
        }
        *loaded = Some(journal.clone());
        Ok(journal)
    }

    fn restore_record(
        &self,
        trusted: &Update,
        journal: &Arc<Journal>,
        record: Record,
        refused: bool,
    ) -> Result<()> {
        let mut update = trusted.clone();
        update.windows_recovery = None;
        update.version = record.version.clone();
        update.signature = record.signature.clone();
        update.target = record.target.clone();
        update.current_exe_args = record
            .current_args
            .iter()
            .map(|v| OsString::from_wide(v))
            .collect();
        let metadata = match record.phase {
            Phase::Prepared => {
                // Do not deserialize an App handle, callback, pubkey, install
                // path or executable from disk. Rebind only to this binary's
                // trusted App context and reverify the original signed bytes.
                if trusted.binding()? != record.binding
                    || trusted.current_version != record.source_version
                {
                    return Err(journal::pending(
                        "Prepared update belongs to a different App binary/configuration",
                    ));
                }
                let (lease, installer) =
                    verified_artifact::verify_retained(journal, &record, &trusted.config.pubkey)?;
                let launch = launch_plan::LaunchPlan::new(
                    &installer,
                    trusted.config.install_mode(),
                    &update.current_exe_args,
                    &trusted.installer_args,
                )
                .map_err(journal::pending)?;
                let launch = completion::bind_plan(
                    launch,
                    journal.completion_directory(),
                    &record,
                    &trusted.config.pubkey,
                )?;
                update.windows_install = Arc::new(InstallTransaction::restored(
                    record.payload_digest,
                    PreparedWindowsInstaller {
                        _read_lease: lease,
                        _installer: installer,
                        launch,
                    },
                ));
                PendingInstall {
                    candidate_id: record.candidate_id.clone(),
                    version: record.version.clone(),
                    state: RecoveryState::Retryable,
                    phase: "cleanup",
                    message: Some(
                        "Original signed candidate recovered; App cleanup must run again".into(),
                    ),
                }
            }
            Phase::LaunchIntent | Phase::Accepted => {
                // PID disappearance/version strings are not installation
                // receipts. Preserve the original nonce and do not replay.
                PendingInstall { candidate_id: record.candidate_id.clone(), version: record.version.clone(), state: RecoveryState::Blocked,
                        phase: "launch_unknown", message: Some("Original installer handoff requires verified completion; another launch is blocked".into()) }
            }
        };
        let key = CandidateKey {
            transaction: Arc::as_ptr(&update.windows_install) as usize,
            digest: record.payload_digest,
        };
        let retained = RetainedInstall {
            update,
            identity: record.payload_digest,
            journal: Some(journal.clone()),
        };
        if refused {
            self.catalog.restore_refused(key, metadata, retained)
        } else {
            self.catalog.restore(key, metadata, retained)
        }
    }

    pub(super) fn pending(&self, trusted: &Update) -> Result<Option<PendingInstall>> {
        let journal = self.initialize(trusted)?;
        let Some(mut pending) = self.catalog.snapshot()? else {
            return Ok(None);
        };
        // Never replace the live transaction's cleanup/launch state. A recovered
        // blocked owner cannot become retryable from elapsed time or exit alone.
        // Only authenticated refusal or verified completion may retire its block.
        if pending.state != RecoveryState::Blocked {
            return Ok(Some(pending));
        }
        let record = match journal.reconcile_dispatch()? {
            Some(record) => record,
            None => return self.terminal_snapshot(&pending),
        };
        if record.candidate_id != pending.candidate_id {
            return Err(journal::pending("Installer observation owner changed"));
        }
        if record.phase == Phase::Prepared {
            // Only authenticated explicit refusal can undo delegated intent.
            // Reverify the signed bytes and compiled policy, retain the nonce,
            // and force fresh cleanup; a timeout alone never reaches this path.
            self.restore_record(trusted, &journal, record, true)?;
            return self.catalog.snapshot();
        }
        if record.phase != Phase::Accepted {
            return Ok(Some(pending));
        }
        let independent_exit = if record.witness.is_some() {
            journal.witness_store()?.exit(&record)?
        } else {
            None
        };
        if let (Some(persisted), Some(independent)) = (&record.process_exit, &independent_exit) {
            if persisted != independent {
                return Err(journal::pending(
                    "Conflicting durable and independent installer exit evidence",
                ));
            }
        }
        let observation = if let Some(exit) = record.process_exit.or(independent_exit) {
            Observation::Exited(exit)
        } else if let Some(identity) = record.process {
            self.observe_process(&identity)?
        } else {
            return Ok(Some(pending));
        };
        let (phase, mut message) = match observation {
            Observation::Running => ("installer_running", "Exact accepted installer process is still running; another launch is blocked".into()),
            Observation::Exited(exit) => {
                journal.record_process_exit(&record.candidate_id, &exit)?;
                ("installer_exited", format!("Exact accepted installer process exited with code {}; installed files and completion/rollback still require verification; replay is blocked", exit.code))
            }
            Observation::Missing => ("installer_unobserved", "Accepted installer PID is unavailable; disappearance is not a completion receipt; replay is blocked".into()),
            Observation::IdentityMismatch => ("installer_unobserved", "Accepted installer PID no longer matches its recorded creation identity; replay is blocked".into()),
            Observation::Unavailable(error) => ("installer_unobserved", format!("Exact accepted installer process cannot be observed: {error}; replay is blocked")),
        };
        if record.inventory.is_some() && phase == "installer_exited" {
            // File identity is an additional fact, never permission to replay
            // or proof that the installer/service/rollback has completed.
            let current = match journal.snapshot()? {
                Some(record) => record,
                None => return self.terminal_snapshot(&pending),
            };
            match super::failure::observe(trusted, &current, journal.completion_directory()) {
                Ok(Some(outcome)) => {
                    pending.phase = outcome.phase();
                    pending.message = Some(format!("Publisher-declared native installer callback reports {}; rollback/repair remains unverified and replay is blocked; {message}", pending.phase));
                    return Ok(Some(pending));
                }
                Err(error) => {
                    message.push_str(&format!("; failure callback not verified: {error}"))
                }
                Ok(None) => (),
            }
            match completion::reconcile(
                trusted,
                &current,
                journal.completion_directory(),
                |proof| {
                    self.catalog
                        .retire_verified(&current.candidate_id, || journal.retire_completed(proof))
                },
            ) {
                Ok(Some(())) => return self.terminal_snapshot(&pending),
                Err(error) => message.push_str(&format!("; completion not verified: {error}")),
                Ok(None) => (),
            }
            match inventory::observe_installed(trusted, &current) {
                Ok(Some(evidence)) => {
                    message = format!("Signed installed inventory matches {} files ({} bytes); completion/rollback reconciliation is still required; replay is blocked; {message}", evidence.files, evidence.bytes);
                }
                Err(error) => {
                    message.push_str(&format!("; installed inventory not verified: {error}"))
                }
                Ok(None) => (),
            }
        }
        pending.phase = phase;
        pending.message = Some(message);
        Ok(Some(pending))
    }

    pub(super) fn pending_for(
        &self,
        trusted: &Update,
        candidate_id: Option<&str>,
    ) -> Result<Option<PendingInstall>> {
        let pending = self.pending(trusted)?;
        if let Some(id) = candidate_id {
            if !pending
                .as_ref()
                .is_some_and(|current| current.candidate_id == id)
            {
                let store = self.initialize(trusted)?;
                if let Some(completed) = completion::historical(trusted, &store, id)? {
                    return Ok(Some(completed));
                }
            }
        }
        Ok(pending)
    }
}

impl Updater {
    fn recovery_template(&self) -> Update {
        Update {
            windows_recovery: None,
            windows_install: Default::default(),
            run_on_main_thread: self.run_on_main_thread.clone(),
            config: self.config.clone(),
            on_before_exit: self.on_before_exit.clone(),
            before_windows_install: self.before_windows_install.clone(),
            body: None,
            current_version: self.current_version.to_string(),
            version: self.current_version.to_string(),
            date: None,
            target: self.target.clone().unwrap_or_else(|| "windows".into()),
            download_url: Url::parse("https://invalid.invalid/recovered-offline-candidate")
                .expect("static URL"),
            signature: String::new(),
            raw_json: serde_json::Value::Null,
            timeout: None,
            proxy: None,
            no_proxy: true,
            headers: HeaderMap::new(),
            extract_path: self.extract_path.clone(),
            app_name: self.app_name.clone(),
            installer_args: self.installer_args.clone(),
            current_exe_args: self.current_exe_args.clone(),
            configure_client: None,
        }
    }

    pub(crate) fn pending_windows_install(
        &self,
        candidate_id: Option<&str>,
    ) -> Result<Option<PendingInstall>> {
        self.windows_recovery
            .pending_for(&self.recovery_template(), candidate_id)
    }

    pub(crate) async fn pending_windows_install_async(&self) -> Result<Option<PendingInstall>> {
        let owner = self.windows_recovery.clone();
        let trusted = self.recovery_template();
        tauri::async_runtime::spawn_blocking(move || owner.pending(&trusted))
            .await
            .map_err(journal::pending)?
    }

    pub(crate) fn resume_windows_install(&self, id: &str) -> Result<()> {
        self.windows_recovery.pending(&self.recovery_template())?;
        Update::run_windows_attempt(self.windows_recovery.catalog.resume(id)?, None)
    }
}

impl Update {
    fn binding(&self) -> Result<Binding> {
        let path = std::env::current_exe().map_err(journal::pending)?;
        let file = journal::protected_read(&path).map_err(journal::pending)?;
        let mut hash = Sha256::new();
        std::io::copy(&mut &file, &mut hash).map_err(journal::pending)?;
        Ok(Binding {
            executable: path.as_os_str().encode_wide().collect(),
            executable_digest: hash.finalize().into(),
            app_name: self.app_name.clone(),
            key_digest: journal::digest(self.config.pubkey.as_bytes()),
            install_mode: self.config.install_mode().to_string(),
            installer_args: self
                .installer_args
                .iter()
                .map(|s| s.encode_wide().collect())
                .collect(),
        })
    }

    pub(super) fn stage_durable(
        &self,
        bytes: &[u8],
        journal: &Arc<Journal>,
        id: &str,
    ) -> Result<PreparedWindowsInstaller> {
        let (kind, entry, installer) = installer_bytes(bytes)?;
        let mut record = Record {
            candidate_id: id.into(),
            source_version: self.current_version.clone(),
            version: self.version.clone(),
            target: self.target.clone(),
            signature: self.signature.clone(),
            payload_digest: journal::digest(bytes),
            installer_digest: journal::digest(&installer),
            kind,
            archive_entry: entry,
            binding: self.binding()?,
            current_args: self
                .current_exe_args
                .iter()
                .map(|s| s.encode_wide().collect())
                .collect(),
            phase: Phase::Prepared,
            process: None,
            process_exit: None,
            witness: None,
            dispatcher: None,
            inventory: None,
        };
        record.inventory = inventory::from_update(self, &record)?;
        let executable = windows_installer(kind, journal.path(id, Some(kind))?);
        // Compile all launch data BEFORE journaling/cleanup, including limits.
        let launch = launch_plan::LaunchPlan::new(
            &executable,
            self.config.install_mode(),
            &self.current_exe_args,
            &self.installer_args,
        )
        .map_err(Error::WindowsInstallPreflight)?;
        let launch = completion::bind_plan(
            launch,
            journal.completion_directory(),
            &record,
            &self.config.pubkey,
        )?;
        let lease = journal.prepare(record, bytes, &installer)?;
        Ok(PreparedWindowsInstaller {
            _read_lease: lease,
            _installer: executable,
            launch,
        })
    }
}

#[cfg(test)]
mod inventory_regression {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn completion_next_process_replaces_only_cached_witness_not_creation_identity() {
        use super::super::process_witness::tests::OwnedChild;
        let temp = tempfile::tempdir().unwrap();
        let owner = WindowsRecovery::new(temp.path().to_owned());
        let mut first = OwnedChild::spawn(0);
        let first_id = first.identity();
        assert_eq!(
            owner.observe_process(&first_id).unwrap(),
            Observation::Running
        );
        first.finish();
        assert!(matches!(
            owner.observe_process(&first_id).unwrap(),
            Observation::Exited(_)
        ));
        let mut next = OwnedChild::spawn(0);
        let next_id = next.identity();
        let mut forged = next_id.clone();
        forged.created += 1;
        assert_eq!(
            owner.observe_process(&forged).unwrap(),
            Observation::IdentityMismatch
        );
        assert!(owner.witness.lock().unwrap().is_none());
        assert_eq!(
            owner.observe_process(&next_id).unwrap(),
            Observation::Running
        );
        assert_eq!(
            owner.witness.lock().unwrap().as_ref().unwrap().identity(),
            &next_id
        );
        next.finish();
        assert!(matches!(
            owner.observe_process(&next_id).unwrap(),
            Observation::Exited(_)
        ));
    }

    #[test]
    fn unauthenticated_install_inventory_is_rejected_before_cleanup_or_journal_publication() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/signed-inert-stage.json"
        ))
        .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut update = super::super::tests::refusing_update(&fixture, calls.clone());
        let temp = tempfile::tempdir().unwrap();
        let recovery = Arc::new(WindowsRecovery::new(temp.path().join("store")));
        update.windows_recovery = Some(recovery.clone());
        update.raw_json = serde_json::json!({
            "windows_install_inventory": {"document": "{}", "signature": "untrusted"}
        });
        assert!(update
            .install_windows_checked(fixture["payload"].as_str().unwrap().as_bytes())
            .is_err());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "untrusted inventory must not drain App services"
        );
        assert!(recovery
            .initialize(&update)
            .unwrap()
            .snapshot()
            .unwrap()
            .is_none());
        let failed = recovery.catalog.snapshot().unwrap().unwrap();
        assert_eq!(failed.state, RecoveryState::Failed);
        assert_eq!(failed.phase, "staging");
        assert_eq!(recovery.pending(&update).unwrap(), Some(failed.clone()));
        assert!(recovery.catalog.resume(&failed.candidate_id).is_err());

        // No installer was published or dispatched. A corrected manifest must
        // permit a fresh candidate, not require restarting a healthy App.
        update.raw_json = serde_json::Value::Null;
        let result =
            update.install_windows_checked(fixture["payload"].as_str().unwrap().as_bytes());
        assert!(
            matches!(
                result,
                Err(Error::WindowsInstallPending {
                    phase: "cleanup",
                    ..
                })
            ),
            "{result:?}"
        );
        let next = recovery.catalog.snapshot().unwrap().unwrap();
        assert_ne!(failed.candidate_id, next.candidate_id);
        assert_eq!(next.state, RecoveryState::Retryable);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

pub(super) fn process_identity(
    handle: &OwnedHandle,
) -> std::result::Result<ProcessIdentity, String> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetProcessId, GetProcessTimes},
    };
    let mut created: FILETIME = unsafe { std::mem::zeroed() };
    let mut exit = created;
    let mut kernel = created;
    let mut user = created;
    let pid = unsafe { GetProcessId(handle.as_raw_handle()) };
    if pid == 0
        || unsafe {
            GetProcessTimes(
                handle.as_raw_handle(),
                &mut created,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        } == 0
    {
        return Err(format!(
            "Accepted installer identity could not be recorded: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(ProcessIdentity {
        pid,
        created: ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64,
    })
}

/// Legacy observer custody regression harness. Production delegates the launch
/// itself to detached_dispatch before the OS call, not a later handle transfer.
#[cfg(test)]
pub(super) fn dispatch(
    journal: Option<&Journal>,
    id: &str,
    launch: impl FnOnce() -> std::result::Result<Option<OwnedHandle>, String>,
) -> std::result::Result<(), String> {
    // Preflight before any installer launch: an independent, byte-identical
    // copy cannot keep the installed App image mapped during replacement.
    let helper = journal
        .map(|j| super::detached_witness::PendingWitness::prepare(j, id))
        .transpose()
        .map_err(|e| e.to_string())?;
    if let Some(journal) = journal {
        journal
            .transition(id, Phase::Prepared, Phase::LaunchIntent, None)
            .map_err(|e| e.to_string())?;
    }
    match launch() {
        Ok(process) => {
            if let Some(journal) = journal {
                let identity = process.as_ref().map(process_identity).transpose()?;
                journal
                    .transition(id, Phase::LaunchIntent, Phase::Accepted, identity)
                    .map_err(|e| e.to_string())?;
                if let Some(helper) = helper {
                    let process = process.as_ref().ok_or_else(|| {
                        "OS accepted the installer without a process handle; replay is blocked"
                            .to_string()
                    })?;
                    helper
                        .handoff(journal, id, process)
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }
        Err(error) => {
            if let Some(journal) = journal {
                journal
                    .transition(id, Phase::LaunchIntent, Phase::Prepared, None)
                    .map_err(|e| e.to_string())?;
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::windows_journal::Kind;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/signed-inert-stage.json"
        ))
        .unwrap()
    }
    fn update(count: Arc<AtomicUsize>, owner: Option<Arc<WindowsRecovery>>) -> Update {
        let mut update = super::super::tests::refusing_update(&fixture(), count);
        update.windows_recovery = owner;
        update
    }
    fn stage(owner: Arc<WindowsRecovery>, count: Arc<AtomicUsize>) -> String {
        let update = update(count, Some(owner.clone()));
        let f = fixture();
        let result = update.install(f["payload"].as_str().unwrap().as_bytes());
        assert!(
            matches!(
                result,
                Err(Error::WindowsInstallPending {
                    phase: "cleanup",
                    ..
                })
            ),
            "{result:?}"
        );
        owner.catalog.snapshot().unwrap().unwrap().candidate_id
    }

    #[test]
    fn uncertain_journal_publication_never_becomes_terminal_staging_failure() {
        let root = tempfile::tempdir().unwrap();
        let owner = Arc::new(WindowsRecovery::new(root.path().join("recovery")));
        let count = Arc::new(AtomicUsize::new(0));
        let update = update(count.clone(), Some(owner.clone()));
        let journal = owner.initialize(&update).unwrap();
        assert!(journal.snapshot().unwrap().is_none());
        // Native FILE_SHARE_READ denies replacement of our private active
        // record. A failed durable publication poisons authority even though
        // its in-memory slot never acquired a prepared record.
        let lease = journal::protected_read(&owner.root.join("active.dpapi")).unwrap();
        let fixture = fixture();
        let bytes = fixture["payload"].as_str().unwrap().as_bytes();
        let result = update.install_windows_checked(bytes);
        assert!(
            matches!(result, Err(Error::WindowsInstallPending { .. })),
            "{result:?}"
        );
        assert!(journal.snapshot().is_err());
        assert!(!update.windows_install.has_prepared());
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let blocked = owner.catalog.snapshot().unwrap().unwrap();
        assert_eq!(blocked.state, RecoveryState::Blocked);
        assert!(owner.pending(&update).is_err());
        drop(lease);
        assert!(update.install_windows_checked(bytes).is_err());
        assert!(owner.catalog.resume(&blocked.candidate_id).is_err());
        assert_eq!(owner.catalog.snapshot().unwrap(), Some(blocked));
        assert!(journal.snapshot().is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn missing_prepared_recovery_is_not_an_initial_staging_rejection() {
        let root = tempfile::tempdir().unwrap();
        let owner = WindowsRecovery::new(root.path().join("recovery"));
        let count = Arc::new(AtomicUsize::new(0));
        let update = update(count.clone(), None);
        let journal = owner.initialize(&update).unwrap();
        let fixture = fixture();
        let identity = journal::digest(fixture["payload"].as_str().unwrap().as_bytes());
        let attempt = owner
            .catalog
            .begin(
                CandidateKey {
                    transaction: Arc::as_ptr(&update.windows_install) as usize,
                    digest: identity,
                },
                update.version.clone(),
                RetainedInstall {
                    update: update.clone(),
                    identity,
                    journal: Some(journal.clone()),
                },
            )
            .unwrap();
        let id = attempt.id().to_owned();
        let result = Update::run_windows_attempt(attempt, None);
        assert!(matches!(
            result,
            Err(Error::WindowsInstallPending { phase: "task", .. })
        ));
        assert!(journal.snapshot().unwrap().is_none());
        assert!(!update.windows_install.has_prepared());
        let blocked = owner.catalog.snapshot().unwrap().unwrap();
        assert_eq!(blocked.candidate_id, id);
        assert_eq!(blocked.state, RecoveryState::Blocked);
        assert!(owner.pending(&update).is_err());
        assert!(owner.catalog.resume(&id).is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn invalid_native_launch_data_never_drains_app_or_publishes_candidate() {
        for durable in [false, true] {
            for invalid in 0..4 {
                let root = tempfile::tempdir().unwrap();
                let owner = Arc::new(WindowsRecovery::new(root.path().join("recovery")));
                let count = Arc::new(AtomicUsize::new(0));
                let mut update = update(count.clone(), durable.then(|| owner.clone()));
                match invalid {
                    0 => update.current_exe_args = vec![OsString::from("bad\0argv0")],
                    1 => {
                        update.current_exe_args =
                            vec![OsString::from("app"), OsString::from("visible\0hidden")]
                    }
                    2 => update.installer_args = vec![OsString::from("/P\0/S")],
                    _ => update.installer_args = vec![OsString::from("🦀".repeat(16384))],
                }
                let fixture = fixture();
                let bytes = fixture["payload"].as_str().unwrap().as_bytes();
                let result = if durable {
                    update.install(bytes)
                } else {
                    update.install_windows_with_owner(bytes, &owner.catalog)
                };
                assert!(result.is_err(), "durable={durable}, invalid={invalid}");
                assert_eq!(count.load(Ordering::SeqCst), 0, "cleanup must not start");
                assert!(!update.windows_install.has_prepared());
                let failed = owner.catalog.snapshot().unwrap().unwrap();
                assert_eq!(failed.state, RecoveryState::Failed);
                assert_eq!(failed.phase, "staging");
                assert!(owner.catalog.resume(&failed.candidate_id).is_err());
                if durable {
                    assert!(owner
                        .loaded
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .snapshot()
                        .unwrap()
                        .is_none());
                }
                // No unknown dispatch exists: fixing data allows a fresh check,
                // not a resume/restage of a previously accepted installer.
                update.current_exe_args.clear();
                update.installer_args.clear();
                let result = if durable {
                    update.install(bytes)
                } else {
                    update.install_windows_with_owner(bytes, &owner.catalog)
                };
                assert!(matches!(
                    result,
                    Err(Error::WindowsInstallPending {
                        phase: "cleanup",
                        ..
                    })
                ));
                assert_eq!(count.load(Ordering::SeqCst), 1);
                let repaired = owner.catalog.snapshot().unwrap().unwrap();
                assert_eq!(repaired.state, RecoveryState::Retryable);
                assert_ne!(repaired.candidate_id, failed.candidate_id);
            }
        }
    }

    #[test]
    fn shared_artifact_verifier_rejects_key_signature_and_member_rebinding() {
        let root = tempfile::tempdir().unwrap();
        let owner = Arc::new(WindowsRecovery::new(root.path().join("recovery")));
        let count = Arc::new(AtomicUsize::new(0));
        let id = stage(owner.clone(), count.clone());
        let journal = owner.loaded.lock().unwrap().as_ref().unwrap().clone();
        let record = journal.snapshot().unwrap().unwrap();
        let fixture = fixture();
        let key = fixture["publicKey"].as_str().unwrap();
        let (lease, _) = verified_artifact::verify_retained(&journal, &record, key).unwrap();
        assert!(std::fs::write(journal.path(&id, Some(record.kind)).unwrap(), b"replace").is_err());
        drop(lease);
        assert!(verified_artifact::verify_retained(&journal, &record, "").is_err());
        assert!(
            verified_artifact::verify_retained(&journal, &record, "journal-provided-key").is_err()
        );
        for field in 0..6 {
            let mut altered = record.clone();
            match field {
                0 => altered.payload_digest[0] ^= 1,
                1 => altered.installer_digest[0] ^= 1,
                2 => altered.archive_entry = Some("different.exe".into()),
                3 => altered.kind = Kind::Msi,
                4 => altered.signature = "not-the-checked-signature".into(),
                _ => altered.binding.key_digest[0] ^= 1,
            }
            assert!(
                verified_artifact::verify_retained(&journal, &altered, key).is_err(),
                "field={field}"
            );
        }
        assert_eq!(journal.snapshot().unwrap().unwrap(), record);
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "verification cannot drain again"
        );
    }

    #[test]
    fn recovered_os_exit_is_durable_evidence_not_installation_or_retry_permission() {
        use super::super::process_witness::tests::OwnedChild;
        for code in [0, 259, 1603] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("recovery");
            let count = Arc::new(AtomicUsize::new(0));
            let first = Arc::new(WindowsRecovery::new(path.clone()));
            let id = stage(first.clone(), count.clone());
            let mut child = OwnedChild::spawn(code);
            let identity = child.identity();
            let journal = first.loaded.lock().unwrap().as_ref().unwrap().clone();
            journal
                .transition(&id, Phase::Prepared, Phase::LaunchIntent, None)
                .unwrap();
            journal
                .transition(
                    &id,
                    Phase::LaunchIntent,
                    Phase::Accepted,
                    Some(identity.clone()),
                )
                .unwrap();
            drop(journal);
            drop(first);
            let recovered = WindowsRecovery::new(path.clone());
            let trusted = update(count.clone(), None);
            let running = recovered.pending(&trusted).unwrap().unwrap();
            assert_eq!(running.candidate_id, id);
            assert_eq!(running.state, RecoveryState::Blocked);
            assert_eq!(running.phase, "installer_running");
            child.finish();
            let exited = recovered.pending(&trusted).unwrap().unwrap();
            assert_eq!(exited.state, RecoveryState::Blocked);
            assert_eq!(exited.phase, "installer_exited");
            assert!(exited.message.unwrap().contains(&format!("code {code};")));
            assert!(recovered.catalog.resume(&id).is_err());
            let record = recovered
                .loaded
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .snapshot()
                .unwrap()
                .unwrap();
            assert_eq!(record.phase, Phase::Accepted);
            assert_eq!(record.process_exit.as_ref().unwrap().identity, identity);
            assert_eq!(record.process_exit.as_ref().unwrap().code, code as u32);
            let bytes = std::fs::read(path.join("active.dpapi")).unwrap();
            recovered.pending(&trusted).unwrap();
            assert_eq!(std::fs::read(path.join("active.dpapi")).unwrap(), bytes);
            drop(recovered);
            let restarted = WindowsRecovery::new(path);
            let persisted = restarted.pending(&trusted).unwrap().unwrap();
            assert_eq!(persisted.phase, "installer_exited");
            assert_eq!(persisted.state, RecoveryState::Blocked);
            assert_eq!(persisted.candidate_id, id);
            assert!(
                restarted.witness.lock().unwrap().is_none(),
                "persisted evidence needs no PID lookup"
            );
            assert!(restarted.catalog.resume(&id).is_err());
            assert_eq!(
                count.load(Ordering::SeqCst),
                1,
                "observation never reruns cleanup or install"
            );
        }
    }

    #[test]
    fn independent_receipt_reconciles_through_production_pending_after_handle_owners_drop() {
        use super::super::process_witness::tests::OwnedChild;
        use std::time::{Duration, Instant};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        let count = Arc::new(AtomicUsize::new(0));
        let first = Arc::new(WindowsRecovery::new(path.clone()));
        let id = stage(first.clone(), count.clone());
        let journal = first.loaded.lock().unwrap().as_ref().unwrap().clone();
        let mut child = OwnedChild::spawn(0);
        dispatch(Some(&journal), &id, || Ok(Some(child.handle()))).unwrap();
        let record = journal.snapshot().unwrap().unwrap();
        let helper = ProcessWitness::attach(&record.witness.as_ref().unwrap().helper)
            .unwrap_or_else(|e| panic!("{e:?}"));
        // The in-process catalog still thinks cleanup failed. The durable
        // Accepted fence must win over a retryable transaction error.
        let result = Update::run_windows_attempt(first.catalog.resume(&id).unwrap(), None);
        assert!(matches!(
            result,
            Err(Error::WindowsInstallPending {
                phase: "launch_unknown",
                ..
            })
        ));
        assert_eq!(
            first.catalog.snapshot().unwrap().unwrap().state,
            RecoveryState::Blocked
        );
        assert!(first.catalog.resume(&id).is_err());
        drop(journal);
        drop(first);
        child.finish();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Observation::Exited(exit) = helper.observe() {
                assert_eq!(exit.code, 0);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(helper);
        let recovered = WindowsRecovery::new(path);
        let trusted = update(count.clone(), None);
        let pending = recovered.pending(&trusted).unwrap().unwrap();
        assert_eq!(pending.candidate_id, id);
        assert_eq!(pending.phase, "installer_exited");
        assert_eq!(pending.state, RecoveryState::Blocked);
        assert!(pending.message.unwrap().contains("code 0;"));
        assert!(recovered.catalog.resume(&id).is_err());
        let durable = recovered
            .loaded
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .unwrap();
        assert_eq!(durable.process_exit.unwrap().code, 0);
    }

    #[test]
    fn exit_publication_failure_does_not_release_or_replay_the_original_candidate() {
        use super::super::process_witness::tests::OwnedChild;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        let first = Arc::new(WindowsRecovery::new(path.clone()));
        let id = stage(first.clone(), Arc::new(AtomicUsize::new(0)));
        let mut child = OwnedChild::spawn(0);
        let journal = first.loaded.lock().unwrap().as_ref().unwrap().clone();
        journal
            .transition(&id, Phase::Prepared, Phase::LaunchIntent, None)
            .unwrap();
        journal
            .transition(
                &id,
                Phase::LaunchIntent,
                Phase::Accepted,
                Some(child.identity()),
            )
            .unwrap();
        drop(journal);
        drop(first);
        let recovered = WindowsRecovery::new(path.clone());
        let trusted = update(Arc::new(AtomicUsize::new(0)), None);
        assert_eq!(
            recovered.pending(&trusted).unwrap().unwrap().phase,
            "installer_running"
        );
        child.finish();
        let publication_fence = journal::protected_read(&path.join("active.dpapi")).unwrap();
        assert!(recovered.pending(&trusted).is_err());
        assert!(
            recovered.pending(&trusted).is_err(),
            "old in-memory state must not hide an uncertain write"
        );
        assert!(recovered.catalog.resume(&id).is_err());
        drop(publication_fence);
        drop(recovered);
        let restarted = WindowsRecovery::new(path);
        let journal = restarted.initialize(&trusted).unwrap();
        let old = journal.snapshot().unwrap().unwrap();
        assert_eq!(old.phase, Phase::Accepted);
        assert!(old.process_exit.is_none());
        assert_eq!(
            restarted.pending(&trusted).unwrap().unwrap().state,
            RecoveryState::Blocked
        );
        assert!(restarted.catalog.resume(&id).is_err());
    }

    #[test]
    fn dropping_every_old_resource_restores_same_candidate_with_fresh_guard_and_no_restage() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        let first = Arc::new(WindowsRecovery::new(path.clone()));
        let old_count = Arc::new(AtomicUsize::new(0));
        let id = stage(first.clone(), old_count.clone());
        let record = first
            .loaded
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .unwrap();
        let installer = first
            .loaded
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .path(&id, Some(record.kind))
            .unwrap();
        let before = std::fs::metadata(&installer).unwrap().modified().unwrap();
        drop(first);
        let fresh = Arc::new(WindowsRecovery::new(path));
        let count = Arc::new(AtomicUsize::new(0));
        let trusted = update(count.clone(), None);
        fresh.initialize(&trusted).unwrap();
        assert_eq!(fresh.catalog.snapshot().unwrap().unwrap().candidate_id, id);
        assert_eq!(
            std::fs::metadata(&installer).unwrap().modified().unwrap(),
            before
        );
        assert!(std::fs::write(&installer, b"tamper").is_err());
        assert!(matches!(
            Update::run_windows_attempt(fresh.catalog.resume(&id).unwrap(), None),
            Err(Error::WindowsInstallPending {
                phase: "cleanup",
                ..
            })
        ));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(old_count.load(Ordering::SeqCst), 1);
        assert!(fresh.catalog.resume("different-candidate").is_err());
        let other = update(count.clone(), Some(fresh.clone()));
        assert!(matches!(
            other.install(fixture()["payload"].as_str().unwrap().as_bytes()),
            Err(Error::WindowsInstallBusy)
        ));
    }

    #[test]
    fn altered_payload_installer_and_trusted_configuration_never_reach_cleanup() {
        for mode in ["payload", "installer", "key", "args", "source-version"] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("recovery");
            let first = Arc::new(WindowsRecovery::new(path.clone()));
            let id = stage(first.clone(), Arc::new(AtomicUsize::new(0)));
            let journal = first.loaded.lock().unwrap().as_ref().unwrap().clone();
            let payload = journal.path(&id, None).unwrap();
            let installer = journal.path(&id, Some(Kind::Nsis)).unwrap();
            drop(journal);
            drop(first);
            let count = Arc::new(AtomicUsize::new(0));
            let mut trusted = update(count.clone(), None);
            match mode {
                "payload" => std::fs::write(payload, b"MZ not the original signature").unwrap(),
                "installer" => std::fs::write(installer, b"MZ changed installer").unwrap(),
                "key" => trusted.config.pubkey.push('x'),
                "args" => trusted.installer_args.push("/untrusted".into()),
                "source-version" => trusted.current_version = "8.0.0".into(),
                _ => unreachable!(),
            }
            let fresh = WindowsRecovery::new(path);
            assert!(fresh.initialize(&trusted).is_err(), "{mode}");
            assert_eq!(count.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn crashed_launch_intent_and_accepted_handoff_never_authorize_replay() {
        for accepted in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("recovery");
            let first = Arc::new(WindowsRecovery::new(path.clone()));
            let id = stage(first.clone(), Arc::new(AtomicUsize::new(0)));
            let journal = first.loaded.lock().unwrap().as_ref().unwrap().clone();
            journal
                .transition(&id, Phase::Prepared, Phase::LaunchIntent, None)
                .unwrap();
            if accepted {
                journal
                    .transition(&id, Phase::LaunchIntent, Phase::Accepted, None)
                    .unwrap();
            }
            drop(journal);
            drop(first);
            let count = Arc::new(AtomicUsize::new(0));
            let trusted = update(count.clone(), None);
            let recovered = WindowsRecovery::new(path);
            recovered.initialize(&trusted).unwrap();
            let pending = recovered.catalog.snapshot().unwrap().unwrap();
            assert_eq!(pending.state, RecoveryState::Blocked);
            assert_eq!(pending.candidate_id, id);
            assert!(recovered.catalog.resume(&id).is_err());
            assert_eq!(count.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    #[ignore = "parent runs exact owned child and verifies its durable checkpoint"]
    fn owned_crash_stage_child() {
        let root = PathBuf::from(std::env::var_os("GROK_OWNED_UPDATER_CRASH_ROOT").unwrap());
        assert_eq!(
            std::fs::read(root.join("purpose")).unwrap(),
            b"inert signed staging and exact owned process kill only"
        );
        let owner = Arc::new(WindowsRecovery::new(root.join("recovery")));
        let id = stage(owner.clone(), Arc::new(AtomicUsize::new(0)));
        if std::env::var("GROK_OWNED_UPDATER_CRASH_PHASE").as_deref() == Ok("intent") {
            owner
                .loaded
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .transition(&id, Phase::Prepared, Phase::LaunchIntent, None)
                .unwrap();
        }
        let mut marker = File::options()
            .write(true)
            .create_new(true)
            .open(root.join("ready.tmp"))
            .unwrap();
        use std::io::Write;
        marker.write_all(id.as_bytes()).unwrap();
        marker.sync_all().unwrap();
        drop(marker);
        std::fs::rename(root.join("ready.tmp"), root.join("ready")).unwrap();
        loop {
            std::hint::black_box(&owner);
            std::thread::park();
        }
    }

    #[test]
    fn native_process_death_recovers_original_signed_candidate() {
        native_crash_case(false);
    }

    #[test]
    fn native_process_death_after_durable_intent_blocks_replay() {
        native_crash_case(true);
    }

    fn native_crash_case(intent: bool) {
        use std::time::{Duration, Instant};
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("purpose"),
            b"inert signed staging and exact owned process kill only",
        )
        .unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "updater::windows_install::durable::tests::owned_crash_stage_child",
                "--ignored",
                "--nocapture",
            ])
            .env("GROK_OWNED_UPDATER_CRASH_ROOT", root.path())
            .env(
                "GROK_OWNED_UPDATER_CRASH_PHASE",
                if intent { "intent" } else { "prepared" },
            )
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !root.path().join("ready").exists()
            && Instant::now() < deadline
            && child.try_wait().unwrap().is_none()
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let ready = std::fs::read_to_string(root.path().join("ready"));
        let competing = Journal::open(&root.path().join("recovery"));
        let was_exclusive = competing.is_err();
        drop(competing);
        // Terminate only the exact Child handle we created, never a searched PID.
        let _ = child.kill();
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert!(
            was_exclusive,
            "child must own the OS journal lock before its forced death"
        );
        let id = ready.expect("owned child did not publish its durable checkpoint");
        let recovered = WindowsRecovery::new(root.path().join("recovery"));
        let count = Arc::new(AtomicUsize::new(0));
        let trusted = update(count.clone(), None);
        recovered.initialize(&trusted).unwrap();
        assert_eq!(
            recovered.catalog.snapshot().unwrap().unwrap().candidate_id,
            id
        );
        if intent {
            assert_eq!(
                recovered.catalog.snapshot().unwrap().unwrap().state,
                RecoveryState::Blocked
            );
            assert!(recovered.catalog.resume(&id).is_err());
            assert_eq!(count.load(Ordering::SeqCst), 0);
        } else {
            assert!(matches!(
                Update::run_windows_attempt(recovered.catalog.resume(&id).unwrap(), None),
                Err(Error::WindowsInstallPending {
                    phase: "cleanup",
                    ..
                })
            ));
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn dispatch_writes_intent_before_call_and_only_explicit_refusal_allows_retry() {
        let root = tempfile::tempdir().unwrap();
        let owner = Arc::new(WindowsRecovery::new(root.path().join("recovery")));
        let id = stage(owner.clone(), Arc::new(AtomicUsize::new(0)));
        let journal = owner.loaded.lock().unwrap().as_ref().unwrap().clone();
        let calls = AtomicUsize::new(0);
        assert!(dispatch(Some(&journal), &id, || {
            assert_eq!(
                journal.snapshot().unwrap().unwrap().phase,
                Phase::LaunchIntent
            );
            calls.fetch_add(1, Ordering::SeqCst);
            Err("explicit OS refusal".into())
        })
        .is_err());
        assert_eq!(journal.snapshot().unwrap().unwrap().phase, Phase::Prepared);
        assert!(dispatch(Some(&journal), &id, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(None) // protocol test, NOT an installer completion assertion
        })
        .is_err()); // Accepted without a handle can never authorize App exit.
        assert_eq!(journal.snapshot().unwrap().unwrap().phase, Phase::Accepted);
        assert!(dispatch(Some(&journal), &id, || panic!(
            "must not replay accepted handoff"
        ))
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn publication_error_or_dispatch_panic_cannot_grant_another_launch() {
        for mode in ["before", "after", "panic"] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("recovery");
            let owner = Arc::new(WindowsRecovery::new(path.clone()));
            let id = stage(owner.clone(), Arc::new(AtomicUsize::new(0)));
            let journal = owner.loaded.lock().unwrap().as_ref().unwrap().clone();
            let mut fence = if mode == "before" {
                Some(journal::protected_read(&path.join("active.dpapi")).unwrap())
            } else {
                None
            };
            let calls = AtomicUsize::new(0);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                dispatch(Some(&journal), &id, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    if mode == "panic" {
                        panic!("unverified dispatch result");
                    }
                    fence = Some(journal::protected_read(&path.join("active.dpapi")).unwrap());
                    Ok(None)
                })
            }));
            assert!(result.is_err() || result.unwrap().is_err());
            assert_eq!(calls.load(Ordering::SeqCst), usize::from(mode != "before"));
            assert!(dispatch(Some(&journal), &id, || panic!("second launch")).is_err());
            drop(fence);
            drop(journal);
            drop(owner);
            let recovered = WindowsRecovery::new(path);
            recovered
                .initialize(&update(Arc::new(AtomicUsize::new(0)), None))
                .unwrap();
            assert_eq!(
                recovered.catalog.snapshot().unwrap().unwrap().state,
                if mode == "before" {
                    RecoveryState::Retryable
                } else {
                    RecoveryState::Blocked
                }
            );
        }
    }

    #[cfg(feature = "zip")]
    #[test]
    fn archive_recovery_selects_only_one_verified_root_installer_without_extraction() {
        use std::io::Write;
        fn archive(names: &[&str]) -> Vec<u8> {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for name in names {
                zip.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"MZ inert owned test; not executable")
                    .unwrap();
            }
            zip.finish().unwrap().into_inner()
        }
        let (kind, entry, bytes) =
            installer_bytes(&archive(&["owned.exe", "../unrelated.txt"])).unwrap();
        assert_eq!(kind, Kind::Nsis);
        assert_eq!(entry.as_deref(), Some("owned.exe"));
        assert!(bytes.starts_with(b"MZ"));
        for names in [
            vec!["../escape.exe"],
            vec!["a.exe", "b.exe"],
            vec!["mismatch.msi"],
            vec!["nested/owned.exe"],
        ] {
            assert!(installer_bytes(&archive(&names)).is_err(), "{names:?}");
        }
        // The pinned ZIP reader collapses both by_name AND by_index duplicates.
        // Rename equal-width raw names after writing a standards-valid archive.
        let mut duplicate = archive(&["a.exe", "b.exe"]);
        for index in 0..duplicate.len().saturating_sub(4) {
            if &duplicate[index..index + 5] == b"b.exe" {
                duplicate[index] = b'a';
            }
        }
        assert_eq!(
            zip::ZipArchive::new(Cursor::new(&duplicate)).unwrap().len(),
            1
        );
        assert!(
            installer_bytes(&duplicate).is_err(),
            "duplicate physical installers must not collapse to one"
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.set_comment("metadata comment 中 文");
        let mut options = zip::write::FullFileOptions::default().large_file(true);
        options
            .add_extra_data(
                0xcafe,
                b"owned central metadata".to_vec().into_boxed_slice(),
                true,
            )
            .unwrap();
        zip.start_file("程序.exe", options).unwrap();
        zip.write_all(b"MZ inert ZIP64 owned fixture").unwrap();
        zip.start_file("notes.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"unrelated metadata is not extracted")
            .unwrap();
        let zip64 = zip.finish().unwrap().into_inner();
        let (kind, name, bytes) = installer_bytes(&zip64).unwrap();
        assert_eq!(kind, Kind::Nsis);
        assert_eq!(name.as_deref(), Some("程序.exe"));
        assert_eq!(bytes, b"MZ inert ZIP64 owned fixture");
        for cut in 0..zip64.len() {
            assert!(
                installer_bytes(&zip64[..cut]).is_err(),
                "truncated ZIP at {cut}"
            );
        }
    }

    #[test]
    fn actual_shell_refusal_restores_retryable_original_journal_state() {
        let root = tempfile::tempdir().unwrap();
        let owner = Arc::new(WindowsRecovery::new(root.path().join("recovery")));
        let id = stage(owner.clone(), Arc::new(AtomicUsize::new(0)));
        let journal = owner.loaded.lock().unwrap().as_ref().unwrap().clone();
        for _ in 0..2 {
            assert!(dispatch(Some(&journal), &id, || launch_process(
                &root.path().join("nonexistent-owned-installer.exe"),
                OsStr::new("")
            ))
            .is_err());
            assert_eq!(journal.snapshot().unwrap().unwrap().phase, Phase::Prepared);
        }
    }

    #[test]
    fn late_dispatch_refusal_reverifies_same_nonce_and_requires_fresh_cleanup() {
        use journal::{dispatch::Event, witness::Ticket};
        let root = tempfile::tempdir().unwrap();
        let owner = Arc::new(WindowsRecovery::new(root.path().join("recovery")));
        let count = Arc::new(AtomicUsize::new(0));
        let id = stage(owner.clone(), count.clone());
        let trusted = update(count.clone(), Some(owner.clone()));
        let journal = owner.loaded.lock().unwrap().as_ref().unwrap().clone();
        let original = journal.snapshot().unwrap().unwrap();
        let store = journal.witness_store().unwrap();
        let ticket = Ticket {
            id: uuid::Uuid::new_v4().to_string(),
            helper: ProcessIdentity {
                pid: 12,
                created: 13,
            },
        };
        store
            .publish_dispatch(&original, &ticket, Event::Ready)
            .unwrap();
        let delegated = journal.delegate_dispatch(&id, ticket.clone()).unwrap();
        owner
            .catalog
            .resume(&id)
            .unwrap()
            .finish(Err(journal::pending("Unobserved original dispatch")), true)
            .unwrap_err();
        for _ in 0..3 {
            assert_eq!(
                owner.pending(&trusted).unwrap().unwrap().state,
                RecoveryState::Blocked
            );
            assert!(owner.catalog.resume(&id).is_err());
        }
        store
            .publish_dispatch(
                &delegated,
                &ticket,
                Event::Rejected {
                    message: "Owned explicit OS refusal fixture".into(),
                },
            )
            .unwrap();
        let pending = owner.pending(&trusted).unwrap().unwrap();
        assert_eq!(pending.candidate_id, id);
        assert_eq!(pending.state, RecoveryState::Retryable);
        assert_eq!(pending.phase, "cleanup");
        assert_eq!(journal.snapshot().unwrap().unwrap(), original);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(matches!(
            Update::run_windows_attempt(owner.catalog.resume(&id).unwrap(), None),
            Err(Error::WindowsInstallPending {
                phase: "cleanup",
                ..
            })
        ));
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(journal.snapshot().unwrap().unwrap(), original);
    }
}
