// Included beside jobs_tests.rs so these tests exercise the real publisher and
// reuse its fixed-preview fixtures. No production fault hooks are introduced.

type StagedRetryReceiptRows = Vec<Vec<Option<Vec<u8>>>>;

fn staged_retry_receipt_rows(conn: &Connection, job: &str) -> StagedRetryReceiptRows {
    let mut statement = conn
        .prepare(
            "SELECT CAST(rowid AS BLOB), CAST(receipt_id AS BLOB),
                    CAST(job_id AS BLOB), CAST(candidate_id AS BLOB),
                    CAST(phase AS BLOB), CAST(import_id AS BLOB),
                    CAST(revision_id AS BLOB), CAST(json AS BLOB),
                    CAST(created_at AS BLOB)
             FROM receipts WHERE job_id=?1 ORDER BY rowid",
        )
        .unwrap();
    statement
        .query_map(params![job], |row| {
            (0..9)
                .map(|column| row.get::<_, Option<Vec<u8>>>(column))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn staged_retry_phases(conn: &Connection, job: &str) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT phase FROM receipts WHERE job_id=?1 ORDER BY rowid")
        .unwrap();
    statement
        .query_map(params![job], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn staged_retry_assert_no_catalog_rows(conn: &Connection) {
    let counts: (i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM entries), (SELECT COUNT(*) FROM revisions)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (0, 0));
}

fn staged_retry_cancel_at(conn: &Connection, job: &str, item: &CommitItem, phase: &str) -> Receipt {
    let cancelled = publish_item(conn, job, item, &|| {
        latest_receipt(conn, job, &item.candidate_id)
            .unwrap()
            .is_some_and(|receipt| receipt.phase == phase)
    })
    .unwrap();
    assert_eq!(cancelled.phase, "cancelled");
    let receipt = latest_receipt(conn, job, &item.candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, phase);
    let destination = revision_dir(
        receipt.import_id.as_deref().unwrap(),
        receipt.revision_id.as_deref().unwrap(),
    )
    .unwrap();
    assert!(!destination.exists());
    staged_retry_assert_no_catalog_rows(conn);
    receipt
}

fn staged_retry_assert_finished(
    conn: &Connection,
    job: &str,
    item: &CommitItem,
    before: &StagedRetryReceiptRows,
    reserved: &Receipt,
    expected_phases: &[&str],
) -> JobItem {
    let completed = publish_item(conn, job, item, &|| false).unwrap();
    assert_eq!(completed.phase, "committed");
    assert_eq!(completed.import_id, reserved.import_id);
    assert_eq!(completed.revision_id, reserved.revision_id);
    let after = staged_retry_receipt_rows(conn, job);
    assert_eq!(
        after.get(..before.len()),
        Some(before.as_slice()),
        "all existing receipt columns, including raw JSON and timestamps, must remain byte-identical"
    );
    assert_eq!(
        staged_retry_phases(conn, job),
        expected_phases
            .iter()
            .map(|phase| (*phase).to_string())
            .collect::<Vec<_>>()
    );
    let wrong_ids: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1 AND
             (import_id IS NULL OR revision_id IS NULL OR import_id<>?2 OR revision_id<>?3)",
            params![job, reserved.import_id, reserved.revision_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(wrong_ids, 0);
    let counts: (i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM entries), (SELECT COUNT(*) FROM revisions)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (1, 1));
    let repeated = publish_item(conn, job, item, &|| false).unwrap();
    assert_eq!(repeated.phase, "committed");
    assert_eq!(repeated.import_id, completed.import_id);
    assert_eq!(repeated.revision_id, completed.revision_id);
    assert_eq!(staged_retry_receipt_rows(conn, job), after);
    completed
}

const STAGED_RETRY_PHASES: &[&str] = &["prepared", "staged", "published", "linked", "committed"];

#[test]
fn staged_retry_reuses_persisted_stage_without_duplicate_phase_or_ids() {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("staged-retry");
    let conn = open_catalog().unwrap();
    let item = sample_item("staged-retry", "immutable staged body");
    let job = "staged-retry-job";
    let reserved = staged_retry_cancel_at(&conn, job, &item, "staged");
    let before = staged_retry_receipt_rows(&conn, job);
    assert_eq!(before.len(), 2);
    assert_eq!(staged_retry_phases(&conn, job), ["prepared", "staged"]);
    staged_retry_assert_finished(&conn, job, &item, &before, &reserved, STAGED_RETRY_PHASES);
}

#[test]
fn staged_retry_prepared_cancellation_resumes_without_replacing_prepared_row() {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("prepared-retry");
    let conn = open_catalog().unwrap();
    let item = sample_item("prepared-retry", "immutable prepared body");
    let job = "prepared-retry-job";
    let reserved = staged_retry_cancel_at(&conn, job, &item, "prepared");
    let before = staged_retry_receipt_rows(&conn, job);
    assert_eq!(before.len(), 1);
    assert_eq!(staged_retry_phases(&conn, job), ["prepared"]);
    staged_retry_assert_finished(&conn, job, &item, &before, &reserved, STAGED_RETRY_PHASES);
}

#[test]
fn staged_retry_missing_owned_stage_rebuilds_without_appending_staged_receipt() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("missing-stage-retry");
    let conn = open_catalog().unwrap();
    let item = sample_item("missing-stage-retry", "rebuild only from fixed preview");
    let job = "missing-stage-retry-job";
    let reserved = staged_retry_cancel_at(&conn, job, &item, "staged");
    let before = staged_retry_receipt_rows(&conn, job);
    let stage = staging_dir()
        .join(sha256_bytes(job.as_bytes()))
        .join(sha256_bytes(item.candidate_id.as_bytes()));
    let owned_home = fs::canonicalize(&home.path).unwrap();
    let owned_stage = fs::canonicalize(&stage).unwrap();
    assert!(owned_stage.starts_with(&owned_home));
    assert_ne!(owned_stage, owned_home);
    assert_ne!(
        owned_stage,
        fs::canonicalize(&item.prepared.as_ref().unwrap().path).unwrap()
    );
    fs::remove_dir_all(&owned_stage).unwrap();
    assert!(!stage.exists());
    staged_retry_assert_finished(&conn, job, &item, &before, &reserved, STAGED_RETRY_PHASES);
}

#[test]
fn staged_retry_tampered_persisted_file_hash_fails_closed_without_receipt_rewrite() {
    let _home =
        crate::session_migration::test_home::IsolatedAppHome::acquire("tampered-stage-retry");
    let conn = open_catalog().unwrap();
    let item = sample_item(
        "tampered-stage-retry",
        "do not bless a forged persisted hash",
    );
    let job = "tampered-stage-retry-job";
    let reserved = staged_retry_cancel_at(&conn, job, &item, "staged");
    let mut tampered = latest_receipt_json(&conn, job);
    tampered["fileHashes"]["events.jsonl"] = serde_json::json!("0".repeat(64));
    replace_latest_receipt_json(&conn, job, &tampered);
    let before = staged_retry_receipt_rows(&conn, job);
    let error = publish_item(&conn, job, &item, &|| false).unwrap_err();
    assert_eq!(error.code, ErrorCode::SourceChanged);
    assert_eq!(staged_retry_receipt_rows(&conn, job), before);
    assert_eq!(latest_receipt_json(&conn, job), tampered);
    staged_retry_assert_no_catalog_rows(&conn);
    assert!(!revision_dir(
        reserved.import_id.as_deref().unwrap(),
        reserved.revision_id.as_deref().unwrap(),
    )
    .unwrap()
    .exists());
}

#[test]
fn staged_retry_missing_persisted_hashes_are_not_regenerated_into_the_receipt() {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("missing-hash-retry");
    let conn = open_catalog().unwrap();
    let item = sample_item(
        "missing-hash-retry",
        "missing persisted hash is not authorization",
    );
    let job = "missing-hash-retry-job";
    let reserved = staged_retry_cancel_at(&conn, job, &item, "staged");
    let original = latest_receipt_json(&conn, job);
    for variant in 0..3 {
        let mut missing = original.clone();
        match variant {
            0 => {
                missing["fileHashes"]
                    .as_object_mut()
                    .unwrap()
                    .remove("events.jsonl");
            }
            1 => missing["fileHashes"] = serde_json::json!({}),
            _ => {
                missing.as_object_mut().unwrap().remove("fileHashes");
            }
        }
        replace_latest_receipt_json(&conn, job, &missing);
        let before = staged_retry_receipt_rows(&conn, job);
        let error = publish_item(&conn, job, &item, &|| false).unwrap_err();
        assert_eq!(
            error.code,
            if variant == 2 {
                ErrorCode::SchemaUnsupported
            } else {
                ErrorCode::SourceChanged
            },
            "variant {variant}"
        );
        assert_eq!(staged_retry_receipt_rows(&conn, job), before);
        assert_eq!(latest_receipt_json(&conn, job), missing);
        staged_retry_assert_no_catalog_rows(&conn);
    }
    assert!(!revision_dir(
        reserved.import_id.as_deref().unwrap(),
        reserved.revision_id.as_deref().unwrap(),
    )
    .unwrap()
    .exists());
}

#[test]
fn staged_retry_project_link_allows_legitimate_published_decision_receipt() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("project-stage-retry");
    let conn = open_catalog().unwrap();
    let target = home.path.join("owned-project");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), b"owned fixture remains unchanged").unwrap();
    let mut item = sample_item("project-stage-retry", "project-linked immutable history");
    item.create_project = true;
    item.explicit_target = Some(target.to_string_lossy().into_owned());
    let job = "project-stage-retry-job";
    let reserved = staged_retry_cancel_at(&conn, job, &item, "staged");
    assert!(crate::store::load_projects_snapshot().unwrap().is_empty());
    let before = staged_retry_receipt_rows(&conn, job);
    let completed = staged_retry_assert_finished(
        &conn,
        job,
        &item,
        &before,
        &reserved,
        &[
            "prepared",
            "staged",
            "published",
            "published",
            "linked",
            "committed",
        ],
    );
    let projects = crate::store::load_projects_snapshot().unwrap();
    assert_eq!(projects.len(), 1);
    let mut statement = conn
        .prepare("SELECT json FROM receipts WHERE job_id=?1 AND phase='published' ORDER BY rowid")
        .unwrap();
    let published = statement
        .query_map(params![job], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str::<serde_json::Value>(&row.unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(published.len(), 2);
    assert!(published[0]["mappingProjectId"].is_null());
    assert_eq!(published[1]["mappingProjectId"], projects[0].id);
    assert_eq!(published[1]["createdProject"], true);
    assert_eq!(published[1]["projectDecision"], "create");
    assert_eq!(
        completed.mapping_project_id.as_deref(),
        Some(projects[0].id.as_str())
    );
    assert!(!projects[0].trusted && !projects[0].pinned);
    assert_eq!(
        fs::canonicalize(&projects[0].path).unwrap(),
        fs::canonicalize(&target).unwrap()
    );
    let receipt = latest_receipt_json(&conn, job);
    assert_eq!(receipt["createdProject"], true);
    assert_eq!(receipt["projectTrustedAtLink"], false);
    assert_eq!(
        fs::read(target.join("keep.txt")).unwrap(),
        b"owned fixture remains unchanged"
    );
}

#[test]
fn staged_retry_modified_fixed_preview_still_fails_closed() {
    let _home =
        crate::session_migration::test_home::IsolatedAppHome::acquire("modified-preview-retry");
    let conn = open_catalog().unwrap();
    let item = sample_item(
        "modified-preview-retry",
        "fixed preview cannot change during retry",
    );
    let job = "modified-preview-retry-job";
    let reserved = staged_retry_cancel_at(&conn, job, &item, "staged");
    let before = staged_retry_receipt_rows(&conn, job);
    let path = item.prepared.as_ref().unwrap().path.join("assets.json");
    let mut modified = fs::read(&path).unwrap();
    modified.push(b' ');
    fs::write(&path, &modified).unwrap();
    assert!(publish_item(&conn, job, &item, &|| false).is_err());
    assert_eq!(staged_retry_receipt_rows(&conn, job), before);
    assert_eq!(fs::read(path).unwrap(), modified);
    staged_retry_assert_no_catalog_rows(&conn);
    assert!(!revision_dir(
        reserved.import_id.as_deref().unwrap(),
        reserved.revision_id.as_deref().unwrap(),
    )
    .unwrap()
    .exists());
}

#[test]
fn staged_retry_repeated_partial_stage_rebuilds_keep_the_same_checkpoint() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("partial-stage-retry");
    let conn = open_catalog().unwrap();
    let item = sample_item(
        "partial-stage-retry",
        "each retry rebuilds the same immutable data",
    );
    let job = "partial-stage-retry-job";
    let reserved = staged_retry_cancel_at(&conn, job, &item, "staged");
    let before = staged_retry_receipt_rows(&conn, job);
    let stage = staging_dir()
        .join(sha256_bytes(job.as_bytes()))
        .join(sha256_bytes(item.candidate_id.as_bytes()));
    let owned_home = fs::canonicalize(&home.path).unwrap();
    let owned_stage = fs::canonicalize(&stage).unwrap();
    let preview = &item.prepared.as_ref().unwrap().path;
    assert!(owned_stage.starts_with(&owned_home));
    assert_ne!(owned_stage, owned_home);
    assert_ne!(owned_stage, fs::canonicalize(preview).unwrap());
    assert!(!preview.join("selection.json").exists());
    let destination = revision_dir(
        reserved.import_id.as_deref().unwrap(),
        reserved.revision_id.as_deref().unwrap(),
    )
    .unwrap();
    for _ in 0..3 {
        fs::remove_file(stage.join("selection.json")).unwrap();
        fs::write(stage.join("events.jsonl"), b"partial staging data").unwrap();
        // Fixed previews have no selection.json. Its reappearance proves this
        // cancellation occurs after reconstruction, not at the pre-copy check.
        let cancelled =
            publish_item(&conn, job, &item, &|| stage.join("selection.json").exists()).unwrap();
        assert_eq!(cancelled.phase, "cancelled");
        assert!(stage.join("selection.json").is_file());
        assert_eq!(
            fs::read(stage.join("events.jsonl")).unwrap(),
            fs::read(preview.join("events.jsonl")).unwrap()
        );
        assert!(!destination.exists());
        assert_eq!(staged_retry_receipt_rows(&conn, job), before);
        staged_retry_assert_no_catalog_rows(&conn);
    }
    staged_retry_assert_finished(&conn, job, &item, &before, &reserved, STAGED_RETRY_PHASES);
}
