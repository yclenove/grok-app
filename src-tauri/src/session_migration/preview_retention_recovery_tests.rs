//! Recovery probes use real enqueue/child exits; only negative corruption is injected.
use super::*;
use std::path::PathBuf;

fn crashed(home: &IsolatedAppHome, mode: &str) -> serde_json::Value {
    let (mut child, signal) = start_child(home, mode);
    assert_eq!(
        child.0.wait().unwrap().code(),
        Some(if mode.starts_with("retention-before-") {
            73
        } else {
            74
        })
    );
    serde_json::from_slice(&std::fs::read(signal).unwrap()).unwrap()
}

fn recovery_file() -> PathBuf {
    let entries: Vec<_> = std::fs::read_dir(catalog::imports_dir().join("preview-retention"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1);
    entries[0].clone()
}

fn paths(report: &serde_json::Value) -> Vec<PathBuf> {
    serde_json::from_value(report["paths"].clone()).unwrap()
}

fn retry_input(report: &serde_json::Value) -> (JobRecord, Vec<CommitItem>) {
    let job: JobRecord = serde_json::from_value(report["job"].clone()).unwrap();
    let items = report["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|input| job_control::decode_input(input.as_str().unwrap(), &job).unwrap())
        .collect();
    (job, items)
}

#[test]
fn multi_input_crash_repairs_before_new_acceptance_without_provider_sources() {
    let home = IsolatedAppHome::acquire("retention-recovery-multi");
    let report = crashed(&home, "retention-before-multi");
    let conn = catalog::open_catalog().unwrap();
    let recovery = recovery_file();
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&recovery).unwrap()).unwrap();
    assert_eq!(receipt["entries"].as_array().unwrap().len(), 2);
    let raw = std::fs::read_to_string(&recovery).unwrap();
    assert!(!raw.contains("synthetic concurrency input"));
    assert!(!raw.contains("rootPath"));
    let (requested, items) = retry_input(&report);
    let accepted = job_control::enqueue(requested, items).unwrap();
    assert!(!recovery.exists());
    for path in paths(&report) {
        assert!(
            jobs::read_preview_lease(&path)
                .unwrap()
                .unwrap()
                .retained_by_job
        );
    }
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        jobs::get_job(&conn, &accepted.job_id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
    for path in paths(&report) {
        assert!(!path.exists());
    }
}

#[test]
fn lost_ack_same_request_returns_before_reinspecting_protected_input() {
    let home = IsolatedAppHome::acquire("retention-recovery-lost-ack");
    let report = crashed(&home, "retention-after-multi");
    let recovery = recovery_file();
    let (requested, mut items) = retry_input(&report);
    for item in &mut items {
        item.prepared = None;
    }
    let accepted = job_control::enqueue(requested.clone(), items).unwrap();
    assert_eq!(accepted.job_id, requested.job_id);
    // The priority return does not inspect/repair inputs. Explicit cleanup
    // authenticates and retires the still-durable accepted recovery epoch.
    assert!(recovery.exists());
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(!recovery.exists());
    assert_eq!(job_control::run_pending().unwrap(), 1);
    for path in paths(&report) {
        assert!(!path.exists());
    }
}

#[test]
fn orphan_reconciliation_preserves_newer_expiry_and_batch() {
    let home = IsolatedAppHome::acquire("retention-recovery-renewed");
    let report = crashed(&home, "retention-before-commit");
    let path = paths(&report).remove(0);
    let _conn = catalog::open_catalog().unwrap();
    jobs::renew_preview_lease(&path, 1500).unwrap();
    jobs::adopt_preview_for_batch(&path, "newer-synthetic-batch").unwrap();
    let before = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    let after = jobs::read_preview_lease(&path).unwrap().unwrap();
    assert!(!after.retained_by_job);
    assert_eq!(after.expires_unix, before.expires_unix);
    assert_eq!(after.prepare_batch_id, before.prepare_batch_id);
}

#[test]
fn expired_unaccepted_epoch_is_reclaimed_only_after_authenticated_repair() {
    let home = IsolatedAppHome::acquire("retention-recovery-expired");
    let report = crashed(&home, "retention-before-multi");
    let _conn = catalog::open_catalog().unwrap();
    let recovery = recovery_file();
    for path in paths(&report) {
        jobs::expire_preview_lease_file(&path).unwrap();
    }
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 2);
    assert!(!recovery.exists());
    for path in paths(&report) {
        assert!(!path.exists());
    }
}

#[test]
fn malformed_partial_or_missing_recovery_never_authorizes_orphan_deletion() {
    for corruption in ["malformed", "partial", "missing", "future", "foreign"] {
        let home = IsolatedAppHome::acquire(&format!("retention-recovery-{corruption}"));
        let report = crashed(&home, "retention-before-commit");
        let _conn = catalog::open_catalog().unwrap();
        let recovery = recovery_file();
        let path = paths(&report).remove(0);
        let before = std::fs::read(path.join("lease.json")).unwrap();
        let original = std::fs::read(&recovery).unwrap();
        match corruption {
            "malformed" => std::fs::write(&recovery, b"{incomplete").unwrap(),
            "partial" => std::fs::rename(&recovery, recovery.with_extension("pending")).unwrap(),
            "missing" => std::fs::remove_file(&recovery).unwrap(),
            _ => {
                let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
                if corruption == "future" {
                    value["version"] = serde_json::json!(99);
                } else {
                    value["epochId"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
                }
                std::fs::write(&recovery, serde_json::to_vec(&value).unwrap()).unwrap();
            }
        }
        assert!(
            jobs::cleanup_unreferenced_preview_dirs().is_err(),
            "{corruption}"
        );
        assert_eq!(
            std::fs::read(path.join("lease.json")).unwrap(),
            before,
            "{corruption}"
        );
        assert!(path.is_dir());
        assert_eq!(
            jobs::prepared_hashes(&path).unwrap(),
            serde_json::from_value::<jobs::PreviewLease>(report["originalLease"].clone())
                .unwrap()
                .file_hashes
        );
    }
}

#[test]
fn replaced_immutable_identity_defers_then_explicit_retry_really_repairs() {
    let home = IsolatedAppHome::acquire("retention-recovery-replaced");
    let report = crashed(&home, "retention-before-commit");
    let _conn = catalog::open_catalog().unwrap();
    let path = paths(&report).remove(0);
    let recovery = recovery_file();
    let original = jobs::read_preview_lease(&path).unwrap().unwrap();
    let mut replaced = original.clone();
    replaced.source_revision = "different-immutable-revision".into();
    jobs::write_preview_lease(&path, &replaced).unwrap();
    let error = jobs::cleanup_unreferenced_preview_dirs().unwrap_err();
    assert!(error.message.contains("deferred"));
    assert!(recovery.exists());
    assert!(
        jobs::read_preview_lease(&path)
            .unwrap()
            .unwrap()
            .retained_by_job
    );
    jobs::write_preview_lease(&path, &original).unwrap();
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(!recovery.exists());
    assert!(
        !jobs::read_preview_lease(&path)
            .unwrap()
            .unwrap()
            .retained_by_job
    );
}

#[test]
fn partial_accepted_input_set_is_not_proof_for_any_retention_mutation() {
    let home = IsolatedAppHome::acquire("retention-recovery-partial-accepted");
    let report = crashed(&home, "retention-after-multi");
    let conn = catalog::open_catalog().unwrap();
    let recovery = recovery_file();
    let before: Vec<_> = paths(&report)
        .iter()
        .map(|path| std::fs::read(path.join("lease.json")).unwrap())
        .collect();
    // Negative corruption only: the complete positive was generated by the
    // child's real SQL acceptance, never fabricated proof/input/job rows.
    conn.execute(
        "DELETE FROM job_items WHERE job_id=?1 AND candidate_id='second'",
        [report["jobId"].as_str().unwrap()],
    )
    .unwrap();
    assert!(jobs::cleanup_unreferenced_preview_dirs().is_err());
    assert!(recovery.exists());
    for (path, original) in paths(&report).iter().zip(before) {
        assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), original);
        assert!(path.is_dir());
    }
}

#[test]
fn replaced_catalog_cannot_reclassify_an_accepted_crash_as_rejection() {
    let home = IsolatedAppHome::acquire("retention-recovery-catalog-replaced");
    let report = crashed(&home, "retention-after-multi");
    let recovery = recovery_file();
    let original_receipt = std::fs::read(&recovery).unwrap();
    let before: Vec<_> = paths(&report)
        .iter()
        .map(|path| std::fs::read(path.join("lease.json")).unwrap())
        .collect();
    // Normal enqueue supplied the entire accepted positive. Only this isolated
    // catalog replacement is injected; never construct a positive proof row.
    let conn = catalog::open_catalog().unwrap();
    assert_eq!(
        jobs::get_job(&conn, report["jobId"].as_str().unwrap())
            .unwrap()
            .unwrap()
            .state,
        JobState::Queued
    );
    drop(conn);
    let catalog_path = catalog::catalog_path();
    let backup = home.path.join("original-accepted-catalog.sqlite");
    std::fs::rename(&catalog_path, &backup).unwrap();
    assert!(jobs::cleanup_unreferenced_preview_dirs().is_err());
    assert!(
        !catalog_path.exists(),
        "readonly cleanup must not create a catalog"
    );
    // A different legitimately initialized empty catalog is not the original
    // acceptance epoch, even though it has the same supported schema version.
    drop(catalog::open_catalog().unwrap());
    assert!(
        jobs::cleanup_unreferenced_preview_dirs().is_err(),
        "absence in a replacement catalog is not a rejected original COMMIT"
    );
    assert_eq!(std::fs::read(&recovery).unwrap(), original_receipt);
    for (path, bytes) in paths(&report).iter().zip(before) {
        assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), bytes);
        assert!(path.is_dir());
    }
    std::fs::remove_file(&catalog_path).unwrap();
    std::fs::rename(&backup, &catalog_path).unwrap();
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(!recovery.exists());
    assert_eq!(job_control::run_pending().unwrap(), 1);
    for path in paths(&report) {
        assert!(!path.exists());
    }
}

#[test]
fn same_epoch_preacceptance_backup_is_not_the_original_acceptance_catalog() {
    let home = IsolatedAppHome::acquire("retention-recovery-same-epoch-backup");
    // Generate a real supported catalog and backup before the child's normal
    // acceptance. The backup intentionally shares its genuine logical UUID;
    // no positive job, witness, proof or input row is constructed by the test.
    let conn = catalog::open_catalog().unwrap();
    let epoch = catalog::retention_catalog_epoch(&conn).unwrap().unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM jobs", [], |row| row.get::<_, u64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    let catalog_path = catalog::catalog_path();
    let older = home.path.join("preacceptance-catalog.sqlite");
    std::fs::copy(&catalog_path, &older).unwrap();
    let report = crashed(&home, "retention-after-multi");
    let conn = catalog::open_catalog().unwrap();
    assert_eq!(
        jobs::get_job(&conn, report["jobId"].as_str().unwrap())
            .unwrap()
            .unwrap()
            .state,
        JobState::Queued
    );
    assert_eq!(
        catalog::retention_catalog_epoch(&conn).unwrap().as_deref(),
        Some(epoch.as_str())
    );
    drop(conn);
    let recovery = recovery_file();
    let original_receipt = std::fs::read(&recovery).unwrap();
    let before: Vec<_> = paths(&report)
        .iter()
        .map(|path| std::fs::read(path.join("lease.json")).unwrap())
        .collect();
    let original = home.path.join("original-accepted-catalog.sqlite");
    std::fs::rename(&catalog_path, &original).unwrap();
    std::fs::rename(&older, &catalog_path).unwrap();
    let replacement = catalog::open_catalog_readonly().unwrap();
    assert_eq!(
        catalog::retention_catalog_epoch(&replacement)
            .unwrap()
            .as_deref(),
        Some(epoch.as_str())
    );
    assert!(
        jobs::get_job(&replacement, report["jobId"].as_str().unwrap())
            .unwrap()
            .is_none()
    );
    drop(replacement);
    assert!(
        jobs::cleanup_unreferenced_preview_dirs().is_err(),
        "a same-UUID older backup does not prove the original COMMIT was rejected"
    );
    assert_eq!(std::fs::read(&recovery).unwrap(), original_receipt);
    for (path, bytes) in paths(&report).iter().zip(before) {
        assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), bytes);
    }
    std::fs::remove_file(&catalog_path).unwrap();
    std::fs::rename(&original, &catalog_path).unwrap();
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(!recovery.exists());
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert!(paths(&report).iter().all(|path| !path.exists()));
}

#[test]
fn missing_foreign_or_malformed_catalog_epoch_defers_without_adopting_recovery() {
    for corruption in ["missing", "foreign", "malformed", "blob", "oversized"] {
        let home = IsolatedAppHome::acquire(&format!("retention-catalog-epoch-{corruption}"));
        let report = crashed(&home, "retention-after-multi");
        let conn = catalog::open_catalog().unwrap();
        let original = catalog::retention_catalog_epoch(&conn).unwrap().unwrap();
        let recovery = recovery_file();
        let original_receipt = std::fs::read(&recovery).unwrap();
        let before: Vec<_> = paths(&report)
            .iter()
            .map(|path| std::fs::read(path.join("lease.json")).unwrap())
            .collect();
        let key = "preview_retention_catalog_epoch";
        match corruption {
            "missing" => {
                conn.execute("DELETE FROM meta WHERE key=?1", [key])
                    .unwrap();
            }
            "blob" => {
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    rusqlite::params![original.as_bytes(), key],
                )
                .unwrap();
            }
            _ => {
                let replaced = match corruption {
                    "foreign" => uuid::Uuid::new_v4().to_string(),
                    "oversized" => "x".repeat(1024 * 1024),
                    _ => "not-a-canonical-uuid".into(),
                };
                conn.execute("UPDATE meta SET value=?1 WHERE key=?2", [&replaced, key])
                    .unwrap();
            }
        }
        assert!(
            jobs::cleanup_unreferenced_preview_dirs().is_err(),
            "{corruption}"
        );
        if corruption != "foreign" {
            assert!(catalog::open_catalog().is_err(), "{corruption}");
        }
        if corruption == "missing" {
            assert!(catalog::retention_catalog_epoch(&conn).unwrap().is_none());
        }
        assert_eq!(std::fs::read(&recovery).unwrap(), original_receipt);
        for (path, bytes) in paths(&report).iter().zip(before) {
            assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), bytes);
        }
        conn.execute(
            "INSERT OR REPLACE INTO meta(key,value) VALUES(?1,?2)",
            [key, &original],
        )
        .unwrap();
        assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
        assert!(!recovery.exists());
        assert_eq!(job_control::run_pending().unwrap(), 1);
        for path in paths(&report) {
            assert!(!path.exists());
        }
    }
}

#[test]
fn supported_legacy_catalog_initializes_once_only_without_pending_recovery() {
    let _home = IsolatedAppHome::acquire("retention-catalog-legacy");
    let conn = catalog::open_catalog().unwrap();
    let key = "preview_retention_catalog_epoch";
    conn.execute("DELETE FROM meta WHERE key=?1", [key])
        .unwrap();
    assert!(
        catalog::retention_catalog_epoch(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .is_none()
    );
    let writer = catalog::open_catalog().unwrap();
    let epoch = catalog::retention_catalog_epoch(&writer).unwrap().unwrap();
    assert_eq!(
        catalog::retention_catalog_epoch(&catalog::open_catalog().unwrap()).unwrap(),
        Some(epoch)
    );
    conn.execute("DELETE FROM meta WHERE key=?1", [key])
        .unwrap();
    let directory = catalog::imports_dir().join("preview-retention");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("unknown.pending"), b"partial").unwrap();
    assert!(catalog::open_catalog().is_err());
    assert!(catalog::retention_catalog_epoch(&conn).unwrap().is_none());
    assert_eq!(
        std::fs::read(directory.join("unknown.pending")).unwrap(),
        b"partial"
    );
}

#[test]
fn atomic_acceptance_witness_never_substitutes_for_complete_accepted_rows() {
    for corruption in [
        "missing-witness",
        "changed-witness",
        "blob-witness",
        "oversized-witness",
        "invalid-utf8-witness",
        "missing-rows",
    ] {
        let home = IsolatedAppHome::acquire(&format!("retention-witness-{corruption}"));
        let report = crashed(&home, "retention-after-multi");
        let conn = catalog::open_catalog().unwrap();
        let recovery = recovery_file();
        let original_receipt = std::fs::read(&recovery).unwrap();
        let before: Vec<_> = paths(&report)
            .iter()
            .map(|path| std::fs::read(path.join("lease.json")).unwrap())
            .collect();
        let (key, digest): (String, String) = conn
            .query_row(
                "SELECT key,value FROM meta WHERE key LIKE 'preview_retention_accepted:%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        match corruption {
            "missing-witness" => {
                conn.execute("DELETE FROM meta WHERE key=?1", [&key])
                    .unwrap();
            }
            "changed-witness" => {
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    [&"0".repeat(64), &key],
                )
                .unwrap();
            }
            "blob-witness" => {
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    rusqlite::params![digest.as_bytes(), &key],
                )
                .unwrap();
            }
            "oversized-witness" => {
                conn.execute(
                    "UPDATE meta SET value=CAST(zeroblob(16777216) AS TEXT) WHERE key=?1",
                    [&key],
                )
                .unwrap();
            }
            "invalid-utf8-witness" => {
                conn.execute(
                    "UPDATE meta SET value=CAST(?1 AS TEXT) WHERE key=?2",
                    rusqlite::params![vec![0xff_u8; 64], &key],
                )
                .unwrap();
            }
            _ => {
                let tx = conn.unchecked_transaction().unwrap();
                for table in ["job_items", "jobs"] {
                    tx.execute(
                        &format!("DELETE FROM {table} WHERE job_id=?1"),
                        [report["jobId"].as_str().unwrap()],
                    )
                    .unwrap();
                }
                tx.commit().unwrap();
                assert_eq!(
                    conn.query_row("SELECT value FROM meta WHERE key=?1", [&key], |row| row
                        .get::<_, String>(
                        0
                    ))
                    .unwrap(),
                    digest
                );
            }
        }
        assert!(
            jobs::cleanup_unreferenced_preview_dirs().is_err(),
            "{corruption}"
        );
        assert_eq!(std::fs::read(&recovery).unwrap(), original_receipt);
        for (path, bytes) in paths(&report).iter().zip(before) {
            assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), bytes);
            assert!(path.is_dir());
        }
    }
}

#[test]
fn bounded_retention_metadata_distinguishes_absence_type_length_and_utf8() {
    let _home = IsolatedAppHome::acquire("retention-meta-point-bounds");
    let conn = catalog::open_catalog().unwrap();
    let key = "synthetic_retention_meta";
    assert!(catalog::retention_metadata_text(&conn, key, 36)
        .unwrap()
        .is_none());
    conn.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2)",
        [key, &"x".repeat(36)],
    )
    .unwrap();
    assert_eq!(
        catalog::retention_metadata_text(&conn, key, 36).unwrap(),
        Some("x".repeat(36))
    );
    for corrupt in ["integer", "blob", "short", "large", "utf8"] {
        match corrupt {
            "integer" => conn.execute("UPDATE meta SET value=1 WHERE key=?1", [key]),
            "blob" => conn.execute("UPDATE meta SET value=zeroblob(36) WHERE key=?1", [key]),
            "short" => conn.execute("UPDATE meta SET value='x' WHERE key=?1", [key]),
            "large" => conn.execute(
                "UPDATE meta SET value=CAST(zeroblob(16777216) AS TEXT) WHERE key=?1",
                [key],
            ),
            _ => conn.execute(
                "UPDATE meta SET value=CAST(?1 AS TEXT) WHERE key=?2",
                rusqlite::params![vec![0xff_u8; 36], key],
            ),
        }
        .unwrap();
        assert!(
            catalog::retention_metadata_text(&conn, key, 36).is_err(),
            "{corrupt}"
        );
    }
    // The schema's TEXT affinity coerces the integer, so the reader refuses its
    // one-byte length. It never initializes over a malformed existing epoch.
    conn.execute(
        "UPDATE meta SET value='wrong' WHERE key='preview_retention_catalog_epoch'",
        [],
    )
    .unwrap();
    assert!(catalog::open_catalog().is_err());
}

#[test]
fn old_or_unknown_physical_binding_defers_without_touching_recovery() {
    for corrupt in ["old-format", "missing-file", "unknown-os", "wrong-file"] {
        let home = IsolatedAppHome::acquire(&format!("retention-physical-{corrupt}"));
        let report = crashed(&home, "retention-before-multi");
        let recovery = recovery_file();
        let original = std::fs::read(&recovery).unwrap();
        let before: Vec<_> = paths(&report)
            .iter()
            .map(|path| std::fs::read(path.join("lease.json")).unwrap())
            .collect();
        let mut changed: serde_json::Value = serde_json::from_slice(&original).unwrap();
        match corrupt {
            "old-format" => changed["version"] = serde_json::json!(2),
            "missing-file" => {
                changed.as_object_mut().unwrap().remove("catalogFile");
            }
            "unknown-os" => changed["catalogFile"]["platform"] = serde_json::json!("unknown"),
            _ => {
                #[cfg(windows)]
                {
                    changed["catalogFile"]["file"] = serde_json::json!(0);
                }
                #[cfg(unix)]
                {
                    changed["catalogFile"]["inode"] = serde_json::json!(0);
                }
            }
        }
        let bytes = serde_json::to_vec(&changed).unwrap();
        std::fs::write(&recovery, &bytes).unwrap();
        assert!(
            jobs::cleanup_unreferenced_preview_dirs().is_err(),
            "{corrupt}"
        );
        assert_eq!(std::fs::read(&recovery).unwrap(), bytes);
        for (path, expected) in paths(&report).iter().zip(before) {
            assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), expected);
        }
        std::fs::write(&recovery, &original).unwrap();
        assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
        assert!(!recovery.exists());
        let (requested, inputs) = retry_input(&report);
        job_control::enqueue(requested, inputs).unwrap();
        assert_eq!(job_control::run_pending().unwrap(), 1);
    }
}

#[test]
fn child_crash_between_two_retention_writes_repairs_the_complete_input_set() {
    let home = IsolatedAppHome::acquire("retention-mid-write-crash");
    let report = crashed(&home, "retention-before-mid-multi");
    let conn = catalog::open_catalog().unwrap();
    let paths = paths(&report);
    assert_eq!(paths.len(), 2);
    assert!(
        jobs::read_preview_lease(&paths[0])
            .unwrap()
            .unwrap()
            .retained_by_job
    );
    assert!(
        !jobs::read_preview_lease(&paths[1])
            .unwrap()
            .unwrap()
            .retained_by_job
    );
    assert!(jobs::get_job(&conn, report["jobId"].as_str().unwrap())
        .unwrap()
        .is_none());
    let before: Vec<_> = paths
        .iter()
        .map(|path| jobs::read_preview_lease(path).unwrap().unwrap())
        .collect();
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    for (path, original) in paths.iter().zip(before) {
        let repaired = jobs::read_preview_lease(path).unwrap().unwrap();
        assert!(!repaired.retained_by_job);
        assert_eq!(repaired.expires_unix, original.expires_unix);
        assert_eq!(repaired.prepare_batch_id, original.prepare_batch_id);
        assert_eq!(repaired.file_hashes, original.file_hashes);
    }
    let (requested, items) = retry_input(&report);
    job_control::enqueue(requested, items).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert!(paths.iter().all(|path| !path.exists()));
}

#[test]
fn aborted_child_epoch_preserves_a_genuine_competing_accepted_owner() {
    let home = IsolatedAppHome::acquire("retention-competing-accepted-child");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let encoded = job_control::encode_input(&item).unwrap();
    let accepted = job_control::enqueue(job(), vec![item]).unwrap();
    std::fs::write(
        catalog::imports_dir().join("synthetic-retention-input.json"),
        serde_json::to_vec(&serde_json::json!({"job": job(), "input": encoded})).unwrap(),
    )
    .unwrap();
    let report = crashed(&home, "retention-before-owned");
    let recovery = recovery_file();
    assert!(jobs::get_job(&conn, report["jobId"].as_str().unwrap())
        .unwrap()
        .is_none());
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(!recovery.exists());
    assert_eq!(
        jobs::get_job(&conn, &accepted.job_id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Queued
    );
    for path in paths(&report) {
        assert!(
            jobs::read_preview_lease(&path)
                .unwrap()
                .unwrap()
                .retained_by_job
        );
    }
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        jobs::get_job(&conn, &accepted.job_id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
    assert!(paths(&report).iter().all(|path| !path.exists()));
}

#[test]
fn valid_looking_overlapping_recovery_receipts_refuse_the_whole_pass() {
    let home = IsolatedAppHome::acquire("retention-overlapping-recovery");
    let report = crashed(&home, "retention-before-multi");
    let _conn = catalog::open_catalog().unwrap();
    let recovery = recovery_file();
    let original = std::fs::read(&recovery).unwrap();
    let mut duplicate: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let epoch = uuid::Uuid::new_v4().to_string();
    duplicate["epochId"] = serde_json::json!(epoch);
    let duplicate_path = recovery.parent().unwrap().join(format!("{epoch}.json"));
    let duplicate_bytes = serde_json::to_vec(&duplicate).unwrap();
    std::fs::write(&duplicate_path, &duplicate_bytes).unwrap();
    let before: Vec<_> = paths(&report)
        .iter()
        .map(|path| std::fs::read(path.join("lease.json")).unwrap())
        .collect();
    assert!(jobs::cleanup_unreferenced_preview_dirs().is_err());
    assert_eq!(std::fs::read(&recovery).unwrap(), original);
    assert_eq!(std::fs::read(&duplicate_path).unwrap(), duplicate_bytes);
    for (path, bytes) in paths(&report).iter().zip(before) {
        assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), bytes);
    }
    std::fs::remove_file(duplicate_path).unwrap();
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(!recovery.exists());
    for path in paths(&report) {
        assert!(
            !jobs::read_preview_lease(&path)
                .unwrap()
                .unwrap()
                .retained_by_job
        );
    }
}

#[test]
fn receipt_write_sync_and_rename_failures_reject_before_retention_or_sql_acceptance() {
    for point in ["write", "sync", "rename"] {
        let _home = IsolatedAppHome::acquire(&format!("retention-receipt-io-{point}"));
        let conn = catalog::open_catalog().unwrap();
        let item = input();
        let path = item.prepared.as_ref().unwrap().path.clone();
        let original = std::fs::read(path.join("lease.json")).unwrap();
        let requested = job();
        job_control::preview_retention::fail_receipt_once(point);
        let error = job_control::enqueue(requested.clone(), vec![item.clone()]).unwrap_err();
        assert!(error.message.contains(point), "{error:?}");
        assert!(jobs::get_job(&conn, &requested.job_id).unwrap().is_none());
        assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), original);
        assert!(recovery_file()
            .extension()
            .is_some_and(|extension| extension == "pending"));
        // Partial metadata must not be silently overwritten by a later epoch.
        assert!(job_control::enqueue(requested, vec![item]).is_err());
        assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), original);
    }
}

#[test]
fn accepted_receipt_retirement_failure_keeps_ack_and_reconciles_on_cleanup() {
    let _home = IsolatedAppHome::acquire("retention-receipt-retirement-io");
    let conn = catalog::open_catalog().unwrap();
    let item = input();
    let path = item.prepared.as_ref().unwrap().path.clone();
    let requested = job();
    job_control::preview_retention::fail_receipt_once("remove");
    let accepted = job_control::enqueue(requested.clone(), vec![item.clone()]).unwrap();
    assert_eq!(accepted.job_id, requested.job_id);
    let recovery = recovery_file();
    assert!(recovery.exists());
    let mut no_prepared = item;
    no_prepared.prepared = None;
    assert_eq!(
        job_control::enqueue(requested, vec![no_prepared])
            .unwrap()
            .job_id,
        accepted.job_id
    );
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(!recovery.exists());
    assert!(
        jobs::read_preview_lease(&path)
            .unwrap()
            .unwrap()
            .retained_by_job
    );
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        jobs::get_job(&conn, &accepted.job_id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
    assert!(!path.exists());
}

#[cfg(windows)]
#[test]
fn repeated_repair_io_failures_preserve_epoch_until_explicit_retry_succeeds() {
    use std::os::windows::fs::OpenOptionsExt;
    let home = IsolatedAppHome::acquire("retention-recovery-repeat-io");
    let report = crashed(&home, "retention-before-commit");
    let _conn = catalog::open_catalog().unwrap();
    let path = paths(&report).remove(0);
    let recovery = recovery_file();
    let original = std::fs::read(&recovery).unwrap();
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path.join("lease.json"))
        .unwrap();
    for _ in 0..2 {
        assert!(jobs::cleanup_unreferenced_preview_dirs().is_err());
        assert_eq!(std::fs::read(&recovery).unwrap(), original);
        assert!(path.is_dir());
        assert!(
            jobs::read_preview_lease(&path)
                .unwrap()
                .unwrap()
                .retained_by_job
        );
    }
    drop(locked);
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(!recovery.exists());
    assert!(
        !jobs::read_preview_lease(&path)
            .unwrap()
            .unwrap()
            .retained_by_job
    );
}
