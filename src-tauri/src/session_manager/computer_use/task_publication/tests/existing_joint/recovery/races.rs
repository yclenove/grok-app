//! The real original task-controls registry owns native recovery, not this waiter.
use super::*;
use crate::computer_use::task_controls::original_worker_for_test;
use crate::session_manager::computer_use::task_recovery::TaskResumeOutcome;

type RecoveryWaiter = tokio::task::JoinHandle<Result<TaskResumeOutcome, String>>;

#[derive(Default)]
pub(super) struct RecoveryRaceProbe {
    worker: Option<RecoveryWaiter>,
    entry: Option<uuid::Uuid>,
    duplicate_checked: bool,
    joined: bool,
}

pub(super) fn is_command(command: &str) -> bool {
    matches!(
        command,
        "host-resume-race-start"
            | "host-resume-race-held"
            | "host-resume-race-duplicate"
            | "host-resume-race-finish"
    )
}

impl RecoveryRaceProbe {
    pub(super) fn reject_and_join(
        &mut self,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<(), String> {
        self.reject_with_and_join(
            runtime,
            "Computer Use original scope or recovery observation unavailable",
        )
    }

    pub(super) fn reject_with_and_join(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        expected: &'static str,
    ) -> Result<(), String> {
        if self.entry.is_none() || self.joined || self.duplicate_checked {
            return Err("interrupted original recovery join misplaced".into());
        }
        let outcome = runtime
            .block_on(
                self.worker
                    .as_mut()
                    .ok_or("original recovery waiter missing")?,
            )
            .map_err(|_| "interrupted original recovery waiter did not join")?;
        self.worker.take();
        self.joined = true;
        if outcome.as_ref().err().map(String::as_str) != Some(expected)
            || original_worker_for_test()?.is_some()
        {
            return Err("interrupted original recovery published or retained its owner".into());
        }
        Ok(())
    }

    pub(super) fn command(
        &mut self,
        command: &str,
        runtime: &tokio::runtime::Runtime,
        context: &Context<'_>,
        paused: Option<&HostExecutionAddress>,
    ) -> Result<Value, String> {
        let paused = paused.ok_or("original recovery race pause missing")?;
        match command {
            "host-resume-race-start" => {
                if self.worker.is_some()
                    || self.entry.is_some()
                    || self.joined
                    || original_worker_for_test()?.is_some()
                    || paused.key != *context.key
                    || paused.run_id != context.approved.run_id
                {
                    return Err("original native recovery start misplaced".into());
                }
                let manager = context.manager.clone();
                let address = paused.clone();
                self.worker = Some(runtime.spawn(async move {
                    manager
                        .resume_computer_use_task_execution("view-app", &address)
                        .await
                }));
                Ok(json!({"started":true}))
            }
            "host-resume-race-held" => {
                if self.entry.is_some() || self.joined {
                    return Err("original recovery registry capture repeated".into());
                }
                self.entry = Some(
                    original_worker_for_test()?
                        .ok_or("original recovery registry owner missing")?,
                );
                self.held(context)
            }
            "host-resume-race-duplicate" => {
                if self.duplicate_checked {
                    return Err("native concurrent Continue repeated".into());
                }
                self.held(context)?;
                let result = runtime.block_on(
                    context
                        .manager
                        .resume_computer_use_task_execution("view-app", paused),
                )?;
                if !matches!(result, TaskResumeOutcome::InProgress) {
                    return Err("concurrent Continue did not retain original recovery".into());
                }
                self.duplicate_checked = true;
                let mut reply = self.held(context)?;
                reply["inProgress"] = json!(true);
                Ok(reply)
            }
            "host-resume-race-finish" => {
                if !self.duplicate_checked || self.joined || self.entry.is_none() {
                    return Err("original recovery natural join misplaced".into());
                }
                let receipt = runtime
                    .block_on(
                        self.worker
                            .as_mut()
                            .ok_or("original recovery waiter missing")?,
                    )
                    .map_err(|_| "original recovery waiter did not join")??;
                self.worker.take();
                self.joined = true;
                let TaskResumeOutcome::Resumed { ref address } = receipt else {
                    return Err("original recovery did not publish its receipt".into());
                };
                if address.key != *context.key
                    || address.run_id != context.approved.run_id
                    || address.generation == paused.generation
                    || original_worker_for_test()?.is_some()
                {
                    return Err("original recovery changed owner or left its worker live".into());
                }
                let mut reply = snapshot(context)?;
                if reply["executable"] != 1 || reply["pendingContinuation"] != true {
                    return Err("original joined recovery did not publish one authority".into());
                }
                reply["ok"] = json!(true);
                reply["outcome"] = serde_json::to_value(receipt).unwrap();
                reply["originalWorkerJoined"] = json!(true);
                reply["originalWorkerRetired"] = json!(true);
                reply["originalWaiterJoined"] = json!(true);
                reply["inProgressReplies"] = json!(1);
                println!("original native recovery: same registry entry retained during concurrent Continue; original waiter/worker joined; one fresh authority published");
                Ok(reply)
            }
            _ => Err("unknown original recovery race command".into()),
        }
    }

    fn held(&self, context: &Context<'_>) -> Result<Value, String> {
        self.require_original_pending()?;
        let mut reply = snapshot(context)?;
        if reply["executable"] != 0
            || reply["pendingContinuation"] != false
            || reply["state"]["executions"][0]["phase"] != "resuming"
            || reply["state"]["executions"][0]["hasFreshObservation"] != false
        {
            return Err("held original recovery published before native return".into());
        }
        reply["originalWorkerRetained"] = json!(true);
        reply["originalWaiterPending"] = json!(true);
        Ok(reply)
    }

    pub(super) fn require_original_pending(&self) -> Result<(), String> {
        if self.entry.is_none()
            || original_worker_for_test()? != self.entry
            || self
                .worker
                .as_ref()
                .is_none_or(tokio::task::JoinHandle::is_finished)
        {
            return Err("original native recovery worker was replaced or settled early".into());
        }
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        broker: &crate::computer_use::ComputerUseBroker,
    ) -> Result<(), String> {
        if let Some(worker) = self.worker.as_mut() {
            // The native peer has finished/released its original gate first.
            broker.fence_task_consent("view-app");
            let _outcome = runtime
                .block_on(worker)
                .map_err(|_| "original recovery failure cleanup waiter did not join")?;
            self.worker.take();
            if original_worker_for_test()?.is_some() {
                return Err("original recovery cleanup left a live control owner".into());
            }
        }
        Ok(())
    }

    pub(super) fn fence_pending(&self, broker: &crate::computer_use::ComputerUseBroker) {
        if self.worker.is_some() {
            broker.fence_task_consent("view-app");
        }
    }
}

fn snapshot(context: &Context<'_>) -> Result<Value, String> {
    let broker = context.fixture.broker();
    let state = context.fixture.original_host_task_view()?;
    if !context.fixture.original_transport_is_current()
        || !state.feature_enabled
        || state
            .request
            .as_ref()
            .is_none_or(|row| row.key != *context.key)
        || state.executions.len() != 1
        || state.executions[0].run_id != context.approved.run_id
        || state.executions[0].key != *context.key
        || context.approvals.owners() != 0
        || context.approvals.started() != 1
        || context.approvals.settings()["computerUseEnabled"] != true
    {
        return Err("original recovery race changed its owner or settings".into());
    }
    Ok(
        json!({"state":state,"executable":broker.executable_grant_count("view-app"),
        "continuations":broker.continuation_count("view-app"),
        "pendingContinuation":broker.claim_continuation("view-app").is_some()}),
    )
}

pub(in super::super) fn validate_evidence(evidence: &Value) -> Result<(), String> {
    if evidence["inProgressReplies"] != 1
        || evidence["nativeCaptures"] != 1
        || [
            "originalDocumentMatched",
            "originalWorkerRetained",
            "originalWaiterPending",
            "duplicateDidNotCapture",
            "originalWorkerJoined",
            "originalWorkerRetired",
            "originalWaiterJoined",
            "originalNativeResultReturned",
        ]
        .iter()
        .any(|field| evidence[field] != true)
    {
        return Err("original concurrent recovery evidence incomplete".into());
    }
    println!("original concurrent native recovery witness: {evidence}");
    Ok(())
}
