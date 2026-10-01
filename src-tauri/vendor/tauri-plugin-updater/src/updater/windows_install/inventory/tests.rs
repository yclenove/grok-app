use super::*;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "completion_tests.rs"]
mod completion_tests;
#[path = "failure_tests.rs"]
mod failure_tests;

fn sign(bytes: &[u8]) -> serde_json::Value {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("owned-sign-input");
    std::fs::write(&file, bytes).unwrap();
    let output = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sign-inventory.mjs"))
        .arg(file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn signed(value: &serde_json::Value) -> SignedInventory {
    let document = value.to_string();
    let signature = sign(document.as_bytes())["signature"]
        .as_str()
        .unwrap()
        .into();
    SignedInventory {
        document,
        signature,
    }
}

fn setup() -> (Update, Record, serde_json::Value) {
    let payload = b"MZ owned inventory fixture, not an executable";
    let metadata = sign(payload);
    let update = super::super::tests::refusing_update(&metadata, Arc::new(AtomicUsize::new(0)));
    let temp = tempfile::tempdir().unwrap();
    let journal = Arc::new(Journal::open(temp.path()).unwrap());
    let prepared = update
        .stage_durable(payload, &journal, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let record = journal.snapshot().unwrap().unwrap();
    drop(prepared);
    drop(journal);
    let name = std::env::current_exe()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let value = serde_json::json!({
        "schema":1,"domain":"grok-windows-install-inventory-v1",
        "app_name":record.binding.app_name,"version":record.version,"target":record.target,
        "arch":std::env::consts::ARCH,"kind":"Nsis","payload_sha256":hex(&record.payload_digest),
        "executable":name,"files":[
            {"path":name,"size":8,"sha256":hex(&journal::digest(b"test app"))},
            {"path":"resources/中文.txt","size":9,"sha256":hex(&journal::digest(b"test data"))}
        ]
    });
    (update, record, value)
}

fn image(root: &Path, value: &serde_json::Value) {
    std::fs::write(
        root.join(value["executable"].as_str().unwrap()),
        b"test app",
    )
    .unwrap();
    std::fs::create_dir(root.join("resources")).unwrap();
    std::fs::write(root.join("resources/中文.txt"), b"test data").unwrap();
}

#[test]
fn exact_signed_inventory_binds_payload_version_target_architecture_and_compiled_key() {
    let (update, record, value) = setup();
    let envelope = signed(&value);
    envelope.verify(&update.config.pubkey, &record).unwrap();
    assert!(envelope.verify("", &record).is_err());
    assert!(envelope.verify("other publisher", &record).is_err());
    for (field, wrong) in [
        ("schema", serde_json::json!(2)),
        ("domain", serde_json::json!("other-domain")),
        ("app_name", serde_json::json!("other-app")),
        ("version", serde_json::json!("9.9.9")),
        ("target", serde_json::json!("other-target")),
        ("arch", serde_json::json!("other-arch")),
        ("kind", serde_json::json!("Msi")),
        ("payload_sha256", serde_json::json!("00".repeat(32))),
    ] {
        let mut wrong_value = value.clone();
        wrong_value[field] = wrong;
        assert!(
            signed(&wrong_value)
                .verify(&update.config.pubkey, &record)
                .is_err(),
            "{field}"
        );
    }
    let mut changed = envelope;
    changed.document.push(' ');
    assert!(changed.verify(&update.config.pubkey, &record).is_err());
}

#[test]
fn unsafe_paths_devices_case_aliases_and_file_ancestors_are_rejected_before_io() {
    for path in [
        "",
        "../app.exe",
        "/app.exe",
        "C:/app.exe",
        "//host/share",
        "a\\b",
        "a:b",
        "a//b",
        "a/./b",
        "file.",
        "file ",
        "a/NUL.txt",
        "COM¹.log",
        "lpt9",
        "PRN",
        "CONIN$",
        "a?b",
        "a~1",
        "a\0b",
        "a\nb",
    ] {
        assert!(relative_path(path).is_err(), "{path:?}");
    }
    relative_path("资源/中文🙂.txt").unwrap();
    let (update, record, mut value) = setup();
    let duplicate = value["files"][1].clone();
    value["files"].as_array_mut().unwrap().push(duplicate);
    assert!(signed(&value)
        .verify(&update.config.pubkey, &record)
        .is_err());
    value["files"][2]["path"] = serde_json::json!("RESOURCES/中文.TXT");
    assert!(signed(&value)
        .verify(&update.config.pubkey, &record)
        .is_err());
    value["files"][2]["path"] = serde_json::json!("resources");
    assert!(signed(&value)
        .verify(&update.config.pubkey, &record)
        .is_err());
}

#[test]
fn installed_inventory_reads_exact_native_files_but_never_retires_or_replays() {
    let (update, record, value) = setup();
    let root = tempfile::tempdir().unwrap();
    image(root.path(), &value);
    let inventory = signed(&value);
    let observed = inventory
        .inspect(&update.config.pubkey, &record, root.path())
        .unwrap();
    assert_eq!(observed.files, 2);
    assert_eq!(observed.bytes, 17);
    assert_eq!(
        observed.document_sha256,
        journal::digest(inventory.document.as_bytes())
    );
    let mut accepted = record.clone();
    accepted.phase = Phase::Accepted;
    accepted.inventory = Some(inventory);
    assert!(
        observe_installed(&update, &accepted).is_err(),
        "no exit/current-version identity"
    );
    assert_eq!(accepted.candidate_id, record.candidate_id);
}

#[test]
fn producer_document_survives_exact_byte_signing_and_native_consumer_verification() {
    let (update, record, value) = setup();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("image");
    std::fs::create_dir(&root).unwrap();
    image(&root, &value);
    let payload = temp.path().join("payload.exe");
    std::fs::write(&payload, b"MZ owned inventory fixture, not an executable").unwrap();
    let output = temp.path().join("inventory.json");
    let result = Command::new("node")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../scripts/windows-install-inventory.mjs"),
        )
        .arg("--root")
        .arg(&root)
        .arg("--payload")
        .arg(&payload)
        .arg("--app-name")
        .arg(&record.binding.app_name)
        .arg("--version")
        .arg(&record.version)
        .arg("--target")
        .arg(&record.target)
        .arg("--arch")
        .arg(std::env::consts::ARCH)
        .arg("--kind")
        .arg("Nsis")
        .arg("--executable")
        .arg(value["executable"].as_str().unwrap())
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let document = std::fs::read_to_string(output).unwrap();
    let signature = sign(document.as_bytes())["signature"]
        .as_str()
        .unwrap()
        .into();
    let inventory = SignedInventory {
        document,
        signature,
    };
    assert_eq!(
        inventory
            .inspect(&update.config.pubkey, &record, &root)
            .unwrap()
            .files,
        2
    );
}

#[test]
fn native_junction_ancestor_cannot_redirect_an_authenticated_inventory() {
    let (update, record, value) = setup();
    let root = tempfile::tempdir().unwrap();
    image(root.path(), &value);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("中文.txt"), b"test data").unwrap();
    std::fs::remove_dir_all(root.path().join("resources")).unwrap();
    let result = Command::new("node")
        .args([
            "-e",
            "require('node:fs').symlinkSync(process.argv[1],process.argv[2],'junction')",
        ])
        .arg(outside.path())
        .arg(root.path().join("resources"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(signed(&value)
        .inspect(&update.config.pubkey, &record, root.path())
        .is_err());
    std::fs::remove_dir(root.path().join("resources")).unwrap();
    assert_eq!(
        std::fs::read(outside.path().join("中文.txt")).unwrap(),
        b"test data"
    );
}

#[test]
fn current_binary_file_match_remains_blocked_until_separate_completion_reconciliation() {
    let (mut update, record, mut value) = setup();
    let path = std::env::current_exe().unwrap();
    let file = journal::protected_read(&path).unwrap();
    let size = file.metadata().unwrap().len();
    let mut hash = Sha256::new();
    std::io::copy(&mut &file, &mut hash).unwrap();
    value["files"] = serde_json::json!([{"path":value["executable"],"size":size,"sha256":hex(&hash.finalize().into())}]);
    drop(file);
    update.raw_json = serde_json::json!({"windows_install_inventory":signed(&value)});
    let temp = tempfile::tempdir().unwrap();
    let journal = Arc::new(Journal::open(temp.path()).unwrap());
    let nonce = uuid::Uuid::new_v4().to_string();
    let prepared = update
        .stage_durable(
            b"MZ owned inventory fixture, not an executable",
            &journal,
            &nonce,
        )
        .unwrap();
    // Synthetic process facts isolate pending-state logic. Native file hashing
    // is real; this test does NOT claim a real installer has completed.
    let identity = journal::ProcessIdentity { pid: 1, created: 1 };
    journal
        .transition(&nonce, Phase::Prepared, Phase::LaunchIntent, None)
        .unwrap();
    journal
        .transition(
            &nonce,
            Phase::LaunchIntent,
            Phase::Accepted,
            Some(identity.clone()),
        )
        .unwrap();
    journal
        .record_process_exit(
            &nonce,
            &journal::ProcessExit {
                identity,
                exited: 2,
                code: 0,
            },
        )
        .unwrap();
    drop(prepared);
    drop(journal);
    update.current_version = record.version;
    let recovery = super::super::durable::WindowsRecovery::new(temp.path().to_owned());
    let pending = recovery.pending(&update).unwrap().unwrap();
    assert_eq!(
        pending.state,
        crate::install_recovery::RecoveryState::Blocked
    );
    assert_eq!(pending.candidate_id, nonce);
    assert!(pending
        .message
        .unwrap()
        .starts_with("Signed installed inventory matches 1 files"));
    assert!(recovery.catalog.resume(&nonce).is_err());
    assert_eq!(
        recovery
            .initialize(&update)
            .unwrap()
            .snapshot()
            .unwrap()
            .unwrap()
            .phase,
        Phase::Accepted
    );
}

#[test]
fn native_file_replacement_and_missing_directory_cannot_match_inventory() {
    let (update, record, value) = setup();
    let signed = signed(&value);
    let root = tempfile::tempdir().unwrap();
    image(root.path(), &value);
    std::fs::write(root.path().join("resources/中文.txt"), b"evil data").unwrap();
    assert!(signed
        .inspect(&update.config.pubkey, &record, root.path())
        .is_err());
    std::fs::remove_file(root.path().join("resources/中文.txt")).unwrap();
    std::fs::remove_dir(root.path().join("resources")).unwrap();
    assert!(signed
        .inspect(&update.config.pubkey, &record, root.path())
        .is_err());
    assert!(
        !root.path().join("resources").exists(),
        "verification must not create installation directories"
    );
}

#[test]
fn native_hardlink_alias_and_existing_writer_are_not_accepted_as_installed_files() {
    let (update, record, value) = setup();
    let signed = signed(&value);
    let root = tempfile::tempdir().unwrap();
    image(root.path(), &value);
    let path = root.path().join("resources/中文.txt");
    std::fs::hard_link(&path, root.path().join("owned-hardlink.txt")).unwrap();
    assert!(signed
        .inspect(&update.config.pubkey, &record, root.path())
        .is_err());
    std::fs::remove_file(root.path().join("owned-hardlink.txt")).unwrap();
    let writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    assert!(signed
        .inspect(&update.config.pubkey, &record, root.path())
        .is_err());
    drop(writer);
    signed
        .inspect(&update.config.pubkey, &record, root.path())
        .unwrap();
}

#[test]
fn signed_inventory_is_persisted_reverified_on_recovery_and_bound_into_dispatch_ticket() {
    let (mut update, record, value) = setup();
    let envelope = signed(&value);
    update.raw_json = serde_json::json!({"windows_install_inventory":envelope});
    let temp = tempfile::tempdir().unwrap();
    let journal = Arc::new(Journal::open(temp.path()).unwrap());
    let prepared = update
        .stage_durable(
            b"MZ owned inventory fixture, not an executable",
            &journal,
            &uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
    let saved = journal.snapshot().unwrap().unwrap();
    assert_eq!(saved.inventory, Some(envelope.clone()));
    let fingerprint = journal::dispatch::fingerprint(&saved).unwrap();
    drop(prepared);
    drop(journal);
    let reopened = Arc::new(Journal::open(temp.path()).unwrap());
    let saved_again = reopened.snapshot().unwrap().unwrap();
    let (lease, installer) = super::super::verified_artifact::verify_retained(
        &reopened,
        &saved_again,
        &update.config.pubkey,
    )
    .unwrap();
    drop(lease);
    drop(installer);
    let mut changed = saved_again;
    changed.inventory.as_mut().unwrap().document.push(' ');
    assert_ne!(
        journal::dispatch::fingerprint(&changed).unwrap(),
        fingerprint
    );
    assert!(super::super::verified_artifact::verify_retained(
        &reopened,
        &changed,
        &update.config.pubkey
    )
    .is_err());
    assert_eq!(saved.version, record.version);
}

#[test]
fn platform_inventory_is_selected_by_exact_checked_artifact_and_conflicts_are_rejected() {
    let (mut update, record, value) = setup();
    let envelope = signed(&value);
    let platform = serde_json::json!({"url":update.download_url,"signature":update.signature,"windows_install_inventory":envelope});
    update.raw_json = serde_json::json!({"platforms":{"windows-x86_64":platform,"windows-x86_64-nsis":platform,"darwin-aarch64":{"url":"https://example.invalid/other","signature":"other","windows_install_inventory":{}}}});
    assert_eq!(from_update(&update, &record).unwrap(), Some(envelope));
    update.raw_json["platforms"]["windows-x86_64-nsis"]
        .as_object_mut()
        .unwrap()
        .remove("windows_install_inventory");
    assert!(from_update(&update, &record).is_err());
    update.raw_json = serde_json::json!({"windows_install_inventory":null});
    assert!(from_update(&update, &record).is_err());
    update.raw_json = serde_json::Value::Null;
    assert!(
        from_update(&update, &record).unwrap().is_none(),
        "legacy candidate may not gain inventory evidence"
    );
}

#[test]
fn invalid_inventory_staging_is_terminal_without_cleanup_and_valid_inventory_keeps_same_candidate()
{
    let (mut update, record, value) = setup();
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let guard_calls = calls.clone();
    update.before_windows_install = Some(Arc::new(move || {
        guard_calls.fetch_add(1, Ordering::SeqCst);
        Err("owned test refuses cleanup".into())
    }));
    let recovery = Arc::new(super::super::durable::WindowsRecovery::new(
        temp.path().join("store"),
    ));
    update.windows_recovery = Some(recovery.clone());
    let mut wrong = value.clone();
    wrong["version"] = serde_json::json!("8.8.8");
    update.raw_json = serde_json::json!({"windows_install_inventory":signed(&wrong)});
    let payload = b"MZ owned inventory fixture, not an executable";
    assert!(update.install_windows_checked(payload).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(recovery
        .initialize(&update)
        .unwrap()
        .snapshot()
        .unwrap()
        .is_none());
    assert_eq!(
        recovery.catalog.snapshot().unwrap().unwrap().state,
        crate::install_recovery::RecoveryState::Failed
    );
    update.windows_install = Default::default();
    update.raw_json = serde_json::json!({"windows_install_inventory":signed(&value)});
    assert!(update.install_windows_checked(payload).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let journal = recovery.initialize(&update).unwrap();
    assert_eq!(journal.snapshot().unwrap().unwrap().version, record.version);
    let nonce = journal.snapshot().unwrap().unwrap().candidate_id;
    assert!(update.install_windows_checked(payload).is_err());
    assert_eq!(journal.snapshot().unwrap().unwrap().candidate_id, nonce);
}
