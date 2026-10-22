//! Production plan codec checks, not proof of result ownership or a new writer.
use super::*;
use crate::session_migration::{
    engine, hash,
    identity_association::{self, AssociationConfirmation},
    jobs::{JobRecord, JobState},
    selection::{AllowedSelection, PreviewSelection, SelectionSet},
};

#[path = "publication_job_id_tests.rs"]
mod job_ids;

const PLAN: &str = "11111111-1111-4111-8111-111111111111";
const PREVIEW: &str = "22222222-2222-4222-8222-222222222222";
const CREATED: &str = "2026-10-14T00:00:00+00:00";
const EXPIRY: &str = "2026-10-14T00:01:00+00:00";

fn selections() -> SelectionSet {
    SelectionSet::single(PreviewSelection {
        preview_id: PREVIEW.into(),
        leaf_id: "synthetic-leaf".into(),
        selection: AllowedSelection::for_events(vec!["synthetic-event".into()]),
    })
}

fn confirmation() -> AssociationConfirmation {
    AssociationConfirmation {
        version: 1,
        import_id: "imp:33333333-3333-4333-8333-333333333333".into(),
        expected_revision: "44444444-4444-4444-8444-444444444444".into(),
        preview_id: PREVIEW.into(),
        review_digest: "b".repeat(64),
    }
}

fn record(protocol: PublicationProtocol, associated: bool) -> Value {
    let selection = selections();
    let confirmations = if associated {
        vec![confirmation()]
    } else {
        vec![]
    };
    let base = json!({
        "version": 1,
        "previewIds": [PREVIEW], "mappings": [],
        "snapshots": [{"previewId": PREVIEW, "snapshotDigest": "a".repeat(64)}],
        "selectionSet": selection, "selectionHash": selection.hash(), "options": {}
    });
    let payload = protocol.bind_payload(
        identity_association::bind_payload(base, &confirmations, PLAN, CREATED, EXPIRY),
        PLAN,
        CREATED,
        EXPIRY,
    );
    let digest = hash::options_hash(&payload);
    let mut record = payload;
    // These fields were outside ordinary v1's canonical hash and are still
    // outside it; v2/v3 include their original/new explicitly bound lifetime.
    record["planId"] = json!(PLAN);
    record["created"] = json!(CREATED);
    record["expiry"] = json!(EXPIRY);
    record["planHash"] = json!(digest);
    record
}

fn decode_record(record: &Value) -> Result<engine::PlanState, ImportError> {
    engine::job_entry_plan_json(
        PLAN,
        record["planHash"].as_str().unwrap(),
        &record.to_string(),
    )
}

#[test]
fn publication_protocol_dispatch_requires_exact_version_and_descriptor() {
    for version in [1, 2] {
        let original = json!({"version": version});
        assert_eq!(
            PublicationProtocol::decode(&original.to_string(), &original).unwrap(),
            PublicationProtocol::Legacy
        );
        for descriptor in [Value::Null, json!(false), json!({"version": 1})] {
            let changed = json!({"version":version,"publicationOutcome":descriptor});
            assert!(PublicationProtocol::decode(&changed.to_string(), &changed).is_err());
        }
    }
    let original = json!({"version":3,"publicationOutcome":{"version":1}});
    assert_eq!(
        PublicationProtocol::decode(&original.to_string(), &original).unwrap(),
        PublicationProtocol::SignedOutcomeV1
    );
    for descriptor in [
        Value::Null,
        json!(true),
        json!(1),
        json!({}),
        json!({"version":1,"authorized":true}),
        json!({"version":"1"}),
        json!({"version":1.0}),
    ] {
        let changed = json!({"version":3,"publicationOutcome":descriptor});
        assert!(PublicationProtocol::decode(&changed.to_string(), &changed).is_err());
    }
    assert!(PublicationProtocol::decode(r#"{"version":3}"#, &json!({"version":3})).is_err());
    for version in [0, 2, 99] {
        let changed = json!({"version":3,"publicationOutcome":{"version":version}});
        assert_eq!(
            PublicationProtocol::decode(&changed.to_string(), &changed)
                .unwrap_err()
                .code,
            if version == 0 {
                ErrorCode::SchemaUnsupported
            } else {
                ErrorCode::ImportSchemaTooNew
            }
        );
    }
    for version in [0, 4, u64::MAX] {
        let changed = json!({"version":version});
        assert!(PublicationProtocol::decode(&changed.to_string(), &changed).is_err());
    }
}

#[test]
fn publication_protocol_original_text_refuses_duplicate_fields() {
    for raw in [
        r#"{"version":1,"version":3,"publicationOutcome":{"version":1}}"#,
        r#"{"version":3,"publicationOutcome":{"version":1},"publicationOutcome":{"version":1}}"#,
        r#"{"version":3,"publicationOutcome":{"version":0,"version":1}}"#,
    ] {
        let collapsed: Value = serde_json::from_str(raw).unwrap();
        assert!(PublicationProtocol::decode(raw, &collapsed).is_err());
    }
    assert!(PublicationProtocol::decode(
        r#"{"version":1}"#,
        &json!({"version":3,"publicationOutcome":{"version":1}})
    )
    .is_err());
}

#[test]
fn publication_protocol_internal_association_dispatch_cannot_relabel_record_version() {
    for (protocol, associated) in [
        (PublicationProtocol::Legacy, false),
        (PublicationProtocol::Legacy, true),
        (PublicationProtocol::SignedOutcomeV1, false),
        (PublicationProtocol::SignedOutcomeV1, true),
    ] {
        let original = record(protocol, associated);
        let actual = original["version"].as_u64().unwrap();
        for dispatch in [0, 1, 2, 3, 4] {
            let decoded = identity_association::parse_record(&original, dispatch, &selections());
            assert_eq!(decoded.is_ok(), dispatch == actual);
        }
    }
}

#[test]
fn publication_protocol_legacy_binding_preserves_exact_payload_bytes() {
    let v1 = json!({"version":1,"options":{"note":"中文"},"mappings":[]});
    let expected_v1 = r#"{"mappings":[],"options":{"note":"中文"},"version":1}"#;
    assert_eq!(hash::canonical_json(&v1), expected_v1);
    assert_eq!(
        hash::canonical_json(&PublicationProtocol::Legacy.bind_payload(v1, PLAN, CREATED, EXPIRY)),
        expected_v1
    );
    let v2 = json!({"version":2,"planId":PLAN,"created":CREATED,"expiry":EXPIRY,
        "identityAssociations":[{"version":1}],"options":{}});
    let expected_v2 = concat!(
        "{\"created\":\"2026-10-14T00:00:00+00:00\",",
        "\"expiry\":\"2026-10-14T00:01:00+00:00\",",
        "\"identityAssociations\":[{\"version\":1}],\"options\":{},",
        "\"planId\":\"11111111-1111-4111-8111-111111111111\",\"version\":2}"
    );
    assert_eq!(
        hash::canonical_json(&PublicationProtocol::Legacy.bind_payload(v2, PLAN, CREATED, EXPIRY)),
        expected_v2
    );
    for associated in [false, true] {
        let old = record(PublicationProtocol::Legacy, associated);
        let loaded = decode_record(&old).unwrap();
        assert_eq!(loaded.publication_protocol, PublicationProtocol::Legacy);
        assert_eq!(loaded.identity_associations.len(), usize::from(associated));
    }
}

#[test]
fn publication_protocol_v3_codec_binds_marker_identity_lifetime_and_associations() {
    for associated in [false, true] {
        let original = record(PublicationProtocol::SignedOutcomeV1, associated);
        let loaded = decode_record(&original).unwrap();
        assert_eq!(
            loaded.publication_protocol,
            PublicationProtocol::SignedOutcomeV1
        );
        assert_eq!(loaded.identity_associations.len(), usize::from(associated));
        // The command DTO is unchanged; protocol selection is re-read from
        // accepted original bytes, not a caller's deserialized PlanState.
        assert!(serde_json::to_value(&loaded)
            .unwrap()
            .get("publicationProtocol")
            .is_none());
        for field in [
            "publicationOutcome",
            "created",
            "expiry",
            "planId",
            "version",
        ] {
            let mut changed = original.clone();
            match field {
                "publicationOutcome" => {
                    changed.as_object_mut().unwrap().remove(field);
                }
                "created" => changed[field] = json!("2026-10-14T00:00:01+00:00"),
                "expiry" => changed[field] = json!("2026-10-14T00:00:59+00:00"),
                "planId" => changed[field] = json!("55555555-5555-4555-8555-555555555555"),
                "version" => changed[field] = json!(if associated { 2 } else { 1 }),
                _ => unreachable!(),
            }
            assert!(decode_record(&changed).is_err(), "{field}");
        }
        // Coordinated deletion of the marker plus a downgraded version still
        // fails the ORIGINAL plan hash, even though the old shape is valid.
        let mut downgrade = original.clone();
        downgrade
            .as_object_mut()
            .unwrap()
            .remove("publicationOutcome");
        downgrade["version"] = json!(if associated { 2 } else { 1 });
        assert!(decode_record(&downgrade).is_err());
        if associated {
            let mut changed = original.clone();
            changed["identityAssociations"][0]["reviewDigest"] = json!("c".repeat(64));
            assert!(decode_record(&changed).is_err());
        }
    }
}

#[test]
fn publication_protocol_refuses_options_flags_and_mismatched_job_before_io() {
    for field in ["publicationOutcome", "publicationOutcomeVersion"] {
        for value in [Value::Null, json!(false), json!({"version":1})] {
            let mut options = json!({});
            options[field] = value;
            assert!(identity_association::reject_behavior_options(&options).is_err());
        }
    }
    for associated in [false, true] {
        let plan =
            decode_record(&record(PublicationProtocol::SignedOutcomeV1, associated)).unwrap();
        let job = JobRecord {
            job_id: "66666666-6666-4666-8666-666666666666".into(),
            request_id: "synthetic-not-a-permission".into(),
            plan_id: PLAN.into(),
            plan_hash: plan.plan_hash.clone(),
            state: JobState::Queued,
            state_version: 1,
            items: vec![],
            created_at: CREATED.into(),
            item_count: 0,
            next_item_cursor: None,
            progress: None,
        };
        // There is deliberately no App home/preview/project/source here.
        // A misplaced guard would try real preview I/O and return another code.
        assert_eq!(
            engine::commit_items_for_plan(&plan, &job, false)
                .unwrap_err()
                .code,
            ErrorCode::PlanStale
        );
    }
}

#[test]
fn publication_protocol_does_not_add_an_ordinary_plan_size_limit() {
    let mut ordinary = record(PublicationProtocol::Legacy, false);
    // Beyond the existing association-routing budget; ordinary plans must not
    // acquire that unrelated cap just to classify publication protocol absence.
    ordinary["options"]["syntheticLargeOrdinaryField"] = json!("x".repeat(16 * 1024 * 1024 + 1));
    let mut payload = ordinary.clone();
    for field in ["planId", "planHash", "created", "expiry"] {
        payload.as_object_mut().unwrap().remove(field);
    }
    ordinary["planHash"] = json!(hash::options_hash(&payload));
    assert_eq!(
        decode_record(&ordinary).unwrap().publication_protocol,
        PublicationProtocol::Legacy
    );
}
