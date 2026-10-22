//! Synthetic codec/journal checks only. No accepted producer or Undo grant is
//! manufactured here, and no private user home is read.
use super::{record::*, *};
use crate::session_migration::test_home::IsolatedAppHome;
use serde_json::{json, Value};

const JOB: &str = "11111111-1111-4111-8111-111111111111";
const PLAN: &str = "22222222-2222-4222-8222-222222222222";
const RECEIPT: &str = "33333333-3333-4333-8333-333333333333";
const IMPORT: &str = "imp:44444444-4444-4444-8444-444444444444";
const REVISION: &str = "55555555-5555-4555-8555-555555555555";
const PROJECT: &str = "66666666-6666-4666-8666-666666666666";
const OTHER: &str = "77777777-7777-4777-8777-777777777777";

fn binding() -> Binding {
    Binding {
        job_id: JOB.into(),
        context_digest: "a".repeat(64),
        plan_id: PLAN.into(),
        plan_hash: "b".repeat(64),
        plan_digest: "c".repeat(64),
        acceptance_digest: "d".repeat(64),
        candidate_digest: "e".repeat(64),
        input_digest: "f".repeat(64),
    }
}

fn allocation(project: ProjectAllocation) -> Stage {
    Stage::Allocated(Allocation {
        receipt_id: RECEIPT.into(),
        import_id: IMPORT.into(),
        revision_id: REVISION.into(),
        previous_revision: None,
        created_entry: true,
        project,
    })
}

fn create(allow_unassigned: bool) -> ProjectAllocation {
    ProjectAllocation::Create {
        project_id: PROJECT.into(),
        path_digest: "0".repeat(64),
        allow_unassigned,
    }
}

fn created(id: &str) -> ProjectResult {
    ProjectResult::Created {
        project_id: id.into(),
        path_digest: "0".repeat(64),
        snapshot_digest: "1".repeat(64),
    }
}

fn matched(id: &str) -> ProjectResult {
    ProjectResult::Matched {
        project_id: id.into(),
        path_digest: "0".repeat(64),
        snapshot_digest: "1".repeat(64),
    }
}

fn stored(allocation: &VerifiedRecord, project: ProjectResult) -> Stage {
    Stage::Stored(StoreResult {
        allocation_digest: allocation.digest.clone(),
        linked_receipt_digest: "2".repeat(64),
        project,
    })
}

fn committed(store: &VerifiedRecord) -> Stage {
    Stage::Committed(Committed {
        store_digest: store.digest.clone(),
        receipt_digest: "3".repeat(64),
        item_digest: "4".repeat(64),
    })
}

fn verified(auth: &InstallationAuthenticator, sealed: &SealedRecord) -> VerifiedRecord {
    verify(
        auth,
        sealed.encoded.as_bytes(),
        &sealed.body.binding,
        sealed.body.stage.kind(),
    )
    .unwrap()
}

fn corrupt(auth: &InstallationAuthenticator, sealed: &SealedRecord, value: &Value) {
    assert!(verify(
        auth,
        value.to_string().as_bytes(),
        &sealed.body.binding,
        sealed.body.stage.kind()
    )
    .is_err());
}

#[test]
fn outcome_codec_roundtrip_all_real_result_shapes_and_chain() {
    let _home = IsolatedAppHome::acquire("outcome-roundtrip");
    let auth = InstallationAuthenticator::load().unwrap();
    for (project, result) in [
        (create(false), created(PROJECT)),
        (create(false), matched(OTHER)),
        (create(true), ProjectResult::UnassignedReported {}),
        (
            ProjectAllocation::Unassigned {},
            ProjectResult::Unassigned {},
        ),
        (
            ProjectAllocation::Match {
                project_id: PROJECT.into(),
                path_digest: "0".repeat(64),
                allow_unassigned: false,
            },
            matched(PROJECT),
        ),
        (
            ProjectAllocation::Match {
                project_id: "legacy-中文-project".into(),
                path_digest: "0".repeat(64),
                allow_unassigned: false,
            },
            matched("legacy-中文-project"),
        ),
        (
            ProjectAllocation::Match {
                project_id: PROJECT.into(),
                path_digest: "0".repeat(64),
                allow_unassigned: true,
            },
            ProjectResult::UnassignedReported {},
        ),
    ] {
        let allocated = seal(&auth, binding(), allocation(project)).unwrap();
        let a = verified(&auth, &allocated);
        let stored = seal(&auth, binding(), stored(&a, result)).unwrap();
        let s = verified(&auth, &stored);
        s.follows(&a).unwrap();
        let committed = seal(&auth, binding(), committed(&s)).unwrap();
        let c = verified(&auth, &committed);
        c.follows(&s).unwrap();
        assert!(c.follows(&a).is_err());
        assert!(a.follows(&s).is_err());
        assert!(s.follows(&s).is_err());
        assert!(allocated.encoded.len() < RECORD_BYTES);
    }
}

#[test]
fn outcome_codec_original_bytes_reject_duplicates_at_each_depth() {
    let _home = IsolatedAppHome::acquire("outcome-duplicates");
    let auth = InstallationAuthenticator::load().unwrap();
    let record = seal(&auth, binding(), allocation(create(false))).unwrap();
    for (field, replacement) in [
        ("\"body\":", "\"body\":null,\"body\":"),
        ("\"mac\":", "\"mac\":null,\"mac\":"),
        ("\"version\":1", "\"version\":1,\"version\":1"),
        ("\"binding\":", "\"binding\":null,\"binding\":"),
        ("\"jobId\":", "\"jobId\":null,\"jobId\":"),
        ("\"allocated\":", "\"allocated\":null,\"allocated\":"),
        (
            "\"createdEntry\":true",
            "\"createdEntry\":true,\"createdEntry\":true",
        ),
        ("\"create\":", "\"create\":null,\"create\":"),
        ("\"projectId\":", "\"projectId\":null,\"projectId\":"),
    ] {
        assert!(record.encoded.contains(field));
        let changed = record.encoded.replacen(field, replacement, 1);
        assert!(verify(&auth, changed.as_bytes(), &binding(), StageKind::Allocated).is_err());
    }
}

#[test]
fn outcome_codec_schema_dispatch_utf8_and_bounded_metadata() {
    let _home = IsolatedAppHome::acquire("outcome-schema");
    let auth = InstallationAuthenticator::load().unwrap();
    let record = seal(&auth, binding(), allocation(create(false))).unwrap();
    for (version, code) in [
        (0, ErrorCode::SchemaUnsupported),
        (2, ErrorCode::ImportSchemaTooNew),
        (u32::MAX, ErrorCode::ImportSchemaTooNew),
    ] {
        let mut value: Value = serde_json::from_str(&record.encoded).unwrap();
        value["body"]["version"] = json!(version);
        let error = verify(
            &auth,
            value.to_string().as_bytes(),
            &binding(),
            StageKind::Allocated,
        )
        .err()
        .unwrap();
        assert_eq!(error.code, code);
    }
    for raw in [
        vec![],
        vec![0xff],
        vec![b' '; RECORD_BYTES + 1],
        [vec![0xef, 0xbb, 0xbf], record.encoded.as_bytes().to_vec()].concat(),
    ] {
        assert!(verify(&auth, &raw, &binding(), StageKind::Allocated).is_err());
    }
    for kind in [StageKind::Stored, StageKind::Committed] {
        assert!(verify(&auth, record.encoded.as_bytes(), &binding(), kind).is_err());
    }
    // Ordinary input itself is larger than the association record budget. Only
    // its original-byte digest enters this separate result metadata record.
    let large = "synthetic ordinary input ".repeat(800_000);
    assert!(large.len() > 16 * 1024 * 1024);
    let mut long_binding = binding();
    long_binding.input_digest = sha256_bytes(large.as_bytes());
    let large_input_record = seal(&auth, long_binding, allocation(create(false))).unwrap();
    verified(&auth, &large_input_record);
}

#[path = "publication_outcome_journal_tests.rs"]
mod journal_tests;
#[path = "publication_native_project_tests.rs"]
mod native_project_tests;
#[path = "publication_outcome_shape_tests.rs"]
mod shape;
