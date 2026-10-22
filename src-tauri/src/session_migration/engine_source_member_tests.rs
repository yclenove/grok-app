//! Fixed source provenance must survive the real scan/preview boundaries.
use super::*;
use crate::session_migration::{
    catalog, scan, source_locations, sources, test_home::IsolatedAppHome,
};
use serde_json::{json, Value};

#[path = "engine_source_member_record_tests.rs"]
mod durable_records;

struct Fixture {
    _home: IsolatedAppHome,
    root: PathBuf,
    scan: scan::ScanState,
}

impl Fixture {
    fn new() -> Self {
        let home = IsolatedAppHome::acquire("fixed-source-member");
        reset_runtime();
        let primary = home.path.join("primary cursor");
        let root = home.path.join("associated cursor");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("selected.jsonl"),
            format!(
                "{}\n",
                json!({
                    "composerId":"fixed-member", "role":"user", "content":"FIXED_MEMBER_A"
                })
            ),
        )
        .unwrap();
        let conn = catalog::open_catalog().unwrap();
        let source =
            sources::register(&conn, Provider::Cursor, &primary, None, "synthetic").unwrap();
        let location = source_locations::associate(
            &conn,
            &source_locations::SourceAssociationRequest {
                source_id: source.source_id.clone(),
                provider: Provider::Cursor,
                host: "local".into(),
                path: root.to_string_lossy().into_owned(),
                expected_primary_root: source.root_path,
            },
        )
        .unwrap();
        let scan = scan::begin(vec![serde_json::from_value(json!({
            "provider":"cursor", "sourceId":source.source_id, "locationId":location.location_id,
            "host":"local", "path":root
        }))
        .unwrap()])
        .unwrap();
        scan::run(&scan.scan_id).unwrap();
        let scan = scan::state(&scan.scan_id, scan.epoch).unwrap();
        assert_eq!(scan.discovered_count, 1, "{scan:?}");
        Self {
            _home: home,
            root,
            scan,
        }
    }

    fn preview(&self) -> String {
        let c = &self.scan.candidates[0];
        preview(&c.candidate_id, Some(&c.source_revision))
            .unwrap()
            .0
    }

    fn rescan(&self) -> scan::ScanState {
        let scan = scan::begin(self.scan.source_paths.clone()).unwrap();
        scan::run(&scan.scan_id).unwrap();
        scan::state(&scan.scan_id, scan.epoch).unwrap()
    }
}

fn raw(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn associated_member_pin_is_immutable_scan_and_preview_provenance() {
    let f = Fixture::new();
    let reference = scan::candidate(&f.scan.candidates[0].candidate_id).unwrap();
    let encoded = serde_json::to_value(&reference).unwrap();
    let pin = &encoded["sourceMember"];
    assert!(
        pin.is_object(),
        "formal scan must pin the exact source member"
    );
    let preview_id = f.preview();
    let path = crate::session_migration::jobs::find_preview_dir(&preview_id)
        .unwrap()
        .unwrap();
    let lease = raw(&path.join("lease.json"));
    let manifest = raw(&path.join("manifest.json"));
    assert_eq!(&lease["sourceMember"], pin);
    assert_eq!(&manifest["sourceMember"], pin);
    assert_eq!(pin["version"], 1);
    assert_eq!(pin["reference"]["location"]["kind"], "associated");
    assert_eq!(
        pin["reference"]["sourceRevision"],
        reference.candidate.source_revision
    );
    let before = std::fs::read(f.root.join("selected.jsonl")).unwrap();
    std::fs::write(
        f.root.join("late.jsonl"),
        format!(
            "{}\n",
            json!({
                "composerId":"fixed-member", "role":"user", "content":"FIXED_MEMBER_B"
            })
        ),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(scan::candidate(&reference.candidate.candidate_id).unwrap()).unwrap(),
        encoded
    );
    assert_eq!(
        std::fs::read(f.root.join("selected.jsonl")).unwrap(),
        before
    );
    assert_eq!(raw(&path.join("lease.json")), lease);
}

#[test]
fn unsupported_source_member_lease_is_rejected_by_resident_and_disk_commit() {
    for reopened in [false, true] {
        let f = Fixture::new();
        let preview_id = f.preview();
        let plan = plan(vec![preview_id.clone()], vec![]).unwrap();
        let path = crate::session_migration::jobs::find_preview_dir(&preview_id)
            .unwrap()
            .unwrap();
        let mut lease = raw(&path.join("lease.json"));
        lease["sourceMember"] = json!({"version":99});
        std::fs::write(path.join("lease.json"), serde_json::to_vec(&lease).unwrap()).unwrap();
        if reopened {
            reset_runtime();
        }
        assert!(
            commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).is_err(),
            "reopened={reopened}: unknown member provenance must not be silently ignored"
        );
        assert!(list_entries(&open_catalog().unwrap()).unwrap().is_empty());
    }
}

#[test]
fn unsupported_source_member_lease_is_rejected_before_plan() {
    let f = Fixture::new();
    let preview_id = f.preview();
    let path = crate::session_migration::jobs::find_preview_dir(&preview_id)
        .unwrap()
        .unwrap();
    let mut lease = raw(&path.join("lease.json"));
    lease["sourceMember"] = json!({"version":99});
    std::fs::write(path.join("lease.json"), serde_json::to_vec(&lease).unwrap()).unwrap();
    assert!(
        plan(vec![preview_id], vec![]).is_err(),
        "unknown provenance must not become a plan"
    );
}

#[test]
fn pinned_preview_remains_publishable_and_readable_when_source_is_offline() {
    let f = Fixture::new();
    let preview_id = f.preview();
    let path = crate::session_migration::jobs::find_preview_dir(&preview_id)
        .unwrap()
        .unwrap();
    let pin = raw(&path.join("manifest.json"))["sourceMember"].clone();
    let plan = plan(vec![preview_id], vec![]).unwrap();
    reset_runtime();
    let offline = f.root.with_file_name("offline associated cursor");
    let before = std::fs::read(f.root.join("selected.jsonl")).unwrap();
    std::fs::rename(&f.root, &offline).unwrap();
    let queued = commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = job_get(&queued.job_id).unwrap();
    assert_eq!(job.state, JobState::Completed, "{job:?}");
    let item = &job.items[0];
    let import_id = item.import_id.as_deref().unwrap();
    let revision = item.revision_id.as_deref().unwrap();
    let archive = catalog::revision_dir(import_id, revision).unwrap();
    assert_eq!(raw(&archive.join("manifest.json"))["sourceMember"], pin);
    reset_runtime();
    let (events, _, total) = events_page(import_id, Some(revision), None, Some(20), None).unwrap();
    assert_eq!(total, 1);
    assert!(serde_json::to_string(&events)
        .unwrap()
        .contains("FIXED_MEMBER_A"));
    assert_eq!(
        std::fs::read(offline.join("selected.jsonl")).unwrap(),
        before
    );
}

#[test]
fn legacy_candidate_without_pin_retains_existing_preview_and_commit_contract() {
    let f = Fixture::new();
    let reference = scan::candidate(&f.scan.candidates[0].candidate_id).unwrap();
    let mut value = serde_json::to_value(reference).unwrap();
    value.as_object_mut().unwrap().remove("sourceMember");
    let legacy = serde_json::from_value(value).unwrap();
    let preview_id = preview_reference(legacy, None, None).unwrap().0;
    let path = crate::session_migration::jobs::find_preview_dir(&preview_id)
        .unwrap()
        .unwrap();
    assert!(raw(&path.join("lease.json")).get("sourceMember").is_none());
    assert!(raw(&path.join("manifest.json"))
        .get("sourceMember")
        .is_none());
    let plan = plan(vec![preview_id], vec![]).unwrap();
    let (_, legacy_plan_json) = load_plan_json(&open_catalog().unwrap(), &plan.plan_id).unwrap();
    assert!(
        serde_json::from_str::<Value>(&legacy_plan_json).unwrap()["snapshots"][0]
            .get("sourceMember")
            .is_none()
    );
    reset_runtime();
    let queued = commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    assert_eq!(job_get(&queued.job_id).unwrap().state, JobState::Completed);
    assert_eq!(
        load_plan_json(&open_catalog().unwrap(), &plan.plan_id)
            .unwrap()
            .1,
        legacy_plan_json
    );
}

#[test]
fn pin_version_proof_location_revision_and_seal_mutations_are_rejected() {
    let f = Fixture::new();
    let reference = scan::candidate(&f.scan.candidates[0].candidate_id).unwrap();
    let original = serde_json::to_value(reference).unwrap();
    for (pointer, replacement) in [
        ("/sourceMember/version", json!(99)),
        ("/sourceMember/proofVersion", json!(99)),
        ("/sourceMember/groupId", json!("foreign group")),
        (
            "/sourceMember/reference/location/locationId",
            json!(Uuid::new_v4().to_string()),
        ),
        (
            "/sourceMember/reference/sourceRevision",
            json!("different listing revision"),
        ),
        ("/sourceMember/seal", json!("00".repeat(32))),
    ] {
        let mut altered = original.clone();
        *altered.pointer_mut(pointer).unwrap() = replacement;
        let reference = serde_json::from_value(altered).unwrap();
        assert!(
            preview_reference(reference, None, None).is_err(),
            "{pointer}"
        );
    }
    let mut unknown = original;
    unknown["sourceMember"]["reference"]["location"]["unrecognized"] = json!(true);
    assert!(serde_json::from_value::<scan::CandidateRef>(unknown).is_err());
    assert!(list_entries(&open_catalog().unwrap()).unwrap().is_empty());
}

fn prepare_member(f: &Fixture) -> String {
    use crate::session_migration::{prepare, scan_selection};
    let candidate = &f.scan.candidates[0];
    let (preview_id, details) = preview(&candidate.candidate_id, None).unwrap();
    let selection = scan_selection::freeze_current(&f.scan.scan_id, f.scan.epoch).unwrap();
    let batch = prepare::start(
        &f.scan.scan_id,
        f.scan.epoch,
        &selection.selection_id,
        vec![prepare::PrepareOverride {
            candidate_id: candidate.candidate_id.clone(),
            preview_id,
            leaf_id: details.active_leaf.unwrap(),
            selection: details.default_selection,
        }],
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = prepare::status_raw(&batch.batch_id).unwrap();
        if status.state.can_plan() && status.active == 0 {
            assert_eq!(status.prepared, 1, "{status:?}");
            return batch.batch_id;
        }
        assert!(
            Instant::now() < deadline,
            "prepare did not finish: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn rewrite_plan(plan: &PlanState, change: impl FnOnce(&mut Value)) -> String {
    let conn = open_catalog().unwrap();
    let (_, json) = load_plan_json(&conn, &plan.plan_id).unwrap();
    let mut value: Value = serde_json::from_str(&json).unwrap();
    change(&mut value);
    let hash = crate::session_migration::hash::options_hash(&canonical_plan_payload(
        &plan.preview_ids,
        &plan.mappings,
        value["snapshots"].as_array().unwrap(),
        &plan.selection_set,
        &plan.selection_hash,
        &value["options"],
    ));
    value["planHash"] = json!(hash);
    conn.execute(
        "UPDATE plans SET plan_hash=?1,json=?2 WHERE plan_id=?3",
        rusqlite::params![hash, value.to_string(), plan.plan_id],
    )
    .unwrap();
    hash
}

#[test]
fn durable_plan_snapshot_contains_versioned_fixed_member_digest() {
    let f = Fixture::new();
    let plan = plan(vec![f.preview()], vec![]).unwrap();
    let (_, json) = load_plan_json(&open_catalog().unwrap(), &plan.plan_id).unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();
    let binding = &value["snapshots"][0]["sourceMember"];
    assert_eq!(binding["version"], 1, "plan must retain the member binding");
    assert_eq!(binding["digest"].as_str().unwrap().len(), 64);
    assert!(
        serde_json::to_value(&plan)
            .unwrap()
            .get("snapshots")
            .is_none(),
        "internal binding must not change the command DTO"
    );
}

#[test]
fn rehashed_plan_snapshot_substitution_fails_resident_and_disk_commit() {
    for disk in [false, true] {
        for mutation in [
            "digest",
            "missing",
            "duplicate",
            "member-version",
            "member-digest",
            "member-missing",
            "member-unknown",
        ] {
            let f = Fixture::new();
            let plan = plan(vec![f.preview()], vec![]).unwrap();
            let hash = rewrite_plan(&plan, |value| match mutation {
                "digest" => value["snapshots"][0]["snapshotDigest"] = json!("foreign snapshot"),
                "missing" => value["snapshots"] = json!([]),
                "duplicate" => {
                    let snapshot = value["snapshots"][0].clone();
                    value["snapshots"].as_array_mut().unwrap().push(snapshot);
                }
                "member-version" => {
                    value["snapshots"][0]["sourceMember"] =
                        json!({"version":99,"digest":"00".repeat(32)});
                }
                "member-digest" => {
                    value["snapshots"][0]["sourceMember"] =
                        json!({"version":1,"digest":"00".repeat(32)});
                }
                "member-missing" => {
                    value["snapshots"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("sourceMember");
                }
                "member-unknown" => {
                    value["snapshots"][0]["sourceMember"]["unrecognized"] = json!(true);
                }
                _ => unreachable!(),
            });
            if disk {
                reset_runtime();
            }
            assert!(
                commit(&plan.plan_id, &hash, &Uuid::new_v4().to_string()).is_err(),
                "disk={disk}, mutation={mutation}: canonical hash alone cannot authorize another snapshot"
            );
            assert!(list_entries(&open_catalog().unwrap()).unwrap().is_empty());
        }
    }
}

fn mutate_prepare_pin(batch_id: &str, mutation: &str) {
    let conn = crate::session_migration::prepare::open_existing(batch_id).unwrap();
    let input: String = conn
        .query_row("SELECT input_json FROM items WHERE seq=1", [], |r| r.get(0))
        .unwrap();
    let mut input: Value = serde_json::from_str(&input).unwrap();
    match mutation {
        "version" => input["sourceMember"]["version"] = json!(99),
        "seal" => input["sourceMember"]["seal"] = json!("00".repeat(32)),
        "missing" => {
            input.as_object_mut().unwrap().remove("sourceMember");
        }
        _ => unreachable!(),
    }
    conn.execute(
        "UPDATE items SET input_json=?1 WHERE seq=1",
        [input.to_string()],
    )
    .unwrap();
}

#[test]
fn reset_prepare_plan_rejects_changed_or_removed_member_pin() {
    for mutation in ["version", "seal", "missing"] {
        let f = Fixture::new();
        let batch = prepare_member(&f);
        mutate_prepare_pin(&batch, mutation);
        reset_runtime();
        crate::session_migration::prepare::reset_runtime();
        assert!(
            crate::session_migration::prepare::validated_plan_inputs(&batch).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn cached_prepare_mapping_rejects_changed_or_removed_member_pin() {
    for mutation in ["version", "seal", "missing"] {
        let f = Fixture::new();
        let batch = prepare_member(&f);
        mutate_prepare_pin(&batch, mutation);
        reset_runtime();
        crate::session_migration::prepare::reset_runtime();
        assert!(
            crate::session_migration::prepare::mappings(&batch, None).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn readonly_group_and_member_commands_reject_invalid_pin_without_source_io() {
    let f = Fixture::new();
    let reference = scan::candidate(&f.scan.candidates[0].candidate_id).unwrap();
    let group = reference.source_member.as_ref().unwrap().group_id.clone();
    let conn = scan::open_scan(&f.scan.scan_id, f.scan.epoch).unwrap();
    let original = serde_json::to_value(reference).unwrap();
    let schema_before: Vec<(String, Option<String>)> = conn
        .prepare("SELECT name,sql FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    std::fs::rename(
        &f.root,
        f.root.with_file_name("source offline for metadata"),
    )
    .unwrap();
    assert!(crate::session_migration::scan_groups::page(
        &f.scan.scan_id,
        f.scan.epoch,
        None,
        Some(2),
        &Default::default()
    )
    .is_ok());
    assert!(
        crate::session_migration::scan_groups::members(
            &f.scan.scan_id,
            f.scan.epoch,
            &group,
            None,
            Some(2)
        )
        .is_ok(),
        "metadata must remain readable offline"
    );
    let mut value = original;
    value["sourceMember"]["seal"] = json!("00".repeat(32));
    conn.execute("UPDATE candidates SET json=?1", [value.to_string()])
        .unwrap();
    let groups = tauri::async_runtime::block_on(crate::commands::session_import_scan_groups(
        f.scan.scan_id.clone(),
        f.scan.epoch,
        None,
        Some(2),
        None,
    ));
    let members =
        tauri::async_runtime::block_on(crate::commands::session_import_scan_group_members(
            f.scan.scan_id.clone(),
            f.scan.epoch,
            group,
            None,
            Some(2),
        ));
    let schema_after: Vec<(String, Option<String>)> = conn
        .prepare("SELECT name,sql FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(schema_before, schema_after, "metadata read must not DDL");
    assert!(
        groups.is_err() && members.is_err(),
        "groups={groups:?}, members={members:?}"
    );
}

#[test]
fn another_legitimately_scanned_pin_cannot_replace_plan_or_prepare_authority() {
    let f = Fixture::new();
    let batch = prepare_member(&f);
    let other = f.rescan();
    let other = scan::candidate(&other.candidates[0].candidate_id).unwrap();
    let conn = crate::session_migration::prepare::open_existing(&batch).unwrap();
    let original: String = conn
        .query_row("SELECT input_json FROM items WHERE seq=1", [], |r| r.get(0))
        .unwrap();
    let mut altered: Value = serde_json::from_str(&original).unwrap();
    altered["sourceMember"] = serde_json::to_value(&other.source_member).unwrap();
    conn.execute(
        "UPDATE items SET input_json=?1 WHERE seq=1",
        [altered.to_string()],
    )
    .unwrap();
    assert!(crate::session_migration::prepare::validated_plan_inputs(&batch).is_err());
    assert!(crate::session_migration::prepare::mappings(&batch, None).is_err());
    conn.execute("UPDATE items SET input_json=?1 WHERE seq=1", [original])
        .unwrap();
    let prepared = crate::session_migration::prepare::validated_plan_inputs(&batch).unwrap();
    let plan = plan_from_validated_prepare(prepared, vec![]).unwrap();
    let hash = rewrite_plan(&plan, |value| {
        value["snapshots"][0]["sourceMember"]["digest"] =
            json!(crate::session_migration::hash::options_hash(&json!([
                "session-import-plan-member-v1",
                other.source_member
            ])));
    });
    reset_runtime();
    assert!(commit(&plan.plan_id, &hash, &Uuid::new_v4().to_string()).is_err());
    assert!(list_entries(&open_catalog().unwrap()).unwrap().is_empty());
}

#[test]
fn pinned_prepare_offline_reset_mapping_plan_commit_and_reader_keep_member_a() {
    let f = Fixture::new();
    let batch = prepare_member(&f);
    let source_bytes = std::fs::read(f.root.join("selected.jsonl")).unwrap();
    let conn = crate::session_migration::prepare::open_existing(&batch).unwrap();
    let input_before: String = conn
        .query_row("SELECT input_json FROM items WHERE seq=1", [], |r| r.get(0))
        .unwrap();
    reset_runtime();
    crate::session_migration::prepare::reset_runtime();
    let offline = f.root.with_file_name("prepared source offline");
    std::fs::rename(&f.root, &offline).unwrap();
    let mappings = crate::session_migration::prepare::mappings(&batch, None).unwrap();
    let prepared = crate::session_migration::prepare::validated_plan_inputs(&batch).unwrap();
    let plan = plan_from_validated_prepare(prepared, mappings).unwrap();
    reset_runtime();
    let job = commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = job_get(&job.job_id).unwrap();
    assert_eq!(job.state, JobState::Completed, "{job:?}");
    reset_runtime();
    let item = &job.items[0];
    let (events, _, count) = events_page(
        item.import_id.as_deref().unwrap(),
        item.revision_id.as_deref(),
        None,
        Some(20),
        None,
    )
    .unwrap();
    assert_eq!(count, 1);
    assert!(serde_json::to_string(&events)
        .unwrap()
        .contains("FIXED_MEMBER_A"));
    let input_after: String = conn
        .query_row("SELECT input_json FROM items WHERE seq=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(input_after, input_before);
    assert_eq!(
        std::fs::read(offline.join("selected.jsonl")).unwrap(),
        source_bytes
    );
}

#[test]
fn rescan_same_member_deduplicates_without_rewriting_original_revision_or_ownership() {
    let f = Fixture::new();
    let first = plan(vec![f.preview()], vec![]).unwrap();
    let first_job = commit(
        &first.plan_id,
        &first.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let first_job = job_get(&first_job.job_id).unwrap();
    let original = &first_job.items[0];
    let archive = catalog::revision_dir(
        original.import_id.as_deref().unwrap(),
        original.revision_id.as_deref().unwrap(),
    )
    .unwrap();
    let before = crate::session_migration::jobs::published_hashes(&archive).unwrap();
    let old_receipt = crate::session_migration::jobs::latest_receipt(
        &open_catalog().unwrap(),
        &first_job.job_id,
        &original.candidate_id,
    )
    .unwrap()
    .unwrap();
    reset_runtime();
    let scan = f.rescan();
    let candidate = &scan.candidates[0];
    let second_preview = preview(&candidate.candidate_id, None).unwrap().0;
    let second = plan(vec![second_preview], vec![]).unwrap();
    reset_runtime();
    let queued = commit(
        &second.plan_id,
        &second.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let second_job = job_get(&queued.job_id).unwrap();
    assert_eq!(second_job.state, JobState::Completed, "{second_job:?}");
    assert_eq!(second_job.items[0].outcome.as_deref(), Some("deduplicated"));
    assert_eq!(second_job.items[0].import_id, original.import_id);
    assert_eq!(second_job.items[0].revision_id, original.revision_id);
    assert_eq!(
        crate::session_migration::jobs::published_hashes(&archive).unwrap(),
        before
    );
    let conn = open_catalog().unwrap();
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM revisions", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        serde_json::to_value(
            crate::session_migration::jobs::latest_receipt(
                &conn,
                &first_job.job_id,
                &original.candidate_id
            )
            .unwrap()
            .unwrap()
        )
        .unwrap(),
        serde_json::to_value(old_receipt).unwrap()
    );
    assert!(
        crate::session_migration::jobs::latest_receipt(
            &conn,
            &second_job.job_id,
            &second_job.items[0].candidate_id
        )
        .unwrap()
        .is_none(),
        "dedup must not claim ownership of another job's revision"
    );
}
