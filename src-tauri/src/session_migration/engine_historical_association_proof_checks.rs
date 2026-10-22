//! Normal pinned inspect/plan/input protocol drafts, not accepted authority.
//! Store probes are explicitly unaccepted rows in a rolled-back transaction.
use super::*;
use crate::session_migration::{
    catalog,
    identity_association::{self, AssociationRequest},
    job_control,
    job_input_association::AssociationInput,
    registration_provenance::{self, AssociationProof, ProofRecord},
    registration_provenance_store as proof_store,
};

pub(super) fn exercise(
    plan: &PlanState,
    job: &JobRecord,
    item: &jobs::CommitItem,
    encoded: &str,
    association: &AssociationInput,
) -> usize {
    let request = AssociationRequest {
        version: 1,
        import_id: association.confirmation.import_id.clone(),
        expected_revision: association.confirmation.expected_revision.clone(),
        preview: plan.selection_set.single_entry().unwrap().clone(),
    };
    let witness = identity_association::inspect_witness(&request).unwrap();
    assert_eq!(
        witness.review().unwrap(),
        identity_association::inspect(&request).unwrap()
    );
    assert_eq!(
        witness.review().unwrap().review_digest,
        association.confirmation.review_digest
    );
    let (actual_association, input) =
        job_control::inspect_association_draft_input(encoded, job).unwrap();
    assert_eq!(&actual_association, association);
    assert_eq!(input.candidate_id, item.candidate_id);
    assert_eq!(input.source_id, item.installation_id);
    assert_eq!(input.provider, item.provider);
    assert_eq!(
        input.native_session_id,
        item.adapter.graph.native_session_id
    );
    assert_eq!(
        input.digest,
        job_control::inspect_input_metadata(encoded, job).unwrap()
    );
    let draft = AssociationProof::draft(&witness, association.clone(), input.clone(), job, encoded)
        .unwrap();
    let record = ProofRecord::Association(Box::new(draft));
    let raw = serde_json::to_string(&record).unwrap();
    let parsed = ProofRecord::parse(&raw).unwrap();
    assert!(matches!(parsed, ProofRecord::Association(_)));
    assert_eq!(serde_json::to_string(&parsed).unwrap(), raw);
    assert_eq!(parsed.digest().unwrap(), record.digest().unwrap());
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let canonical_wire = value.to_string();
    assert_eq!(value["version"], 2);
    assert_eq!(value["input"], json!(input));
    assert_eq!(value["association"], json!(association));
    let mut checks = 3;

    // Required/duplicate fields are strict at each nested object boundary.
    for pointer in ["", "/input", "/association", "/association/confirmation"] {
        let object = value.pointer(pointer).unwrap().as_object().unwrap();
        for key in object.keys() {
            let mut changed = value.clone();
            changed
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(
                ProofRecord::parse(&changed.to_string()).is_err(),
                "missing {pointer}/{key}"
            );
            for invalid in [json!(null), json!([])] {
                let mut wrong_type = value.clone();
                wrong_type.pointer_mut(pointer).unwrap()[key] = invalid;
                assert!(
                    ProofRecord::parse(&wrong_type.to_string()).is_err(),
                    "type {pointer}/{key}"
                );
            }
            let nested = serde_json::to_string(&value.pointer(pointer).unwrap()).unwrap();
            let duplicated = format!("{{\"{key}\":{},{}", object[key], &nested[1..]);
            let wire = if pointer.is_empty() {
                duplicated
            } else {
                canonical_wire.replacen(&nested, &duplicated, 1)
            };
            assert_ne!(
                wire, canonical_wire,
                "duplicate fixture must alter {pointer}/{key}"
            );
            assert!(
                ProofRecord::parse(&wire).is_err(),
                "duplicate {pointer}/{key}"
            );
            checks += 1;
        }
        let mut changed = value.clone();
        changed.pointer_mut(pointer).unwrap()["authorized"] = json!(true);
        assert!(
            ProofRecord::parse(&changed.to_string()).is_err(),
            "unknown {pointer}"
        );
        checks += 1;
    }
    for (pointer, replacement) in [
        ("/version", json!(1)),
        ("/version", json!(3)),
        ("/version", json!(null)),
        ("/version", json!("2")),
        ("/host", json!("remote")),
        ("/originalProvider", json!("codex")),
        ("/input/provider", json!("claude")),
        ("/nativeFamily", json!("cursor-composer-v1")),
        ("/originalNormalizerVersion", json!("1.0.1")),
        ("/currentNormalizerVersion", json!("1.0.7")),
        ("/originalInstallation", json!("local:obsolete")),
        ("/input/sourceId", json!(Uuid::new_v4().to_string())),
        ("/originalNative", value["input"]["nativeSessionId"].clone()),
        (
            "/input/nativeSessionId",
            json!("cursor-identity-v1:transcript-locator:bad"),
        ),
        ("/originalLocator", json!("cursor-transcript:foreign.jsonl")),
        ("/input/legacyId", json!("cursor-transcript:foreign.jsonl")),
        ("/anchorDigest", json!("bad")),
        ("/input/digest", json!("A".repeat(64))),
        ("/receiptCutoff", json!(0)),
        ("/expectedRevision", json!("invalid")),
        ("/anchorReceipt", json!("invalid")),
        ("/importId", json!(Uuid::new_v4().to_string())),
        (
            "/association/confirmation/importId",
            json!(format!("imp:{}", Uuid::new_v4())),
        ),
        (
            "/association/confirmation/expectedRevision",
            json!(Uuid::new_v4().to_string()),
        ),
        ("/association/jobId", json!("invalid")),
        ("/association/planId", json!("invalid")),
        ("/association/requestId", json!("")),
        ("/association/planHash", json!("bad")),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            ProofRecord::parse(&changed.to_string()).is_err(),
            "shape {pointer}"
        );
        checks += 1;
    }
    for field in [
        "candidate",
        "source",
        "provider",
        "native",
        "locator",
        "digest",
    ] {
        let mut changed = input.clone();
        match field {
            "candidate" => changed.candidate_id = "foreign-candidate".into(),
            "source" => changed.source_id = Uuid::new_v4().to_string(),
            "provider" => changed.provider = "claude".into(),
            "native" => changed.native_session_id = "foreign-native".into(),
            "locator" => changed.legacy_id = "cursor-transcript:foreign.jsonl".into(),
            "digest" => changed.digest = "0".repeat(64),
            _ => unreachable!(),
        }
        assert!(
            AssociationProof::draft(&witness, association.clone(), changed, job, encoded).is_err(),
            "input {field}"
        );
        checks += 1;
    }
    for field in ["job", "request", "plan", "hash", "confirmation", "preview"] {
        let mut changed = association.clone();
        match field {
            "job" => changed.job_id = Uuid::new_v4().to_string(),
            "request" => changed.request_id = "foreign-request".into(),
            "plan" => changed.plan_id = Uuid::new_v4().to_string(),
            "hash" => changed.plan_hash = "0".repeat(64),
            "confirmation" => changed.confirmation.review_digest = "0".repeat(64),
            "preview" => changed.confirmation.preview_id = Uuid::new_v4().to_string(),
            _ => unreachable!(),
        }
        assert!(
            AssociationProof::draft(&witness, changed, input.clone(), job, encoded).is_err(),
            "context {field}"
        );
        checks += 1;
    }

    let conn = open_catalog().unwrap();
    let before_receipts = receipts(&conn);
    let before_entry =
        serde_json::to_value(catalog::get_entry(&conn, IMPORT).unwrap().unwrap()).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    assert!(!proof_store::verify_schema(&tx).unwrap());
    tx.execute_batch(proof_store::SQL).unwrap();
    tx.execute(
        "INSERT INTO meta(key,value) VALUES(?1,'1')",
        [proof_store::VERSION_KEY],
    )
    .unwrap();
    // Even a well-formed, digest-correct protocol draft is not accepted proof.
    tx.execute(
        "INSERT INTO registration_provenance(import_id,json,digest) VALUES(?1,?2,?3)",
        rusqlite::params![IMPORT, raw, record.digest().unwrap()],
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&proof_store::get(&tx, IMPORT).unwrap().unwrap()).unwrap(),
        raw
    );
    let old_entry = catalog::get_entry(&tx, IMPORT).unwrap().unwrap();
    assert_eq!(old_entry.installation_id, witness.receipt.installation_id);
    assert_eq!(
        old_entry.native_session_id,
        witness.receipt.native_session_id
    );
    assert!(registration_provenance::authorize(&tx, &old_entry, &witness.receipt).is_err());
    checks += 1;
    // Shape-valid stale/context mutations need actual acceptance, not codec
    // heuristics. They remain readable records, but never authorize a reader.
    for (pointer, replacement) in [
        ("/anchorReceipt", json!(Uuid::new_v4().to_string())),
        ("/anchorDigest", json!("0".repeat(64))),
        ("/receiptCutoff", json!(witness.receipt_cutoff + 1)),
        ("/input/digest", json!("0".repeat(64))),
        ("/input/candidateId", json!("foreign-candidate")),
        ("/association/jobId", json!(Uuid::new_v4().to_string())),
        ("/association/requestId", json!("foreign-request")),
        ("/association/planId", json!(Uuid::new_v4().to_string())),
        ("/association/planHash", json!("0".repeat(64))),
        (
            "/association/confirmation/reviewDigest",
            json!("0".repeat(64)),
        ),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        let altered = ProofRecord::parse(&changed.to_string()).unwrap();
        tx.execute(
            "UPDATE registration_provenance SET json=?1,digest=?2 WHERE import_id=?3",
            rusqlite::params![
                serde_json::to_string(&altered).unwrap(),
                altered.digest().unwrap(),
                IMPORT
            ],
        )
        .unwrap();
        assert!(proof_store::get(&tx, IMPORT).unwrap().is_some());
        assert!(
            registration_provenance::authorize(&tx, &old_entry, &witness.receipt).is_err(),
            "unaccepted {pointer}"
        );
        checks += 1;
    }
    tx.rollback().unwrap();
    assert!(!proof_store::verify_schema(&conn).unwrap());
    assert_eq!(receipts(&conn), before_receipts);
    assert_eq!(
        serde_json::to_value(catalog::get_entry(&conn, IMPORT).unwrap().unwrap()).unwrap(),
        before_entry
    );
    assert_eq!(
        witness.review().unwrap(),
        identity_association::inspect(&request).unwrap()
    );
    checks + 1
}
