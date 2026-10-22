//! Negative mutations over genuine public registered-enqueue producer facts.
use super::*;
use rusqlite::OptionalExtension;

fn meta_value(conn: &Connection, key: &str) -> String {
    conn.query_row("SELECT value FROM meta WHERE key=?1", [key], |row| {
        row.get(0)
    })
    .unwrap()
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_publication_exclusion_negatives() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 1);
    let planned = plan(previews, vec![]).unwrap();
    let queued = commit(
        &planned.plan_id,
        &planned.plan_hash,
        "synthetic-exclusion-negative-producer",
    )
    .unwrap();
    let item =
        job_control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
    let ticket_key = format!("job_publication_exclusion:{}", queued.job_id);
    let scope_key = format!("job_entry_scope:{}", queued.job_id);
    let accepted_key = format!("job_entry_scope_accepted:{}", queued.job_id);
    let encoded = meta_value(&conn, &ticket_key);
    let ticket: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    let scope: serde_json::Value = serde_json::from_str(&meta_value(&conn, &scope_key)).unwrap();
    assert_eq!(
        scope["publicationDigest"],
        hash::sha256_bytes(encoded.as_bytes())
    );
    assert_eq!(ticket["items"].as_array().unwrap().len(), 1);
    let fence_key = format!(
        "publication_tuple_fence:{}",
        ticket["items"][0]["tupleDigest"].as_str().unwrap()
    );
    let original = catalog_rows(&conn);
    let history = text(IMPORT, REVISION);
    for fault in [
        "missing-ticket",
        "blob-ticket",
        "oversized-ticket",
        "future-ticket",
        "unknown-ticket-field",
        "rewritten-ticket",
        "rewritten-fence-ticket",
        "wrong-candidate",
        "wrong-tuple",
        "wrong-input",
        "missing-fence",
        "changed-fence",
        "malformed-fence",
        "durable-input",
        "scope-digest",
        "missing-scope-witness",
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        match fault {
            "missing-ticket" => {
                tx.execute("DELETE FROM meta WHERE key=?1", [&ticket_key])
                    .unwrap();
            }
            "blob-ticket" => {
                tx.execute(
                    "UPDATE meta SET value=CAST(value AS BLOB) WHERE key=?1",
                    [&ticket_key],
                )
                .unwrap();
            }
            "oversized-ticket" => {
                tx.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![" ".repeat(16 * 1024 * 1024 + 1), ticket_key],
                )
                .unwrap();
            }
            "missing-fence" => {
                tx.execute("DELETE FROM meta WHERE key=?1", [&fence_key])
                    .unwrap();
            }
            "changed-fence" => {
                tx.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![Uuid::new_v4().to_string(), fence_key],
                )
                .unwrap();
            }
            "malformed-fence" => {
                tx.execute(
                    "UPDATE meta SET value='not-a-uuid' WHERE key=?1",
                    [&fence_key],
                )
                .unwrap();
            }
            "durable-input" => {
                tx.execute(
                    "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1",
                    [&queued.job_id],
                )
                .unwrap();
            }
            "missing-scope-witness" => {
                tx.execute("DELETE FROM meta WHERE key=?1", [&accepted_key])
                    .unwrap();
            }
            "scope-digest" => {
                let mut damaged = scope.clone();
                damaged["publicationDigest"] = json!("0".repeat(64));
                let raw = damaged.to_string();
                tx.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![raw, scope_key],
                )
                .unwrap();
                tx.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![hash::sha256_bytes(raw.as_bytes()), accepted_key],
                )
                .unwrap();
            }
            _ => {
                let mut damaged = ticket.clone();
                match fault {
                    "future-ticket" => damaged["version"] = json!(2),
                    "unknown-ticket-field" => damaged["uncheckedOrdinary"] = json!(true),
                    "rewritten-ticket" => damaged["contextDigest"] = json!("0".repeat(64)),
                    "rewritten-fence-ticket" => {
                        let nonce = Uuid::new_v4().to_string();
                        damaged["items"][0]["fence"] = json!(nonce);
                        tx.execute(
                            "UPDATE meta SET value=?1 WHERE key=?2",
                            params![nonce, fence_key],
                        )
                        .unwrap();
                    }
                    "wrong-candidate" => {
                        damaged["items"][0]["candidateId"] = json!("foreign-candidate")
                    }
                    "wrong-tuple" => damaged["items"][0]["tupleDigest"] = json!("0".repeat(64)),
                    "wrong-input" => damaged["items"][0]["inputDigest"] = json!("0".repeat(64)),
                    _ => unreachable!(),
                }
                let raw = damaged.to_string();
                tx.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![raw, ticket_key],
                )
                .unwrap();
                // Shape negatives still reach their actual codec behind a
                // checksum-correct independently accepted binding mutation.
                // The ticket+fence coordinated rewrite leaves the independent
                // scope binding intact and MUST NOT become a grant.
                if !matches!(
                    fault,
                    "rewritten-ticket" | "rewritten-fence-ticket" | "wrong-candidate"
                ) {
                    let mut rebound = scope.clone();
                    rebound["publicationDigest"] = json!(hash::sha256_bytes(raw.as_bytes()));
                    let scope_raw = rebound.to_string();
                    tx.execute(
                        "UPDATE meta SET value=?1 WHERE key=?2",
                        params![scope_raw, scope_key],
                    )
                    .unwrap();
                    tx.execute(
                        "UPDATE meta SET value=?1 WHERE key=?2",
                        params![hash::sha256_bytes(scope_raw.as_bytes()), accepted_key],
                    )
                    .unwrap();
                }
            }
        }
        let damaged = catalog_rows(&tx);
        let outcome = jobs::publish_item(&tx, &queued.job_id, &item, &|| false);
        let after = catalog_rows(&tx);
        tx.rollback().unwrap();
        assert!(outcome.is_err(), "{fault}: {outcome:?}");
        assert_eq!(
            after, damaged,
            "no repair/receipt/project/catalog effects: {fault}"
        );
        assert_eq!(catalog_rows(&conn), original);
        assert_eq!(text(IMPORT, REVISION), history);
        println!("PUBLICATION_EXCLUSION_NEGATIVE={fault}:refused:unchanged");
    }
    assert_eq!(job_control::run_pending().unwrap(), 1);
    ordinary_history(&job_get(&queued.job_id).unwrap());
    reset_runtime();
    assert_eq!(job_control::run_pending().unwrap(), 0);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_publication_exclusion_writer_aborts() {
    for stage in ["ticket", "fence"] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let previews = cursor_source(&home, 1);
        let planned = plan(previews, vec![]).unwrap();
        let requested = requested_job(&planned, &format!("synthetic-exclusion-abort-{stage}"));
        let original = catalog_rows(&conn);
        let prefix = if stage == "ticket" {
            "job_publication_exclusion:"
        } else {
            "publication_tuple_fence:"
        };
        conn.execute_batch(&format!("CREATE TRIGGER synthetic_exclusion_abort BEFORE INSERT ON meta WHEN NEW.key LIKE '{prefix}%' BEGIN SELECT RAISE(ABORT,'synthetic exclusion writer abort'); END;")).unwrap();
        let items = engine_commit::commit_items_for_plan(&planned, &requested, false).unwrap();
        let outcome = job_control::enqueue_registered(requested.clone(), items);
        let after = catalog_rows(&conn);
        conn.execute_batch("DROP TRIGGER synthetic_exclusion_abort")
            .unwrap();
        assert!(outcome.is_err(), "{stage}");
        assert_eq!(after, original, "full acceptance rollback: {stage}");
        assert!(!job_control::preview_retention::pending());
        let items = engine_commit::commit_items_for_plan(&planned, &requested, false).unwrap();
        let retry = job_control::enqueue_registered(requested.clone(), items).unwrap();
        assert_eq!(retry.job_id, requested.job_id);
        assert_eq!(job_control::run_pending().unwrap(), 1);
        ordinary_history(&job_get(&retry.job_id).unwrap());
        println!("PUBLICATION_EXCLUSION_WRITER_ABORT={stage}:rollback:same-request-retry");
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_publication_exclusion_detached_fence_race() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 1);
    let planned = plan(previews, vec![]).unwrap();
    let requested = requested_job(&planned, "synthetic-exclusion-detached-fence-race");
    let items = engine_commit::commit_items_for_plan(&planned, &requested, false).unwrap();
    let input: serde_json::Value =
        serde_json::from_str(&job_control::encode_input(&items[0]).unwrap()).unwrap();
    let key = format!(
        "publication_tuple_fence:{}",
        hash::options_hash(&json!([
            input["sourceId"],
            input["provider"],
            input["adapter"]["graph"]["nativeSessionId"]
        ]))
    );
    assert!(conn
        .query_row("SELECT value FROM meta WHERE key=?1", [&key], |row| row
            .get::<_, String>(
            0
        ))
        .optional()
        .unwrap()
        .is_none());
    let observed = std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_observed = observed.clone();
    let hook_key = key.clone();
    job_control::provenance_scope::test_after_capture(move || {
        let other = open_catalog().unwrap();
        other
            .execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2)",
                params![hook_key, Uuid::new_v4().to_string()],
            )
            .unwrap();
        *hook_observed.borrow_mut() = Some(catalog_rows(&other));
    });
    let outcome = job_control::enqueue_registered(requested.clone(), items);
    let after = catalog_rows(&conn);
    conn.execute("DELETE FROM meta WHERE key=?1", [&key])
        .unwrap();
    assert_eq!(outcome.unwrap_err().code, ErrorCode::PlanStale);
    assert_eq!(
        after,
        observed.borrow().as_ref().unwrap().clone(),
        "no partial acceptance after fence CAS race"
    );
    let items = engine_commit::commit_items_for_plan(&planned, &requested, false).unwrap();
    assert_eq!(
        job_control::enqueue_registered(requested.clone(), items)
            .unwrap()
            .job_id,
        requested.job_id
    );
    assert_eq!(job_control::run_pending().unwrap(), 1);
    println!(
        "PUBLICATION_EXCLUSION_FENCE_RACE=detached-after-capture:CAS-refusal:same-request-retry"
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_publication_exclusion_final_writer_metadata_only() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 1);
    let planned = plan(previews, vec![]).unwrap();
    let queued = commit(
        &planned.plan_id,
        &planned.plan_hash,
        "synthetic-exclusion-metadata-writer",
    )
    .unwrap();
    let item =
        job_control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
    let preview = item.prepared.as_ref().unwrap().path.clone();
    let moved = home.path.join("synthetic-preview-detached-at-linked");
    let hook_preview = preview.clone();
    let hook_moved = moved.clone();
    crate::session_migration::preview_lifecycle_tests::test_publish_point("linked", move || {
        std::fs::rename(hook_preview, hook_moved).unwrap();
    });
    let crypto =
        crate::session_migration::publication_crypto::JobCrypto::prepare(&conn, &queued.job_id)
            .unwrap();
    let result = jobs::publish_for_worker(&conn, &queued.job_id, 0, &item, &|| false, &crypto);
    let reached = moved.exists();
    if reached {
        std::fs::rename(&moved, &preview).unwrap();
    }
    assert!(reached, "test must reach the actual final writer boundary");
    assert_eq!(
        result.unwrap().expect_recorded().phase,
        "committed",
        "final writer cannot reopen a prepared preview"
    );
    drop(item);
    reset_runtime();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    ordinary_history(&job_get(&queued.job_id).unwrap());
    assert_eq!(job_control::run_pending().unwrap(), 0);
    println!("PUBLICATION_EXCLUSION_METADATA_WRITER=preview-detached-after-link:committed:exact-once-recovery");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_publication_exclusion_final_writer_fence_cas() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 1);
    let planned = plan(previews, vec![]).unwrap();
    let queued = commit(
        &planned.plan_id,
        &planned.plan_hash,
        "synthetic-exclusion-final-fence-CAS",
    )
    .unwrap();
    let item =
        job_control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
    let ticket: serde_json::Value = serde_json::from_str(&meta_value(
        &conn,
        &format!("job_publication_exclusion:{}", queued.job_id),
    ))
    .unwrap();
    let key = format!(
        "publication_tuple_fence:{}",
        ticket["items"][0]["tupleDigest"].as_str().unwrap()
    );
    let nonce = meta_value(&conn, &key);
    let hook_key = key.clone();
    let before_entries: Vec<_> = list_entries(&conn)
        .unwrap()
        .into_iter()
        .map(|entry| serde_json::to_value(entry).unwrap())
        .collect();
    crate::session_migration::preview_lifecycle_tests::test_publish_point("linked", move || {
        open_catalog()
            .unwrap()
            .execute(
                "UPDATE meta SET value=?1 WHERE key=?2",
                params![Uuid::new_v4().to_string(), hook_key],
            )
            .unwrap();
    });
    let crypto =
        crate::session_migration::publication_crypto::JobCrypto::prepare(&conn, &queued.job_id)
            .unwrap();
    let result = jobs::publish_for_worker(&conn, &queued.job_id, 0, &item, &|| false, &crypto);
    let after_entries: Vec<_> = list_entries(&conn)
        .unwrap()
        .into_iter()
        .map(|entry| serde_json::to_value(entry).unwrap())
        .collect();
    let refused_receipt = jobs::latest_receipt(&conn, &queued.job_id, &item.candidate_id).unwrap();
    conn.execute("UPDATE meta SET value=?1 WHERE key=?2", params![nonce, key])
        .unwrap();
    assert!(
        result.is_err(),
        "captured negative is not an unchecked grant in the final writer"
    );
    assert_eq!(after_entries, before_entries);
    let linked = refused_receipt.unwrap();
    assert_eq!(
        linked.phase, "linked",
        "real pre-commit receipt remains recoverable"
    );
    let linked_bytes: String = conn
        .query_row(
            "SELECT json FROM receipts WHERE receipt_id=?1",
            [&linked.receipt_id],
            |row| row.get(0),
        )
        .unwrap();
    let count_before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let retry = jobs::publish_for_worker(&conn, &queued.job_id, 0, &item, &|| false, &crypto)
        .unwrap()
        .expect_recorded();
    assert_eq!(retry.phase, "committed");
    let receipt = jobs::latest_receipt(&conn, &queued.job_id, &item.candidate_id)
        .unwrap()
        .unwrap();
    // Receipt IDs name append-only PHASE events, not the publication attempt.
    // Advancing linked -> committed adds one event, retaining the allocated IDs.
    assert_ne!(receipt.receipt_id, linked.receipt_id);
    assert_eq!(receipt.import_id, linked.import_id);
    assert_eq!(receipt.revision_id, linked.revision_id);
    assert_eq!(
        conn.query_row(
            "SELECT json FROM receipts WHERE receipt_id=?1",
            [&linked.receipt_id],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        linked_bytes
    );
    let count_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count_after,
        count_before + 1,
        "exactly one appended committed phase"
    );
    drop(item);
    reset_runtime();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    ordinary_history(&job_get(&queued.job_id).unwrap());
    assert_eq!(job_control::run_pending().unwrap(), 0);
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        count_after
    );
    println!(
        "PUBLICATION_EXCLUSION_FINAL_CAS=fence-changed-after-link:refused:same-receipt-recovery"
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_worker_shared_schema_refusal() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 1);
    let planned = plan(previews, vec![]).unwrap();
    let confirmed = confirmed_plan();
    let association = acceptance(&confirmed, "synthetic-shared-schema-head").unwrap();
    let later = commit(
        &planned.plan_id,
        &planned.plan_hash,
        "synthetic-shared-schema-later",
    )
    .unwrap();
    let key = crate::session_migration::registration_provenance_store::VERSION_KEY;
    let captured = meta_value(&conn, key);
    conn.execute("UPDATE meta SET value='invalid' WHERE key=?1", [key])
        .unwrap();
    let fault_state = catalog_rows(&conn);
    let outcome = job_control::run_pending();
    let after = catalog_rows(&conn);
    conn.execute(
        "UPDATE meta SET value=?1 WHERE key=?2",
        params![captured, key],
    )
    .unwrap();
    // EXACTLY the same public code as a malformed point proof. Only its typed
    // origin differs; a code/message whitelist would incorrectly drain here.
    let error = outcome.unwrap_err();
    assert_eq!(error.code, ErrorCode::SchemaUnsupported);
    assert!(!error.is_job_local());
    assert_eq!(
        after, fault_state,
        "shared failure must stop before any queued job mutation"
    );
    assert_eq!(
        job_get(&association.job_id).unwrap().state,
        JobState::Queued
    );
    assert_eq!(job_get(&later.job_id).unwrap().state, JobState::Queued);
    assert_eq!(job_control::run_pending().unwrap(), 2);
    ordinary_history(&job_get(&later.job_id).unwrap());
    reset_runtime();
    assert_eq!(job_control::run_pending().unwrap(), 0);
    println!("WORKER_SHARED_SCHEMA_REFUSAL=same-public-code:different-typed-origin:no-drain:restored-retry");
}
