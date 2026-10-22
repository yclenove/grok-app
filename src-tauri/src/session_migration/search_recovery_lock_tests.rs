use super::*;

fn assert_locked_artifact_is_retryable(slot: usize, future: bool) {
    let mut layout = [Artifact::Current; 3];
    if future {
        layout[slot] = Artifact::Future;
    }
    let fixture = fixture(layout);
    let live = index_path(&fixture.0);
    let paths = [
        live.clone(),
        index_backup_path(&live),
        fixture.0.join("event-index.sqlite.tmp"),
    ];
    let blocker = Connection::open(&paths[slot]).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let before = snapshot(&fixture.0);
    for rebuild in [false, true] {
        let result = if rebuild {
            rebuild_index(&fixture.0, &|| false)
        } else {
            ensure_index(&fixture.0)
        };
        assert!(
            before == snapshot(&fixture.0),
            "unreadable artifact must not trigger any recovery mutation: slot={slot}, future={future}, rebuild={rebuild}"
        );
        let error = result.expect_err("exclusive lock must not become success");
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert_eq!(error.retry, "retry");
    }
    blocker.execute_batch("ROLLBACK;").unwrap();
    drop(blocker);
    if future {
        assert_eq!(
            ensure_index(&fixture.0).unwrap_err().code,
            ErrorCode::ImportSchemaTooNew
        );
        assert!(before == snapshot(&fixture.0));
    } else {
        ensure_index(&fixture.0).unwrap();
        assert_eq!(
            index_user_version(&live).unwrap(),
            Some(INDEX_SCHEMA_VERSION)
        );
    }
}

#[test]
fn locked_current_live_is_retryable_without_mutation() {
    assert_locked_artifact_is_retryable(0, false);
}

#[test]
fn locked_current_backup_is_retryable_without_mutation() {
    assert_locked_artifact_is_retryable(1, false);
}

#[test]
fn locked_current_tmp_prevents_backup_cleanup() {
    assert_locked_artifact_is_retryable(2, false);
}

#[test]
fn locked_future_live_is_not_misclassified_as_corrupt() {
    assert_locked_artifact_is_retryable(0, true);
}

#[test]
fn locked_future_backup_is_not_deleted_or_promoted() {
    assert_locked_artifact_is_retryable(1, true);
}

#[test]
fn locked_future_tmp_prevents_backup_cleanup_and_rebuild() {
    assert_locked_artifact_is_retryable(2, true);
}
