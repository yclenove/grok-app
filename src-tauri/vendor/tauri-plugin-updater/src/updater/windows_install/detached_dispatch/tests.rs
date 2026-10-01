use super::super::super::windows_journal::{ProcessExit, ProcessIdentity};
use super::*;
use std::path::Path;
use std::sync::{atomic::AtomicUsize, Arc, OnceLock};

// This deterministic PUBLIC TEST key is never included in a product binary.
const KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IFBVQkxJQyBURVNUIE9OTFkgZGlzcGF0Y2gga2V5ClJXUlVSVk5VVDA1TVdRT2hCNy96emhDK0hYRGRHT2RMd0psbjVOWXdtNlVOWHgzY2htUVNWVEc0";
const PURPOSE: &[u8] = b"grok updater owned process fixture v1";

fn trusted() -> WindowsDispatchTrust {
    WindowsDispatchTrust {
        config: Config {
            pubkey: KEY.into(),
            ..Default::default()
        },
        app_name: "owned-updater-recovery-test".into(),
        source_version: "0.0.0".into(),
        target: "windows-x86_64".into(),
    }
}

fn fixture(inert: bool) -> &'static (Vec<u8>, String) {
    static NATIVE: OnceLock<(Vec<u8>, String)> = OnceLock::new();
    static INERT: OnceLock<(Vec<u8>, String)> = OnceLock::new();
    (if inert { &INERT } else { &NATIVE }).get_or_init(|| {
        let temp = tempfile::tempdir().unwrap();
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/build-dispatch-payload.mjs");
        let output = Command::new("node")
            .arg(script)
            .arg(temp.path())
            .arg(if inert { "inert" } else { "native" })
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), KEY);
        (
            std::fs::read(temp.path().join("owned-dispatch-payload.exe")).unwrap(),
            std::fs::read_to_string(temp.path().join("signature.txt")).unwrap(),
        )
    })
}

fn staged(root: &Path, inert: bool) -> (Arc<Journal>, super::super::PreparedWindowsInstaller) {
    let (bytes, signature) = fixture(inert);
    let metadata = serde_json::json!({ "publicKey": KEY, "signature": signature });
    let update = super::super::tests::refusing_update(&metadata, Arc::new(AtomicUsize::new(0)));
    let journal = Arc::new(Journal::open(root).unwrap());
    std::fs::write(root.join("owned-dispatch-test-purpose"), PURPOSE).unwrap();
    let prepared = update
        .stage_durable(bytes, &journal, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    (journal, prepared)
}

fn wait_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(Instant::now() < deadline, "waiting for {}", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_exit(child: &mut Child, expected: i32) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(expected));
            return;
        }
        if Instant::now() >= deadline {
            // This handle was created by this test, never looked up by name/PID.
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("owned child did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn barrier(store: &Store, record: &Record, phase: &str) -> Result<()> {
    let path = store.artifact_path(&record.candidate_id, None)?;
    let root = path.parent().unwrap();
    if !root.join(format!("barrier-{phase}")).exists() {
        return Ok(());
    }
    std::fs::write(root.join(format!("reached-{phase}")), b"owned test barrier")
        .map_err(journal::pending)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.join(format!("release-{phase}")).exists() {
        if Instant::now() >= deadline {
            return Err(journal::pending("Owned test barrier not released"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[test]
#[ignore = "private copied worker; invoked by owned parent tests"]
fn dispatch_worker() {
    std::process::exit(if run(trusted()).is_ok() { 0 } else { 65 });
}

#[test]
#[ignore = "owned parent deliberately dies after durable delegation"]
fn dispatch_parent() {
    let root = std::env::current_dir().unwrap();
    assert_eq!(std::fs::read(root.join("parent-purpose")).unwrap(), PURPOSE);
    // Journal initialization must precede markers in its separate store.
    let root = root.join("store");
    let (journal, _prepared) = staged(&root, false);
    let phase = std::fs::read_to_string(root.parent().unwrap().join("phase")).unwrap();
    std::fs::write(root.join(format!("barrier-{phase}")), b"owned").unwrap();
    std::fs::write(root.join("installer-exit-code"), "259").unwrap();
    let record = journal.snapshot().unwrap().unwrap();
    let worker = PendingDispatch::prepare(&journal, &record.candidate_id).unwrap();
    worker.authorize(&journal).unwrap();
    wait_path(&root.join(format!("reached-{phase}")));
    // No destructors / handoff pipe / acceptance ACK. Worker survives this App.
    std::process::exit(73);
}

#[test]
fn durable_worker_survives_real_parent_death_before_and_after_os_acceptance() {
    for phase in ["before-launch", "after-launch"] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("parent-purpose"), PURPOSE).unwrap();
        std::fs::write(temp.path().join("phase"), phase).unwrap();
        let mut parent = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "updater::windows_install::detached_dispatch::tests::dispatch_parent",
                "--ignored",
                "--test-threads=1",
            ])
            .current_dir(temp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait_exit(&mut parent, 73);
        let root = temp.path().join("store");
        let journal = Journal::open(&root).unwrap();
        let record = journal.reconcile_dispatch().unwrap().unwrap();
        assert_eq!(record.phase, Phase::LaunchIntent);
        let ticket = record.dispatcher.as_ref().unwrap();
        let observer = ProcessWitness::attach(&ticket.helper).unwrap();
        assert_eq!(observer.observe(), Observation::Running);
        assert!(PendingDispatch::prepare(&journal, &record.candidate_id).is_err());
        assert!(journal
            .delegate_dispatch(&record.candidate_id, ticket.clone())
            .is_err());
        if phase == "before-launch" {
            assert!(!root.join("installer-started").exists());
        }
        std::fs::write(
            root.join(format!("release-{phase}")),
            b"release owned worker",
        )
        .unwrap();
        wait_path(&root.join("installer-started"));
        let pid: u32 = std::fs::read_to_string(root.join("installer-started"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        std::fs::write(
            root.join("installer-release"),
            b"release owned installer fixture",
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let recovered = journal.reconcile_dispatch().unwrap().unwrap();
            if let Some(exit) = recovered.process_exit {
                assert_eq!(recovered.phase, Phase::Accepted);
                assert_eq!(exit.code, 259);
                assert_eq!(exit.identity.pid, pid);
                assert_eq!(recovered.candidate_id, record.candidate_id);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no independent exit receipt after {phase}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        while observer.observe() == Observation::Running {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        // Reading evidence repeatedly does not dispatch another process.
        for _ in 0..5 {
            assert_eq!(
                journal.reconcile_dispatch().unwrap().unwrap().phase,
                Phase::Accepted
            );
        }
    }
}

#[test]
fn prepared_worker_cannot_launch_without_exact_durable_grant() {
    let root = tempfile::tempdir().unwrap();
    let (journal, _prepared) = staged(&root.path().join("store"), false);
    let record = journal.snapshot().unwrap().unwrap();
    let mut worker = PendingDispatch::prepare(&journal, &record.candidate_id).unwrap();
    assert_eq!(journal.snapshot().unwrap().unwrap().phase, Phase::Prepared);
    let path = worker
        .store
        .artifact_path(&record.candidate_id, None)
        .unwrap();
    assert!(!path.parent().unwrap().join("installer-started").exists());
    let mut wrong = worker.ticket.clone();
    wrong.helper.created += 1;
    assert!(journal
        .delegate_dispatch(&record.candidate_id, wrong)
        .is_err());
    assert!(!path.parent().unwrap().join("installer-started").exists());
    worker.child.kill().unwrap();
    worker.child.wait().unwrap();
    assert_eq!(journal.snapshot().unwrap().unwrap().phase, Phase::Prepared);
}

#[test]
fn explicit_os_refusal_restores_same_candidate_and_forbids_ticket_reuse() {
    let root = tempfile::tempdir().unwrap();
    let (journal, _prepared) = staged(&root.path().join("store"), true);
    let record = journal.snapshot().unwrap().unwrap();
    let mut worker = PendingDispatch::prepare(&journal, &record.candidate_id).unwrap();
    worker.authorize(&journal).unwrap();
    wait_exit(&mut worker.child, 0);
    let recovered = journal.reconcile_dispatch().unwrap().unwrap();
    assert_eq!(recovered, record);
    assert!(journal
        .delegate_dispatch(&record.candidate_id, worker.ticket.clone())
        .is_err());
    // A fresh, separately verified helper can retry this same original nonce.
    let mut next = PendingDispatch::prepare(&journal, &record.candidate_id).unwrap();
    assert_ne!(worker.ticket, next.ticket);
    next.authorize(&journal).unwrap();
    wait_exit(&mut next.child, 0);
    assert_eq!(journal.reconcile_dispatch().unwrap().unwrap(), record);
}

#[test]
fn dispatch_receipts_reject_conflicts_rebinding_and_phase_bypass() {
    let root = tempfile::tempdir().unwrap();
    let (journal, _prepared) = staged(&root.path().join("store"), true);
    let record = journal.snapshot().unwrap().unwrap();
    let store = journal.witness_store().unwrap();
    let ticket = Ticket {
        id: uuid::Uuid::new_v4().to_string(),
        helper: ProcessIdentity {
            pid: 10,
            created: 20,
        },
    };
    assert!(journal
        .delegate_dispatch(&record.candidate_id, ticket.clone())
        .is_err());
    store
        .publish_dispatch(&record, &ticket, Event::Ready)
        .unwrap();
    let delegated = journal
        .delegate_dispatch(&record.candidate_id, ticket.clone())
        .unwrap();
    assert!(journal
        .transition(
            &record.candidate_id,
            Phase::LaunchIntent,
            Phase::Prepared,
            None
        )
        .is_err());
    let process = ProcessIdentity {
        pid: 30,
        created: 40,
    };
    store
        .publish_dispatch(
            &delegated,
            &ticket,
            Event::Accepted {
                process: Some(process.clone()),
            },
        )
        .unwrap();
    assert!(store
        .publish_dispatch(
            &delegated,
            &ticket,
            Event::Rejected {
                message: "forged".into()
            }
        )
        .is_err());
    let mut changed = delegated.clone();
    changed.current_args.push(vec![65]);
    assert!(store.dispatch_event(&changed, &ticket, "outcome").is_err());
    let wrong = ProcessExit {
        identity: ProcessIdentity {
            pid: 31,
            created: 40,
        },
        exited: 41,
        code: 0,
    };
    store
        .publish_dispatch(&delegated, &ticket, Event::Exited(wrong))
        .unwrap();
    assert!(journal.reconcile_dispatch().is_err());
    assert_eq!(
        journal.snapshot().unwrap().unwrap().phase,
        Phase::LaunchIntent
    );
}

#[test]
fn exit_only_receipt_proves_acceptance_not_installation_or_retry() {
    let root = tempfile::tempdir().unwrap();
    let (journal, _prepared) = staged(&root.path().join("store"), true);
    let record = journal.snapshot().unwrap().unwrap();
    let store = journal.witness_store().unwrap();
    let ticket = Ticket {
        id: uuid::Uuid::new_v4().to_string(),
        helper: ProcessIdentity {
            pid: 10,
            created: 20,
        },
    };
    store
        .publish_dispatch(&record, &ticket, Event::Ready)
        .unwrap();
    let delegated = journal
        .delegate_dispatch(&record.candidate_id, ticket.clone())
        .unwrap();
    let exit = ProcessExit {
        identity: ProcessIdentity {
            pid: 30,
            created: 40,
        },
        exited: 41,
        code: 1603,
    };
    store
        .publish_dispatch(&delegated, &ticket, Event::Exited(exit.clone()))
        .unwrap();
    let recovered = journal.reconcile_dispatch().unwrap().unwrap();
    assert_eq!(recovered.phase, Phase::Accepted);
    assert_eq!(recovered.process_exit, Some(exit));
    assert!(journal
        .delegate_dispatch(&record.candidate_id, ticket)
        .is_err());
}

#[test]
fn worker_preflight_rejects_compiled_policy_and_payload_tampering() {
    for mode in ["payload", "installer"] {
        let root = tempfile::tempdir().unwrap();
        let (journal, prepared) = staged(&root.path().join("store"), true);
        let record = journal.snapshot().unwrap().unwrap();
        // Mutate only this test's private store; the child must independently
        // reject it before any durable grant or OS dispatch is published.
        drop(prepared);
        let payload = journal.path(&record.candidate_id, None).unwrap();
        match mode {
            "payload" => std::fs::write(&payload, b"MZ corrupted owned bytes").unwrap(),
            "installer" => {
                // record's bound source image is never user-modified
                let path = journal
                    .path(&record.candidate_id, Some(record.kind))
                    .unwrap();
                std::fs::write(path, b"MZ substituted installer").unwrap();
            }
            _ => unreachable!(),
        }
        assert!(PendingDispatch::prepare(&journal, &record.candidate_id).is_err());
        assert_eq!(journal.snapshot().unwrap().unwrap().phase, Phase::Prepared);
        assert!(!payload.parent().unwrap().join("installer-started").exists());
    }
}

#[test]
fn every_compiled_trust_binding_is_checked_without_journal_key_authority() {
    let root = tempfile::tempdir().unwrap();
    let (journal, _prepared) = staged(&root.path().join("store"), true);
    let original = journal.snapshot().unwrap().unwrap();
    let image = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    assert!(validate_policy(&original, &trusted(), &image).is_ok());
    for field in ["key", "name", "version", "target", "mode", "args", "image"] {
        let mut policy = trusted();
        match field {
            "key" => policy.config.pubkey = "journal-provided-key".into(),
            "name" => policy.app_name.push('x'),
            "version" => policy.source_version = "9.9.9".into(),
            "target" => policy.target = "linux".into(),
            "mode" => {
                policy.config.windows = Some(
                    serde_json::from_value(serde_json::json!({"installMode":"quiet"})).unwrap(),
                )
            }
            "args" => {
                policy.config.windows = Some(
                    serde_json::from_value(serde_json::json!({"installerArgs":["/foreign"]}))
                        .unwrap(),
                )
            }
            "image" => (),
            _ => unreachable!(),
        }
        assert!(
            validate_policy(
                &original,
                &policy,
                if field == "image" {
                    b"different executable"
                } else {
                    &image
                }
            )
            .is_err(),
            "{field}"
        );
    }
    let store = journal.witness_store().unwrap();
    let mut unsigned = original.clone();
    unsigned.signature = "invalid signature".into();
    assert!(verified_artifact::verify_in_store(&store, &unsigned, KEY).is_err());
    assert!(verified_artifact::verify_in_store(&store, &original, "journal-provided-key").is_err());
}

#[test]
fn dead_authorized_worker_is_unknown_not_refusal_or_replay_permission() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("store");
    let (journal, _prepared) = staged(&root, true);
    let original = journal.snapshot().unwrap().unwrap();
    std::fs::write(root.join("barrier-before-launch"), b"owned").unwrap();
    let mut worker = PendingDispatch::prepare(&journal, &original.candidate_id).unwrap();
    worker.authorize(&journal).unwrap();
    wait_path(&root.join("reached-before-launch"));
    worker.child.kill().unwrap();
    worker.child.wait().unwrap();
    for _ in 0..3 {
        assert_eq!(
            journal.reconcile_dispatch().unwrap().unwrap().phase,
            Phase::LaunchIntent
        );
        assert!(PendingDispatch::prepare(&journal, &original.candidate_id).is_err());
    }
    assert!(!root.join("installer-started").exists());
}

#[test]
fn product_transaction_dispatches_independently_and_pending_keeps_exit_quarantined() {
    use super::super::durable::WindowsRecovery;
    use crate::install_recovery::RecoveryState;
    use std::sync::atomic::Ordering;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("store");
    let owner = Arc::new(WindowsRecovery::new(root.clone()));
    let (bytes, signature) = fixture(false);
    let metadata = serde_json::json!({ "publicKey": KEY, "signature": signature });
    let count = Arc::new(AtomicUsize::new(0));
    let mut update = super::super::tests::refusing_update(&metadata, count.clone());
    let journal = owner.initialize(&update).unwrap();
    std::fs::write(root.join("owned-dispatch-test-purpose"), PURPOSE).unwrap();
    std::fs::write(root.join("installer-exit-code"), "1603").unwrap();
    let cleaned = count.clone();
    update.before_windows_install = Some(Arc::new(move || {
        cleaned.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    // Safety: the owned fixture's exit callback ALWAYS panics before process::exit.
    // The production transaction catches it and must preserve the accepted fence.
    update.windows_recovery = Some(owner.clone());
    assert!(update.install(bytes).is_err());
    let record = journal.snapshot().unwrap().unwrap();
    assert_eq!(record.phase, Phase::Accepted);
    let helper = ProcessWitness::attach(&record.dispatcher.as_ref().unwrap().helper).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(
        owner.pending(&update).unwrap().unwrap().state,
        RecoveryState::Blocked
    );
    assert!(owner.catalog.resume(&record.candidate_id).is_err());
    wait_path(&root.join("installer-started"));
    std::fs::write(root.join("installer-release"), b"release").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let pending = owner.pending(&update).unwrap().unwrap();
        assert_eq!(pending.state, RecoveryState::Blocked);
        if pending.phase == "installer_exited" {
            assert!(pending.message.unwrap().contains("1603"));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    while helper.observe() == Observation::Running {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(owner.catalog.resume(&record.candidate_id).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn authorized_live_worker_is_not_timed_out_or_replaced_before_os_outcome() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("store");
    let (journal, _prepared) = staged(&root, true);
    let record = journal.snapshot().unwrap().unwrap();
    std::fs::write(root.join("barrier-before-launch"), b"owned").unwrap();
    let worker = PendingDispatch::prepare(&journal, &record.candidate_id).unwrap();
    let observer = ProcessWitness::attach(&worker.ticket.helper).unwrap();
    worker.authorize(&journal).unwrap();
    wait_path(&root.join("reached-before-launch"));
    let owner = journal.clone();
    let (finished, result) = std::sync::mpsc::channel();
    let task = std::thread::spawn(move || {
        finished.send(worker.wait(&owner)).unwrap();
    });
    // Regression: a ten-second ACK deadline used to abandon a live Shell/UAC
    // handoff and strand the still-running App after eventual acceptance.
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(11)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(observer.observe(), Observation::Running);
    assert_eq!(
        journal.snapshot().unwrap().unwrap().phase,
        Phase::LaunchIntent
    );
    assert!(PendingDispatch::prepare(&journal, &record.candidate_id).is_err());
    std::fs::write(root.join("release-before-launch"), b"release owned handoff").unwrap();
    assert!(result
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .is_err());
    task.join().unwrap();
    assert_eq!(journal.snapshot().unwrap().unwrap(), record);
    let deadline = Instant::now() + Duration::from_secs(5);
    while observer.observe() == Observation::Running {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
}
