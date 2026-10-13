//! Actual original Host Pause/Chat Stop while a real native result is held.
use super::*;
use crate::acp_client::computer_use::DecisionDelivery;
use grok_computer_use_core::broker::HostExecutionPhase;
use grok_computer_use_core::task_consent::AuthorityClaim;

#[derive(Default)]
pub(super) struct RecoveryTerminalProbe {
    original: Option<(AuthorityClaim, DecisionDelivery)>,
    interrupted: Option<&'static str>,
    joined: bool,
    replay_checked: bool,
}

pub(super) fn is_command(command: &str) -> bool {
    matches!(
        command,
        "host-resume-terminal-pause"
            | "host-resume-terminal-stop"
            | "host-resume-terminal-finish"
            | "host-resume-terminal-state"
            | "host-resume-terminal-replay"
    )
}

impl RecoveryTerminalProbe {
    pub(super) fn original_delivery(&self) -> Result<&(AuthorityClaim, DecisionDelivery), String> {
        self.original
            .as_ref()
            .ok_or_else(|| "original recovery delivery missing".into())
    }

    pub(super) fn retain_delivery(&mut self, fixture: &TaskTransportFixture) -> Result<(), String> {
        // Pin the real first claim/delivery BEFORE the original stdin admission.
        if self.original.is_none() {
            let claim = fixture
                .broker()
                .claim_continuation("view-app")
                .ok_or("original recovery claim missing")?;
            let delivery = fixture
                .client
                .computer_use_decision("view-app", "synthetic-view-agent")
                .ok_or("original recovery delivery missing")?;
            if delivery.was_admitted() {
                return Err("original recovery delivery already consumed".into());
            }
            self.original = Some((claim, delivery));
        }
        Ok(())
    }

    pub(super) fn command(
        &mut self,
        command: &str,
        runtime: &tokio::runtime::Runtime,
        context: &mut Context<'_>,
        race: &mut races::RecoveryRaceProbe,
    ) -> Result<Value, String> {
        match command {
            "host-resume-terminal-pause" | "host-resume-terminal-stop" => {
                if self.interrupted.is_some() || self.joined {
                    return Err("original recovery interruption repeated".into());
                }
                let before = current_address(context)?;
                let view = context.fixture.original_host_task_view()?;
                if view.executions.len() != 1
                    || view.executions[0].phase != HostExecutionPhase::Resuming
                    || view.executions[0].has_fresh_observation
                {
                    return Err("original native recovery was not held".into());
                }
                if command == "host-resume-terminal-pause" {
                    // Read the same original run/key's CURRENT generation, not its old address.
                    runtime.block_on(
                        context
                            .manager
                            .pause_computer_use_task_execution("view-app", &before),
                    )?;
                    self.interrupted = Some("pause");
                    if current_address(context)?.generation == before.generation {
                        return Err("original Host Pause did not fence recovery generation".into());
                    }
                } else {
                    context.manager.fence_computer_use_chat_stop("view-app");
                    self.interrupted = Some("chat-stop");
                }
                self.snapshot(context)
            }
            "host-resume-terminal-finish" => {
                if self.interrupted.is_none() || self.joined {
                    return Err("original recovery terminal join misplaced".into());
                }
                race.reject_and_join(runtime)?;
                self.joined = true;
                runtime.block_on(context.fixture.original_continuation_wire_empty())?;
                let mut reply = self.snapshot(context)?;
                reply["originalWorkerJoined"] = json!(true);
                reply["originalWorkerRetired"] = json!(true);
                reply["originalWaiterJoined"] = json!(true);
                reply["zeroPeerWire"] = json!(true);
                reply["exactRejection"] =
                    json!("Computer Use original scope or recovery observation unavailable");
                Ok(reply)
            }
            "host-resume-terminal-replay" => {
                if !self.joined || self.replay_checked {
                    return Err("original interrupted recovery replay misplaced".into());
                }
                let (_, delivery) = self
                    .original
                    .as_ref()
                    .ok_or("original recovery delivery missing")?;
                runtime.block_on(
                    context
                        .fixture
                        .consumed_continuation_cannot_restart_recovery(
                            delivery,
                            self.interrupted == Some("chat-stop"),
                        ),
                )?;
                self.replay_checked = true;
                self.snapshot(context)
            }
            "host-resume-terminal-state" => self.snapshot(context),
            _ => Err("unknown original recovery terminal command".into()),
        }
    }

    fn snapshot(&self, context: &Context<'_>) -> Result<Value, String> {
        let broker = context.fixture.broker();
        let state = context.fixture.original_host_task_view()?;
        let (claim, delivery) = self
            .original
            .as_ref()
            .ok_or("original recovery delivery missing")?;
        if self.interrupted.is_none()
            || !context.fixture.original_transport_is_current()
            || !state.feature_enabled
            || state
                .request
                .as_ref()
                .is_some_and(|row| row.key != *context.key)
            || state.executions.iter().any(|row| {
                row.run_id != context.approved.run_id
                    || row.key != *context.key
                    || row.has_fresh_observation
            })
            || broker.executable_grant_count("view-app") != 0
            || broker.claim_continuation("view-app").is_some()
            || broker.validate_continuation(claim).is_ok()
            || !delivery.was_admitted()
            || context.approvals.owners() != 0
            || context.approvals.started() != 1
            || context.approvals.settings()["computerUseEnabled"] != true
        {
            return Err(
                "original recovery terminal retained authority or changed its owner".into(),
            );
        }
        let stopped = broker
            .stop_state(&context.approved.run_id)
            .map_err(|e| e.to_string())?
            == StopState::Stopped;
        if self.interrupted == Some("chat-stop") && !state.grants.is_empty() {
            return Err("original Chat Stop retained a task grant".into());
        }
        if self.joined
            && (self.interrupted == Some("pause")
                && (state.executions.len() != 1
                    || state.executions[0].phase != HostExecutionPhase::Paused)
                || self.interrupted == Some("chat-stop") && !stopped)
        {
            return Err("original recovery terminal has not physically settled".into());
        }
        Ok(
            json!({"state":state,"executable":0,"pendingContinuation":false,
            "retainedContinuationRecords":broker.continuation_count("view-app"),
            "originalClaimRetired":true,"originalDeliveryConsumed":true,"originalTransportRetained":true,
            "stored":true,"interruption":self.interrupted,"originalStopped":stopped,
            "replayChecked":self.replay_checked}),
        )
    }
}

pub(super) fn current_address(context: &Context<'_>) -> Result<HostExecutionAddress, String> {
    let view = context.fixture.original_host_task_view()?;
    let row = view
        .executions
        .iter()
        .find(|row| row.run_id == context.approved.run_id && row.key == *context.key)
        .ok_or("original recovery execution missing")?;
    Ok(HostExecutionAddress {
        key: row.key.clone(),
        run_id: row.run_id.clone(),
        generation: row.generation.clone(),
    })
}

pub(in super::super) fn validate_evidence(scenario: &str, evidence: &Value) -> Result<(), String> {
    let interruption = if scenario == "active-host-resume-pause" {
        "pause"
    } else {
        "chat-stop"
    };
    if evidence["interruption"] != interruption
        || evidence["nativeCaptures"] != 1
        || [
            "originalDocumentMatched",
            "originalWorkerRetained",
            "originalWaiterPending",
            "originalWorkerJoined",
            "originalWorkerRetired",
            "originalWaiterJoined",
            "originalNativeResultReturned",
            "zeroPeerWire",
            "originalClaimRetired",
            "oldDeliveryNotRevived",
            "originalDocumentsRetained",
            "threeSharesRetained",
            "pairRetained",
            "featureAndStoreRetained",
        ]
        .iter()
        .any(|field| evidence[field] != true)
    {
        return Err("original interrupted recovery evidence incomplete".into());
    }
    println!("original interrupted native recovery witness: {evidence}");
    Ok(())
}

fn checks() -> Vec<&'static str> {
    let mut checks = RECOVERY_CHECKS[..8].to_vec();
    checks.extend([
        "original-recovery-native-return-held",
        "original-recovery-interrupted-without-authority",
        "original-recovery-late-return-rejected-and-joined",
        "original-recovery-old-delivery-and-shares-retained",
    ]);
    checks
}

#[test]
#[ignore = "manual native gate: CFT140+, original current-generation Pause during real recovery"]
fn existing_task_joint_host_resume_pause_rejects_late_native_return_then_allows_new_user_continue()
{
    let mut checks = checks();
    checks.extend_from_slice(&RECOVERY_CHECKS[8..]);
    scenario("active-host-resume-pause", &checks);
}

#[test]
#[ignore = "manual native gate: CFT140+, original Chat Stop during real recovery"]
fn existing_task_joint_host_resume_chat_stop_rejects_late_native_return_and_old_delivery() {
    scenario("active-host-resume-chat-stop", &checks());
}
