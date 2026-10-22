//! Synthetic receipt/native/result primitives, NOT an accepted publisher grant.
//! No fixture bypasses the normal V3 rollout or source/proof authentication.
use super::super::record::{ProjectResult, Stage};
use super::*;
use crate::session_migration::{
    adapters::generic,
    catalog,
    jobs::{JobRecord, JobState},
    selection::{validate_prepared_selection, PreparedSelectionIdentity},
    test_home::IsolatedAppHome,
};
use rusqlite::params;

struct Fixture {
    item: CommitItem,
    verified: PreparedSelectionValidation,
    auth: InstallationAuthenticator,
    allocated: VerifiedRecord,
}

impl Fixture {
    fn new(project: ProjectAllocation) -> Self {
        let adapter = generic::normalize_bytes(
            br#"[{"role":"user","content":"synthetic transition"}]"#,
            "transition.json",
        )
        .unwrap();
        let item = jobs::prepare_test_commit_item(CommitItem {
            candidate_id: "transition-candidate-中文".into(),
            installation_id: "installation".into(),
            provider: "generic-file".into(),
            root_path: "synthetic-source".into(),
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
        let prepared = item.prepared.as_ref().unwrap();
        let verified = validate_prepared_selection(
            item.selection_set.single_entry().unwrap(),
            &prepared.path,
            PreparedSelectionIdentity::default(),
        )
        .unwrap();
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
        let auth = InstallationAuthenticator::load().unwrap();
        let sealed = seal(
            &auth,
            binding.clone(),
            Stage::Allocated(Allocation {
                receipt_id: uuid::Uuid::new_v4().to_string(),
                import_id: crate::session_migration::ids::new_import_id(),
                revision_id: uuid::Uuid::new_v4().to_string(),
                previous_revision: None,
                created_entry: true,
                project,
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
        Self {
            item,
            verified,
            auth,
            allocated,
        }
    }

    fn publication(&self) -> SignedPublication<'_> {
        SignedPublication {
            auth: &self.auth,
            allocated: verify(
                &self.auth,
                self.allocated.encoded.as_bytes(),
                &self.allocated.body.binding,
                StageKind::Allocated,
            )
            .unwrap(),
            item: &self.item,
            verified: &self.verified,
        }
    }

    fn receipt(&self) -> Receipt {
        let allocation = self.allocated.allocation().unwrap();
        let selection = self.item.selection_set.canonicalized();
        let mut receipt = Receipt {
            version: jobs::DURABLE_SCHEMA_VERSION,
            source_member: self.verified.source_member.clone(),
            receipt_id: allocation.receipt_id.clone(),
            job_id: self.allocated.body.binding.job_id.clone(),
            candidate_id: self.item.candidate_id.clone(),
            installation_id: self.item.installation_id.clone(),
            provider: self.item.provider.clone(),
            native_session_id: self.item.adapter.graph.native_session_id.clone(),
            source_id: self.verified.source_id.clone(),
            root_path: self.verified.root_path.clone(),
            legacy_id: self.verified.legacy_id.clone(),
            phase: "prepared".into(),
            import_id: Some(allocation.import_id.clone()),
            revision_id: Some(allocation.revision_id.clone()),
            snapshot_hash: self.verified.snapshot_digest.clone(),
            options_hash: crate::session_migration::hash::options_hash(&self.item.options),
            selection_hash: selection.hash(),
            selection_set: selection,
            previous_revision: allocation.previous_revision.clone(),
            created_entry: allocation.created_entry,
            file_hashes: Default::default(),
            mapping_project_id: None,
            created_project: false,
            project_decision: None,
            project_name_at_link: None,
            project_path_at_link: None,
            project_pinned_at_link: None,
            project_trusted_at_link: None,
        };
        native_link::planned(allocation, &mut receipt);
        receipt
    }

    fn first_three(&self, tx: &Transaction<'_>) -> Receipt {
        let mut rec = self.receipt();
        for phase in ["prepared", "staged", "published"] {
            rec.phase = phase.into();
            rec.receipt_id =
                receipt::phase_id(self.allocated.allocation().unwrap(), phase).unwrap();
            if phase == "staged" {
                rec.file_hashes
                    .insert("manifest.json".into(), "f".repeat(64));
            }
            receipt::insert_once(tx, &rec).unwrap();
        }
        rec
    }

    fn allocated_row(&self, tx: &Transaction<'_>) {
        let sealed = seal(
            &self.auth,
            self.allocated.body.binding.clone(),
            self.allocated.body.stage.clone(),
        )
        .unwrap();
        journal::insert(tx, &self.auth, &sealed).unwrap();
    }

    fn linked(&self, tx: &Transaction<'_>) -> Receipt {
        self.allocated_row(tx);
        let mut rec = self.first_three(tx);
        let project = native_link::apply(&self.allocated, &self.auth, "unused", &mut rec).unwrap();
        rec.phase = "linked".into();
        rec.receipt_id = receipt::phase_id(self.allocated.allocation().unwrap(), "linked").unwrap();
        receipt::insert_once(tx, &rec).unwrap();
        let sealed = seal(
            &self.auth,
            self.allocated.body.binding.clone(),
            Stage::Stored(StoreResult {
                allocation_digest: self.allocated.digest.clone(),
                linked_receipt_digest: receipt::digest(&rec).unwrap(),
                project,
            }),
        )
        .unwrap();
        journal::insert(tx, &self.auth, &sealed).unwrap();
        rec
    }
}

#[test]
fn stable_receipt_prefix_roundtrip_exact_retry_and_no_skipped_or_foreign_rows() {
    let _home = IsolatedAppHome::acquire("transition-receipt-prefix");
    let fixture = Fixture::new(ProjectAllocation::Unassigned {});
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let rec = fixture.first_three(&tx);
    assert_eq!(fixture.publication().current(&tx).unwrap().len(), 3);
    let changes = tx.total_changes();
    receipt::insert_once(&tx, &rec).unwrap();
    assert_eq!(tx.total_changes(), changes);
    for phase in receipt::PHASES {
        let id = receipt::phase_id(fixture.allocated.allocation().unwrap(), phase).unwrap();
        assert_eq!(
            id,
            receipt::phase_id(fixture.allocated.allocation().unwrap(), phase).unwrap()
        );
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }
    assert!(receipt::phase_id(fixture.allocated.allocation().unwrap(), "unknown").is_err());
    tx.commit().unwrap();
    drop(conn);
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    assert_eq!(fixture.publication().current(&tx).unwrap().len(), 3);
    for fault in ["hole", "foreign", "column", "phase", "body", "planned"] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        match fault {
            "hole" => {
                tx.execute("DELETE FROM receipts WHERE phase='staged'", [])
                    .unwrap();
            }
            "foreign" => {
                let mut foreign = rec.clone();
                foreign.receipt_id = uuid::Uuid::new_v4().to_string();
                jobs::write_receipt(&tx, &foreign).unwrap();
            }
            "column" => {
                tx.execute(
                    "UPDATE receipts SET candidate_id='foreign' WHERE phase='published'",
                    [],
                )
                .unwrap();
            }
            "phase" => {
                tx.execute(
                    "UPDATE receipts SET phase='unknown' WHERE phase='published'",
                    [],
                )
                .unwrap();
            }
            "body" => {
                tx.execute("UPDATE receipts SET json=json_set(json,'$.optionsHash','changed') WHERE phase='published'", []).unwrap();
            }
            "planned" => {
                tx.execute("UPDATE receipts SET json=json_set(json,'$.createdProject',json('true')) WHERE phase='published'", []).unwrap();
            }
            _ => unreachable!(),
        }
        let changes = tx.total_changes();
        assert!(fixture.publication().current(&tx).is_err(), "{fault}");
        assert_eq!(tx.total_changes(), changes);
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
}

#[test]
fn receipt_insert_once_rejects_conflicts_and_late_insert_trigger() {
    let _home = IsolatedAppHome::acquire("transition-receipt-insert");
    let fixture = Fixture::new(ProjectAllocation::Unassigned {});
    let conn = catalog::open_catalog().unwrap();
    let rec = fixture.receipt();
    let tx = conn.unchecked_transaction().unwrap();
    receipt::insert_once(&tx, &rec).unwrap();
    let mut changed = rec.clone();
    changed.created_entry = false;
    let before = tx.total_changes();
    assert!(receipt::insert_once(&tx, &changed).is_err());
    assert_eq!(tx.total_changes(), before);
    tx.rollback().unwrap();
    conn.execute_batch("CREATE TRIGGER corrupt_receipt AFTER INSERT ON receipts BEGIN UPDATE receipts SET json=json_set(json,'$.createdEntry',json('false')) WHERE receipt_id=NEW.receipt_id; END").unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    assert!(receipt::insert_once(&tx, &rec).is_err());
    tx.rollback().unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM receipts", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn stored_result_and_linked_receipt_are_required_together_and_never_repaired() {
    let _home = IsolatedAppHome::acquire("transition-store-chain");
    let fixture = Fixture::new(ProjectAllocation::Unassigned {});
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let rec = fixture.linked(&tx);
    assert_eq!(fixture.publication().current(&tx).unwrap().len(), 4);
    for fault in ["receipt", "result", "result-type", "snapshot"] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        match fault {
            "receipt" => {
                tx.execute("DELETE FROM receipts WHERE phase='linked'", [])
                    .unwrap();
            }
            "result" => {
                tx.execute("DELETE FROM meta WHERE key LIKE '%:stored'", [])
                    .unwrap();
            }
            "result-type" => {
                tx.execute("UPDATE meta SET value=X'00' WHERE key LIKE '%:stored'", [])
                    .unwrap();
            }
            "snapshot" => {
                tx.execute("UPDATE receipts SET json=json_set(json,'$.projectNameAtLink','forged') WHERE phase='linked'", []).unwrap();
            }
            _ => unreachable!(),
        }
        let changes = tx.total_changes();
        assert!(fixture.publication().current(&tx).is_err(), "{fault}");
        assert_eq!(changes, tx.total_changes());
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
    fixture.publication().stored(&tx, &rec).unwrap();
}

#[test]
fn final_result_binds_exact_ack_slot_item_version_and_durable_phase() {
    let _home = IsolatedAppHome::acquire("transition-final-ack");
    let fixture = Fixture::new(ProjectAllocation::Unassigned {});
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let linked = fixture.linked(&tx);
    let item = JobItem {
        candidate_id: fixture.item.candidate_id.clone(),
        title: Some("Synthetic 中文".into()),
        phase: "committed".into(),
        import_id: linked.import_id.clone(),
        revision_id: linked.revision_id.clone(),
        outcome: Some("complete".into()),
        error: None,
        mapping_project_id: None,
    };
    // Deliberate projections for a primitive verifier test, not an accepted job.
    let job = JobRecord {
        job_id: linked.job_id.clone(),
        request_id: "request".into(),
        plan_id: fixture.allocated.body.binding.plan_id.clone(),
        plan_hash: "b".repeat(64),
        state: JobState::Running,
        state_version: 2,
        items: vec![item.clone()],
        created_at: "synthetic".into(),
        item_count: 1,
        next_item_cursor: None,
        progress: None,
    };
    jobs::save_job(&tx, &job).unwrap();
    tx.execute("INSERT INTO job_items(job_id,candidate_id,phase,result_json) VALUES(?1,?2,'committed','{}')", params![job.job_id, item.candidate_id]).unwrap();
    let stored = fixture.publication().stored(&tx, &linked).unwrap();
    let mut rec = linked.clone();
    rec.phase = "committed".into();
    rec.receipt_id =
        receipt::phase_id(fixture.allocated.allocation().unwrap(), "committed").unwrap();
    let sealed = seal(
        &fixture.auth,
        fixture.allocated.body.binding.clone(),
        Stage::Committed(Committed {
            store_digest: stored.digest,
            receipt_digest: receipt::digest(&rec).unwrap(),
            item_digest: sha256_bytes(canonical(&item).unwrap().as_bytes()),
        }),
    )
    .unwrap();
    receipt::insert_once(&tx, &rec).unwrap();
    journal::insert(&tx, &fixture.auth, &sealed).unwrap();
    let changes = tx.total_changes();
    for _ in 0..10 {
        assert_eq!(
            fixture
                .publication()
                .require_committed(&tx, &rec, &item, 0, Some(2))
                .unwrap(),
            2
        );
    }
    assert_eq!(tx.total_changes(), changes);
    // A later job-header transition may advance the consistent version without
    // changing this exact item ACK. Replay reports that verified current
    // version; it must not assume the original final-writer version is current.
    tx.execute_batch("SAVEPOINT later_header").unwrap();
    tx.execute(
        "UPDATE jobs SET state_version=3,json=json_set(json,'$.stateVersion',3)",
        [],
    )
    .unwrap();
    let changes = tx.total_changes();
    assert_eq!(
        fixture
            .publication()
            .require_committed(&tx, &rec, &item, 0, None)
            .unwrap(),
        3
    );
    assert_eq!(tx.total_changes(), changes);
    assert!(fixture
        .publication()
        .require_committed(&tx, &rec, &item, 0, Some(2))
        .is_err());
    tx.execute_batch("ROLLBACK TO later_header; RELEASE later_header")
        .unwrap();
    assert!(fixture
        .publication()
        .require_committed(&tx, &rec, &item, 1, Some(2))
        .is_err());
    assert!(fixture
        .publication()
        .require_committed(&tx, &rec, &item, 0, Some(3))
        .is_err());
    for fault in [
        "item",
        "phase",
        "json-version",
        "column-version",
        "unknown-ack",
        "missing-final",
        "missing-receipt",
    ] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        match fault {
            "item" => {
                tx.execute(
                    "UPDATE jobs SET json=json_set(json,'$.items[0].mappingProjectId','foreign')",
                    [],
                )
                .unwrap();
            }
            "phase" => {
                tx.execute("UPDATE job_items SET phase='running'", [])
                    .unwrap();
            }
            "json-version" => {
                tx.execute("UPDATE jobs SET json=json_set(json,'$.stateVersion',3)", [])
                    .unwrap();
            }
            "column-version" => {
                tx.execute("UPDATE jobs SET state_version=3", []).unwrap();
            }
            "unknown-ack" => {
                tx.execute(
                    "UPDATE jobs SET json=json_set(json,'$.items[0].unknown',true)",
                    [],
                )
                .unwrap();
            }
            "missing-final" => {
                tx.execute("DELETE FROM meta WHERE key LIKE '%:committed'", [])
                    .unwrap();
            }
            "missing-receipt" => {
                tx.execute("DELETE FROM receipts WHERE phase='committed'", [])
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = tx.total_changes();
        let valid = fixture
            .publication()
            .require_latest(&tx, &rec)
            .and_then(|()| {
                fixture
                    .publication()
                    .require_committed(&tx, &rec, &item, 0, Some(2))
            });
        assert!(valid.is_err(), "{fault}");
        assert_eq!(tx.total_changes(), before);
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
}

#[test]
fn actual_native_result_distinguishes_created_recovered_match_alias_and_fallback() {
    let home = IsolatedAppHome::acquire("transition-native-result");
    let path = home.path.join("target");
    std::fs::create_dir(&path).unwrap();
    let fixture = Fixture::new(ProjectAllocation::Create {
        project_id: uuid::Uuid::new_v4().to_string(),
        path_digest: sha256_bytes(path.to_str().unwrap().as_bytes()),
        allow_unassigned: true,
    });
    let mut rec = fixture.receipt();
    let created = native_link::apply(
        &fixture.allocated,
        &fixture.auth,
        path.to_str().unwrap(),
        &mut rec,
    )
    .unwrap();
    assert!(matches!(created, ProjectResult::Created { .. }));
    assert!(rec.created_project);
    let before = std::fs::read(crate::paths::projects_file()).unwrap();
    let mut rec = fixture.receipt();
    assert!(matches!(
        native_link::apply(
            &fixture.allocated,
            &fixture.auth,
            path.to_str().unwrap(),
            &mut rec
        )
        .unwrap(),
        ProjectResult::Created { .. }
    ));
    assert!(std::fs::read(crate::paths::projects_file()).unwrap() == before);
    // A corrupt origin is never an explicit-unassigned success.
    let mut rows: serde_json::Value = serde_json::from_slice(&before).unwrap();
    rows[0]["importPublicationOrigin"] = serde_json::json!("corrupt");
    crate::store_lock::write_bytes_replace(
        &crate::paths::projects_file(),
        &serde_json::to_vec(&rows).unwrap(),
    )
    .unwrap();
    assert!(native_link::apply(
        &fixture.allocated,
        &fixture.auth,
        path.to_str().unwrap(),
        &mut fixture.receipt()
    )
    .is_err());
    let foreign_path = home.path.join("foreign");
    std::fs::create_dir(&foreign_path).unwrap();
    let foreign =
        crate::store::add_project(foreign_path.to_string_lossy().into_owned(), false).unwrap();
    for target in [foreign_path.clone(), foreign_path.join(".")] {
        let fixture = Fixture::new(ProjectAllocation::Create {
            project_id: uuid::Uuid::new_v4().to_string(),
            path_digest: sha256_bytes(target.to_str().unwrap().as_bytes()),
            allow_unassigned: false,
        });
        let mut rec = fixture.receipt();
        let result = native_link::apply(
            &fixture.allocated,
            &fixture.auth,
            target.to_str().unwrap(),
            &mut rec,
        )
        .unwrap();
        assert!(matches!(
            result,
            ProjectResult::Matched { .. } | ProjectResult::MatchedAlias { .. }
        ));
        assert_eq!(rec.mapping_project_id.as_deref(), Some(foreign.id.as_str()));
        assert!(!rec.created_project);
    }
    let missing = home.path.join("not-created");
    let fixture = Fixture::new(ProjectAllocation::Create {
        project_id: uuid::Uuid::new_v4().to_string(),
        path_digest: sha256_bytes(missing.to_str().unwrap().as_bytes()),
        allow_unassigned: true,
    });
    let mut rec = fixture.receipt();
    assert!(matches!(
        native_link::apply(
            &fixture.allocated,
            &fixture.auth,
            missing.to_str().unwrap(),
            &mut rec
        )
        .unwrap(),
        ProjectResult::UnassignedReported {}
    ));
    assert!(!rec.created_project);
    assert!(rec.mapping_project_id.is_none());
}

#[test]
fn native_storage_failure_cannot_be_reported_as_an_allowed_fallback() {
    let home = IsolatedAppHome::acquire("transition-native-storage");
    let existing = home.path.join("existing");
    std::fs::create_dir(&existing).unwrap();
    crate::store::add_project(existing.to_string_lossy().into_owned(), false).unwrap();
    let path = home.path.join("target");
    std::fs::create_dir(&path).unwrap();
    let fixture = Fixture::new(ProjectAllocation::Create {
        project_id: uuid::Uuid::new_v4().to_string(),
        path_digest: sha256_bytes(path.to_str().unwrap().as_bytes()),
        allow_unassigned: true,
    });
    let before = std::fs::read(crate::paths::projects_file()).unwrap();
    let temporary = crate::paths::projects_file().with_file_name("projects.json.tmp");
    std::fs::create_dir(&temporary).unwrap();
    assert!(native_link::apply(
        &fixture.allocated,
        &fixture.auth,
        path.to_str().unwrap(),
        &mut fixture.receipt()
    )
    .is_err());
    assert!(std::fs::read(crate::paths::projects_file()).unwrap() == before);
    std::fs::remove_dir(temporary).unwrap();
}

#[test]
fn explicit_match_path_drift_is_a_refusal_or_an_explicit_unassigned_result() {
    let home = IsolatedAppHome::acquire("transition-native-match");
    let path = home.path.join("target");
    std::fs::create_dir(&path).unwrap();
    let project = crate::store::add_project(path.to_string_lossy().into_owned(), false).unwrap();
    for allow_unassigned in [false, true] {
        let fixture = Fixture::new(ProjectAllocation::Match {
            project_id: project.id.clone(),
            path_digest: sha256_bytes(project.path.as_bytes()),
            allow_unassigned,
        });
        let mut rec = fixture.receipt();
        assert!(matches!(
            native_link::apply(&fixture.allocated, &fixture.auth, "unused", &mut rec).unwrap(),
            ProjectResult::Matched { .. }
        ));
        assert!(!rec.created_project);
    }
    for allow_unassigned in [false, true] {
        let fixture = Fixture::new(ProjectAllocation::Match {
            project_id: project.id.clone(),
            path_digest: "0".repeat(64),
            allow_unassigned,
        });
        let mut rec = fixture.receipt();
        let result = native_link::apply(&fixture.allocated, &fixture.auth, "unused", &mut rec);
        if allow_unassigned {
            assert!(matches!(
                result.unwrap(),
                ProjectResult::UnassignedReported {}
            ));
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn receipt_point_reader_keeps_uncapped_legacy_body_and_strict_original_json() {
    let _home = IsolatedAppHome::acquire("transition-receipt-large");
    let fixture = Fixture::new(ProjectAllocation::Unassigned {});
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let mut rec = fixture.receipt();
    // The point reader is shape-only; normal V3 phase validation separately
    // refuses a planned receipt containing native result fields. No new body
    // cap is imposed on the existing receipt2 storage format itself.
    rec.project_name_at_link = Some("x".repeat(17 * 1024 * 1024));
    receipt::insert_once(&tx, &rec).unwrap();
    assert_eq!(
        receipt::read(&tx, &rec.receipt_id)
            .unwrap()
            .unwrap()
            .project_name_at_link
            .unwrap()
            .len(),
        17 * 1024 * 1024
    );
    let original = serde_json::to_string(&rec).unwrap();
    for raw in [
        original.replacen("\"version\":2", "\"version\":2,\"version\":2", 1),
        original.replacen("\"version\":2", "\"unknown\":true,\"version\":2", 1),
        "null".into(),
    ] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        tx.execute(
            "UPDATE receipts SET json=?2 WHERE receipt_id=?1",
            params![rec.receipt_id, raw],
        )
        .unwrap();
        let changes = tx.total_changes();
        assert!(receipt::read(&tx, &rec.receipt_id).is_err());
        assert_eq!(tx.total_changes(), changes);
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
    tx.execute(
        "UPDATE receipts SET json=X'FF' WHERE receipt_id=?1",
        [&rec.receipt_id],
    )
    .unwrap();
    assert!(receipt::read(&tx, &rec.receipt_id).is_err());
}

#[test]
fn actual_result_cannot_disagree_with_linked_receipt_or_promote_a_match() {
    let home = IsolatedAppHome::acquire("transition-result-receipt");
    let path = home.path.join("target");
    std::fs::create_dir(&path).unwrap();
    let fixture = Fixture::new(ProjectAllocation::Create {
        project_id: uuid::Uuid::new_v4().to_string(),
        path_digest: sha256_bytes(path.to_str().unwrap().as_bytes()),
        allow_unassigned: false,
    });
    let mut rec = fixture.receipt();
    let result = native_link::apply(
        &fixture.allocated,
        &fixture.auth,
        path.to_str().unwrap(),
        &mut rec,
    )
    .unwrap();
    let allocation = fixture.allocated.allocation().unwrap();
    native_link::require_result(allocation, &rec, &result).unwrap();
    for fault in ["id", "ownership", "path", "decision", "snapshot"] {
        let mut changed = rec.clone();
        match fault {
            "id" => changed.mapping_project_id = Some("foreign".into()),
            "ownership" => changed.created_project = false,
            "path" => changed.project_path_at_link = Some("foreign-path".into()),
            "decision" => changed.project_decision = Some("match".into()),
            "snapshot" => changed.project_pinned_at_link = None,
            _ => unreachable!(),
        }
        assert!(
            native_link::require_result(allocation, &changed, &result).is_err(),
            "{fault}"
        );
    }
    let ProjectResult::Created {
        project_id,
        path_digest,
        snapshot_digest,
    } = result
    else {
        panic!("expected actual native creation");
    };
    let matched = ProjectResult::Matched {
        project_id,
        path_digest,
        snapshot_digest,
    };
    assert!(native_link::require_result(allocation, &rec, &matched).is_err());
}
