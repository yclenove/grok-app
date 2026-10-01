//! A verified private App copy owns OS dispatch and the original process handle.
//! The parent's sole grant is a durable ticket bound to this exact live process.
use super::super::windows_journal::{
    self as journal,
    dispatch::{fingerprint, Event},
    witness::{Store, Ticket},
    Journal, Phase, Record,
};
use super::{
    durable::process_identity,
    launch_plan::LaunchPlan,
    process_witness::{Observation, ProcessWitness},
    verified_artifact,
};
use crate::{Config, Result};
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::os::windows::{
    ffi::{OsStrExt, OsStringExt},
    io::{AsHandle, BorrowedHandle},
    process::CommandExt,
};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW,
};

const ARG: &str = "--grok-update-dispatch-v1";

/// Compiled host policy. Never construct this from a journal, arguments, IPC,
/// environment override, or untrusted sidecar configuration.
pub struct WindowsDispatchTrust {
    pub config: Config,
    pub app_name: String,
    pub source_version: String,
    /// The host's updater target override, or "windows" for the default target.
    pub target: String,
}

/// Called before Tauri/UI initialization. The policy callback is lazy and used
/// only for an exact reserved invocation; disabled/unsigned hosts must refuse.
pub fn windows_update_dispatch_entrypoint(
    trusted: impl FnOnce() -> std::result::Result<WindowsDispatchTrust, String>,
) -> Option<i32> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|arg| arg != ARG) {
        return None;
    }
    Some(
        if args.len() == 2 && trusted().map_err(journal::pending).and_then(run).is_ok() {
            0
        } else {
            65
        },
    )
}

pub(super) struct PendingDispatch {
    child: Child,
    ticket: Ticket,
    store: Store,
    original: Record,
    _image: File,
}

impl PendingDispatch {
    pub fn prepare(journal: &Journal, candidate: &str) -> Result<Self> {
        let original = journal
            .snapshot()?
            .filter(|r| r.candidate_id == candidate && r.phase == Phase::Prepared)
            .ok_or_else(|| {
                journal::pending("Dispatcher requires the original prepared candidate")
            })?;
        let store = journal.witness_store()?;
        let id = uuid::Uuid::new_v4().to_string();
        let (exe, image) = store.copy_dispatcher(&original, &id)?;
        let mut command = Command::new(&exe);
        #[cfg(not(test))]
        command.arg(ARG);
        #[cfg(test)]
        command.args([
            "--exact",
            "updater::windows_install::detached_dispatch::tests::dispatch_worker",
            "--ignored",
            "--test-threads=1",
        ]);
        command
            .creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW)
            .current_dir(
                exe.parent()
                    .ok_or_else(|| journal::pending("Dispatcher root missing"))?,
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command.spawn().map_err(journal::pending)?;
        let helper = process_identity(
            &child
                .as_handle()
                .try_clone_to_owned()
                .map_err(journal::pending)?,
        )
        .map_err(journal::pending)?;
        let mut pending = Self {
            child,
            ticket: Ticket { id, helper },
            store,
            original,
            _image: image,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if pending
                .store
                .dispatch_event(&pending.original, &pending.ticket, "ready")?
                == Some(Event::Ready)
            {
                return Ok(pending);
            }
            if let Some(status) = pending.child.try_wait().map_err(journal::pending)? {
                return Err(journal::pending(format!(
                    "Dispatcher refused preflight ({status}); no launch grant published"
                )));
            }
            if Instant::now() >= deadline {
                return Err(journal::pending(
                    "Dispatcher preflight is unverified; no launch grant published",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn authorize(&self, journal: &Journal) -> Result<Record> {
        journal.delegate_dispatch(&self.original.candidate_id, self.ticket.clone())
    }

    fn wait(mut self, journal: &Journal) -> Result<()> {
        // After authorization, elapsed observation time is not terminal. UAC
        // and Shell/DDE handoff can legitimately wait on a person. Keep this
        // owned task alive while the exact helper lives; UI waiters may detach.
        loop {
            let record = journal
                .reconcile_dispatch()?
                .ok_or_else(|| journal::pending("Dispatch candidate missing"))?;
            if record.candidate_id != self.original.candidate_id {
                return Err(journal::pending("Dispatch owner changed"));
            }
            match record.phase {
                Phase::Accepted if record.process.is_some() => return Ok(()),
                Phase::Accepted => {
                    return Err(journal::pending(
                        "OS accepted without an observable process; replay blocked",
                    ))
                }
                Phase::Prepared => return Err(journal::pending(
                    "OS explicitly refused the delegated installer; original candidate retained",
                )),
                Phase::LaunchIntent => (),
            }
            if let Some(status) = self.child.try_wait().map_err(journal::pending)? {
                // Re-read after observed exit; the final publication can race the
                // previous poll. Caller also fences the persistent launch intent.
                let record = journal
                    .reconcile_dispatch()?
                    .ok_or_else(|| journal::pending("Dispatch candidate missing"))?;
                if record.phase == Phase::Accepted && record.process.is_some() {
                    return Ok(());
                }
                return Err(journal::pending(format!(
                    "Dispatcher exited ({status}); journal outcome governs recovery"
                )));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

pub(super) fn dispatch(journal: &Journal, candidate: &str) -> std::result::Result<(), String> {
    (|| {
        let worker = PendingDispatch::prepare(journal, candidate)?;
        worker.authorize(journal)?;
        worker.wait(journal)
    })()
    .map_err(|e: crate::Error| e.to_string())
}

fn run(trusted: WindowsDispatchTrust) -> Result<()> {
    let exe = std::env::current_exe().map_err(journal::pending)?;
    let store = Store::open(
        exe.parent()
            .ok_or_else(|| journal::pending("Dispatcher root missing"))?,
    )?;
    let original = store.record()?;
    if original.phase != Phase::Prepared {
        return Err(journal::pending(
            "Dispatcher did not start from a prepared candidate",
        ));
    }
    let name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| journal::pending("Invalid dispatcher filename"))?;
    let ticket_id = name
        .strip_prefix(&format!("{}.", original.candidate_id))
        .and_then(|s| s.strip_suffix(".dispatcher.exe"))
        .ok_or_else(|| journal::pending("Dispatcher filename is not candidate-bound"))?;
    if store.dispatcher_executable(&original.candidate_id, ticket_id)? != exe {
        return Err(journal::pending("Dispatcher image path mismatch"));
    }
    let handle = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) }
        .try_clone_to_owned()
        .map_err(journal::pending)?;
    let ticket = Ticket {
        id: ticket_id.to_owned(),
        helper: process_identity(&handle).map_err(journal::pending)?,
    };
    let mut image = journal::protected_read(&exe).map_err(journal::pending)?;
    let mut bytes = Vec::new();
    image.read_to_end(&mut bytes).map_err(journal::pending)?;
    let args = validate_policy(&original, &trusted, &bytes)?;
    let (_lease, installer) =
        verified_artifact::verify_in_store(&store, &original, &trusted.config.pubkey)?;
    let current_args: Vec<_> = original
        .current_args
        .iter()
        .map(|a| OsString::from_wide(a))
        .collect();
    let plan = LaunchPlan::new(
        &installer,
        trusted.config.install_mode(),
        &current_args,
        &args,
    )
    .map_err(journal::pending)?;
    let plan = super::completion::bind_plan(
        plan,
        exe.parent()
            .ok_or_else(|| journal::pending("Dispatcher root missing"))?,
        &original,
        &trusted.config.pubkey,
    )?;
    store.publish_dispatch(&original, &ticket, Event::Ready)?;
    let expected = fingerprint(&original)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let authorized = loop {
        let record = store.record()?;
        if fingerprint(&record)? != expected {
            return Err(journal::pending("Dispatcher immutable candidate changed"));
        }
        if let Some(owner) = &record.dispatcher {
            if owner != &ticket || record.phase != Phase::LaunchIntent {
                return Err(journal::pending(
                    "Dispatcher grant belongs to another process/phase",
                ));
            }
            break record;
        }
        if record.phase != Phase::Prepared || Instant::now() >= deadline {
            return Err(journal::pending(
                "Dispatcher received no durable launch grant",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    #[cfg(test)]
    tests::barrier(&store, &authorized, "before-launch")?;
    // The only production OS-launch site for a durable Windows transaction.
    // Never loop/retry this call: we, not the exiting App, own its exact handle.
    let process = match plan.launch() {
        Ok(process) => process,
        Err(message) => {
            return store.publish_dispatch(&authorized, &ticket, Event::Rejected { message })
        }
    };
    let identity = process
        .as_ref()
        .map(process_identity)
        .transpose()
        .map_err(journal::pending)?;
    #[cfg(test)]
    tests::barrier(&store, &authorized, "after-launch")?;
    let accepted = store.publish_dispatch(
        &authorized,
        &ticket,
        Event::Accepted {
            process: identity.clone(),
        },
    );
    let (Some(handle), Some(identity)) = (process, identity) else {
        return accepted;
    };
    let witness = ProcessWitness::from_handle(handle, &identity)
        .map_err(|e| journal::pending(format!("Dispatcher handle identity: {e:?}")))?;
    loop {
        match witness.observe() {
            Observation::Running => std::thread::sleep(Duration::from_millis(100)),
            Observation::Exited(exit) => {
                return store.publish_dispatch(&authorized, &ticket, Event::Exited(exit))
            }
            other => {
                return Err(journal::pending(format!(
                    "Dispatcher process observation unavailable: {other:?}"
                )))
            }
        }
    }
}

fn validate_policy(
    original: &Record,
    trusted: &WindowsDispatchTrust,
    bytes: &[u8],
) -> Result<Vec<OsString>> {
    let args = trusted
        .config
        .windows
        .as_ref()
        .map(|w| w.installer_args.clone())
        .unwrap_or_default();
    if journal::digest(bytes) != original.binding.executable_digest
        || trusted.config.pubkey.is_empty()
        || journal::digest(trusted.config.pubkey.as_bytes()) != original.binding.key_digest
        || trusted.app_name != original.binding.app_name
        || trusted.source_version != original.source_version
        || trusted.target != original.target
        || trusted.config.install_mode().to_string() != original.binding.install_mode
        || args
            .iter()
            .map(|a| a.encode_wide().collect::<Vec<_>>())
            .collect::<Vec<_>>()
            != original.binding.installer_args
    {
        return Err(journal::pending(
            "Dispatcher does not match compiled host policy/image",
        ));
    }
    Ok(args)
}

#[cfg(test)]
mod tests;
