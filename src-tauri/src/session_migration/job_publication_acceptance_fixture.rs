//! Deliberately synthetic V3 SQL context, not a registered acceptance grant.
use super::*;
use crate::session_migration::{
    adapters::generic,
    catalog,
    job_control::DurableInput,
    jobs::{self, CommitItem, JobItem, JobRecord, JobState, PreparedRevision},
    mapping_scope,
    plan_snapshot::PlanSnapshot,
    selection::SelectionSet,
};
use rusqlite::{params, Connection};
use std::sync::Arc;

pub(super) struct Fixture {
    pub job: JobRecord,
    pub plan: String,
    pub scope: String,
    pub prepared: Vec<(DurableInput, Arc<PreparedRevision>)>,
}

impl Fixture {
    // Synthetic identity metadata only, NOT registered source/proof authority.
    pub fn identities(
        inputs: &[(DurableInput, Arc<PreparedRevision>)],
    ) -> Vec<crate::session_migration::job_control::association_acceptance::Identity> {
        inputs
            .iter()
            .map(|(input, _)| {
                crate::session_migration::job_control::association_acceptance::Identity::Legacy(
                    input.input_identity().unwrap(),
                    None,
                )
            })
            .collect()
    }

    pub fn new(count: usize) -> Self {
        let mut prepared = Vec::new();
        for index in 0..count {
            let adapter = generic::normalize_bytes(
                br#"[{"role":"user","content":"synthetic acceptance input"}]"#,
                &format!("acceptance-{index}.json"),
            )
            .unwrap();
            let mut item = jobs::prepare_test_commit_item(CommitItem {
                candidate_id: format!("candidate-中文-{index}"),
                installation_id: "installation".into(),
                provider: "generic-file".into(),
                root_path: "synthetic-source".into(),
                adapter,
                prepared: None,
                options: mapping_scope::options(None),
                mapping_project_id: None,
                explicit_reimport: false,
                create_project: false,
                import_unassigned_anyway: false,
                explicit_target: None,
                identity_association: None,
                selection_set: Default::default(),
            });
            // Match the immutable synthetic lease's registered identity. This
            // fixture does not pretend to have source/proof authorization.
            item.installation_id = "test-source:installation".into();
            prepared.push(DurableInput::from_item(&item).unwrap());
        }
        let plan_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now();
        let created = now.to_rfc3339();
        let expiry = (now
            + chrono::Duration::seconds(crate::session_migration::budget::PREVIEW_TTL_SECS as i64))
        .to_rfc3339();
        let selections = SelectionSet {
            entries: prepared
                .iter()
                .flat_map(|(input, _)| input.selection_set.entries.clone())
                .collect(),
            ..Default::default()
        }
        .canonicalized();
        let snapshots: Vec<_> = prepared
            .iter()
            .map(|(input, _)| {
                PlanSnapshot::capture(
                    input
                        .selection_set
                        .single_entry()
                        .unwrap()
                        .preview_id
                        .clone(),
                    input.preview_digest.clone(),
                    input.source_member.as_ref(),
                )
                .unwrap()
            })
            .collect();
        let base = json!({"version":1,"previewIds":selections.entries.iter().map(|s| &s.preview_id).collect::<Vec<_>>(), "mappings":[], "snapshots":snapshots, "selectionSet":selections, "selectionHash":selections.hash(), "options":{}});
        let mut plan =
            PublicationProtocol::SignedOutcomeV1.bind_payload(base, &plan_id, &created, &expiry);
        let plan_hash = hash::options_hash(&plan);
        plan["planHash"] = json!(plan_hash);
        let job = JobRecord {
            job_id: PublicationProtocol::SignedOutcomeV1.new_job_id(),
            request_id: uuid::Uuid::new_v4().to_string(),
            plan_id,
            plan_hash,
            state: JobState::Queued,
            state_version: 1,
            items: prepared
                .iter()
                .map(|(input, _)| JobItem {
                    candidate_id: input.candidate_id.clone(),
                    title: None,
                    phase: "queued".into(),
                    import_id: None,
                    revision_id: None,
                    outcome: None,
                    error: None,
                    mapping_project_id: None,
                })
                .collect(),
            created_at: created,
            item_count: count as u32,
            next_item_cursor: None,
            progress: None,
        };
        Self {
            job,
            plan: plan.to_string(),
            scope: "synthetic scope; no registered grant".into(),
            prepared,
        }
    }

    pub fn capture(&self) -> producer::PreparedAcceptance {
        let conn = catalog::open_catalog().unwrap();
        producer::PreparedAcceptance::capture(
            &conn,
            &self.job,
            &self.plan,
            &hash::sha256_bytes(self.scope.as_bytes()),
            &self.prepared,
            &Self::identities(&self.prepared),
        )
        .unwrap()
    }

    pub fn seed(&self, conn: &Connection) {
        let plan: Value = serde_json::from_str(&self.plan).unwrap();
        catalog::persist_plan_json(
            conn,
            &self.job.plan_id,
            &self.job.plan_hash,
            &self.plan,
            plan["created"].as_str().unwrap(),
            plan["expiry"].as_str().unwrap(),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            params![
                format!("job_entry_plan_snapshot:{}", self.job.job_id),
                self.plan
            ],
        )
        .unwrap();
        jobs::save_job(conn, &self.job).unwrap();
        for (input, _) in &self.prepared {
            conn.execute("INSERT INTO job_items(job_id,candidate_id,phase,result_json) VALUES(?1,?2,'queued',?3)", params![self.job.job_id, input.candidate_id, serde_json::to_string(input).unwrap()]).unwrap();
        }
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            params![format!("job_entry_scope:{}", self.job.job_id), self.scope],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            params![
                format!("job_entry_scope_accepted:{}", self.job.job_id),
                hash::sha256_bytes(self.scope.as_bytes())
            ],
        )
        .unwrap();
    }

    pub fn header_key(&self) -> String {
        format!("publication_acceptance:v1:{}:header", self.job.job_id)
    }
    pub fn member_key(&self, index: usize) -> String {
        format!(
            "publication_acceptance:v1:{}:member:{}",
            self.job.job_id,
            hash::sha256_bytes(self.prepared[index].0.candidate_id.as_bytes())
        )
    }
    pub fn metadata_count(&self, conn: &Connection) -> u32 {
        conn.query_row(
            "SELECT count(*) FROM meta WHERE key LIKE ?1",
            [format!("publication_acceptance:v1:{}:%", self.job.job_id)],
            |row| row.get(0),
        )
        .unwrap()
    }
}

pub(super) fn sql_digest(conn: &Connection) -> String {
    let mut tables = Vec::new();
    for query in [
        "SELECT json_object('id',job_id,'request',request_id,'hash',plan_hash,'state',state,'version',state_version,'json',json,'created',created_at) FROM jobs ORDER BY job_id",
        "SELECT json_object('job',job_id,'candidate',candidate_id,'phase',phase,'input',result_json) FROM job_items ORDER BY job_id,candidate_id",
        "SELECT json_object('job',job_id,'index',item_index,'candidate',candidate_id,'json',json) FROM job_item_states ORDER BY job_id,item_index",
        "SELECT json_object('key',key,'kind',typeof(value),'bytes',hex(CAST(value AS BLOB))) FROM meta ORDER BY key",
        "SELECT json_object('id',plan_id,'hash',plan_hash,'json',json,'created',created_at,'expiry',expires_at) FROM plans ORDER BY plan_id",
        "SELECT json_object('id',import_id,'installation',installation_id,'provider',provider,'native',native_session_id,'title',title,'alias',alias,'hidden',hidden,'current',current_revision,'created',created_at) FROM entries ORDER BY import_id",
    ] {
        let mut statement = conn.prepare(query).unwrap();
        let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
        tables.push(rows.map(|row| hash::sha256_bytes(row.unwrap().as_bytes())).collect::<Vec<_>>());
    }
    hash::options_hash(&json!(tables))
}
