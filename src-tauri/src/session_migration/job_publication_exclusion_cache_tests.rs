use super::*;
use crate::session_migration::publication_write_fence::install;

const D: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn database() -> (Connection, String) {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);")
        .unwrap();
    let record = serde_json::json!({
        "version": 1, "jobId": "j", "contextDigest": D,
        "items": [{ "candidateId": "a", "tupleDigest": D, "inputDigest": D,
                    "fence": uuid::Uuid::new_v4().to_string() }],
    })
    .to_string();
    conn.execute(
        "INSERT INTO meta VALUES(?1,?2)",
        params![record_key("j"), record],
    )
    .unwrap();
    (conn, hash::sha256_bytes(record.as_bytes()))
}

fn tamper(conn: &Connection) {
    conn.execute(
        "UPDATE meta SET value=value || ' ' WHERE key=?1",
        [record_key("j")],
    )
    .unwrap();
}

#[test]
fn a_verified_record_is_reused_only_while_its_row_is_unchanged() {
    let (conn, digest) = database();
    let _fence = install(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    let first = verified_record(&tx, "j", &digest).unwrap();
    let again = verified_record(&tx, "j", &digest).unwrap();
    assert!(
        std::rc::Rc::ptr_eq(&first, &again),
        "unchanged row is reused"
    );
    tamper(&tx);
    assert!(
        verified_record(&tx, "j", &digest).is_err(),
        "changed bytes are re-verified"
    );
}

#[test]
fn a_rolled_back_change_still_forces_a_fresh_verification() {
    let (conn, digest) = database();
    let _fence = install(&conn);
    let first = {
        let tx = conn.unchecked_transaction().unwrap();
        verified_record(&tx, "j", &digest).unwrap()
    };
    let tx = conn.unchecked_transaction().unwrap();
    tamper(&tx);
    tx.rollback().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let fresh = verified_record(&tx, "j", &digest).unwrap();
    assert!(
        !std::rc::Rc::ptr_eq(&first, &fresh),
        "stale stamp is not reused"
    );
}

#[test]
fn unfenced_connections_always_read_and_verify_the_bytes() {
    let (conn, digest) = database();
    let tx = conn.unchecked_transaction().unwrap();
    verified_record(&tx, "j", &digest).unwrap();
    tamper(&tx);
    assert!(verified_record(&tx, "j", &digest).is_err());
}

#[test]
fn a_different_accepted_digest_never_hits_the_cache() {
    let (conn, digest) = database();
    let _fence = install(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    verified_record(&tx, "j", &digest).unwrap();
    assert!(verified_record(&tx, "j", D).is_err());
}
