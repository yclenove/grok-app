//! Synthetic codec/producer primitives only; V3 normal acceptance stays fenced.
//! Every key and input belongs to an isolated fixture, never a user account.
use super::*;
use crate::session_migration::{
    publication_protocol::PublicationProtocol, test_home::IsolatedAppHome,
};
use serde_json::{json, Value};

#[path = "job_publication_allocation_tests.rs"]
mod allocation_tests;
#[path = "job_publication_acceptance_codec_tests.rs"]
mod codec_tests;
#[path = "job_publication_acceptance_fixture.rs"]
mod fixture;
#[path = "job_publication_acceptance_journal_tests.rs"]
mod journal_tests;
#[path = "job_publication_acceptance_producer_tests.rs"]
mod producer_tests;

fn header() -> Header {
    Header {
        version: VERSION,
        outcome_version: 1,
        job_id: "11111111-1111-8111-8111-111111111111".into(),
        context_digest: "a".repeat(64),
        plan_id: "22222222-2222-4222-8222-222222222222".into(),
        plan_hash: "b".repeat(64),
        plan_digest: "c".repeat(64),
        scope_digest: "d".repeat(64),
        member_count: 1,
        members_digest: "e".repeat(64),
    }
}

fn member(header: &VerifiedHeader) -> Member {
    Member {
        version: VERSION,
        job_id: header.body.job_id.clone(),
        header_digest: header.digest.clone(),
        candidate_digest: "f".repeat(64),
        input_digest: "0".repeat(64),
        allocation_digest: "1".repeat(64),
    }
}

#[test]
fn acceptance_codec_canonical_bytes_membership_and_separate_domains() {
    let _home = IsolatedAppHome::acquire("acceptance-canonical");
    let auth = InstallationAuthenticator::load().unwrap();
    let encoded = seal(&auth, HEADER_DOMAIN, header()).unwrap();
    assert!(encoded == seal(&auth, HEADER_DOMAIN, header()).unwrap());
    let verified = verify_header(&auth, encoded.as_bytes(), &header().job_id).unwrap();
    assert!(verified.digest == hash::sha256_bytes(encoded.as_bytes()));
    let pretty =
        serde_json::to_string_pretty(&serde_json::from_str::<Value>(&encoded).unwrap()).unwrap();
    assert!(
        verify_header(&auth, pretty.as_bytes(), &header().job_id)
            .unwrap()
            .digest
            == verified.digest
    );
    let wrong_domain = seal(&auth, MEMBER_DOMAIN, header()).unwrap();
    assert!(verify_header(&auth, wrong_domain.as_bytes(), &header().job_id).is_err());
    let encoded = seal(&auth, MEMBER_DOMAIN, member(&verified)).unwrap();
    let parsed = verify_member(&auth, encoded.as_bytes(), &verified, &"f".repeat(64)).unwrap();
    assert!(parsed.body.input_digest == "0".repeat(64));
    let wrong_domain = seal(&auth, HEADER_DOMAIN, member(&verified)).unwrap();
    assert!(verify_member(&auth, wrong_domain.as_bytes(), &verified, &"f".repeat(64)).is_err());
    let mut a = std::collections::BTreeMap::new();
    a.insert("b".repeat(64), "c".repeat(64));
    a.insert("a".repeat(64), "d".repeat(64));
    let mut b = std::collections::BTreeMap::new();
    b.insert("a".repeat(64), "d".repeat(64));
    b.insert("b".repeat(64), "c".repeat(64));
    assert_eq!(record::members_digest(&a), record::members_digest(&b));
    b.remove(&"a".repeat(64));
    assert_ne!(record::members_digest(&a), record::members_digest(&b));
}

#[test]
fn acceptance_codec_every_binding_rejects_unsigned_edits_and_cross_context_replay() {
    let _home = IsolatedAppHome::acquire("acceptance-bindings");
    let auth = InstallationAuthenticator::load().unwrap();
    let encoded = seal(&auth, HEADER_DOMAIN, header()).unwrap();
    for field in [
        "contextDigest",
        "planId",
        "planHash",
        "planDigest",
        "scopeDigest",
        "memberCount",
        "membersDigest",
        "jobId",
    ] {
        let mut changed: Value = serde_json::from_str(&encoded).unwrap();
        changed["body"][field] = match field {
            "memberCount" => json!(2),
            "jobId" => json!(PublicationProtocol::SignedOutcomeV1.new_job_id()),
            "planId" => json!(uuid::Uuid::new_v4().to_string()),
            _ => json!("9".repeat(64)),
        };
        assert!(
            verify_header(&auth, changed.to_string().as_bytes(), &header().job_id).is_err(),
            "{field}"
        );
    }
    let verified = verify_header(&auth, encoded.as_bytes(), &header().job_id).unwrap();
    let encoded = seal(&auth, MEMBER_DOMAIN, member(&verified)).unwrap();
    for field in [
        "jobId",
        "headerDigest",
        "candidateDigest",
        "inputDigest",
        "allocationDigest",
    ] {
        let mut changed: Value = serde_json::from_str(&encoded).unwrap();
        changed["body"][field] = json!(if field == "jobId" {
            PublicationProtocol::SignedOutcomeV1.new_job_id()
        } else {
            "9".repeat(64)
        });
        assert!(
            verify_member(
                &auth,
                changed.to_string().as_bytes(),
                &verified,
                &"f".repeat(64)
            )
            .is_err(),
            "{field}"
        );
    }
    assert!(verify_member(&auth, encoded.as_bytes(), &verified, &"9".repeat(64)).is_err());
    let mut other = header();
    other.scope_digest = "9".repeat(64);
    let other = seal(&auth, HEADER_DOMAIN, other).unwrap();
    let other = verify_header(&auth, other.as_bytes(), &header().job_id).unwrap();
    assert!(verify_member(&auth, encoded.as_bytes(), &other, &"f".repeat(64)).is_err());
}

#[test]
fn acceptance_codec_strict_shape_duplicate_unknown_missing_type_and_versions() {
    let _home = IsolatedAppHome::acquire("acceptance-shape");
    let auth = InstallationAuthenticator::load().unwrap();
    let encoded = seal(&auth, HEADER_DOMAIN, header()).unwrap();
    for (needle, replacement) in [
        ("\"body\":", "\"body\":null,\"body\":"),
        ("\"mac\":", "\"mac\":null,\"mac\":"),
        ("\"version\":1", "\"version\":1,\"version\":1"),
        ("\"scopeDigest\":", "\"scopeDigest\":null,\"scopeDigest\":"),
    ] {
        assert!(encoded.contains(needle));
        assert!(verify_header(
            &auth,
            encoded.replacen(needle, replacement, 1).as_bytes(),
            &header().job_id
        )
        .is_err());
    }
    let original: Value = serde_json::from_str(&encoded).unwrap();
    for level in ["", "body"] {
        let fields: Vec<String> = if level.is_empty() {
            original.as_object().unwrap()
        } else {
            original[level].as_object().unwrap()
        }
        .keys()
        .cloned()
        .collect();
        for field in fields {
            let mut changed = original.clone();
            let object = if level.is_empty() {
                &mut changed
            } else {
                &mut changed[level]
            };
            object.as_object_mut().unwrap().remove(&field);
            assert!(
                verify_header(&auth, changed.to_string().as_bytes(), &header().job_id).is_err()
            );
        }
        let mut changed = original.clone();
        let object = if level.is_empty() {
            &mut changed
        } else {
            &mut changed[level]
        };
        object["unknown"] = json!(true);
        assert!(verify_header(&auth, changed.to_string().as_bytes(), &header().job_id).is_err());
    }
    for field in ["version", "outcomeVersion"] {
        for (version, expected) in [
            (0, ErrorCode::SchemaUnsupported),
            (2, ErrorCode::ImportSchemaTooNew),
        ] {
            let mut changed = original.clone();
            changed["body"][field] = json!(version);
            assert_eq!(
                verify_header(&auth, changed.to_string().as_bytes(), &header().job_id)
                    .err()
                    .unwrap()
                    .code,
                expected
            );
        }
    }
    for (field, value) in [
        ("version", json!("1")),
        ("memberCount", json!(1.0)),
        ("memberCount", json!(0)),
        ("scopeDigest", json!("D".repeat(64))),
        ("jobId", json!(uuid::Uuid::new_v4().to_string())),
        ("planId", json!("opaque-plan")),
    ] {
        let mut changed = original.clone();
        changed["body"][field] = value;
        assert!(verify_header(&auth, changed.to_string().as_bytes(), &header().job_id).is_err());
    }
}

#[test]
fn acceptance_codec_member_strict_shape_and_metadata_byte_budget() {
    let _home = IsolatedAppHome::acquire("acceptance-member-shape");
    let auth = InstallationAuthenticator::load().unwrap();
    let h = seal(&auth, HEADER_DOMAIN, header()).unwrap();
    let h = verify_header(&auth, h.as_bytes(), &header().job_id).unwrap();
    let encoded = seal(&auth, MEMBER_DOMAIN, member(&h)).unwrap();
    let original: Value = serde_json::from_str(&encoded).unwrap();
    for field in [
        "version",
        "jobId",
        "headerDigest",
        "candidateDigest",
        "inputDigest",
        "allocationDigest",
    ] {
        let needle = format!("\"{field}\":");
        let changed = encoded.replacen(&needle, &format!("{needle}null,{needle}"), 1);
        assert!(verify_member(&auth, changed.as_bytes(), &h, &"f".repeat(64)).is_err());
        let mut changed = original.clone();
        changed["body"].as_object_mut().unwrap().remove(field);
        assert!(verify_member(&auth, changed.to_string().as_bytes(), &h, &"f".repeat(64)).is_err());
    }
    let mut future = original.clone();
    future["body"]["version"] = json!(2);
    assert_eq!(
        verify_member(&auth, future.to_string().as_bytes(), &h, &"f".repeat(64))
            .err()
            .unwrap()
            .code,
        ErrorCode::ImportSchemaTooNew
    );
    let mut unknown = original.clone();
    unknown["body"]["unknown"] = json!(true);
    assert!(verify_member(&auth, unknown.to_string().as_bytes(), &h, &"f".repeat(64)).is_err());
    for raw in [
        vec![],
        vec![0xff],
        vec![b' '; RECORD_BYTES + 1],
        [vec![0xef, 0xbb, 0xbf], encoded.as_bytes().to_vec()].concat(),
    ] {
        assert!(verify_header(&auth, &raw, &header().job_id).is_err());
        assert!(verify_member(&auth, &raw, &h, &"f".repeat(64)).is_err());
    }
}
