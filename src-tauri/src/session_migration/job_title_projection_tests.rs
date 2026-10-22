use super::*;
use rusqlite::StatementStatus;

fn item(id: &str, title: Option<&str>) -> JobItem {
    JobItem {
        candidate_id: id.into(),
        title: title.map(str::to_owned),
        phase: "queued".into(),
        import_id: None,
        revision_id: None,
        outcome: None,
        error: None,
        mapping_project_id: None,
    }
}

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE job_items(job_id TEXT NOT NULL, candidate_id TEXT NOT NULL,
         result_json TEXT, PRIMARY KEY(job_id,candidate_id));",
    )
    .unwrap();
    conn
}

fn insert(conn: &Connection, job: &str, candidate: &str, raw: &str) {
    conn.execute(
        "INSERT INTO job_items VALUES(?1,?2,?3)",
        params![job, candidate, raw],
    )
    .unwrap();
}

#[test]
fn complete_or_empty_page_never_reads_durable_inputs() {
    // No durable-input table: a projected page must not need it at all.
    let conn = Connection::open_in_memory().unwrap();
    let mut page = vec![
        item("known", Some("Original title")),
        item("empty", Some("")),
    ];
    hydrate(&conn, "job", &mut page).unwrap();
    assert_eq!(page[0].title.as_deref(), Some("Original title"));
    assert_eq!(page[1].title.as_deref(), Some(""));
    hydrate(&conn, "job", &mut []).unwrap();
}

#[test]
fn mixed_page_reads_only_missing_titles_and_preserves_the_projection() {
    let conn = database();
    // Reading this known row would fail String conversion; it is not needed.
    insert(
        &conn,
        "job",
        "known",
        r#"{"adapter":{"graph":{"title":17}}}"#,
    );
    insert(
        &conn,
        "job",
        "legacy",
        r#"{"adapter":{"graph":{"title":"旧标题"}}}"#,
    );
    let mut page = vec![item("known", Some("Current title")), item("legacy", None)];
    hydrate(&conn, "job", &mut page).unwrap();
    assert_eq!(page[0].title.as_deref(), Some("Current title"));
    assert_eq!(page[1].title.as_deref(), Some("旧标题"));
    assert_eq!(page[1].candidate_id, "legacy");
    assert_eq!(page[1].phase, "queued");
}

#[test]
fn legacy_fallback_remains_job_and_page_scoped() {
    let conn = database();
    insert(
        &conn,
        "job",
        "legacy",
        r#"{"adapter":{"graph":{"nativeSessionId":"native-fallback"}}}"#,
    );
    insert(
        &conn,
        "other",
        "legacy",
        r#"{"adapter":{"graph":{"title":"Wrong owner"}}}"#,
    );
    insert(
        &conn,
        "job",
        "off-page",
        r#"{"adapter":{"graph":{"title":17}}}"#,
    );
    let mut page = vec![item("legacy", None), item("missing", None)];
    hydrate(&conn, "job", &mut page).unwrap();
    assert_eq!(page[0].title.as_deref(), Some("native-fallback"));
    assert_eq!(page[1].title, None);
}

#[test]
fn malformed_null_and_empty_legacy_titles_keep_the_id_fallback() {
    let conn = database();
    insert(&conn, "job", "invalid", "{bad");
    insert(&conn, "job", "null", "null");
    insert(
        &conn,
        "job",
        "empty",
        r#"{"adapter":{"graph":{"title":"","nativeSessionId":"not-selected"}}}"#,
    );
    let mut page = ["invalid", "null", "empty"].map(|id| item(id, None));
    hydrate(&conn, "job", &mut page).unwrap();
    assert!(page.iter().all(|row| row.title.is_none()));
    assert_eq!(
        conn.total_changes(),
        3,
        "hydration must not persist a repair"
    );
}

#[test]
fn missing_input_table_is_still_an_error_for_a_legacy_page() {
    let conn = Connection::open_in_memory().unwrap();
    let error = hydrate(&conn, "job", &mut [item("legacy", None)]).unwrap_err();
    assert_eq!(error.code, ErrorCode::SnapshotUnavailable);
}

#[test]
fn metadata_sql_work_is_bounded_by_requested_page_not_batch_size() {
    let conn = database();
    let raw = serde_json::json!({"adapter":{"graph":{"title":"Title",
        "nativeSessionId":"Native", "irrelevantText":"synthetic ".repeat(400)}}})
    .to_string();
    for index in 0..1200 {
        insert(&conn, "job", &format!("c{index:04}"), &raw);
    }
    let ids =
        serde_json::to_string(&(0..50).map(|i| format!("c{i:04}")).collect::<Vec<_>>()).unwrap();
    let measure = || {
        let mut stmt = conn.prepare(TITLE_SQL).unwrap();
        let rows = stmt
            .query_map(params!["job", ids], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows.len(), 50);
        assert!(rows
            .iter()
            .all(|(_, title)| title.as_deref() == Some("Title")));
        stmt.get_status(StatementStatus::VmStep)
    };
    let small_steps = measure();
    for index in 1200..2400 {
        insert(&conn, "job", &format!("c{index:04}"), &raw);
    }
    let large_steps = measure();
    eprintln!("TITLE_LOOKUP_VM_STEPS page=50 batch1200={small_steps} batch2400={large_steps}");
    assert!(
        small_steps < 6000,
        "metadata lookup scans unrelated job rows: {small_steps}"
    );
    assert_eq!(
        small_steps, large_steps,
        "page work must not grow with the batch"
    );
}
