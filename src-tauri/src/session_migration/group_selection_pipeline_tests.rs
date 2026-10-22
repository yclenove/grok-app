//! Real prepare/plan/publish/recovery boundaries, not a grouping helper demo.
use super::*;
use crate::session_migration::{
    cursor::{decode_bound_cursor, encode_bound_cursor_at, CursorScope},
    job_control,
    jobs::JobState,
    prepare,
};
use std::time::{Duration, Instant};

fn wait_terminal(id: &str) -> prepare::PrepareBatchStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = prepare::status(id).unwrap();
        if !matches!(
            status.state,
            prepare::PrepareBatchState::Queued
                | prepare::PrepareBatchState::Preparing
                | prepare::PrepareBatchState::Cancelling
        ) && !prepare::running_for_test(id)
        {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "owned prepare did not finish: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn linked_only(f: &Fixture) -> selection::GroupSelection {
    let frozen = f.freeze();
    selection::set_many(
        &f.scan.scan_id,
        1,
        &frozen.selection_id,
        frozen.state_version,
        std::slice::from_ref(&f.other.group_id),
        false,
    )
    .unwrap()
}

fn prepare_one(f: &Fixture, s: &selection::GroupSelection) -> prepare::PrepareBatchStatus {
    let started = prepare::start(&f.scan.scan_id, 1, &s.selection_id, vec![]).unwrap();
    let done = wait_terminal(&started.batch_id);
    assert_eq!(
        (done.state, done.total, done.prepared, done.failed),
        (prepare::PrepareBatchState::Completed, 1, 1, 0)
    );
    done
}

fn batch_reference(id: &str) -> scan::CandidateRef {
    let conn = prepare::open_existing(id).unwrap();
    let raw: String = conn
        .query_row("SELECT input_json FROM items WHERE seq=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn fixed_groups_prepare_only_one_member_and_existing_batches_plans_and_revisions_stay_immutable() {
    let f = Fixture::new();
    let source_bytes: Vec<_> = ["a.jsonl", "b.jsonl", "other.jsonl"]
        .iter()
        .map(|name| (*name, std::fs::read(f.root.join(name)).unwrap()))
        .collect();
    let a = linked_only(&f);
    let batch_a = prepare_one(&f, &a);
    let reference_a = batch_reference(&batch_a.batch_id);
    assert_eq!(
        reference_a.candidate.candidate_id,
        f.members[0].candidate.candidate_id
    );
    let b = f.choose_second(&a).selection;
    assert_eq!(
        batch_reference(&batch_a.batch_id).source_member,
        reference_a.source_member
    );
    assert_eq!(
        batch_reference(&batch_a.batch_id).candidate,
        reference_a.candidate
    );
    let batch_b = prepare_one(&f, &b);
    let reference_b = batch_reference(&batch_b.batch_id);
    assert_eq!(
        reference_b.candidate.candidate_id,
        f.members[1].candidate.candidate_id
    );
    assert_ne!(reference_a.source_member, reference_b.source_member);
    engine::reset_runtime();
    let plan_a = engine::plan_from_validated_prepare(
        prepare::validated_plan_inputs(&batch_a.batch_id).unwrap(),
        prepare::mappings(&batch_a.batch_id, None).unwrap(),
    )
    .unwrap();
    let plan_b = engine::plan_from_validated_prepare(
        prepare::validated_plan_inputs(&batch_b.batch_id).unwrap(),
        prepare::mappings(&batch_b.batch_id, None).unwrap(),
    )
    .unwrap();
    let offline = f.root.with_file_name("offline cursor");
    std::fs::rename(&f.root, &offline).unwrap();
    engine::reset_runtime();
    assert_eq!(f.get(), b);
    let mut entries = Vec::new();
    let mut archived = Vec::new();
    for (plan, reference) in [(plan_a, reference_a), (plan_b, reference_b)] {
        let job = engine::commit(
            &plan.plan_id,
            &plan.plan_hash,
            &uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        let done = engine::job_get(&job.job_id).unwrap();
        assert_eq!((done.state, done.item_count), (JobState::Completed, 1));
        let item = &done.items[0];
        assert_eq!(item.candidate_id, reference.candidate.candidate_id);
        let import_id = item.import_id.as_deref().unwrap();
        let revision = item.revision_id.as_deref().unwrap();
        let archive = catalog::revision_dir(import_id, revision).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(archive.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(
            manifest["sourceMember"],
            serde_json::to_value(&reference.source_member).unwrap()
        );
        engine::reset_runtime();
        let (events, next, total) =
            engine::events_page(import_id, Some(revision), None, Some(10), None).unwrap();
        assert_eq!((events.len(), total), (1, 1));
        assert!(next.is_none());
        assert_eq!(events[0]["kind"], "message");
        assert_eq!(events[0]["role"], "user");
        assert_eq!(events[0]["complete"], true);
        assert_eq!(
            events[0]["blocks"],
            json!([{"kind":"text", "text":reference.candidate.title}])
        );
        entries.push((import_id.to_string(), revision.to_string()));
        archived.push((
            archive.join("manifest.json"),
            std::fs::read(archive.join("manifest.json")).unwrap(),
        ));
    }
    assert_eq!(entries[0].0, entries[1].0);
    assert_ne!(entries[0].1, entries[1].1);
    assert_eq!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .len(),
        1
    );
    for (path, bytes) in archived {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    for (name, bytes) in source_bytes {
        assert_eq!(std::fs::read(offline.join(name)).unwrap(), bytes);
    }
}

#[test]
fn online_source_change_fails_the_fixed_member_instead_of_substituting_another_version() {
    let f = Fixture::new();
    let s = linked_only(&f);
    let path = std::path::Path::new(&f.members[0].candidate.locator);
    assert!(path.is_file());
    let original = std::fs::read(path).unwrap();
    std::fs::write(
        path,
        format!(
            "{}\n",
            json!({"composerId":"same","role":"user","content":"CHANGED_SOURCE_BODY"})
        ),
    )
    .unwrap();
    let started = prepare::start(&f.scan.scan_id, 1, &s.selection_id, vec![]).unwrap();
    let failed = wait_terminal(&started.batch_id);
    assert_eq!(
        (failed.state, failed.total, failed.failed),
        (prepare::PrepareBatchState::Failed, 1, 1)
    );
    let items = prepare::items_page(&started.batch_id, None, Some(10)).unwrap();
    assert_eq!(
        items.items[0].candidate_id,
        f.members[0].candidate.candidate_id
    );
    assert_eq!(
        items.items[0].error.as_ref().unwrap().code,
        ErrorCode::SourceChanged
    );
    assert!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .is_empty()
    );
    std::fs::write(path, original).unwrap();
    engine::reset_runtime();
    prepare::retry(&started.batch_id).unwrap();
    assert_eq!(
        wait_terminal(&started.batch_id).state,
        prepare::PrepareBatchState::Completed
    );
    assert_eq!(
        batch_reference(&started.batch_id).candidate.candidate_id,
        f.members[0].candidate.candidate_id
    );
}

#[test]
fn actual_paused_discovery_adds_late_members_and_groups_without_changing_the_capture_or_cursor() {
    let home = IsolatedAppHome::acquire("group-capture-late-scan");
    engine::reset_runtime();
    let roots: Vec<_> = [
        "primary",
        "other captured group",
        "late member",
        "new group",
    ]
    .iter()
    .map(|name| home.path.join(name))
    .collect();
    for (index, root) in roots.iter().enumerate() {
        std::fs::create_dir_all(root).unwrap();
        let native = match index {
            0 | 2 => "same",
            1 => "other-captured-group",
            _ => "late-new-group",
        };
        std::fs::write(
            root.join("one.jsonl"),
            format!(
                "{}\n",
                json!({"composerId":native,"role":"user","content":format!("LATE_MEMBER_{index}")})
            ),
        )
        .unwrap();
    }
    let conn = catalog::open_catalog().unwrap();
    let primary = crate::session_migration::sources::register(
        &conn,
        Provider::Cursor,
        &roots[0],
        None,
        "synthetic",
    )
    .unwrap();
    let mut requests = vec![scan::ScanSourcePath {
        provider: "cursor".into(),
        path: roots[0].to_string_lossy().into_owned(),
        source_id: Some(primary.source_id.clone()),
        location_id: None,
        host: None,
        profile: String::new(),
    }];
    for root in &roots[1..] {
        let location = crate::session_migration::source_locations::associate(
            &conn,
            &crate::session_migration::source_locations::SourceAssociationRequest {
                source_id: primary.source_id.clone(),
                provider: Provider::Cursor,
                host: "local".into(),
                path: root.to_string_lossy().into_owned(),
                expected_primary_root: primary.root_path.clone(),
            },
        )
        .unwrap();
        requests.push(scan::ScanSourcePath {
            provider: "cursor".into(),
            path: root.to_string_lossy().into_owned(),
            source_id: Some(primary.source_id.clone()),
            location_id: Some(location.location_id),
            host: Some("local".into()),
            profile: String::new(),
        });
    }
    let started = scan::begin(requests).unwrap();
    for _ in 0..20 {
        super::super::run_slice(&started.scan_id, 1).unwrap();
        let count = scan::state(&started.scan_id, 1).unwrap().discovered_count;
        assert!(count <= 2);
        if count == 2 {
            break;
        }
    }
    let paused = scan::state(&started.scan_id, 1).unwrap();
    assert_eq!(
        (paused.state.as_str(), paused.discovered_count),
        ("paused", 2)
    );
    let initial = selection::get(&started.scan_id, 1).unwrap();
    let captured = selection::freeze_current(
        &started.scan_id,
        1,
        &initial.selection_id,
        initial.state_version,
    )
    .unwrap();
    assert_eq!(captured.selected_count, 2);
    let before = selection::page(
        &started.scan_id,
        1,
        &captured.selection_id,
        None,
        Some(1),
        true,
    )
    .unwrap();
    let old_tail = selection::page(
        &started.scan_id,
        1,
        &captured.selection_id,
        before.next_cursor.as_deref(),
        Some(1),
        true,
    )
    .unwrap();
    assert!(before.next_cursor.is_some());
    assert!(old_tail.next_cursor.is_none());
    let snapshot = format!("1:fixed-groups-v1:{}", captured.state_version);
    let scope = CursorScope {
        owner: &captured.selection_id,
        snapshot: &snapshot,
        leaf: &started.scan_id,
        filter: "selected-fixed-members",
    };
    let original_cursor = before.next_cursor.as_deref().unwrap();
    let original_claims = decode_bound_cursor(original_cursor, scope).unwrap();
    // Reissuing a signed cursor renews its wall-clock expiry. Model the adjacent
    // second deterministically instead of relying on how long discovery takes.
    let earlier_cursor = encode_bound_cursor_at(
        scope,
        original_claims.offset,
        original_claims.mark.clone(),
        original_claims.exp - 1,
    )
    .unwrap();
    assert_ne!(earlier_cursor, original_cursor);
    scan::run(&started.scan_id).unwrap();
    let done = scan::state(&started.scan_id, 1).unwrap();
    assert_eq!(done.discovered_count, 4);
    assert_eq!(selection::get(&started.scan_id, 1).unwrap(), captured);
    let after = selection::page(
        &started.scan_id,
        1,
        &captured.selection_id,
        None,
        Some(1),
        true,
    )
    .unwrap();
    assert_eq!(after.selection, before.selection);
    assert_eq!(
        serde_json::to_value(&after.choices).unwrap(),
        serde_json::to_value(&before.choices).unwrap()
    );
    let renewed_cursor = after.next_cursor.as_deref().unwrap();
    let renewed_claims = decode_bound_cursor(renewed_cursor, scope).unwrap();
    let mut expected_claims = original_claims;
    expected_claims.exp = renewed_claims.exp;
    // Owner, version, snapshot, leaf, filter, offset and mark must still match.
    // Both tokens have independently passed HMAC and expiry verification above.
    assert_eq!(renewed_claims, expected_claims);
    for cursor in [original_cursor, earlier_cursor.as_str(), renewed_cursor] {
        assert_eq!(
            serde_json::to_value(
                selection::page(
                    &started.scan_id,
                    1,
                    &captured.selection_id,
                    Some(cursor),
                    Some(1),
                    true
                )
                .unwrap()
            )
            .unwrap(),
            serde_json::to_value(&old_tail).unwrap()
        );
    }
    let groups =
        scan_groups::page(&started.scan_id, 1, None, Some(10), &Default::default()).unwrap();
    assert_eq!(groups.total_count, 3);
    let original = groups
        .groups
        .iter()
        .find(|group| group.member_count == 2)
        .unwrap();
    assert_eq!(
        before.choices[0].candidate.candidate_id,
        original.first_member.candidate_id
    );
    let refreshed = selection::freeze_current(
        &started.scan_id,
        1,
        &captured.selection_id,
        captured.state_version,
    )
    .unwrap();
    assert_eq!(refreshed.selected_count, 3);
    assert_eq!(
        selection::membership(
            &started.scan_id,
            1,
            &refreshed.selection_id,
            std::slice::from_ref(&original.group_id)
        )
        .unwrap()
        .choices[0]
            .candidate
            .candidate_id,
        before.choices[0].candidate.candidate_id
    );
}

#[test]
fn fixed_choice_cursors_bind_domain_scan_version_and_filter() {
    let f = Fixture::new();
    let s = f.freeze();
    let first = selection::page(&f.scan.scan_id, 1, &s.selection_id, None, Some(1), true).unwrap();
    let cursor = first.next_cursor.as_deref().unwrap();
    assert_eq!(
        selection::page(
            &f.scan.scan_id,
            1,
            &s.selection_id,
            Some(cursor),
            Some(1),
            false
        )
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    assert!(selection::page(
        &f.scan.scan_id,
        1,
        &f.scan.selection.selection_id,
        Some(cursor),
        None,
        true
    )
    .is_err());
    let mut corrupt = cursor.as_bytes().to_vec();
    let last = corrupt.len() - 1;
    corrupt[last] = if corrupt[last] == b'A' { b'B' } else { b'A' };
    assert!(selection::page(
        &f.scan.scan_id,
        1,
        &s.selection_id,
        Some(std::str::from_utf8(&corrupt).unwrap()),
        None,
        true
    )
    .is_err());
    let changed = f.choose_second(&s).selection;
    assert_eq!(
        selection::page(
            &f.scan.scan_id,
            1,
            &s.selection_id,
            Some(cursor),
            Some(1),
            true
        )
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        f.choice(&changed, &f.other.group_id).candidate,
        f.other.first_member
    );
}

#[test]
fn ten_thousand_metadata_groups_have_bounded_fixed_pages_with_no_available_source_bodies() {
    let home = IsolatedAppHome::acquire("group-metadata-10k");
    engine::reset_runtime();
    let root = home.path.join("empty synthetic source");
    std::fs::create_dir_all(&root).unwrap();
    let registration = crate::session_migration::sources::register(
        &catalog::open_catalog().unwrap(),
        Provider::GenericFile,
        &root,
        None,
        "synthetic",
    )
    .unwrap();
    let started = scan::begin(vec![scan::ScanSourcePath {
        provider: "generic-file".into(),
        path: root.to_string_lossy().into_owned(),
        source_id: Some(registration.source_id.clone()),
        location_id: None,
        host: None,
        profile: String::new(),
    }])
    .unwrap();
    let conn = scan::open_scan_group_writer(&started.scan_id, 1).unwrap();
    for index in 0..10_000 {
        // Actual scan insertion and grouping, but deliberately no body file.
        // Any accidental normalization or source-body read must therefore fail.
        let candidate = crate::session_migration::types::SourceCandidate {
            candidate_id: format!("metadata-fixture:{index}"),
            provider: Provider::GenericFile,
            native_session_id: format!("metadata-{index}"),
            title: if index < 40 {
                format!("metadata-{index}-{}", "长元数据".repeat(1000))
            } else {
                format!("metadata-{index}")
            },
            cwd: None,
            started_at: None,
            updated_at: None,
            locator: root
                .join(format!("absent-{index}.json"))
                .to_string_lossy()
                .into_owned(),
            source_revision: format!("synthetic-metadata-{index}"),
            archived: None,
            completeness: "metadata-only".into(),
        };
        super::super::add(&conn, &started.scan_id, &registration, candidate).unwrap();
    }
    std::fs::rename(&root, root.with_file_name("offline empty source")).unwrap();
    let initial = selection::get(&started.scan_id, 1).unwrap();
    let watch = Instant::now();
    let captured = selection::freeze_current(
        &started.scan_id,
        1,
        &initial.selection_id,
        initial.state_version,
    )
    .unwrap();
    assert_eq!(
        (captured.selected_count, captured.choice_count),
        (10_000, 10_000)
    );
    let capture_ms = watch.elapsed().as_millis();
    let mut cursor = None;
    let mut found = std::collections::HashSet::new();
    let mut pages = 0;
    let mut largest = 0;
    loop {
        let page = selection::page(
            &started.scan_id,
            1,
            &captured.selection_id,
            cursor.as_deref(),
            Some(200),
            true,
        )
        .unwrap();
        assert!(page.choices.len() <= 200 && !page.choices.is_empty());
        let bytes = serde_json::to_vec(&page).unwrap().len();
        largest = largest.max(bytes);
        assert!(bytes <= crate::session_migration::budget::READ_PAGE_TEXT_BYTES);
        for choice in page.choices {
            assert!(choice.selected);
            assert!(found.insert(choice.candidate.candidate_id));
        }
        pages += 1;
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
        assert!(pages <= 10_000);
    }
    assert_eq!(found.len(), 10_000);
    assert!(
        pages > 50,
        "large metadata must hit the byte ceiling before the count ceiling"
    );
    assert!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .is_empty()
    );
    println!("GROUP_METADATA_10K capture_ms={capture_ms} total_ms={} pages={pages} max_wire_bytes={largest}", watch.elapsed().as_millis());
}

#[test]
fn changing_the_selector_after_the_first_real_prepare_page_aborts_without_a_mixed_batch() {
    let home = IsolatedAppHome::acquire("group-prepare-interleave");
    engine::reset_runtime();
    let root = home.path.join("metadata source");
    std::fs::create_dir_all(&root).unwrap();
    let registration = crate::session_migration::sources::register(
        &catalog::open_catalog().unwrap(),
        Provider::GenericFile,
        &root,
        None,
        "synthetic",
    )
    .unwrap();
    let started = scan::begin(vec![scan::ScanSourcePath {
        provider: "generic-file".into(),
        path: root.to_string_lossy().into_owned(),
        source_id: Some(registration.source_id.clone()),
        location_id: None,
        host: None,
        profile: String::new(),
    }])
    .unwrap();
    let conn = scan::open_scan_group_writer(&started.scan_id, 1).unwrap();
    for index in 0..201 {
        super::super::add(
            &conn,
            &started.scan_id,
            &registration,
            crate::session_migration::types::SourceCandidate {
                candidate_id: format!("interleave-fixture:{index}"),
                provider: Provider::GenericFile,
                native_session_id: format!("metadata-{index}"),
                title: format!("metadata-{index}"),
                cwd: None,
                started_at: None,
                updated_at: None,
                locator: root
                    .join(format!("absent-{index}.json"))
                    .to_string_lossy()
                    .into_owned(),
                source_revision: format!("synthetic-{index}"),
                archived: None,
                completeness: "metadata-only".into(),
            },
        )
        .unwrap();
    }
    let initial = selection::get(&started.scan_id, 1).unwrap();
    let s = selection::freeze_current(
        &started.scan_id,
        1,
        &initial.selection_id,
        initial.state_version,
    )
    .unwrap();
    let copy = s.clone();
    let observed = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let callback_count = observed.clone();
    let observer = crate::session_migration::prepare_selection::observe_pages_for_test(
        started.scan_id.clone(),
        move || {
            // This seam is AFTER a real page read, before the real input-copy loop.
            // It deterministically represents a user mutation between page reads.
            let page = callback_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if page == 0 {
                selection::clear(
                    &copy.scan_id,
                    copy.epoch,
                    &copy.selection_id,
                    copy.state_version,
                )
                .unwrap();
            }
        },
    );
    let error = prepare::start(&started.scan_id, 1, &s.selection_id, vec![]).unwrap_err();
    drop(observer);
    assert_eq!(error.code, ErrorCode::PlanStale);
    assert_eq!(observed.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        selection::get(&started.scan_id, 1).unwrap().selected_count,
        0
    );
    assert!(engine::recovery().unwrap().prepare.is_none());
    assert!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .is_empty()
    );
}
