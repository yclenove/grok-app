//! Result/effect primitives, NOT accepted source/producer grants. The concrete
//! Normal Undo authenticates real acceptance, never these synthetic seals.
use super::*;
use crate::session_migration::{
    publication_outcome::undo, selection::SelectionSet, types::AdapterOutput,
};

fn seed_catalog(fixture: &Fixture, tx: &Transaction<'_>, item: &JobItem) -> catalog::CatalogEntry {
    let input: serde_json::Value = serde_json::from_str(&fixture.input).unwrap();
    let adapter: AdapterOutput = serde_json::from_value(input["adapter"].clone()).unwrap();
    let selection: SelectionSet = serde_json::from_value(input["selectionSet"].clone()).unwrap();
    let allocation = fixture.allocated.allocation().unwrap();
    let entry = catalog::CatalogEntry {
        import_id: allocation.import_id.clone(),
        installation_id: fixture.rec.installation_id.clone(),
        provider: fixture.rec.provider.clone(),
        native_session_id: fixture.rec.native_session_id.clone(),
        title: "editable display title".into(),
        alias: Some("local alias".into()),
        hidden: false,
        current_revision: Some(allocation.revision_id.clone()),
        created_at: "synthetic".into(),
    };
    catalog::upsert_entry(tx, &entry).unwrap();
    let semantic = jobs::revision_identity::key_for_intent(
        &crate::session_migration::hash::options_hash(&input["options"]),
        &selection,
        None,
        false,
        None,
        false,
    )
    .unwrap();
    let path = catalog::entries_dir()
        .join(crate::session_migration::ids::import_id_dir_component(
            &entry.import_id,
        ))
        .join(&allocation.revision_id);
    catalog::insert_revision(
        tx,
        &allocation.revision_id,
        &entry.import_id,
        &fixture.rec.snapshot_hash,
        &semantic,
        item.outcome.as_deref().unwrap(),
        &serde_json::to_string(&adapter.inventory).unwrap(),
        &path.to_string_lossy(),
    )
    .unwrap();
    entry
}

fn point(fixture: &Fixture, tx: &Transaction<'_>) -> Result<undo::Point, ImportError> {
    job_control::inspect_recovery_input(&fixture.input, |original| {
        undo::point(tx, &fixture.auth, &fixture.allocated, &original)
    })
    .unwrap()
}

fn save_undone(fixture: &Fixture, tx: &Transaction<'_>) -> JobRecord {
    let json: String = tx
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&fixture.rec.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let mut job: JobRecord = serde_json::from_str(&json).unwrap();
    job.state = JobState::Completed;
    job.state_version += 1;
    job.items[0].phase = "undone".into();
    jobs::save_job(tx, &job).unwrap();
    job
}

#[test]
fn undo_terminal_rechecks_original_result_but_new_ack_and_catalog_effect_without_preview() {
    let home = IsolatedAppHome::acquire("undo-result-new-ack");
    let fixture = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let item = fixture.seed(&tx);
    let mut entry = seed_catalog(&fixture, &tx, &item);
    tx.commit().unwrap();
    drop(conn);
    std::fs::rename(&fixture.preview, home.path.join("disposed-preview")).unwrap();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let before = point(&fixture, &tx).unwrap();
    assert_eq!(
        require_ack(&tx, &fixture.rec.job_id, &item, 0, Some(3)).unwrap(),
        3
    );
    catalog::hide_entry(&tx, &entry.import_id).unwrap();
    catalog::insert_tombstone(
        &tx,
        &entry.installation_id,
        &entry.provider,
        &entry.native_session_id,
    )
    .unwrap();
    let job = save_undone(&fixture, &tx);
    let mut tombstoned = false;
    before
        .project_entry(&mut entry, &mut tombstoned, &job.items[0])
        .unwrap();
    let changes = tx.total_changes();
    for _ in 0..10 {
        let after = point(&fixture, &tx).unwrap();
        before.require_after(&after, &job.items[0]).unwrap();
        after.require_entry(&entry, tombstoned).unwrap();
        require_ack(&tx, &job.job_id, &job.items[0], 0, Some(job.state_version)).unwrap();
        assert!(require_ack(&tx, &job.job_id, &item, 0, Some(3)).is_err());
    }
    assert!(entry.hidden && tombstoned);
    assert_eq!(tx.total_changes(), changes);
    assert!(!fixture.preview.exists());
}

#[test]
fn undo_final_recheck_rejects_late_result_revision_entry_and_ack_faults_without_repair() {
    let _home = IsolatedAppHome::acquire("undo-result-late-fault");
    let fixture = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let item = fixture.seed(&tx);
    let mut expected_entry = seed_catalog(&fixture, &tx, &item);
    let before = point(&fixture, &tx).unwrap();
    catalog::hide_entry(&tx, &expected_entry.import_id).unwrap();
    catalog::insert_tombstone(
        &tx,
        &expected_entry.installation_id,
        &expected_entry.provider,
        &expected_entry.native_session_id,
    )
    .unwrap();
    let job = save_undone(&fixture, &tx);
    let mut tombstoned = false;
    before
        .project_entry(&mut expected_entry, &mut tombstoned, &job.items[0])
        .unwrap();
    for sql in [
        "UPDATE receipts SET json=json_set(json,'$.mappingProjectId','foreign') WHERE phase='committed'",
        "UPDATE receipts SET json=json_set(json,'$.createdProject',json('true')) WHERE phase='linked'",
        "DELETE FROM meta WHERE key LIKE '%:stored'",
        "DELETE FROM meta WHERE key LIKE '%:committed'",
        "UPDATE revisions SET inventory_json='{}'",
        "UPDATE entries SET alias='late foreign alias'",
        "UPDATE entries SET hidden=0",
        "DELETE FROM tombstones",
        "UPDATE jobs SET json=json_set(json,'$.items[0].mappingProjectId','foreign')",
        "UPDATE jobs SET json=json_set(json,'$.items[0].unknown',true)",
        "UPDATE jobs SET state_version=88",
        "UPDATE job_items SET phase='committed'",
    ] {
        tx.execute_batch("SAVEPOINT late_fault").unwrap();
        tx.execute_batch(sql).unwrap();
        let changes = tx.total_changes();
        let result = point(&fixture, &tx).and_then(|after| {
            before.require_after(&after, &job.items[0])?;
            after.require_entry(&expected_entry, tombstoned)?;
            require_ack(&tx, &job.job_id, &job.items[0], 0, Some(job.state_version)).map(|_| ())
        });
        assert!(result.is_err(), "late fault accepted: {sql}");
        assert_eq!(tx.total_changes(), changes, "verification wrote/ repaired: {sql}");
        tx.execute_batch("ROLLBACK TO late_fault; RELEASE late_fault").unwrap();
    }
}

#[test]
fn undo_effect_projection_keeps_later_conflicts_and_existing_tombstones_without_created_reuse() {
    let _home = IsolatedAppHome::acquire("undo-result-effects");
    let fixture = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let item = fixture.seed(&tx);
    let entry = seed_catalog(&fixture, &tx, &item);
    let before = point(&fixture, &tx).unwrap();
    let mut expected = item;
    expected.phase = "conflict".into();
    expected.error = Some("later revision kept".into());
    let mut projected = entry.clone();
    projected.current_revision = Some(uuid::Uuid::new_v4().to_string());
    let later = canonical(&projected).unwrap();
    let mut tombstoned = false;
    before
        .project_entry(&mut projected, &mut tombstoned, &expected)
        .unwrap();
    assert_eq!(canonical(&projected).unwrap(), later);
    assert!(!tombstoned);
    expected.phase = "undone".into();
    assert!(before
        .project_entry(&mut projected, &mut tombstoned, &expected)
        .is_err());
    tombstoned = true;
    before
        .project_entry(&mut projected, &mut tombstoned, &expected)
        .unwrap();
    assert_eq!(canonical(&projected).unwrap(), later);
    let mut shared = entry;
    tombstoned = false;
    expected.phase = "conflict".into();
    before
        .project_entry(&mut shared, &mut tombstoned, &expected)
        .unwrap();
    expected.phase = "undone".into();
    before
        .project_entry(&mut shared, &mut tombstoned, &expected)
        .unwrap();
    assert!(shared.hidden && tombstoned);
}
