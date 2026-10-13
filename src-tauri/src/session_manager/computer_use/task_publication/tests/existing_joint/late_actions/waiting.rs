//! One Continue while the original PNG still owns its native admission.
use super::*;
use crate::computer_use::task_controls::original_worker_for_test;
use crate::session_manager::computer_use::task_recovery::TaskResumeOutcome;

#[derive(Default)]
pub(super) struct CaptureWait {
    waiter: Option<tokio::task::JoinHandle<Result<TaskResumeOutcome, String>>>,
    address: Option<HostExecutionAddress>,
    entry: Option<uuid::Uuid>,
    duplicate_checked: bool,
    stopped: bool,
}

pub(super) fn is_command(command: &str) -> bool {
    matches!(
        command,
        "late-capture-wait-start"
            | "late-capture-wait-state"
            | "late-capture-wait-held"
            | "late-capture-wait-duplicate"
            | "late-capture-wait-stop"
            | "late-capture-wait-finish"
    )
}

impl LateActionProbe {
    pub(super) fn waiting_command(
        &mut self,
        command: &str,
        runtime: &tokio::runtime::Runtime,
        mut context: recovery::Context<'_>,
    ) -> Result<Value, String> {
        if !matches!(
            context.scenario,
            "active-late-capture-early-continue" | "active-late-capture-early-stop"
        ) {
            return Err("early Continue unavailable in this scenario".into());
        }
        match command {
            "late-capture-wait-start" => {
                if !self.interrupted
                    || self.joined
                    || self.waiting.address.is_some()
                    || !self
                        .pause
                        .as_ref()
                        .is_some_and(tokio::task::JoinHandle::is_finished)
                {
                    return Err("early Continue not at the original paused capture cut".into());
                }
                self.finish_pause(runtime)?;
                self.joined = true;
                let reply = self.snapshot(&context)?;
                if reply["existingIdle"] != false
                    || reply["executable"] != 0
                    || reply["state"]["executions"][0]["phase"] != "paused"
                    || original_worker_for_test()?.is_some()
                {
                    return Err("early Continue lost the original held admission".into());
                }
                let row = &context.fixture.original_host_task_view()?.executions[0];
                let address = HostExecutionAddress {
                    key: row.key.clone(),
                    run_id: row.run_id.clone(),
                    generation: row.generation.clone(),
                };
                let manager = context.manager.clone();
                let original = address.clone();
                self.waiting.address = Some(address.clone());
                self.waiting.waiter = Some(runtime.spawn(async move {
                    manager
                        .resume_computer_use_task_execution("view-app", &original)
                        .await
                }));
                Ok(json!({"started":true,"address":address}))
            }
            "late-capture-wait-held" => {
                if self.waiting.entry.is_some() || self.waiting.address.is_none() {
                    return Err("early Continue registry pin repeated or missing".into());
                }
                self.waiting.entry =
                    Some(original_worker_for_test()?.ok_or("early Continue worker missing")?);
                self.waiting_held(runtime, &mut context)
            }
            "late-capture-wait-duplicate" => {
                if self.waiting.duplicate_checked {
                    return Err("early Continue duplicate repeated".into());
                }
                self.waiting_held(runtime, &mut context)?;
                let outcome =
                    runtime.block_on(context.manager.resume_computer_use_task_execution(
                        "view-app",
                        self.waiting.address.as_ref().unwrap(),
                    ))?;
                if !matches!(outcome, TaskResumeOutcome::InProgress) {
                    return Err(
                        "duplicate early Continue did not share the original reservation".into(),
                    );
                }
                self.waiting.duplicate_checked = true;
                let mut reply = self.waiting_held(runtime, &mut context)?;
                reply["inProgress"] = json!(true);
                Ok(reply)
            }
            "late-capture-wait-stop" => {
                if context.scenario != "active-late-capture-early-stop"
                    || !self.waiting.duplicate_checked
                    || self.waiting.stopped
                {
                    return Err("waiting Stop misplaced".into());
                }
                self.waiting_held(runtime, &mut context)?;
                context.manager.fence_computer_use_chat_stop("view-app");
                self.waiting.stopped = true;
                let reply = self.snapshot(&context)?;
                if reply["executable"] != 0 || reply["pendingContinuation"] != false {
                    return Err("waiting Stop retained recovery authority".into());
                }
                Ok(reply)
            }
            "late-capture-wait-finish" => {
                if !self.waiting.duplicate_checked || self.waiting.entry.is_none() {
                    return Err("early Continue original join misplaced".into());
                }
                let result = runtime
                    .block_on(
                        self.waiting
                            .waiter
                            .as_mut()
                            .ok_or("early Continue waiter missing")?,
                    )
                    .map_err(|_| "early Continue waiter did not join")?;
                self.waiting.waiter.take();
                if original_worker_for_test()?.is_some() {
                    return Err("early Continue left its native worker live".into());
                }
                let mut reply = self.snapshot(&context)?;
                if self.waiting.stopped {
                    if result.is_ok()
                        || reply["executable"] != 0
                        || reply["pendingContinuation"] != false
                    {
                        return Err("waiting Stop resurrected the original recovery".into());
                    }
                    runtime.block_on(context.fixture.original_continuation_wire_empty())?;
                    reply["rejected"] = json!(true);
                    reply["rejection"] = json!(result.err());
                } else {
                    let receipt = result?;
                    let TaskResumeOutcome::Resumed { ref address } = receipt else {
                        return Err("early Continue did not resume".into());
                    };
                    if address.key != *context.key
                        || address.run_id != context.approved.run_id
                        || address.generation == self.waiting.address.as_ref().unwrap().generation
                        || reply["executable"] != 1
                        || reply["pendingContinuation"] != true
                    {
                        return Err("early Continue did not publish one fresh authority".into());
                    }
                    self.resumed = true;
                    reply["outcome"] = serde_json::to_value(receipt).unwrap();
                }
                reply["originalWorkerJoined"] = json!(true);
                reply["originalWorkerRetired"] = json!(true);
                reply["originalWaiterJoined"] = json!(true);
                Ok(reply)
            }
            "late-capture-wait-state" => self.snapshot(&context),
            _ => Err("unknown early Continue command".into()),
        }
    }

    fn waiting_held(
        &self,
        runtime: &tokio::runtime::Runtime,
        context: &mut recovery::Context<'_>,
    ) -> Result<Value, String> {
        let mut reply = self.snapshot(context)?;
        if self.waiting.entry.is_none()
            || original_worker_for_test()? != self.waiting.entry
            || self
                .waiting
                .waiter
                .as_ref()
                .is_none_or(tokio::task::JoinHandle::is_finished)
            || reply["existingIdle"] != false
            || reply["executable"] != 0
            || reply["pendingContinuation"] != false
            || reply["state"]["executions"][0]["phase"] != "resuming"
            || reply["state"]["executions"][0]["hasFreshObservation"] != false
        {
            return Err(
                "early Continue published or replaced its worker before the original PNG joined"
                    .into(),
            );
        }
        runtime.block_on(context.fixture.original_continuation_wire_empty())?;
        reply["originalWorkerRetained"] = json!(true);
        reply["originalWaiterPending"] = json!(true);
        reply["zeroPeerWire"] = json!(true);
        Ok(reply)
    }
}

impl CaptureWait {
    pub(super) fn is_pending(&self) -> bool {
        self.waiter.is_some()
    }

    pub(super) fn finish(&mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
        if let Some(waiter) = self.waiter.as_mut() {
            let _outcome = runtime
                .block_on(waiter)
                .map_err(|_| "early Continue cleanup waiter did not join")?;
            self.waiter.take();
            if original_worker_for_test()?.is_some() {
                return Err("early Continue cleanup retained a native owner".into());
            }
        }
        Ok(())
    }
}
