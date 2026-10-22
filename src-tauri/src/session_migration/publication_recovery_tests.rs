//! Independent result/receipt/ACK primitives with synthetic signed records.
//! These do not fabricate accepted source/proof grants or open normal V3.
use super::*;
use crate::session_migration::{
    adapters::generic,
    job_control,
    jobs::{CommitItem, JobRecord, JobState},
    selection::{validate_prepared_selection, PreparedSelectionIdentity},
    test_home::IsolatedAppHome,
};
use rusqlite::params;

#[path = "publication_undo_tests.rs"]
mod undo_tests;

struct Fixture {
    input: String,
    allocated: VerifiedRecord,
    auth: InstallationAuthenticator,
    rec: Receipt,
    preview: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let adapter = generic::normalize_bytes(
            br#"[{"role":"user","content":"durable recovery primitive"}]"#,
            "recovery.json",
        )
        .unwrap();
        let mut item = jobs::prepare_test_commit_item(CommitItem {
            candidate_id: "recovery-candidate-中文".into(),
            installation_id: "recovery-source".into(),
            provider: "generic-file".into(),
            root_path: "synthetic-root".into(),
            adapter,
            prepared: None,
            options: serde_json::json!({}),
            mapping_project_id: None,
            explicit_reimport: false,
            create_project: false,
            import_unassigned_anyway: false,
            explicit_target: None,
            selection_set: Default::default(),
            identity_association: None,
        });
        item.installation_id = "test-source:recovery-source".into();
        let input = job_control::encode_input(&item).unwrap();
        let prepared = item.prepared.as_ref().unwrap();
        let verified = validate_prepared_selection(
            item.selection_set.single_entry().unwrap(),
            &prepared.path,
            PreparedSelectionIdentity::default(),
        )
        .unwrap();
        let auth = InstallationAuthenticator::load().unwrap();
        let binding = Binding {
            job_id: PublicationProtocol::SignedOutcomeV1.new_job_id(),
            context_digest: "a".repeat(64),
            plan_id: uuid::Uuid::new_v4().to_string(),
            plan_hash: "b".repeat(64),
            plan_digest: "c".repeat(64),
            acceptance_digest: "d".repeat(64),
            candidate_digest: sha256_bytes(item.candidate_id.as_bytes()),
            input_digest: sha256_bytes(input.as_bytes()),
        };
        let sealed = seal(
            &auth,
            binding.clone(),
            Stage::Allocated(Allocation {
                receipt_id: uuid::Uuid::new_v4().to_string(),
                import_id: crate::session_migration::ids::new_import_id(),
                revision_id: uuid::Uuid::new_v4().to_string(),
                previous_revision: None,
                created_entry: true,
                project: ProjectAllocation::Unassigned {},
            }),
        )
        .unwrap();
        let allocated = verify(
            &auth,
            sealed.encoded.as_bytes(),
            &binding,
            StageKind::Allocated,
        )
        .unwrap();
        let allocation = allocated.allocation().unwrap();
        let selection = item.selection_set.canonicalized();
        let mut rec = Receipt {
            version: jobs::DURABLE_SCHEMA_VERSION,
            source_member: verified.source_member,
            receipt_id: allocation.receipt_id.clone(),
            job_id: binding.job_id,
            candidate_id: item.candidate_id,
            installation_id: item.installation_id,
            provider: item.provider,
            native_session_id: item.adapter.graph.native_session_id,
            source_id: verified.source_id,
            root_path: verified.root_path,
            legacy_id: verified.legacy_id,
            phase: "prepared".into(),
            import_id: Some(allocation.import_id.clone()),
            revision_id: Some(allocation.revision_id.clone()),
            snapshot_hash: verified.snapshot_digest,
            options_hash: crate::session_migration::hash::options_hash(&item.options),
            selection_hash: selection.hash(),
            selection_set: selection,
            previous_revision: None,
            created_entry: true,
            file_hashes: Default::default(),
            mapping_project_id: None,
            created_project: false,
            project_decision: None,
            project_name_at_link: None,
            project_path_at_link: None,
            project_pinned_at_link: None,
            project_trusted_at_link: None,
        };
        native_link::planned(allocation, &mut rec);
        Self {
            input,
            allocated,
            auth,
            rec,
            preview: prepared.path.clone(),
        }
    }

    fn seed(&self, tx: &Transaction<'_>) -> JobItem {
        let allocation = self.allocated.allocation().unwrap();
        let binding = &self.allocated.body.binding;
        let sealed = seal(
            &self.auth,
            binding.clone(),
            self.allocated.body.stage.clone(),
        )
        .unwrap();
        journal::insert(tx, &self.auth, &sealed).unwrap();
        let mut rec = self.rec.clone();
        let mut stored_digest = None;
        for phase in receipt::PHASES {
            rec.phase = phase.into();
            rec.receipt_id = receipt::phase_id(allocation, phase).unwrap();
            if phase == "staged" {
                rec.file_hashes
                    .insert("manifest.json".into(), "f".repeat(64));
            }
            receipt::insert_once(tx, &rec).unwrap();
            if phase == "linked" {
                let sealed = seal(
                    &self.auth,
                    binding.clone(),
                    Stage::Stored(record::StoreResult {
                        allocation_digest: self.allocated.digest.clone(),
                        linked_receipt_digest: receipt::digest(&rec).unwrap(),
                        project: record::ProjectResult::Unassigned {},
                    }),
                )
                .unwrap();
                journal::insert(tx, &self.auth, &sealed).unwrap();
                stored_digest = Some(sha256_bytes(sealed.encoded.as_bytes()));
            }
        }
        let item = job_control::inspect_recovery_input(&self.input, |original| {
            original.result(&allocation.import_id, &allocation.revision_id, None)
        })
        .unwrap();
        let sealed = seal(
            &self.auth,
            binding.clone(),
            Stage::Committed(record::Committed {
                store_digest: stored_digest.unwrap(),
                receipt_digest: receipt::digest(&rec).unwrap(),
                item_digest: sha256_bytes(canonical(&item).unwrap().as_bytes()),
            }),
        )
        .unwrap();
        journal::insert(tx, &self.auth, &sealed).unwrap();
        let job = JobRecord {
            job_id: binding.job_id.clone(),
            request_id: "synthetic-recovery-request".into(),
            plan_id: binding.plan_id.clone(),
            plan_hash: binding.plan_hash.clone(),
            state: JobState::Running,
            state_version: 3,
            items: vec![item.clone()],
            created_at: "synthetic".into(),
            item_count: 1,
            next_item_cursor: None,
            progress: None,
        };
        jobs::save_job(tx, &job).unwrap();
        tx.execute("INSERT INTO job_items(job_id,candidate_id,phase,result_json) VALUES(?1,?2,'committed',?3)", params![job.job_id, item.candidate_id, self.input]).unwrap();
        item
    }

    fn read(&self, tx: &Transaction<'_>) -> Result<Option<Terminal>, ImportError> {
        job_control::inspect_recovery_input(&self.input, |original| {
            terminal(tx, &self.auth, &self.allocated, &original)
        })
        .unwrap()
    }
}

#[test]
fn independent_terminal_reader_survives_disposed_preview_and_reopened_catalog_without_writes() {
    let home = IsolatedAppHome::acquire("independent-result-reader");
    let fixture = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let expected = fixture.seed(&tx);
    tx.commit().unwrap();
    drop(conn);
    std::fs::rename(&fixture.preview, home.path.join("retained-unused-preview")).unwrap();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let changes = tx.total_changes();
    for _ in 0..10 {
        let result = fixture.read(&tx).unwrap().unwrap();
        assert_eq!(
            canonical(&result.item).unwrap(),
            canonical(&expected).unwrap()
        );
        assert_eq!(
            require_ack(&tx, &fixture.rec.job_id, &result.item, 0, None).unwrap(),
            3
        );
    }
    assert_eq!(tx.total_changes(), changes);
    assert!(!fixture.preview.exists());
}

#[test]
fn independent_reader_rejects_holes_mutable_rewrites_missing_results_and_unknown_ack_fields() {
    let _home = IsolatedAppHome::acquire("independent-result-refusal");
    let fixture = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    fixture.seed(&tx);
    for sql in [
        "DELETE FROM receipts WHERE phase='staged'",
        "DELETE FROM receipts WHERE phase='committed'",
        "DELETE FROM meta WHERE key LIKE '%:stored'",
        "DELETE FROM meta WHERE key LIKE '%:committed'",
        "UPDATE receipts SET json=json_set(json,'$.createdProject',json('true')) WHERE phase='linked'",
        "UPDATE receipts SET json=json_set(json,'$.mappingProjectId','foreign') WHERE phase='committed'",
        "UPDATE jobs SET json=json_set(json,'$.items[0].title','forged')",
        "UPDATE jobs SET json=json_set(json,'$.items[0].unknown',true)",
        "UPDATE job_items SET phase='queued'",
    ] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        tx.execute_batch(sql).unwrap();
        let changes = tx.total_changes();
        let result = fixture.read(&tx).and_then(|terminal| {
            let result = terminal.ok_or_else(invalid)?;
            require_ack(&tx, &fixture.rec.job_id, &result.item, 0, None).map(|_| ())
        });
        assert!(result.is_err(), "{sql}");
        assert_eq!(tx.total_changes(), changes);
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault").unwrap();
    }
}

#[test]
fn metadata_view_cannot_be_used_as_signed_source_acceptance_or_crypto_grant() {
    let _home = IsolatedAppHome::acquire("independent-result-no-grant");
    let fixture = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    // Prepare for an ABSENT job; this loads no signed key and grants nothing.
    let crypto = JobCrypto::prepare(&conn, &fixture.rec.job_id).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    fixture.seed(&tx);
    tx.commit().unwrap();
    let changes = conn.total_changes();
    let error = job_control::inspect_recovery_input(&fixture.input, |original| {
        recover(&conn, &fixture.rec.job_id, 0, &original, &crypto)
    })
    .unwrap()
    .err()
    .unwrap();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(conn.total_changes(), changes);
    assert!(jobs::get_job(&conn, &fixture.rec.job_id).is_err());
}

#[test]
fn independent_dedup_reader_uses_real_old_revision_not_reserved_ids_or_created_ownership() {
    let _home = IsolatedAppHome::acquire("independent-dedup-reader");
    let fixture = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    let job = JobRecord {
        job_id: "legacy-archive-owner".into(),
        request_id: "legacy-request".into(),
        plan_id: "legacy-plan".into(),
        plan_hash: "legacy-hash".into(),
        state: JobState::Queued,
        state_version: 1,
        items: vec![],
        created_at: "synthetic".into(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    };
    let item = job_control::decode_input(&fixture.input, &job).unwrap();
    let old = jobs::publish_item(&conn, &job.job_id, &item, &|| false).unwrap();
    let import = old.import_id.as_deref().unwrap();
    let revision = old.revision_id.as_deref().unwrap();
    let rec = jobs::latest_receipt_for_revision(&conn, import, revision)
        .unwrap()
        .unwrap();
    let entry = catalog::get_entry(&conn, import).unwrap().unwrap();
    let binding = &fixture.allocated.body.binding;
    let allocation = seal(
        &fixture.auth,
        binding.clone(),
        Stage::Allocated(Allocation {
            receipt_id: uuid::Uuid::new_v4().to_string(),
            import_id: import.into(),
            revision_id: uuid::Uuid::new_v4().to_string(),
            previous_revision: Some(revision.into()),
            created_entry: false,
            project: ProjectAllocation::Unassigned {},
        }),
    )
    .unwrap();
    let allocated = verify(
        &fixture.auth,
        allocation.encoded.as_bytes(),
        binding,
        StageKind::Allocated,
    )
    .unwrap();
    let result = job_control::inspect_recovery_input(&fixture.input, |original| {
        original.dedup_result(import, revision)
    })
    .unwrap();
    let sealed = seal(
        &fixture.auth,
        binding.clone(),
        Stage::Deduplicated(record::Deduplicated {
            allocation_digest: allocated.digest.clone(),
            import_id: import.into(),
            revision_id: revision.into(),
            receipt_id: rec.receipt_id.clone(),
            receipt_digest: receipt::digest(&rec).unwrap(),
            revision_digest: dedup::revision_digest(&conn, import, revision).unwrap(),
            selection_hash: rec.selection_hash.clone(),
            entry_identity_digest: dedup::identity_digest(&entry).unwrap(),
            item_digest: sha256_bytes(canonical(&result).unwrap().as_bytes()),
        }),
    )
    .unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    journal::insert(&tx, &fixture.auth, &allocation).unwrap();
    journal::insert(&tx, &fixture.auth, &sealed).unwrap();
    // A new display title cannot rewrite the original recorded ACK.
    tx.execute(
        "UPDATE entries SET title='user edited display title' WHERE import_id=?1",
        [import],
    )
    .unwrap();
    let changes = tx.total_changes();
    let recovered = job_control::inspect_recovery_input(&fixture.input, |original| {
        terminal(&tx, &fixture.auth, &allocated, &original)
    })
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(
        canonical(&recovered.item).unwrap(),
        canonical(&result).unwrap()
    );
    assert_eq!(recovered.receipt.receipt_id, rec.receipt_id);
    assert_ne!(
        recovered.item.revision_id.as_deref(),
        Some(allocated.allocation().unwrap().revision_id.as_str())
    );
    assert!(!allocated.allocation().unwrap().created_entry);
    let undo_point = job_control::inspect_recovery_input(&fixture.input, |original| {
        super::super::undo::point(&tx, &fixture.auth, &allocated, &original)
    })
    .unwrap()
    .unwrap();
    let mut forbidden = result.clone();
    forbidden.phase = "undone".into();
    assert!(undo_point.require_after(&undo_point, &forbidden).is_err());
    // The dedup job's terminal record cannot become the old receipt's publisher
    // or confer its native created ownership, even with a valid old archive.
    assert!(
        job_control::inspect_recovery_input(&fixture.input, |original| {
            require_original_terminal(&tx, &fixture.auth, &allocated, &original, &rec)
        })
        .unwrap()
        .is_err()
    );
    assert_eq!(tx.total_changes(), changes);
    tx.execute("UPDATE receipts SET json=json_set(json,'$.mappingProjectId','foreign') WHERE receipt_id=?1", [&rec.receipt_id]).unwrap();
    let changes = tx.total_changes();
    assert!(
        job_control::inspect_recovery_input(&fixture.input, |original| {
            terminal(&tx, &fixture.auth, &allocated, &original)
        })
        .unwrap()
        .is_err()
    );
    assert_eq!(tx.total_changes(), changes);
}

#[test]
fn original_publisher_terminal_reselects_unique_ack_slot_without_preview_or_writes() {
    let home = IsolatedAppHome::acquire("prior-owner-terminal-slot");
    let fixture = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    fixture.seed(&tx);
    let rec = fixture.read(&tx).unwrap().unwrap().receipt;
    // Synthetic terminal/ACK codec fixture only, not accepted membership.
    tx.execute("UPDATE jobs SET json=json_set(json,'$.items',json_array(json_object('candidateId','other','phase','queued'),json_extract(json,'$.items[0]')),'$.itemCount',2)", []).unwrap();
    tx.commit().unwrap();
    std::fs::rename(&fixture.preview, home.path.join("disposed-preview")).unwrap();
    drop(conn);
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let changes = tx.total_changes();
    for _ in 0..10 {
        job_control::inspect_recovery_input(&fixture.input, |original| {
            require_original_terminal(&tx, &fixture.auth, &fixture.allocated, &original, &rec)
        })
        .unwrap()
        .unwrap();
    }
    assert_eq!(tx.total_changes(), changes);
    assert!(!fixture.preview.exists());
}

#[test]
fn original_publisher_terminal_rejects_foreign_receipt_duplicate_candidate_and_unknown_ack() {
    let _home = IsolatedAppHome::acquire("prior-owner-terminal-refusal");
    let fixture = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    fixture.seed(&tx);
    let rec = fixture.read(&tx).unwrap().unwrap().receipt;
    let read = |rec: &Receipt| {
        job_control::inspect_recovery_input(&fixture.input, |original| {
            require_original_terminal(&tx, &fixture.auth, &fixture.allocated, &original, rec)
        })
        .unwrap()
    };
    for fault in ["job", "candidate", "project", "revision"] {
        let mut foreign = rec.clone();
        match fault {
            "job" => foreign.job_id = PublicationProtocol::SignedOutcomeV1.new_job_id(),
            "candidate" => foreign.candidate_id = "other".into(),
            "project" => foreign.mapping_project_id = Some("foreign".into()),
            "revision" => foreign.revision_id = Some(uuid::Uuid::new_v4().to_string()),
            _ => unreachable!(),
        }
        let changes = tx.total_changes();
        assert!(read(&foreign).is_err(), "{fault}");
        assert_eq!(tx.total_changes(), changes);
    }
    for sql in [
        "UPDATE jobs SET json=json_insert(json,'$.items[#]',json_extract(json,'$.items[0]'))",
        "UPDATE jobs SET json=json_set(json,'$.items[0].unknown',true)",
        "UPDATE jobs SET json=json_set(json,'$.stateVersion',99)",
    ] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        tx.execute_batch(sql).unwrap();
        let changes = tx.total_changes();
        assert!(read(&rec).is_err(), "{sql}");
        assert_eq!(tx.total_changes(), changes);
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
}
