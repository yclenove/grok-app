//! Synthetic public-wire adversarial fixtures, independent of the decoder.
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;
use serde_json::json;

fn frame(tag: u8, hash: &str) -> Vec<u8> {
    let mut bytes = vec![tag, 32];
    bytes.extend(hex::decode(hash).unwrap());
    bytes
}

fn database(home: &IsolatedAppHome, messages: &[Value], order: &[usize]) -> (PathBuf, Vec<String>) {
    let directory = home.path.join("wire source #合成").join("synthetic-wire");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("store.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE blobs(id TEXT PRIMARY KEY,data BLOB); CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT);").unwrap();
    let hashes: Vec<_> = messages
        .iter()
        .map(|message| {
            let bytes = serde_json::to_vec(message).unwrap();
            let hash = sha256_bytes(&bytes);
            conn.execute(
                "INSERT OR IGNORE INTO blobs(id,data) VALUES(?1,?2)",
                rusqlite::params![hash, bytes],
            )
            .unwrap();
            hash
        })
        .collect();
    let root: Vec<u8> = order
        .iter()
        .flat_map(|index| frame(0x0a, &hashes[*index]))
        .collect();
    let hash = sha256_bytes(&root);
    conn.execute(
        "INSERT INTO blobs(id,data) VALUES(?1,?2)",
        rusqlite::params![hash, root],
    )
    .unwrap();
    let meta = hex::encode(
        serde_json::to_vec(
            &json!({"latestRootBlobId":hash,"name":"合成 Wire","createdAt":1767225600000i64}),
        )
        .unwrap(),
    );
    conn.execute("INSERT INTO meta(key,value) VALUES('0',?1)", [meta])
        .unwrap();
    drop(conn);
    (path, hashes)
}

fn texts(out: &AdapterOutput) -> Vec<String> {
    out.graph
        .events
        .iter()
        .map(ImportedEvent::searchable_text)
        .collect()
}

#[test]
fn source_fidelity_repeated_leaf_occurrences_and_equal_bodies_keep_identity() {
    let home = IsolatedAppHome::acquire("wire-repeat");
    let (path, hashes) = database(
        &home,
        &[
            json!({"id":"one","role":"user","content":"同样正文"}),
            json!({"id":"two","role":"user","content":"同样正文"}),
        ],
        &[0, 1, 0],
    );
    let before = std::fs::read(&path).unwrap();
    let out = super::super::normalize_store(&path).unwrap();
    assert_eq!(out.outcome, ImportOutcome::Complete);
    assert_eq!(texts(&out), vec!["同样正文"; 3]);
    assert_eq!(
        out.graph
            .events
            .iter()
            .map(ImportedEvent::id)
            .collect::<HashSet<_>>()
            .len(),
        3
    );
    for (index, event) in out.graph.events.iter().enumerate() {
        assert_eq!(event.base().ordinal, index as u32);
        assert_eq!(event.base().spans[0].record, hashes[[0, 1, 0][index]]);
        assert_eq!(event.base().timestamp, None);
        assert_eq!(event.base().time_origin, TimeOrigin::Unknown);
    }
    assert!(!out
        .graph
        .relations
        .iter()
        .any(|relation| relation.kind == RelationKind::Fork));
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn source_fidelity_tools_stay_typed_and_abandoned_results_cannot_pair() {
    let home = IsolatedAppHome::acquire("wire-tools");
    let messages = [
        json!({"role":"user","content":"开始"}),
        json!({"role":"assistant","content":null,"tool_calls":[{"id":"t1","function":{"name":"Read","arguments":"{\"path\":\"a.txt\"}"}}]}),
        json!({"role":"tool","tool_call_id":"t1","content":"SOURCE_TOOL_RESULT","is_error":false}),
        json!({"role":"assistant","content":[{"type":"tool_use","toolCallId":"t2","toolName":"Write","args":{"path":"b.txt"}}]}),
        json!({"role":"assistant","content":[{"type":"future-video","value":"opaque"}]}),
        json!({"role":"tool","tool_call_id":"t2","content":"ABANDONED_TOOL_RESULT","is_error":false}),
    ];
    let (path, _) = database(&home, &messages, &[0, 1, 2, 3, 4]);
    let out = super::super::normalize_store(&path).unwrap();
    assert_eq!(
        out.graph
            .events
            .iter()
            .map(ImportedEvent::kind_name)
            .collect::<Vec<_>>(),
        vec![
            "message",
            "tool-call",
            "tool-result",
            "tool-call",
            "unsupported"
        ]
    );
    assert_eq!(out.outcome, ImportOutcome::Partial);
    assert_eq!(
        out.issues
            .iter()
            .filter(|issue| issue.code == "unpaired-tool-call")
            .count(),
        1
    );
    assert!(out.issues.iter().any(|issue| issue.code == "unknown-block"));
    assert!(!texts(&out).join("\n").contains("ABANDONED_TOOL_RESULT"));
    match &out.graph.events[1] {
        ImportedEvent::ToolCall {
            call_id,
            name,
            input: ToolInput::Json { value },
            parent_message_id,
            ..
        } => {
            assert_eq!(call_id, "t1");
            assert_eq!(name, "Read");
            assert_eq!(value, &json!({"path":"a.txt"}));
            assert!(
                parent_message_id.is_none(),
                "tool-only assistant must not claim the preceding user as its message"
            );
        }
        other => panic!("unexpected call {other:?}"),
    }
    match &out.graph.events[2] {
        ImportedEvent::ToolResult {
            call_id, outcome, ..
        } => {
            assert_eq!(call_id, "t1");
            assert_eq!(*outcome, ToolOutcome::Success);
        }
        other => panic!("unexpected result {other:?}"),
    }
}

#[test]
fn source_fidelity_modern_result_blocks_and_mixed_calls_keep_source_ownership() {
    let home = IsolatedAppHome::acquire("wire-modern-tools");
    let (path, _) = database(
        &home,
        &[
            json!({"role":"assistant","content":[{"type":"text","text":"检查"},{"type":"tool_call","toolCallId":"t1","toolName":"Read","arguments":"{}"},{"type":"tool_call","toolCallId":"t2","toolName":"Read","arguments":"{}"}]}),
            json!({"role":"tool","is_error":false,"content":[{"type":"tool_result","toolCallId":"t1","content":"result-one"},{"type":"tool_result","tool_use_id":"t2","content":"result-two"}]}),
        ],
        &[0, 1],
    );
    let out = super::super::normalize_store(&path).unwrap();
    assert_eq!(out.outcome, ImportOutcome::Complete);
    assert_eq!(
        out.graph
            .events
            .iter()
            .map(ImportedEvent::kind_name)
            .collect::<Vec<_>>(),
        vec![
            "message",
            "tool-call",
            "tool-call",
            "tool-result",
            "tool-result"
        ]
    );
    for event in &out.graph.events[1..3] {
        match event {
            ImportedEvent::ToolCall {
                parent_message_id, ..
            } => assert_eq!(parent_message_id.as_deref(), Some(out.graph.events[0].id())),
            other => panic!("not a call {other:?}"),
        }
    }
    assert_eq!(texts(&out)[3..], ["result-one", "result-two"]);
}

#[test]
fn source_fidelity_absent_metadata_does_not_import_arbitrary_json_leaves() {
    let home = IsolatedAppHome::acquire("wire-no-metadata");
    let (path, _) = database(
        &home,
        &[json!({"role":"user","content":"MUST_NOT_IMPORT"})],
        &[0],
    );
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE meta;").unwrap();
    drop(conn);
    assert_eq!(
        super::super::normalize_store(&path).unwrap_err().code,
        ErrorCode::FormatUnsupported
    );
}

#[test]
fn source_fidelity_truncated_tree_is_partial_and_keeps_valid_prefix() {
    let home = IsolatedAppHome::acquire("wire-truncated-tree");
    let (path, _) = database(&home, &[json!({"role":"user","content":"合法前缀"})], &[0]);
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE blobs SET data=CAST(data || x'12200102' AS BLOB) WHERE length(data)=34",
        [],
    )
    .unwrap();
    drop(conn);
    let out = super::super::normalize_store(&path).unwrap();
    assert_eq!(texts(&out), vec!["合法前缀"]);
    assert_eq!(out.outcome, ImportOutcome::Partial);
    assert!(out
        .issues
        .iter()
        .any(|issue| issue.message.contains("truncated")));
}

#[test]
fn source_fidelity_invalid_db_metadata_never_falls_back_to_all_blobs() {
    for meta in ["f", "zz", "ff", "7b7d", "5b5d"] {
        let home = IsolatedAppHome::acquire("wire-meta-invalid");
        let (path, _) = database(
            &home,
            &[json!({"role":"user","content":"MUST_NOT_FALL_BACK"})],
            &[0],
        );
        let conn = Connection::open(&path).unwrap();
        conn.execute("UPDATE meta SET value=?1", [meta]).unwrap();
        drop(conn);
        assert_eq!(
            super::super::normalize_store(&path).unwrap_err().code,
            ErrorCode::FormatUnsupported
        );
    }
    let home = IsolatedAppHome::acquire("wire-missing-root");
    let (path, _) = database(
        &home,
        &[json!({"role":"user","content":"MUST_NOT_FALL_BACK"})],
        &[0],
    );
    let conn = Connection::open(&path).unwrap();
    let metadata =
        hex::encode(serde_json::to_vec(&json!({"latestRootBlobId":"ab".repeat(32)})).unwrap());
    conn.execute("UPDATE meta SET value=?1", [metadata])
        .unwrap();
    drop(conn);
    assert_eq!(
        super::super::normalize_store(&path).unwrap_err().code,
        ErrorCode::PartialHistory
    );
}

#[test]
fn source_fidelity_missing_cycle_and_bad_leaf_are_partial_in_source_order() {
    let home = IsolatedAppHome::acquire("wire-damaged");
    let (path, hashes) = database(
        &home,
        &[
            json!({"role":"user","content":"先"}),
            json!({"role":"assistant","content":"后"}),
        ],
        &[0, 1],
    );
    let conn = Connection::open(&path).unwrap();
    let root = "ab".repeat(32);
    let missing = "cd".repeat(32);
    let malformed = "ef".repeat(32);
    let mut bytes = frame(0x0a, &hashes[0]);
    bytes.extend(frame(0x12, &root));
    bytes.extend(frame(0x0a, &missing));
    bytes.extend(frame(0x0a, &malformed));
    bytes.extend(frame(0x0a, &hashes[1]));
    conn.execute(
        "INSERT INTO blobs(id,data) VALUES(?1,?2)",
        rusqlite::params![root, bytes],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO blobs(id,data) VALUES(?1,?2)",
        rusqlite::params![malformed, vec![34u8, 127, b'{']],
    )
    .unwrap();
    let metadata = hex::encode(serde_json::to_vec(&json!({"latestRootBlobId":root})).unwrap());
    conn.execute("UPDATE meta SET value=?1", [metadata])
        .unwrap();
    drop(conn);
    let out = super::super::normalize_store(&path).unwrap();
    assert_eq!(texts(&out), vec!["先", "后"]);
    assert_eq!(out.outcome, ImportOutcome::Partial);
    for code in ["store-cycle", "missing-blob", "unknown-block"] {
        assert!(
            out.issues.iter().any(|issue| issue.code == code),
            "missing diagnostic {code}"
        );
    }
}

#[test]
fn source_fidelity_blob_budget_applies_only_to_reachable_records() {
    let home = IsolatedAppHome::acquire("wire-record-budget");
    let (path, hashes) = database(&home, &[json!({"role":"user","content":"保留"})], &[0]);
    let conn = Connection::open(&path).unwrap();
    let oversized = "ef".repeat(32);
    conn.execute(
        "INSERT INTO blobs(id,data) VALUES(?1,zeroblob(?2))",
        rusqlite::params![oversized, budget::JSON_RECORD_MAX_BYTES as i64 + 1],
    )
    .unwrap();
    drop(conn);
    assert_eq!(
        texts(&super::super::normalize_store(&path).unwrap()),
        vec!["保留"]
    );
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE blobs SET data=?1 WHERE id=?2",
        rusqlite::params![frame(0x0a, &oversized), hashes[0]],
    )
    .unwrap();
    // Replace the selected root's leaf reference, keeping an oversized source leaf.
    let root = "ab".repeat(32);
    conn.execute(
        "INSERT INTO blobs(id,data) VALUES(?1,?2)",
        rusqlite::params![root, frame(0x0a, &oversized)],
    )
    .unwrap();
    let metadata = hex::encode(serde_json::to_vec(&json!({"latestRootBlobId":root})).unwrap());
    conn.execute("UPDATE meta SET value=?1", [metadata])
        .unwrap();
    drop(conn);
    assert_eq!(
        super::super::normalize_store(&path).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
}
