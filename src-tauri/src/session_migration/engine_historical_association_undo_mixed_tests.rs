//! Genuine mixed accepted jobs: one bad association must not partially Undo
//! an ordinary member placed before it in the mutable result order.
use super::*;

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_mixed_results_refuse_before_any_write() {
    for fault in ["foreign-previous", "coordinated-foreign-result"] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let association = confirmed_plan();
        let previews = cursor_source(&home, 2);
        let ordinary = plan(vec![previews[0].clone()], vec![]).unwrap();
        let unrelated = plan(vec![previews[1].clone()], vec![]).unwrap();
        let foreign = commit(
            &unrelated.plan_id,
            &unrelated.plan_hash,
            "synthetic-mixed-undo-independent",
        )
        .unwrap();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        let foreign = job_get(&foreign.job_id).unwrap();
        assert_eq!(foreign.items[0].phase, "committed");
        let foreign_import = foreign.items[0].import_id.as_deref().unwrap();
        let foreign_revision = foreign.items[0].revision_id.as_deref().unwrap();
        let mut selection = association.selection_set.clone();
        selection.entries.extend(ordinary.selection_set.entries);
        let mut preview_ids = association.preview_ids;
        preview_ids.extend(ordinary.preview_ids);
        let mixed = AssociationPlanRequest {
            version: 1,
            preview_ids,
            mappings: vec![],
            selection_set: selection,
            identity_associations: association.identity_associations,
        }
        .build()
        .unwrap();
        let queued = acceptance(&mixed, &format!("synthetic-mixed-undo-{fault}")).unwrap();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        let completed = job_get(&queued.job_id).unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.items.len(), 2);
        assert!(completed.items.iter().all(|item| item.phase == "committed"));
        let associated = completed
            .items
            .iter()
            .find(|item| item.import_id.as_deref() == Some(IMPORT))
            .unwrap();
        let ordinary = completed
            .items
            .iter()
            .find(|item| item.candidate_id != associated.candidate_id)
            .unwrap();
        let ordinary_import = ordinary.import_id.as_deref().unwrap();
        let ordinary_revision = ordinary.revision_id.as_deref().unwrap();
        assert_ne!(ordinary_import, foreign_import);
        assert_ne!(ordinary_import, IMPORT);
        let association_revision = associated.revision_id.as_deref().unwrap();
        let archive_pairs = [
            (IMPORT, REVISION),
            (IMPORT, association_revision),
            (ordinary_import, ordinary_revision),
            (foreign_import, foreign_revision),
        ];
        let hashes: Vec<_> = archive_pairs
            .iter()
            .map(|(import, revision)| {
                let path = catalog::revision_dir(import, revision).unwrap();
                let hashes = jobs::published_hashes(&path).unwrap();
                (path, hashes)
            })
            .collect();
        for (import, revision) in archive_pairs {
            assert!(events_page(import, Some(revision), None, Some(20), None).is_ok());
        }
        let receipt = jobs::latest_receipt(&conn, &completed.job_id, &associated.candidate_id)
            .unwrap()
            .unwrap();
        let mut changed_receipt = serde_json::to_value(&receipt).unwrap();
        // Both members were genuinely accepted and published. Only the negative
        // fault permutes their mutable display order; it creates no membership.
        let mut changed_job = completed.clone();
        changed_job
            .items
            .sort_by_key(|item| item.candidate_id == associated.candidate_id);
        assert_eq!(changed_job.items[0].candidate_id, ordinary.candidate_id);
        if fault == "foreign-previous" {
            changed_receipt["previousRevision"] = json!(foreign_revision);
        } else {
            changed_receipt["importId"] = json!(foreign_import);
            changed_receipt["revisionId"] = json!(foreign_revision);
            changed_job.items[1].import_id = Some(foreign_import.into());
            changed_job.items[1].revision_id = Some(foreign_revision.into());
        }
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(
            "UPDATE receipts SET import_id=?1,revision_id=?2,json=?3 WHERE receipt_id=?4",
            params![
                changed_receipt["importId"].as_str().unwrap(),
                changed_receipt["revisionId"].as_str().unwrap(),
                changed_receipt.to_string(),
                receipt.receipt_id
            ],
        )
        .unwrap();
        tx.execute(
            "UPDATE jobs SET json=?1 WHERE job_id=?2",
            params![
                serde_json::to_string(&changed_job).unwrap(),
                completed.job_id
            ],
        )
        .unwrap();
        tx.commit().unwrap();
        assert!(conn.is_autocommit());
        let catalog_before = raw_catalog_rows(&conn);
        let native_before = native_store_bytes();
        let result = engine::undo_import_job(&completed.job_id);
        let unchanged = raw_catalog_rows(&conn) == catalog_before;
        println!(
            "ASSOCIATION_UNDO_MIXED={}",
            json!({"fault":fault,"ordinaryFirst":true,"code":result.as_ref().err().map(|error| error.code.as_str()),"catalogUnchanged":unchanged})
        );
        let error = result.expect_err("bad association must refuse before ordinary Undo");
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert!(unchanged, "mixed refusal partially changed catalog");
        assert!(native_store_bytes() == native_before);
        for (path, expected) in hashes {
            assert_eq!(jobs::published_hashes(&path).unwrap(), expected);
        }
        assert_eq!(
            get_entry(&conn, ordinary_import)
                .unwrap()
                .unwrap()
                .current_revision
                .as_deref(),
            Some(ordinary_revision)
        );
        assert!(!get_entry(&conn, ordinary_import).unwrap().unwrap().hidden);
        require_seed(
            &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
            &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
        );
    }
}
