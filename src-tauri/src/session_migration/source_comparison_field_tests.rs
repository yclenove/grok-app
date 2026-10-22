use super::*;
use crate::session_migration::{engine, jobs, source_comparison::tests::Fixture, types::Provider};

fn request(f: &Fixture, position: u32, side: FieldSide, field: &str) -> FieldRequest {
    let mut rows = f.request.clone();
    rows.limit = Some(100);
    let compared = f.command(rows).unwrap();
    let side_name = match side {
        FieldSide::Left => "left",
        FieldSide::Right => "right",
    };
    FieldRequest {
        version: 1,
        left: f.request.left.clone(),
        right: f.request.right.clone(),
        side,
        position,
        field: field.into(),
        expected_fields_digest: compared["rows"][position as usize][side_name]["fieldsDigest"]
            .as_str()
            .unwrap()
            .into(),
        cursor: None,
        limit_bytes: Some(4096),
    }
}

fn command(request: FieldRequest) -> Result<Value, String> {
    tauri::async_runtime::block_on(crate::commands::session_import_source_comparison_field(
        request,
    ))
}

#[test]
fn source_comparison_field_command_reads_unicode_escaped_full_body_offline_without_mutation() {
    let f = Fixture::with_prefix(2, &"界🚀\n\"\\".repeat(900));
    let selector =
        crate::session_migration::group_selection::get(&f.scan.scan_id, f.scan.epoch).unwrap();
    let sources: Vec<_> = ["a.jsonl", "b.jsonl", "other.jsonl"]
        .into_iter()
        .map(|file| (file, std::fs::read(f.root.join(file)).unwrap()))
        .collect();
    let mut r = request(&f, 0, FieldSide::Left, "blocks");
    r.limit_bytes = Some(4093);
    let first = command(r.clone()).unwrap();
    assert!(first["nextCursor"].is_string());
    std::fs::rename(&f.root, f.root.with_file_name("offline")).unwrap();
    engine::reset_runtime();
    let mut text = String::new();
    let mut offset = 0;
    let mut cursors = std::collections::HashSet::new();
    let mut digest = None;
    loop {
        let part = command(r.clone()).unwrap();
        assert_eq!(part["startByte"], offset);
        let chunk = part["text"].as_str().unwrap();
        assert!(chunk.len() <= 4093 && !chunk.is_empty());
        assert_eq!(
            part["endByte"].as_u64().unwrap(),
            offset + chunk.len() as u64
        );
        assert_eq!(part["fieldsDigest"], r.expected_fields_digest);
        assert_eq!(part["available"], true);
        assert!(serde_json::to_vec(&part).unwrap().len() <= budget::READ_PAGE_TEXT_BYTES);
        if let Some(previous) = &digest {
            assert_eq!(&part["fieldDigest"], previous);
        }
        digest = Some(part["fieldDigest"].clone());
        text.push_str(chunk);
        offset = part["endByte"].as_u64().unwrap();
        match part["nextCursor"].as_str() {
            Some(raw) => {
                assert!(cursors.insert(raw.to_string()));
                r.cursor = Some(raw.into());
            }
            None => {
                assert_eq!(part["complete"], true);
                assert_eq!(part["totalBytes"], offset);
                break;
            }
        }
    }
    assert_eq!(digest.unwrap(), hash::sha256_bytes(text.as_bytes()));
    let blocks: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        blocks[0]["text"],
        format!("{}a.jsonl BODY 0", "界🚀\n\"\\".repeat(900))
    );
    for (file, before) in sources {
        assert_eq!(
            std::fs::read(f.root.with_file_name("offline").join(file)).unwrap(),
            before
        );
    }
    assert_eq!(
        crate::session_migration::group_selection::get(&f.scan.scan_id, f.scan.epoch).unwrap(),
        selector
    );
}

#[test]
fn source_comparison_field_missing_is_not_json_null_and_names_are_all_typed_body_keys() {
    let f = Fixture::new(1);
    let missing = command(request(&f, 0, FieldSide::Left, "summary")).unwrap();
    assert_eq!(missing["available"], false);
    assert_eq!(missing["text"], "");
    assert!(missing["fieldDigest"].is_null());
    assert_eq!(missing["startByte"], 0);
    assert_eq!(missing["endByte"], 0);
    assert_eq!(missing["totalBytes"], 0);
    assert_eq!(missing["complete"], true);
    assert!(missing["nextCursor"].is_null());
    let pair = FixedPair::load(&f.request.left, &f.request.right).unwrap();
    let event = pair.left.event(0).unwrap().unwrap();
    let body = fields(&event).unwrap();
    assert_eq!(
        missing["fieldNames"],
        json!(body.as_object().unwrap().keys().collect::<Vec<_>>())
    );
    for field in ["id", "ordinal", "spans", "timestamp", "timeOrigin"] {
        assert!(!missing["fieldNames"]
            .as_array()
            .unwrap()
            .contains(&json!(field)));
    }
    assert_eq!(field_text(Some(&Value::Null)).as_deref(), Some("null"));
    assert_eq!(field_text(None), None);
}

#[test]
fn source_comparison_field_formal_typed_tools_and_unknown_payload_are_readable() {
    let records = vec![
        json!({"role":"user","composerId":"same","content":"hello"}),
        json!({"role":"assistant","composerId":"same","content":[{"type":"tool_use","id":"call","name":"Read","input":null}]}),
        json!({"role":"user","composerId":"same","content":[{"type":"tool_result","tool_use_id":"call","content":"完整结果🚀"}]}),
        json!({"role":"assistant","composerId":"same","content":[{"type":"unknown-block","value":"opaque"}]}),
    ];
    let f = Fixture::from_records(
        Provider::Cursor,
        vec![("a.jsonl", records.clone()), ("b.jsonl", records)],
    );
    let pair = FixedPair::load(&f.request.left, &f.request.right).unwrap();
    let mut kinds = std::collections::BTreeSet::new();
    for position in 0..4 {
        let event = pair.left.event(position).unwrap().unwrap();
        kinds.insert(event.kind_name());
        let body = fields(&event).unwrap();
        for (field, value) in body.as_object().unwrap() {
            let part = command(request(&f, position as u32, FieldSide::Left, field)).unwrap();
            assert_eq!(part["text"], hash::canonical_json(value), "{field}");
            assert_eq!(part["available"], true);
            assert_eq!(part["eventId"], event.id());
        }
        if event.kind_name() == "tool-call" {
            let input = command(request(&f, position as u32, FieldSide::Left, "input")).unwrap();
            let value: Value = serde_json::from_str(input["text"].as_str().unwrap()).unwrap();
            assert!(value["value"].is_null());
        }
    }
    assert!(
        kinds.contains("message")
            && kinds.contains("tool-call")
            && kinds.contains("tool-result")
            && kinds.contains("unsupported")
    );
}

#[test]
fn source_comparison_field_compaction_serialization_preserves_full_summary_and_unknown_coverage() {
    use crate::session_migration::types::{ContentBlock, EventBase, ImportedEvent, TimeOrigin};
    let event = ImportedEvent::Compaction {
        base: EventBase {
            id: "compact".into(),
            ordinal: 0,
            spans: vec![],
            timestamp: None,
            time_origin: TimeOrigin::Unknown,
        },
        summary: vec![ContentBlock::text("完整摘要🚀".repeat(900))],
        covered_event_ids: None,
        coverage_known: false,
    };
    let body = fields(&event).unwrap();
    let text = field_text(body.get("summary")).unwrap();
    assert!(text.contains(&"完整摘要🚀".repeat(900)));
    assert_eq!(
        field_text(body.get("coverage_known")).as_deref(),
        Some("false")
    );
    assert_eq!(field_text(body.get("covered_event_ids")), None);
    // This is a typed-body unit check, not forged non-Cursor group membership.
}

#[test]
fn source_comparison_field_cursor_rejects_rebinding_forgery_row_cursor_and_intrinsic_bad_offsets() {
    let f = Fixture::with_prefix(2, &"界".repeat(700));
    let mut r = request(&f, 0, FieldSide::Left, "blocks");
    r.limit_bytes = Some(4);
    let page = command(r.clone()).unwrap();
    let raw = page["nextCursor"].as_str().unwrap().to_string();
    let pair = FixedPair::load(&r.left, &r.right).unwrap();
    let filter = format!(
        "source-comparison-field:v1:{}",
        hash::options_hash(&json!({
            "side":r.side, "position":r.position, "field":r.field,
            "fieldsDigest":r.expected_fields_digest, "fieldDigest":page["fieldDigest"],
        }))
    );
    let body = fields(&pair.left.event(0).unwrap().unwrap()).unwrap();
    let text = field_text(body.get("blocks")).unwrap();
    let inside_unicode = (text.find('界').unwrap() + 1) as u32;
    for (offset, mark, exp) in [
        (u32::MAX, None, chrono::Utc::now().timestamp() + 100),
        (inside_unicode, None, chrono::Utc::now().timestamp() + 100),
        (
            4,
            Some("alien".into()),
            chrono::Utc::now().timestamp() + 100,
        ),
        (4, None, 1),
    ] {
        let mut bad = r.clone();
        bad.cursor =
            Some(cursor::encode_bound_cursor_at(pair.scope(&filter), offset, mark, exp).unwrap());
        assert!(command(bad).is_err());
    }
    for change in 0..7 {
        let mut bad = r.clone();
        bad.cursor = Some(raw.clone());
        match change {
            0 => std::mem::swap(&mut bad.left, &mut bad.right),
            1 => {
                let other = request(&f, 0, FieldSide::Right, "blocks");
                bad.side = other.side;
                bad.expected_fields_digest = other.expected_fields_digest;
            }
            2 => {
                bad.position = 1;
                bad.expected_fields_digest =
                    request(&f, 1, FieldSide::Left, "blocks").expected_fields_digest;
            }
            3 => bad.field = "role".into(),
            4 => bad.cursor = Some(cursor::tamper_cursor_field(&raw, "offset", json!(7))),
            5 => bad.cursor = Some(cursor::tamper_cursor_signature(&raw)),
            _ => {
                bad.cursor = Some(
                    f.command(f.request.clone()).unwrap()["nextCursor"]
                        .as_str()
                        .unwrap()
                        .into(),
                )
            }
        }
        assert!(command(bad).is_err(), "change {change}");
    }
    r.cursor = Some(raw);
    assert!(command(r).is_ok());
}

#[test]
fn source_comparison_field_rejects_foreign_identity_missing_tail_digest_pointer_and_version() {
    let f = Fixture::new(1);
    let valid = request(&f, 0, FieldSide::Left, "blocks");
    for change in 0..9 {
        let mut bad = valid.clone();
        match change {
            0 => bad.version = 2,
            1 => bad.position = 1,
            2 => bad.expected_fields_digest = "0".repeat(64),
            3 => bad.field = "/blocks/0/text".into(),
            4 => bad.left.candidate_id = bad.right.candidate_id.clone(),
            5 => bad.right = bad.left.clone(),
            6 => bad.left.leaf_id = "foreign".into(),
            7 => bad.right.source_revision = "foreign".into(),
            _ => {
                let other = f
                    .scan
                    .candidates
                    .iter()
                    .find(|c| c.locator.ends_with("other.jsonl"))
                    .unwrap();
                let (id, details) =
                    engine::preview(&other.candidate_id, Some(&other.source_revision)).unwrap();
                bad.right = ComparisonSide {
                    preview_id: id,
                    candidate_id: other.candidate_id.clone(),
                    source_revision: other.source_revision.clone(),
                    leaf_id: details.active_leaf.unwrap(),
                };
            }
        }
        assert!(command(bad).is_err(), "change {change}");
    }
    let mut unknown = serde_json::to_value(valid).unwrap();
    unknown["path"] = json!("a.jsonl");
    assert!(serde_json::from_value::<FieldRequest>(unknown).is_err());
}

#[test]
fn source_comparison_field_rechecks_expiry_bytes_and_member_lease_in_ram_or_reloaded_including_absent(
) {
    for reopened in [false, true] {
        for mutation in 0..3 {
            for absent in [false, true] {
                let f = Fixture::new(1);
                let r = request(
                    &f,
                    0,
                    FieldSide::Left,
                    if absent { "summary" } else { "blocks" },
                );
                let path = jobs::find_preview_dir(&f.request.right.preview_id)
                    .unwrap()
                    .unwrap();
                match mutation {
                    0 => engine::expire_preview(&f.request.right.preview_id),
                    1 => std::fs::write(path.join("events.jsonl"), "{}\n").unwrap(),
                    _ => {
                        let file = path.join("lease.json");
                        let mut lease: Value =
                            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
                        lease["sourceMember"]["reference"]["sourceRevision"] = json!("tampered");
                        std::fs::write(file, serde_json::to_vec(&lease).unwrap()).unwrap();
                    }
                }
                if reopened {
                    engine::reset_runtime();
                }
                assert!(command(r).is_err());
            }
        }
    }
}

#[test]
fn source_comparison_field_segment_boundaries_and_request_byte_cap_are_enforced() {
    assert_eq!(segment_end("🚀界", 0, 4).unwrap(), 4);
    assert!(segment_end("🚀界", 0, 1).is_err());
    assert!(segment_end("🚀界", 1, 9).is_err());
    assert!(segment_end("🚀界", 8, 9).is_err());
    let records = |role: &str| {
        vec![
            json!({"composerId":"same","role":role,"content":"header"}),
            json!({"composerId":"same","role":role,"content":"界\"\\".repeat(40000)}),
        ]
    };
    let f = Fixture::from_records(
        Provider::Cursor,
        vec![
            ("a.jsonl", records("user")),
            ("b.jsonl", records("assistant")),
        ],
    );
    let mut r = request(&f, 1, FieldSide::Left, "blocks");
    r.limit_bytes = Some(usize::MAX);
    let part = command(r).unwrap();
    assert!(part["text"].as_str().unwrap().len() <= FIELD_MAX_BYTES);
    assert!(part["nextCursor"].is_string());
    assert!(serde_json::to_vec(&part).unwrap().len() <= budget::READ_PAGE_TEXT_BYTES);
}
