//! Real scan/preview/prepare workers and the two-database plan handoff.
use super::*;
use crate::session_migration::{prepare, scan_selection};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn source_scan() -> scan::ScanState {
    let home = PathBuf::from(std::env::var_os("GROK_APP_HOME").unwrap());
    scan::scan_start(vec![(
        Provider::Codex,
        home.join("source").to_string_lossy().into(),
    )])
    .unwrap()
}

pub(super) fn child_work(instruction: &serde_json::Value) -> bool {
    match instruction["work"].as_str().unwrap() {
        "scan" => {
            let home = PathBuf::from(std::env::var_os("GROK_APP_HOME").unwrap());
            let scan = scan::begin(vec![scan::ScanSourcePath {
                provider: "codex".into(),
                path: home.join("source").to_string_lossy().into(),
                source_id: None,
                location_id: None,
                host: None,
                profile: String::new(),
            }])
            .unwrap();
            std::fs::write(home.join("quit-scan-id"), &scan.scan_id).unwrap();
            scan::run(&scan.scan_id).unwrap();
        }
        "preview" => {
            let scan = source_scan();
            let candidate = &scan.candidates[0];
            engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
        }
        "prepare" | "recover_prepare" => {
            if instruction["work"] == "prepare" {
                let scan = source_scan();
                let selection = scan_selection::freeze_current(&scan.scan_id, scan.epoch).unwrap();
                let overrides = if instruction["override"] == true {
                    let candidate = &scan.candidates[0];
                    let (preview_id, details) =
                        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision))
                            .unwrap();
                    vec![prepare::PrepareOverride {
                        candidate_id: candidate.candidate_id.clone(),
                        preview_id,
                        leaf_id: details.active_leaf.unwrap(),
                        selection: details.default_selection,
                    }]
                } else {
                    vec![]
                };
                prepare::start(
                    &scan.scan_id,
                    scan.epoch,
                    &selection.selection_id,
                    overrides,
                )
                .unwrap();
            } else {
                let recovery = prepare::latest_recoverable().unwrap();
                assert!(recovery.failed.is_none());
                assert!(recovery.status.is_some());
            }
            // The real asynchronous worker exits the process at its hook.
            // Keep the test thread alive without polling or sleeping.
            let (_keepalive, receiver) = std::sync::mpsc::channel::<()>();
            receiver
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("prepare worker did not reach quit boundary");
        }
        "plan" => {
            let batch = instruction["batch"].as_str().unwrap();
            let inputs = prepare::validated_plan_inputs(batch).unwrap();
            let mappings = prepare::mappings(batch, None).unwrap();
            engine::plan_from_validated_prepare(inputs, mappings).unwrap();
        }
        _ => return false,
    }
    true
}

fn preview_paths() -> BTreeSet<PathBuf> {
    std::fs::read_dir(catalog::staging_dir().join("previews"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect()
}

fn batch_id() -> String {
    let paths = std::fs::read_dir(prepare::batch_root().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "sqlite")
        })
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 1);
    paths[0].file_stem().unwrap().to_str().unwrap().into()
}

#[test]
fn scan_quit_during_discovery_expires_the_orphan_and_allows_fresh_rescan() {
    for point in ["scan_started", "scan_candidate", "scan_finished"] {
        let home = IsolatedAppHome::acquire("scan-quit-boundary");
        let _reset = ResetRuntime;
        plan(&home, false);
        let source = home.path.join("source/sessions/rollout.jsonl");
        let original = std::fs::read(&source).unwrap();
        child(
            &home,
            json!({"work":"scan","point":point}),
            EXIT_AT_BOUNDARY,
        );
        let id = std::fs::read_to_string(home.path.join("quit-scan-id")).unwrap();
        assert!(scan::adopt_orphaned_scans().unwrap() >= 1);
        let restored = scan::state(&id, 1).unwrap();
        assert_eq!(
            restored.state,
            if point == "scan_finished" {
                "completed"
            } else {
                "expired"
            }
        );
        assert_eq!(
            restored.discovered_count,
            u32::from(point != "scan_started")
        );
        scan::cleanup_expired_scan_files().unwrap();
        if point != "scan_finished" {
            assert_eq!(scan::state(&id, 1).unwrap_err().code, ErrorCode::PlanStale);
        }
        assert_eq!(source_scan().candidates.len(), 1);
        assert!(catalog::list_entries(&catalog::open_catalog().unwrap())
            .unwrap()
            .is_empty());
        assert_eq!(std::fs::read(source).unwrap(), original);
    }
}

#[test]
fn preview_quit_around_lease_publication_reclaims_only_incomplete_snapshot() {
    for point in ["preview_before_lease", "preview_after_lease"] {
        let home = IsolatedAppHome::acquire("preview-quit-boundary");
        let _reset = ResetRuntime;
        plan(&home, false);
        let before = preview_paths();
        child(
            &home,
            json!({"work":"preview","point":point}),
            EXIT_AT_BOUNDARY,
        );
        let added = preview_paths()
            .difference(&before)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(added.len(), 1);
        let path = &added[0];
        let id = path.file_name().unwrap().to_str().unwrap();
        engine::reset_runtime();
        if point == "preview_before_lease" {
            assert!(jobs::read_preview_lease(path).unwrap().is_none());
            jobs::cleanup_unreferenced_preview_dirs().unwrap();
            assert!(!path.exists());
        } else {
            assert!(jobs::read_preview_lease(path).unwrap().is_some());
            assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
            let (events, _) = engine::preview_events_page(id, None, Some(50), None).unwrap();
            assert_eq!(events.len(), 2);
            let planned = engine::plan(vec![id.to_string()], vec![]).unwrap();
            let job = engine::commit(&planned.plan_id, &planned.plan_hash, "preview-quit").unwrap();
            std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
            child(&home, json!({"work":"recover","completed":1}), 0);
            assert_eq!(
                engine::job_get(&job.job_id).unwrap().items[0].phase,
                "committed"
            );
            assert!(!path.exists());
        }
    }
}

#[test]
fn prepare_quit_recovers_claims_and_preserves_already_recorded_previews() {
    for point in [
        "prepare_queued",
        "prepare_claimed",
        "prepare_preview_ready",
        "prepare_item_recorded",
        "prepare_batch_finished",
    ] {
        let home = IsolatedAppHome::acquire("prepare-quit-boundary");
        let _reset = ResetRuntime;
        plan(&home, false);
        let source = home.path.join("source/sessions/rollout.jsonl");
        let original = std::fs::read(&source).unwrap();
        child(
            &home,
            json!({"work":"prepare","point":point}),
            EXIT_AT_BOUNDARY,
        );
        let batch = batch_id();
        let before = prepare::items_page(&batch, None, Some(50)).unwrap().items;
        let at_quit = preview_paths();
        if point != "prepare_batch_finished" {
            child(
                &home,
                json!({"work":"recover_prepare","point":"prepare_batch_finished"}),
                EXIT_AT_BOUNDARY,
            );
        }
        let status = prepare::status_raw(&batch).unwrap();
        assert_eq!(
            status.state,
            prepare::PrepareBatchState::Completed,
            "{point}: {status:?}"
        );
        assert_eq!((status.prepared, status.failed, status.active), (1, 0, 0));
        let after = prepare::items_page(&batch, None, Some(50)).unwrap().items;
        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0].attempt,
            if matches!(point, "prepare_claimed" | "prepare_preview_ready") {
                2
            } else {
                1
            }
        );
        if before[0].state == prepare::PrepareItemState::Prepared {
            assert_eq!(after[0].preview_id, before[0].preview_id);
        }
        let planned = engine::plan_from_validated_prepare(
            prepare::validated_plan_inputs(&batch).unwrap(),
            prepare::mappings(&batch, None).unwrap(),
        )
        .unwrap();
        let queued = engine::commit(&planned.plan_id, &planned.plan_hash, "prepare-quit").unwrap();
        std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
        child(&home, json!({"work":"recover","completed":1}), 0);
        assert_eq!(
            engine::job_get(&queued.job_id).unwrap().items[0].phase,
            "committed"
        );
        assert_eq!(job_control::run_pending().unwrap(), 0);
        // A preview built before its item write is an unaccepted orphan, not a
        // durable batch member. Expiry must make it reclaimable after recovery.
        for path in at_quit.iter().filter(|path| path.exists()) {
            if jobs::read_preview_lease(path).unwrap().is_some() {
                jobs::expire_preview_lease_file(path).unwrap();
            }
        }
        jobs::cleanup_unreferenced_preview_dirs().unwrap();
        assert!(at_quit.iter().all(|path| !path.exists()));
        assert_eq!(
            std::fs::read(home.path.join("offline-source/sessions/rollout.jsonl")).unwrap(),
            original
        );
    }
}

#[test]
fn prepare_initialization_quit_never_recovers_empty_or_partially_overridden_work() {
    for point in [
        "prepare_header_written",
        "prepare_inputs_written",
        "prepare_override_written",
    ] {
        let home = IsolatedAppHome::acquire("prepare-initialization-quit");
        let _reset = ResetRuntime;
        plan(&home, false);
        child(
            &home,
            json!({"work":"prepare","point":point,"override":true}),
            EXIT_AT_BOUNDARY,
        );
        let incomplete = batch_id();
        let conn = prepare::open_existing(&incomplete).unwrap();
        let counts: (u64, u64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM meta),(SELECT COUNT(*) FROM items)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            counts,
            (0, 0),
            "{point}: partial initialization became durable"
        );
        let recovery = prepare::latest_recoverable().unwrap();
        assert!(
            recovery.status.is_none(),
            "{point}: must not report empty completion"
        );
        assert!(
            recovery.failed.is_some(),
            "{point}: interrupted creation must be explained"
        );
        assert!(prepare::status_raw(&incomplete).is_err());
        assert!(catalog::list_entries(&catalog::open_catalog().unwrap())
            .unwrap()
            .is_empty());
        let guard = crate::session_migration::preview_lifecycle::operation().unwrap();
        drop(guard);
        // A separate newly requested batch still uses the real worker. Keep the
        // incomplete DB as evidence; recovery must not rewrite it into success.
        child(
            &home,
            json!({"work":"prepare","point":"prepare_batch_finished"}),
            EXIT_AT_BOUNDARY,
        );
        let recovery = prepare::latest_recoverable().unwrap();
        assert!(recovery.failed.is_some());
        assert_eq!(
            recovery.status.unwrap().state,
            prepare::PrepareBatchState::Completed
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM meta", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn plan_quit_between_catalog_and_batch_receipt_never_publishes_an_inert_plan() {
    for point in ["plan_catalog_persisted", "plan_finalized"] {
        let home = IsolatedAppHome::acquire("plan-quit-boundary");
        let _reset = ResetRuntime;
        plan(&home, false);
        child(
            &home,
            json!({"work":"prepare","point":"prepare_batch_finished"}),
            EXIT_AT_BOUNDARY,
        );
        let batch = batch_id();
        child(
            &home,
            json!({"work":"plan","point":point,"batch":batch}),
            EXIT_AT_BOUNDARY,
        );
        engine::reset_runtime();
        let restored = engine::recovery().unwrap();
        let (id, hash) = if point == "plan_catalog_persisted" {
            assert!(restored.plan.is_none());
            assert_eq!(restored.prepare.unwrap().batch_id, batch);
            let conn = catalog::open_catalog().unwrap();
            let (id, hash, _, _) = catalog::latest_plan_row(&conn).unwrap().unwrap();
            assert!(engine::commit(&id, &hash, "inert-plan-quit").is_err());
            let planned = engine::plan_from_validated_prepare(
                prepare::validated_plan_inputs(&batch).unwrap(),
                prepare::mappings(&batch, None).unwrap(),
            )
            .unwrap();
            (planned.plan_id, planned.plan_hash)
        } else {
            let planned = restored.plan.unwrap();
            assert!(restored.prepare.is_none());
            (planned.plan_id, planned.plan_hash)
        };
        let queued = engine::commit(&id, &hash, "plan-quit").unwrap();
        std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
        child(&home, json!({"work":"recover","completed":1}), 0);
        assert_eq!(
            engine::job_get(&queued.job_id).unwrap().items[0].phase,
            "committed"
        );
        assert_eq!(job_control::run_pending().unwrap(), 0);
    }
}
