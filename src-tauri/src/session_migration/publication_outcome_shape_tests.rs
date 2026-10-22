use super::*;

#[test]
fn outcome_codec_unknown_missing_and_wrong_types_never_become_defaults() {
    let _home = IsolatedAppHome::acquire("outcome-strict-shape");
    let auth = InstallationAuthenticator::load().unwrap();
    let record = seal(&auth, binding(), allocation(create(false))).unwrap();
    let original: Value = serde_json::from_str(&record.encoded).unwrap();
    for pointer in [
        "",
        "/body",
        "/body/binding",
        "/body/stage/allocated",
        "/body/stage/allocated/project/create",
    ] {
        let mut value = original.clone();
        value.pointer_mut(pointer).unwrap()["authorized"] = json!(true);
        corrupt(&auth, &record, &value);
    }
    for (pointer, fields) in [
        ("", vec!["body", "mac"]),
        ("/body", vec!["version", "binding", "stage"]),
        (
            "/body/binding",
            vec![
                "jobId",
                "contextDigest",
                "planId",
                "planHash",
                "planDigest",
                "acceptanceDigest",
                "candidateDigest",
                "inputDigest",
            ],
        ),
        (
            "/body/stage/allocated",
            vec![
                "receiptId",
                "importId",
                "revisionId",
                "previousRevision",
                "createdEntry",
                "project",
            ],
        ),
        (
            "/body/stage/allocated/project/create",
            vec!["projectId", "pathDigest", "allowUnassigned"],
        ),
    ] {
        for field in fields {
            let mut value = original.clone();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            corrupt(&auth, &record, &value);
            for wrong in [Value::Null, json!([]), json!({}), json!(1.0)] {
                // null is the deliberate representation of previousRevision.
                if field == "previousRevision" && wrong.is_null() {
                    continue;
                }
                let mut value = original.clone();
                value.pointer_mut(pointer).unwrap()[field] = wrong;
                corrupt(&auth, &record, &value);
            }
        }
    }
}

#[test]
fn outcome_codec_every_original_acceptance_binding_is_sealed() {
    let _home = IsolatedAppHome::acquire("outcome-acceptance-bindings");
    let auth = InstallationAuthenticator::load().unwrap();
    let record = seal(&auth, binding(), allocation(create(false))).unwrap();
    let original: Value = serde_json::from_str(&record.encoded).unwrap();
    for field in [
        "jobId",
        "contextDigest",
        "planId",
        "planHash",
        "planDigest",
        "acceptanceDigest",
        "candidateDigest",
        "inputDigest",
    ] {
        let mut value = original.clone();
        value["body"]["binding"][field] = json!(if field.ends_with("Id") {
            OTHER.into()
        } else {
            "9".repeat(64)
        });
        corrupt(&auth, &record, &value);
        // Even coordinating the expected plain binding cannot forge the MAC.
        let coordinated: Binding =
            serde_json::from_value(value["body"]["binding"].clone()).unwrap();
        assert!(verify(
            &auth,
            value.to_string().as_bytes(),
            &coordinated,
            StageKind::Allocated
        )
        .is_err());
    }
}

#[test]
fn outcome_codec_all_allocation_and_store_results_are_sealed() {
    let _home = IsolatedAppHome::acquire("outcome-result-bindings");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocated = seal(&auth, binding(), allocation(create(false))).unwrap();
    let a = verified(&auth, &allocated);
    let stored = seal(&auth, binding(), stored(&a, created(PROJECT))).unwrap();
    let s = verified(&auth, &stored);
    let committed = seal(&auth, binding(), committed(&s)).unwrap();
    for (record, changes) in [
        (
            &allocated,
            vec![
                ("/body/stage/allocated/receiptId", json!(OTHER)),
                (
                    "/body/stage/allocated/importId",
                    json!(format!("imp:{OTHER}")),
                ),
                ("/body/stage/allocated/revisionId", json!(OTHER)),
                ("/body/stage/allocated/previousRevision", json!(OTHER)),
                ("/body/stage/allocated/createdEntry", json!(false)),
                (
                    "/body/stage/allocated/project/create/projectId",
                    json!(OTHER),
                ),
                (
                    "/body/stage/allocated/project/create/pathDigest",
                    json!("9".repeat(64)),
                ),
                (
                    "/body/stage/allocated/project/create/allowUnassigned",
                    json!(true),
                ),
            ],
        ),
        (
            &stored,
            vec![
                ("/body/stage/stored/allocationDigest", json!("9".repeat(64))),
                (
                    "/body/stage/stored/linkedReceiptDigest",
                    json!("9".repeat(64)),
                ),
                ("/body/stage/stored/project/created/projectId", json!(OTHER)),
                (
                    "/body/stage/stored/project/created/pathDigest",
                    json!("9".repeat(64)),
                ),
                (
                    "/body/stage/stored/project/created/snapshotDigest",
                    json!("9".repeat(64)),
                ),
            ],
        ),
        (
            &committed,
            vec![
                ("/body/stage/committed/storeDigest", json!("9".repeat(64))),
                ("/body/stage/committed/receiptDigest", json!("9".repeat(64))),
                ("/body/stage/committed/itemDigest", json!("9".repeat(64))),
            ],
        ),
    ] {
        let original: Value = serde_json::from_str(&record.encoded).unwrap();
        for (pointer, replacement) in changes {
            let mut changed = original.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            corrupt(&auth, record, &changed);
        }
    }
}

#[test]
fn outcome_codec_link_requires_exact_allocation_binding_and_phase() {
    let _home = IsolatedAppHome::acquire("outcome-linked-identity");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocated = seal(&auth, binding(), allocation(create(false))).unwrap();
    let a = verified(&auth, &allocated);
    let mut other_allocation = allocation(create(false));
    if let Stage::Allocated(row) = &mut other_allocation {
        row.receipt_id = OTHER.into();
    }
    let another = verified(&auth, &seal(&auth, binding(), other_allocation).unwrap());
    let stored = seal(&auth, binding(), stored(&another, created(PROJECT))).unwrap();
    let s = verified(&auth, &stored);
    assert!(s.follows(&a).is_err());
    let mut different = binding();
    different.candidate_digest = "9".repeat(64);
    let different = verified(
        &auth,
        &seal(&auth, different, super::stored(&a, created(PROJECT))).unwrap(),
    );
    assert!(different.follows(&a).is_err());
    let c = verified(&auth, &seal(&auth, binding(), committed(&s)).unwrap());
    c.follows(&s).unwrap();
    assert!(c.follows(&another).is_err());
}

#[test]
fn outcome_codec_create_intent_does_not_prove_foreign_created_result() {
    let _home = IsolatedAppHome::acquire("outcome-project-identity");
    let auth = InstallationAuthenticator::load().unwrap();
    for (project, rejected) in [
        (create(false), created(OTHER)),
        (create(false), ProjectResult::UnassignedReported {}),
        (create(true), ProjectResult::Unassigned {}),
        (ProjectAllocation::Unassigned {}, created(PROJECT)),
        (
            ProjectAllocation::Match {
                project_id: PROJECT.into(),
                path_digest: "0".repeat(64),
                allow_unassigned: false,
            },
            created(PROJECT),
        ),
        (
            ProjectAllocation::Match {
                project_id: PROJECT.into(),
                path_digest: "0".repeat(64),
                allow_unassigned: false,
            },
            matched(OTHER),
        ),
    ] {
        let a = verified(&auth, &seal(&auth, binding(), allocation(project)).unwrap());
        let s = verified(
            &auth,
            &seal(&auth, binding(), stored(&a, rejected)).unwrap(),
        );
        assert!(s.follows(&a).is_err());
    }
    let a = verified(
        &auth,
        &seal(&auth, binding(), allocation(create(false))).unwrap(),
    );
    let mut wrong_path = matched(OTHER);
    if let ProjectResult::Matched { path_digest, .. } = &mut wrong_path {
        *path_digest = "9".repeat(64);
    }
    let s = verified(
        &auth,
        &seal(&auth, binding(), stored(&a, wrong_path)).unwrap(),
    );
    assert!(s.follows(&a).is_err());
}

#[test]
fn outcome_codec_fixed_v1_canonical_body_and_phase_domains() {
    let _home = IsolatedAppHome::acquire("outcome-v1-wire-fixture");
    let auth = InstallationAuthenticator::load().unwrap();
    let record = seal(
        &auth,
        binding(),
        allocation(ProjectAllocation::Unassigned {}),
    )
    .unwrap();
    // Pinned from an independently constructed, sorted ASCII JSON fixture.
    // No installation-specific MAC is recorded or printed.
    assert_eq!(
        sha256_bytes(canonical(&record.body).unwrap().as_bytes()),
        "70837416cb0fb54c65884390e62e1423c220d027978925a2d2e9d2c3b32eaab5"
    );
    assert_eq!(
        StageKind::Allocated.domain(),
        "session-import-publication-allocation-v1"
    );
    assert_eq!(
        StageKind::Stored.domain(),
        "session-import-publication-store-result-v1"
    );
    assert_eq!(
        StageKind::Committed.domain(),
        "session-import-publication-committed-v1"
    );
    assert_eq!(StageKind::Allocated.key(), "allocated");
    assert_eq!(StageKind::Stored.key(), "stored");
    assert_eq!(StageKind::Committed.key(), "committed");
}

#[test]
fn outcome_codec_invalid_ids_digests_and_allocation_relationships_cannot_be_sealed() {
    let _home = IsolatedAppHome::acquire("outcome-invalid-producer-shape");
    let auth = InstallationAuthenticator::load().unwrap();
    for raw in [
        "",
        "system:general",
        "../escape",
        "00000000-0000-0000-0000-000000000000",
        "urn:uuid:11111111-1111-4111-8111-111111111111",
    ] {
        let changed = Binding {
            job_id: raw.into(),
            ..binding()
        };
        assert!(seal(&auth, changed, allocation(create(false))).is_err());
    }
    for raw in [
        "",
        &"0".repeat(63),
        &"0".repeat(65),
        &"A".repeat(64),
        &"g".repeat(64),
    ] {
        let changed = Binding {
            input_digest: raw.into(),
            ..binding()
        };
        assert!(seal(&auth, changed, allocation(create(false))).is_err());
    }
    for (created_entry, previous) in [(true, OTHER), (false, REVISION), (false, "invalid")] {
        let mut changed = allocation(create(false));
        if let Stage::Allocated(row) = &mut changed {
            row.created_entry = created_entry;
            row.previous_revision = Some(previous.into());
        }
        assert!(seal(&auth, binding(), changed).is_err());
    }
    for id in ["", "system:general", "bad\nrow", &"a".repeat(257)] {
        let project = ProjectAllocation::Match {
            project_id: id.into(),
            path_digest: "0".repeat(64),
            allow_unassigned: false,
        };
        assert!(seal(&auth, binding(), allocation(project)).is_err());
    }
}

#[test]
fn outcome_codec_mac_installation_and_domain_are_not_interchangeable() {
    let record = {
        let _home = IsolatedAppHome::acquire("outcome-first-installation");
        let auth = InstallationAuthenticator::load().unwrap();
        let record = seal(&auth, binding(), allocation(create(false))).unwrap();
        for domain in [
            "published-verification-v1",
            StageKind::Stored.domain(),
            StageKind::Committed.domain(),
        ] {
            let mut changed: Value = serde_json::from_str(&record.encoded).unwrap();
            changed["mac"] = json!(hex::encode(
                auth.sign(domain, canonical(&record.body).unwrap().as_bytes())
            ));
            corrupt(&auth, &record, &changed);
        }
        for mac in [
            "",
            "0",
            &"0".repeat(63),
            &"0".repeat(65),
            &"Z".repeat(64),
            &"0".repeat(64),
        ] {
            let mut changed: Value = serde_json::from_str(&record.encoded).unwrap();
            changed["mac"] = json!(mac);
            corrupt(&auth, &record, &changed);
        }
        record
    };
    let _home = IsolatedAppHome::acquire("outcome-second-installation");
    let other = InstallationAuthenticator::load().unwrap();
    assert!(verify(
        &other,
        record.encoded.as_bytes(),
        &binding(),
        StageKind::Allocated
    )
    .is_err());
}
