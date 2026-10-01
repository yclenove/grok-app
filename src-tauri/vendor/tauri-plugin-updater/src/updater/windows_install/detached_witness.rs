//! Independent handle custody across App exit. This worker only observes an
//! already accepted process: it cannot launch, retry, or certify an installation.
use super::super::windows_journal::{self as journal, witness::Store};
use super::process_witness::{Observation, ProcessWitness};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT},
    System::{Pipes::PeekNamedPipe, Threading::GetCurrentProcessId},
};

#[cfg(test)]
use super::{
    super::windows_journal::{witness::Ticket, Journal, Phase},
    durable::process_identity,
};
#[cfg(test)]
use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
};
#[cfg(test)]
use windows_sys::Win32::{
    Foundation::DuplicateHandle,
    System::Threading::{
        GetCurrentProcess, CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

const ARG: &str = "--grok-update-process-witness-v1";
const MAX_BOOTSTRAP: usize = 4096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bootstrap {
    handle: usize,
    ticket: String,
}

#[cfg(test)]
pub(super) struct PendingWitness {
    child: Child,
    id: String,
    store: Store,
    _image: File,
}

#[cfg(test)]
impl PendingWitness {
    pub fn prepare(journal: &Journal, candidate: &str) -> Result<Self> {
        let record = journal
            .snapshot()?
            .filter(|r| r.candidate_id == candidate && r.phase == Phase::Prepared)
            .ok_or_else(|| {
                journal::pending("Witness preflight requires the original prepared candidate")
            })?;
        let store = journal.witness_store()?;
        let id = uuid::Uuid::new_v4().to_string();
        let (exe, image) = store.copy_executable(&record, &id)?;
        let mut command = Command::new(&exe);
        #[cfg(not(test))]
        command.arg(ARG);
        #[cfg(test)]
        command.args([
            "--exact",
            "updater::windows_install::detached_witness::tests::detached_witness_child",
            "--ignored",
            "--test-threads=1",
        ]);
        // No fallback to a job-bound worker: failure is safe before dispatch.
        command
            .creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW)
            .current_dir(
                exe.parent()
                    .ok_or_else(|| journal::pending("Witness directory missing"))?,
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command.spawn().map_err(journal::pending)?;
        Ok(Self {
            child,
            id,
            store,
            _image: image,
        })
    }

    pub fn handoff(
        mut self,
        journal: &Journal,
        candidate: &str,
        process: &OwnedHandle,
    ) -> Result<()> {
        let child_handle = self
            .child
            .as_handle()
            .try_clone_to_owned()
            .map_err(journal::pending)?;
        let helper = process_identity(&child_handle).map_err(journal::pending)?;
        let record = journal.authorize_witness(
            candidate,
            Ticket {
                id: self.id.clone(),
                helper,
            },
        )?;
        let mut remote = std::ptr::null_mut();
        // Transfer the exact OS handle, not an OpenProcess(PID) re-attachment.
        // The duplicate cannot terminate, inject into, or mutate the installer.
        if unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                process.as_raw_handle(),
                child_handle.as_raw_handle(),
                &mut remote,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                0,
            )
        } == 0
        {
            return Err(journal::pending(std::io::Error::last_os_error()));
        }
        let bytes = serde_json::to_vec(&Bootstrap {
            handle: remote as usize,
            ticket: self.id.clone(),
        })
        .map_err(journal::pending)?;
        let mut stdin = self
            .child
            .stdin
            .take()
            .ok_or_else(|| journal::pending("Private witness bootstrap pipe missing"))?;
        stdin
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .map_err(journal::pending)?;
        stdin.write_all(&bytes).map_err(journal::pending)?;
        // Retain the write end until custody ACK. PeekNamedPipe can report a
        // broken pipe once every writer closes, even during bootstrap framing.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.store.ready(&record)? {
                return Ok(());
            }
            if let Some(status) = self.child.try_wait().map_err(journal::pending)? {
                return Err(journal::pending(format!(
                    "Independent witness exited before custody acknowledgement ({status}); replay is blocked",
                )));
            }
            if Instant::now() >= deadline {
                // Observation timeout is not worker death; do not kill, relaunch,
                // replace the ticket, or turn the accepted installer retryable.
                return Err(journal::pending(
                    "Independent witness acknowledgement is still unverified; replay is blocked",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
impl Drop for PendingWitness {
    fn drop(&mut self) {
        if let Some(stdin) = self.child.stdin.take() {
            // Pre-dispatch abandonment/refusal: EOF cancels only this unused
            // helper. Once handed off, leave the observer alive, even on timeout.
            drop(stdin);
            let deadline = Instant::now() + Duration::from_secs(2);
            while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

pub(super) fn entrypoint() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|arg| arg != ARG) {
        return None;
    }
    Some(if args.len() == 2 && run().is_ok() {
        0
    } else {
        65
    })
}

fn read_bootstrap() -> Result<Bootstrap> {
    let stdin = std::io::stdin();
    // std::io::Stdin buffers ahead. Reading the four-byte prefix through it
    // would hide the body from PeekNamedPipe. Use an unbuffered owned duplicate.
    let mut input = File::from(
        stdin
            .as_handle()
            .try_clone_to_owned()
            .map_err(journal::pending)?,
    );
    let raw = input.as_raw_handle();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut bytes = Vec::new();
    let mut expected = 4;
    loop {
        let mut available = 0;
        if unsafe {
            PeekNamedPipe(
                raw,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(journal::pending(format!(
                "Witness requires its private parent pipe: {}",
                std::io::Error::last_os_error()
            )));
        }
        if available > 0 {
            let mut part = vec![0; (available as usize).min(expected - bytes.len())];
            input.read_exact(&mut part).map_err(journal::pending)?;
            bytes.extend(part);
            if bytes.len() == 4 {
                let len = u32::from_le_bytes(bytes[..4].try_into().expect("four bytes")) as usize;
                if len == 0 || len > MAX_BOOTSTRAP {
                    return Err(journal::pending("Invalid witness bootstrap size"));
                }
                expected = 4 + len;
            }
            if bytes.len() == expected {
                return serde_json::from_slice(&bytes[4..]).map_err(journal::pending);
            }
        }
        if Instant::now() >= deadline {
            return Err(journal::pending("Witness received no bounded bootstrap"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn run() -> Result<()> {
    let exe = std::env::current_exe().map_err(journal::pending)?;
    let root = exe
        .parent()
        .ok_or_else(|| journal::pending("Witness root missing"))?;
    let store = Store::open(root)?;
    let bootstrap = read_bootstrap()?;
    let record = store.record()?;
    let ticket = record
        .witness
        .as_ref()
        .ok_or_else(|| journal::pending("Witness is not authorized"))?;
    if ticket.id != bootstrap.ticket
        || store.executable(&record.candidate_id, &ticket.id)? != exe
        || ticket.helper.pid != unsafe { GetCurrentProcessId() }
    {
        return Err(journal::pending(
            "Witness executable/candidate/owner mismatch",
        ));
    }
    let mut image = journal::protected_read(&exe).map_err(journal::pending)?;
    let mut bytes = Vec::new();
    image.read_to_end(&mut bytes).map_err(journal::pending)?;
    if journal::digest(&bytes) != record.binding.executable_digest {
        return Err(journal::pending("Witness image bytes changed"));
    }
    let self_witness = ProcessWitness::attach(&ticket.helper)
        .map_err(|e| journal::pending(format!("Witness process identity: {e:?}")))?;
    if self_witness.observe() != Observation::Running {
        return Err(journal::pending("Witness creation identity is not live"));
    }
    let raw = bootstrap.handle as *mut std::ffi::c_void;
    let mut flags = 0;
    if bootstrap.handle <= 4
        || bootstrap.handle >= isize::MAX as usize
        || raw == std::io::stdin().as_raw_handle()
        || unsafe { GetHandleInformation(raw, &mut flags) } == 0
        || flags & HANDLE_FLAG_INHERIT != 0
    {
        return Err(journal::pending("Invalid private witness process handle"));
    }
    // This process owns the parent's non-inherited duplicate. No handle is
    // accepted through IPC, env vars, filesystem contents, or a PID lookup.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let identity = record
        .process
        .as_ref()
        .ok_or_else(|| journal::pending("Witness process missing"))?;
    let witness = ProcessWitness::from_handle(handle, identity)
        .map_err(|e| journal::pending(format!("Witness installer identity: {e:?}")))?;
    store.publish(&record, None)?;
    loop {
        match witness.observe() {
            Observation::Running => std::thread::sleep(Duration::from_millis(100)),
            Observation::Exited(exit) => return store.publish(&record, Some(exit)),
            other => {
                return Err(journal::pending(format!(
                    "Independent observation unavailable: {other:?}"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::windows_journal::{Binding, Kind, ProcessIdentity, Record};
    use super::super::process_witness::tests::OwnedChild;
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    fn prepared(root: &Path) -> (Journal, String) {
        let exe = std::env::current_exe().unwrap();
        let journal = Journal::open(root).unwrap();
        let record = Record {
            candidate_id: uuid::Uuid::new_v4().to_string(),
            source_version: "1.0.0".into(),
            version: "1.0.1".into(),
            target: "windows-x86_64".into(),
            signature: "private-inert-fixture".into(),
            payload_digest: journal::digest(b"fixture"),
            installer_digest: journal::digest(b"fixture"),
            kind: Kind::Nsis,
            archive_entry: None,
            binding: Binding {
                executable: exe.as_os_str().encode_wide().collect(),
                executable_digest: journal::digest(&std::fs::read(exe).unwrap()),
                app_name: "private-fixture".into(),
                key_digest: [1; 32],
                install_mode: "passive".into(),
                installer_args: vec![],
            },
            current_args: vec![],
            phase: Phase::Prepared,
            process: None,
            process_exit: None,
            witness: None,
            dispatcher: None,
            inventory: None,
        };
        let id = record.candidate_id.clone();
        drop(journal.prepare(record, b"fixture", b"fixture").unwrap());
        (journal, id)
    }

    fn wait_exit(witness: &ProcessWitness) -> journal::ProcessExit {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match witness.observe() {
                Observation::Exited(exit) => return exit,
                Observation::Running => (),
                other => panic!("unexpected observation {other:?}"),
            }
            assert!(
                Instant::now() < deadline,
                "owned child remains live after test deadline"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn exact_handle_handoff_persists_exit_without_app_polling_and_cannot_replace_ticket() {
        let root = tempfile::tempdir().unwrap();
        let (journal, id) = prepared(&root.path().join("recovery"));
        let mut child = OwnedChild::spawn(259);
        super::super::durable::dispatch(Some(&journal), &id, || Ok(Some(child.handle()))).unwrap();
        let record = journal.snapshot().unwrap().unwrap();
        let ticket = record.witness.as_ref().unwrap();
        let helper = ProcessWitness::attach(&ticket.helper).unwrap_or_else(|e| panic!("{e:?}"));
        let store = journal.witness_store().unwrap();
        assert!(store.ready(&record).unwrap());
        assert!(store.exit(&record).unwrap().is_none());
        assert!(journal.authorize_witness(&id, ticket.clone()).is_err());
        assert!(PendingWitness::prepare(&journal, &id).is_err());
        drop(journal);
        child.finish();
        assert_eq!(wait_exit(&helper).code, 0);
        let exit = store.exit(&record).unwrap().unwrap();
        assert_eq!(exit.code, 259);
        assert_eq!(exit.identity, record.process.clone().unwrap());
        let recovered = Journal::open(&root.path().join("recovery")).unwrap();
        recovered.record_process_exit(&id, &exit).unwrap();
        assert_eq!(
            recovered.snapshot().unwrap().unwrap().phase,
            Phase::Accepted
        );
    }

    #[test]
    fn independent_receipts_reject_cross_ticket_conflict_and_plaintext_tampering() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        let (journal, id) = prepared(&path);
        let process = ProcessIdentity {
            pid: 10,
            created: 20,
        };
        journal
            .transition(&id, Phase::Prepared, Phase::LaunchIntent, None)
            .unwrap();
        journal
            .transition(
                &id,
                Phase::LaunchIntent,
                Phase::Accepted,
                Some(process.clone()),
            )
            .unwrap();
        let ticket = Ticket {
            id: uuid::Uuid::new_v4().to_string(),
            helper: ProcessIdentity {
                pid: 30,
                created: 40,
            },
        };
        let record = journal.authorize_witness(&id, ticket.clone()).unwrap();
        let store = journal.witness_store().unwrap();
        store.publish(&record, None).unwrap();
        let exit = journal::ProcessExit {
            identity: process,
            exited: 50,
            code: 0,
        };
        store.publish(&record, Some(exit.clone())).unwrap();
        store.publish(&record, Some(exit.clone())).unwrap();
        let changed = journal::ProcessExit {
            code: 1603,
            ..exit.clone()
        };
        assert!(store.publish(&record, Some(changed)).is_err());
        let exit_path = path.join(format!("{id}.{}.exit.dpapi", ticket.id));
        let ready_path = path.join(format!("{id}.{}.ready.dpapi", ticket.id));
        let ready_bytes = std::fs::read(&ready_path).unwrap();
        let valid = std::fs::read(&exit_path).unwrap();
        assert!(!valid.windows(4).any(|b| b == b"code"));
        std::fs::write(&exit_path, ready_bytes).unwrap();
        assert!(
            store.exit(&record).is_err(),
            "ready cannot stand in for an exit"
        );
        let mut other = record.clone();
        other.witness.as_mut().unwrap().id = uuid::Uuid::new_v4().to_string();
        let other_path = path.join(format!(
            "{id}.{}.exit.dpapi",
            other.witness.as_ref().unwrap().id
        ));
        std::fs::write(other_path, &valid).unwrap();
        assert!(
            store.exit(&other).is_err(),
            "DPAPI ciphertext is ticket-bound"
        );
        std::fs::write(&exit_path, b"{\"code\":0}").unwrap();
        assert!(store.exit(&record).is_err());
        std::fs::write(&exit_path, valid).unwrap();
        assert_eq!(store.exit(&record).unwrap(), Some(exit));
        assert_eq!(journal.snapshot().unwrap().unwrap().phase, Phase::Accepted);
    }

    #[test]
    fn failed_helper_after_acceptance_cannot_restore_prepared_or_replace_custody() {
        let root = tempfile::tempdir().unwrap();
        let (journal, id) = prepared(&root.path().join("recovery"));
        let mut helper = PendingWitness::prepare(&journal, &id).unwrap();
        // Fault injection only terminates this test-owned helper, never the
        // installer, another App, or a process found by name/PID.
        helper.child.kill().unwrap();
        helper.child.wait().unwrap();
        let mut child = OwnedChild::spawn(0);
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
        assert!(helper.handoff(&journal, &id, &child.handle()).is_err());
        let record = journal.snapshot().unwrap().unwrap();
        assert_eq!(record.phase, Phase::Accepted);
        assert!(record.process_exit.is_none());
        assert!(record.witness.is_some());
        assert!(!journal.witness_store().unwrap().ready(&record).unwrap());
        assert!(PendingWitness::prepare(&journal, &id).is_err());
        assert!(
            super::super::durable::dispatch(Some(&journal), &id, || panic!("must never replay"))
                .is_err()
        );
        child.finish();
    }

    #[test]
    fn forged_bootstrap_and_unregistered_copied_image_never_acknowledge() {
        let root = tempfile::tempdir().unwrap();
        let (journal, id) = prepared(&root.path().join("recovery"));
        for frame in [vec![255, 255, 255, 127], vec![2, 0, 0, 0, b'{', b'}']] {
            let mut helper = PendingWitness::prepare(&journal, &id).unwrap();
            helper
                .child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(&frame)
                .unwrap();
            let handle = helper.child.as_handle().try_clone_to_owned().unwrap();
            let identity = process_identity(&handle).unwrap();
            let witness =
                ProcessWitness::from_handle(handle, &identity).unwrap_or_else(|e| panic!("{e:?}"));
            assert_ne!(wait_exit(&witness).code, 0);
            assert_eq!(journal.snapshot().unwrap().unwrap().phase, Phase::Prepared);
            assert!(journal.snapshot().unwrap().unwrap().witness.is_none());
        }
    }

    #[test]
    fn actual_parent_exit_does_not_lose_the_installer_exit_receipt() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("owned-witness-parent.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let mut parent = Command::new(exe)
            .args([
                "--exact",
                "updater::windows_install::detached_witness::tests::detached_parent_child",
                "--ignored",
                "--test-threads=1",
            ])
            .env("GROK_OWNED_WITNESS_PARENT", root.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = parent.try_wait().unwrap() {
                assert!(status.success(), "owned parent failed {status}");
                break;
            }
            if Instant::now() >= deadline {
                let _ = parent.kill();
                let _ = parent.wait();
                panic!("owned parent not terminal");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(parent); // The original parent process really exited, not simulated.
        let info: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("children.json")).unwrap())
                .unwrap();
        let recovery = root.path().join("recovery");
        let journal = Journal::open(&recovery).unwrap();
        let record = journal.snapshot().unwrap().unwrap();
        let helper = ProcessWitness::attach(&record.witness.as_ref().unwrap().helper)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let installer = ProcessWitness::attach(record.process.as_ref().unwrap())
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(helper.observe(), Observation::Running);
        assert_eq!(installer.observe(), Observation::Running);
        let store = journal.witness_store().unwrap();
        assert!(store.ready(&record).unwrap());
        drop(journal); // No App polling or owner exists while the installer exits.
        let release = std::path::PathBuf::from(info["release"].as_str().unwrap());
        assert_eq!(release.file_name().unwrap(), "release");
        std::fs::write(&release, b"release owned fixture").unwrap();
        assert_eq!(wait_exit(&installer).code, 1603);
        assert_eq!(wait_exit(&helper).code, 0);
        let recovered = Journal::open(&recovery).unwrap();
        let exit = store.exit(&record).unwrap().unwrap();
        assert_eq!(exit.code, 1603);
        recovered
            .record_process_exit(&record.candidate_id, &exit)
            .unwrap();
        assert_eq!(
            recovered.snapshot().unwrap().unwrap().phase,
            Phase::Accepted
        );
        drop(installer);
        // Only the root named by our exact fixture child, with its purpose marker.
        let child_root = release.parent().unwrap();
        assert_eq!(
            std::fs::read(child_root.join("fixture-purpose")).unwrap(),
            b"owned process witness only"
        );
        std::fs::remove_dir_all(child_root).unwrap();
    }

    #[test]
    #[ignore = "private parent fixture exercises real process death after custody handoff"]
    fn detached_parent_child() {
        let root = std::path::PathBuf::from(
            std::env::var_os("GROK_OWNED_WITNESS_PARENT").expect("owned fixture only"),
        );
        assert_eq!(
            std::env::current_exe().unwrap().parent(),
            Some(root.as_path())
        );
        let (journal, id) = prepared(&root.join("recovery"));
        let child = OwnedChild::spawn(1603);
        super::super::durable::dispatch(Some(&journal), &id, || Ok(Some(child.handle()))).unwrap();
        std::fs::write(
            root.join("children.json"),
            serde_json::to_vec(&serde_json::json!({"release": child.release_path()})).unwrap(),
        )
        .unwrap();
        std::process::exit(0); // Bypass all destructors, including Child handles.
    }

    #[test]
    #[ignore = "only a private copied harness with a bounded parent pipe may run this"]
    fn detached_witness_child() {
        super::run().unwrap();
    }
}
