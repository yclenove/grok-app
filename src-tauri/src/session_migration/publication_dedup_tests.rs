//! Actual legacy archive verification plus synthetic outcome/ACK primitives.
//! No accepted signed grant or normal V3 producer is fabricated here.
use super::*;
use crate::session_migration::{adapters::generic, test_home::IsolatedAppHome};
use serde_json::json;

type ArchiveObserver = Box<dyn FnOnce(&Connection, &Receipt)>;
thread_local! {
    static ARCHIVE_OBSERVER: std::cell::RefCell<Option<ArchiveObserver>> = const { std::cell::RefCell::new(None) };
}

struct ObserveArchive;

impl Drop for ObserveArchive {
    fn drop(&mut self) {
        ARCHIVE_OBSERVER.with(|observer| observer.borrow_mut().take());
    }
}

fn observe_archive(observer: impl FnOnce(&Connection, &Receipt) + 'static) -> ObserveArchive {
    ARCHIVE_OBSERVER.with(|slot| {
        assert!(slot.borrow_mut().replace(Box::new(observer)).is_none());
    });
    ObserveArchive
}

/// Observation/fault injection only: never replace the real archive verifier.
pub(super) fn before_archive(conn: &Connection, receipt: &Receipt) {
    assert!(
        conn.is_autocommit(),
        "archive verification must be outside SQL"
    );
    let observer = ARCHIVE_OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(observer) = observer {
        observer(conn, receipt);
    }
}

struct Fixture {
    item: CommitItem,
    reuse: Reuse,
    auth: InstallationAuthenticator,
    allocation: SealedRecord,
    allocated: VerifiedRecord,
    item_result: JobItem,
    sealed: SealedRecord,
}

fn item(text: &str) -> CommitItem {
    let adapter = generic::normalize_bytes(
        serde_json::to_string(&json!([{"role":"user","content":text}]))
            .unwrap()
            .as_bytes(),
        "reuse.json",
    )
    .unwrap();
    jobs::prepare_test_commit_item(CommitItem {
        candidate_id: "reuse-candidate-中文".into(),
        installation_id: "synthetic-installation".into(),
        provider: "generic-file".into(),
        root_path: "synthetic-root".into(),
        adapter,
        prepared: None,
        options: json!({}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: Default::default(),
    })
}

impl Fixture {
    fn new(conn: &Connection) -> Self {
        let item = item("verified old archive");
        let old = jobs::publish_item(conn, "legacy-reuse-owner", &item, &|| false).unwrap();
        let import = old.import_id.unwrap();
        let revision = old.revision_id.unwrap();
        let entry = catalog::get_entry(conn, &import).unwrap().unwrap();
        let reuse = Reuse::capture(conn, &item, &entry, &revision).unwrap();
        let auth = InstallationAuthenticator::load().unwrap();
        let binding = Binding {
            job_id: PublicationProtocol::SignedOutcomeV1.new_job_id(),
            context_digest: "a".repeat(64),
            plan_id: uuid::Uuid::new_v4().to_string(),
            plan_hash: "b".repeat(64),
            plan_digest: "c".repeat(64),
            acceptance_digest: "d".repeat(64),
            candidate_digest: sha256_bytes(item.candidate_id.as_bytes()),
            input_digest: "e".repeat(64),
        };
        let allocation = seal(
            &auth,
            binding.clone(),
            Stage::Allocated(Allocation {
                receipt_id: uuid::Uuid::new_v4().to_string(),
                import_id: import.clone(),
                revision_id: uuid::Uuid::new_v4().to_string(),
                previous_revision: Some(revision.clone()),
                created_entry: false,
                project: ProjectAllocation::Unassigned {},
            }),
        )
        .unwrap();
        let allocated = verify(
            &auth,
            allocation.encoded.as_bytes(),
            &binding,
            StageKind::Allocated,
        )
        .unwrap();
        let item_result = JobItem {
            candidate_id: item.candidate_id.clone(),
            title: item.adapter.graph.title.clone(),
            phase: "committed".into(),
            import_id: Some(import),
            revision_id: Some(revision),
            outcome: Some("deduplicated".into()),
            error: None,
            mapping_project_id: None,
        };
        let sealed = seal(
            &auth,
            binding,
            Stage::Deduplicated(reuse.data(&allocated, &item_result).unwrap()),
        )
        .unwrap();
        Self {
            item,
            reuse,
            auth,
            allocation,
            allocated,
            item_result,
            sealed,
        }
    }

    fn store(&self, tx: &Transaction<'_>) {
        journal::insert(tx, &self.auth, &self.allocation).unwrap();
        journal::insert(tx, &self.auth, &self.sealed).unwrap();
    }

    fn key(&self, kind: StageKind) -> String {
        format!(
            "publication_outcome:v1:{}:{}:{}",
            self.allocated.body.binding.job_id,
            self.allocated.body.binding.candidate_digest,
            kind.key()
        )
    }
}

#[test]
fn dedup_reuses_verified_archive_and_retains_reserved_ids_unused_across_restart() {
    let _home = IsolatedAppHome::acquire("dedup-actual-archive");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let before: (u32, u32) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM revisions),(SELECT COUNT(*) FROM receipts)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    fixture.store(&tx);
    let changes = tx.total_changes();
    fixture.store(&tx);
    let result =
        journal::load_deduplicated(&tx, &fixture.auth, &fixture.allocated.body.binding).unwrap();
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(result.encoded, fixture.sealed.encoded);
    assert!(journal::load_committed(&tx, &fixture.auth, &fixture.allocated.body.binding).is_err());
    let Stage::Deduplicated(data) = &result.body.stage else {
        panic!("not dedup");
    };
    assert_ne!(
        data.revision_id,
        fixture.allocated.allocation().unwrap().revision_id
    );
    fixture.reuse.require(&tx).unwrap();
    tx.commit().unwrap();
    drop(conn);
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    assert_eq!(
        journal::load_deduplicated(&tx, &fixture.auth, &fixture.allocated.body.binding)
            .unwrap()
            .encoded,
        fixture.sealed.encoded
    );
    let after: (u32, u32) = tx
        .query_row(
            "SELECT (SELECT COUNT(*) FROM revisions),(SELECT COUNT(*) FROM receipts)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn dedup_old_noncurrent_revision_is_not_the_accepted_previous_current() {
    let _home = IsolatedAppHome::acquire("dedup-noncurrent");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let newer = item("different newer body");
    let new_result = jobs::publish_item(&conn, "legacy-newer-owner", &newer, &|| false).unwrap();
    let entry = catalog::get_entry(&conn, &fixture.reuse.entry.import_id)
        .unwrap()
        .unwrap();
    let old_revision = fixture.item_result.revision_id.as_deref().unwrap();
    assert_ne!(entry.current_revision.as_deref(), Some(old_revision));
    assert_eq!(entry.current_revision, new_result.revision_id);
    let reuse = Reuse::capture(&conn, &fixture.item, &entry, old_revision).unwrap();
    assert_eq!(
        reuse
            .projected_entry(&fixture.item, old_revision)
            .current_revision,
        entry.current_revision
    );
    let mut explicit = fixture.item.clone();
    explicit.explicit_reimport = true;
    assert_eq!(
        reuse
            .projected_entry(&explicit, old_revision)
            .current_revision
            .as_deref(),
        Some(old_revision)
    );
    assert_eq!(
        catalog::get_entry(&conn, &entry.import_id)
            .unwrap()
            .unwrap()
            .current_revision,
        entry.current_revision
    );
}

#[test]
fn dedup_preflight_rejects_changed_archive_without_writing_catalog() {
    let _home = IsolatedAppHome::acquire("dedup-file-tamper");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let revision = fixture.item_result.revision_id.as_deref().unwrap();
    let dir = catalog::revision_dir(&fixture.reuse.entry.import_id, revision).unwrap();
    std::fs::write(dir.join("events.jsonl"), b"tampered old archive\n").unwrap();
    let before = conn.total_changes();
    assert!(Reuse::capture(&conn, &fixture.item, &fixture.reuse.entry, revision).is_err());
    assert_eq!(conn.total_changes(), before);
}

#[test]
fn dedup_snapshot_rechecks_receipt_revision_identity_and_latest_point_without_repairs() {
    let _home = IsolatedAppHome::acquire("dedup-row-tamper");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    for fault in [
        "entry", "revision", "receipt", "column", "missing", "append",
    ] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        match fault {
            "entry" => {
                tx.execute("UPDATE entries SET native_session_id='foreign'", [])
                    .unwrap();
            }
            "revision" => {
                tx.execute(
                    "UPDATE revisions SET dir_path='foreign',options_hash='foreign'",
                    [],
                )
                .unwrap();
            }
            "receipt" => {
                tx.execute("UPDATE receipts SET json=json_set(json,'$.snapshotHash','foreign') WHERE receipt_id=?1", [&fixture.reuse.receipt.receipt_id]).unwrap();
            }
            "column" => {
                tx.execute(
                    "UPDATE receipts SET phase='foreign' WHERE receipt_id=?1",
                    [&fixture.reuse.receipt.receipt_id],
                )
                .unwrap();
            }
            "missing" => {
                tx.execute(
                    "DELETE FROM receipts WHERE receipt_id=?1",
                    [&fixture.reuse.receipt.receipt_id],
                )
                .unwrap();
            }
            "append" => {
                let mut rec = fixture.reuse.receipt.clone();
                rec.receipt_id = uuid::Uuid::new_v4().to_string();
                jobs::write_receipt(&tx, &rec).unwrap();
            }
            _ => unreachable!(),
        }
        let before = tx.total_changes();
        assert!(fixture.reuse.require(&tx).is_err(), "{fault}");
        assert_eq!(tx.total_changes(), before);
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
}

#[test]
fn dedup_terminal_codec_strict_fields_domain_and_every_result_binding() {
    let _home = IsolatedAppHome::acquire("dedup-strict-codec");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let original: serde_json::Value = serde_json::from_str(&fixture.sealed.encoded).unwrap();
    let fields = original["body"]["stage"]["deduplicated"]
        .as_object()
        .unwrap();
    for field in fields.keys() {
        for replacement in [
            None,
            Some(json!(null)),
            Some(json!([])),
            Some(json!("9".repeat(64))),
        ] {
            let mut changed = original.clone();
            let object = changed["body"]["stage"]["deduplicated"]
                .as_object_mut()
                .unwrap();
            match replacement {
                Some(value) => {
                    object.insert(field.clone(), value);
                }
                None => {
                    object.remove(field);
                }
            }
            assert!(
                verify(
                    &fixture.auth,
                    changed.to_string().as_bytes(),
                    &fixture.allocated.body.binding,
                    StageKind::Deduplicated
                )
                .is_err(),
                "{field}"
            );
        }
    }
    let mut unknown = original.clone();
    unknown["body"]["stage"]["deduplicated"]["createdProject"] = json!(true);
    assert!(verify(
        &fixture.auth,
        unknown.to_string().as_bytes(),
        &fixture.allocated.body.binding,
        StageKind::Deduplicated
    )
    .is_err());
    for kind in [
        StageKind::Allocated,
        StageKind::Stored,
        StageKind::Committed,
    ] {
        assert!(verify(
            &fixture.auth,
            fixture.sealed.encoded.as_bytes(),
            &fixture.allocated.body.binding,
            kind
        )
        .is_err());
    }
    let duplicate = fixture.sealed.encoded.replace(
        "\"deduplicated\":{",
        "\"deduplicated\":{\"itemDigest\":\"bad\",",
    );
    assert!(verify(
        &fixture.auth,
        duplicate.as_bytes(),
        &fixture.allocated.body.binding,
        StageKind::Deduplicated
    )
    .is_err());
}

#[test]
fn dedup_cannot_claim_created_entry_reserved_revision_or_another_import() {
    let _home = IsolatedAppHome::acquire("dedup-allocation-constraints");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let Stage::Deduplicated(original) = &fixture.sealed.body.stage else {
        panic!("not dedup");
    };
    for fault in ["new-revision", "import", "link", "new-entry"] {
        let mut data = original.clone();
        let mut allocation = fixture.allocated.allocation().unwrap().clone();
        match fault {
            "new-revision" => data.revision_id.clone_from(&allocation.revision_id),
            "import" => data.import_id = crate::session_migration::ids::new_import_id(),
            "link" => data.allocation_digest = "0".repeat(64),
            "new-entry" => {
                allocation.created_entry = true;
                allocation.previous_revision = None;
            }
            _ => unreachable!(),
        }
        let allocated = seal(
            &fixture.auth,
            fixture.allocated.body.binding.clone(),
            Stage::Allocated(allocation),
        )
        .unwrap();
        let allocated = verify(
            &fixture.auth,
            allocated.encoded.as_bytes(),
            &allocated.body.binding,
            StageKind::Allocated,
        )
        .unwrap();
        if fault != "link" {
            data.allocation_digest.clone_from(&allocated.digest);
        }
        let result = seal(
            &fixture.auth,
            fixture.allocated.body.binding.clone(),
            Stage::Deduplicated(data),
        )
        .unwrap();
        let result = verify(
            &fixture.auth,
            result.encoded.as_bytes(),
            &result.body.binding,
            StageKind::Deduplicated,
        )
        .unwrap();
        assert!(result.follows(&allocated).is_err(), "{fault}");
    }
}

#[test]
fn dedup_missing_points_conflicts_and_raw_parallel_terminal_are_rejected_read_only() {
    let _home = IsolatedAppHome::acquire("dedup-journal-negative");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    fixture.store(&tx);
    for kind in [StageKind::Allocated, StageKind::Deduplicated] {
        tx.execute_batch("SAVEPOINT missing").unwrap();
        tx.execute("DELETE FROM meta WHERE key=?1", [fixture.key(kind)])
            .unwrap();
        let before = tx.total_changes();
        assert!(
            journal::load_deduplicated(&tx, &fixture.auth, &fixture.allocated.body.binding)
                .is_err()
        );
        assert_eq!(tx.total_changes(), before);
        tx.execute_batch("ROLLBACK TO missing; RELEASE missing")
            .unwrap();
    }
    let Stage::Deduplicated(mut data) = fixture.sealed.body.stage.clone() else {
        panic!("not dedup");
    };
    data.item_digest = "f".repeat(64);
    let changed = seal(
        &fixture.auth,
        fixture.allocated.body.binding.clone(),
        Stage::Deduplicated(data),
    )
    .unwrap();
    let before = tx.total_changes();
    assert!(journal::insert(&tx, &fixture.auth, &changed).is_err());
    assert_eq!(tx.total_changes(), before);
    let stored = seal(
        &fixture.auth,
        fixture.allocated.body.binding.clone(),
        Stage::Stored(StoreResult {
            allocation_digest: fixture.allocated.digest.clone(),
            linked_receipt_digest: "0".repeat(64),
            project: super::super::super::record::ProjectResult::Unassigned {},
        }),
    )
    .unwrap();
    assert!(journal::insert(&tx, &fixture.auth, &stored).is_err());
    tx.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2)",
        [fixture.key(StageKind::Stored), stored.encoded],
    )
    .unwrap();
    let before = tx.total_changes();
    assert!(
        journal::load_deduplicated(&tx, &fixture.auth, &fixture.allocated.body.binding).is_err()
    );
    assert!(journal::load_committed(&tx, &fixture.auth, &fixture.allocated.body.binding).is_err());
    assert_eq!(tx.total_changes(), before);
}

#[test]
fn dedup_late_result_trigger_rolls_back_without_replacing_original_archive() {
    let _home = IsolatedAppHome::acquire("dedup-late-trigger");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    journal::insert(&tx, &fixture.auth, &fixture.allocation).unwrap();
    tx.commit().unwrap();
    conn.execute_batch("CREATE TRIGGER alter_dedup AFTER INSERT ON meta WHEN NEW.key LIKE '%:deduplicated' BEGIN UPDATE meta SET value='{}' WHERE key=NEW.key; UPDATE entries SET hidden=1; END;").unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    assert!(journal::insert(&tx, &fixture.auth, &fixture.sealed).is_err());
    tx.rollback().unwrap();
    assert!(
        !catalog::get_entry(&conn, &fixture.reuse.entry.import_id)
            .unwrap()
            .unwrap()
            .hidden
    );
    let tx = conn.unchecked_transaction().unwrap();
    assert!(!journal::present(
        &tx,
        &fixture.allocated.body.binding,
        StageKind::Deduplicated
    )
    .unwrap());
    fixture.reuse.require(&tx).unwrap();
}

#[test]
fn dedup_completed_ack_cannot_be_treated_as_an_unstarted_result_after_deletion() {
    let _home = IsolatedAppHome::acquire("dedup-ack-no-resign");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    let job = &fixture.allocated.body.binding.job_id;
    let body = json!({"items":[fixture.item_result],"stateVersion":7});
    tx.execute("INSERT INTO jobs(job_id,request_id,plan_hash,state,state_version,json,created_at) VALUES(?1,'synthetic','hash','running',7,?2,'synthetic')", params![job, body.to_string()]).unwrap();
    tx.execute(
        "INSERT INTO job_items(job_id,candidate_id,phase) VALUES(?1,?2,'committed')",
        params![job, fixture.item.candidate_id],
    )
    .unwrap();
    fixture.store(&tx);
    tx.execute(
        "DELETE FROM meta WHERE key=?1",
        [fixture.key(StageKind::Deduplicated)],
    )
    .unwrap();
    let before = tx.total_changes();
    assert!(require_unacknowledged(&tx, job, &fixture.item.candidate_id, 0).is_err());
    assert!(require_unacknowledged(&tx, job, &fixture.item.candidate_id, 1).is_err());
    assert!(
        journal::load_deduplicated(&tx, &fixture.auth, &fixture.allocated.body.binding).is_err()
    );
    assert_eq!(tx.total_changes(), before);
    tx.execute(
        "UPDATE jobs SET json=json_set(json,'$.items[0].phase','queued')",
        [],
    )
    .unwrap();
    assert!(require_unacknowledged(&tx, job, &fixture.item.candidate_id, 0).is_err());
    tx.execute("UPDATE job_items SET phase='queued'", [])
        .unwrap();
    assert!(require_unacknowledged(&tx, job, &fixture.item.candidate_id, 0).is_err());
    tx.execute("UPDATE jobs SET json=json_set(json,'$.items[0].importId',NULL,'$.items[0].revisionId',NULL,'$.items[0].outcome',NULL)", []).unwrap();
    require_unacknowledged(&tx, job, &fixture.item.candidate_id, 0).unwrap();
    tx.execute(
        "UPDATE jobs SET json=json_set(json,'$.items[0].unknown',true)",
        [],
    )
    .unwrap();
    assert!(require_unacknowledged(&tx, job, &fixture.item.candidate_id, 0).is_err());
}

#[test]
fn dedup_rechecks_semantic_index_after_the_initial_duplicate_lookup() {
    let _home = IsolatedAppHome::acquire("dedup-semantic-race");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let revision = fixture.item_result.revision_id.as_deref().unwrap();
    conn.execute(
        "UPDATE revisions SET options_hash='coordinated-wrong-semantic-key'",
        [],
    )
    .unwrap();
    let before = conn.total_changes();
    assert!(Reuse::capture(&conn, &fixture.item, &fixture.reuse.entry, revision).is_err());
    assert_eq!(conn.total_changes(), before);
}

#[test]
fn dedup_target_must_still_have_the_accepted_current_not_merely_a_reusable_old_revision() {
    let _home = IsolatedAppHome::acquire("dedup-accepted-current-cas");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let allocation = fixture.allocated.allocation().unwrap();
    require_reuse_target(allocation, &fixture.reuse.entry).unwrap();
    let mut changed = fixture.reuse.entry.clone();
    changed.current_revision = Some(uuid::Uuid::new_v4().to_string());
    let before = conn.total_changes();
    assert!(require_reuse_target(allocation, &changed).is_err());
    assert_eq!(conn.total_changes(), before);
    // A genuinely accepted newer current may still reuse an older revision;
    // the accepted previous current and reused archive IDs are not conflated.
    let mut accepted = allocation.clone();
    accepted
        .previous_revision
        .clone_from(&changed.current_revision);
    require_reuse_target(&accepted, &changed).unwrap();
    assert_ne!(changed.current_revision, fixture.item_result.revision_id);
    accepted.created_entry = true;
    assert!(require_reuse_target(&accepted, &changed).is_err());
    accepted.created_entry = false;
    accepted.import_id = crate::session_migration::ids::new_import_id();
    assert!(require_reuse_target(&accepted, &changed).is_err());
}

#[test]
fn dedup_signed_old_owner_never_uses_legacy_receipt_authorization() {
    let _home = IsolatedAppHome::acquire("dedup-signed-old-owner");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let revision = fixture.item_result.revision_id.as_deref().unwrap();
    let signed = PublicationProtocol::SignedOutcomeV1.new_job_id();
    conn.execute(
        "UPDATE receipts SET job_id=?1,json=json_set(json,'$.jobId',?1) WHERE receipt_id=?2",
        params![signed, fixture.reuse.receipt.receipt_id],
    )
    .unwrap();
    let before = conn.total_changes();
    let error = Reuse::capture(&conn, &fixture.item, &fixture.reuse.entry, revision)
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(conn.total_changes(), before);
}

#[test]
fn dedup_catalog_projection_retains_alias_custom_title_and_newer_current_revision() {
    let _home = IsolatedAppHome::acquire("dedup-title-projection");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let revision = fixture.item_result.revision_id.as_deref().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    // Pure catalog/title projection primitives, not Codex source authorization.
    for scenario in ["automatic", "custom", "newer", "explicit"] {
        tx.execute_batch("SAVEPOINT scenario").unwrap();
        let mut item = fixture.item.clone();
        item.provider = "codex".into();
        item.adapter.graph.title = Some("Updated source title 中文".into());
        item.explicit_reimport = scenario == "explicit";
        let mut entry = fixture.reuse.entry.clone();
        entry.provider = item.provider.clone();
        entry.alias = Some("My preserved alias".into());
        entry.title.clone_from(&entry.native_session_id);
        if scenario == "custom" {
            entry.title = "User title".into();
        }
        if matches!(scenario, "newer" | "explicit") {
            entry.current_revision = Some(uuid::Uuid::new_v4().to_string());
            entry.hidden = true;
        }
        tx.execute("UPDATE entries SET provider=?2,title=?3,alias=?4,current_revision=?5,hidden=?6 WHERE import_id=?1", params![entry.import_id,entry.provider,entry.title,entry.alias,entry.current_revision,entry.hidden]).unwrap();
        // Test the pure projection, not a fabricated crypto/owner witness.
        let expected = Reuse::projected_from(&entry, &item, revision);
        if item.explicit_reimport {
            tx.execute(
                "UPDATE entries SET hidden=0,current_revision=?1 WHERE import_id=?2",
                params![revision, entry.import_id],
            )
            .unwrap();
        }
        jobs::repair_deduplicated_title(&tx, &item, &entry.import_id, revision).unwrap();
        let actual = catalog::get_entry(&tx, &entry.import_id).unwrap().unwrap();
        assert_eq!(
            canonical(&actual).unwrap(),
            canonical(&expected).unwrap(),
            "{scenario}"
        );
        assert_eq!(actual.alias.as_deref(), Some("My preserved alias"));
        tx.execute_batch("ROLLBACK TO scenario; RELEASE scenario")
            .unwrap();
    }
}

#[test]
fn dedup_preflight_verifies_real_legacy_archive_outside_snapshot_including_legacy_semantic_key() {
    let _home = IsolatedAppHome::acquire("dedup-archive-snapshot-boundary");
    let conn = catalog::open_catalog().unwrap();
    let fixture = Fixture::new(&conn);
    let rec = &fixture.reuse.receipt;
    conn.execute(
        "UPDATE revisions SET options_hash=?1 WHERE import_id=?2 AND revision_id=?3",
        params![
            jobs::revision_semantic_key(&rec.options_hash, &rec.selection_set.hash()),
            rec.import_id,
            rec.revision_id
        ],
    )
    .unwrap();
    let visited = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = visited.clone();
    let _observer = observe_archive(move |conn, _| {
        assert!(conn.is_autocommit());
        observed.set(true);
    });
    let before = conn.total_changes();
    let reused = Reuse::capture(
        &conn,
        &fixture.item,
        &fixture.reuse.entry,
        fixture.item_result.revision_id.as_deref().unwrap(),
    )
    .unwrap();
    assert!(visited.get());
    assert_eq!(reused.selection_hash, fixture.reuse.selection_hash);
    assert_eq!(conn.total_changes(), before);
}

#[test]
fn dedup_preflight_rejects_sql_changes_during_real_out_of_snapshot_archive_verification() {
    for fault in ["entry", "receipt", "revision"] {
        let _home = IsolatedAppHome::acquire("dedup-archive-recheck-race");
        let conn = catalog::open_catalog().unwrap();
        let fixture = Fixture::new(&conn);
        let fault_changes = std::rc::Rc::new(std::cell::Cell::new(None));
        let observed = fault_changes.clone();
        let _observer = observe_archive(move |conn, rec| {
            assert!(conn.is_autocommit());
            match fault {
                "entry" => conn.execute(
                    "UPDATE entries SET title='concurrent edit' WHERE import_id=?1",
                    [rec.import_id.as_deref().unwrap()],
                ),
                "receipt" => conn.execute(
                    "UPDATE receipts SET json=json_set(json,'$.mappingProjectId','foreign') WHERE receipt_id=?1",
                    [&rec.receipt_id],
                ),
                "revision" => conn.execute(
                    "UPDATE revisions SET inventory_json='{}' WHERE import_id=?1 AND revision_id=?2",
                    params![rec.import_id, rec.revision_id],
                ),
                _ => unreachable!(),
            }.unwrap();
            observed.set(Some(conn.total_changes()));
        });
        assert!(
            Reuse::capture(
                &conn,
                &fixture.item,
                &fixture.reuse.entry,
                fixture.item_result.revision_id.as_deref().unwrap(),
            )
            .is_err(),
            "{fault}"
        );
        assert_eq!(
            Some(conn.total_changes()),
            fault_changes.get(),
            "{fault}: no repair or target writes"
        );
    }
}
