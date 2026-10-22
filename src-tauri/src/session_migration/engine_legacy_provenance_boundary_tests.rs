//! Production-path boundaries around current-schema historical registrations.
//! All data is synthetic; these are not vendor or old-normalizer replay claims.
use super::*;
use crate::session_migration::{catalog, job_control, sources, workspace, workspace_association};
use rusqlite::params;

#[test]
fn strict_proof_dispatch_keeps_real_v1_bytes_and_digest() {
    use crate::session_migration::registration_provenance::{Proof, ProofRecord};
    use crate::session_migration::registration_provenance_store as proof_store;
    let home = IsolatedAppHome::acquire("legacy-proof-wire-compatibility");
    let (seed, _, _, _) = accept_legacy(cursor_source(&home, 1));
    let conn = open_catalog().unwrap();
    let (raw, digest): (String, String) = conn
        .query_row(
            "SELECT json,digest FROM registration_provenance WHERE import_id=?1",
            [&seed.import_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let legacy: Proof = serde_json::from_str(&raw).unwrap();
    let parsed = ProofRecord::parse(&raw).unwrap();
    assert!(matches!(parsed, ProofRecord::Legacy(_)));
    assert_eq!(serde_json::to_string(&legacy).unwrap(), raw);
    assert_eq!(serde_json::to_string(&parsed).unwrap(), raw);
    assert_eq!(legacy.digest().unwrap(), digest);
    assert_eq!(parsed.digest().unwrap(), digest);
    assert_eq!(
        serde_json::to_string(&proof_store::get(&conn, &seed.import_id).unwrap().unwrap()).unwrap(),
        raw
    );
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for version in [
        json!(null),
        json!("1"),
        json!(-1),
        json!(0),
        json!(2),
        json!(3),
    ] {
        let mut changed = value.clone();
        changed["version"] = version;
        assert!(ProofRecord::parse(&changed.to_string()).is_err());
    }
    for key in value.as_object().unwrap().keys() {
        let mut changed = value.clone();
        changed.as_object_mut().unwrap().remove(key);
        assert!(
            ProofRecord::parse(&changed.to_string()).is_err(),
            "missing {key}"
        );
        for invalid in [json!(null), json!([])] {
            let mut wrong_type = value.clone();
            wrong_type[key] = invalid;
            assert!(
                ProofRecord::parse(&wrong_type.to_string()).is_err(),
                "type {key}"
            );
        }
        let duplicate = format!("{{\"{key}\":{},{}", value[key], &raw[1..]);
        assert!(ProofRecord::parse(&duplicate).is_err(), "duplicate {key}");
    }
    for key in value["input"].as_object().unwrap().keys() {
        let mut changed = value.clone();
        changed["input"].as_object_mut().unwrap().remove(key);
        assert!(
            ProofRecord::parse(&changed.to_string()).is_err(),
            "missing input {key}"
        );
    }
    for field in ["authorized", "input.authorized"] {
        let mut changed = value.clone();
        if field == "authorized" {
            changed[field] = json!(true);
        } else {
            changed["input"]["authorized"] = json!(true);
        }
        assert!(
            ProofRecord::parse(&changed.to_string()).is_err(),
            "unknown {field}"
        );
    }
    assert!(ProofRecord::parse(&format!("{raw}{}", " ".repeat(32769 - raw.len()))).is_err());
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn matching_current_identity_cannot_ignore_an_unaccepted_new_proof_protocol() {
    use crate::session_migration::registration_provenance as provenance;
    let home = IsolatedAppHome::acquire("legacy-proof-current-protocol-refusal");
    let (seed, accepted, _, _) = accept_legacy(cursor_source(&home, 1));
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let conn = open_catalog().unwrap();
    let entry = catalog::get_entry(&conn, &seed.import_id).unwrap().unwrap();
    let receipt = jobs::latest_receipt(&conn, &accepted.job_id, &accepted.items[0].candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(entry.installation_id, receipt.installation_id);
    assert_eq!(entry.native_session_id, receipt.native_session_id);
    let original: (String, String) = conn
        .query_row(
            "SELECT json,digest FROM registration_provenance WHERE import_id=?1",
            [&seed.import_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    provenance::authorize(&conn, &entry, &receipt).unwrap();
    for version in [2, 3] {
        // A deliberately unaccepted/corrupt row is not an accepted proof. Even
        // matching current receipt identity must not bypass protocol checks.
        let mut value: serde_json::Value = serde_json::from_str(&original.0).unwrap();
        value["version"] = json!(version);
        let digest = crate::session_migration::hash::options_hash(&value);
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(
            "UPDATE registration_provenance SET json=?1,digest=?2 WHERE import_id=?3",
            params![value.to_string(), digest, seed.import_id],
        )
        .unwrap();
        let authorized = provenance::authorize(&tx, &entry, &receipt);
        tx.rollback().unwrap();
        assert!(
            authorized.is_err(),
            "unaccepted proof version {version} cannot take the current-identity fast path"
        );
    }
    let after: (String, String) = conn
        .query_row(
            "SELECT json,digest FROM registration_provenance WHERE import_id=?1",
            [&seed.import_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(after, original);
    provenance::authorize(&conn, &entry, &receipt).unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn proof_bound_metadata_lookup_does_not_load_json_or_digest_bodies() {
    use crate::session_migration::registration_provenance_store as proof_store;
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(proof_store::SQL).unwrap();
    let payload_columns = |sql: &str| -> Vec<(i64, i64)> {
        conn.prepare(&format!("EXPLAIN {sql}"))
            .unwrap()
            .query_map(["synthetic-absent"], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(3)?, // P2 is the source column; P3 is the output register.
                    row.get::<_, i64>(6)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .filter_map(|(opcode, column, flags)| {
                (opcode == "Column" && matches!(column, 1 | 2)).then_some((column, flags))
            })
            .collect()
    };
    let columns = payload_columns(proof_store::ROW_METADATA_SQL);
    assert_eq!(columns.len(), 2);
    // SQLite OPFLAG_TYPEOFARG=128 reads the storage type, not the payload.
    assert!(columns.iter().all(|(_, flags)| flags & 128 != 0));
    let body_columns =
        payload_columns("SELECT json,digest FROM registration_provenance WHERE import_id=?1");
    assert_eq!(body_columns.len(), 2);
    assert!(body_columns.iter().all(|(_, flags)| flags & 128 == 0));
}

#[test]
fn unreadable_proof_row_is_not_the_same_as_an_absent_proof() {
    use crate::session_migration::registration_provenance_store as proof_store;
    let home = IsolatedAppHome::acquire("legacy-proof-unreadable-row");
    let conn = open_catalog().unwrap();
    assert!(proof_store::get(&conn, "absent-before-extension")
        .unwrap()
        .is_none());
    assert_no_provenance_extension();
    let (seed, _, _, _) = accept_legacy(cursor_source(&home, 1));
    let (raw, digest): (String, String) = conn
        .query_row(
            "SELECT json,digest FROM registration_provenance WHERE import_id=?1",
            [&seed.import_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(proof_store::get(&conn, "absent-after-extension")
        .unwrap()
        .is_none());
    let boundary = format!("{raw}{}", " ".repeat(32768 - raw.len()));
    conn.execute(
        "UPDATE registration_provenance SET json=?1 WHERE import_id=?2",
        params![boundary, seed.import_id],
    )
    .unwrap();
    assert_eq!(
        proof_store::get(&conn, &seed.import_id)
            .unwrap()
            .unwrap()
            .digest()
            .unwrap(),
        digest
    );
    conn.execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    for (json, changed_digest) in [
        (format!("{boundary} "), digest.clone()),
        (raw.clone(), "0".repeat(63)),
        (raw.clone(), "0".repeat(65)),
        (raw.clone(), "0".repeat(100_000)),
    ] {
        conn.execute(
            "UPDATE registration_provenance SET json=?1,digest=?2 WHERE import_id=?3",
            params![json, changed_digest, seed.import_id],
        )
        .unwrap();
        assert_eq!(
            proof_store::get(&conn, &seed.import_id).unwrap_err().code,
            ErrorCode::SchemaUnsupported,
            "a present unreadable row must not be reported absent"
        );
        let unchanged: (String, String) = conn
            .query_row(
                "SELECT json,digest FROM registration_provenance WHERE import_id=?1",
                [&seed.import_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(unchanged, (json, changed_digest));
        assert!(events_window(
            &seed.import_id,
            Some(&seed.revision),
            None,
            Some(20),
            None,
            None
        )
        .is_err());
        assert_archive_unchanged(&seed);
    }
    conn.execute(
        "UPDATE registration_provenance SET json=?1,digest=?2 WHERE import_id=?3",
        params![raw, digest, seed.import_id],
    )
    .unwrap();
    conn.execute_batch("PRAGMA ignore_check_constraints=OFF;")
        .unwrap();
    assert!(proof_store::get(&conn, &seed.import_id).unwrap().is_some());
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

fn accept(pending: &PlanState) -> Result<JobRecord, ImportError> {
    commit(
        &pending.plan_id,
        &pending.plan_hash,
        &Uuid::new_v4().to_string(),
    )
}

fn stored_rows(conn: &rusqlite::Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn historical_locator_spellings_preserve_adapter_family_and_opaque_ids() {
    for prefix in [
        "",
        "codex:",
        "claude:",
        "cursor-transcript:",
        "cursor-store:",
        "file:",
    ] {
        for (ordinary, verbatim) in [
            (r"C:\synthetic\history", r"\\?\C:\synthetic\history"),
            (
                r"\\synthetic\share\history",
                r"\\?\UNC\synthetic\share\history",
            ),
        ] {
            let mut expected = vec![format!("{prefix}{ordinary}"), format!("{prefix}{verbatim}")];
            expected.sort();
            for spelling in [ordinary, verbatim] {
                let mut actual = sources::locator_spellings(&format!("{prefix}{spelling}"));
                actual.sort();
                assert_eq!(actual, expected);
            }
        }
    }
    for opaque in [
        r"unknown:C:\synthetic\history",
        "cursor-composer:synthetic-id",
        "codex:synthetic-id",
    ] {
        assert_eq!(sources::locator_spellings(opaque), [opaque]);
    }
}

#[test]
fn in_flight_legacy_publication_refuses_adoption_until_publisher_returns() {
    use std::cell::Cell;
    for after_undo in [false, true] {
        let home = IsolatedAppHome::acquire("legacy-publication-overlap");
        let previews = cursor_source(&home, 1);
        let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
        let producer_options = json!({"rawRetention":"normalized-only"});
        let (delayed, item) = queue_registration(
            &previews[0],
            seed.installation.clone(),
            false,
            producer_options.clone(),
        );
        let canonical = resident(&previews[0]).source_id;
        let pending = plan(previews, vec![]).unwrap();
        let conn = open_catalog().unwrap();
        assert_ne!(
            jobs::latest_receipt_for_revision(&conn, &seed.import_id, &seed.revision)
                .unwrap()
                .unwrap()
                .options_hash,
            crate::session_migration::hash::options_hash(&producer_options)
        );
        let probes = Cell::new(0);
        let refused = Cell::new(false);
        let request = Uuid::new_v4().to_string();
        // A real caller cannot adopt while the existing publisher owns its locks.
        let published = jobs::publish_item(&conn, &delayed, &item, &|| {
            let probe = probes.get() + 1;
            probes.set(probe);
            if probe == 2 {
                assert_eq!(
                    jobs::latest_receipt(&conn, &delayed, &item.candidate_id)
                        .unwrap()
                        .unwrap()
                        .phase,
                    "prepared"
                );
                let counts = table_counts();
                let error = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap_err();
                assert_eq!(error.code, ErrorCode::SourceBusy);
                assert_eq!(entry_identity(&seed).0, seed.installation);
                assert_eq!(table_counts(), counts);
                assert_no_provenance_extension();
                refused.set(true);
            }
            false
        })
        .unwrap();
        assert!(refused.get());
        assert_eq!(published.phase, "committed");
        assert_eq!(entry_identity(&seed).0, seed.installation);
        assert_archive_unchanged(&seed);
        // Record the rightful publisher through its actual recovery worker.
        job_control::run_pending().unwrap();
        reset_runtime();
        assert_eq!(job_get(&delayed).unwrap().state, JobState::Completed);
        // The exact unaccepted request/plan remains explicitly retryable.
        let accepted = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap();
        assert_eq!(entry_identity(&seed).0, canonical);
        assert_eq!(
            commit(&pending.plan_id, &pending.plan_hash, &request)
                .unwrap()
                .job_id,
            accepted.job_id
        );
        if after_undo {
            // Undo the winning refresh, then its seed, only after lock release.
            assert_eq!(
                jobs::undo_job(&conn, &delayed).unwrap().items[0].phase,
                "undone"
            );
            assert_eq!(
                jobs::undo_job(&conn, &seed.job_id).unwrap().items[0].phase,
                "undone"
            );
        }
        job_control::run_pending().unwrap();
        reset_runtime();
        let entry = get_entry(&open_catalog().unwrap(), &seed.import_id)
            .unwrap()
            .unwrap();
        assert_eq!(entry.installation_id, canonical);
        assert_eq!(table_counts().0, 1);
        if after_undo {
            assert!(entry.hidden);
            assert!(catalog::is_tombstoned(
                &open_catalog().unwrap(),
                &canonical,
                "cursor",
                &entry.native_session_id
            )
            .unwrap());
            assert!(events_window(&seed.import_id, None, None, Some(20), None, None).is_err());
        } else {
            let completed = job_get(&accepted.job_id).unwrap();
            assert_eq!(completed.state, JobState::Completed);
            assert_eq!(entry.current_revision, completed.items[0].revision_id);
            events_window(
                &seed.import_id,
                Some(&seed.revision),
                None,
                Some(20),
                None,
                None,
            )
            .unwrap();
        }
        assert_archive_unchanged(&seed);
        reset_runtime();
    }
}

#[cfg(windows)]
#[test]
fn distinct_locator_spellings_reject_the_whole_adoption() {
    let home = IsolatedAppHome::acquire("legacy-cross-spelling");
    let previews = cursor_source(&home, 1);
    let fixed = resident(&previews[0]);
    let path = fixed.legacy_id.strip_prefix("cursor-transcript:").unwrap();
    let locators: Vec<_> = sources::locator_spellings(path)
        .into_iter()
        .map(|path| format!("cursor-transcript:{path}"))
        .collect();
    assert_eq!(locators.len(), 2, "ordinary and verbatim local path");
    let seeds: Vec<_> = locators
        .iter()
        .map(|locator| {
            seed_registration(
                &previews[0],
                format!(
                    "local:{}",
                    crate::session_migration::hash::sha256_bytes(locator.as_bytes())
                ),
            )
        })
        .collect();
    assert_ne!(seeds[0].import_id, seeds[1].import_id);
    let before: Vec<_> = seeds.iter().map(entry_identity).collect();
    let counts = table_counts();
    let pending = plan(previews, vec![]).unwrap();
    let error = accept(&pending).unwrap_err();
    assert_eq!(error.code, ErrorCode::MappingConflict);
    assert!(error
        .message
        .contains("legacy locators map to different imports"));
    assert_eq!(table_counts(), counts);
    assert_no_provenance_extension();
    reset_runtime();
    assert_eq!(seeds.iter().map(entry_identity).collect::<Vec<_>>(), before);
    for seed in &seeds {
        events_window(
            &seed.import_id,
            Some(&seed.revision),
            None,
            Some(20),
            None,
            None,
        )
        .unwrap();
        assert_archive_unchanged(seed);
    }
}

#[test]
fn foreign_registration_preflight_never_adopts_or_initializes_proof() {
    let home = IsolatedAppHome::acquire("legacy-foreign-preflight");
    let previews = cursor_source(&home, 1);
    let registered = resident(&previews[0]).source_id;
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let before = entry_identity(&seed);
    let pending = plan(previews, vec![]).unwrap();
    let counts = table_counts();
    let conn = open_catalog().unwrap();
    for installation in [&registered, &seed.installation] {
        for (provider, host) in [("codex", "local"), ("cursor", "ssh:synthetic")] {
            conn.execute(
                "UPDATE sources SET provider=?1,host=?2 WHERE installation_id=?3",
                params![provider, host, installation],
            )
            .unwrap();
            let error = accept(&pending).unwrap_err();
            assert!(
                matches!(
                    error.code,
                    ErrorCode::MappingConflict
                        | ErrorCode::HistoryAccessUnavailable
                        | ErrorCode::SourceNotFound
                ),
                "{error:?}"
            );
            assert_eq!(entry_identity(&seed), before);
            assert_eq!(table_counts(), counts);
            assert_no_provenance_extension();
            assert_archive_unchanged(&seed);
            conn.execute(
                "UPDATE sources SET provider='cursor',host='local' WHERE installation_id=?1",
                [installation],
            )
            .unwrap();
        }
    }
    accept(&pending).unwrap();
    job_control::run_pending().unwrap();
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[cfg(windows)]
#[test]
fn real_lease_replacement_failure_rolls_back_adoption_then_retries_or_expires() {
    use std::os::windows::fs::OpenOptionsExt;
    for retry in [false, true] {
        let home = IsolatedAppHome::acquire("legacy-retention-rollback");
        let previews = cursor_source(&home, 1);
        let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
        let before = entry_identity(&seed);
        let path = resident(&previews[0]).prepared.path.clone();
        let lease_path = path.join("lease.json");
        let pending = plan(previews, vec![]).unwrap();
        // Planning legitimately renews expiry. Freeze bytes after that operation.
        let original = std::fs::read(&lease_path).unwrap();
        let counts = table_counts();
        // Permit reading but deny the real replacement rename, after preflight.
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&lease_path)
            .unwrap();
        let error = accept(&pending).unwrap_err();
        drop(locked);
        assert_eq!(error.code, ErrorCode::SnapshotUnavailable, "{error:?}");
        assert_eq!(std::fs::read(&lease_path).unwrap(), original);
        assert_eq!(entry_identity(&seed), before);
        assert_eq!(table_counts(), counts);
        assert_no_provenance_extension();
        assert_archive_unchanged(&seed);
        assert!(!std::fs::read_dir(&path).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lease-")));
        if retry {
            let accepted = accept(&pending).unwrap();
            job_control::run_pending().unwrap();
            assert_eq!(
                job_get(&accepted.job_id).unwrap().state,
                JobState::Completed
            );
            events_window(
                &seed.import_id,
                Some(&seed.revision),
                None,
                Some(20),
                None,
                None,
            )
            .unwrap();
        } else {
            jobs::expire_preview_lease_file(&path).unwrap();
            reset_runtime();
            jobs::cleanup_unreferenced_preview_dirs().unwrap();
            assert!(
                !path.exists(),
                "failed acceptance has no durable retention owner"
            );
        }
        assert_archive_unchanged(&seed);
        reset_runtime();
    }
}

#[test]
fn adopted_proof_and_job_inputs_survive_rebuild_but_future_schema_is_refused() {
    let home = IsolatedAppHome::acquire("legacy-proof-rebuild");
    let (seed, accepted, _, _) = accept_legacy(cursor_source(&home, 1));
    job_control::run_pending().unwrap();
    let conn = open_catalog().unwrap();
    let identity = entry_identity(&seed);
    let current = get_entry(&conn, &seed.import_id)
        .unwrap()
        .unwrap()
        .current_revision
        .unwrap();
    let current_hashes =
        jobs::published_hashes(&catalog::revision_dir(&seed.import_id, &current).unwrap()).unwrap();
    let queries = [
        "SELECT json||digest FROM registration_provenance ORDER BY import_id",
        "SELECT job_id||candidate_id||result_json FROM job_items ORDER BY job_id,candidate_id",
        "SELECT installation_id||provider||COALESCE(profile,'')||COALESCE(host,'')||root_path FROM sources ORDER BY installation_id",
    ];
    let before: Vec<_> = queries.iter().map(|sql| stored_rows(&conn, sql)).collect();
    let counts = table_counts();
    catalog::rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap();
    reset_runtime();
    assert_eq!(entry_identity(&seed), identity);
    assert_eq!(table_counts(), counts);
    for (index, sql) in queries.iter().enumerate() {
        assert_eq!(stored_rows(&open_catalog().unwrap(), sql), before[index]);
    }
    for revision in [&seed.revision, &current] {
        events_window(&seed.import_id, Some(revision), None, Some(20), None, None).unwrap();
    }
    assert_eq!(
        jobs::published_hashes(&catalog::revision_dir(&seed.import_id, &current).unwrap()).unwrap(),
        current_hashes
    );
    assert_eq!(
        job_get(&accepted.job_id).unwrap().state,
        JobState::Completed
    );
    conn.execute(
        "UPDATE meta SET value='2' WHERE key='registration_provenance_schema_version'",
        [],
    )
    .unwrap();
    assert_eq!(
        catalog::rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {})
            .unwrap_err()
            .code,
        ErrorCode::ImportSchemaTooNew
    );
    assert_eq!(
        stored_rows(
            &conn,
            "SELECT value FROM meta WHERE key='registration_provenance_schema_version'"
        ),
        ["2"]
    );
    assert_eq!(stored_rows(&conn, queries[0]), before[0]);
    conn.execute(
        "UPDATE meta SET value='1' WHERE key='registration_provenance_schema_version'",
        [],
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn unscoped_workspace_roots_and_unassigned_membership_are_not_silently_adopted() {
    let home = IsolatedAppHome::acquire("legacy-workspace-preserve");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let conn = open_catalog().unwrap();
    let workspace_path = home
        .path
        .join("synthetic legacy project")
        .to_string_lossy()
        .into_owned();
    let saved = workspace::source_workspace_from_folder(&workspace_path);
    assert!(saved.source_identity.is_none());
    catalog::upsert_workspace(&conn, &saved).unwrap();
    for mapping in workspace::propose_mappings(&[Some(workspace_path)], None) {
        catalog::upsert_mapping(&conn, &mapping).unwrap();
    }
    // A legacy source can legitimately retain an explicit unassigned record.
    let association = workspace_association::update(workspace_association::UpdateRequest {
        import_id: seed.import_id.clone(),
        expected_version: 0,
        request_id: Uuid::new_v4().to_string(),
        choice: None,
    })
    .unwrap();
    assert!(!association.source_ready);
    let queries = [
        "SELECT json FROM workspaces ORDER BY source_workspace_id",
        "SELECT json FROM mappings ORDER BY root_id",
        "SELECT json FROM entry_workspaces ORDER BY import_id",
    ];
    let before: Vec<_> = queries.iter().map(|sql| stored_rows(&conn, sql)).collect();
    accept(&plan(previews, vec![]).unwrap()).unwrap();
    job_control::run_pending().unwrap();
    reset_runtime();
    let conn = open_catalog().unwrap();
    for (index, sql) in queries.iter().enumerate() {
        assert_eq!(stored_rows(&conn, sql), before[index]);
    }
    let association = workspace_association::get(&seed.import_id).unwrap();
    assert_eq!(
        association.status,
        workspace_association::AssociationStatus::Unassigned
    );
    assert!(association.choice.is_none());
    assert_eq!(association.version, 1);
    assert!(catalog::list_workspaces(&conn).unwrap()[0]
        .source_identity
        .is_none());
    assert!(catalog::list_mappings(&conn)
        .unwrap()
        .iter()
        .all(|mapping| mapping.source.is_none() && mapping.unassigned && !mapping.create_project));
    assert_archive_unchanged(&seed);
    reset_runtime();
}

fn delayed_legacy_producer(after_undo: bool, explicit_reimport: bool) {
    let home = IsolatedAppHome::acquire("legacy-late-producer");
    let previews = cursor_source(&home, 1);
    let old_preview = &previews[0];
    let seed = seed_registration(old_preview, legacy_registration(old_preview));
    // Set enqueue order BEFORE acceptance. Never mutate immutable accepted
    // context just to move a genuine old producer behind the canonical job.
    let (delayed, _) = {
        let _clock = crate::session_migration::clock::Fixed::at(
            chrono::Utc::now() + chrono::Duration::hours(1),
        );
        queue_registration(
            old_preview,
            seed.installation.clone(),
            explicit_reimport,
            json!({}),
        )
    };
    let conn = open_catalog().unwrap();
    assert_eq!(job_get(&delayed).unwrap().state, JobState::Queued);
    let root = resident(old_preview).root_path;
    let record = json!({"composerId":"legacy-pipeline-0","role":"user","content":"SYNTHETIC_NEWER_CANONICAL_HISTORY"});
    std::fs::write(
        std::path::Path::new(&root).join("0.jsonl"),
        format!("{record}\n"),
    )
    .unwrap();
    let scan = scan_start(vec![(Provider::Cursor, root)]).unwrap();
    assert_eq!(scan.candidates.len(), 1);
    let next = &scan.candidates[0];
    let next_preview = preview(&next.candidate_id, Some(&next.source_revision))
        .unwrap()
        .0;
    let canonical = resident(&next_preview).source_id;
    let accepted = accept(&plan(vec![next_preview], vec![]).unwrap()).unwrap();
    assert_eq!(entry_identity(&seed).0, canonical);
    if after_undo {
        let undone = jobs::undo_job(&conn, &seed.job_id).unwrap();
        assert_eq!(undone.items[0].phase, "undone");
    }
    job_control::run_pending().unwrap();
    reset_runtime();
    let obsolete = job_get(&delayed).unwrap();
    assert_eq!(obsolete.state, JobState::CompletedWithIssues);
    assert_eq!(obsolete.items[0].phase, "failed");
    assert_eq!(
        crate::session_migration::errors::parse_command_err(
            obsolete.items[0].error.as_deref().unwrap()
        ),
        Some(ErrorCode::PlanStale)
    );
    let old_receipts: i64 = open_catalog()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1",
            [&delayed],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        old_receipts, 0,
        "obsolete unstarted worker allocates no receipt"
    );
    let entry = get_entry(&open_catalog().unwrap(), &seed.import_id)
        .unwrap()
        .unwrap();
    let count: i64 = open_catalog()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        count, 1,
        "late old producer must not allocate a second legacy entry"
    );
    assert_eq!(entry.installation_id, canonical);
    if after_undo {
        assert!(entry.hidden);
        assert!(catalog::is_tombstoned(
            &open_catalog().unwrap(),
            &canonical,
            "cursor",
            &entry.native_session_id
        )
        .unwrap());
        assert!(events_window(&seed.import_id, None, None, Some(20), None, None).is_err());
    } else {
        let completed = job_get(&accepted.job_id).unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(entry.current_revision, completed.items[0].revision_id);
        assert_ne!(
            entry.current_revision.as_deref(),
            Some(seed.revision.as_str())
        );
        events_window(
            &seed.import_id,
            Some(&seed.revision),
            None,
            Some(20),
            None,
            None,
        )
        .unwrap();
    }
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn queued_legacy_producer_cannot_create_another_entry_or_replace_a_newer_revision() {
    delayed_legacy_producer(false, false);
}

#[test]
fn queued_legacy_producer_cannot_resurrect_a_canonical_tombstone() {
    delayed_legacy_producer(true, false);
}

#[test]
fn old_explicit_reimport_cannot_restore_a_superseded_registration() {
    delayed_legacy_producer(true, true);
}

#[test]
fn non_adopted_legacy_explicit_reimport_keeps_its_original_tombstone_contract() {
    let home = IsolatedAppHome::acquire("legacy-reimport-owner-control");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let (queued, _) = queue_registration(&previews[0], seed.installation.clone(), true, json!({}));
    assert_eq!(
        jobs::undo_job(&open_catalog().unwrap(), &seed.job_id)
            .unwrap()
            .items[0]
            .phase,
        "undone"
    );
    job_control::run_pending().unwrap();
    let result = job_get(&queued).unwrap();
    assert_eq!(result.state, JobState::Completed);
    assert_eq!(
        result.items[0].import_id.as_deref(),
        Some(seed.import_id.as_str())
    );
    let entry = get_entry(&open_catalog().unwrap(), &seed.import_id)
        .unwrap()
        .unwrap();
    assert!(!entry.hidden);
    assert_eq!(entry.installation_id, seed.installation);
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}
