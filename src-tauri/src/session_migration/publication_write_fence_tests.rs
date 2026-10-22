use super::*;

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE job_items(job_id TEXT, candidate_id TEXT, phase TEXT, result_json TEXT);
         CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
         CREATE TABLE receipts(job_id TEXT, candidate_id TEXT, phase TEXT);
         INSERT INTO job_items VALUES('j','a','queued','{}'),('j','b','queued','{}');
         INSERT INTO meta VALUES('publication_acceptance:v1:j:header','h'),
                                ('publication_acceptance:v1:j:member:00','m'),
                                ('publication_outcome:j:a','o');",
    )
    .unwrap();
    conn
}

#[test]
fn unfenced_connections_fall_back_to_the_complete_snapshot() {
    let conn = database();
    assert!(!check(&conn, "j", "a").unwrap());
}

#[test]
fn own_row_and_ordinary_writes_pass_inside_one_transaction() {
    let conn = database();
    let _fence = install(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute(
        "UPDATE job_items SET phase='committed' WHERE candidate_id='a'",
        [],
    )
    .unwrap();
    tx.execute("INSERT INTO receipts VALUES('j','a','prepared')", [])
        .unwrap();
    tx.execute(
        "UPDATE meta SET value='o2' WHERE key='publication_outcome:j:a'",
        [],
    )
    .unwrap();
    assert!(check(&tx, "j", "a").unwrap());
}

#[test]
fn a_trigger_changing_another_items_input_is_refused() {
    let conn = database();
    conn.execute_batch(
        "CREATE TRIGGER corrupt AFTER INSERT ON receipts BEGIN
           UPDATE job_items SET result_json=result_json || ' ' WHERE candidate_id='b'; END;",
    )
    .unwrap();
    let _fence = install(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute("INSERT INTO receipts VALUES('j','a','prepared')", [])
        .unwrap();
    assert!(check(&tx, "j", "a").is_err());
}

#[test]
fn acceptance_rows_are_immutable_inside_publication() {
    for key in [
        "publication_acceptance:v1:j:header",
        "publication_acceptance:v1:j:member:00",
    ] {
        let conn = database();
        let _fence = install(&conn);
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute("UPDATE meta SET value='x' WHERE key=?1", [key])
            .unwrap();
        assert!(check(&tx, "j", "a").is_err(), "{key}");
    }
}

#[test]
fn an_unclassifiable_deletion_requires_the_complete_snapshot() {
    let conn = database();
    let _fence = install(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute("DELETE FROM meta WHERE key='publication_outcome:j:a'", [])
        .unwrap();
    assert!(!check(&tx, "j", "a").unwrap());
}

#[test]
fn commit_and_rollback_end_the_recorded_transaction_and_drop_uninstalls() {
    let conn = database();
    {
        let _fence = install(&conn);
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute("UPDATE job_items SET phase='x' WHERE candidate_id='b'", [])
            .unwrap();
        assert!(check(&tx, "j", "a").is_err());
        tx.commit().unwrap();
        assert!(check(&conn, "j", "a").unwrap());
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute("UPDATE job_items SET phase='y' WHERE candidate_id='b'", [])
            .unwrap();
        tx.rollback().unwrap();
        assert!(check(&conn, "j", "a").unwrap());
    }
    assert!(!check(&conn, "j", "a").unwrap());
}

#[test]
fn other_jobs_rows_may_change_and_deleted_rows_need_the_full_snapshot() {
    let conn = database();
    conn.execute("INSERT INTO job_items VALUES('k','z','committed','{}')", [])
        .unwrap();
    let _fence = install(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute(
        "UPDATE job_items SET phase='deduplicated' WHERE job_id='k'",
        [],
    )
    .unwrap();
    assert!(check(&tx, "j", "a").unwrap());
    tx.execute("DELETE FROM job_items WHERE job_id='k'", [])
        .unwrap();
    assert!(!check(&tx, "j", "a").unwrap());
}

#[test]
fn row_stamps_survive_unrelated_writes_and_expire_on_any_change_to_the_row() {
    let dir = std::env::temp_dir().join(format!("fence-stamp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("catalog.sqlite");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO meta VALUES('plan','p'),('other','o');",
        )
        .unwrap();
        let plan: i64 = conn
            .query_row("SELECT rowid FROM meta WHERE key='plan'", [], |r| r.get(0))
            .unwrap();
        assert!(row_stamp(&conn).unwrap().is_none(), "unfenced");
        let _fence = install(&conn);
        let stamp = row_stamp(&conn).unwrap().unwrap();
        // Writes to other rows, committed or not, keep the stamp valid.
        conn.execute("UPDATE meta SET value='o2' WHERE key='other'", [])
            .unwrap();
        assert!(row_unchanged(&conn, &stamp, "meta", plan).unwrap());
        // Any change to the row itself, even rolled back, expires it.
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute("UPDATE meta SET value='p2' WHERE key='plan'", [])
            .unwrap();
        tx.rollback().unwrap();
        assert!(!row_unchanged(&conn, &stamp, "meta", plan).unwrap());
        // A commit by another connection expires every stamp.
        let fresh = row_stamp(&conn).unwrap().unwrap();
        assert!(row_unchanged(&conn, &fresh, "meta", plan).unwrap());
        let other = Connection::open(&path).unwrap();
        other
            .execute("UPDATE meta SET value='x' WHERE key='other'", [])
            .unwrap();
        assert!(!row_unchanged(&conn, &fresh, "meta", plan).unwrap());
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_stamp_from_another_installation_is_never_valid() {
    let conn = database();
    let stamp = {
        let _first = install(&conn);
        row_stamp(&conn).unwrap().unwrap()
    };
    let _second = install(&conn);
    assert!(!row_unchanged(&conn, &stamp, "meta", 1).unwrap());
}
