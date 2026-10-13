//! Original Chat Stop fence at real ACP stdin admission, not installed App::stop.
use super::*;
use crate::acp_client::computer_use::DecisionDelivery;
use grok_computer_use_core::task_consent::CommitOutcome;

pub(super) struct Context<'a> {
    pub(super) manager: &'a Arc<SessionManager>,
    pub(super) fixture: &'a mut TaskTransportFixture,
    pub(super) approvals: &'a FixtureApprovals,
    pub(super) key: &'a HostRequestKey,
    pub(super) approved: &'a CommitOutcome,
}

#[derive(Default)]
pub(super) struct ChatStopProbe {
    delivery: Option<DecisionDelivery>,
    queued: bool,
    replay_checked: bool,
}

pub(super) fn is_command(command: &str) -> bool {
    matches!(
        command,
        "chat-stop-queued" | "chat-stop-authority" | "chat-stop-replay"
    )
}

impl ChatStopProbe {
    pub(super) fn command(
        &mut self,
        command: &str,
        runtime: &tokio::runtime::Runtime,
        context: Context<'_>,
    ) -> Result<Value, String> {
        match command {
            "chat-stop-queued" => {
                let broker = context.fixture.broker();
                if self.delivery.is_some()
                    || self.queued
                    || broker.executable_grant_count("view-app") != 1
                    || broker.continuation_count("view-app") != 1
                    || !broker.feature_enabled()
                    || context.approvals.settings()["computerUseEnabled"] != true
                {
                    return Err("original Chat Stop queue misplaced".into());
                }
                let (endpoint, delivery, turn) = context
                    .manager
                    .reserve_computer_use_delivery("view-app")
                    .ok_or("original manager did not reserve its Chat Stop continuation")?;
                if endpoint.agent_session_id != "synthetic-view-agent"
                    || !Arc::ptr_eq(&endpoint.acp, &context.fixture.client)
                    || delivery.was_admitted()
                {
                    return Err(
                        "original Chat Stop reservation changed owner or admitted early".into(),
                    );
                }
                self.delivery = Some(delivery.clone());
                let manager = context.manager.clone();
                let result =
                    runtime.block_on(context.fixture.queued_continuation_stops_before_wire(
                        delivery,
                        turn,
                        move || {
                            // Exact first synchronous product boundary; no AppHandle/UI fiction.
                            manager.fence_computer_use_chat_stop("view-app");
                            Ok(())
                        },
                    ));
                crate::turn_lease::clear_lease("view-app");
                result?;
                self.queued = true;
                let mut reply = self.snapshot(&context)?;
                reply["queuedAtOriginalStdin"] = json!(true);
                reply["pendingBeforeStop"] = json!(1);
                reply["continuationBeforeStop"] = json!(1);
                reply["notStarted"] = json!(true);
                reply["originalPromptJoined"] = json!(true);
                reply["zeroPeerWire"] = json!(true);
                reply["originalClaimRetired"] = json!(true);
                Ok(reply)
            }
            "chat-stop-authority" | "chat-stop-replay" => {
                let before = self.snapshot(&context)?;
                if before["originalStopped"] != true {
                    return Err("original Chat Stop run has not physically stopped".into());
                }
                if command == "chat-stop-replay" {
                    if self.replay_checked {
                        return Err("original Chat Stop replay repeated".into());
                    }
                    runtime.block_on(
                        context.fixture.retired_continuation_cannot_restart_chat(
                            self.delivery
                                .as_ref()
                                .ok_or("original Chat Stop delivery missing")?,
                        ),
                    )?;
                    self.replay_checked = true;
                }
                self.snapshot(&context)
            }
            _ => Err("unknown original Chat Stop command".into()),
        }
    }

    fn snapshot(&self, context: &Context<'_>) -> Result<Value, String> {
        let broker = context.fixture.broker();
        let state = context.fixture.original_host_task_view()?;
        // The ledger keeps invalid tickets to reject replay. Record count is
        // history; only an available claim can be an executable continuation.
        let continuations = usize::from(broker.claim_continuation("view-app").is_some());
        // Fixed booleans/counts only: retain the failed condition without logging
        // arbitrary request metadata, transport tokens or settings payloads.
        let witness = json!({
            "queued": self.queued,
            "featureEnabled": state.feature_enabled,
            "grantRows": state.grants.len(),
            "onlyOriginalExecution": state.executions.iter()
                .all(|row| row.run_id == context.approved.run_id),
            "originalRequestRetained": state.request.as_ref()
                .is_none_or(|row| row.key == *context.key),
            "executableGrants": broker.executable_grant_count("view-app"),
            "continuations": continuations,
            "retainedContinuationRecords": broker.continuation_count("view-app"),
            "approvalCalls": context.approvals.started(),
            "ownedWorkers": context.approvals.owners(),
            "storedEnabled": context.approvals.settings()["computerUseEnabled"] == true,
            "originalTransportRetained": context.fixture.original_transport_is_current(),
            "unadmittedDeliveryRetained": self.delivery.as_ref()
                .is_some_and(|delivery| !delivery.was_admitted()),
        });
        if [
            "queued",
            "featureEnabled",
            "onlyOriginalExecution",
            "originalRequestRetained",
            "storedEnabled",
            "originalTransportRetained",
            "unadmittedDeliveryRetained",
        ]
        .iter()
        .any(|field| witness[field] != true)
            || [
                "grantRows",
                "executableGrants",
                "continuations",
                "ownedWorkers",
            ]
            .iter()
            .any(|field| witness[field] != 0)
            || witness["approvalCalls"] != 1
        {
            println!("original Chat Stop snapshot conditions: {witness}");
            return Err(
                "original Chat Stop retained authority or changed its owner/settings".into(),
            );
        }
        Ok(
            json!({"state":state,"runId":context.approved.run_id,"stored":true,
            "grants":0,"continuations":continuations,"ownedWorkers":0,"approvalCalls":1,
            "retainedContinuationRecords":witness["retainedContinuationRecords"],
            "originalStopped":broker.stop_state(&context.approved.run_id).map_err(|e| e.to_string())? == StopState::Stopped,
            "originalTransportRetained":true,"replayChecked":self.replay_checked}),
        )
    }
}

pub(super) fn validate_evidence(evidence: &Value) -> Result<(), String> {
    if evidence["inputRequests"] != 6
        || evidence["rejection"] != "target_unauthorized"
        || evidence["pendingBeforeStop"] != 1
        || evidence["continuationBeforeStop"] != 1
        || [
            "queuedAtOriginalStdin",
            "notStarted",
            "originalPromptJoined",
            "zeroPeerWire",
            "originalClaimRetired",
            "originalRunStopped",
            "oldDeliveryNotRevived",
            "originalDocumentsRetained",
            "threeSharesRetained",
            "finalAuthorityZero",
            "featureAndStoreRetained",
        ]
        .iter()
        .any(|field| evidence[field] != true)
    {
        return Err("original Chat Stop evidence incomplete".into());
    }
    println!("original Chat Stop stdin authority witness: {evidence}");
    Ok(())
}

#[test]
#[ignore = "manual native gate: explicit CFT140+, original Chat Stop fence/ACP/source TaskGrant"]
fn existing_task_joint_chat_stop_before_original_stdin_rejects_queued_continuation() {
    scenario(
        "active-chat-stop-queued",
        &[
            "request-off-no-model-read",
            "real-mcp-first-request-no-authority",
            "original-app-confirm-required",
            "real-unshare-reshare-original-picker",
            "new-task-grant-original-app-publication",
            "original-chat-stop-queued-prompt-fenced-and-joined",
            "chat-stop-zero-authority-shares-retained",
            "chat-stop-old-delivery-cannot-revive",
        ],
    );
}
