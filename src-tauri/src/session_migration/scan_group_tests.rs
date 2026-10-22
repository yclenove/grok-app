//! Production scan contracts for disk-backed logical groups and raw members.
use crate::session_migration::{
    catalog, engine,
    errors::ErrorCode,
    scan, scan_groups, scan_selection,
    source_locations::{self, SourceAssociationRequest},
    sources,
    test_home::IsolatedAppHome,
    types::Provider,
};
use rusqlite::Connection;
use serde_json::json;
use std::path::Path;

fn transcript(root: &Path, name: &str, declaration: serde_json::Value) {
    std::fs::create_dir_all(root).unwrap();
    let mut record = json!({"role":"user", "content":"GROUP_SYNTHETIC_BODY"});
    record
        .as_object_mut()
        .unwrap()
        .extend(declaration.as_object().unwrap().clone());
    std::fs::write(root.join(name), format!("{record}\n")).unwrap();
}

fn database(root: &Path, keys: &[&str]) {
    std::fs::create_dir_all(root).unwrap();
    let conn = Connection::open(root.join("state.vscdb")).unwrap();
    conn.execute_batch("CREATE TABLE ItemTable(key TEXT PRIMARY KEY,value BLOB)")
        .unwrap();
    for key in keys {
        conn.execute(
            "INSERT INTO ItemTable VALUES(?1,?2)",
            rusqlite::params![key, b"{not valid body".as_slice()],
        )
        .unwrap();
    }
}

fn groups(
    done: &scan::ScanState,
    cursor: Option<String>,
    limit: usize,
    filter: Option<scan_groups::SourceGroupFilter>,
) -> scan_groups::SourceGroupPage {
    tauri::async_runtime::block_on(crate::commands::session_import_scan_groups(
        done.scan_id.clone(),
        done.epoch,
        cursor,
        Some(limit),
        filter,
    ))
    .unwrap()
}

fn members(
    done: &scan::ScanState,
    group: &str,
    cursor: Option<String>,
    limit: usize,
) -> scan_groups::SourceGroupMemberPage {
    tauri::async_runtime::block_on(crate::commands::session_import_scan_group_members(
        done.scan_id.clone(),
        done.epoch,
        group.into(),
        cursor,
        Some(limit),
    ))
    .unwrap()
}

fn schema(conn: &Connection) -> Vec<(String, Option<String>)> {
    conn.prepare("SELECT name,sql FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

fn fixed_selection_matches_groups(done: &scan::ScanState, groups: &scan_groups::SourceGroupPage) {
    let initial =
        crate::session_migration::group_selection::get(&done.scan_id, done.epoch).unwrap();
    let frozen = crate::session_migration::group_selection::freeze_current(
        &done.scan_id,
        done.epoch,
        &initial.selection_id,
        initial.state_version,
    )
    .unwrap();
    assert_eq!(frozen.selected_count, groups.total_count);
    let page = crate::session_migration::group_selection::page(
        &done.scan_id,
        done.epoch,
        &frozen.selection_id,
        None,
        Some(200),
        true,
    )
    .unwrap();
    assert!(page.next_cursor.is_none());
    assert!(page.choices.iter().all(|choice| choice.selected));
    let actual: std::collections::HashSet<_> = page
        .choices
        .iter()
        .map(|choice| choice.candidate.candidate_id.as_str())
        .collect();
    let expected: std::collections::HashSet<_> = groups
        .groups
        .iter()
        .map(|group| group.first_member.candidate_id.as_str())
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn linked_cursor_formats_are_materialized_as_one_disk_group_without_losing_members() {
    let home = IsolatedAppHome::acquire("scan-group-production");
    engine::reset_runtime();
    let root = home.path.join("synthetic cursor");
    std::fs::create_dir_all(&root).unwrap();
    let transcript = root.join("same.jsonl");
    std::fs::write(
        &transcript,
        format!(
            "{}\n",
            json!({"composerId":"group-fact", "role":"user", "content":"TRANSCRIPT_GROUP_MARKER"})
        ),
    )
    .unwrap();
    let db = root.join("state.vscdb");
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch("CREATE TABLE ItemTable(key TEXT PRIMARY KEY,value BLOB)")
        .unwrap();
    conn.execute(
        "INSERT INTO ItemTable VALUES('composerData:group-fact',?1)",
        [serde_json::to_vec(
            &json!({"conversation":[{"id":"message","type":1,"content":"COMPOSER_GROUP_MARKER"}]}),
        )
        .unwrap()],
    )
    .unwrap();
    drop(conn);
    let original = [
        (transcript.clone(), std::fs::read(&transcript).unwrap()),
        (db.clone(), std::fs::read(&db).unwrap()),
    ];
    let done = scan::scan_start(vec![(
        Provider::Cursor,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    assert_eq!(done.issue_count, 0, "{done:?}");
    assert_eq!(done.discovered_count, 2);
    assert_eq!(
        done.candidates[0].native_session_id,
        done.candidates[1].native_session_id
    );
    for (path, bytes) in original {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    let conn = scan::open_scan(&done.scan_id, done.epoch).unwrap();
    let has_groups: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='source_groups')", [], |row| row.get(0)).unwrap();
    assert!(
        has_groups,
        "Formal scan currently lists duplicate formats but does not materialize logical groups"
    );
    let groups: u32 = conn
        .query_row("SELECT count(*) FROM source_groups", [], |row| row.get(0))
        .unwrap();
    let members: u32 = conn
        .query_row("SELECT count(*) FROM source_group_members", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!((groups, members), (1, 2));
    assert_eq!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .len(),
        0,
        "Discovery must not normalize/import the other members"
    );
    engine::reset_runtime();
}

#[test]
fn formal_group_commands_keep_typed_families_and_unproven_stores_independent_without_reading_bodies(
) {
    let home = IsolatedAppHome::acquire("group-families");
    let root = home.path.join("cursor");
    transcript(&root, "composer.jsonl", json!({"composerId":"same"}));
    transcript(&root, "session.jsonl", json!({"sessionId":"same"}));
    transcript(&root, "fallback.jsonl", json!({}));
    transcript(
        &root.join("subagents"),
        "composer.jsonl",
        json!({"composerId":"same"}),
    );
    database(&root, &["composerData:same", "chatData:same"]);
    let store = root.join("same");
    std::fs::create_dir_all(&store).unwrap();
    Connection::open(store.join("store.db"))
        .unwrap()
        .execute_batch("CREATE TABLE unproven(x)")
        .unwrap();
    let done = scan::scan_start(vec![(
        Provider::Cursor,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    assert_eq!(done.issue_count, 0, "{done:?}");
    assert_eq!(done.discovered_count, 7);
    let page = groups(&done, None, 200, None);
    assert_eq!(page.total_count, 6);
    fixed_selection_matches_groups(&done, &page);
    assert_eq!(
        page.groups.iter().filter(|g| g.member_count == 2).count(),
        1
    );
    let linked = page.groups.iter().find(|g| g.member_count == 2).unwrap();
    assert_eq!(linked.proof, "typed-native");
    let member_page = members(&done, &linked.group_id, None, 200);
    assert_eq!(member_page.members.len(), 2);
    let formats: std::collections::HashSet<_> = member_page
        .members
        .iter()
        .map(|m| m.format.as_str())
        .collect();
    assert!(formats.contains("cursor-transcript") && formats.contains("cursor-composer"));
    for member in member_page.members {
        let original = scan::candidate(&member.candidate.candidate_id).unwrap();
        assert_eq!(
            serde_json::to_value(&original.candidate).unwrap(),
            serde_json::to_value(&member.candidate).unwrap()
        );
        assert_eq!(original.location, member.location);
        assert_eq!(member.location.unwrap().kind, "primary");
    }
    assert_eq!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .len(),
        0
    );
    assert!(page
        .groups
        .iter()
        .any(|g| g.first_member.completeness == "store" && g.proof == "artifact"));
}

#[test]
fn same_native_fact_only_links_explicitly_associated_roots_not_two_installations() {
    let home = IsolatedAppHome::acquire("group-installations");
    let a = home.path.join("a");
    let b = home.path.join("b");
    let c = home.path.join("c");
    for root in [&a, &b, &c] {
        transcript(root, "declared.jsonl", json!({"composerId":"same"}));
        transcript(root, "fallback.jsonl", json!({}));
        transcript(
            &root.join("subagents"),
            "child.jsonl",
            json!({"composerId":"same"}),
        );
    }
    let conn = catalog::open_catalog().unwrap();
    let primary = sources::register(&conn, Provider::Cursor, &a, None, "synthetic").unwrap();
    let location = source_locations::associate(
        &conn,
        &SourceAssociationRequest {
            source_id: primary.source_id.clone(),
            provider: Provider::Cursor,
            host: "local".into(),
            path: b.to_string_lossy().into_owned(),
            expected_primary_root: primary.root_path.clone(),
        },
    )
    .unwrap();
    drop(conn);
    let started = scan::begin(vec![
        scan::ScanSourcePath {
            provider: "cursor".into(),
            path: a.to_string_lossy().into_owned(),
            source_id: Some(primary.source_id.clone()),
            location_id: None,
            host: None,
            profile: String::new(),
        },
        scan::ScanSourcePath {
            provider: "cursor".into(),
            path: b.to_string_lossy().into_owned(),
            source_id: Some(primary.source_id.clone()),
            location_id: Some(location.location_id.clone()),
            host: Some("local".into()),
            profile: String::new(),
        },
        scan::ScanSourcePath {
            provider: "cursor".into(),
            path: c.to_string_lossy().into_owned(),
            source_id: None,
            location_id: None,
            host: None,
            profile: String::new(),
        },
    ])
    .unwrap();
    scan::run(&started.scan_id).unwrap();
    let page = groups(&started, None, 200, None);
    assert_eq!(page.total_count, 8);
    fixed_selection_matches_groups(&started, &page);
    assert_eq!(
        page.groups.iter().filter(|g| g.member_count == 2).count(),
        1
    );
    let linked = page.groups.iter().find(|g| g.member_count == 2).unwrap();
    let linked_members = members(&started, &linked.group_id, None, 200);
    assert_eq!(
        linked_members
            .members
            .iter()
            .filter(|m| m.location.as_ref().unwrap().kind == "associated")
            .count(),
        1
    );
    let filtered = groups(
        &started,
        None,
        200,
        Some(scan_groups::SourceGroupFilter {
            provider: Some(Provider::Cursor),
            source_id: Some(primary.source_id),
        }),
    );
    assert_eq!(filtered.total_count, 5);
}

#[test]
fn group_and_member_keyset_pages_are_bounded_and_cursor_owner_filter_epoch_are_checked() {
    let home = IsolatedAppHome::acquire("group-pages");
    let root = home.path.join("cursor");
    for index in 0..211 {
        transcript(
            &root,
            &format!("linked-{index}.jsonl"),
            json!({"composerId":"linked"}),
        );
    }
    for index in 0..201 {
        transcript(
            &root,
            &format!("session-{index}.jsonl"),
            json!({"sessionId":format!("independent-{index}")}),
        );
    }
    let done = scan::scan_start(vec![(
        Provider::Cursor,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    let first = groups(&done, None, 9999, None);
    assert_eq!(first.total_count, 202);
    assert!(!first.groups.is_empty() && first.groups.len() <= 200);
    let mut all_groups = first.groups.clone();
    let mut cursor = first.next_cursor.clone();
    let mut group_pages = 1;
    while let Some(next) = cursor {
        let page = groups(&done, Some(next), 9999, None);
        assert!(!page.groups.is_empty() && page.groups.len() <= 200);
        assert!(
            serde_json::to_vec(&page).unwrap().len()
                <= crate::session_migration::budget::READ_PAGE_TEXT_BYTES
        );
        assert_eq!(page.total_count, 202);
        cursor = page.next_cursor;
        all_groups.extend(page.groups);
        group_pages += 1;
        assert!(group_pages <= 202, "pagination must advance");
    }
    assert_eq!(all_groups.len(), 202);
    assert_eq!(
        all_groups
            .iter()
            .map(|g| &g.group_id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        202
    );
    assert!(group_pages > 1);
    let linked = all_groups.iter().find(|g| g.member_count == 211).unwrap();
    let m1 = members(&done, &linked.group_id, None, 9999);
    assert!(!m1.members.is_empty() && m1.members.len() <= 200);
    assert!(
        serde_json::to_vec(&m1).unwrap().len()
            <= crate::session_migration::budget::READ_PAGE_TEXT_BYTES
    );
    let mut all_members = m1.members.clone();
    let mut cursor = m1.next_cursor.clone();
    let mut member_pages = 1;
    while let Some(next) = cursor {
        let page = members(&done, &linked.group_id, Some(next), 9999);
        assert!(!page.members.is_empty() && page.members.len() <= 200);
        assert!(
            serde_json::to_vec(&page).unwrap().len()
                <= crate::session_migration::budget::READ_PAGE_TEXT_BYTES
        );
        assert_eq!(page.member_count, 211);
        cursor = page.next_cursor;
        all_members.extend(page.members);
        member_pages += 1;
        assert!(member_pages <= 211, "member pagination must advance");
    }
    assert!(member_pages > 1);
    assert_eq!(all_members.len(), 211);
    assert_eq!(
        all_members
            .iter()
            .map(|m| &m.candidate.candidate_id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        211
    );
    let failed = scan_groups::page(
        &done.scan_id,
        1,
        first.next_cursor.as_deref(),
        Some(50),
        &scan_groups::SourceGroupFilter {
            provider: Some(Provider::Codex),
            source_id: None,
        },
    )
    .unwrap_err();
    assert_eq!(failed.code, ErrorCode::PlanStale);
    assert_eq!(
        scan_groups::members(&done.scan_id, 2, &linked.group_id, None, None)
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        scan_groups::members(
            &done.scan_id,
            1,
            &linked.group_id,
            first.next_cursor.as_deref(),
            None
        )
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    let other = all_groups.iter().find(|g| g.member_count == 1).unwrap();
    assert_eq!(
        scan_groups::members(
            &done.scan_id,
            1,
            &other.group_id,
            m1.next_cursor.as_deref(),
            None
        )
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    let another = scan::scan_start(vec![(
        Provider::Cursor,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    assert_eq!(
        scan_groups::page(
            &another.scan_id,
            1,
            first.next_cursor.as_deref(),
            None,
            &Default::default()
        )
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        scan_groups::members(&another.scan_id, 1, &linked.group_id, None, None)
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    let mut tampered = first.next_cursor.unwrap();
    tampered.push('x');
    assert_eq!(
        scan_groups::page(&done.scan_id, 1, Some(&tampered), None, &Default::default())
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
}

#[test]
fn group_reads_do_not_initialize_legacy_or_repair_marked_corrupt_scans() {
    let home = IsolatedAppHome::acquire("group-readonly");
    let root = home.path.join("cursor");
    transcript(&root, "a.jsonl", json!({"sessionId":"a"}));
    for (mutation, expected) in [
        ("DROP TABLE source_group_members; DROP TABLE source_groups; ALTER TABLE meta DROP COLUMN group_schema_version; DROP TABLE candidate_selection_overrides; DROP TABLE candidate_selections", ErrorCode::PlanStale),
        ("DROP TABLE source_group_members", ErrorCode::SchemaUnsupported),
        ("DROP INDEX source_group_members_page", ErrorCode::SchemaUnsupported),
        ("UPDATE meta SET group_schema_version=2", ErrorCode::ImportSchemaTooNew),
    ] {
        let done = scan::scan_start(vec![(Provider::Cursor, root.to_string_lossy().into_owned())]).unwrap();
        let conn = scan::open_scan(&done.scan_id, done.epoch).unwrap();
        conn.execute_batch(mutation).unwrap();
        let before = schema(&conn);
        let result = tauri::async_runtime::block_on(crate::commands::session_import_scan_groups(done.scan_id.clone(), 1, None, None, None));
        assert_eq!(crate::session_migration::errors::parse_command_err(&result.unwrap_err()), Some(expected));
        assert_eq!(schema(&conn), before);
        let readonly = scan::open_scan_readonly(&done.scan_id, done.epoch).unwrap();
        assert!(readonly.execute("UPDATE meta SET state='failed'", []).is_err());
        assert_eq!(scan::page(&done.scan_id, 1, None, None).unwrap().0.len(), 1);
    }
}

#[test]
fn group_member_insertion_failure_rolls_back_raw_candidate_and_group() {
    let home = IsolatedAppHome::acquire("group-rollback");
    let root = home.path.join("cursor");
    transcript(&root, "a.jsonl", json!({"composerId":"a"}));
    let started = scan::begin(vec![scan::ScanSourcePath {
        provider: "cursor".into(),
        path: root.to_string_lossy().into_owned(),
        source_id: None,
        location_id: None,
        host: None,
        profile: String::new(),
    }])
    .unwrap();
    let conn = scan::open_scan(&started.scan_id, 1).unwrap();
    conn.execute_batch("CREATE TRIGGER deny_member BEFORE INSERT ON source_group_members BEGIN SELECT RAISE(ABORT,'synthetic member write failure'); END").unwrap();
    assert!(scan::run(&started.scan_id).is_err());
    for table in ["candidates", "source_groups", "source_group_members"] {
        assert_eq!(
            conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                .get::<_, u32>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(scan::state(&started.scan_id, 1).unwrap().state, "failed");
}

#[test]
fn duplicate_and_late_members_do_not_rewrite_fixed_refs_frozen_selection_or_group_order() {
    let home = IsolatedAppHome::acquire("group-fixed");
    let root = home.path.join("cursor");
    transcript(&root, "a.jsonl", json!({"composerId":"a"}));
    transcript(&root, "b.jsonl", json!({"composerId":"b"}));
    let done = scan::scan_start(vec![(
        Provider::Cursor,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    let first = groups(&done, None, 1, None);
    let reference = scan::candidate(&first.groups[0].first_member.candidate_id).unwrap();
    let original = serde_json::to_value(&reference).unwrap();
    let selection = scan_selection::freeze_current(&done.scan_id, 1).unwrap();
    let fixed =
        scan_selection::page(&done.scan_id, 1, &selection.selection_id, None, None).unwrap();
    let source = sources::SourceRegistration {
        source_id: reference.source_id.clone(),
        provider: Provider::Cursor,
        root_path: reference.root_path.clone(),
        profile: String::new(),
        location: reference.location.clone(),
    };
    let conn = scan::open_scan(&done.scan_id, 1).unwrap();
    let mut duplicate = reference.candidate.clone();
    duplicate.native_session_id = "cursor-identity-v1:composer:changed".into();
    duplicate.source_revision = "changed".into();
    super::add(&conn, &done.scan_id, &source, duplicate).unwrap();
    assert_eq!(
        serde_json::to_value(scan::candidate(&reference.candidate.candidate_id).unwrap()).unwrap(),
        original
    );
    let raw = reference
        .candidate
        .native_session_id
        .strip_prefix("cursor-identity-v1:composer:")
        .unwrap();
    transcript(&root, "late.jsonl", json!({"composerId":raw}));
    let late = crate::session_migration::discovery::file_candidate(
        Provider::Cursor,
        &Path::new(&reference.root_path).join("late.jsonl"),
        Path::new(&reference.root_path),
    )
    .unwrap();
    super::add(&conn, &done.scan_id, &source, late).unwrap();
    let refreshed = groups(&done, None, 1, None);
    assert_eq!(refreshed.groups[0].group_id, first.groups[0].group_id);
    assert_eq!(
        refreshed.groups[0].first_member.candidate_id,
        reference.candidate.candidate_id
    );
    assert_eq!(
        (refreshed.total_count, refreshed.groups[0].member_count),
        (2, 2)
    );
    let second = groups(&done, first.next_cursor, 1, None);
    assert_eq!(second.groups.len(), 1);
    assert_ne!(second.groups[0].group_id, refreshed.groups[0].group_id);
    let still_fixed =
        scan_selection::page(&done.scan_id, 1, &selection.selection_id, None, None).unwrap();
    assert_eq!(fixed, still_fixed);
    assert_eq!(
        serde_json::to_value(scan::candidate(&reference.candidate.candidate_id).unwrap()).unwrap(),
        original
    );
}

#[test]
fn other_providers_keep_each_artifact_separate_and_group_page_bytes_are_explicitly_bounded() {
    let home = IsolatedAppHome::acquire("group-other-budget");
    let root = home.path.join("generic");
    std::fs::create_dir_all(&root).unwrap();
    for index in 0..2 {
        std::fs::write(
            root.join(format!("{index}.json")),
            r#"[{"role":"user","content":"SAME_BODY"}]"#,
        )
        .unwrap();
    }
    let done = scan::scan_start(vec![(
        Provider::GenericFile,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    let page = groups(&done, None, 200, None);
    assert_eq!(page.total_count, 2);
    assert!(page
        .groups
        .iter()
        .all(|g| g.member_count == 1 && g.proof == "artifact"));
    let candidate = &page.groups[0].first_member;
    let conn = scan::open_scan(&done.scan_id, 1).unwrap();
    let mut reference = scan::candidate(&candidate.candidate_id).unwrap();
    reference.candidate.title = "x".repeat(crate::session_migration::budget::READ_PAGE_TEXT_BYTES);
    conn.execute(
        "UPDATE candidates SET json=?1 WHERE candidate_id=?2",
        rusqlite::params![
            serde_json::to_string(&reference).unwrap(),
            candidate.candidate_id
        ],
    )
    .unwrap();
    assert_eq!(
        scan_groups::page(&done.scan_id, 1, None, None, &Default::default())
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    assert_eq!(
        scan_groups::members(&done.scan_id, 1, &page.groups[0].group_id, None, None)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
}
