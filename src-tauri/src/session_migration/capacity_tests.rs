use super::*;
use crate::session_migration::jobs::{JobState, PreviewLease};
use crate::session_migration::scan::CandidateRef;
use crate::session_migration::selection::{AllowedSelection, PreviewSelection};
use crate::session_migration::types::{
    AdapterOutput, ContentBlock, EventBase, ImportOutcome, ImportedEvent, Inventory, MessageRole,
    NormalizedGraph, Provider, SourceCandidate, SourceEvidence, TimeOrigin,
};
use rusqlite::params;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Instant;
use uuid::Uuid;

#[path = "capacity_cancel_tests.rs"]
mod cancel;
#[path = "capacity_home_tests.rs"]
mod home_contracts;
#[path = "capacity_mixed_tests.rs"]
mod mixed;
#[path = "capacity_publish_tests.rs"]
mod publish;

struct CapacityHome {
    root: PathBuf,
    _environment: crate::session_migration::test_home::AppHomeEnvGuard,
}

impl CapacityHome {
    fn new() -> Self {
        Self::with_setup(|_| {})
    }

    fn with_setup(setup: impl FnOnce(&Path)) -> Self {
        let environment = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let root = std::env::temp_dir().join(format!("session-capacity-{}", Uuid::new_v4()));
        // Own cleanup and byte-exact restoration before any fallible setup.
        let home = Self {
            root,
            _environment: environment,
        };
        let app_home = home.root.join("app");
        std::fs::create_dir_all(&app_home).unwrap();
        std::env::set_var("GROK_APP_HOME", &app_home);
        setup(&app_home);
        crate::paths::ensure_app_dirs().unwrap();
        super::reset_runtime();
        crate::session_migration::engine::reset_runtime();
        home
    }
}

impl Drop for CapacityHome {
    fn drop(&mut self) {
        super::reset_runtime();
        crate::session_migration::engine::reset_runtime();
        let _ = std::fs::remove_dir_all(&self.root);
        // The environment field restores the inherited value before unlocking.
    }
}

fn sample_adapter(index: usize) -> AdapterOutput {
    AdapterOutput {
        evidence: SourceEvidence {
            provider: Provider::GenericFile,
            source_version: None,
            schema_fingerprint: "window4-capacity-v1".into(),
            adapter_version: "1.0.0".into(),
            platform: "synthetic".into(),
            artifacts: Vec::new(),
        },
        graph: NormalizedGraph {
            events: vec![ImportedEvent::Message {
                base: EventBase {
                    id: "e1".into(),
                    ordinal: 0,
                    spans: Vec::new(),
                    timestamp: None,
                    time_origin: TimeOrigin::Unknown,
                },
                role: MessageRole::User,
                blocks: vec![ContentBlock::text("bounded capacity event")],
                complete: true,
            }],
            relations: Vec::new(),
            active_leaf: Some("e1".into()),
            native_session_id: format!("capacity-native-{index:05}"),
            title: Some(format!("Capacity {index:05}")),
            cwd: Some(r"C:\synthetic\shared-workspace".into()),
        },
        inventory: Inventory::default(),
        issues: Vec::new(),
        assets: Vec::new(),
        outcome: ImportOutcome::Complete,
    }
}

fn link_or_copy(from: &Path, to: &Path) -> bool {
    if std::fs::hard_link(from, to).is_ok() {
        false
    } else {
        std::fs::copy(from, to).expect("copy capacity fixture file");
        true
    }
}

fn set_manifest_string(manifest: &mut Value, pointer: &str, value: String) {
    *manifest
        .pointer_mut(pointer)
        .unwrap_or_else(|| panic!("missing template manifest field {pointer}")) =
        Value::String(value);
}

fn create_capacity_batch(count: usize) -> (String, usize) {
    create_capacity_batch_from_root(count, None)
}

fn create_capacity_batch_from_root(count: usize, source_root: Option<&Path>) -> (String, usize) {
    let batch_id = Uuid::new_v4().to_string();
    let batch_path = batch_root().unwrap().join(format!("{batch_id}.sqlite"));
    let mut conn = crate::session_migration::sqlite_open::open(&batch_path).unwrap();
    configure(&conn).unwrap();
    create_schema(&conn).unwrap();
    let now = chrono::Utc::now();
    conn.execute(
        "INSERT INTO meta(id,schema_version,batch_id,scan_id,epoch,selection_id,
          selection_version,state,state_version,cancel_requested,created_at,updated_at,
          retention_expires_unix)
         VALUES(1,?1,?2,?3,1,?4,1,'completed',2,0,?5,?5,?6)",
        params![
            PREPARE_BATCH_VERSION,
            batch_id,
            Uuid::new_v4().to_string(),
            Uuid::new_v4().to_string(),
            now.to_rfc3339(),
            now.timestamp() + RETENTION_SECS,
        ],
    )
    .unwrap();

    let options = serde_json::json!({"rawRetention": "normalized-only"});
    let template_id = Uuid::new_v4().to_string();
    let mut template_adapter = sample_adapter(0);
    let template = crate::session_migration::jobs::prepare_revision(
        &mut template_adapter,
        &options,
        None,
        Some(&template_id),
    )
    .unwrap();
    let template_manifest: Value =
        serde_json::from_slice(&std::fs::read(template.path.join("manifest.json")).unwrap())
            .unwrap();
    let parent = crate::session_migration::catalog::staging_dir().join("previews");
    // The synthetic batch skips discovery, not source ownership. Use the same
    // registered local installation required by the real accepted-job boundary.
    // One installation still owns every candidate in the capacity batch.
    let registration = crate::session_migration::sources::register(
        &crate::session_migration::catalog::open_catalog().unwrap(),
        Provider::GenericFile,
        source_root.unwrap_or(&parent),
        None,
        "synthetic-capacity",
    )
    .unwrap();
    let root_path = registration.root_path;
    let source_id = registration.source_id;
    let static_files = ["events.jsonl", "assets.json", "event-index.sqlite"];
    let mut hard_link_fallbacks = 0usize;

    let tx = conn.transaction().unwrap();
    {
        let mut insert = tx
            .prepare(
                "INSERT INTO items(seq,candidate_id,title,provider,source_revision,input_json,
                 state,attempt,preview_id,selection_json,error_json)
                 VALUES(?1,?2,?3,'generic-file',?4,?5,'prepared',1,?6,?7,NULL)",
            )
            .unwrap();
        for index in 0..count {
            let preview_id = if index == 0 {
                template_id.clone()
            } else {
                Uuid::new_v4().to_string()
            };
            let path = parent.join(&preview_id);
            let adapter = sample_adapter(index);
            let snapshot_digest =
                crate::session_migration::hash::normalized_digest(&adapter, &options);
            let file_hashes = if index == 0 {
                template.file_hashes.clone()
            } else {
                std::fs::create_dir(&path).unwrap();
                for name in static_files {
                    hard_link_fallbacks +=
                        usize::from(link_or_copy(&template.path.join(name), &path.join(name)));
                }
                let mut manifest = template_manifest.clone();
                set_manifest_string(&mut manifest, "/sourceSnapshotId", snapshot_digest.clone());
                set_manifest_string(
                    &mut manifest,
                    "/graph/nativeSessionId",
                    adapter.graph.native_session_id.clone(),
                );
                set_manifest_string(
                    &mut manifest,
                    "/graph/title",
                    adapter.graph.title.clone().unwrap(),
                );
                let bytes = serde_json::to_vec(&manifest).unwrap();
                std::fs::write(path.join("manifest.json"), &bytes).unwrap();
                let mut hashes = template.file_hashes.clone();
                hashes.insert(
                    "manifest.json".into(),
                    crate::session_migration::hash::sha256_bytes(&bytes),
                );
                hashes
            };
            let candidate_id = format!("capacity-candidate-{index:05}");
            let source_revision = format!("capacity-source-revision-{index:05}");
            let legacy_id = format!("capacity-legacy-{index:05}");
            crate::session_migration::jobs::write_preview_lease(
                &path,
                &PreviewLease {
                    source_member: None,
                    preview_id: preview_id.clone(),
                    candidate_id: candidate_id.clone(),
                    source_revision: source_revision.clone(),
                    prepare_batch_id: Some(batch_id.clone()),
                    source_id: source_id.clone(),
                    root_path: root_path.clone(),
                    legacy_id: legacy_id.clone(),
                    snapshot_digest,
                    expires_unix: now.timestamp()
                        + crate::session_migration::budget::PREVIEW_TTL_SECS as i64,
                    retained_by_job: false,
                    file_hashes,
                },
            )
            .unwrap();
            let reference = CandidateRef {
                source_member: None,
                candidate: SourceCandidate {
                    candidate_id: candidate_id.clone(),
                    provider: Provider::GenericFile,
                    native_session_id: adapter.graph.native_session_id,
                    title: adapter.graph.title.unwrap(),
                    cwd: adapter.graph.cwd,
                    started_at: None,
                    updated_at: None,
                    locator: format!("synthetic://capacity/{index:05}"),
                    source_revision: source_revision.clone(),
                    archived: None,
                    completeness: "complete".into(),
                },
                source_id: source_id.clone(),
                location: None,
                root_path: root_path.clone(),
                legacy_id,
            };
            let selection = PreviewSelection {
                preview_id: preview_id.clone(),
                leaf_id: "e1".into(),
                selection: AllowedSelection {
                    event_ids: vec!["e1".into()],
                    ..AllowedSelection::default()
                },
            };
            insert
                .execute(params![
                    index + 1,
                    candidate_id,
                    format!("Capacity {index:05}"),
                    source_revision,
                    serde_json::to_string(&reference).unwrap(),
                    preview_id,
                    serde_json::to_string(&selection).unwrap(),
                ])
                .unwrap();
            crate::session_migration::prepare_mapping::store(
                &tx,
                &candidate_id,
                &preview_id,
                &crate::session_migration::mapping_scope::MappingSubject::new(
                    reference.source_id.clone(),
                    "generic-file".into(),
                    reference.candidate.cwd.clone(),
                ),
            )
            .unwrap();
        }
    }
    tx.commit().unwrap();
    drop(conn);
    drop(template);
    (batch_id, hard_link_fallbacks)
}

fn elapsed_ms<T>(operation: impl FnOnce() -> T) -> (T, u128) {
    let started = Instant::now();
    let value = operation();
    (value, started.elapsed().as_millis())
}

fn json_bytes(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).unwrap().len()
}

fn report_stage(name: &str, count: usize, elapsed_ms: u128) {
    println!("WINDOW4_STAGE candidateCount={count} name={name} elapsedMs={elapsed_ms}");
}

#[test]
#[ignore = "capacity harness; run with GROK_WINDOW4_CAPACITY=1 and 1000, 3000 or 10000 candidates"]
fn window4_capacity_benchmark() {
    capacity_benchmark(false);
}

#[test]
#[ignore = "full publication harness; opt in with GROK_WINDOW4_CAPACITY=1 and 1000, 3000 or 10000 candidates"]
fn window4_full_publish_benchmark() {
    capacity_benchmark(true);
}

fn capacity_benchmark(full_publish: bool) {
    assert_eq!(
        std::env::var("GROK_WINDOW4_CAPACITY").as_deref(),
        Ok("1"),
        "capacity harness requires an explicit opt-in"
    );
    let count: usize = std::env::var("GROK_WINDOW4_CANDIDATES")
        .expect("GROK_WINDOW4_CANDIDATES")
        .parse()
        .expect("candidate count is an integer");
    assert!(matches!(count, 1_000 | 3_000 | 10_000));

    let _home = CapacityHome::new();
    let profile = crate::session_migration::codec_profile::Session::default();
    let profile_scope = profile.enter("prepare");
    let base_disk_bytes =
        crate::session_migration::jobs::dir_size(&crate::session_migration::catalog::imports_dir());
    let ((batch_id, hard_link_fallbacks), fixture_ms) = elapsed_ms(|| create_capacity_batch(count));
    report_stage("fixture", count, fixture_ms);
    let fixture_disk_bytes =
        crate::session_migration::jobs::dir_size(&crate::session_migration::catalog::imports_dir());

    crate::session_migration::jobs::reset_preview_directory_scans();
    let (status, status_ms) = elapsed_ms(|| super::status(&batch_id).unwrap());
    report_stage("status", count, status_ms);
    let (items_page, items_page_ms) =
        elapsed_ms(|| super::items_page(&batch_id, None, Some(50)).unwrap());
    report_stage("items-page", count, items_page_ms);
    let (mappings, mappings_ms) = elapsed_ms(|| super::mappings(&batch_id, None).unwrap());
    report_stage("mappings", count, mappings_ms);
    let (prepared, plan_inputs_ms) =
        elapsed_ms(|| super::validated_plan_inputs(&batch_id).unwrap());
    report_stage("plan-inputs", count, plan_inputs_ms);
    let (plan, plan_ms) = elapsed_ms(|| {
        crate::session_migration::engine::plan_from_validated_prepare(prepared, mappings.clone())
            .unwrap()
    });
    report_stage("plan", count, plan_ms);
    let plan_ref = crate::session_migration::engine::DurablePlanRef::from(&plan);
    let plan_disk_bytes =
        crate::session_migration::jobs::dir_size(&crate::session_migration::catalog::imports_dir());
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let durable_plan_bytes: i64 = conn
        .query_row(
            "SELECT length(json) FROM plans WHERE plan_id=?1",
            params![plan.plan_id],
            |row| row.get(0),
        )
        .unwrap();
    drop(conn);

    crate::session_migration::engine::reset_runtime();
    super::reset_runtime();
    let (recovery, recovery_ms) =
        elapsed_ms(|| crate::session_migration::engine::recovery().unwrap());
    report_stage("recovery", count, recovery_ms);
    let (job, commit_setup_ms) = elapsed_ms(|| {
        crate::session_migration::engine::commit(
            &plan.plan_id,
            &plan.plan_hash,
            &format!("capacity-request-{count}"),
        )
        .unwrap()
    });
    report_stage("commit-setup", count, commit_setup_ms);
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let job_header_bytes: i64 = conn
        .query_row(
            "SELECT length(json) FROM jobs WHERE job_id=?1",
            params![job.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let durable_job_input_bytes: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(length(result_json)),0) FROM job_items WHERE job_id=?1",
            params![job.job_id],
            |row| row.get(0),
        )
        .unwrap();
    drop(conn);
    let commit_disk_bytes =
        crate::session_migration::jobs::dir_size(&crate::session_migration::catalog::imports_dir());
    let directory_scans = crate::session_migration::jobs::preview_directory_scans();

    assert_eq!(status.total as usize, count);
    assert_eq!(status.prepared as usize, count);
    assert_eq!(items_page.items.len(), 50);
    assert_eq!(mappings.len(), 1);
    assert_eq!(job.state, JobState::Queued);
    assert_eq!(job.item_count as usize, count);
    assert_eq!(job.items.len(), 50);
    assert_eq!(directory_scans, 0);
    assert!(status_ms < 500, "status took {status_ms}ms");
    assert!(items_page_ms < 500, "items page took {items_page_ms}ms");

    let status_ipc_bytes = json_bytes(&status);
    let items_ipc_bytes = json_bytes(&items_page);
    let mappings_ipc_bytes = json_bytes(&mappings);
    let plan_ref_ipc_bytes = json_bytes(&plan_ref);
    let recovery_ipc_bytes = json_bytes(&recovery);
    let commit_ipc_bytes = json_bytes(&job);
    assert!(status_ipc_bytes < 4 * 1024);
    assert!(items_ipc_bytes < 64 * 1024);
    assert!(mappings_ipc_bytes < 4 * 1024);
    assert!(plan_ref_ipc_bytes < 512);
    assert!(recovery_ipc_bytes < 4 * 1024);
    assert!(commit_ipc_bytes < 64 * 1024);

    println!(
        "WINDOW4_CAPACITY {}",
        serde_json::to_string(&serde_json::json!({
            "candidateCount": count,
            "fixtureMs": fixture_ms,
            "statusMs": status_ms,
            "itemsPageMs": items_page_ms,
            "planInputsMs": plan_inputs_ms,
            "mappingsMs": mappings_ms,
            "planMs": plan_ms,
            "recoveryMs": recovery_ms,
            "commitSetupMs": commit_setup_ms,
            "directoryEnumerations": directory_scans,
            "hardLinkFallbacks": hard_link_fallbacks,
            "baseLogicalDiskBytes": base_disk_bytes,
            "fixtureLogicalDiskBytes": fixture_disk_bytes,
            "planLogicalDiskBytes": plan_disk_bytes,
            "commitLogicalDiskBytes": commit_disk_bytes,
            "durablePlanBytes": durable_plan_bytes,
            "jobHeaderBytes": job_header_bytes,
            "durableJobInputBytes": durable_job_input_bytes,
            "statusIpcBytes": status_ipc_bytes,
            "itemsIpcBytes": items_ipc_bytes,
            "mappingsIpcBytes": mappings_ipc_bytes,
            "planRefIpcBytes": plan_ref_ipc_bytes,
            "recoveryIpcBytes": recovery_ipc_bytes,
            "commitIpcBytes": commit_ipc_bytes,
        }))
        .unwrap()
    );
    drop(profile_scope);
    profile.report("prepare");
    if full_publish {
        publish::verify(&job, count, commit_disk_bytes);
    }
}
