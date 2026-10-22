//! Production identity contracts. Synthetic sources, never installed histories.
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;
use serde_json::{json, Value};

fn write_transcript(root: &Path, relative: &str, records: &[Value]) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).unwrap();
        bytes.push(b'\n');
    }
    std::fs::write(&path, bytes).unwrap();
    path
}

fn scan_cursor(root: &Path) -> ScanState {
    scan_start(vec![(
        Provider::Cursor,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap()
}

fn publish(candidate: &SourceCandidate) -> (String, String) {
    let (preview_id, details) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    assert!(details.event_count > 0);
    let plan = plan(vec![preview_id], vec![]).unwrap();
    let queued = commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = job_get(&queued.job_id).unwrap();
    assert_eq!(job.state, JobState::Completed, "{job:?}");
    (
        job.items[0].import_id.clone().unwrap(),
        job.items[0].revision_id.clone().unwrap(),
    )
}

fn read_text(import_id: &str, revision: &str) -> String {
    let (events, next, total) =
        events_page(import_id, Some(revision), None, Some(20), None).unwrap();
    assert!(next.is_none());
    assert_eq!(events.len(), total as usize);
    serde_json::to_string(&events).unwrap()
}

fn assert_independent_publications(root: &Path, expected: &[(&str, &str)]) {
    let scan = scan_cursor(root);
    assert_eq!(scan.candidates.len(), expected.len());
    let published: Vec<_> = scan.candidates.iter().map(publish).collect();
    reset_runtime();
    // Each pinned revision must remain readable even before checking whether
    // publication accidentally reused one catalog entry for distinct facts.
    for (candidate, (import_id, revision)) in scan.candidates.iter().zip(&published) {
        let (_, marker) = expected
            .iter()
            .find(|(suffix, _)| candidate.locator.replace('\\', "/").ends_with(suffix))
            .unwrap();
        assert!(read_text(import_id, revision).contains(marker));
    }
    assert_eq!(
        published
            .iter()
            .map(|(import_id, _)| import_id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        expected.len(),
        "Distinct identity facts must not publish revisions into one entry"
    );
    let conn = open_catalog().unwrap();
    assert_eq!(list_entries(&conn).unwrap().len(), expected.len());
    for (candidate, (import_id, _)) in scan.candidates.iter().zip(&published) {
        let entry = get_entry(&conn, import_id).unwrap().unwrap();
        assert_eq!(entry.native_session_id, candidate.native_session_id);
    }
    for candidate in scan_cursor(root).candidates {
        let index = scan
            .candidates
            .iter()
            .position(|prior| prior.locator == candidate.locator)
            .unwrap();
        assert_eq!(
            publish(&candidate),
            published[index],
            "Retry must be stable"
        );
    }
}

#[test]
fn cursor_composer_and_legacy_same_suffix_publish_distinct_histories() {
    let home = IsolatedAppHome::acquire("cursor-key-family-collision");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("state.vscdb");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value BLOB);")
        .unwrap();
    for (key, marker) in [
        ("composerData:same-native", "COMPOSER_FAMILY_MARKER"),
        ("chatData:same-native", "LEGACY_FAMILY_MARKER"),
    ] {
        conn.execute(
            "INSERT INTO ItemTable(key,value) VALUES(?1,?2)",
            rusqlite::params![
                key,
                serde_json::to_vec(&json!({"conversation":[
                    {"id":"same-message", "type":1, "content":marker}
                ]}))
                .unwrap()
            ],
        )
        .unwrap();
    }
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    assert_independent_publications(
        &root,
        &[
            ("#composerData:same-native", "COMPOSER_FAMILY_MARKER"),
            ("#chatData:same-native", "LEGACY_FAMILY_MARKER"),
        ],
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
    reset_runtime();
}

#[test]
fn cursor_unverified_same_named_store_directories_publish_distinct_histories() {
    let home = IsolatedAppHome::acquire("cursor-store-location-collision");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    let mut originals = Vec::new();
    for (project, marker) in [
        ("project A", "STORE_A_MARKER"),
        ("project B", "STORE_B_MARKER"),
    ] {
        let directory = root.join(project).join("same-directory");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("store.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE blobs(id TEXT PRIMARY KEY, data BLOB);")
            .unwrap();
        conn.execute(
            "INSERT INTO blobs(id,data) VALUES(?1,?2)",
            rusqlite::params![
                "root",
                serde_json::to_vec(&json!({"role":"user", "content":marker})).unwrap()
            ],
        )
        .unwrap();
        drop(conn);
        let meta = directory.join("meta.json");
        std::fs::write(&meta, br#"{"rootBlobId":"root"}"#).unwrap();
        for source in [path, meta] {
            originals.push((source.clone(), std::fs::read(source).unwrap()));
        }
    }
    assert_independent_publications(
        &root,
        &[
            ("project A/same-directory/store.db", "STORE_A_MARKER"),
            ("project B/same-directory/store.db", "STORE_B_MARKER"),
        ],
    );
    for (path, before) in originals {
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    reset_runtime();
}

#[test]
fn cursor_transcript_session_and_composer_declarations_keep_their_identity_family() {
    let home = IsolatedAppHome::acquire("cursor-transcript-family-collision");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    let mut originals = Vec::new();
    for (field, marker) in [
        ("sessionId", "SESSION_FAMILY_MARKER"),
        ("composerId", "COMPOSER_DECLARATION_MARKER"),
    ] {
        let mut record = json!({"role":"user", "content":marker});
        record[field] = json!("same-native");
        let path = write_transcript(&root, &format!("{field}.jsonl"), &[record]);
        originals.push((path.clone(), std::fs::read(path).unwrap()));
    }
    assert_independent_publications(
        &root,
        &[
            ("sessionId.jsonl", "SESSION_FAMILY_MARKER"),
            ("composerId.jsonl", "COMPOSER_DECLARATION_MARKER"),
        ],
    );
    for (path, before) in originals {
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    reset_runtime();
}

#[test]
fn cursor_explicit_composer_declaration_and_database_key_keep_one_proven_identity() {
    let home = IsolatedAppHome::acquire("cursor-proven-composer-association");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    let transcript = write_transcript(
        &root,
        "export.jsonl",
        &[json!({"composerId":"proven-native", "role":"user", "content":"PROVEN_TRANSCRIPT"})],
    );
    let database = root.join("state.vscdb");
    let conn = rusqlite::Connection::open(&database).unwrap();
    conn.execute_batch("CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value BLOB);")
        .unwrap();
    conn.execute(
        "INSERT INTO ItemTable(key,value) VALUES(?1,?2)",
        rusqlite::params![
            "composerData:proven-native",
            serde_json::to_vec(&json!({"conversation":[
                {"id":"db-message", "type":1, "content":"PROVEN_DATABASE"}
            ]}))
            .unwrap()
        ],
    )
    .unwrap();
    drop(conn);
    let originals: Vec<_> = [transcript, database]
        .into_iter()
        .map(|path| (path.clone(), std::fs::read(path).unwrap()))
        .collect();
    let scan = scan_cursor(&root);
    assert_eq!(
        scan.candidates.len(),
        2,
        "Grouping UI is not implemented by dropping variants"
    );
    for candidate in &scan.candidates {
        assert_eq!(
            candidate.native_session_id,
            "cursor-identity-v1:composer:proven-native"
        );
    }
    let published: Vec<_> = scan.candidates.iter().map(publish).collect();
    assert_eq!(published[0].0, published[1].0);
    assert_ne!(
        published[0].1, published[1].1,
        "Different bodies remain separate revisions"
    );
    reset_runtime();
    for (candidate, (import_id, revision)) in scan.candidates.iter().zip(&published) {
        let (expected, excluded) = if candidate.locator.ends_with("export.jsonl") {
            ("PROVEN_TRANSCRIPT", "PROVEN_DATABASE")
        } else {
            ("PROVEN_DATABASE", "PROVEN_TRANSCRIPT")
        };
        let text = read_text(import_id, revision);
        assert!(text.contains(expected));
        assert!(
            !text.contains(excluded),
            "Sources must not splice their bodies together"
        );
    }
    for (path, before) in originals {
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    reset_runtime();
}

#[test]
fn cursor_late_identity_family_cannot_replace_the_fixed_discovery_reference() {
    let home = IsolatedAppHome::acquire("cursor-late-family-change");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    let mut records = vec![json!({"sessionId":"same-native", "role":"user", "content":"FIRST"})];
    for index in 0..70 {
        records.push(
            json!({"id":format!("message-{index}"), "role":"assistant", "content":"synthetic"}),
        );
    }
    records.push(json!({"composerId":"same-native", "role":"assistant", "content":"LATE_FACT"}));
    let path = write_transcript(&root, "late.jsonl", &records);
    let before = std::fs::read(&path).unwrap();
    let scan = scan_cursor(&root);
    assert_eq!(scan.candidates.len(), 1);
    assert_eq!(
        scan.candidates[0].native_session_id,
        "cursor-identity-v1:session:same-native"
    );
    let error = preview(
        &scan.candidates[0].candidate_id,
        Some(&scan.candidates[0].source_revision),
    )
    .expect_err("Late proof cannot silently change the selected identity family");
    assert_eq!(error.code, ErrorCode::SourceChanged);
    assert!(list_entries(&open_catalog().unwrap()).unwrap().is_empty());
    assert_eq!(std::fs::read(path).unwrap(), before);
    reset_runtime();
}

#[test]
fn cursor_same_named_files_without_native_id_publish_distinct_histories() {
    let home = IsolatedAppHome::acquire("cursor-identity-collision");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    let a = write_transcript(
        &root,
        "project A/same.jsonl",
        &[json!({"id":"message-only","role":"user","content":"PROJECT_A_MARKER"})],
    );
    let b = write_transcript(
        &root,
        "project B/same.jsonl",
        &[json!({"id":"message-only","role":"user","content":"PROJECT_B_MARKER"})],
    );
    let before = [std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap()];
    let scan = scan_cursor(&root);
    assert_eq!(scan.candidates.len(), 2);
    assert_ne!(
        scan.candidates[0].native_session_id, scan.candidates[1].native_session_id,
        "A message ID or filename stem is not a shared session identity"
    );
    let published: Vec<_> = scan.candidates.iter().map(publish).collect();
    assert_ne!(published[0].0, published[1].0);
    reset_runtime();
    for (candidate, (import_id, revision)) in scan.candidates.iter().zip(&published) {
        let text = read_text(import_id, revision);
        let expected = if candidate.locator.contains("project A") {
            "PROJECT_A_MARKER"
        } else {
            "PROJECT_B_MARKER"
        };
        assert!(text.contains(expected));
        let entry = get_entry(&open_catalog().unwrap(), import_id)
            .unwrap()
            .unwrap();
        assert_eq!(entry.native_session_id, candidate.native_session_id);
    }
    let rescanned = scan_cursor(&root);
    for candidate in &rescanned.candidates {
        let expected = scan
            .candidates
            .iter()
            .position(|c| c.locator == candidate.locator)
            .unwrap();
        assert_eq!(
            publish(candidate).0,
            published[expected].0,
            "Reimport identity must be stable"
        );
    }
    assert_eq!(std::fs::read(a).unwrap(), before[0]);
    assert_eq!(std::fs::read(b).unwrap(), before[1]);
    reset_runtime();
}

#[test]
fn cursor_declared_session_identity_survives_the_production_pipeline() {
    let home = IsolatedAppHome::acquire("cursor-declared-identity");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    write_transcript(
        &root,
        "export-not-the-session-id.jsonl",
        &[
            json!({"sessionId":"native-declared-session","id":"msg-1","role":"user","content":"问"}),
            json!({"sessionId":"native-declared-session","id":"msg-2","role":"assistant","content":"答"}),
        ],
    );
    let scan = scan_cursor(&root);
    assert_eq!(scan.candidates.len(), 1);
    assert_eq!(
        scan.candidates[0].native_session_id,
        "cursor-identity-v1:session:native-declared-session"
    );
    let (import_id, revision) = publish(&scan.candidates[0]);
    reset_runtime();
    let entry = get_entry(&open_catalog().unwrap(), &import_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        entry.native_session_id,
        "cursor-identity-v1:session:native-declared-session"
    );
    assert!(read_text(&import_id, &revision).contains("答"));
    reset_runtime();
}

#[test]
fn cursor_blank_prompt_keeps_filename_title_without_weakening_identity() {
    let home = IsolatedAppHome::acquire("cursor-identity-readable-fallback");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    let mut paths = Vec::new();
    for (relative, declared, marker) in [
        ("project A/空白 🚀.jsonl", None, "FALLBACK_A"),
        ("project B/空白 🚀.jsonl", None, "FALLBACK_B"),
        (
            "declared/空白 🚀.jsonl",
            Some("native-readable-fallback"),
            "DECLARED_MAIN",
        ),
        (
            "subagents/空白 🚀.jsonl",
            Some("native-readable-fallback"),
            "DECLARED_CHILD",
        ),
    ] {
        let path = write_transcript(
            &root,
            relative,
            &[
                json!({"sessionId":declared,"role":"user","content":" \n\t "}),
                json!({"sessionId":declared,"role":"assistant","content":marker}),
            ],
        )
        .canonicalize()
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        paths.push((path, bytes, marker));
    }
    let scan = scan_cursor(&root);
    assert_eq!(scan.candidates.len(), 4);
    let identities: std::collections::HashSet<_> = scan
        .candidates
        .iter()
        .map(|candidate| candidate.native_session_id.as_str())
        .collect();
    assert_eq!(identities.len(), 4);
    let mut published = Vec::new();
    for candidate in &scan.candidates {
        assert_eq!(candidate.title, "空白 🚀");
        let (_, details) =
            preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
        assert_eq!(details.title.as_deref(), Some("空白 🚀"));
        published.push(publish(candidate));
    }
    reset_runtime();
    let conn = open_catalog().unwrap();
    for (candidate, (import_id, revision)) in scan.candidates.iter().zip(&published) {
        let entry = get_entry(&conn, import_id).unwrap().unwrap();
        assert_eq!(entry.title, "空白 🚀");
        assert_eq!(entry.native_session_id, candidate.native_session_id);
        let (_, _, marker) = paths
            .iter()
            .find(|(path, _, _)| path.to_string_lossy() == candidate.locator)
            .unwrap();
        assert!(read_text(import_id, revision).contains(marker));
    }
    assert_eq!(
        published
            .iter()
            .map(|(import_id, _)| import_id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    for (path, bytes, _) in paths {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    reset_runtime();
}

#[test]
fn cursor_conflicting_declared_identity_after_discovery_budget_blocks_preview() {
    let home = IsolatedAppHome::acquire("cursor-late-identity-conflict");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    let mut records = vec![json!({"sessionId":"one","role":"user","content":"first"})];
    for index in 0..70 {
        records.push(json!({"id":format!("msg-{index}"),"role":"assistant","content":"synthetic"}));
    }
    records.push(json!({"sessionId":"two","role":"user","content":"other session"}));
    let path = write_transcript(&root, "mixed.jsonl", &records);
    let before = std::fs::read(&path).unwrap();
    let scan = scan_cursor(&root);
    assert_eq!(scan.candidates.len(), 1, "Listing remains bounded");
    let result = preview(
        &scan.candidates[0].candidate_id,
        Some(&scan.candidates[0].source_revision),
    );
    let error = result.expect_err("Mixed session facts must not publish one guessed identity");
    assert_eq!(error.code, ErrorCode::FormatUnsupported);
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert!(list_entries(&open_catalog().unwrap()).unwrap().is_empty());
    reset_runtime();
}

#[test]
fn cursor_same_declared_id_on_two_installations_stays_separate() {
    let home = IsolatedAppHome::acquire("cursor-two-installations");
    reset_runtime();
    let mut candidates = Vec::new();
    for installation in ["one", "two"] {
        let root = home.path.join(installation);
        write_transcript(
            &root,
            "same.jsonl",
            &[json!({"sessionId":"shared-native-id","role":"user","content":installation})],
        );
        let scan = scan_cursor(&root);
        assert_eq!(scan.candidates.len(), 1);
        candidates.push(scan.candidates[0].clone());
    }
    assert_eq!(
        candidates[0].native_session_id,
        candidates[1].native_session_id
    );
    let published: Vec<_> = candidates.iter().map(publish).collect();
    assert_ne!(published[0].0, published[1].0);
    let conn = open_catalog().unwrap();
    let a = get_entry(&conn, &published[0].0).unwrap().unwrap();
    let b = get_entry(&conn, &published[1].0).unwrap().unwrap();
    assert_ne!(a.installation_id, b.installation_id);
    assert_eq!(
        a.native_session_id,
        "cursor-identity-v1:session:shared-native-id"
    );
    assert_eq!(
        b.native_session_id,
        "cursor-identity-v1:session:shared-native-id"
    );
    reset_runtime();
}

#[test]
fn cursor_subagent_does_not_reuse_its_parents_declared_identity() {
    let home = IsolatedAppHome::acquire("cursor-subagent-identity");
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    let records =
        [json!({"sessionId":"parent-id","id":"same-message-id","role":"user","content":"same"})];
    write_transcript(&root, "main.jsonl", &records);
    write_transcript(&root, "subagents/main.jsonl", &records);
    let scan = scan_cursor(&root);
    assert_eq!(scan.candidates.len(), 2);
    assert_ne!(
        scan.candidates[0].native_session_id,
        scan.candidates[1].native_session_id
    );
    let published: Vec<_> = scan.candidates.iter().map(publish).collect();
    assert_ne!(published[0].0, published[1].0);
    reset_runtime();
}

#[test]
fn cursor_invalid_or_over_budget_declared_identity_is_not_a_filename_fallback() {
    let home = IsolatedAppHome::acquire("cursor-invalid-identity");
    reset_runtime();
    for (index, (value, expected)) in [
        (json!(17), ErrorCode::FormatUnsupported),
        (json!(" "), ErrorCode::FormatUnsupported),
        (json!("identity\nother"), ErrorCode::FormatUnsupported),
        (json!("x".repeat(4097)), ErrorCode::ResourceLimit),
    ]
    .into_iter()
    .enumerate()
    {
        let root = home.path.join(format!("synthetic-{index}"));
        write_transcript(
            &root,
            "named.jsonl",
            &[json!({"sessionId":value,"role":"user","content":"synthetic"})],
        );
        let scan = scan_cursor(&root);
        assert!(scan.complete && scan.candidates.is_empty());
        assert_eq!(scan.issues.len(), 1);
        assert_eq!(scan.issues[0].code, expected.as_str());
    }
    reset_runtime();
}
