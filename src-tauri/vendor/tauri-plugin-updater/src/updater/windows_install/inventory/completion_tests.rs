use super::*;
use crate::updater::windows_install::{
    completion,
    durable::WindowsRecovery,
    process_witness::{tests::OwnedChild, Observation, ProcessWitness},
};

const PAYLOAD: &[u8] = b"MZ owned inventory fixture, not an executable";

#[test]
#[ignore = "requires an explicitly executed private-hive NSIS fixture, not synthetic callback bytes"]
fn native_nsis_callback_from_private_hive_matches_production_verifier() {
    use std::os::windows::ffi::OsStrExt;
    let fixture = std::env::var_os("GROK_NSIS_NATIVE_CALLBACK_FIXTURE").expect(
        "run scripts/windows-nsis-private-hive.test.mjs and supply its native-callback.json",
    );
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let field = |name: &str| evidence[name].as_str().unwrap();
    let native = &evidence["process"];
    assert_eq!(native["handleHeld"], true);
    assert_eq!(native["exitCode"], 0);
    let identity = journal::ProcessIdentity {
        pid: native["pid"].as_u64().unwrap().try_into().unwrap(),
        created: native["created"].as_str().unwrap().parse().unwrap(),
    };
    let (_, mut record, _) = setup();
    record.candidate_id = field("nonce").into();
    record.version = field("version").into();
    record.binding.app_name = field("product").into();
    record.binding.executable = OsStr::new(field("executable")).encode_wide().collect();
    record.phase = Phase::Accepted;
    record.process = Some(identity.clone());
    record.process_exit = Some(journal::ProcessExit {
        identity,
        exited: native["exited"].as_str().unwrap().parse().unwrap(),
        code: 0,
    });
    let contract = completion::Contract {
        protocol: completion::PROTOCOL.into(),
        bundle_id: field("bundleId").into(),
        install_scope: "currentUser".into(),
        failure_protocol: None,
    };
    contract.validate(record.kind).unwrap();
    let receipt = std::fs::read(field("receipt")).unwrap();
    assert_eq!(hex(&journal::digest(&receipt)), field("receiptSha256"));
    // This checks the production parser against actual callback bytes and an
    // independently held native process identity. It does NOT grant journal
    // retirement or claim that the inert fixture is an installed signed App.
    completion::verify_receipt(&receipt, &record, &contract).unwrap();
    let mut wrong = record.clone();
    wrong.candidate_id = uuid::Uuid::new_v4().to_string();
    assert!(completion::verify_receipt(&receipt, &wrong, &contract).is_err());
    let mut wrong = record.clone();
    wrong.process_exit.as_mut().unwrap().identity.created += 1;
    assert!(completion::verify_receipt(&receipt, &wrong, &contract).is_err());
    let mut wrong = record.clone();
    wrong.process_exit.as_mut().unwrap().code = 1;
    assert!(completion::verify_receipt(&receipt, &wrong, &contract).is_err());
    assert!(completion::verify_receipt(&receipt[..receipt.len() - 2], &record, &contract).is_err());
}

fn contract() -> serde_json::Value {
    serde_json::json!({"protocol":completion::PROTOCOL,"bundle_id":"com.grokapp.desktop","install_scope":"currentUser"})
}

#[test]
fn rejected_signed_completion_contract_is_terminal_before_publication() {
    use crate::install_recovery::RecoveryState;
    for (field, invalid) in [
        ("protocol", "unsupported-v2"),
        ("failure_protocol", "unsupported-failure-v2"),
        ("install_scope", "perMachine"),
        ("bundle_id", ""),
        ("bundle_id", "com.grokapp/desktop"),
    ] {
        let (_, _, mut value) = setup();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut update = super::super::super::tests::refusing_update(&sign(PAYLOAD), calls.clone());
        let temp = tempfile::tempdir().unwrap();
        let recovery = Arc::new(WindowsRecovery::new(temp.path().join("store")));
        update.windows_recovery = Some(recovery.clone());
        value["completion"] = contract();
        value["completion"][field] = invalid.into();
        update.raw_json = serde_json::json!({"windows_install_inventory":signed(&value)});
        let result = update.install_windows_checked(PAYLOAD);
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!update.windows_install.has_prepared());
        assert!(recovery
            .initialize(&update)
            .unwrap()
            .snapshot()
            .unwrap()
            .is_none());
        let failed = recovery.catalog.snapshot().unwrap().unwrap();
        assert_eq!(
            failed.state,
            RecoveryState::Failed,
            "{field}: {invalid}; {result:?}"
        );
        assert_eq!(failed.phase, "staging");
        assert!(matches!(result, Err(Error::WindowsInstallPreflight(_))));
        assert_eq!(recovery.pending(&update).unwrap(), Some(failed.clone()));
        assert!(recovery.catalog.resume(&failed.candidate_id).is_err());

        // Correct the signed contract, keeping the same authenticated payload.
        // The test guard refuses cleanup, so no fixture installer can launch.
        value["completion"] = contract();
        update.raw_json = serde_json::json!({"windows_install_inventory":signed(&value)});
        let result = update.install_windows_checked(PAYLOAD);
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
        let next = recovery.pending(&update).unwrap().unwrap();
        assert_eq!(next.state, RecoveryState::Retryable);
        assert_ne!(next.candidate_id, failed.candidate_id);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(recovery.catalog.resume(&failed.candidate_id).is_err());
        // Once published, a cleanup refusal keeps the same transaction. The
        // preflight fix must not release or replace this recoverable owner.
        let result = update.install_windows_checked(PAYLOAD);
        assert!(matches!(
            result,
            Err(Error::WindowsInstallPending {
                phase: "cleanup",
                ..
            })
        ));
        let retry = recovery.pending(&update).unwrap().unwrap();
        assert_eq!(retry.candidate_id, next.candidate_id);
        assert_eq!(retry.state, RecoveryState::Retryable);
        let record = recovery
            .initialize(&update)
            .unwrap()
            .snapshot()
            .unwrap()
            .unwrap();
        assert_eq!(record.candidate_id, next.candidate_id);
        assert_eq!(record.phase, Phase::Prepared);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

// Actual owned native process exit + actual current-test-image hashing. Callback
// bytes are a fixture, NOT a claim that a real installer/registry was executed.
pub(super) fn ready(code: i32) -> (Update, Record, tempfile::TempDir, serde_json::Value) {
    ready_with_contract(code, contract())
}

pub(super) fn ready_with_contract(
    code: i32,
    contract: serde_json::Value,
) -> (Update, Record, tempfile::TempDir, serde_json::Value) {
    let (mut update, original, mut value) = setup();
    let path = std::env::current_exe().unwrap();
    let mut file = journal::protected_read(&path).unwrap();
    let size = file.metadata().unwrap().len();
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash).unwrap();
    drop(file);
    value["files"] = serde_json::json!([{"path":value["executable"],"size":size,"sha256":hex(&hash.finalize().into())}]);
    value["completion"] = contract;
    update.raw_json = serde_json::json!({"windows_install_inventory":signed(&value)});
    let temp = tempfile::tempdir().unwrap();
    let journal = Arc::new(Journal::open(temp.path()).unwrap());
    let nonce = uuid::Uuid::new_v4().to_string();
    let prepared = update.stage_durable(PAYLOAD, &journal, &nonce).unwrap();
    let mut child = OwnedChild::spawn(code);
    let identity = child.identity();
    let witness = ProcessWitness::from_handle(child.handle(), &identity).unwrap();
    journal
        .transition(&nonce, Phase::Prepared, Phase::LaunchIntent, None)
        .unwrap();
    journal
        .transition(&nonce, Phase::LaunchIntent, Phase::Accepted, Some(identity))
        .unwrap();
    child.finish();
    let Observation::Exited(exit) = witness.observe() else {
        panic!("owned process has not exited")
    };
    journal.record_process_exit(&nonce, &exit).unwrap();
    let record = journal.snapshot().unwrap().unwrap();
    drop(prepared);
    drop(journal);
    update.current_version = original.version;
    (update, record, temp, value)
}

pub(super) fn fields(record: &Record) -> Vec<String> {
    let executable = PathBuf::from(OsString::from_wide(&record.binding.executable));
    let process = record.process.as_ref().unwrap();
    vec![
        completion::PROTOCOL.into(),
        record.candidate_id.clone(),
        record.version.clone(),
        record.binding.app_name.clone(),
        executable.file_name().unwrap().to_str().unwrap().into(),
        "com.grokapp.desktop".into(),
        executable.parent().unwrap().to_str().unwrap().into(),
        process.pid.to_string(),
        process.created.to_string(),
        "complete".into(),
    ]
}
pub(super) fn encode(fields: &[String]) -> Vec<u8> {
    let text = fields.join("\r\n") + "\r\n";
    [0xfeff]
        .into_iter()
        .chain(text.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect()
}
fn write_receipt(root: &Path, record: &Record) -> PathBuf {
    let path = completion::receipt_path(root, record).unwrap();
    std::fs::write(&path, encode(&fields(record))).unwrap();
    path
}

#[test]
fn completion_contract_requires_publisher_signature_exact_protocol_and_nsis_scope() {
    let (update, record, mut value) = setup();
    assert!(signed(&value)
        .completion(&update.config.pubkey, &record)
        .unwrap()
        .is_none());
    value["completion"] = contract();
    assert!(signed(&value)
        .completion(&update.config.pubkey, &record)
        .unwrap()
        .is_some());
    for wrong in [
        serde_json::Value::Null,
        serde_json::json!("untrusted"),
        serde_json::json!({"protocol":"unknown","bundle_id":"com.grokapp.desktop","install_scope":"currentUser"}),
        serde_json::json!({"protocol":completion::PROTOCOL,"bundle_id":"bad\nline","install_scope":"currentUser"}),
        serde_json::json!({"protocol":completion::PROTOCOL,"bundle_id":"com.grokapp.desktop","install_scope":"allUsers"}),
        serde_json::json!({"protocol":completion::PROTOCOL,"bundle_id":"com.grokapp.desktop","install_scope":"currentUser","extra":true}),
    ] {
        value["completion"] = wrong;
        assert!(signed(&value)
            .verify(&update.config.pubkey, &record)
            .is_err());
    }
    value["completion"] = contract();
    let mut mismatch = signed(&value);
    mismatch.document.push(' ');
    assert!(mismatch.completion(&update.config.pubkey, &record).is_err());
    let mut msi = record.clone();
    msi.kind = Kind::Msi;
    value["kind"] = serde_json::json!("Msi");
    assert!(signed(&value)
        .completion(&update.config.pubkey, &msi)
        .is_err());
}

#[test]
fn completion_receipt_rejects_stale_identity_wrong_layout_partial_and_nonzero_exit() {
    let (update, record, temp, _) = ready(0);
    let contract = record
        .inventory
        .as_ref()
        .unwrap()
        .completion(&update.config.pubkey, &record)
        .unwrap()
        .unwrap();
    let valid = encode(&fields(&record));
    completion::verify_receipt(&valid, &record, &contract).unwrap();
    for index in 0..10 {
        let mut changed = fields(&record);
        changed[index].push('x');
        assert!(
            completion::verify_receipt(&encode(&changed), &record, &contract).is_err(),
            "field {index}"
        );
    }
    for invalid in [
        valid[..valid.len() - 2].to_vec(),
        valid[2..].to_vec(),
        [valid.clone(), vec![0]].concat(),
        vec![0xff, 0xfe, 0x00, 0xd8],
    ] {
        assert!(completion::verify_receipt(&invalid, &record, &contract).is_err());
    }
    let mut wrong = record.clone();
    wrong.process_exit.as_mut().unwrap().code = 1;
    assert!(completion::verify_receipt(&valid, &wrong, &contract).is_err());
    wrong = record.clone();
    wrong.process = None;
    assert!(completion::verify_receipt(&valid, &wrong, &contract).is_err());
    let recovery = WindowsRecovery::new(temp.path().to_owned());
    assert_eq!(
        recovery.pending(&update).unwrap().unwrap().state,
        crate::install_recovery::RecoveryState::Blocked
    );
    assert!(recovery
        .initialize(&update)
        .unwrap()
        .snapshot()
        .unwrap()
        .is_some());
}

#[test]
fn completion_retires_exact_journal_durably_and_releases_next_candidate_without_replay() {
    let (mut update, record, temp, mut value) = ready(0);
    write_receipt(temp.path(), &record);
    let recovery = Arc::new(WindowsRecovery::new(temp.path().to_owned()));
    let completed = recovery.pending(&update).unwrap().unwrap();
    assert_eq!(
        completed.state,
        crate::install_recovery::RecoveryState::Completed
    );
    assert_eq!(completed.candidate_id, record.candidate_id);
    assert_eq!(completed.phase, "completed");
    assert!(completed.message.is_none());
    assert!(recovery.catalog.resume(&record.candidate_id).is_err());
    assert!(recovery
        .initialize(&update)
        .unwrap()
        .snapshot()
        .unwrap()
        .is_none());
    assert!(temp
        .path()
        .join(format!("{}.completed.dpapi", record.candidate_id))
        .is_file());
    drop(recovery);
    let recovery = Arc::new(WindowsRecovery::new(temp.path().to_owned()));
    assert!(
        recovery.pending(&update).unwrap().is_none(),
        "a new App process must not restore retired nonce"
    );
    let historical = recovery
        .pending_for(&update, Some(&record.candidate_id))
        .unwrap()
        .unwrap();
    assert_eq!(
        historical.state,
        crate::install_recovery::RecoveryState::Completed
    );
    assert_eq!(historical.candidate_id, record.candidate_id);
    update.version = "9.9.9".into();
    value["version"] = serde_json::json!(update.version);
    update.raw_json = serde_json::json!({"windows_install_inventory":signed(&value)});
    update.windows_recovery = Some(recovery.clone());
    // The existing fixture guard refuses cleanup; no installer is dispatched.
    assert!(update.install_windows_checked(PAYLOAD).is_err());
    let next = recovery.catalog.snapshot().unwrap().unwrap();
    assert_ne!(next.candidate_id, record.candidate_id);
    assert_eq!(next.version, "9.9.9");
    assert_eq!(
        next.state,
        crate::install_recovery::RecoveryState::Retryable
    );
    assert_eq!(
        recovery
            .initialize(&update)
            .unwrap()
            .snapshot()
            .unwrap()
            .unwrap()
            .phase,
        Phase::Prepared
    );
    // A slow window can finish observing its original completion without
    // adopting the later candidate or releasing that candidate's cleanup block.
    assert_eq!(
        recovery
            .pending_for(&update, Some(&record.candidate_id))
            .unwrap()
            .unwrap(),
        historical
    );
    assert_eq!(
        recovery
            .pending_for(&update, Some(&next.candidate_id))
            .unwrap()
            .unwrap(),
        next
    );
    assert!(recovery
        .pending_for(&update, Some("../not-a-candidate"))
        .is_err());
    assert_eq!(recovery.catalog.snapshot().unwrap().unwrap(), next);
    std::fs::write(
        completion::receipt_path(temp.path(), &record).unwrap(),
        b"owned historical corruption",
    )
    .unwrap();
    assert!(recovery
        .pending_for(&update, Some(&record.candidate_id))
        .is_err());
    assert_eq!(recovery.catalog.snapshot().unwrap().unwrap(), next);
}

#[test]
fn completion_failed_terminal_publication_never_releases_original_or_next_update() {
    let (update, record, temp, _) = ready(0);
    write_receipt(temp.path(), &record);
    let path = temp
        .path()
        .join(format!("{}.completed.dpapi", record.candidate_id));
    std::fs::write(&path, b"owned corrupt publication fixture").unwrap();
    let recovery = WindowsRecovery::new(temp.path().to_owned());
    assert_eq!(
        recovery.pending(&update).unwrap().unwrap().state,
        crate::install_recovery::RecoveryState::Blocked
    );
    assert_eq!(
        recovery
            .initialize(&update)
            .unwrap()
            .snapshot()
            .unwrap()
            .unwrap(),
        record
    );
    assert!(recovery.catalog.resume(&record.candidate_id).is_err());
    // Repair ONLY this test-owned corruption. Production has no clear/reset IPC.
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        recovery.pending(&update).unwrap().unwrap().state,
        crate::install_recovery::RecoveryState::Completed
    );
}

#[test]
fn completion_file_and_receipt_leases_extend_through_durable_publication() {
    let (update, record, value) = setup();
    let temp = tempfile::tempdir().unwrap();
    image(temp.path(), &value);
    let envelope = signed(&value);
    envelope
        .with_inspected(&update.config.pubkey, &record, temp.path(), |_| {
            assert!(std::fs::OpenOptions::new()
                .write(true)
                .open(temp.path().join("resources/中文.txt"))
                .is_err());
            Ok(())
        })
        .unwrap();
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temp.path().join("resources/中文.txt"))
        .is_ok());
    let (update, record, temp, _) = ready(0);
    let receipt = write_receipt(temp.path(), &record);
    completion::reconcile(&update, &record, temp.path(), |_| {
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(&receipt)
            .is_err());
        Ok(())
    })
    .unwrap()
    .unwrap();
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(receipt)
        .is_ok());
}

#[test]
fn completion_reopens_after_terminal_archive_but_failed_active_slot_clear() {
    let (update, record, temp, _) = ready(0);
    write_receipt(temp.path(), &record);
    let active = temp.path().join("active.dpapi");
    let before = std::fs::read(&active).unwrap();
    let recovery = WindowsRecovery::new(temp.path().to_owned());
    recovery.initialize(&update).unwrap();
    // Native sharing violation only on the final atomic active-slot replacement.
    // Terminal DPAPI receipt is already durable; no fault-injection production API.
    let deny_replace = journal::protected_read(&active).unwrap();
    let pending = recovery.pending(&update).unwrap().unwrap();
    assert_eq!(
        pending.state,
        crate::install_recovery::RecoveryState::Blocked
    );
    assert!(pending.message.unwrap().contains("completion not verified"));
    assert_eq!(std::fs::read(&active).unwrap(), before);
    let terminal = temp
        .path()
        .join(format!("{}.completed.dpapi", record.candidate_id));
    let archive_before = std::fs::read(&terminal).unwrap();
    assert!(!archive_before.is_empty());
    assert!(recovery.catalog.resume(&record.candidate_id).is_err());
    drop(deny_replace);
    drop(recovery);
    // Reopen authoritative disk state and redo all checks, not merely existence
    // of the archive. The same immutable completion can finish idempotently.
    let reopened = WindowsRecovery::new(temp.path().to_owned());
    assert_eq!(
        reopened.pending(&update).unwrap().unwrap().state,
        crate::install_recovery::RecoveryState::Completed
    );
    assert_eq!(std::fs::read(terminal).unwrap(), archive_before);
    assert!(reopened
        .initialize(&update)
        .unwrap()
        .snapshot()
        .unwrap()
        .is_none());
}

#[test]
fn completion_preexisting_path_and_hardlinked_receipt_fail_closed() {
    let (update, mut record, mut value) = setup();
    value["completion"] = contract();
    record.inventory = Some(signed(&value));
    let temp = tempfile::tempdir().unwrap();
    let path = completion::receipt_path(temp.path(), &record).unwrap();
    std::fs::write(path, b"stale callback must not be reused").unwrap();
    let launch = super::super::launch_plan::LaunchPlan::new(
        &WindowsUpdaterType::nsis(PathBuf::from(r"C:\owned.exe"), None),
        Default::default(),
        &[],
        &[],
    )
    .unwrap();
    assert!(completion::bind_plan(launch, temp.path(), &record, &update.config.pubkey).is_err());
    let (update, record, temp, _) = ready(0);
    let receipt = write_receipt(temp.path(), &record);
    std::fs::hard_link(receipt, temp.path().join("receipt-alias")).unwrap();
    assert!(
        completion::reconcile::<()>(&update, &record, temp.path(), |_| {
            panic!("multiply linked receipt must not grant completion")
        })
        .is_err()
    );
}
