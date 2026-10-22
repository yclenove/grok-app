//! One-off compatibility facts, captured/rechecked by the existing writer.
//! Reads never migrate and a retained plan alone never grants ordinary absence.
use super::*;

#[cfg(test)]
thread_local! { static AFTER_CAPTURE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None); }

#[cfg(test)]
pub(in crate::session_migration) fn test_after_capture(hook: impl FnOnce() + 'static) {
    AFTER_CAPTURE.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

pub(in crate::session_migration) struct PreparedCompatibility {
    closure_digest: String,
    scopes: Vec<PreparedScope>,
}

pub(super) fn closure_digest(records: &[ProofRecord]) -> Result<String, ImportError> {
    let mut bindings = records
        .iter()
        .map(|proof| Ok((proof.job_id(), proof.import_id(), proof.digest()?)))
        .collect::<Result<Vec<_>, ImportError>>()?;
    bindings.sort();
    Ok(hash::options_hash(&serde_json::json!(bindings)))
}

pub(in crate::session_migration) fn capture(
    conn: &Connection,
) -> Result<Option<PreparedCompatibility>, ImportError> {
    if conn.is_autocommit() {
        let captured = {
            let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
            capture(&tx)?
        };
        // The detached read snapshot has ended before a second connection can
        // change evidence. The existing acceptance writer must recheck it.
        #[cfg(test)]
        if captured.is_some() {
            if let Some(hook) = AFTER_CAPTURE.with(|slot| slot.borrow_mut().take()) {
                hook();
            }
        }
        return Ok(captured);
    }
    let mut statement = conn
        .prepare(
            "SELECT job_id FROM jobs WHERE NOT EXISTS(SELECT 1 FROM meta
             WHERE key='job_entry_scope:' || jobs.job_id
             OR key='job_entry_scope_accepted:' || jobs.job_id) ORDER BY job_id",
        )
        .map_err(sqlite_storage_error)?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(sqlite_storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_storage_error)?;
    if ids.is_empty() {
        return Ok(None);
    }
    // A missing scope/plan for a V8 job is not an older producer to migrate.
    // Reject before reading the legacy proof closure or preparing any repair.
    for id in &ids {
        if crate::session_migration::publication_protocol::PublicationProtocol::for_job_id(id)?
            != crate::session_migration::publication_protocol::PublicationProtocol::Legacy
        {
            return Err(refusal());
        }
    }
    let records = old_owner::migration_records(conn)?;
    let digest = closure_digest(&records)?;
    let mut by_job: BTreeMap<&str, Vec<&ProofRecord>> = BTreeMap::new();
    for proof in &records {
        by_job.entry(proof.job_id()).or_default().push(proof);
    }
    let mut scopes = Vec::new();
    for id in ids {
        // A partial new-format producer is corruption, not a genuine older
        // unsealed job. Do not discover this as a UNIQUE failure after writes.
        if intent::present(conn, &id)? {
            return Err(refusal());
        }
        let proofs = by_job.get(id.as_str()).map(Vec::as_slice).unwrap_or(&[]);
        let associated = proofs
            .iter()
            .any(|proof| matches!(proof, ProofRecord::Association(_)));
        let job = if associated {
            job_input_association::bounded_job_metadata(conn, &id)?
        } else {
            job_input_association::ordinary_job_metadata(conn, &id)?
        };
        let retained: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE plan_id=?1)",
                [&job.plan_id],
                |row| row.get(0),
            )
            .map_err(sqlite_storage_error)?;
        if !retained {
            // No-plan compatibility needs independent original producer facts;
            // do not manufacture an ordinary label or drop its strict fallback.
            continue;
        }
        let plan = engine::job_entry_plan(conn, &job.plan_id, &job.plan_hash)?;
        plan.publication_protocol.require_job_id(&job.job_id)?;
        let (_, plan_json) = catalog::load_plan_json(conn, &job.plan_id)?;
        let expected: BTreeSet<_> = plan
            .identity_associations
            .iter()
            .map(|confirmation| confirmation.import_id.as_str())
            .collect();
        let associations: Vec<_> = proofs
            .iter()
            .filter_map(|proof| match proof {
                ProofRecord::Association(proof) => Some(proof),
                _ => None,
            })
            .collect();
        let selected: BTreeSet<_> = associations
            .iter()
            .map(|proof| proof.import_id.as_str())
            .collect();
        if expected != selected {
            return Err(refusal());
        }
        if let Some(first) = associations.first() {
            let members = first.job_members(conn)?;
            if members.count() != associations.len() {
                return Err(refusal());
            }
            for proof in associations {
                proof.verify_job_member(conn, &members)?;
            }
        }
        let mut references = proofs
            .iter()
            .map(|proof| {
                Ok(Reference {
                    version: if matches!(proof, ProofRecord::Association(_)) {
                        2
                    } else {
                        1
                    },
                    import_id: proof.import_id().into(),
                    digest: proof.digest()?,
                })
            })
            .collect::<Result<Vec<_>, ImportError>>()?;
        references.sort_by(|left, right| left.import_id.cmp(&right.import_id));
        let intent = intent::PreparedIntent::capture(&job, &plan, &plan_json)?;
        let record = Record {
            version: 1,
            job_id: job.job_id.clone(),
            context_digest: context_digest(&job),
            plan_id: job.plan_id,
            plan_hash: job.plan_hash,
            plan_json_digest: hash::sha256_bytes(plan_json.as_bytes()),
            references,
            intent_digest: Some(intent.digest()),
            publication_digest: None,
        };
        record.validate(&id)?;
        scopes.push(PreparedScope {
            record,
            plan_json,
            intent: Some(intent),
            publication: None,
            acceptance: None,
            publication_protocol: plan.publication_protocol,
        });
    }
    Ok((!scopes.is_empty()).then_some(PreparedCompatibility {
        closure_digest: digest,
        scopes,
    }))
}

impl PreparedCompatibility {
    pub(in crate::session_migration) fn record(
        &self,
        conn: &rusqlite::Transaction<'_>,
    ) -> Result<(), ImportError> {
        if conn.is_autocommit()
            || closure_digest(&old_owner::migration_records(conn)?)? != self.closure_digest
        {
            return Err(stale("legacy job proof closure changed after preflight"));
        }
        for scope in &self.scopes {
            let job = if scope
                .record
                .references
                .iter()
                .any(|proof| proof.version == 2)
            {
                job_input_association::bounded_job_metadata(conn, &scope.record.job_id)?
            } else {
                job_input_association::ordinary_job_metadata(conn, &scope.record.job_id)?
            };
            scope.record(conn, &job)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_job_id_missing_signed_scope_is_never_legacy_compatibility() {
        for (id, code) in [
            (
                "abcdef12-1234-8567-8123-abcdef123456",
                ErrorCode::HistoryAccessUnavailable,
            ),
            (
                "abcdef12-1234-9567-8123-abcdef123456",
                ErrorCode::ImportSchemaTooNew,
            ),
        ] {
            // Only the locator survives; a legacy proof read would fail on
            // missing tables. There is no signed producer fixture or repair.
            let mut conn = Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE jobs(job_id TEXT PRIMARY KEY); CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);").unwrap();
            conn.execute("INSERT INTO jobs VALUES(?1)", [id]).unwrap();
            let before = conn.total_changes();
            {
                let tx = conn.transaction().unwrap();
                let error = capture(&tx).err().expect("new route cannot be migrated");
                assert_eq!(error.code, code);
                assert!(error.is_job_local());
            }
            assert_eq!(conn.total_changes(), before);
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM meta", [], |row| row.get::<_, u64>(0))
                    .unwrap(),
                0
            );
        }
    }
}
