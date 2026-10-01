use super::completion_tests::{encode, fields, ready, ready_with_contract};
use super::*;
use crate::install_recovery::RecoveryState;
use crate::updater::windows_install::{completion, durable::WindowsRecovery, failure};

fn contract() -> serde_json::Value {
    serde_json::json!({"protocol":completion::PROTOCOL,"bundle_id":"com.grokapp.desktop","install_scope":"currentUser","failure_protocol":failure::PROTOCOL})
}

fn bytes(record: &Record, outcome: &str) -> Vec<u8> {
    let mut values = fields(record);
    values[0] = failure::PROTOCOL.into();
    values[9] = outcome.into();
    encode(&values)
}

#[test]
fn failure_contract_requires_exact_signed_opt_in_and_never_downgrades_null() {
    let (update, record, mut value) = setup();
    value["completion"] = contract();
    let accepted = signed(&value)
        .completion(&update.config.pubkey, &record)
        .unwrap()
        .unwrap();
    assert_eq!(
        accepted.failure_protocol.as_deref(),
        Some(failure::PROTOCOL)
    );
    for invalid in [
        serde_json::Value::Null,
        serde_json::json!(false),
        serde_json::json!([]),
        serde_json::json!("unknown"),
    ] {
        value["completion"]["failure_protocol"] = invalid;
        assert!(signed(&value)
            .completion(&update.config.pubkey, &record)
            .is_err());
    }
    value["completion"] = contract();
    let mut tampered = signed(&value);
    tampered.document = tampered
        .document
        .replace(failure::PROTOCOL, "grok-nsis-install-failed-v2");
    assert!(tampered.completion(&update.config.pubkey, &record).is_err());
    value["completion"]
        .as_object_mut()
        .unwrap()
        .remove("failure_protocol");
    assert!(signed(&value)
        .completion(&update.config.pubkey, &record)
        .unwrap()
        .unwrap()
        .failure_protocol
        .is_none());
}

#[test]
fn native_failure_evidence_remains_blocked_across_reopen_and_cannot_replay_or_replace() {
    for (outcome, phase) in [
        ("failed", "installer_failed"),
        ("cancelled", "installer_cancelled"),
    ] {
        let (mut update, record, temp, _) = ready_with_contract(2, contract());
        let receipt = failure::receipt_path(temp.path(), &record).unwrap();
        std::fs::write(&receipt, bytes(&record, outcome)).unwrap();
        for _ in 0..2 {
            let recovery = Arc::new(WindowsRecovery::new(temp.path().to_owned()));
            let pending = recovery.pending(&update).unwrap().unwrap();
            assert_eq!(pending.state, RecoveryState::Blocked);
            assert_eq!(pending.phase, phase);
            assert_eq!(pending.candidate_id, record.candidate_id);
            assert!(pending
                .message
                .unwrap()
                .contains("rollback/repair remains unverified"));
            assert!(recovery.catalog.resume(&record.candidate_id).is_err());
            update.windows_recovery = Some(recovery.clone());
            assert!(update
                .install_windows_checked(b"not another candidate")
                .is_err());
            let durable = recovery
                .initialize(&update)
                .unwrap()
                .snapshot()
                .unwrap()
                .unwrap();
            assert_eq!(durable.candidate_id, record.candidate_id);
            assert_eq!(durable.phase, Phase::Accepted);
            assert!(!temp
                .path()
                .join(format!("{}.completed.dpapi", record.candidate_id))
                .exists());
            update.windows_recovery = None;
        }
        std::fs::remove_file(receipt).unwrap();
        let missing = WindowsRecovery::new(temp.path().to_owned())
            .pending(&update)
            .unwrap()
            .unwrap();
        assert_eq!(missing.state, RecoveryState::Blocked);
        assert_eq!(
            missing.phase, "installer_exited",
            "missing callback must not retain an inferred failure label"
        );
    }
}

#[test]
fn failure_parser_rejects_every_wrong_field_and_missing_exact_nonzero_exit() {
    let (_, record, _temp, _) = ready_with_contract(2, contract());
    let contract: completion::Contract = serde_json::from_value(contract()).unwrap();
    let verify = |data: &[u8], record: &Record| {
        completion::verify_callback(
            data,
            record,
            &contract,
            failure::PROTOCOL,
            &["failed", "cancelled"],
            false,
        )
    };
    let valid = bytes(&record, "failed");
    assert_eq!(verify(&valid, &record).unwrap(), "failed");
    let mut raw = fields(&record);
    raw[0] = failure::PROTOCOL.into();
    raw[9] = "failed".into();
    for index in 0..10 {
        let mut changed = raw.clone();
        changed[index].push('x');
        assert!(verify(&encode(&changed), &record).is_err(), "field {index}");
    }
    for invalid in [
        valid[..valid.len() - 2].to_vec(),
        valid[2..].to_vec(),
        [valid.clone(), vec![0]].concat(),
        vec![0xff, 0xfe, 0x00, 0xd8],
        vec![0; 32770],
    ] {
        assert!(verify(&invalid, &record).is_err());
    }
    let mut wrong = record.clone();
    wrong.process_exit.as_mut().unwrap().code = 0;
    assert!(verify(&valid, &wrong).is_err());
    wrong = record.clone();
    wrong.process_exit.as_mut().unwrap().identity.created += 1;
    assert!(verify(&valid, &wrong).is_err());
    wrong = record.clone();
    wrong.process_exit = None;
    assert!(verify(&valid, &wrong).is_err());
    wrong = record.clone();
    wrong.phase = Phase::LaunchIntent;
    assert!(verify(&valid, &wrong).is_err());
    wrong = record.clone();
    wrong.process = None;
    assert!(verify(&valid, &wrong).is_err());
}

#[test]
fn failure_is_not_inferred_for_legacy_or_forged_callbacks_and_conflicts_never_complete() {
    let (update, record, temp, _) = ready(2);
    std::fs::write(
        failure::receipt_path(temp.path(), &record).unwrap(),
        bytes(&record, "failed"),
    )
    .unwrap();
    assert!(failure::observe(&update, &record, temp.path())
        .unwrap()
        .is_none());
    for code in [0, 2] {
        let (update, record, temp, _) = ready_with_contract(code, contract());
        let path = failure::receipt_path(temp.path(), &record).unwrap();
        std::fs::write(&path, bytes(&record, "failed")).unwrap();
        std::fs::write(
            completion::receipt_path(temp.path(), &record).unwrap(),
            encode(&fields(&record)),
        )
        .unwrap();
        assert!(failure::observe(&update, &record, temp.path()).is_err());
        assert!(
            completion::reconcile::<()>(&update, &record, temp.path(), |_| panic!(
                "conflict cannot retire"
            ))
            .is_err()
        );
        let pending = WindowsRecovery::new(temp.path().to_owned())
            .pending(&update)
            .unwrap()
            .unwrap();
        assert_eq!(pending.state, RecoveryState::Blocked);
        assert_eq!(pending.phase, "installer_exited");
        assert!(pending
            .message
            .unwrap()
            .contains("failure callback not verified"));
    }
}

#[test]
fn failure_callback_read_lease_rejects_hardlinks_directories_oversize_and_wrong_app() {
    let (mut update, record, temp, _) = ready_with_contract(2, contract());
    let path = failure::receipt_path(temp.path(), &record).unwrap();
    for invalid in [b"truncated".to_vec(), vec![0; 32770]] {
        std::fs::write(&path, invalid).unwrap();
        assert!(failure::observe(&update, &record, temp.path()).is_err());
    }
    std::fs::write(&path, bytes(&record, "failed")).unwrap();
    let alias = temp.path().join("owned-alias");
    std::fs::hard_link(&path, &alias).unwrap();
    assert!(failure::observe(&update, &record, temp.path()).is_err());
    std::fs::remove_file(alias).unwrap();
    assert_eq!(
        failure::observe(&update, &record, temp.path()).unwrap(),
        Some(failure::Outcome::Failed)
    );
    update.app_name.push('x');
    assert!(failure::observe(&update, &record, temp.path()).is_err());
    update.app_name.pop();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(failure::observe(&update, &record, temp.path()).is_err());
}

#[test]
fn preexisting_failure_receipt_refuses_launch_before_cleanup() {
    let (update, mut record, mut value) = setup();
    value["completion"] = contract();
    record.inventory = Some(signed(&value));
    let temp = tempfile::tempdir().unwrap();
    let plan = || {
        super::super::launch_plan::LaunchPlan::new(
            &WindowsUpdaterType::nsis(PathBuf::from(r"C:\owned.exe"), None),
            Default::default(),
            &[],
            &[],
        )
        .unwrap()
    };
    completion::bind_plan(plan(), temp.path(), &record, &update.config.pubkey).unwrap();
    let path = failure::receipt_path(temp.path(), &record).unwrap();
    std::fs::write(&path, b"stale failure callback").unwrap();
    assert!(completion::bind_plan(plan(), temp.path(), &record, &update.config.pubkey).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"stale failure callback");
}

#[test]
#[ignore = "requires actual inert NSIS failure and MUI abort fixture callbacks"]
fn native_nsis_failure_callbacks_match_production_verifier() {
    use std::os::windows::ffi::OsStrExt;
    for (variable, outcome) in [
        ("GROK_NSIS_NATIVE_FAILURE_FIXTURE", "failed"),
        ("GROK_NSIS_NATIVE_CANCEL_FIXTURE", "cancelled"),
    ] {
        let fixture =
            std::env::var_os(variable).expect("run scripts/windows-nsis-failure.test.mjs first");
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
        let field = |name: &str| evidence[name].as_str().unwrap();
        let native = &evidence["process"];
        assert_eq!(native["handleHeld"], true);
        assert_ne!(native["exitCode"], 0);
        assert_eq!(field("outcome"), outcome);
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
            code: native["exitCode"].as_u64().unwrap().try_into().unwrap(),
        });
        let contract = completion::Contract {
            protocol: completion::PROTOCOL.into(),
            bundle_id: field("bundleId").into(),
            install_scope: "currentUser".into(),
            failure_protocol: Some(failure::PROTOCOL.into()),
        };
        contract.validate(record.kind).unwrap();
        let callback = std::fs::read(field("receipt")).unwrap();
        assert_eq!(hex(&journal::digest(&callback)), field("receiptSha256"));
        assert_eq!(
            completion::verify_callback(
                &callback,
                &record,
                &contract,
                failure::PROTOCOL,
                &["failed", "cancelled"],
                false
            )
            .unwrap(),
            outcome
        );
        record.process_exit.as_mut().unwrap().code = 0;
        assert!(completion::verify_callback(
            &callback,
            &record,
            &contract,
            failure::PROTOCOL,
            &["failed", "cancelled"],
            false
        )
        .is_err());
    }
}
