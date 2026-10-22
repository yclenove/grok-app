//! Force rescan/recovery/delete to overlap an active signed refresh publisher.
use super::*;
use crate::session_migration::preview_lifecycle_tests::test_publish_point;
use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

struct ReleasePublisher(Option<mpsc::Sender<()>>);

impl Drop for ReleasePublisher {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

fn rescan_plan(home: &IsolatedAppHome) -> engine::PlanState {
    let scan = scan::scan_start(vec![(
        Provider::Codex,
        home.path.join("source").to_string_lossy().into(),
    )])
    .unwrap();
    assert_eq!(scan.candidates.len(), 1);
    let candidate = &scan.candidates[0];
    let (preview, _) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    engine::plan(vec![preview], vec![]).unwrap()
}

#[test]
fn delete_rescan_and_startup_recovery_serialize_with_active_publication() {
    for point in ["staged", "after_catalog_commit"] {
        let home = IsolatedAppHome::acquire("publish-delete-rescan-recovery");
        let _reset = ResetRuntime;
        let first = finish(&plan(&home, true), "concurrency-first");
        let import_id = first.items[0].import_id.clone().unwrap();
        let first_revision = first.items[0].revision_id.clone().unwrap();
        let source = home.path.join("source/sessions/rollout.jsonl");
        let mut writer = std::fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .unwrap();
        writeln!(writer, "{}", json!({"type":"event_msg","payload":{"type":"user_message","message":"new external source revision"}})).unwrap();
        drop(writer);
        let original = std::fs::read(&source).unwrap();
        let planned = rescan_plan(&home);
        let queued =
            engine::commit(&planned.plan_id, &planned.plan_hash, "concurrency-refresh").unwrap();
        std::thread::scope(|scope| {
            let (reached_tx, reached_rx) = mpsc::channel();
            let (resume_tx, resume_rx) = mpsc::channel();
            let mut release = ReleasePublisher(Some(resume_tx));
            let publisher = scope.spawn(move || {
                test_publish_point(point, move || {
                    reached_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(30)).unwrap();
                });
                job_control::run_pending()
            });
            reached_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            let (deleting_tx, deleting_rx) = mpsc::channel();
            let deleting_id = import_id.clone();
            let deletion = scope.spawn(move || {
                test_publish_point("before_delete_lock", move || deleting_tx.send(()).unwrap());
                let conn = catalog::open_catalog().unwrap();
                jobs::delete_imported(&conn, &deleting_id)
            });
            deleting_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            let (recovering_tx, recovering_rx) = mpsc::channel();
            let recovery = scope.spawn(move || {
                test_publish_point("before_worker_lock", move || {
                    recovering_tx.send(()).unwrap()
                });
                job_control::run_pending()
            });
            recovering_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            // The first worker owns worker.lock and publish.lock at the hook.
            // A recovery worker waits for that owner; read/scan/GC stay usable.
            let restored = engine::recovery().unwrap();
            assert_eq!(restored.job.unwrap().job_id, queued.job_id);
            assert!(restored.plan.is_none());
            scan::recover_scans_on_startup().unwrap();
            let rescanned = scan::scan_start(vec![(
                Provider::Codex,
                home.path.join("source").to_string_lossy().into(),
            )])
            .unwrap();
            assert_eq!(rescanned.candidates.len(), 1);
            let conn = catalog::open_catalog().unwrap();
            let current = catalog::get_entry(&conn, &import_id).unwrap().unwrap();
            assert!(!current.hidden);
            if point == "staged" {
                assert_eq!(
                    current.current_revision.as_deref(),
                    Some(first_revision.as_str())
                );
            } else {
                assert_ne!(
                    current.current_revision.as_deref(),
                    Some(first_revision.as_str())
                );
            }
            assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
            release.0.take().unwrap().send(()).unwrap();
            assert_eq!(publisher.join().unwrap().unwrap(), 1);
            deletion.join().unwrap().unwrap();
            assert_eq!(recovery.join().unwrap().unwrap(), 0);
            let done = engine::job_get(&queued.job_id).unwrap();
            assert_eq!(done.state, jobs::JobState::Completed, "{point}: {done:?}");
            assert_eq!(done.items[0].phase, "committed");
            let hidden = catalog::get_entry(&conn, &import_id).unwrap().unwrap();
            assert!(hidden.hidden);
            assert_eq!(hidden.current_revision, done.items[0].revision_id);
            assert!(catalog::is_tombstoned(
                &conn,
                &hidden.installation_id,
                &hidden.provider,
                &hidden.native_session_id
            )
            .unwrap());
            let counts: (u64, u64, u64) = conn
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM entries),(SELECT COUNT(*) FROM revisions),
             (SELECT COUNT(*) FROM receipts WHERE phase='committed')",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(counts, (1, 2, 2));
            assert!(catalog::revision_dir(&import_id, &first_revision)
                .unwrap()
                .is_dir());
            let blocked = rescan_plan(&home);
            let blocked = engine::commit(
                &blocked.plan_id,
                &blocked.plan_hash,
                "concurrency-tombstone",
            )
            .unwrap();
            engine::reset_runtime();
            std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
            assert_eq!(job_control::run_pending().unwrap(), 1);
            let blocked = engine::job_get(&blocked.job_id).unwrap();
            assert_eq!(blocked.state, jobs::JobState::CompletedWithIssues);
            assert_eq!(blocked.items[0].phase, "tombstoned");
            assert_eq!(job_control::run_pending().unwrap(), 0);
            assert!(catalog::list_entries(&conn).unwrap().is_empty());
            assert!(
                catalog::get_entry(&conn, &import_id)
                    .unwrap()
                    .unwrap()
                    .hidden
            );
            assert_eq!(
                std::fs::read(home.path.join("offline-source/sessions/rollout.jsonl")).unwrap(),
                original
            );
        });
    }
}
