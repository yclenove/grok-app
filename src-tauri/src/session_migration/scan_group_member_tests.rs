//! Exact member commands reuse the real scan fixture, not a forged membership.
use super::*;
use crate::session_migration::errors::parse_command_err;
use rusqlite::params;

fn exact(
    f: &Fixture,
    group: &str,
    candidate: &str,
) -> Result<scan_groups::SourceGroupMember, String> {
    tauri::async_runtime::block_on(crate::commands::session_import_scan_group_member(
        f.scan.scan_id.clone(),
        f.scan.epoch,
        group.into(),
        candidate.into(),
    ))
}

#[test]
fn exact_member_reads_the_chosen_nonfirst_metadata_offline_without_mutating_selection_or_schema() {
    let f = Fixture::new();
    let frozen = f.freeze();
    let accepted = f.choose_second(&frozen).selection;
    let selected = f.choice(&accepted, &f.group.group_id);
    let first = scan_groups::members(&f.scan.scan_id, 1, &f.group.group_id, None, Some(1)).unwrap();
    assert_ne!(
        first.members[0].candidate.candidate_id,
        selected.candidate.candidate_id
    );
    let conn = scan::open_scan(&f.scan.scan_id, 1).unwrap();
    let schema_before: i64 = conn
        .query_row("PRAGMA schema_version", [], |r| r.get(0))
        .unwrap();
    let references_before: Vec<String> = conn
        .prepare("SELECT json FROM candidates ORDER BY seq")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let body = std::fs::read(f.root.join("a.jsonl")).unwrap();
    let offline = f._home.path.join("offline-cursor");
    std::fs::rename(&f.root, &offline).unwrap();
    let actual = exact(&f, &f.group.group_id, &selected.candidate.candidate_id).unwrap();
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(&f.members[1]).unwrap()
    );
    assert!(
        serde_json::to_vec(&actual).unwrap().len()
            <= crate::session_migration::budget::READ_PAGE_TEXT_BYTES
    );
    assert_eq!(
        serde_json::to_value(f.get()).unwrap(),
        serde_json::to_value(&accepted).unwrap()
    );
    assert_eq!(
        serde_json::to_value(f.choice(&accepted, &f.group.group_id)).unwrap(),
        serde_json::to_value(selected).unwrap()
    );
    assert_eq!(
        conn.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        schema_before
    );
    let references_after: Vec<String> = conn
        .prepare("SELECT json FROM candidates ORDER BY seq")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(references_after, references_before);
    assert_eq!(std::fs::read(offline.join("a.jsonl")).unwrap(), body);
}

#[test]
fn exact_member_rejects_other_group_scan_epoch_missing_and_path_shaped_ids_without_fallback() {
    let f = Fixture::new();
    let id = &f.members[1].candidate.candidate_id;
    for (group, candidate, expected) in [
        (
            f.other.group_id.clone(),
            id.clone(),
            ErrorCode::SourceNotFound,
        ),
        (
            f.group.group_id.clone(),
            format!("candidate:{}:{}", f.scan.scan_id, "f".repeat(64)),
            ErrorCode::SourceNotFound,
        ),
        (
            f.group.group_id.clone(),
            "candidate:foreign:abc".into(),
            ErrorCode::PlanStale,
        ),
        (
            f.group.group_id.clone(),
            f.root.to_string_lossy().into_owned(),
            ErrorCode::PlanStale,
        ),
        ("group:foreign:abc".into(), id.clone(), ErrorCode::PlanStale),
    ] {
        assert_eq!(
            parse_command_err(&exact(&f, &group, &candidate).unwrap_err()),
            Some(expected)
        );
    }
    assert_eq!(
        scan_groups::member(&f.scan.scan_id, 2, &f.group.group_id, id)
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    let other_scan = scan::scan_start(vec![(
        Provider::Cursor,
        f.root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    assert_eq!(
        scan_groups::member(&other_scan.scan_id, 1, &f.group.group_id, id)
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        exact(&f, &f.group.group_id, id)
            .unwrap()
            .candidate
            .candidate_id,
        *id
    );
}

#[test]
fn exact_member_checks_all_group_identity_columns_and_attested_candidate_metadata() {
    let f = Fixture::new();
    let id = &f.members[1].candidate.candidate_id;
    let conn = scan::open_scan(&f.scan.scan_id, 1).unwrap();
    for column in ["source_id", "provider", "host", "identity", "proof"] {
        let original: String = conn
            .query_row(
                &format!("SELECT {column} FROM source_groups WHERE group_id=?1"),
                [&f.group.group_id],
                |r| r.get(0),
            )
            .unwrap();
        let changed = if column == "proof" {
            "artifact"
        } else {
            "tampered"
        };
        conn.execute(
            &format!("UPDATE source_groups SET {column}=?1 WHERE group_id=?2"),
            params![changed, f.group.group_id],
        )
        .unwrap();
        assert_eq!(
            parse_command_err(&exact(&f, &f.group.group_id, id).unwrap_err()),
            Some(ErrorCode::SchemaUnsupported),
            "{column}"
        );
        conn.execute(
            &format!("UPDATE source_groups SET {column}=?1 WHERE group_id=?2"),
            params![original, f.group.group_id],
        )
        .unwrap();
        exact(&f, &f.group.group_id, id).unwrap();
    }
    let raw: String = conn
        .query_row(
            "SELECT json FROM candidates WHERE candidate_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    for (kind, expected) in [
        ("root", ErrorCode::PlanStale),
        ("legacy", ErrorCode::PlanStale),
        ("revision", ErrorCode::PlanStale),
        ("location", ErrorCode::PlanStale),
        ("seal", ErrorCode::PlanStale),
        ("missing-pin", ErrorCode::PlanStale),
        ("new-pin", ErrorCode::ImportSchemaTooNew),
        ("unknown-pin", ErrorCode::SchemaUnsupported),
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        match kind {
            "root" => value["root_path"] = json!("changed"),
            "legacy" => value["legacy_id"] = json!("changed"),
            "revision" => value["candidate"]["sourceRevision"] = json!("changed"),
            "location" => value["location"]["host"] = json!("changed"),
            "seal" => value["sourceMember"]["seal"] = json!("0".repeat(64)),
            "missing-pin" => value
                .as_object_mut()
                .unwrap()
                .remove("sourceMember")
                .map(|_| ())
                .unwrap(),
            "new-pin" => value["sourceMember"]["version"] = json!(2),
            "unknown-pin" => value["sourceMember"]["unknown"] = json!(true),
            _ => unreachable!(),
        }
        conn.execute(
            "UPDATE candidates SET json=?1 WHERE candidate_id=?2",
            params![value.to_string(), id],
        )
        .unwrap();
        assert_eq!(
            parse_command_err(&exact(&f, &f.group.group_id, id).unwrap_err()),
            Some(expected),
            "{kind}"
        );
        conn.execute(
            "UPDATE candidates SET json=?1 WHERE candidate_id=?2",
            params![raw, id],
        )
        .unwrap();
        exact(&f, &f.group.group_id, id).unwrap();
    }
}

#[test]
fn exact_member_does_not_upgrade_legacy_or_repair_incompatible_group_schema() {
    for (mutation, expected) in [
        ("DROP TABLE source_group_members; DROP TABLE source_groups; ALTER TABLE meta DROP COLUMN group_schema_version", ErrorCode::PlanStale),
        ("DROP INDEX source_group_members_page", ErrorCode::SchemaUnsupported),
        ("UPDATE meta SET group_schema_version=2", ErrorCode::ImportSchemaTooNew),
    ] {
        let f = Fixture::new();
        let conn = scan::open_scan(&f.scan.scan_id, 1).unwrap();
        conn.execute_batch(mutation).unwrap();
        let before: i64 = conn.query_row("PRAGMA schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(parse_command_err(&exact(&f, &f.group.group_id, &f.members[1].candidate.candidate_id).unwrap_err()), Some(expected));
        assert_eq!(conn.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0)).unwrap(), before);
    }
}

#[test]
fn exact_member_rejects_oversized_metadata_before_copying_it_into_the_response() {
    let f = Fixture::new();
    let id = &f.members[1].candidate.candidate_id;
    let conn = scan::open_scan(&f.scan.scan_id, 1).unwrap();
    let mut reference = scan::candidate(id).unwrap();
    reference.candidate.title = "中".repeat(100000);
    conn.execute(
        "UPDATE candidates SET json=?1 WHERE candidate_id=?2",
        params![serde_json::to_string(&reference).unwrap(), id],
    )
    .unwrap();
    assert_eq!(
        parse_command_err(&exact(&f, &f.group.group_id, id).unwrap_err()),
        Some(ErrorCode::ResourceLimit)
    );
}
