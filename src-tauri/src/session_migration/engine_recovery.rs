//! Restore the latest import intent without reopening already-consumed plans.

use super::*;
use crate::session_migration::catalog::{
    latest_active_job_id, latest_draft_json, latest_plan_job_id, latest_plan_row,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurablePlanRef {
    pub plan_id: String,
    pub plan_hash: String,
    #[serde(default)]
    pub preview_count: usize,
    #[serde(default)]
    pub association_targets: Vec<super::super::identity_association::AssociationConfirmation>,
}

impl From<&PlanState> for DurablePlanRef {
    fn from(plan: &PlanState) -> Self {
        Self {
            plan_id: plan.plan_id.clone(),
            plan_hash: plan.plan_hash.clone(),
            preview_count: plan.preview_ids.len(),
            association_targets: plan.identity_associations.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySnapshot {
    pub plan: Option<DurablePlanRef>,
    pub job: Option<JobRecord>,
    pub prepare: Option<super::super::prepare::PrepareBatchStatus>,
    pub draft: Option<HandoffDraft>,
    pub expired: bool,
    pub failed: Option<String>,
}

pub fn recovery() -> Result<RecoverySnapshot, ImportError> {
    let conn = open_catalog()?;
    let mut job = match latest_active_job_id(&conn)? {
        Some(job_id) => Some(job_items_page(&job_id, None, None)?),
        None => None,
    };
    let mut expired = false;
    let mut failed = None;
    // Select the newest intent before checking consumption. Filtering submitted plans
    // out of the query would resurrect an older, abandoned plan.
    let plan = match latest_plan_row(&conn)? {
        Some((plan_id, plan_hash, _, _)) => {
            if let Some(job_id) = latest_plan_job_id(&conn, &plan_id, &plan_hash)? {
                if job.is_none() {
                    let submitted = job_items_page(&job_id, None, None)?;
                    if submitted.state != JobState::Completed {
                        job = Some(submitted);
                    }
                }
                // Durable job inputs, not the preview TTL, own retry/recovery now.
                None
            } else {
                match super::engine_prepare_plan::load_durable_plan(&plan_id, &plan_hash) {
                    // The durable record owns the full selection. Recovery only
                    // returns its reference; do not retain another full plan in RAM.
                    Ok(plan) => Some(DurablePlanRef::from(&plan)),
                    Err(err) if err.message.contains("expired") => {
                        expired = true;
                        None
                    }
                    Err(err) => {
                        failed = Some(err.message);
                        None
                    }
                }
            }
        }
        None => None,
    };
    let draft = match latest_draft_json(&conn)? {
        Some((json, expires_at)) => {
            let expiry_past = chrono::DateTime::parse_from_rfc3339(&expires_at)
                .ok()
                .is_some_and(|ts| ts.with_timezone(&chrono::Utc) <= chrono::Utc::now());
            let restored = (|| {
                let mut draft = serde_json::from_str::<HandoffDraft>(&json)
                    .map_err(crate::session_migration::continuation_intent::error)?;
                crate::session_migration::continuation_create::check_draft(&draft)?;
                let creating =
                    crate::session_migration::continuation_intent::hydrate(&conn, &mut draft)?;
                Ok::<_, ImportError>((draft, creating))
            })();
            match restored {
                Ok((_, false)) if expiry_past => {
                    expired = true;
                    None
                }
                Ok((draft, _)) => {
                    rt(|r| {
                        r.drafts.insert(draft.draft_id.clone(), draft.clone());
                    });
                    Some(draft)
                }
                Err(err) => {
                    failed = Some(format!("draft restore failed: {}", err.message));
                    None
                }
            }
        }
        None => None,
    };
    let prepare_recovery = if job.is_none() && plan.is_none() {
        super::super::prepare::latest_recoverable()?
    } else {
        super::super::prepare_query::PrepareRecovery::default()
    };
    expired |= prepare_recovery.expired;
    if failed.is_none() {
        failed = prepare_recovery.failed;
    }
    Ok(RecoverySnapshot {
        plan,
        job,
        prepare: prepare_recovery.status,
        draft,
        expired,
        failed,
    })
}
