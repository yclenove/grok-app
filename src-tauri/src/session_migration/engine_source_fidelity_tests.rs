//! Source times and roles must survive the actual import pipeline, not just adapters.
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;
use crate::session_migration::types::ImportedEvent;
use serde_json::{json, Value};

const MATRIX: &str = include_str!("../../tests/fixtures/session-migration/temporal-matrix.json");

fn input_case(name: &str) -> Value {
    let matrix: Value = serde_json::from_str(MATRIX).unwrap();
    matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap()
        .clone()
}

fn write_source(home: &IsolatedAppHome, case: &Value) -> (PathBuf, PathBuf) {
    let root = home.path.join("synthetic source #时间");
    std::fs::create_dir_all(&root).unwrap();
    if let Some(database) = case.get("database") {
        if let Some(blobs) = database["blobs"].as_array() {
            let directory = root.join("synthetic-v247");
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join("store.db");
            let conn = rusqlite::Connection::open(&path).unwrap();
            for statement in database["tables"].as_array().unwrap() {
                conn.execute_batch(statement.as_str().unwrap()).unwrap();
            }
            conn.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2)",
                rusqlite::params![
                    database["metaRow"]["key"].as_str().unwrap(),
                    database["metaRow"]["valueHex"].as_str().unwrap()
                ],
            )
            .unwrap();
            for blob in blobs {
                conn.execute(
                    "INSERT INTO blobs(id,data) VALUES(?1,?2)",
                    rusqlite::params![
                        blob["id"].as_str().unwrap(),
                        hex::decode(blob["dataHex"].as_str().unwrap()).unwrap()
                    ],
                )
                .unwrap();
            }
            drop(conn);
            return (root, path);
        }
        let path = root.join("state.vscdb");
        let conn = rusqlite::Connection::open(&path).unwrap();
        let table = database["table"].as_str().unwrap();
        assert!(matches!(table, "ItemTable" | "cursorDiskKV"));
        conn.execute_batch(&format!(
            "CREATE TABLE {table} (key TEXT PRIMARY KEY, value BLOB);"
        ))
        .unwrap();
        for row in database["keyValues"].as_array().unwrap() {
            conn.execute(
                &format!("INSERT INTO {table}(key,value) VALUES(?1,?2)"),
                rusqlite::params![
                    row["key"].as_str().unwrap(),
                    serde_json::to_vec(&row["value"]).unwrap()
                ],
            )
            .unwrap();
        }
        drop(conn);
        (root, path)
    } else {
        let path = root.join("synthetic-time.jsonl");
        let mut bytes = Vec::new();
        for record in case["records"].as_array().unwrap() {
            serde_json::to_writer(&mut bytes, record).unwrap();
            bytes.push(b'\n');
        }
        std::fs::write(&path, bytes).unwrap();
        (root, path)
    }
}

fn projection(events: Vec<Value>) -> Vec<Value> {
    events.into_iter().map(|raw| {
        let event: ImportedEvent = serde_json::from_value(raw).unwrap();
        match event {
            ImportedEvent::Message { base, role, blocks, .. } => json!({
                "role": role,
                "text": blocks.iter().filter_map(|block| block.plain_text()).collect::<Vec<_>>().join(""),
                "ordinal": base.ordinal,
                "timestamp": base.timestamp,
                "timeOrigin": base.time_origin
            }),
            other => panic!("unexpected source event: {other:?}"),
        }
    }).collect()
}

fn assert_pipeline(name: &str) {
    let case = input_case(name);
    assert_case(case);
}

fn assert_case(case: Value) {
    let name = case["name"].as_str().unwrap();
    let home = IsolatedAppHome::acquire("source-fidelity");
    reset_runtime();
    let before = chrono::Utc::now();
    let (root, source) = write_source(&home, &case);
    let source_bytes = std::fs::read(&source).unwrap();
    let provider = Provider::parse(case["provider"].as_str().unwrap()).unwrap();
    let candidates = scan_start(vec![(provider, root.to_string_lossy().into_owned())])
        .unwrap()
        .candidates;
    assert_eq!(
        candidates.len(),
        case["expectedCandidates"].as_u64().unwrap_or(1) as usize,
        "{name}: production scan"
    );
    let candidate = if let Some(suffix) = case["selectedSuffix"].as_str() {
        candidates
            .iter()
            .find(|candidate| candidate.locator.ends_with(suffix))
            .unwrap()
    } else {
        &candidates[0]
    };
    let (preview_id, details) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let expected = case["expected"].as_array().unwrap();
    assert_eq!(details.events.len(), 4, "{name}: preview event count");
    for (event, wanted) in details.events.iter().zip(expected) {
        assert_eq!(json!(event.role), wanted["role"], "{name}: preview role");
        assert_eq!(
            json!(event.timestamp),
            wanted["timestamp"],
            "{name}: preview time"
        );
        assert_eq!(
            event.text,
            wanted["text"].as_str().unwrap(),
            "{name}: preview body"
        );
    }
    let plan = plan(vec![preview_id], vec![]).unwrap();
    let queued = commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = job_get(&queued.job_id).unwrap();
    assert_eq!(job.state, JobState::Completed, "{name}: {job:?}");
    let import_id = job.items[0].import_id.as_deref().unwrap();
    let revision = job.items[0].revision_id.as_deref().unwrap();
    reset_runtime();
    let mut cursor = None;
    let mut actual = Vec::new();
    for _ in 0..3 {
        let (events, next, total) =
            events_page(import_id, Some(revision), cursor.as_deref(), Some(2), None).unwrap();
        assert_eq!(total, 4, "{name}: paged total");
        assert!(events.len() <= 2);
        actual.extend(projection(events));
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none(), "{name}: bounded reader exhausted");
    let expected_projection: Vec<_> = expected
        .iter()
        .map(|event| {
            json!({
                "role": event["role"], "text": event["text"], "ordinal": event["ordinal"],
                "timestamp": event["timestamp"], "timeOrigin": event["timeOrigin"]
            })
        })
        .collect();
    assert_eq!(
        actual, expected_projection,
        "{name}: immutable reader projection"
    );
    if let Some(marker) = case["excludedMarker"].as_str() {
        let (hits, next) =
            search_imported_with_leaf(import_id, Some(revision), None, marker, None, Some(20))
                .unwrap();
        assert!(
            hits.is_empty() && next.is_none(),
            "{name}: abandoned search marker"
        );
        let exported = export_markdown(import_id, Some(revision), None).unwrap();
        let draft = continue_prepare_with_selection(
            import_id,
            Some(revision),
            None,
            None,
            Some("fragment-only".into()),
            None,
        )
        .unwrap();
        assert!(!exported.contains(marker));
        assert!(!serde_json::to_string(&draft).unwrap().contains(marker));
        let archive = crate::session_migration::catalog::revision_dir(import_id, revision)
            .unwrap()
            .join("events.jsonl");
        let bytes = std::fs::read_to_string(archive).unwrap();
        assert!(!bytes.contains(marker), "{name}: abandoned archive marker");
    }
    let conn = open_catalog().unwrap();
    let entry = get_entry(&conn, import_id).unwrap().unwrap();
    let imported = chrono::DateTime::parse_from_rfc3339(&entry.created_at).unwrap();
    let revision_created: String = conn
        .query_row(
            "SELECT created_at FROM revisions WHERE revision_id=?1",
            [revision],
            |row| row.get(0),
        )
        .unwrap();
    let published = chrono::DateTime::parse_from_rfc3339(&revision_created).unwrap();
    let after = chrono::Utc::now();
    assert!(
        imported >= before && imported <= after,
        "{name}: separate entry import time"
    );
    assert!(
        published >= before && published <= after,
        "{name}: separate revision import time"
    );
    for event in expected {
        if let Some(time) = event["timestamp"].as_str() {
            assert_ne!(
                time, entry.created_at,
                "{name}: no fabricated event import time"
            );
            assert_ne!(time, revision_created);
        }
    }
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(
            crate::session_migration::catalog::revision_dir(import_id, revision)
                .unwrap()
                .join("manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["evidence"]["schemaFingerprint"], case["shape"]);
    assert_eq!(
        std::fs::read(source).unwrap(),
        source_bytes,
        "{name}: source unchanged"
    );
    reset_runtime();
}

#[test]
fn codex_four_source_times_survive_publication() {
    assert_pipeline("codex-four-source-times");
}
#[test]
fn codex_missing_backwards_offsets_survive_publication() {
    assert_pipeline("codex-missing-backwards-offsets");
}
#[test]
fn claude_four_source_times_survive_publication() {
    assert_pipeline("claude-four-source-times");
}
#[test]
fn claude_missing_backwards_offsets_survive_publication() {
    assert_pipeline("claude-missing-backwards-offsets");
}
#[test]
fn cursor_transcript_four_source_times_survive_publication() {
    assert_pipeline("cursor-transcript-four-source-times");
}
#[test]
fn cursor_transcript_missing_backwards_offsets_survive_publication() {
    assert_pipeline("cursor-transcript-missing-backwards-offsets");
}
#[test]
fn cursor_numeric_composer_four_source_times_survive_publication() {
    assert_pipeline("cursor-composer-four-source-times");
}
#[test]
fn cursor_numeric_composer_missing_backwards_offsets_survive_publication() {
    assert_pipeline("cursor-composer-missing-backwards-offsets");
}
#[test]
fn cursor_legacy_four_source_times_survive_publication() {
    assert_pipeline("cursor-legacy-four-source-times");
}
#[test]
fn cursor_legacy_missing_backwards_offsets_survive_publication() {
    assert_pipeline("cursor-legacy-missing-backwards-offsets");
}

#[test]
fn cursor_inline_legacy_ids_and_content_survive_publication() {
    for case_name in [
        "cursor-legacy-four-source-times",
        "cursor-legacy-missing-backwards-offsets",
    ] {
        let mut case = input_case(case_name);
        let data = &mut case["database"]["keyValues"][1]["value"];
        let mut rows = data.as_object_mut().unwrap().remove("messages").unwrap();
        for (index, row) in rows.as_array_mut().unwrap().iter_mut().enumerate() {
            let message = row.as_object_mut().unwrap();
            message.insert("id".into(), json!(format!("inline-{index}")));
            let text = message.remove("text").unwrap();
            message.insert("content".into(), text);
        }
        data["conversation"] = rows;
        assert_case(case);
    }
}

#[test]
fn cursor_legacy_locator_keeps_key_family_with_same_composer_id() {
    let mut case = input_case("cursor-legacy-four-source-times");
    case["expectedCandidates"] = json!(2);
    case["selectedSuffix"] = json!("#chatData:synthetic-legacy-time");
    case["database"]["keyValues"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "key":"composerData:synthetic-legacy-time",
            "value":{"messages":[{"type":2,"text":"WRONG_COMPOSER_MARKER"}]}
        }));
    assert_case(case);
}

#[test]
fn cursor_direct_time_units_and_invalid_values_are_explicit() {
    let mut case = input_case("cursor-legacy-four-source-times");
    let rows = case["database"]["keyValues"][1]["value"]["messages"]
        .as_array_mut()
        .unwrap();
    let values = [json!("invalid"), json!(1.5), json!(true), json!(i64::MAX)];
    for (row, value) in rows.iter_mut().zip(values) {
        row.as_object_mut().unwrap().remove("createdAt");
        row["timestamp"] = value;
        row["sessionCreatedAt"] = json!(1767225600000i64);
    }
    for expected in case["expected"].as_array_mut().unwrap() {
        expected["timestamp"] = Value::Null;
        expected["timeOrigin"] = json!("unknown");
    }
    assert_case(case);
    let mut case = input_case("cursor-legacy-four-source-times");
    case["database"]["keyValues"][1]["value"]["messages"][0]["timestamp"] = json!(1000);
    case["expected"][0]["timestamp"] = json!("1970-01-01T00:00:01.000Z");
    assert_case(case);
}

#[test]
fn cursor_store_wire_preserves_selected_root_order_and_excludes_abandoned_history() {
    let case: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/session-migration/store-wire.json"
    ))
    .unwrap();
    assert_case(case);
}
