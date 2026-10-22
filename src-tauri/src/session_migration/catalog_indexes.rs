//! Optional performance indexes. Rows and durable schema versions stay intact;
//! read-only callers can still browse an older catalog before writer upgrade.

use rusqlite::{Connection, TransactionBehavior};

use super::errors::{storage_error, ImportError};

pub(super) fn ensure_receipt_indexes(conn: &Connection) -> Result<(), ImportError> {
    let present: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index'
         AND name IN ('receipts_job_candidate', 'receipts_import_revision')",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if present == 2 {
        return Ok(());
    }
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(storage_error)?;
    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS receipts_job_candidate ON receipts(job_id,candidate_id);
         CREATE INDEX IF NOT EXISTS receipts_import_revision ON receipts(import_id,revision_id);",
    )
    .map_err(storage_error)?;
    tx.commit().map_err(storage_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_migration::catalog;
    use crate::session_migration::test_home::IsolatedAppHome;

    fn assert_indexed(conn: &Connection, predicate: &str, index: &str) {
        let sql = format!("EXPLAIN QUERY PLAN SELECT json FROM receipts WHERE {predicate} ORDER BY rowid DESC LIMIT 1");
        let plan = conn
            .prepare(&sql)
            .unwrap()
            .query_map([], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        assert!(plan.contains(&format!("USING INDEX {index}")), "{plan}");
        assert!(!plan.contains("USE TEMP B-TREE"), "{plan}");
    }

    #[test]
    fn existing_catalog_adds_receipt_indexes_without_changing_rows_or_version() {
        let _home = IsolatedAppHome::acquire("receipt-index-upgrade");
        let conn = catalog::open_catalog().unwrap();
        ensure_receipt_indexes(&conn).unwrap();
        conn.execute_batch(
            "DROP INDEX receipts_job_candidate; DROP INDEX receipts_import_revision;",
        )
        .unwrap();
        conn.execute("INSERT INTO receipts(receipt_id,job_id,candidate_id,phase,json,created_at) VALUES('legacy','job','candidate','prepared','opaque legacy receipt','date')", []).unwrap();
        let version: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);
        let conn = catalog::open_catalog().unwrap();
        ensure_receipt_indexes(&conn).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT json FROM receipts WHERE receipt_id='legacy'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "opaque legacy receipt"
        );
        assert_eq!(
            conn.query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            version
        );
        assert_indexed(
            &conn,
            "job_id='job' AND candidate_id='candidate'",
            "receipts_job_candidate",
        );
        assert_indexed(
            &conn,
            "import_id='import' AND revision_id='revision'",
            "receipts_import_revision",
        );
    }

    #[test]
    fn readonly_catalog_does_not_create_optional_receipt_indexes() {
        let _home = IsolatedAppHome::acquire("receipt-index-readonly");
        let conn = catalog::open_catalog().unwrap();
        ensure_receipt_indexes(&conn).unwrap();
        conn.execute_batch(
            "DROP INDEX receipts_job_candidate; DROP INDEX receipts_import_revision;",
        )
        .unwrap();
        drop(conn);
        let conn = catalog::open_catalog_readonly().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name LIKE 'receipts_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn concurrent_writer_upgrade_is_idempotent() {
        let _home = IsolatedAppHome::acquire("receipt-index-concurrent");
        let conn = catalog::open_catalog().unwrap();
        ensure_receipt_indexes(&conn).unwrap();
        conn.execute_batch(
            "DROP INDEX receipts_job_candidate; DROP INDEX receipts_import_revision;",
        )
        .unwrap();
        drop(conn);
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        ensure_receipt_indexes(&catalog::open_catalog().unwrap()).unwrap();
                    })
                })
                .collect();
            for thread in threads {
                thread.join().unwrap();
            }
        });
        let conn = catalog::open_catalog().unwrap();
        assert_indexed(
            &conn,
            "job_id='job' AND candidate_id='candidate'",
            "receipts_job_candidate",
        );
    }
}
