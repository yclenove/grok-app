//! Formal scan contracts for choosing one fixed source member per group.
use crate::session_migration::{
    catalog, engine, errors::ErrorCode, group_selection as selection, scan, scan_groups,
    test_home::IsolatedAppHome, types::Provider,
};
use serde_json::json;

#[path = "group_selection_pipeline_tests.rs"]
mod pipeline;

#[path = "scan_group_member_tests.rs"]
mod exact_member;

struct Fixture {
    _home: IsolatedAppHome,
    root: std::path::PathBuf,
    scan: scan::ScanState,
    group: scan_groups::SourceGroup,
    other: scan_groups::SourceGroup,
    members: Vec<scan_groups::SourceGroupMember>,
}

impl Fixture {
    fn new() -> Self {
        let home = IsolatedAppHome::acquire("fixed-group-choice");
        engine::reset_runtime();
        let root = home.path.join("cursor");
        std::fs::create_dir_all(&root).unwrap();
        for (name, id, body) in [
            ("a.jsonl", "same", "MEMBER_A"),
            ("b.jsonl", "same", "MEMBER_B"),
            ("other.jsonl", "other", "MEMBER_OTHER"),
        ] {
            std::fs::write(
                root.join(name),
                format!(
                    "{}\n",
                    json!({"composerId":id,"role":"user","content":body})
                ),
            )
            .unwrap();
        }
        let scan = scan::scan_start(vec![(
            Provider::Cursor,
            root.to_string_lossy().into_owned(),
        )])
        .unwrap();
        assert_eq!((scan.discovered_count, scan.issue_count), (3, 0));
        let page =
            scan_groups::page(&scan.scan_id, 1, None, Some(10), &Default::default()).unwrap();
        assert_eq!(page.total_count, 2);
        let group = page
            .groups
            .iter()
            .find(|g| g.member_count == 2)
            .unwrap()
            .clone();
        let other = page
            .groups
            .iter()
            .find(|g| g.member_count == 1)
            .unwrap()
            .clone();
        let members = scan_groups::members(&scan.scan_id, 1, &group.group_id, None, Some(10))
            .unwrap()
            .members;
        Self {
            _home: home,
            root,
            scan,
            group,
            other,
            members,
        }
    }

    fn get(&self) -> selection::GroupSelection {
        tauri::async_runtime::block_on(crate::commands::session_import_group_selection_get(
            self.scan.scan_id.clone(),
            1,
        ))
        .unwrap()
    }

    fn freeze(&self) -> selection::GroupSelection {
        let initial = self.get();
        tauri::async_runtime::block_on(crate::commands::session_import_group_selection_freeze(
            self.scan.scan_id.clone(),
            1,
            initial.selection_id,
            initial.state_version,
        ))
        .unwrap()
    }

    fn choice(&self, s: &selection::GroupSelection, group: &str) -> selection::GroupChoice {
        let mut page =
            selection::membership(&self.scan.scan_id, 1, &s.selection_id, &[group.into()]).unwrap();
        assert_eq!(page.state_version, s.state_version);
        assert_eq!(page.choices.len(), 1);
        page.choices.remove(0)
    }

    fn choose_second(&self, s: &selection::GroupSelection) -> selection::GroupMemberChange {
        tauri::async_runtime::block_on(crate::commands::session_import_group_selection_choose(
            self.scan.scan_id.clone(),
            1,
            s.selection_id.clone(),
            s.state_version,
            self.group.group_id.clone(),
            self.members[1].candidate.candidate_id.clone(),
        ))
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        engine::reset_runtime();
    }
}

#[test]
fn new_scan_initializes_a_separate_group_selector_without_changing_candidate_selection() {
    let home = IsolatedAppHome::acquire("group-selection-init");
    engine::reset_runtime();
    let root = home.path.join("cursor");
    std::fs::create_dir_all(&root).unwrap();
    for name in ["a.jsonl", "b.jsonl"] {
        std::fs::write(
            root.join(name),
            format!(
                "{}\n",
                json!({"composerId":"one-group", "role":"user", "content":name})
            ),
        )
        .unwrap();
    }
    let done = scan::scan_start(vec![(
        Provider::Cursor,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    assert_eq!(done.discovered_count, 2);
    assert_eq!(done.selection.selected_count, 0);
    assert_eq!(
        done.selection.mode,
        crate::session_migration::scan_selection::CandidateSelectionMode::Explicit
    );
    let conn = scan::open_scan_readonly(&done.scan_id, done.epoch).unwrap();
    let initialized: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='group_selections')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        initialized,
        "formal scan groups need their own durable member selector, not raw candidate freeze"
    );
}

#[test]
fn registered_group_commands_freeze_one_member_and_keep_the_candidate_selector_independent() {
    let f = Fixture::new();
    let initial = f.get();
    assert_eq!(initial, f.get());
    assert_ne!(initial.selection_id, f.scan.selection.selection_id);
    assert_eq!(
        (
            initial.state_version,
            initial.selected_count,
            initial.choice_count
        ),
        (1, 0, 0)
    );
    let frozen = f.freeze();
    assert_eq!(
        (
            frozen.state_version,
            frozen.selected_count,
            frozen.choice_count
        ),
        (2, 2, 2)
    );
    let page =
        tauri::async_runtime::block_on(crate::commands::session_import_group_selection_page(
            f.scan.scan_id.clone(),
            1,
            frozen.selection_id.clone(),
            None,
            Some(10),
            None,
        ))
        .unwrap();
    assert_eq!(page.choices.len(), 2);
    assert_eq!(
        f.choice(&frozen, &f.group.group_id).candidate,
        f.group.first_member
    );
    assert_eq!(
        crate::session_migration::scan_selection::get(&f.scan.scan_id, 1).unwrap(),
        f.scan.selection
    );
    assert_eq!(
        selection::freeze_current(
            &f.scan.scan_id,
            1,
            &frozen.selection_id,
            frozen.state_version
        )
        .unwrap(),
        frozen
    );
    engine::reset_runtime();
    assert_eq!(f.get(), frozen);
}

#[test]
fn deselect_reselect_remembers_the_preferred_member_and_clear_resets_it() {
    let f = Fixture::new();
    let frozen = f.freeze();
    let changed = f.choose_second(&frozen);
    assert_eq!(
        changed.previous_candidate_id.as_deref(),
        Some(f.members[0].candidate.candidate_id.as_str())
    );
    assert!(changed.selected);
    assert_eq!(changed.selection.selected_count, 2);
    let s = selection::set_many(
        &f.scan.scan_id,
        1,
        &frozen.selection_id,
        changed.selection.state_version,
        std::slice::from_ref(&f.group.group_id),
        false,
    )
    .unwrap();
    assert_eq!(s.selected_count, 1);
    let choice = f.choice(&s, &f.group.group_id);
    assert!(!choice.selected);
    assert_eq!(choice.candidate.candidate_id, changed.candidate_id);
    let s =
        selection::freeze_current(&f.scan.scan_id, 1, &s.selection_id, s.state_version).unwrap();
    assert_eq!(s.selected_count, 2);
    assert_eq!(
        f.choice(&s, &f.group.group_id).candidate.candidate_id,
        changed.candidate_id
    );
    let s = selection::clear(&f.scan.scan_id, 1, &s.selection_id, s.state_version).unwrap();
    assert_eq!((s.selected_count, s.choice_count), (0, 0));
    assert_eq!(
        selection::clear(&f.scan.scan_id, 1, &s.selection_id, s.state_version).unwrap(),
        s
    );
    let s =
        selection::freeze_current(&f.scan.scan_id, 1, &s.selection_id, s.state_version).unwrap();
    assert_eq!(
        f.choice(&s, &f.group.group_id).candidate.candidate_id,
        f.members[0].candidate.candidate_id
    );
}

#[test]
fn member_choice_does_not_select_an_unselected_group_and_exact_noops_do_not_advance() {
    let f = Fixture::new();
    let empty = f.get();
    let chosen = f.choose_second(&empty);
    assert!(!chosen.selected);
    assert_eq!(
        (
            chosen.selection.selected_count,
            chosen.selection.choice_count
        ),
        (0, 1)
    );
    assert_eq!(
        f.choose_second(&chosen.selection).selection,
        chosen.selection
    );
    let no_op = selection::set_many(
        &f.scan.scan_id,
        1,
        &empty.selection_id,
        chosen.selection.state_version,
        std::slice::from_ref(&f.group.group_id),
        false,
    )
    .unwrap();
    assert_eq!(no_op, chosen.selection);
    let selected = selection::invert_many(
        &f.scan.scan_id,
        1,
        &empty.selection_id,
        no_op.state_version,
        std::slice::from_ref(&f.group.group_id),
    )
    .unwrap();
    assert_eq!(selected.selected_count, 1);
    assert_eq!(
        f.choice(&selected, &f.group.group_id)
            .candidate
            .candidate_id,
        chosen.candidate_id
    );
}

#[test]
fn bounded_mutations_reject_invalid_owners_duplicates_and_partial_write_errors_atomically() {
    let f = Fixture::new();
    let frozen = f.freeze();
    for groups in [
        vec![f.group.group_id.clone(), "foreign-group".into()],
        vec![f.group.group_id.clone(); 2],
        vec![f.group.group_id.clone(); 201],
    ] {
        assert!(selection::set_many(
            &f.scan.scan_id,
            1,
            &frozen.selection_id,
            frozen.state_version,
            &groups,
            false
        )
        .is_err());
        assert_eq!(f.get(), frozen);
        assert!(f.choice(&frozen, &f.group.group_id).selected);
    }
    assert_eq!(
        selection::clear(
            &f.scan.scan_id,
            1,
            &f.scan.selection.selection_id,
            frozen.state_version
        )
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        selection::clear(
            &f.scan.scan_id,
            2,
            &frozen.selection_id,
            frozen.state_version
        )
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    assert!(selection::choose_member(
        &f.scan.scan_id,
        1,
        &frozen.selection_id,
        frozen.state_version,
        &f.group.group_id,
        &f.other.first_member.candidate_id
    )
    .is_err());
    let other_scan = scan::scan_start(vec![(
        Provider::Cursor,
        f.root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    assert!(selection::choose_member(
        &f.scan.scan_id,
        1,
        &frozen.selection_id,
        frozen.state_version,
        &f.group.group_id,
        &other_scan.candidates[0].candidate_id
    )
    .is_err());
    assert_eq!(f.get(), frozen);
    let descriptor = serde_json::to_value(&frozen).unwrap();
    let mut unknown = descriptor.clone();
    unknown["unrecognized"] = json!(true);
    assert!(serde_json::from_value::<selection::GroupSelection>(unknown).is_err());
    for field in [
        "version",
        "domain",
        "scanId",
        "epoch",
        "stateVersion",
        "selectedCount",
        "choiceCount",
    ] {
        let mut missing = descriptor.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<selection::GroupSelection>(missing).is_err());
    }
}

#[test]
fn concurrent_member_and_selection_edits_have_one_compare_and_swap_winner() {
    let f = Fixture::new();
    let frozen = f.freeze();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for choose in [false, true] {
        let barrier = barrier.clone();
        let s = frozen.clone();
        let group = f.group.group_id.clone();
        let candidate = f.members[1].candidate.candidate_id.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            if choose {
                selection::choose_member(
                    &s.scan_id,
                    s.epoch,
                    &s.selection_id,
                    s.state_version,
                    &group,
                    &candidate,
                )
                .map(|change| change.selection)
            } else {
                selection::set_many(
                    &s.scan_id,
                    s.epoch,
                    &s.selection_id,
                    s.state_version,
                    &[group],
                    false,
                )
            }
        }));
    }
    barrier.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| result
                .as_ref()
                .is_err_and(|e| e.code == ErrorCode::PlanStale))
            .count(),
        1
    );
    assert_eq!(f.get().state_version, frozen.state_version + 1);
}

#[test]
fn a_valid_different_member_pin_cannot_replace_the_users_signed_choice() {
    let f = Fixture::new();
    let frozen = f.freeze();
    let before = f.choice(&frozen, &f.group.group_id);
    let b = scan::candidate(&f.members[1].candidate.candidate_id).unwrap();
    crate::session_migration::source_member::validate_reference(&b).unwrap();
    assert_eq!(b.source_member.as_ref().unwrap().group_id, f.group.group_id);
    let conn = scan::open_scan_group_writer(&f.scan.scan_id, 1).unwrap();
    let raw: String = conn
        .query_row(
            "SELECT json FROM group_selection_choices WHERE group_id=?1",
            [&f.group.group_id],
            |row| row.get(0),
        )
        .unwrap();
    let mut changed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    changed["reference"] = serde_json::to_value(&b).unwrap();
    conn.execute(
        "UPDATE group_selection_choices SET candidate_id=?2,json=?3 WHERE group_id=?1",
        rusqlite::params![
            f.group.group_id,
            b.candidate.candidate_id,
            serde_json::to_string(&changed).unwrap()
        ],
    )
    .unwrap();
    assert_eq!(
        selection::get(&f.scan.scan_id, 1).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert!(selection::page(&f.scan.scan_id, 1, &frozen.selection_id, None, None, true).is_err());
    assert!(crate::session_migration::prepare::start(
        &f.scan.scan_id,
        1,
        &frozen.selection_id,
        vec![]
    )
    .is_err());
    assert!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .is_empty()
    );
    conn.execute(
        "UPDATE group_selection_choices SET candidate_id=?2,json=?3 WHERE group_id=?1",
        rusqlite::params![f.group.group_id, before.candidate.candidate_id, raw],
    )
    .unwrap();
    assert_eq!(f.get(), frozen);
}

#[test]
fn deletion_replay_flags_missing_pins_and_unknown_fields_cannot_shrink_or_change_a_capture() {
    let f = Fixture::new();
    let frozen = f.freeze();
    let conn = scan::open_scan_group_writer(&f.scan.scan_id, 1).unwrap();
    let original: String = conn
        .query_row(
            "SELECT json FROM group_selection_choices WHERE group_id=?1",
            [&f.group.group_id],
            |row| row.get(0),
        )
        .unwrap();
    let original_value: serde_json::Value = serde_json::from_str(&original).unwrap();
    let changed = f.choose_second(&frozen);
    let b_raw: String = conn
        .query_row(
            "SELECT json FROM group_selection_choices WHERE group_id=?1",
            [&f.group.group_id],
            |row| row.get(0),
        )
        .unwrap();
    let a = selection::choose_member(
        &f.scan.scan_id,
        1,
        &frozen.selection_id,
        changed.selection.state_version,
        &f.group.group_id,
        &f.members[0].candidate.candidate_id,
    )
    .unwrap();
    let current: String = conn
        .query_row(
            "SELECT json FROM group_selection_choices WHERE group_id=?1",
            [&f.group.group_id],
            |row| row.get(0),
        )
        .unwrap();
    let mut variants = vec![
        serde_json::from_str::<serde_json::Value>(&b_raw).unwrap(),
        original_value,
    ];
    let value: serde_json::Value = serde_json::from_str(&current).unwrap();
    for mutation in [
        "missing-pin",
        "null-pin",
        "unknown",
        "high-version",
        "flag",
        "generation",
        "seal",
    ] {
        let mut v = value.clone();
        match mutation {
            "missing-pin" => {
                v["reference"]
                    .as_object_mut()
                    .unwrap()
                    .remove("sourceMember");
            }
            "null-pin" => v["reference"]["sourceMember"] = serde_json::Value::Null,
            "unknown" => v["reference"]["candidate"]["unknown"] = json!(true),
            "high-version" => v["version"] = json!(99),
            "flag" => v["selected"] = json!(false),
            "generation" => v["choiceVersion"] = json!(99),
            "seal" => v["seal"] = json!("00".repeat(32)),
            _ => unreachable!(),
        }
        variants.push(v);
    }
    for variant in variants {
        conn.execute(
            "UPDATE group_selection_choices SET json=?2 WHERE group_id=?1",
            rusqlite::params![f.group.group_id, serde_json::to_string(&variant).unwrap()],
        )
        .unwrap();
        assert!(selection::get(&f.scan.scan_id, 1).is_err());
        conn.execute(
            "UPDATE group_selection_choices SET json=?2 WHERE group_id=?1",
            rusqlite::params![f.group.group_id, current],
        )
        .unwrap();
        assert_eq!(f.get(), a.selection);
    }
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute(
        "DELETE FROM group_selection_choices WHERE group_id=?1",
        [&f.group.group_id],
    )
    .unwrap();
    drop(tx);
    assert_eq!(f.get(), a.selection);
    conn.execute(
        "UPDATE group_selection_choices SET selected=0 WHERE group_id=?1",
        [&f.group.group_id],
    )
    .unwrap();
    assert!(selection::get(&f.scan.scan_id, 1).is_err());
    conn.execute(
        "UPDATE group_selection_choices SET selected=1 WHERE group_id=?1",
        [&f.group.group_id],
    )
    .unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute(
        "DELETE FROM group_selection_choices WHERE group_id=?1",
        [&f.group.group_id],
    )
    .unwrap();
    // The same production load boundary is exercised against an uncommitted
    // deletion through a separate read, which must continue seeing old state.
    assert_eq!(f.get(), a.selection);
    tx.commit().unwrap();
    assert!(
        selection::get(&f.scan.scan_id, 1).is_err(),
        "committed deletion must not silently reduce the batch"
    );
}

fn schema(conn: &rusqlite::Connection) -> Vec<(String, Option<String>)> {
    conn.prepare("SELECT name,sql FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

#[test]
fn old_candidate_scans_keep_their_freeze_semantics_and_group_reads_do_not_ddl() {
    let f = Fixture::new();
    let conn = scan::open_scan_group_writer(&f.scan.scan_id, 1).unwrap();
    conn.execute_batch("DROP TABLE group_selection_choices; DROP TABLE group_selections; ALTER TABLE meta DROP COLUMN group_selection_schema_version;").unwrap();
    let original = schema(&conn);
    assert_eq!(
        selection::get(&f.scan.scan_id, 1).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert!(
        selection::freeze_current(&f.scan.scan_id, 1, &uuid::Uuid::new_v4().to_string(), 1)
            .is_err()
    );
    assert_eq!(schema(&conn), original);
    let candidate =
        crate::session_migration::scan_selection::freeze_current(&f.scan.scan_id, 1).unwrap();
    assert_eq!(
        candidate.selected_count, 3,
        "legacy candidate mode still includes both raw members"
    );
    assert_eq!(
        crate::session_migration::scan_group_store::SCHEMA_VERSION,
        1
    );
    assert_eq!(schema(&conn), original);
}

#[test]
fn marked_missing_or_newer_group_selection_schema_is_not_repaired() {
    let f = Fixture::new();
    let conn = scan::open_scan_group_writer(&f.scan.scan_id, 1).unwrap();
    conn.execute("UPDATE meta SET group_selection_schema_version=2", [])
        .unwrap();
    let before = schema(&conn);
    assert_eq!(
        selection::get(&f.scan.scan_id, 1).unwrap_err().code,
        ErrorCode::ImportSchemaTooNew
    );
    assert_eq!(schema(&conn), before);
    conn.execute("UPDATE meta SET group_selection_schema_version=1", [])
        .unwrap();
    conn.execute_batch("DROP TABLE group_selection_choices")
        .unwrap();
    let before = schema(&conn);
    assert_eq!(
        selection::get(&f.scan.scan_id, 1).unwrap_err().code,
        ErrorCode::SchemaUnsupported
    );
    assert_eq!(schema(&conn), before);
}
