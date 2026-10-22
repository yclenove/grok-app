//! Synthetic acceptance/allocation primitives, not source authorization or Undo.
use super::{
    fixture::{self, Fixture},
    *,
};
use crate::session_migration::{
    catalog, mapping_scope,
    publication_outcome::{Allocation, PreparedAllocation, ProjectAllocation},
};
use rusqlite::{params, Connection, Transaction};

fn allocation_key(fixture: &Fixture, index: usize) -> String {
    format!(
        "publication_outcome:v1:{}:{}:allocated",
        fixture.job.job_id,
        hash::sha256_bytes(fixture.prepared[index].0.candidate_id.as_bytes())
    )
}

fn loaded(
    tx: &Transaction<'_>,
    auth: &InstallationAuthenticator,
    fixture: &Fixture,
    index: usize,
) -> Allocation {
    journal::load(tx, auth, &fixture.job.job_id)
        .unwrap()
        .allocated(tx, &fixture.prepared[index].0.candidate_id)
        .unwrap()
        .allocation()
        .unwrap()
        .clone()
}

fn mapping(
    fixture: &mut Fixture,
    project: Option<String>,
    create: bool,
    fallback: bool,
    path: &str,
) {
    assert_eq!(fixture.prepared.len(), 1);
    let input = &mut fixture.prepared[0].0;
    let subject = mapping_scope::MappingSubject::new(
        input.source_id.clone(),
        input.provider.clone(),
        input.adapter.graph.cwd.clone(),
    );
    let mut row = mapping_scope::propose(&[subject], project.clone()).remove(0);
    row.create_project = create;
    row.import_unassigned_anyway = fallback;
    row.explicit_target = Some(path.into());
    input.options = mapping_scope::options(Some(&row));
    input.mapping_project_id = project;
    input.create_project = create;
    input.import_unassigned_anyway = fallback;
    input.explicit_target = Some(path.into());
    let mut plan: Value = serde_json::from_str(&fixture.plan).unwrap();
    plan["mappings"] = json!([row]);
    plan.as_object_mut().unwrap().remove("planHash");
    fixture.job.plan_hash = hash::options_hash(&plan);
    plan["planHash"] = json!(fixture.job.plan_hash);
    fixture.plan = plan.to_string();
}

fn insert_entry(conn: &Connection, fixture: &Fixture, import: &str, revision: &str) {
    let input = &fixture.prepared[0].0;
    conn.execute("INSERT INTO entries(import_id,installation_id,provider,native_session_id,title,current_revision,created_at) VALUES(?1,?2,?3,?4,'synthetic',?5,?6)", params![import, input.installation_id, input.provider, input.adapter.graph.native_session_id, revision, fixture.job.created_at]).unwrap();
}

#[test]
fn accepted_allocations_are_atomic_durable_distinct_and_exact_retry_only() {
    let _home = IsolatedAppHome::acquire("accepted-allocation-birth");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    // Allocation and acceptance roll back with the original job, not a worker
    // attempt. Reusing this exact producer after rollback preserves its IDs.
    let tx = conn.unchecked_transaction().unwrap();
    fixture.seed(&tx);
    producer.record(&tx, &fixture.job).unwrap();
    let first = loaded(&tx, &auth, &fixture, 0);
    let second = loaded(&tx, &auth, &fixture, 1);
    assert_ne!(first.receipt_id, second.receipt_id);
    assert_ne!(first.import_id, second.import_id);
    assert_ne!(first.revision_id, second.revision_id);
    assert!(first.created_entry && first.previous_revision.is_none());
    assert!(matches!(first.project, ProjectAllocation::Unassigned {}));
    tx.rollback().unwrap();
    let count: u32 = conn
        .query_row("SELECT count(*) FROM jobs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(fixture.metadata_count(&conn), 0);
    let tx = conn.unchecked_transaction().unwrap();
    fixture.seed(&tx);
    producer.record(&tx, &fixture.job).unwrap();
    assert!(loaded(&tx, &auth, &fixture, 0) == first);
    let changes = tx.total_changes();
    let before = fixture::sql_digest(&tx);
    producer.record(&tx, &fixture.job).unwrap();
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(fixture::sql_digest(&tx), before);
    tx.commit().unwrap();
    drop(conn);
    let conn = catalog::open_catalog().unwrap();
    let recaptured = fixture.capture();
    let tx = conn.unchecked_transaction().unwrap();
    let before = fixture::sql_digest(&tx);
    let changes = tx.total_changes();
    assert!(loaded(&tx, &auth, &fixture, 0) == first);
    assert!(recaptured.record(&tx, &fixture.job).is_err());
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(fixture::sql_digest(&tx), before);
    producer.record(&tx, &fixture.job).unwrap();
    assert_eq!(tx.total_changes(), changes);
}

#[test]
fn accepted_allocation_missing_corrupt_and_valid_resigned_replacement_are_not_repaired() {
    let _home = IsolatedAppHome::acquire("accepted-allocation-no-remint");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    let mut foreign = loaded(&tx, &auth, &fixture, 1);
    foreign.receipt_id = uuid::Uuid::new_v4().to_string();
    let foreign = PreparedAllocation::new(foreign).unwrap();
    let binding = journal::load(&tx, &auth, &fixture.job.job_id)
        .unwrap()
        .outcome_binding(&tx, &fixture.prepared[1].0.candidate_id)
        .unwrap();
    for fault in ["missing", "corrupt", "blob", "oversized", "valid-resigned"] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        let key = allocation_key(&fixture, 1);
        match fault {
            "missing" | "valid-resigned" => {
                tx.execute("DELETE FROM meta WHERE key=?1", [&key]).unwrap();
            }
            "corrupt" => {
                tx.execute("UPDATE meta SET value='{}' WHERE key=?1", [&key])
                    .unwrap();
            }
            "blob" => {
                tx.execute(
                    "UPDATE meta SET value=CAST(value AS BLOB) WHERE key=?1",
                    [&key],
                )
                .unwrap();
            }
            "oversized" => {
                tx.execute(
                    "UPDATE meta SET value=printf('%09000d',0) WHERE key=?1",
                    [&key],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        if fault == "valid-resigned" {
            foreign.record(&tx, &auth, binding.clone()).unwrap();
        }
        let before = fixture::sql_digest(&tx);
        let changes = tx.total_changes();
        let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
        assert!(
            accepted
                .allocated(&tx, &fixture.prepared[1].0.candidate_id)
                .is_err(),
            "{fault}"
        );
        assert!(producer.record(&tx, &fixture.job).is_err(), "{fault}");
        assert_eq!(
            tx.total_changes(),
            changes,
            "no inserts or repair on {fault}"
        );
        assert_eq!(fixture::sql_digest(&tx), before);
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
}

#[test]
fn accepted_allocation_reader_is_snapshot_bound_and_does_not_use_mutable_results() {
    let _home = IsolatedAppHome::acquire("accepted-allocation-reader");
    let fixture = Fixture::new(1);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    let original = loaded(&tx, &auth, &fixture, 0);
    tx.commit().unwrap();
    let other = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let other_tx = other.unchecked_transaction().unwrap();
    let candidate = &fixture.prepared[0].0.candidate_id;
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    assert!(accepted.allocated(&other_tx, candidate).is_err());
    assert!(accepted.allocated(&tx, "missing").is_err());
    tx.execute("UPDATE jobs SET json=json_set(json,'$.items[0].mappingProjectId','foreign','$.items[0].importId','foreign','$.items[0].revisionId','foreign') WHERE job_id=?1", [&fixture.job.job_id]).unwrap();
    assert!(accepted.allocated(&tx, candidate).is_err());
    assert!(loaded(&tx, &auth, &fixture, 0) == original);
    tx.execute(
        "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1",
        [&fixture.job.job_id],
    )
    .unwrap();
    let changes = tx.total_changes();
    let before = fixture::sql_digest(&tx);
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    assert!(accepted.allocated(&tx, candidate).is_err());
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(fixture::sql_digest(&tx), before);
}

#[test]
fn allocation_insert_trigger_faults_roll_back_acceptance_and_all_allocations() {
    let _home = IsolatedAppHome::acquire("accepted-allocation-trigger");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let before = fixture::sql_digest(&conn);
    // Production insertion follows candidate-digest ordering, not display
    // indices. Delete a row that is already present when the final insert fires.
    let mut order = [0, 1];
    order.sort_by_key(|index| {
        hash::sha256_bytes(fixture.prepared[*index].0.candidate_id.as_bytes())
    });
    for action in [
        format!("DELETE FROM meta WHERE key='{}';", allocation_key(&fixture, order[0])),
        "UPDATE job_items SET result_json=result_json || ' ';".into(),
        "UPDATE meta SET value=value || ' ' WHERE key LIKE 'job_entry_scope:%';".into(),
        "UPDATE jobs SET state_version=2,json=json_set(json,'$.stateVersion',2);".into(),
        format!("INSERT INTO entries(import_id,installation_id,provider,native_session_id,title,created_at) VALUES('imp:{}','{}','{}','{}','late','{}');", uuid::Uuid::new_v4(), fixture.prepared[0].0.installation_id, fixture.prepared[0].0.provider, fixture.prepared[0].0.adapter.graph.native_session_id, fixture.job.created_at),
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute_batch(&format!("CREATE TEMP TRIGGER allocation_late_fault AFTER INSERT ON meta WHEN NEW.key='{}' BEGIN {action} END;", allocation_key(&fixture, order[1]))).unwrap();
        assert!(producer.record(&tx, &fixture.job).is_err(), "late SQL action: {action}");
        tx.rollback().unwrap();
        assert_eq!(fixture::sql_digest(&conn), before);
        let allocations: u32 = conn.query_row("SELECT count(*) FROM meta WHERE key LIKE 'publication_outcome:%'", [], |row| row.get(0)).unwrap();
        assert_eq!(allocations, 0);
    }
}

#[test]
fn allocation_target_revision_is_rechecked_before_acceptance_writes() {
    let _home = IsolatedAppHome::acquire("accepted-allocation-target-cas");
    let fixture = Fixture::new(1);
    let conn = catalog::open_catalog().unwrap();
    let absent = fixture.capture();
    let import = crate::session_migration::ids::new_import_id();
    let revision = uuid::Uuid::new_v4().to_string();
    insert_entry(&conn, &fixture, &import, &revision);
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    let changes = tx.total_changes();
    assert_eq!(
        absent.record(&tx, &fixture.job).unwrap_err().code,
        ErrorCode::MappingConflict
    );
    assert_eq!(tx.total_changes(), changes);
    tx.rollback().unwrap();
    let existing = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    existing.record(&tx, &fixture.job).unwrap();
    let allocation = loaded(&tx, &auth, &fixture, 0);
    assert_eq!(allocation.import_id, import);
    assert_eq!(
        allocation.previous_revision.as_deref(),
        Some(revision.as_str())
    );
    assert!(!allocation.created_entry);
    tx.rollback().unwrap();
    for action in [
        "UPDATE entries SET current_revision=NULL",
        "DELETE FROM entries",
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(action, []).unwrap();
        let before = fixture::sql_digest(&tx);
        let changes = tx.total_changes();
        assert_eq!(
            existing.record(&tx, &fixture.job).unwrap_err().code,
            ErrorCode::MappingConflict
        );
        assert_eq!(tx.total_changes(), changes);
        assert_eq!(fixture::sql_digest(&tx), before);
        tx.rollback().unwrap();
    }
}

#[test]
fn allocation_project_intents_bind_create_match_and_explicit_fallback_without_store_writes() {
    let home = IsolatedAppHome::acquire("accepted-allocation-projects");
    let path = home.path.join("existing-project");
    std::fs::create_dir(&path).unwrap();
    let path = path.to_str().unwrap();
    let project = crate::store::add_project(path.into(), false).unwrap();
    let store_before = std::fs::read(crate::paths::projects_file()).unwrap();
    for (create, id, fallback) in [
        (true, None, false),
        (false, Some(project.id.clone()), false),
        (false, Some("missing-project".into()), true),
    ] {
        let mut fixture = Fixture::new(1);
        mapping(
            &mut fixture,
            id.clone(),
            create,
            fallback,
            "accepted-explicit-target",
        );
        let producer = fixture.capture();
        let auth = InstallationAuthenticator::load().unwrap();
        let conn = catalog::open_catalog().unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        fixture.seed(&tx);
        producer.record(&tx, &fixture.job).unwrap();
        let allocation = loaded(&tx, &auth, &fixture, 0);
        match allocation.project {
            ProjectAllocation::Create {
                project_id,
                path_digest,
                allow_unassigned,
            } => {
                assert!(create && !allow_unassigned);
                assert_ne!(project_id, project.id);
                assert!(uuid::Uuid::parse_str(&project_id).is_ok());
                assert_eq!(path_digest, hash::sha256_bytes(b"accepted-explicit-target"));
            }
            ProjectAllocation::Match {
                project_id,
                path_digest,
                allow_unassigned,
            } => {
                assert!(!create);
                assert_eq!(Some(project_id), id);
                assert_eq!(allow_unassigned, fallback);
                assert_eq!(
                    path_digest,
                    hash::sha256_bytes(if fallback {
                        b"accepted-explicit-target"
                    } else {
                        path.as_bytes()
                    })
                );
            }
            _ => panic!("explicit mapping must not silently become unassigned"),
        }
        tx.rollback().unwrap();
    }
    assert!(std::fs::read(crate::paths::projects_file()).unwrap() == store_before);
}

#[test]
fn allocation_preflight_refuses_writer_missing_match_and_ambiguous_create_before_key_load() {
    let _home = IsolatedAppHome::acquire("accepted-allocation-preflight");
    let conn = catalog::open_catalog().unwrap();
    for fault in ["writer", "missing-match", "create-and-match"] {
        let mut fixture = Fixture::new(1);
        if fault != "writer" {
            mapping(
                &mut fixture,
                Some("missing-project".into()),
                fault == "create-and-match",
                false,
                "synthetic-target",
            );
        }
        let capture = |conn: &Connection| {
            producer::PreparedAcceptance::capture(
                conn,
                &fixture.job,
                &fixture.plan,
                &hash::sha256_bytes(fixture.scope.as_bytes()),
                &fixture.prepared,
                &Fixture::identities(&fixture.prepared),
            )
        };
        if fault == "writer" {
            let tx = conn.unchecked_transaction().unwrap();
            assert!(capture(&tx).is_err());
            assert_eq!(fixture.metadata_count(&tx), 0);
        } else {
            assert_eq!(
                capture(&conn).err().unwrap().code,
                ErrorCode::MappingConflict
            );
        }
        assert!(!catalog::imports_dir().join(".cursor-hmac-secret").exists());
    }
}
