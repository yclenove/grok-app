use super::*;
use crate::session_migration::{
    errors::ErrorCode, publication_protocol::PublicationProtocol, test_home::IsolatedAppHome,
};

fn catalog() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE jobs(job_id TEXT PRIMARY KEY);
        CREATE TABLE job_items(job_id TEXT,candidate_id TEXT);
        INSERT INTO jobs VALUES('legacy');
        INSERT INTO jobs VALUES('abcdef12-1234-8567-8123-abcdef123456');
        INSERT INTO job_items VALUES('legacy','候选');
        INSERT INTO job_items VALUES('abcdef12-1234-8567-8123-abcdef123456','bad');",
    )
    .unwrap();
    conn
}

#[test]
fn crypto_set_selects_candidates_without_loading_unrelated_signed_owners() {
    let _home = IsolatedAppHome::acquire("crypto-set-selection");
    let conn = catalog();
    let cryptos = JobCryptos::prepare(
        &conn,
        &["候选".into(), "候选".into(), "' OR 1=1".into()],
        &[],
    )
    .unwrap();
    assert!(cryptos
        .get("legacy")
        .unwrap()
        .require("legacy", PublicationProtocol::Legacy)
        .unwrap()
        .is_none());
    assert_eq!(
        cryptos
            .get("abcdef12-1234-8567-8123-abcdef123456")
            .err()
            .unwrap()
            .code,
        ErrorCode::HistoryAccessUnavailable
    );
    assert!(!crate::session_migration::catalog::imports_dir()
        .join(".cursor-hmac-secret")
        .exists());
}

#[test]
fn crypto_set_preloads_selected_owners_without_granting_acceptance() {
    let _home = IsolatedAppHome::acquire("crypto-set-isolation");
    let conn = catalog();
    let cryptos =
        JobCryptos::prepare(&conn, &["候选".into(), "bad".into()], &["missing".into()]).unwrap();
    assert!(cryptos.get("legacy").is_ok());
    let signed = cryptos.get("abcdef12-1234-8567-8123-abcdef123456").unwrap();
    assert!(signed
        .require(
            "abcdef12-1234-8567-8123-abcdef123456",
            PublicationProtocol::SignedOutcomeV1
        )
        .unwrap()
        .is_some());
    assert!(crate::session_migration::jobs::get_job_with_crypto(
        &conn,
        "abcdef12-1234-8567-8123-abcdef123456",
        signed
    )
    .is_err());
    assert!(cryptos.get("missing").is_ok());
}

#[test]
fn crypto_set_cannot_prepare_under_transaction_or_accept_new_owner() {
    let _home = IsolatedAppHome::acquire("crypto-set-current-owner");
    let conn = catalog();
    let cryptos = JobCryptos::prepare(&conn, &["候选".into()], &[]).unwrap();
    conn.execute("INSERT INTO job_items VALUES('new-owner','候选')", [])
        .unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let changes = tx.total_changes();
    assert!(JobCryptos::prepare(&tx, &["候选".into()], &[]).is_err());
    assert_eq!(
        cryptos.get("new-owner").err().unwrap().code,
        ErrorCode::HistoryAccessUnavailable
    );
    assert_eq!(tx.total_changes(), changes);
}
