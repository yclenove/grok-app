//! Real atomic native-store primitives with synthetic signed allocations.
//! This is not the normal registered V3 acceptance/publish/Undo pipeline.
use super::*;
use crate::store::{self, import_publication_projects as native, Project};
use std::fs;

const ORIGIN: &str = "importPublicationOrigin";

fn allocated(auth: &InstallationAuthenticator, path: &std::path::Path) -> VerifiedRecord {
    let mut binding = binding();
    binding.job_id =
        crate::session_migration::publication_protocol::PublicationProtocol::SignedOutcomeV1
            .new_job_id();
    let allocation = allocation(ProjectAllocation::Create {
        project_id: uuid::Uuid::new_v4().to_string(),
        path_digest: sha256_bytes(path.to_str().unwrap().as_bytes()),
        allow_unassigned: false,
    });
    verified(auth, &seal(auth, binding, allocation).unwrap())
}

fn read_rows() -> Value {
    serde_json::from_slice(&fs::read(crate::paths::projects_file()).unwrap()).unwrap()
}
fn write_rows(value: &Value) {
    crate::store_lock::write_bytes_replace(
        &crate::paths::projects_file(),
        &serde_json::to_vec(value).unwrap(),
    )
    .unwrap();
}
fn assert_store_unchanged(expected: &[u8]) {
    // Never render signed origin envelopes (including MACs) on a failure.
    assert!(fs::read(crate::paths::projects_file()).unwrap() == expected);
}
fn project_rows() -> Vec<Project> {
    store::load_projects_snapshot().unwrap()
}
fn target(home: &IsolatedAppHome, name: &str) -> std::path::PathBuf {
    let target = home.path.join(name);
    fs::create_dir(&target).unwrap();
    target
}

#[test]
fn native_creation_atomic_origin_recovery_and_exact_removal_keep_directory() {
    let home = IsolatedAppHome::acquire("native-origin-recovery");
    let path = target(&home, "target");
    let marker = path.join("keep.txt");
    fs::write(&marker, b"synthetic project content").unwrap();
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    let created = native::ensure(&creation, path.to_str().unwrap()).unwrap();
    assert_eq!(created.disposition, native::Disposition::Created);
    assert!(!created.project.trusted);
    let rows = read_rows();
    assert!(rows[0][ORIGIN].as_str().is_some());
    let original = fs::read(crate::paths::projects_file()).unwrap();
    // The first store call and all its local values are gone. Recover from the
    // actual durable origin, not receipt.created_project or UUID coincidence.
    let recovered = native::ensure(
        &allocation.project_creation(&auth).unwrap(),
        path.to_str().unwrap(),
    )
    .unwrap();
    assert_eq!(recovered.disposition, native::Disposition::Recovered);
    assert_eq!(recovered.project.id, created.project.id);
    assert_store_unchanged(&original);
    // The ordinary/older removal path still sees an unknown field and refuses.
    assert!(store::remove_import_project(&recovered.project).is_err());
    native::remove(&creation, &recovered.project).unwrap();
    assert!(project_rows().is_empty());
    assert_eq!(fs::read(marker).unwrap(), b"synthetic project content");
}

#[test]
fn native_creation_same_id_without_origin_is_match_never_recovered_or_annotated() {
    let home = IsolatedAppHome::acquire("native-origin-same-id");
    let path = target(&home, "existing");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    let existing = store::ensure_import_project(creation.project_id(), path.to_str().unwrap())
        .unwrap()
        .project;
    let original = fs::read(crate::paths::projects_file()).unwrap();
    let matched = native::ensure(&creation, path.to_str().unwrap()).unwrap();
    assert_eq!(matched.disposition, native::Disposition::Matched);
    assert_eq!(matched.project.id, existing.id);
    assert!(native::remove(&creation, &existing).is_err());
    assert_store_unchanged(&original);
    assert!(read_rows()[0].get(ORIGIN).is_none());
}

#[test]
fn native_creation_same_path_and_canonical_alias_are_matches_with_exact_fields() {
    let home = IsolatedAppHome::acquire("native-origin-same-path");
    let path = target(&home, "existing");
    let existing = store::add_project(path.to_string_lossy().into_owned(), false).unwrap();
    let mut rows = read_rows();
    rows[0]["futureSettings"] = json!({"keep":true});
    rows[0]["spaces"] = json!(["synthetic-space"]);
    write_rows(&rows);
    let original = fs::read(crate::paths::projects_file()).unwrap();
    let auth = InstallationAuthenticator::load().unwrap();
    for path in [path.clone(), path.join(".")] {
        let allocation = allocated(&auth, &path);
        let creation = allocation.project_creation(&auth).unwrap();
        let matched = native::ensure(&creation, path.to_str().unwrap()).unwrap();
        assert_eq!(matched.disposition, native::Disposition::Matched);
        assert_eq!(matched.project.id, existing.id);
        assert!(native::remove(&creation, &matched.project).is_err());
        assert_store_unchanged(&original);
    }
}

#[test]
fn native_creation_foreign_or_changed_allocation_cannot_claim_existing_origin() {
    let home = IsolatedAppHome::acquire("native-origin-other-allocation");
    let path = target(&home, "target");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    let created = native::ensure(&creation, path.to_str().unwrap()).unwrap();
    let original = fs::read(crate::paths::projects_file()).unwrap();
    let mut other_binding = allocation.body.binding.clone();
    other_binding.candidate_digest = "9".repeat(64);
    let other = verified(
        &auth,
        &seal(&auth, other_binding, allocation.body.stage.clone()).unwrap(),
    );
    let other = other.project_creation(&auth).unwrap();
    let matched = native::ensure(&other, path.to_str().unwrap()).unwrap();
    assert_eq!(matched.disposition, native::Disposition::Matched);
    assert!(native::remove(&other, &created.project).is_err());
    assert_store_unchanged(&original);
    let foreign_path = target(&home, "foreign");
    let foreign = store::add_project(foreign_path.to_string_lossy().into_owned(), false).unwrap();
    let before = fs::read(crate::paths::projects_file()).unwrap();
    assert!(native::remove(&creation, &foreign).is_err());
    assert_store_unchanged(&before);
}

#[test]
fn native_creation_origin_survives_native_edits_and_unknown_customization_blocks_removal() {
    let home = IsolatedAppHome::acquire("native-origin-unknown");
    let path = target(&home, "target");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    let created = native::ensure(&creation, path.to_str().unwrap()).unwrap();
    let origin = read_rows()[0][ORIGIN].clone();
    store::rename_project(&created.project.id, "User changed name").unwrap();
    assert!(read_rows()[0][ORIGIN] == origin);
    let current = project_rows().remove(0);
    assert!(native::remove(&creation, &current).is_err());
    store::rename_project(&created.project.id, &created.project.name).unwrap();
    let mut rows = read_rows();
    rows[0]["futureSettings"] = json!({"keep":true});
    write_rows(&rows);
    let before = fs::read(crate::paths::projects_file()).unwrap();
    let current = project_rows().remove(0);
    assert!(native::remove(&creation, &current)
        .unwrap_err()
        .contains("additional settings"));
    assert_store_unchanged(&before);
    assert!(read_rows()[0][ORIGIN] == origin);
}

#[test]
fn native_creation_all_typed_user_customization_is_preserved_even_with_fresh_snapshot() {
    let home = IsolatedAppHome::acquire("native-origin-customization");
    let path = target(&home, "target");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    native::ensure(&creation, path.to_str().unwrap()).unwrap();
    let original = read_rows();
    for (field, value) in [
        ("name", json!("User name")),
        ("path", json!(home.path.join("moved").to_str().unwrap())),
        ("trusted", json!(true)),
        ("pinned", json!(true)),
        ("system", json!(true)),
        ("modelId", json!("synthetic-model")),
        ("effort", json!("high")),
        ("mode", json!("ask")),
        ("permissionPolicy", json!("deny")),
        ("sandboxProfile", json!("workspace")),
        ("color", json!("blue")),
        ("sshAlias", json!("synthetic-host")),
    ] {
        let mut rows = original.clone();
        rows[0][field] = value;
        write_rows(&rows);
        // Give removal the exact *current* row: snapshot equality alone must
        // not promote user-customized data into producer-owned creation.
        let current = project_rows().remove(0);
        let before = fs::read(crate::paths::projects_file()).unwrap();
        assert!(native::remove(&creation, &current).is_err(), "{field}");
        assert_store_unchanged(&before);
        assert!(read_rows()[0][ORIGIN] == original[0][ORIGIN]);
    }
    write_rows(&original);
    native::remove(&creation, &project_rows().remove(0)).unwrap();
    assert!(project_rows().is_empty());
    assert!(path.is_dir());
}

#[test]
fn native_creation_derived_health_and_open_time_do_not_invent_user_customization() {
    let home = IsolatedAppHome::acquire("native-origin-derived");
    let path = target(&home, "target");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    native::ensure(&creation, path.to_str().unwrap()).unwrap();
    let mut rows = read_rows();
    rows[0]["pathOk"] = json!(false);
    rows[0]["lastOpenedAt"] = json!("2000-01-01T00:00:00Z");
    write_rows(&rows);
    let before = fs::read(crate::paths::projects_file()).unwrap();
    let recovered = native::ensure(&creation, path.to_str().unwrap()).unwrap();
    assert_eq!(recovered.disposition, native::Disposition::Recovered);
    assert_store_unchanged(&before);
    native::remove(&creation, &recovered.project).unwrap();
    assert!(path.is_dir());
}

#[test]
fn native_creation_corrupt_missing_future_and_copied_origin_never_repaired() {
    let home = IsolatedAppHome::acquire("native-origin-corrupt");
    let path = target(&home, "target");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    let created = native::ensure(&creation, path.to_str().unwrap()).unwrap();
    let original = read_rows();
    let origin: Value = serde_json::from_str(original[0][ORIGIN].as_str().unwrap()).unwrap();
    for fault in [
        "missing",
        "null",
        "object",
        "oversized",
        "future",
        "duplicate",
        "unknown",
        "mac",
        "snapshot",
        "copied-id",
    ] {
        let mut rows = original.clone();
        let mut value = origin.clone();
        match fault {
            "missing" => {
                rows[0].as_object_mut().unwrap().remove(ORIGIN);
            }
            "null" => rows[0][ORIGIN] = Value::Null,
            "object" => rows[0][ORIGIN] = value.clone(),
            "oversized" => rows[0][ORIGIN] = json!(" ".repeat(8193)),
            "duplicate" => {
                rows[0][ORIGIN] = json!(original[0][ORIGIN].as_str().unwrap().replacen(
                    "\"version\":1",
                    "\"version\":1,\"version\":1",
                    1
                ))
            }
            field => {
                match field {
                    "future" => value["body"]["version"] = json!(2),
                    "unknown" => value["body"]["unknown"] = json!(true),
                    "mac" => value["mac"] = json!("0".repeat(64)),
                    "snapshot" => value["body"]["snapshotDigest"] = json!("9".repeat(64)),
                    "copied-id" => {
                        value["body"]["projectId"] = json!(uuid::Uuid::new_v4().to_string())
                    }
                    _ => unreachable!(),
                }
                rows[0][ORIGIN] = json!(value.to_string());
            }
        }
        write_rows(&rows);
        let before = fs::read(crate::paths::projects_file()).unwrap();
        let result = native::ensure(&creation, path.to_str().unwrap());
        if fault == "missing" {
            assert_eq!(result.unwrap().disposition, native::Disposition::Matched);
        } else {
            assert!(result.is_err(), "{fault}");
        }
        assert!(
            native::remove(&creation, &created.project).is_err(),
            "{fault}"
        );
        assert_store_unchanged(&before);
    }
}

#[test]
fn native_creation_failed_atomic_replace_leaves_no_new_project_or_origin() {
    let home = IsolatedAppHome::acquire("native-origin-failed-replace");
    let existing_path = target(&home, "existing");
    store::add_project(existing_path.to_string_lossy().into_owned(), false).unwrap();
    let path = target(&home, "target");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    let before = fs::read(crate::paths::projects_file()).unwrap();
    let temporary = crate::paths::projects_file().with_file_name("projects.json.tmp");
    fs::create_dir(&temporary).unwrap();
    assert!(native::ensure(&creation, path.to_str().unwrap()).is_err());
    assert_store_unchanged(&before);
    assert!(!project_rows()
        .iter()
        .any(|row| row.id == creation.project_id()));
    fs::remove_dir(temporary).unwrap();
    assert_eq!(
        native::ensure(&creation, path.to_str().unwrap())
            .unwrap()
            .disposition,
        native::Disposition::Created
    );
}

#[test]
fn native_creation_path_binding_and_existing_id_target_conflicts_are_noops() {
    let home = IsolatedAppHome::acquire("native-origin-path-binding");
    let path = target(&home, "expected");
    let other = target(&home, "other");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    store::ensure_import_project(creation.project_id(), other.to_str().unwrap()).unwrap();
    let before = fs::read(crate::paths::projects_file()).unwrap();
    assert!(native::ensure(&creation, other.to_str().unwrap()).is_err());
    assert!(native::ensure(&creation, path.to_str().unwrap()).is_err());
    assert_store_unchanged(&before);
}

#[test]
fn native_creation_retries_under_concurrent_store_writers_have_one_atomic_creator() {
    let home = IsolatedAppHome::acquire("native-origin-concurrent");
    let path = target(&home, "target");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let creation = allocation.project_creation(&auth).unwrap();
                    native::ensure(&creation, path.to_str().unwrap()).unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        results
            .iter()
            .filter(|row| row.disposition == native::Disposition::Created)
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|row| row.disposition == native::Disposition::Recovered)
            .count(),
        7
    );
    assert_eq!(project_rows().len(), 1);
    assert!(read_rows()[0][ORIGIN].is_string());
}

#[test]
fn native_creation_loaded_crypto_operates_with_key_loader_blocked() {
    let home = IsolatedAppHome::acquire("native-origin-loaded-crypto");
    let path = target(&home, "target");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    let key = home.path.join("imports/.cursor-hmac-secret");
    let retained = home.path.join("retained-synthetic-key");
    fs::rename(&key, &retained).unwrap();
    fs::create_dir(&key).unwrap();
    // Evict the loader's path cache using another synthetic home, without a
    // private cache-reset API. The isolated guard still owns environment state.
    let alternate = home.path.join("alternate-synthetic-home");
    std::env::set_var("GROK_APP_HOME", &alternate);
    InstallationAuthenticator::load().unwrap();
    std::env::set_var("GROK_APP_HOME", &home.path);
    assert!(InstallationAuthenticator::load().is_err());
    let created = native::ensure(&creation, path.to_str().unwrap()).unwrap();
    assert_eq!(created.disposition, native::Disposition::Created);
    let before = fs::read(crate::paths::projects_file()).unwrap();
    let recovered = native::ensure(&creation, path.to_str().unwrap()).unwrap();
    assert_eq!(recovered.disposition, native::Disposition::Recovered);
    assert_store_unchanged(&before);
    native::remove(&creation, &recovered.project).unwrap();
    assert!(project_rows().is_empty());
    assert!(InstallationAuthenticator::load().is_err());
    assert!(key.is_dir());
    assert!(fs::read_dir(key).unwrap().next().is_none());
    assert!(retained.is_file());
}

#[test]
fn native_creation_project_references_and_invalid_indexes_keep_owned_row() {
    let home = IsolatedAppHome::acquire("native-origin-references");
    let path = target(&home, "target");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocation = allocated(&auth, &path);
    let creation = allocation.project_creation(&auth).unwrap();
    let created = native::ensure(&creation, path.to_str().unwrap()).unwrap();
    let workspace = crate::workspace_store::upsert_workspace(
        None,
        "Synthetic owned reference".into(),
        created.project.id.clone(),
        vec![],
    )
    .unwrap();
    let before = fs::read(crate::paths::projects_file()).unwrap();
    assert!(native::remove(&creation, &created.project)
        .unwrap_err()
        .contains("workspaces"));
    crate::workspace_store::delete_workspace(&workspace.id).unwrap();
    let _ = store::load_settings();
    let session = store::create_session(
        Some(created.project.id.clone()),
        Some("Synthetic native reference".into()),
        false,
    )
    .unwrap();
    assert!(native::remove(&creation, &created.project)
        .unwrap_err()
        .contains("sessions"));
    assert_store_unchanged(&before);
    store::delete_session(&session.id).unwrap();
    for value in ["broken", "[{}]", "[{\"projectId\":42}]"] {
        fs::write(crate::paths::sessions_index_file(), value).unwrap();
        assert!(native::remove(&creation, &created.project).is_err());
        assert_store_unchanged(&before);
    }
    fs::write(crate::paths::sessions_index_file(), "[]").unwrap();
    native::remove(&creation, &created.project).unwrap();
    assert!(project_rows().is_empty());
}

#[test]
fn native_creation_token_rejects_legacy_match_stage_and_other_installation() {
    let sealed = {
        let home = IsolatedAppHome::acquire("native-origin-first-installation");
        let path = target(&home, "target");
        let auth = InstallationAuthenticator::load().unwrap();
        let allocation = allocated(&auth, &path);
        let ordinary = seal(&auth, binding(), allocation.body.stage.clone()).unwrap();
        assert!(verified(&auth, &ordinary).project_creation(&auth).is_err());
        let matched = seal(
            &auth,
            allocation.body.binding.clone(),
            super::allocation(ProjectAllocation::Match {
                project_id: "legacy-project".into(),
                path_digest: "0".repeat(64),
                allow_unassigned: false,
            }),
        )
        .unwrap();
        assert!(verified(&auth, &matched).project_creation(&auth).is_err());
        let stored = seal(
            &auth,
            allocation.body.binding.clone(),
            super::stored(
                &allocation,
                created(match &allocation.body.stage {
                    Stage::Allocated(Allocation {
                        project: ProjectAllocation::Create { project_id, .. },
                        ..
                    }) => project_id,
                    _ => unreachable!(),
                }),
            ),
        )
        .unwrap();
        assert!(verified(&auth, &stored).project_creation(&auth).is_err());
        allocation
    };
    let _home = IsolatedAppHome::acquire("native-origin-second-installation");
    let other_auth = InstallationAuthenticator::load().unwrap();
    assert!(sealed.project_creation(&other_auth).is_err());
}
