use super::*;
use crate::session_migration::{
    engine, group_selection, jobs, scan, scan_groups, test_home::IsolatedAppHome, types::Provider,
};
use serde_json::{json, Value};
use std::path::PathBuf;

pub(crate) struct Fixture {
    _home: IsolatedAppHome,
    pub root: PathBuf,
    pub scan: scan::ScanState,
    pub request: ComparisonRequest,
}

impl Fixture {
    pub fn new(count: usize) -> Self {
        Self::with_prefix(count, "")
    }

    pub fn with_prefix(count: usize, prefix: &str) -> Self {
        let files = [
            ("a.jsonl", "same", "user", count),
            ("b.jsonl", "same", "assistant", count + 1),
            ("other.jsonl", "other", "user", 1),
        ].into_iter().map(|(file, id, role, tail)| {
            let records = (0..tail)
                .map(|n| {
                    json!({"composerId":id,"role":role,"content":format!("{prefix}{file} BODY {n}"),
                        "timestamp":if file=="a.jsonl" {"2026-10-01T00:00:00Z"} else {"2026-10-02T00:00:00Z"}})
                })
                .collect();
            (file, records)
        }).collect();
        Self::from_records(Provider::Cursor, files)
    }

    pub fn from_records(provider: Provider, files: Vec<(&str, Vec<Value>)>) -> Self {
        let home = IsolatedAppHome::acquire("source-comparison");
        engine::reset_runtime();
        let root = home.path.join("source");
        std::fs::create_dir_all(&root).unwrap();
        for (file, records) in files {
            let body: String = records.iter().map(|record| format!("{record}\n")).collect();
            std::fs::write(root.join(file), body).unwrap();
        }
        let scan = scan::scan_start(vec![(provider, root.to_string_lossy().into_owned())]).unwrap();
        let group = scan_groups::page(
            &scan.scan_id,
            scan.epoch,
            None,
            Some(10),
            &Default::default(),
        )
        .unwrap()
        .groups
        .into_iter()
        .find(|group| group.member_count == 2)
        .unwrap();
        let members =
            scan_groups::members(&scan.scan_id, scan.epoch, &group.group_id, None, Some(10))
                .unwrap()
                .members;
        let side = |file: &str| {
            let member = members
                .iter()
                .find(|member| member.candidate.locator.ends_with(file))
                .unwrap();
            let (preview_id, details) = engine::preview(
                &member.candidate.candidate_id,
                Some(&member.candidate.source_revision),
            )
            .unwrap();
            ComparisonSide {
                preview_id,
                candidate_id: member.candidate.candidate_id.clone(),
                source_revision: member.candidate.source_revision.clone(),
                leaf_id: details.active_leaf.unwrap(),
            }
        };
        let request = ComparisonRequest {
            version: 1,
            left: side("a.jsonl"),
            right: side("b.jsonl"),
            cursor: None,
            limit: Some(1),
        };
        Self {
            _home: home,
            root,
            scan,
            request,
        }
    }

    pub fn command(&self, request: ComparisonRequest) -> Result<Value, String> {
        tauri::async_runtime::block_on(crate::commands::session_import_source_comparison(request))
    }
}

#[test]
fn source_comparison_formal_command_reports_typed_differences_without_selection_or_source_writes() {
    let f = Fixture::new(2);
    let choice = group_selection::get(&f.scan.scan_id, f.scan.epoch).unwrap();
    let before = std::fs::read(f.root.join("a.jsonl")).unwrap();
    let result = f.command(f.request.clone()).unwrap();
    assert_eq!(result["version"], 1);
    assert_eq!(result["alignment"], "position-only");
    assert_eq!(result["equivalence"], "unverified");
    assert_eq!(result["left"]["eventCount"], 2);
    assert_eq!(result["right"]["eventCount"], 3);
    assert_eq!(result["rows"].as_array().unwrap().len(), 1);
    let row = &result["rows"][0];
    assert!(row["differences"]
        .as_array()
        .unwrap()
        .contains(&json!("role")));
    assert!(row["differences"]
        .as_array()
        .unwrap()
        .contains(&json!("blocks")));
    assert!(row["metadataDifferences"]
        .as_array()
        .unwrap()
        .contains(&json!("source-time")));
    assert!(!row["left"]["spans"].as_array().unwrap().is_empty());
    assert_eq!(
        group_selection::get(&f.scan.scan_id, f.scan.epoch).unwrap(),
        choice
    );
    assert_eq!(std::fs::read(f.root.join("a.jsonl")).unwrap(), before);
}

#[test]
fn source_comparison_pagination_binds_fixed_pair_and_retains_missing_tail_after_offline_restart() {
    let f = Fixture::new(2);
    let first = f.command(f.request.clone()).unwrap();
    let cursor = first["nextCursor"].as_str().unwrap().to_string();
    let mut swapped = f.request.clone();
    std::mem::swap(&mut swapped.left, &mut swapped.right);
    swapped.cursor = Some(cursor.clone());
    assert!(f.command(swapped).is_err());
    let mut forged = f.request.clone();
    forged.cursor = Some(format!("{cursor}x"));
    assert!(f.command(forged).is_err());
    std::fs::rename(&f.root, f.root.with_file_name("offline cursor")).unwrap();
    engine::reset_runtime();
    let mut second = f.request.clone();
    second.cursor = Some(cursor);
    let page = f.command(second).unwrap();
    assert_eq!(page["rows"][0]["position"], 1);
    let mut tail = f.request.clone();
    tail.cursor = Some(page["nextCursor"].as_str().unwrap().into());
    let last = f.command(tail).unwrap();
    assert_eq!(last["rows"][0]["position"], 2);
    assert!(last["rows"][0]["left"].is_null());
    assert!(last["rows"][0]["right"].is_object());
    assert_eq!(last["rows"][0]["differences"], json!(["missing-side"]));
    assert!(last["nextCursor"].is_null());
    assert_eq!(last["equivalence"], "unverified");
}

#[test]
fn source_comparison_rejects_foreign_group_unknown_leaf_version_and_tampered_resident_or_disk_lease(
) {
    for reopened in [false, true] {
        let f = Fixture::new(1);
        assert!(f.command(f.request.clone()).is_ok());
        let mut bad = f.request.clone();
        bad.version = 99;
        assert!(f.command(bad).is_err());
        let mut bad = f.request.clone();
        bad.left.candidate_id = f.request.right.candidate_id.clone();
        assert!(f.command(bad).is_err());
        let mut bad = f.request.clone();
        bad.left.source_revision = "not-the-fixed-revision".into();
        assert!(f.command(bad).is_err());
        let mut bad = f.request.clone();
        bad.right.leaf_id = "not-a-leaf".into();
        assert!(f.command(bad).is_err());
        let other = f
            .scan
            .candidates
            .iter()
            .find(|c| c.locator.ends_with("other.jsonl"))
            .unwrap();
        let (id, details) =
            engine::preview(&other.candidate_id, Some(&other.source_revision)).unwrap();
        let mut bad = f.request.clone();
        bad.right = ComparisonSide {
            preview_id: id,
            candidate_id: other.candidate_id.clone(),
            source_revision: other.source_revision.clone(),
            leaf_id: details.active_leaf.unwrap(),
        };
        assert!(f.command(bad).is_err());
        let path = jobs::find_preview_dir(&f.request.left.preview_id)
            .unwrap()
            .unwrap();
        let lease_path = path.join("lease.json");
        let mut lease: Value =
            serde_json::from_slice(&std::fs::read(&lease_path).unwrap()).unwrap();
        lease["sourceMember"]["reference"]["sourceRevision"] = json!("tampered");
        std::fs::write(lease_path, serde_json::to_vec(&lease).unwrap()).unwrap();
        if reopened {
            engine::reset_runtime();
        }
        assert!(f.command(f.request.clone()).is_err());
    }
}

#[test]
fn source_comparison_uses_full_typed_body_not_equal_truncated_excerpts_and_pages_all_positions() {
    let f = Fixture::with_prefix(120, &"界".repeat(900));
    let mut request = f.request.clone();
    request.limit = Some(200);
    let mut positions = Vec::new();
    let mut cursors = std::collections::HashSet::new();
    loop {
        let page = f.command(request.clone()).unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= budget::READ_PAGE_TEXT_BYTES);
        let rows = page["rows"].as_array().unwrap();
        assert!(!rows.is_empty());
        assert!(rows.len() <= budget::READ_PAGE_EVENTS);
        if positions.is_empty() {
            assert!(
                rows.len() < budget::READ_PAGE_EVENTS,
                "UTF-8 byte budget must constrain the page"
            );
            assert_eq!(rows[0]["left"]["excerpt"], rows[0]["right"]["excerpt"]);
            assert_eq!(rows[0]["left"]["truncated"], true);
            assert_ne!(
                rows[0]["left"]["fieldsDigest"],
                rows[0]["right"]["fieldsDigest"]
            );
            assert!(rows[0]["differences"]
                .as_array()
                .unwrap()
                .contains(&json!("blocks")));
        }
        positions.extend(rows.iter().map(|row| row["position"].as_u64().unwrap()));
        match page["nextCursor"].as_str() {
            Some(cursor) => {
                assert!(
                    cursors.insert(cursor.to_string()),
                    "cursor must make progress"
                );
                request.cursor = Some(cursor.to_string());
            }
            None => {
                assert_eq!(page["complete"], true);
                break;
            }
        }
    }
    assert_eq!(positions, (0..121).collect::<Vec<_>>());
}

#[test]
fn source_comparison_snapshot_bytes_and_expiry_are_rechecked_with_or_without_ram_state() {
    for reopened in [false, true] {
        for corrupt in [false, true] {
            let f = Fixture::new(1);
            if corrupt {
                let path = jobs::find_preview_dir(&f.request.left.preview_id)
                    .unwrap()
                    .unwrap();
                std::fs::write(path.join("events.jsonl"), "{}\n").unwrap();
            } else {
                engine::expire_preview(&f.request.left.preview_id);
            }
            if reopened {
                engine::reset_runtime();
            }
            assert!(f.command(f.request.clone()).is_err());
        }
    }
}

#[test]
fn source_comparison_typed_tool_input_result_and_unknown_coverage_are_not_flattened_or_declared_equal(
) {
    use types::{ContentBlock, EventBase, ImportedEvent, TimeOrigin, ToolInput, ToolOutcome};
    let base = EventBase {
        id: "event".into(),
        ordinal: 0,
        spans: vec![],
        timestamp: None,
        time_origin: TimeOrigin::Unknown,
    };
    let call = |value| ImportedEvent::ToolCall {
        base: base.clone(),
        call_id: "one-call".into(),
        name: "Read".into(),
        input: ToolInput::Json { value },
        parent_message_id: None,
    };
    let a = call(json!({"value":1}));
    let b = call(json!({"value":"1"}));
    assert!(row(0, Some(&a), Some(&b)).unwrap()["differences"]
        .as_array()
        .unwrap()
        .contains(&json!("input")));
    let result = |outcome| ImportedEvent::ToolResult {
        base: base.clone(),
        call_id: "one-call".into(),
        blocks: vec![ContentBlock::text("same result")],
        outcome,
    };
    let a = result(ToolOutcome::Success);
    let b = result(ToolOutcome::Unknown);
    let compared = row(0, Some(&a), Some(&b)).unwrap();
    assert!(compared["differences"]
        .as_array()
        .unwrap()
        .contains(&json!("outcome")));
    assert_eq!(compared["uncertain"], true);
    let compact = ImportedEvent::Compaction {
        base,
        summary: vec![ContentBlock::text("summary")],
        covered_event_ids: None,
        coverage_known: false,
    };
    assert_eq!(
        row(0, Some(&compact), Some(&compact)).unwrap()["uncertain"],
        true
    );
    assert!(serde_json::from_value::<ComparisonRequest>(
        json!({"version":1,"left":{},"right":{},"cursor":null,"limit":null,"unknown":true})
    )
    .is_err());
}
