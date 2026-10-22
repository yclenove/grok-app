use super::*;

struct Fixture {
    path: PathBuf,
    _lock: crate::session_migration::test_home::AppHomeEnvGuard,
}
impl Fixture {
    fn new() -> Self {
        let lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let path = std::env::temp_dir().join(format!("scan-contract-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        std::env::set_var("GROK_APP_HOME", path.join("app"));
        Self { path, _lock: lock }
    }
    fn source(&self, count: usize) -> Vec<ScanSourcePath> {
        let source = self.path.join("source");
        std::fs::create_dir_all(&source).unwrap();
        for i in 0..count {
            std::fs::write(
                source.join(format!("{i:05}.json")),
                r#"[{"role":"user","content":"synthetic"}]"#,
            )
            .unwrap();
        }
        vec![ScanSourcePath {
            provider: "generic-file".into(),
            path: source.to_string_lossy().into_owned(),
            source_id: None,
            location_id: None,
            host: None,
            profile: String::new(),
        }]
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        controls()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|_, c| !c.path.starts_with(&self.path));
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn discovery_slices_are_incremental_and_select_all_is_frozen() {
    let f = Fixture::new();
    let scan = begin(f.source(90)).unwrap();
    assert_eq!(scan.state, "queued");
    assert_eq!(scan.discovered_count, 0);
    run_slice(&scan.scan_id, 70).unwrap();
    let first = state(&scan.scan_id, 1).unwrap();
    assert_eq!(first.state, "paused");
    assert_eq!(first.discovered_count, 70);
    assert_eq!(first.candidates.len(), 50);
    let frozen = select_current(&scan.scan_id, 1).unwrap();
    assert_eq!(frozen.len(), 70);
    run(&scan.scan_id).unwrap();
    let done = state(&scan.scan_id, 1).unwrap();
    assert_eq!(done.state, "completed");
    assert_eq!(done.discovered_count, 90);
    assert_eq!(done.extra_count, 20);
    assert_eq!(done.extra_candidates.len(), 20);
    assert_eq!(frozen, select_current(&scan.scan_id, 1).unwrap());
    let (a, next, _, _) = page(&scan.scan_id, 1, None, Some(50)).unwrap();
    let (b, end, _, _) = page(&scan.scan_id, 1, next.as_deref(), Some(50)).unwrap();
    assert_eq!(a.len() + b.len(), 90);
    assert!(end.is_none());
    assert!(a
        .iter()
        .all(|c| !b.iter().any(|other| other.candidate_id == c.candidate_id)));
    assert_eq!(cancel(&scan.scan_id, 1).unwrap().state, "completed");
}

#[test]
fn host_owned_selection_freezes_off_page_candidates_and_pages_without_full_id_payload() {
    let f = Fixture::new();
    let scan = begin(f.source(90)).unwrap();
    run_slice(&scan.scan_id, 70).unwrap();

    let initial = super::super::scan_selection::get(&scan.scan_id, 1).unwrap();
    assert_eq!(
        initial.mode,
        super::super::scan_selection::CandidateSelectionMode::Explicit
    );
    assert_eq!(initial.selected_count, 0);

    let frozen = super::super::scan_selection::freeze_current(&scan.scan_id, 1).unwrap();
    assert_eq!(
        frozen.mode,
        super::super::scan_selection::CandidateSelectionMode::FrozenScan
    );
    assert_eq!(frozen.frozen_through, Some(70));
    assert_eq!(frozen.selected_count, 70);

    run(&scan.scan_id).unwrap();
    let first =
        super::super::scan_selection::page(&scan.scan_id, 1, &frozen.selection_id, None, Some(50))
            .unwrap();
    assert_eq!(first.candidates.len(), 50);
    assert_eq!(first.selected_count, 70);
    let second = super::super::scan_selection::page(
        &scan.scan_id,
        1,
        &frozen.selection_id,
        first.next_cursor.as_deref(),
        Some(50),
    )
    .unwrap();
    assert_eq!(second.candidates.len(), 20);
    assert!(second.next_cursor.is_none());
    assert_eq!(second.selected_count, 70);

    let excluded = second.candidates[0].candidate_id.clone();
    let changed = super::super::scan_selection::set_many(
        &scan.scan_id,
        1,
        &frozen.selection_id,
        std::slice::from_ref(&excluded),
        false,
    )
    .unwrap();
    assert_eq!(changed.selected_count, 69);
    let stale = super::super::scan_selection::page(
        &scan.scan_id,
        1,
        &frozen.selection_id,
        first.next_cursor.as_deref(),
        Some(50),
    )
    .unwrap_err();
    assert_eq!(stale.code, ErrorCode::PlanStale);

    let extras = page(&scan.scan_id, 1, None, Some(90)).unwrap().0;
    let post_freeze = extras[80].candidate_id.clone();
    let membership = super::super::scan_selection::selected_ids_for_candidates(
        &scan.scan_id,
        1,
        &frozen.selection_id,
        &[excluded, post_freeze],
    )
    .unwrap();
    assert!(membership.selected_ids.is_empty());
    assert_eq!(membership.state_version, changed.state_version);
}

#[test]
fn cancel_releases_traversal_without_starting_more_files() {
    let f = Fixture::new();
    let scan = begin(f.source(100)).unwrap();
    run_slice(&scan.scan_id, 4).unwrap();
    assert_eq!(cancel(&scan.scan_id, 1).unwrap().state, "cancelled");
    assert!(control(&scan.scan_id)
        .unwrap()
        .worker
        .lock()
        .unwrap()
        .is_none());
    run(&scan.scan_id).unwrap();
    assert_eq!(state(&scan.scan_id, 1).unwrap().discovered_count, 4);
}

#[test]
fn nonmatching_files_consume_traversal_budget() {
    let f = Fixture::new();
    let sources = f.source(0);
    for i in 0..100 {
        std::fs::write(
            Path::new(&sources[0].path).join(format!("{i}.log")),
            b"ignored",
        )
        .unwrap();
    }
    let scan = begin(sources).unwrap();
    run_slice(&scan.scan_id, 5).unwrap();
    let paused = state(&scan.scan_id, 1).unwrap();
    assert_eq!(paused.state, "paused");
    assert_eq!(paused.discovered_count, 0);
    cancel(&scan.scan_id, 1).unwrap();
}

#[test]
fn source_identity_survives_rescan_and_explicit_move_but_not_same_profile_name() {
    let f = Fixture::new();
    let a = f.path.join("device-a");
    let b = f.path.join("device-b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let conn = super::super::catalog::open_catalog().unwrap();
    let first =
        super::super::sources::register(&conn, Provider::Codex, &a, None, "default").unwrap();
    let again =
        super::super::sources::register(&conn, Provider::Codex, &a, None, "default").unwrap();
    let other =
        super::super::sources::register(&conn, Provider::Codex, &b, None, "default").unwrap();
    assert_eq!(first.source_id, again.source_id);
    assert_ne!(first.source_id, other.source_id);
    let moved = f.path.join("backup");
    std::fs::create_dir_all(&moved).unwrap();
    let relocated = super::super::sources::register(
        &conn,
        Provider::Codex,
        &moved,
        Some(&first.source_id),
        "default",
    )
    .unwrap();
    assert_eq!(first.source_id, relocated.source_id);
    assert_eq!(
        super::super::sources::register(&conn, Provider::Codex, &a, None, "")
            .unwrap()
            .source_id,
        first.source_id
    );
    assert_eq!(super::super::sources::list(&conn).unwrap().len(), 2);
}

#[test]
fn generic_relative_identity_distinguishes_same_named_files_and_single_file() {
    let f = Fixture::new();
    let a = f.path.join("a");
    let b = f.path.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let content = r#"[{"role":"user","content":"same"}]"#;
    for dir in [&a, &b] {
        std::fs::write(dir.join("chat.json"), content).unwrap();
    }
    let ca = super::super::discovery::file_candidate(
        Provider::GenericFile,
        &a.join("chat.json"),
        &f.path,
    )
    .unwrap();
    let cb = super::super::discovery::file_candidate(
        Provider::GenericFile,
        &b.join("chat.json"),
        &f.path,
    )
    .unwrap();
    assert_ne!(ca.native_session_id, cb.native_session_id);
    assert_eq!(ca.source_revision, cb.source_revision);
    let path = a.join("chat.json");
    assert_eq!(
        super::super::discovery::file_candidate(Provider::GenericFile, &path, &path)
            .unwrap()
            .native_session_id,
        "chat.json"
    );
}

#[test]
fn diagnostics_storage_is_bounded_without_losing_the_total() {
    let f = Fixture::new();
    let scan = begin(f.source(0)).unwrap();
    let c = control(&scan.scan_id).unwrap();
    let conn = open(&c).unwrap();
    for _ in 0..150 {
        issue(
            &conn,
            "codex",
            "synthetic",
            &ImportError::new(ErrorCode::SourceNotFound, "missing"),
        )
        .unwrap();
    }
    let state = state(&scan.scan_id, 1).unwrap();
    assert_eq!(state.issues.len(), 100);
    assert_eq!(state.issue_count, 150);
    let count: usize = conn
        .query_row("SELECT COUNT(*) FROM issues", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 100);
}

#[test]
fn ten_thousand_metadata_scan_returns_before_completion_and_cancels() {
    let f = Fixture::new();
    let sources = f.source(10_000);
    let start = std::time::Instant::now();
    let scan = start_background(sources).unwrap();
    let returned = start.elapsed();
    assert!(!scan.complete);
    assert_eq!(scan.discovered_count, 0);
    let mut first = state(&scan.scan_id, 1).unwrap();
    while first.discovered_count == 0 && start.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(10));
        first = state(&scan.scan_id, 1).unwrap();
    }
    let first_at = start.elapsed();
    assert!(first.discovered_count > 0);
    let stop = std::time::Instant::now();
    cancel(&scan.scan_id, 1).unwrap();
    while control(&scan.scan_id)
        .unwrap()
        .running
        .load(Ordering::Acquire)
        && stop.elapsed() < std::time::Duration::from_secs(5)
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!control(&scan.scan_id)
        .unwrap()
        .running
        .load(Ordering::Acquire));
    assert_eq!(state(&scan.scan_id, 1).unwrap().state, "cancelled");
    println!("10k synthetic local files: start={returned:?}; first-visible={first_at:?}; cancel-release={:?}",stop.elapsed());
}

#[test]
fn continue_paused_finds_the_rest_and_cancel_does_not_resume() {
    let f = Fixture::new();
    let scan = begin(f.source(20)).unwrap();
    run_slice(&scan.scan_id, 5).unwrap();
    let paused = state(&scan.scan_id, 1).unwrap();
    assert_eq!(paused.state, "paused");
    assert_eq!(paused.discovered_count, 5);
    assert!(!paused.complete);
    continue_background(&scan.scan_id, 1).unwrap();
    let start = std::time::Instant::now();
    let mut current = state(&scan.scan_id, 1).unwrap();
    while matches!(current.state.as_str(), "paused" | "queued" | "scanning")
        && start.elapsed() < std::time::Duration::from_secs(10)
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
        current = state(&scan.scan_id, 1).unwrap();
    }
    assert_eq!(current.state, "completed");
    assert_eq!(current.discovered_count, 20);
    assert_eq!(cancel(&scan.scan_id, 1).unwrap().state, "completed");
    let second_dir = f.path.join("second");
    std::fs::create_dir_all(&second_dir).unwrap();
    for i in 0..8 {
        std::fs::write(
            second_dir.join(format!("{i:05}.json")),
            r#"[{"role":"user","content":"synthetic"}]"#,
        )
        .unwrap();
    }
    let second = begin(vec![ScanSourcePath {
        provider: "generic-file".into(),
        path: second_dir.to_string_lossy().into_owned(),
        source_id: None,
        location_id: None,
        host: None,
        profile: String::new(),
    }])
    .unwrap();
    run_slice(&second.scan_id, 3).unwrap();
    assert_eq!(state(&second.scan_id, 1).unwrap().state, "paused");
    assert_eq!(cancel(&second.scan_id, 1).unwrap().state, "cancelled");
    continue_background(&second.scan_id, 1).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let after = state(&second.scan_id, 1).unwrap();
    assert_eq!(after.state, "cancelled");
    assert_eq!(after.discovered_count, 3);
}

#[test]
fn foreign_cursor_is_rejected_and_pages_stay_disjoint() {
    let f = Fixture::new();
    let a = begin(f.source(4)).unwrap();
    run(&a.scan_id).unwrap();
    let b = begin(f.source(4)).unwrap();
    run(&b.scan_id).unwrap();
    let (page_a, cursor, _, _) = page(&a.scan_id, 1, None, Some(2)).unwrap();
    assert_eq!(page_a.len(), 2);
    let err = page(&b.scan_id, 1, cursor.as_deref(), Some(2)).unwrap_err();
    assert_eq!(err.code, ErrorCode::PlanStale);
}

#[test]
fn concurrent_state_and_page_do_not_fail() {
    let f = Fixture::new();
    let scan = begin(f.source(40)).unwrap();
    let id = scan.scan_id.clone();
    std::thread::scope(|scope| {
        scope.spawn(|| run_slice(&id, 40).unwrap());
        scope.spawn(|| {
            for _ in 0..30 {
                let _ = state(&id, 1);
                let _ = page(&id, 1, None, Some(10));
            }
        });
    });
    let done = state(&scan.scan_id, 1).unwrap();
    assert!(matches!(
        done.state.as_str(),
        "paused" | "completed" | "cancelled"
    ));
}

#[test]
fn missing_source_is_an_issue_not_a_stuck_scan() {
    let f = Fixture::new();
    let missing = f.path.join("gone");
    let scan = begin(vec![ScanSourcePath {
        provider: "generic-file".into(),
        path: missing.to_string_lossy().into_owned(),
        source_id: None,
        location_id: None,
        host: None,
        profile: String::new(),
    }])
    .unwrap();
    run(&scan.scan_id).unwrap();
    let done = state(&scan.scan_id, 1).unwrap();
    assert_eq!(done.state, "completed");
    assert_eq!(done.discovered_count, 0);
    assert!(done.issue_count >= 1);
    assert_eq!(done.issues[0].code, "SOURCE_NOT_FOUND");
}

#[test]
fn symlink_or_junction_outside_root_is_not_followed() {
    let f = Fixture::new();
    let sources = f.source(1);
    let root = PathBuf::from(&sources[0].path);
    let outside = f.path.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        outside.join("secret.json"),
        r#"[{"role":"user","content":"leaked"}]"#,
    )
    .unwrap();
    let link = root.join("escape");
    let linked = {
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args([
                    "/C",
                    "mklink",
                    "/J",
                    &link.to_string_lossy(),
                    &outside.to_string_lossy(),
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            std::os::unix::fs::symlink(&outside, &link).is_ok()
        }
    };
    if !linked {
        return;
    }
    let scan = begin(sources).unwrap();
    run(&scan.scan_id).unwrap();
    let (all, _, count, _) = page(&scan.scan_id, 1, None, Some(50)).unwrap();
    assert_eq!(count, 1);
    assert!(all.iter().all(|c| !c.locator.contains("secret.json")));
}

#[test]
fn restart_marks_orphaned_scanning_expired_and_cleanup_skips_live_and_foreign_files() {
    let f = Fixture::new();
    let scan = begin(f.source(6)).unwrap();
    run_slice(&scan.scan_id, 3).unwrap();
    assert_eq!(state(&scan.scan_id, 1).unwrap().state, "paused");
    let live = begin(f.source(2)).unwrap();
    let db = control(&scan.scan_id).unwrap().path.clone();
    assert!(db.exists());
    let decoy_dir = super::super::catalog::imports_dir();
    let decoy = decoy_dir.join("do-not-delete.sqlite");
    std::fs::write(&decoy, b"keep").unwrap();
    let foreign = f.path.join("other-root.sqlite");
    std::fs::write(&foreign, b"foreign").unwrap();
    forget_for_restart(&scan.scan_id);
    assert!(adopt_orphaned_scans().unwrap() >= 1);
    let orphan = state(&scan.scan_id, 1).unwrap();
    assert_eq!(orphan.state, "expired");
    assert_eq!(orphan.discovered_count, 3);
    assert_ne!(orphan.state, "scanning");
    assert_ne!(orphan.state, "paused");
    let removed = cleanup_expired_scan_files().unwrap();
    assert!(removed >= 1);
    assert!(!db.exists());
    assert!(!PathBuf::from(format!("{}-wal", db.display())).exists());
    assert!(!PathBuf::from(format!("{}-shm", db.display())).exists());
    assert_eq!(
        state(&scan.scan_id, 1).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert!(decoy.exists());
    assert!(foreign.exists());
    assert_eq!(state(&live.scan_id, 1).unwrap().scan_id, live.scan_id);
    assert!(control(&live.scan_id).unwrap().path.exists());
}

#[test]
fn paged_scan_does_not_pin_native_send_connect_lock() {
    let f = Fixture::new();
    let mgr = crate::session_manager::SessionManager::native();
    let sources = f.source(800);
    tauri::async_runtime::block_on(async move {
        drop(
            mgr.acquire_send_connect_lock()
                .await
                .expect("native send lock must start free"),
        );
        let scan = start_background(sources).unwrap();
        assert!(
            scan.candidates.len() <= super::super::budget::PAGE_SIZE,
            "listing must be paged, not a full catalog dump"
        );
        let start = std::time::Instant::now();
        let mut saw_page = false;
        let mut lock_ok = 0u32;
        loop {
            let scan_id = scan.scan_id.clone();
            let listing =
                tauri::async_runtime::spawn_blocking(move || page(&scan_id, 1, None, Some(50)));
            let send = mgr.acquire_send_connect_lock();
            let (lock_res, page_res) = tokio::join!(send, listing);
            drop(
                lock_res
                    .expect("send_message connect lock must succeed during start_background/page"),
            );
            lock_ok += 1;
            let (page, _, total, complete) = page_res.expect("page join").unwrap();
            assert!(page.len() <= 50);
            if total > 0 {
                saw_page = true;
            }
            if complete || start.elapsed() > std::time::Duration::from_secs(20) {
                break;
            }
        }
        assert!(saw_page, "scan must surface at least one paged listing");
        assert!(lock_ok >= 1);
        let done = state(&scan.scan_id, 1).unwrap();
        assert!(done.candidates.len() <= super::super::budget::PAGE_SIZE);
        assert!(
            done.discovered_count as usize > done.candidates.len()
                || done.discovered_count <= super::super::budget::PAGE_SIZE as u32
        );
        let _guard = mgr
            .acquire_send_connect_lock()
            .await
            .expect("native send lock still acquirable");
        let scan_id = scan.scan_id.clone();
        let (page, _, _, _) =
            tauri::async_runtime::spawn_blocking(move || page(&scan_id, 1, None, Some(50)))
                .await
                .expect("page join")
                .unwrap();
        assert!(page.len() <= 50);
        assert!(
            mgr.connect_lock_busy(),
            "scan listing must not steal the held send lock"
        );
        drop(_guard);
        drop(
            mgr.acquire_send_connect_lock()
                .await
                .expect("native send lock must be free after scan listing"),
        );
    });
}

fn home_contract_fixture() {
    let _fixture = Fixture::new();
}

crate::session_migration::test_home::fixture_home_contract_tests!(home_contract_fixture);
