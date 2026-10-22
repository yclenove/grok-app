//! Bounded negative point reads and genuine ordinary-producer controls.
//! SQL faults apply only to an isolated copy, never to positive authority.
use super::*;
use rusqlite::{params, Connection};

const STALE_OWNER: &str =
    "historical association no longer owns this entry; reimport through the registered source";

fn proof_rows(conn: &Connection) -> Vec<(i64, String, String, String)> {
    conn.prepare("SELECT rowid,import_id,hex(CAST(json AS BLOB)),hex(CAST(digest AS BLOB)) FROM registration_provenance ORDER BY rowid")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_old_owner_point_boundaries() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (queued, item) = queue_old(&home, false);
    let confirmed = confirmed_plan();
    acceptance(&confirmed, "synthetic-old-owner-point-boundaries").unwrap();
    let before = all_receipts(&conn);
    let entry = serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap();
    let proofs = proof_rows(&conn);
    let old_text = text(IMPORT, REVISION);
    let lease_path = item.prepared.as_ref().unwrap().path.join("lease.json");
    let lease = std::fs::read(&lease_path).unwrap();
    let original_pragma: bool = conn
        .pragma_query_value(None, "ignore_check_constraints", |row| row.get(0))
        .unwrap();
    for (fault, expected) in [
        ("index-import-oversize", ErrorCode::SchemaUnsupported),
        ("index-digest-oversize", ErrorCode::SchemaUnsupported),
        ("index-import-utf8", ErrorCode::SnapshotUnavailable),
        ("proof-json-oversize", ErrorCode::SchemaUnsupported),
        ("proof-json-utf8", ErrorCode::SnapshotUnavailable),
        ("owner-marker-oversize", ErrorCode::SnapshotUnavailable),
        ("owner-marker-utf8", ErrorCode::SnapshotUnavailable),
    ] {
        conn.pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        let affected = match fault {
            "index-import-oversize" | "index-digest-oversize" => {
                let (sql, bytes) = if fault == "index-import-oversize" {
                    ("UPDATE registration_association_old_owners SET import_id=?1", 41)
                } else {
                    ("UPDATE registration_association_old_owners SET proof_digest=?1", 65)
                };
                tx.execute(sql, ["a".repeat(bytes)]).unwrap()
            }
            "index-import-utf8" => {
                let mut bytes = vec![b'a'; 40];
                bytes[0] = 0xff;
                tx.execute("UPDATE registration_association_old_owners SET import_id=CAST(?1 AS TEXT)", [bytes]).unwrap()
            }
            "proof-json-oversize" => tx.execute(
                "UPDATE registration_provenance SET json=?1 WHERE import_id=?2",
                params![" ".repeat(32769), IMPORT],
            ).unwrap(),
            "proof-json-utf8" => tx.execute(
                "UPDATE registration_provenance SET json=CAST(x'ff' AS TEXT) WHERE import_id=?1",
                [IMPORT],
            ).unwrap(),
            "owner-marker-oversize" => tx.execute(
                "UPDATE meta SET value=?1 WHERE key LIKE 'registration_association_old_owner:%'",
                ["a".repeat(106)],
            ).unwrap(),
            "owner-marker-utf8" => {
                let mut bytes = vec![b'a'; 105];
                bytes[0] = 0xff;
                tx.execute("UPDATE meta SET value=CAST(?1 AS TEXT) WHERE key LIKE 'registration_association_old_owner:%'", [bytes]).unwrap()
            }
            _ => unreachable!(),
        };
        assert_eq!(
            affected, 1,
            "each fault must reach exactly one genuine owner"
        );
        let outcome = jobs::publish_item(&tx, &queued.job_id, &item, &|| false);
        // Restore both bytes and the connection setting before error assertions.
        let fault_receipts = all_receipts(&tx);
        let fault_entry = serde_json::to_value(get_entry(&tx, IMPORT).unwrap()).unwrap();
        tx.rollback().unwrap();
        conn.pragma_update(None, "ignore_check_constraints", original_pragma)
            .unwrap();
        let error = outcome.unwrap_err();
        assert_eq!(error.code, expected, "{fault}: {error:?}");
        match fault {
            "index-import-oversize" | "index-digest-oversize" => {
                assert_eq!(
                    error.message,
                    "association old-owner schema is incompatible"
                );
            }
            "proof-json-oversize" => {
                assert_eq!(
                    error.message,
                    "registration provenance schema is incompatible"
                );
            }
            "owner-marker-oversize" => {
                assert_eq!(error.message, "retention metadata byte length is invalid");
            }
            "owner-marker-utf8" => {
                assert_eq!(error.message, "retention metadata UTF-8 is invalid");
            }
            "index-import-utf8" | "proof-json-utf8" => {
                assert!(
                    error.message.contains("invalid utf-8 sequence"),
                    "{error:?}"
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(fault_receipts, before, "no receipts before rollback");
        assert_eq!(fault_entry, entry, "no entry mutation before rollback");
        assert_eq!(all_receipts(&conn), before);
        assert_eq!(proof_rows(&conn), proofs);
        assert_eq!(list_entries(&conn).unwrap().len(), 1);
        assert_eq!(std::fs::read(&lease_path).unwrap(), lease);
        assert_eq!(text(IMPORT, REVISION), old_text);
        let healthy = jobs::publish_item(&conn, &queued.job_id, &item, &|| false).unwrap_err();
        assert_eq!(healthy.code, ErrorCode::PlanStale);
        assert_eq!(healthy.message, STALE_OWNER);
        println!(
            "OLD_OWNER_POINT_BOUNDARY={fault}:{}:rollback:healthy-stale",
            error.code.as_str()
        );
    }
    assert_eq!(
        conn.pragma_query_value(None, "ignore_check_constraints", |row| row
            .get::<_, bool>(0))
            .unwrap(),
        original_pragma
    );
}

fn publish_association(conn: &Connection) -> String {
    let confirmed = confirmed_plan();
    let queued = acceptance(&confirmed, "synthetic-ordinary-after-association").unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = jobs::get_job(conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(completed.items[0].phase, "committed");
    assert_eq!(completed.items[0].import_id.as_deref(), Some(IMPORT));
    completed.items[0].revision_id.clone().unwrap()
}

fn publish_ordinary(conn: &Connection, ordinary: &PlanState, request: &str) -> JobRecord {
    assert!(ordinary.identity_associations.is_empty());
    let queued = commit(&ordinary.plan_id, &ordinary.plan_hash, request).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = jobs::get_job(conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(completed.state, JobState::Completed, "{completed:?}");
    assert_eq!(completed.items[0].phase, "committed", "{completed:?}");
    completed
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_unrelated_ordinary_publication() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let old_text = text(IMPORT, REVISION);
    let revision = publish_association(&conn);
    let before = receipts(&conn);
    let entry = serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap();
    let previews = cursor_source(&home, 1);
    let ordinary = plan(previews, vec![]).unwrap();
    let completed = publish_ordinary(&conn, &ordinary, "synthetic-unrelated-ordinary");
    let import = completed.items[0].import_id.as_deref().unwrap();
    assert_ne!(import, IMPORT);
    assert_eq!(list_entries(&conn).unwrap().len(), 2);
    assert_eq!(receipts(&conn), before);
    assert_eq!(
        serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap(),
        entry
    );
    assert_eq!(text(IMPORT, REVISION), old_text);
    assert_eq!(text(IMPORT, &revision), old_text);
    let (events, cursor, count) = events_page(
        import,
        completed.items[0].revision_id.as_deref(),
        None,
        Some(20),
        None,
    )
    .unwrap();
    assert!(cursor.is_none());
    assert_eq!(count, 1);
    assert_eq!(events.len(), 1);
    assert!(serde_json::to_string(&events)
        .unwrap()
        .contains("SYNTHETIC_REGISTERED_HISTORY_0"));
    println!("OLD_OWNER_UNRELATED_ORDINARY=real-scan:plan:registered-enqueue:worker:old-and-associated-readable");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_current_identity_ordinary_dedup() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let old_text = text(IMPORT, REVISION);
    let revision = publish_association(&conn);
    let before = all_receipts(&conn);
    let entry = serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap();
    reset_runtime();
    let source = sources::get(&conn, INSTALLATION).unwrap();
    let started = crate::session_migration::scan::begin(vec![ScanSourcePath {
        provider: "cursor".into(),
        path: source.root_path,
        source_id: Some(INSTALLATION.into()),
        location_id: None,
        host: Some("local".into()),
        profile: source.profile,
    }])
    .unwrap();
    crate::session_migration::scan::run(&started.scan_id).unwrap();
    let scan = scan_pump(&started.scan_id, started.epoch).unwrap();
    assert!(scan.complete && scan.issues.is_empty());
    assert_eq!(scan.candidates.len(), 1);
    let candidate = &scan.candidates[0];
    let (preview_id, _) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let ordinary = plan(vec![preview_id], vec![]).unwrap();
    let completed = publish_ordinary(
        &conn,
        &ordinary,
        "synthetic-current-identity-ordinary-dedup",
    );
    assert_eq!(completed.items[0].import_id.as_deref(), Some(IMPORT));
    assert_eq!(
        completed.items[0].revision_id.as_deref(),
        Some(revision.as_str())
    );
    assert_eq!(completed.items[0].outcome.as_deref(), Some("deduplicated"));
    assert_eq!(all_receipts(&conn), before);
    assert_eq!(
        serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap(),
        entry
    );
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    assert_eq!(text(IMPORT, REVISION), old_text);
    assert_eq!(text(IMPORT, &revision), old_text);
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
    println!("OLD_OWNER_CURRENT_ORDINARY=real-rescan:unchanged-source:dedup:same-import-and-revision:no-new-receipt");
}
