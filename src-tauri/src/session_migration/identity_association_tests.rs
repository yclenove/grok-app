//! Portable protocol checks. Format literals do not claim historical production.
//! Actual archive authorization is covered by the opt-in, pinned V243 replay.
use super::*;
use serde_json::json;

fn request() -> AssociationRequest {
    AssociationRequest {
        version: 1,
        import_id: format!("imp:{}", uuid::Uuid::new_v4()),
        expected_revision: uuid::Uuid::new_v4().to_string(),
        preview: PreviewSelection {
            preview_id: uuid::Uuid::new_v4().to_string(),
            leaf_id: "cursor:tx:1".into(),
            selection: super::super::selection::AllowedSelection::for_events(vec![
                "cursor:tx:1".into()
            ]),
        },
    }
}

fn confirmation() -> AssociationConfirmation {
    let request = request();
    AssociationConfirmation {
        version: 1,
        import_id: request.import_id,
        expected_revision: request.expected_revision,
        preview_id: request.preview.preview_id,
        review_digest: "a".repeat(64),
    }
}

#[test]
fn association_confirmation_wire_is_strict_and_not_a_grant() {
    let original = serde_json::to_value(confirmation()).unwrap();
    for field in [
        "version",
        "importId",
        "expectedRevision",
        "previewId",
        "reviewDigest",
    ] {
        let mut value = original.clone();
        value.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<AssociationConfirmation>(value).is_err());
        let mut value = original.clone();
        value[field] = Value::Null;
        assert!(serde_json::from_value::<AssociationConfirmation>(value).is_err());
    }
    for field in [
        "authorized",
        "path",
        "nativeSessionId",
        "receipt",
        "selection",
    ] {
        let mut value = original.clone();
        value[field] = json!(true);
        assert!(serde_json::from_value::<AssociationConfirmation>(value).is_err());
    }
}

#[test]
fn association_confirmation_requires_known_version_canonical_ids_and_digest() {
    let original = confirmation();
    original.validate().unwrap();
    for field in [
        "import",
        "revision",
        "preview",
        "digest-length",
        "digest-upper",
        "digest-space",
        "digest-unicode",
    ] {
        let mut value = original.clone();
        match field {
            "import" => value.import_id = format!(" {} ", value.import_id),
            "revision" => value.expected_revision = format!("{{{}}}", value.expected_revision),
            "preview" => value.preview_id = "../lease.json".into(),
            "digest-length" => value.review_digest = "a".repeat(63),
            "digest-upper" => value.review_digest = "A".repeat(64),
            "digest-space" => value.review_digest = format!("{} ", "a".repeat(63)),
            "digest-unicode" => value.review_digest = "é".repeat(32),
            _ => unreachable!(),
        }
        assert!(value.validate().is_err(), "{field}");
    }
    let mut value = original;
    value.version = 0;
    assert_eq!(
        value.validate().unwrap_err().code,
        ErrorCode::SchemaUnsupported
    );
    value.version = 2;
    assert_eq!(
        value.validate().unwrap_err().code,
        ErrorCode::ImportSchemaTooNew
    );
}

#[test]
fn association_plan_request_requires_full_explicit_wire_scope() {
    let original = serde_json::to_value(AssociationPlanRequest {
        version: 1,
        preview_ids: vec![],
        mappings: vec![],
        selection_set: super::super::selection::SelectionSet::single(request().preview),
        identity_associations: vec![confirmation()],
    })
    .unwrap();
    for field in [
        "version",
        "previewIds",
        "mappings",
        "selectionSet",
        "identityAssociations",
    ] {
        let mut value = original.clone();
        value.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<AssociationPlanRequest>(value).is_err());
        let mut value = original.clone();
        value[field] = Value::Null;
        assert!(serde_json::from_value::<AssociationPlanRequest>(value).is_err());
    }
    let mut value = original;
    value["authorized"] = json!(true);
    assert!(serde_json::from_value::<AssociationPlanRequest>(value).is_err());
}

#[test]
fn association_record_binding_requires_unique_targets_covered_previews_and_nonempty_v2() {
    use super::super::selection::SelectionSet;
    let mut one = request().preview;
    let mut two = one.clone();
    two.preview_id = uuid::Uuid::new_v4().to_string();
    let mut confirmation = confirmation();
    confirmation.preview_id = one.preview_id.clone();
    let selections = SelectionSet {
        entries: vec![one.clone(), two.clone()],
        ..Default::default()
    };
    let now = chrono::Utc::now();
    let valid = json!({"version":2, "identityAssociations":[confirmation.clone()],
        "planId":uuid::Uuid::new_v4().to_string(), "created":now.to_rfc3339(),
        "expiry":(now + chrono::Duration::seconds(30)).to_rfc3339()});
    let record = |confirmations: Value| {
        let mut value = valid.clone();
        value["identityAssociations"] = confirmations;
        value
    };
    assert_eq!(
        parse_record(&valid, 2, &selections).unwrap(),
        vec![confirmation.clone()]
    );
    for extra in [
        json!(null),
        json!([]),
        json!({}),
        json!([confirmation.clone(), confirmation.clone()]),
    ] {
        assert!(parse_record(&record(extra), 2, &selections).is_err());
    }
    let mut same_target = confirmation.clone();
    same_target.preview_id = two.preview_id.clone();
    assert!(parse_record(
        &record(json!([confirmation.clone(), same_target])),
        2,
        &selections
    )
    .is_err());
    let mut foreign = confirmation.clone();
    foreign.preview_id = uuid::Uuid::new_v4().to_string();
    assert!(parse_record(&record(json!([foreign])), 2, &selections).is_err());
    let mut same_preview = confirmation.clone();
    same_preview.import_id = format!("imp:{}", uuid::Uuid::new_v4());
    assert!(parse_record(&record(json!([confirmation, same_preview])), 2, &selections).is_err());
    one.selection
        .event_ids
        .push(one.selection.event_ids[0].clone());
    assert!(parse_record(&valid, 2, &SelectionSet::single(one)).is_err());
    assert!(parse_record(&json!({}), 2, &selections).is_err());
    assert!(parse_record(&valid, 1, &selections).is_err());
    assert!(parse_record(&valid, 3, &selections).is_err());
}

#[test]
fn association_payload_keeps_v1_hash_and_binds_v2_identity_lifetime_and_raw_order() {
    let original = json!({"version":1,"selectionSet":request().preview});
    assert_eq!(
        bind_payload(original.clone(), &[], "unused", "unused", "unused"),
        original
    );
    let confirmation = confirmation();
    let bound = bind_payload(
        original.clone(),
        std::slice::from_ref(&confirmation),
        "plan",
        "created",
        "expiry",
    );
    let digest = hash::options_hash(&bound);
    for (id, created, expiry) in [
        ("changed", "created", "expiry"),
        ("plan", "changed", "expiry"),
        ("plan", "created", "changed"),
    ] {
        assert_ne!(
            hash::options_hash(&bind_payload(
                original.clone(),
                std::slice::from_ref(&confirmation),
                id,
                created,
                expiry
            )),
            digest
        );
    }
    let mut changed = confirmation;
    changed.review_digest = "b".repeat(64);
    assert_ne!(
        hash::options_hash(&bind_payload(
            original,
            &[changed],
            "plan",
            "created",
            "expiry"
        )),
        digest
    );
}

#[test]
fn association_request_validates_raw_scope_without_canonicalizing_duplicates() {
    let mut value = request();
    value.validate().unwrap();
    value.preview.selection.event_ids.push("cursor:tx:1".into());
    assert_eq!(
        value.validate().unwrap_err().code,
        ErrorCode::HistoryAccessUnavailable
    );
}

#[test]
fn association_request_requires_canonical_opaque_ids_before_filesystem_access() {
    let original = request();
    for field in ["import", "revision", "preview"] {
        let mut value = original.clone();
        match field {
            "import" => value.import_id = format!(" {} ", value.import_id),
            "revision" => value.expected_revision = format!("{{{}}}", value.expected_revision),
            "preview" => value.preview.preview_id = "../manifest.json".into(),
            _ => unreachable!(),
        }
        assert_eq!(inspect(&value).unwrap_err().code, ErrorCode::PlanStale);
    }
}

#[test]
fn association_request_old_and_future_versions_fail_closed() {
    let mut value = request();
    value.version = 0;
    assert_eq!(
        inspect(&value).unwrap_err().code,
        ErrorCode::SchemaUnsupported
    );
    value.version = 2;
    assert_eq!(
        inspect(&value).unwrap_err().code,
        ErrorCode::ImportSchemaTooNew
    );
}

#[test]
fn association_async_command_preserves_fail_closed_domain_errors() {
    let mut value = request();
    value.version = 2;
    let expected = inspect(&value).unwrap_err().into_command_err();
    let actual = tauri::async_runtime::block_on(
        crate::commands::session_import_identity_association_inspect(value),
    )
    .unwrap_err();
    assert_eq!(actual, expected);
}

#[test]
fn association_wire_rejects_missing_unknown_wrong_type_and_grant_fields() {
    let original = serde_json::to_value(request()).unwrap();
    for name in ["version", "importId", "expectedRevision", "preview"] {
        let mut value = original.clone();
        value.as_object_mut().unwrap().remove(name);
        assert!(serde_json::from_value::<AssociationRequest>(value).is_err());
    }
    for name in [
        "authorized",
        "sourceId",
        "nativeSessionId",
        "receipt",
        "path",
    ] {
        let mut value = original.clone();
        value[name] = json!(true);
        assert!(serde_json::from_value::<AssociationRequest>(value).is_err());
    }
    let mut value = original;
    value["version"] = json!("1");
    assert!(serde_json::from_value::<AssociationRequest>(value).is_err());
}

#[test]
fn association_manifest_compatibility_is_an_explicit_format_pair() {
    let value = json!({"schemaVersion":1,"normalizerVersion":"1.0.0",
        "evidence":{"adapterVersion":"1.0.0","provider":"cursor","schemaFingerprint":"cursor-transcript-jsonl-v1"}});
    manifest_version(&value, "1.0.0").unwrap();
    for pointer in [
        "/schemaVersion",
        "/normalizerVersion",
        "/evidence/adapterVersion",
        "/evidence/provider",
        "/evidence/schemaFingerprint",
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = Value::Null;
        assert_eq!(
            manifest_version(&changed, "1.0.0").unwrap_err().code,
            ErrorCode::FormatUnsupported
        );
    }
    assert_eq!(
        manifest_version(&value, "1.0.6").unwrap_err().code,
        ErrorCode::FormatUnsupported
    );
}

#[test]
fn registered_location_validation_requires_and_reuses_snapshot_in_both_journal_modes() {
    for wal in [false, true] {
        location_snapshot(wal);
    }
}

fn location_snapshot(wal: bool) {
    use crate::session_migration::{source_locations, test_home::IsolatedAppHome, types::Provider};
    let home = IsolatedAppHome::acquire("association-location-snapshot");
    let root = home.path.join("synthetic cursor");
    std::fs::create_dir_all(&root).unwrap();
    let writer = catalog::open_catalog().unwrap();
    if wal {
        // Only this disposable fixture uses WAL. Never change the product's
        // chosen journal mode to make a concurrency test easier to pass.
        let mode: String = writer
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    } else {
        let mode: String = writer
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
    }
    let source = sources::register(&writer, Provider::Cursor, &root, None, "synthetic").unwrap();
    let reader = catalog::open_catalog_readonly().unwrap();
    let location = source_locations::page(
        &reader,
        &source.source_id,
        Provider::Cursor,
        "local",
        None,
        Some(1),
    )
    .unwrap()
    .locations
    .remove(0);
    assert_eq!(
        source_locations::validate_registered_with(&reader, &location)
            .unwrap_err()
            .code,
        ErrorCode::SnapshotUnavailable
    );
    let snapshot = reader.unchecked_transaction().unwrap();
    source_locations::validate_registered_with(&snapshot, &location).unwrap();
    let update = "UPDATE sources SET host='ssh:synthetic' WHERE installation_id=?1";
    let concurrent = writer.execute(update, [&source.source_id]);
    if wal {
        assert_eq!(concurrent.unwrap(), 1);
    } else {
        assert_eq!(
            concurrent.unwrap_err().sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy)
        );
    }
    // WAL allows a committed writer, whereas rollback mode blocks it. In both
    // cases the already-established caller snapshot remains coherent.
    source_locations::validate_registered_with(&snapshot, &location).unwrap();
    assert_eq!(reader.total_changes(), 0);
    drop(snapshot);
    if !wal {
        assert_eq!(writer.execute(update, [&source.source_id]).unwrap(), 1);
    }
    let fresh = reader.unchecked_transaction().unwrap();
    assert_eq!(
        source_locations::validate_registered_with(&fresh, &location)
            .unwrap_err()
            .code,
        ErrorCode::SourceNotFound
    );
    assert_eq!(reader.total_changes(), 0);
}
