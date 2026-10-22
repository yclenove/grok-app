use super::*;

#[cfg(test)]
thread_local! {
    static DURABLE_PLAN_LOADS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(in crate::session_migration) fn take_durable_plan_loads() -> u32 {
    DURABLE_PLAN_LOADS.with(|count| count.replace(0))
}

pub(super) fn load_durable_plan(plan_id: &str, plan_hash: &str) -> Result<PlanState, ImportError> {
    let conn = open_catalog()?;
    load_durable_plan_record(&conn, plan_id, plan_hash, true)
}

/// Retained, hash-validated intent only; never an accepted job-read grant.
pub(in crate::session_migration) fn job_entry_plan(
    conn: &rusqlite::Connection,
    plan_id: &str,
    plan_hash: &str,
) -> Result<PlanState, ImportError> {
    load_durable_plan_record(conn, plan_id, plan_hash, false)
}

/// Retained intent metadata, not authority to publish. Accepted proof/input
/// verification must still bind the exact job before any identity changes.
pub(in crate::session_migration) fn association_input_plan(
    conn: &rusqlite::Connection,
    plan_id: &str,
    plan_hash: &str,
) -> Result<PlanState, ImportError> {
    let plan = load_durable_plan_record(conn, plan_id, plan_hash, false)?;
    if plan.identity_associations.is_empty() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "input requires an association plan",
        ));
    }
    Ok(plan)
}

fn load_durable_plan_record(
    conn: &rusqlite::Connection,
    plan_id: &str,
    plan_hash: &str,
    require_unexpired: bool,
) -> Result<PlanState, ImportError> {
    #[cfg(test)]
    DURABLE_PLAN_LOADS.with(|count| count.set(count.get() + 1));
    let (stored_hash, json) = load_plan_json(conn, plan_id)?;
    if stored_hash != plan_hash {
        return Err(ImportError::new(ErrorCode::PlanStale, "plan hash mismatch"));
    }
    decode_durable_plan(plan_id, plan_hash, &json, require_unexpired)
}

/// Accepted original intent uses the SAME canonical hash/shape checks as a
/// retained plan. A checksummed routing label cannot impersonate an ordinary
/// plan whose immutable job plan_hash commits to association confirmations.
pub(in crate::session_migration) fn job_entry_plan_json(
    plan_id: &str,
    plan_hash: &str,
    json: &str,
) -> Result<PlanState, ImportError> {
    #[cfg(test)]
    DURABLE_PLAN_LOADS.with(|count| count.set(count.get() + 1));
    decode_durable_plan(plan_id, plan_hash, json, false)
}

fn decode_durable_plan(
    plan_id: &str,
    stored_hash: &str,
    json: &str,
    require_unexpired: bool,
) -> Result<PlanState, ImportError> {
    #[cfg(test)]
    let _decode =
        crate::session_migration::codec_profile::Span::start("plan-decode-total", json.len());
    #[cfg(test)]
    let json_decode =
        crate::session_migration::codec_profile::Span::start("plan-json-decode", json.len());
    let value: serde_json::Value = serde_json::from_str(json).map_err(|error| {
        ImportError::new(
            ErrorCode::PlanStale,
            format!("durable plan is not json: {error}"),
        )
    })?;
    #[cfg(test)]
    drop(json_decode);
    #[cfg(test)]
    let shape =
        crate::session_migration::codec_profile::Span::start("plan-initial-shape", json.len());
    let version = value
        .get("version")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "durable plan missing version"))?;
    let publication_protocol =
        super::super::publication_protocol::PublicationProtocol::decode(json, &value)?;
    let preview_ids: Vec<String> =
        serde_json::from_value(value.get("previewIds").cloned().ok_or_else(|| {
            ImportError::new(ErrorCode::PlanStale, "durable plan missing previews")
        })?)
        .map_err(|error| ImportError::new(ErrorCode::PlanStale, error.to_string()))?;
    let mappings: Vec<WorkspaceMapping> =
        serde_json::from_value(value.get("mappings").cloned().ok_or_else(|| {
            ImportError::new(ErrorCode::PlanStale, "durable plan missing mappings")
        })?)
        .map_err(|error| ImportError::new(ErrorCode::PlanStale, error.to_string()))?;
    let snapshots = value
        .get("snapshots")
        .cloned()
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "durable plan missing snapshots"))?;
    let selection_set: SelectionSet =
        serde_json::from_value(value.get("selectionSet").cloned().ok_or_else(|| {
            ImportError::new(ErrorCode::PlanStale, "durable plan missing selection")
        })?)
        .map_err(|error| ImportError::new(ErrorCode::PlanStale, error.to_string()))?;
    let selection_hash = value
        .get("selectionHash")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            ImportError::new(ErrorCode::PlanStale, "durable plan missing selection hash")
        })?
        .to_string();
    let options = value
        .get("options")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let identity_associations =
        super::super::identity_association::parse_record(&value, version, &selection_set)?;
    if value.get("planId").and_then(serde_json::Value::as_str) != Some(plan_id)
        || value.get("planHash").and_then(serde_json::Value::as_str) != Some(stored_hash)
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "durable plan identity mismatch",
        ));
    }
    if selection_set.hash() != selection_hash {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "durable plan selection hash mismatch",
        ));
    }
    selection_set.validate_shape()?;
    let preview_set: HashSet<&str> = preview_ids.iter().map(String::as_str).collect();
    let selection_set_ids: HashSet<&str> = selection_set
        .entries
        .iter()
        .map(|entry| entry.preview_id.as_str())
        .collect();
    if preview_set.len() != preview_ids.len()
        || selection_set_ids.len() != selection_set.entries.len()
        || preview_set != selection_set_ids
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "durable plan selections do not cover its previews",
        ));
    }
    let snapshots_arr = snapshots
        .as_array()
        .cloned()
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "durable plan snapshots invalid"))?;
    #[cfg(test)]
    drop(shape);
    #[cfg(test)]
    let canonical =
        crate::session_migration::codec_profile::Span::start("plan-canonical-hash", json.len());
    let payload = canonical_plan_payload(
        &preview_ids,
        &mappings,
        &snapshots_arr,
        &selection_set,
        &selection_hash,
        &options,
    );
    let created_for_hash = value
        .get("created")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let expiry_for_hash = value
        .get("expiry")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let recomputed =
        crate::session_migration::hash::options_hash(&publication_protocol.bind_payload(
            super::super::identity_association::bind_payload(
                payload,
                &identity_associations,
                plan_id,
                created_for_hash,
                expiry_for_hash,
            ),
            plan_id,
            created_for_hash,
            expiry_for_hash,
        ));
    #[cfg(test)]
    drop(canonical);
    if recomputed != stored_hash {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "durable plan record was modified",
        ));
    }
    let snapshots = super::super::plan_snapshot::parse(&snapshots_arr, &preview_ids)?;
    let created = value
        .get("created")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let expiry = value
        .get("expiry")
        .and_then(|value| value.as_str())
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "durable plan missing expiry"))?;
    let expiry_ts = chrono::DateTime::parse_from_rfc3339(expiry).map_err(|error| {
        ImportError::new(
            ErrorCode::PlanStale,
            format!("durable plan expiry is not rfc3339: {error}"),
        )
    })?;
    if require_unexpired && expiry_ts.with_timezone(&chrono::Utc) <= super::super::clock::now() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "durable plan expired",
        ));
    }
    if let Some(origin) = options.get("prepareOrigin") {
        let origin: super::super::prepare_plan::PreparePlanOrigin =
            serde_json::from_value(origin.clone()).map_err(|error| {
                ImportError::new(
                    ErrorCode::PlanStale,
                    format!("invalid prepare plan origin: {error}"),
                )
            })?;
        if origin.version != 1
            || !Uuid::parse_str(&origin.batch_id).is_ok_and(|id| id.to_string() == origin.batch_id)
        {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "invalid prepare plan origin",
            ));
        }
        // Fresh acceptance requires the finalized prepare receipt. A retained
        // historical plan is intent metadata only: its accepted proof/member
        // owners provide authority and must not reopen a disposed batch.
        if require_unexpired {
            super::super::prepare_plan::verify_receipt(&origin, plan_id, stored_hash)?;
        }
    }
    Ok(PlanState {
        publication_protocol,
        snapshots,
        identity_associations,
        plan_id: plan_id.to_string(),
        plan_hash: stored_hash.to_string(),
        preview_ids,
        mappings,
        created,
        selection_set,
        selection_hash,
    })
}

pub(super) struct VerifiedPlanSnapshot {
    pub mapping_subject: super::super::mapping_scope::MappingSubject,
    pub preview_id: String,
    pub snapshot_digest: String,
    pub source_member: Option<super::super::source_member::SourceMemberPin>,
    pub path: PathBuf,
}

pub(crate) fn plan_from_validated_prepare(
    prepared: super::super::prepare::ValidatedPreparePlan,
    mappings: Vec<WorkspaceMapping>,
) -> Result<PlanState, ImportError> {
    plan_from_validated_prepare_with_associations(prepared, mappings, vec![])
}

pub(crate) fn plan_from_validated_prepare_with_associations(
    prepared: super::super::prepare::ValidatedPreparePlan,
    mappings: Vec<WorkspaceMapping>,
    identity_associations: Vec<super::super::identity_association::AssociationConfirmation>,
) -> Result<PlanState, ImportError> {
    // Keep the validated snapshots protected through both database commits.
    let _operation = prepared._operation;
    drop_expired_previews();
    let origin = prepared.origin;
    let snapshots = prepared
        .previews
        .into_iter()
        .map(|preview| VerifiedPlanSnapshot {
            mapping_subject: preview.mapping_subject,
            preview_id: preview.preview_id,
            snapshot_digest: preview.snapshot_digest,
            source_member: preview.source_member,
            path: preview.path,
        })
        .collect();
    super::super::prepare_plan::finalize(&origin, || {
        persist_verified_association_plan(
            prepared.preview_ids,
            mappings,
            prepared.selection_set,
            snapshots,
            serde_json::json!({"prepareOrigin": origin}),
            identity_associations,
        )
    })
}

pub(super) fn persist_verified_plan(
    preview_ids: Vec<String>,
    mappings: Vec<WorkspaceMapping>,
    selection_set: SelectionSet,
    verified: Vec<VerifiedPlanSnapshot>,
) -> Result<PlanState, ImportError> {
    persist_verified_plan_with_options(
        preview_ids,
        mappings,
        selection_set,
        verified,
        serde_json::json!({}),
    )
}

fn persist_verified_plan_with_options(
    preview_ids: Vec<String>,
    mappings: Vec<WorkspaceMapping>,
    selection_set: SelectionSet,
    verified: Vec<VerifiedPlanSnapshot>,
    options: serde_json::Value,
) -> Result<PlanState, ImportError> {
    persist_verified_association_plan(
        preview_ids,
        mappings,
        selection_set,
        verified,
        options,
        vec![],
    )
}

pub(super) fn persist_verified_association_plan(
    mut preview_ids: Vec<String>,
    mut mappings: Vec<WorkspaceMapping>,
    selection_set: SelectionSet,
    mut verified: Vec<VerifiedPlanSnapshot>,
    options: serde_json::Value,
    identity_associations: Vec<super::super::identity_association::AssociationConfirmation>,
) -> Result<PlanState, ImportError> {
    if preview_ids.is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "plan requires at least one preview",
        ));
    }
    selection_set.validate_shape()?;
    let expected: HashSet<&str> = preview_ids.iter().map(String::as_str).collect();
    let selected: HashSet<&str> = selection_set
        .entries
        .iter()
        .map(|entry| entry.preview_id.as_str())
        .collect();
    let prepared: HashSet<&str> = verified
        .iter()
        .map(|entry| entry.preview_id.as_str())
        .collect();
    if expected.len() != preview_ids.len()
        || selected.len() != selection_set.entries.len()
        || prepared.len() != verified.len()
        || expected != selected
        || expected != prepared
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "verified plan inputs must cover each preview exactly once",
        ));
    }
    let identity_associations = super::super::identity_association::review_confirmations(
        &selection_set,
        &identity_associations,
    )?;
    for snapshot in &verified {
        super::super::catalog::ensure_inside_imports(&snapshot.path)?;
        let metadata = std::fs::symlink_metadata(&snapshot.path)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || snapshot.path.file_name().and_then(|name| name.to_str())
                != Some(snapshot.preview_id.as_str())
            || snapshot.snapshot_digest.is_empty()
        {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "verified preview location or identity is invalid",
            ));
        }
        super::super::jobs::renew_preview_lease(&snapshot.path, PREVIEW_TTL_SECS)?;
        rt(|runtime| {
            if let Some(preview) = runtime.previews.get_mut(&snapshot.preview_id) {
                preview.expires = Instant::now() + Duration::from_secs(PREVIEW_TTL_SECS);
            }
        });
    }

    mappings = super::super::mapping_scope::normalize(
        &verified
            .iter()
            .map(|snapshot| snapshot.mapping_subject.clone())
            .collect::<Vec<_>>(),
        mappings,
    )?;
    preview_ids.sort();
    mappings.sort_by(|left, right| left.root_id.cmp(&right.root_id));
    verified.sort_by(|left, right| left.preview_id.cmp(&right.preview_id));
    let snapshots = verified
        .iter()
        .map(|snapshot| {
            let binding = super::super::plan_snapshot::PlanSnapshot::capture(
                snapshot.preview_id.clone(),
                snapshot.snapshot_digest.clone(),
                snapshot.source_member.as_ref(),
            )?;
            serde_json::to_value(binding)
                .map_err(|e| ImportError::new(ErrorCode::PlanStale, e.to_string()))
        })
        .collect::<Result<Vec<_>, ImportError>>()?;
    let selection_set = if identity_associations.is_empty() {
        selection_set.canonicalized()
    } else {
        // Review binds raw ordering. Persist it for exact restart reinspection.
        selection_set
    };
    let plan_id = Uuid::new_v4().to_string();
    let selection_hash = selection_set.hash();
    let created = super::super::clock::now();
    let expiry = created + chrono::Duration::seconds(PREVIEW_TTL_SECS as i64);
    let created_s = created.to_rfc3339();
    let expiry_s = expiry.to_rfc3339();
    let publication_protocol =
        super::super::publication_protocol::PublicationProtocol::SignedOutcomeV1;
    let payload = super::super::identity_association::bind_payload(
        canonical_plan_payload(
            &preview_ids,
            &mappings,
            &snapshots,
            &selection_set,
            &selection_hash,
            &options,
        ),
        &identity_associations,
        &plan_id,
        &created_s,
        &expiry_s,
    );
    let payload = publication_protocol.bind_payload(payload, &plan_id, &created_s, &expiry_s);
    let plan_hash = super::super::hash::options_hash(&payload);
    let mut durable = serde_json::json!({
        "version": payload["version"],
        "planId": plan_id,
        "planHash": plan_hash,
        "previewIds": preview_ids,
        "mappings": mappings,
        "snapshots": snapshots,
        "selectionSet": selection_set,
        "selectionHash": selection_hash,
        "options": options,
        "created": created_s,
        "expiry": expiry_s,
        "publicationOutcome": payload["publicationOutcome"],
    });
    if !identity_associations.is_empty() {
        durable["identityAssociations"] = serde_json::json!(identity_associations);
    }
    let json = serde_json::to_string(&durable)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let conn = open_catalog()?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    persist_plan_json(&conn, &plan_id, &plan_hash, &json, &created_s, &expiry_s)?;
    let plan = PlanState {
        publication_protocol,
        snapshots: super::super::plan_snapshot::parse(&snapshots, &preview_ids)?,
        identity_associations,
        plan_id: plan_id.clone(),
        plan_hash,
        preview_ids,
        mappings,
        created: created_s,
        selection_set,
        selection_hash,
    };
    Ok(plan)
}
