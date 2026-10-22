// Inject contention after the real create/discard connection has opened, at
// the first unlocked lookup. This cannot be hidden by open_catalog's timeout.
thread_local! {
    static BEFORE_REQUEST_LOOKUP: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn before_request_lookup() {
    let callback = BEFORE_REQUEST_LOOKUP.with(|hook| hook.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

fn with_lookup_writer<T>(hold_until_result: bool, action: impl FnOnce() -> T) -> T {
    use std::sync::mpsc;
    use std::time::Duration;
    let mut writer = catalog::open_catalog().unwrap();
    let (begin, ready_to_lock) = mpsc::channel();
    let (locked, ready_to_read) = mpsc::channel();
    let (release, finished) = mpsc::channel();
    BEFORE_REQUEST_LOOKUP.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            begin.send(()).unwrap();
            ready_to_read.recv_timeout(Duration::from_secs(5)).unwrap();
        }));
    });
    let result = std::thread::scope(|scope| {
        let held = scope.spawn(move || {
            ready_to_lock.recv_timeout(Duration::from_secs(5)).unwrap();
            let tx = writer
                .transaction_with_behavior(rusqlite::TransactionBehavior::Exclusive)
                .unwrap();
            locked.send(()).unwrap();
            if hold_until_result {
                finished.recv_timeout(Duration::from_secs(5)).unwrap();
            } else {
                std::thread::sleep(Duration::from_millis(150));
            }
            tx.commit().unwrap();
        });
        let result = action();
        if hold_until_result {
            release.send(()).unwrap();
        }
        held.join().unwrap();
        result
    });
    assert!(BEFORE_REQUEST_LOOKUP.with(|hook| hook.borrow_mut().take().is_none()));
    result
}

#[test]
fn unlocked_request_lookup_waits_for_short_catalog_writer_and_preserves_identity() {
    let home = IsolatedAppHome::acquire("continue-unlocked-reader");
    let prepared = draft(&home);
    let sid = with_lookup_writer(false, || create(&prepared, "short-writer")).unwrap();
    let retry = with_lookup_writer(false, || create(&prepared, "short-writer")).unwrap();
    assert_eq!(retry, sid);
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(
            continuation::sidecar_path(&sid).with_file_name("handoff_draft.txt")
        )
        .unwrap(),
        "edited input"
    );
    assert_eq!(
        continuation::read_sidecar(&sid).unwrap().state,
        DispatchState::Prepared
    );
}

#[test]
fn unlocked_request_lookup_timeout_is_retryable_without_claiming_or_publishing() {
    let home = IsolatedAppHome::acquire("continue-unlocked-busy");
    let prepared = draft(&home);
    let failed = with_lookup_writer(true, || create(&prepared, "long-writer")).unwrap_err();
    assert_eq!(failed.code, ErrorCode::SourceBusy);
    assert_eq!(failed.retry, "retry");
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    let conn = catalog::open_catalog().unwrap();
    assert!(continuation::load_ref_by_request(&conn, "long-writer")
        .unwrap()
        .is_none());
    assert!(intents::load(&conn, "long-writer").unwrap().is_none());
    assert!(catalog::load_draft_json(&conn, &prepared.draft_id)
        .unwrap()
        .is_some());
    let sid = create(&prepared, "long-writer").unwrap();
    assert_eq!(create(&prepared, "long-writer").unwrap(), sid);
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
}

#[test]
fn unlocked_discard_lookup_waits_without_resurrecting_an_interrupted_creation() {
    let home = IsolatedAppHome::acquire("continue-discard-reader");
    let prepared = draft(&home);
    FAIL_AT.with(|point| *point.borrow_mut() = Some("files"));
    let interrupted = create(&prepared, "discard-short-writer");
    FAIL_AT.with(|point| *point.borrow_mut() = None);
    assert!(interrupted.is_err());
    with_lookup_writer(false, || {
        engine::continue_discard(&prepared.draft_id, &prepared.draft_hash)
    })
    .unwrap();
    let conn = catalog::open_catalog().unwrap();
    let intent = intents::load(&conn, "discard-short-writer")
        .unwrap()
        .unwrap();
    assert!(intent.cancelled);
    assert!(!intent.published);
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    assert!(!continuation::sidecar_path(&intent.meta.id)
        .with_file_name("handoff_draft.txt")
        .exists());
    assert!(create(&prepared, "discard-short-writer").is_err());
}
