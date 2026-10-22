//! Abrupt process exits at real signed publication boundaries, without polling.
use super::*;
use crate::session_migration::{job_actions as controls, preview_lifecycle_tests};
use rusqlite::Connection;

#[path = "job_action_quit_tests.rs"]
mod actions;

#[path = "import_preparation_quit_tests.rs"]
mod early;

const EXIT_AT_BOUNDARY: i32 = 86;
const POINTS: &[&str] = &[
    "prepared",
    "staged",
    "after_rename",
    "after_receipt_published",
    "after_native_link",
    "linked",
    "before_catalog_commit",
    "after_catalog_commit",
    "before_job_terminal",
    "after_job_terminal",
    "before_cancel_commit",
    "after_cancel_commit",
    "before_retry_commit",
    "after_retry_commit",
    "before_undo_commit",
    "after_undo_commit",
    "after_undo_native",
    "before_undo_cleanup_commit",
    "after_undo_cleanup_commit",
    "scan_started",
    "scan_candidate",
    "scan_finished",
    "preview_before_lease",
    "preview_after_lease",
    "prepare_queued",
    "prepare_claimed",
    "prepare_preview_ready",
    "prepare_item_recorded",
    "prepare_batch_finished",
    "plan_catalog_persisted",
    "plan_finalized",
    "prepare_header_written",
    "prepare_inputs_written",
    "prepare_override_written",
];

fn child(home: &IsolatedAppHome, instruction: serde_json::Value, expected: i32) {
    let qualified = concat!(module_path!(), "::quit_boundary_child");
    let test = qualified.split_once("::").unwrap().1;
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            test,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("GROK_APP_HOME", &home.path)
        .env("GROK_IMPORT_QUIT_FIXTURE", &home.path)
        .env("GROK_IMPORT_QUIT_INSTRUCTION", instruction.to_string())
        .env_remove("GROK_PUBLISH_CRASH_POINT")
        .env_remove("GROK_IMPORT_QUIT_ASYNC_POINT");
    if matches!(
        instruction["work"].as_str(),
        Some("prepare" | "recover_prepare")
    ) {
        command.env(
            "GROK_IMPORT_QUIT_ASYNC_POINT",
            instruction["point"].as_str().unwrap(),
        );
    }
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{instruction}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if expected == 0 {
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed; 0 ignored"));
    }
}

#[test]
#[ignore = "parent supplies an isolated fixture and always invokes this helper explicitly"]
fn quit_boundary_child() {
    let home = std::env::var_os("GROK_IMPORT_QUIT_FIXTURE").expect("synthetic parent only");
    assert_eq!(std::env::var_os("GROK_APP_HOME"), Some(home.clone()));
    assert!(std::path::Path::new(&home).is_dir());
    let instruction: serde_json::Value =
        serde_json::from_str(&std::env::var("GROK_IMPORT_QUIT_INSTRUCTION").unwrap()).unwrap();
    if instruction["work"] == "recover" {
        assert_eq!(
            job_control::run_pending().unwrap(),
            instruction["completed"].as_u64().unwrap() as u32
        );
        jobs::cleanup_unreferenced_preview_dirs().unwrap();
        return;
    }
    let point = POINTS
        .iter()
        .copied()
        .find(|point| Some(*point) == instruction["point"].as_str())
        .expect("known deterministic boundary");
    preview_lifecycle_tests::test_publish_point(point, || {
        // No Rust destructors run: OS close/SQLite recovery must release locks
        // and roll back any open writer, just as after process termination.
        std::process::exit(EXIT_AT_BOUNDARY);
    });
    if early::child_work(&instruction) {
        panic!("boundary {point} was not reached");
    }
    let job = instruction["job"].as_str().unwrap();
    let action = || serde_json::from_value(instruction["action"].clone()).unwrap();
    match instruction["work"].as_str().unwrap() {
        "publish" => {
            job_control::run_pending().unwrap();
        }
        "cancel" => {
            controls::cancel(job, action()).unwrap();
        }
        "retry" => {
            controls::retry(job, action()).unwrap();
        }
        "undo" => {
            controls::undo(job, action()).unwrap();
        }
        _ => panic!("unknown synthetic operation"),
    }
    panic!("boundary {point} was not reached");
}

fn receipts(conn: &Connection, job: &str) -> Vec<(String, String)> {
    conn.prepare("SELECT receipt_id,json FROM receipts WHERE job_id=?1 ORDER BY rowid")
        .unwrap()
        .query_map([job], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn assert_publication(conn: &Connection, job: &jobs::JobRecord) {
    assert_eq!(job.state, jobs::JobState::Completed, "{job:?}");
    assert_eq!(job.items[0].phase, "committed");
    let imported = job.items[0].import_id.as_deref().unwrap();
    let revision = job.items[0].revision_id.as_deref().unwrap();
    jobs::load_published_auth(imported, Some(revision), None).unwrap();
    let counts: (u64, u64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM entries),(SELECT COUNT(*) FROM revisions)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (1, 1));
    let phases = conn
        .prepare(
            "SELECT phase,COUNT(*) FROM receipts WHERE job_id=?1 GROUP BY phase ORDER BY phase",
        )
        .unwrap()
        .query_map([&job.job_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        phases,
        ["committed", "linked", "prepared", "published", "staged"]
            .map(|phase| (phase.to_string(), 1))
    );
    let projects = crate::store::load_projects_snapshot().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(
        Some(&projects[0].id),
        job.items[0].mapping_project_id.as_ref()
    );
}

#[test]
fn signed_publication_quit_at_every_durable_boundary_recovers_once_offline() {
    for point in &POINTS[..10] {
        let home = IsolatedAppHome::acquire("signed-publication-quit");
        let _reset = ResetRuntime;
        let planned = plan(&home, true);
        let preview = jobs::find_preview_dir(&planned.preview_ids[0])
            .unwrap()
            .unwrap();
        let source = home.path.join("source/sessions/rollout.jsonl");
        let original = std::fs::read(&source).unwrap();
        let queued = engine::commit(&planned.plan_id, &planned.plan_hash, "signed-quit").unwrap();
        child(
            &home,
            json!({"work":"publish","point":point,"job":queued.job_id}),
            EXIT_AT_BOUNDARY,
        );
        let conn = catalog::open_catalog().unwrap();
        let before = receipts(&conn, &queued.job_id);
        let current = engine::job_get(&queued.job_id).unwrap();
        if !matches!(
            *point,
            "after_catalog_commit" | "before_job_terminal" | "after_job_terminal"
        ) {
            assert!(catalog::list_entries(&conn).unwrap().is_empty(), "{point}");
            assert_ne!(current.items[0].phase, "committed", "{point}");
        }
        let project_before = crate::store::load_projects_snapshot().unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), original);
        engine::reset_runtime();
        std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
        child(
            &home,
            json!({"work":"recover","completed":u32::from(*point != "after_job_terminal")}),
            0,
        );
        let recovered = engine::job_get(&queued.job_id).unwrap();
        assert_publication(&conn, &recovered);
        let after = receipts(&conn, &queued.job_id);
        assert!(
            before.iter().all(|receipt| after.contains(receipt)),
            "lost receipt at {point}"
        );
        if let Some(project) = project_before.first() {
            assert_eq!(
                Some(&project.id),
                recovered.items[0].mapping_project_id.as_ref()
            );
        }
        assert!(
            !preview.exists(),
            "completed preview lease leaked at {point}"
        );
        assert_eq!(
            engine::commit(&planned.plan_id, &planned.plan_hash, "signed-quit")
                .unwrap()
                .job_id,
            queued.job_id
        );
        child(&home, json!({"work":"recover","completed":0}), 0);
        assert_eq!(receipts(&conn, &queued.job_id), after);
        assert_eq!(
            std::fs::read(home.path.join("offline-source/sessions/rollout.jsonl")).unwrap(),
            original
        );
    }
}
