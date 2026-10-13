//! Original action ownership across real Host Pause/Chat Stop and native return.
use super::*;
use crate::acp_client::computer_use::DecisionDelivery;
use grok_computer_use_core::broker::{HostExecutionAddress, HostExecutionPhase};
use grok_computer_use_core::task_consent::AuthorityClaim;
mod waiting;

pub(super) fn is_scenario(value: &str) -> bool {
    action_name(value).is_some()
}

fn action_name(value: &str) -> Option<&'static str> {
    let rest = value.strip_prefix("active-late-")?;
    for (slug, action) in [
        ("click", "click"),
        ("set-value", "set_value"),
        ("type-text", "type_text"),
        ("scroll", "scroll"),
        ("wait", "wait"),
    ] {
        if rest.strip_prefix(slug).is_some_and(|suffix| {
            matches!(
                suffix,
                "-pre-pause" | "-post-pause" | "-pre-chat-stop" | "-post-chat-stop"
            )
        }) {
            return Some(action);
        }
    }
    None
}

pub(super) fn is_command(value: &str) -> bool {
    matches!(
        value,
        "late-action-held"
            | "late-action-interrupt"
            | "late-action-state"
            | "late-action-finish"
            | "late-action-replay"
            | "late-action-resume"
    ) || waiting::is_command(value)
}

#[derive(Default)]
pub(super) struct LateActionProbe {
    original: Option<(AuthorityClaim, DecisionDelivery)>,
    address: Option<HostExecutionAddress>,
    pause: Option<tokio::task::JoinHandle<Result<(), String>>>,
    interrupted: bool,
    joined: bool,
    replay_checked: bool,
    resumed: bool,
    waiting: waiting::CaptureWait,
}

impl LateActionProbe {
    pub(super) fn retain_delivery(&mut self, fixture: &TaskTransportFixture) -> Result<(), String> {
        if self.original.is_some() {
            return Ok(());
        }
        let claim = fixture
            .broker()
            .claim_continuation("view-app")
            .ok_or("original action claim missing")?;
        let delivery = fixture
            .client
            .computer_use_decision("view-app", "synthetic-view-agent")
            .ok_or("original action delivery missing")?;
        if delivery.was_admitted() {
            return Err("original action delivery admitted before pin".into());
        }
        self.original = Some((claim, delivery));
        Ok(())
    }

    pub(super) fn command(
        &mut self,
        command: &str,
        runtime: &tokio::runtime::Runtime,
        context: recovery::Context<'_>,
    ) -> Result<Value, String> {
        let chat_stop = context.scenario.ends_with("chat-stop");
        match command {
            "late-action-held" => {
                if self.address.is_some() || self.interrupted {
                    return Err("original held action pin repeated".into());
                }
                let view = context.fixture.original_host_task_view()?;
                let row = view
                    .executions
                    .iter()
                    .find(|row| row.run_id == context.approved.run_id && row.key == *context.key)
                    .ok_or("original held action execution missing")?;
                if row.phase != HostExecutionPhase::Busy {
                    return Err("original action was not admitted and busy".into());
                }
                self.address = Some(HostExecutionAddress {
                    key: row.key.clone(),
                    run_id: row.run_id.clone(),
                    generation: row.generation.clone(),
                });
                self.snapshot(&context)
            }
            "late-action-interrupt" => {
                if self.interrupted || self.address.is_none() {
                    return Err("original action interruption misplaced".into());
                }
                self.interrupted = true;
                if chat_stop {
                    context.manager.fence_computer_use_chat_stop("view-app");
                } else {
                    let address = self.address.clone().unwrap();
                    let manager = context.manager.clone();
                    // Keep the same waiter while its native cleanup may depend on held rows.
                    self.pause = Some(runtime.spawn(async move {
                        manager
                            .pause_computer_use_task_execution("view-app", &address)
                            .await
                    }));
                }
                Ok(json!({"started":true}))
            }
            "late-action-finish" => {
                if !self.interrupted || self.joined {
                    return Err("original action interruption join misplaced".into());
                }
                self.finish(runtime)?;
                self.joined = true;
                if crate::computer_use::task_controls::original_worker_for_test()?.is_some() {
                    return Err("original action retained a control worker".into());
                }
                runtime.block_on(context.fixture.original_continuation_wire_empty())?;
                let reply = self.snapshot(&context)?;
                let row = &reply["state"]["executions"][0];
                if reply["executable"] != 0
                    || reply["pendingContinuation"] != false
                    || row["cleanupPending"] != false
                    || (chat_stop && reply["originalStopped"] != true)
                    || (!chat_stop && row["phase"] != "paused")
                {
                    return Err(
                        "original action did not physically settle after interruption".into(),
                    );
                }
                Ok(reply)
            }
            "late-action-replay" => {
                if !self.joined || self.replay_checked || self.resumed {
                    return Err("original action replay misplaced".into());
                }
                let (_, delivery) = self.original.as_ref().ok_or("original delivery missing")?;
                runtime.block_on(
                    context
                        .fixture
                        .consumed_continuation_cannot_restart_recovery(delivery, chat_stop),
                )?;
                self.replay_checked = true;
                self.snapshot(&context)
            }
            "late-action-resume" => {
                if chat_stop || !self.joined || !self.replay_checked || self.resumed {
                    return Err("new explicit action recovery misplaced".into());
                }
                let view = context.fixture.original_host_task_view()?;
                let row = &view.executions[0];
                let address = HostExecutionAddress {
                    key: row.key.clone(),
                    run_id: row.run_id.clone(),
                    generation: row.generation.clone(),
                };
                let outcome = runtime.block_on(
                    context
                        .manager
                        .resume_computer_use_task_execution("view-app", &address),
                )?;
                self.resumed = true;
                let mut reply = self.snapshot(&context)?;
                reply["outcome"] = serde_json::to_value(outcome).unwrap();
                Ok(reply)
            }
            "late-action-state" => {
                let mut reply = self.snapshot(&context)?;
                reply["pauseWaiterReady"] = json!(self
                    .pause
                    .as_ref()
                    .is_some_and(tokio::task::JoinHandle::is_finished));
                Ok(reply)
            }
            command if waiting::is_command(command) => {
                self.waiting_command(command, runtime, context)
            }
            _ => Err("unknown original late action command".into()),
        }
    }

    fn snapshot(&self, context: &recovery::Context<'_>) -> Result<Value, String> {
        let broker = context.fixture.broker();
        let state = context.fixture.original_host_task_view()?;
        let (claim, delivery) = self
            .original
            .as_ref()
            .ok_or("original action pin missing")?;
        if !context.fixture.original_transport_is_current()
            || !state.feature_enabled
            || context.approvals.owners() != 0
            || context.approvals.started() != 1
            || context.approvals.settings()["computerUseEnabled"] != true
            || !delivery.was_admitted()
            || state
                .executions
                .iter()
                .any(|row| row.run_id != context.approved.run_id || row.key != *context.key)
        {
            return Err("original action owner or settings changed".into());
        }
        Ok(json!({"state":state,
            "existingIdle":broker.tabs().existing_operations_idle(&context.approved.run_id),
            "executable":broker.executable_grant_count("view-app"),
            "pendingContinuation":broker.claim_continuation("view-app").is_some(),
            "retainedContinuationRecords":broker.continuation_count("view-app"),
            "originalClaimRetired":broker.validate_continuation(claim).is_err(),
            "originalDeliveryConsumed":true,"originalTransportRetained":true,"stored":true,
            "originalStopped":broker.stop_state(&context.approved.run_id).map_err(|e| e.to_string())? == StopState::Stopped,
            "originalWaiterJoined":self.joined,"replayChecked":self.replay_checked}))
    }

    pub(super) fn finish(&mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
        self.finish_pause(runtime)?;
        self.waiting.finish(runtime)
    }

    fn finish_pause(&mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
        if let Some(waiter) = self.pause.as_mut() {
            let result = runtime
                .block_on(waiter)
                .map_err(|_| "original action Pause waiter did not join")?;
            self.pause.take();
            result?;
        }
        Ok(())
    }

    pub(super) fn fence_pending(&self, broker: &crate::computer_use::ComputerUseBroker) {
        if self.waiting.is_pending() {
            broker.fence_task_consent("view-app");
        }
    }
}

pub(super) fn validate_evidence(scenario: &str, evidence: &Value) -> Result<(), String> {
    let action = action_name(scenario).ok_or("unsupported original late action scenario")?;
    let post = scenario.contains("-post-");
    let text_action = post && matches!(action, "set_value" | "type_text");
    let (native_status, native_detail) = match action {
        "click" => ("applied", "semantic_click_dispatched"),
        "set_value" | "type_text" => ("verified", "field_value_changed"),
        "scroll" => ("verified", "scroll_position_changed"),
        "wait" => ("verified", "condition_matched"),
        _ => return Err("unsupported original late action".into()),
    };
    let input_type = text_action.then_some(if action == "set_value" {
        "insertReplacementText"
    } else {
        "insertText"
    });
    if evidence["action"] != action
        || evidence["cut"] != if post { "post" } else { "pre" }
        || evidence["interruption"]
            != if scenario.ends_with("chat-stop") {
                "chat-stop"
            } else {
                "pause"
            }
        || evidence["nativeInvokes"] != usize::from(post)
        || evidence["clicks"] != usize::from(post && action == "click")
        || evidence["nativeStatus"] != json!(post.then_some(native_status))
        || evidence["nativeDetail"] != json!(post.then_some(native_detail))
        || evidence["originalActResult"]
            != json!({
                "status": if post { "unknown" } else { "rejected" },
                "detail": if post { "cancelled_after_dispatch" } else { "stale_snapshot" },
                "physicallySettled": true,
            })
        || evidence["textValueMatched"] != true
        || evidence["textEvents"]["beforeinput"] != usize::from(text_action)
        || evidence["textEvents"]["input"] != usize::from(text_action)
        || evidence["textEvents"]["change"] != usize::from(post && action == "set_value")
        || evidence["textEvents"]["inputType"] != json!(input_type)
        || evidence["scrollTop"] != if post && action == "scroll" { 60 } else { 0 }
        || [
            "originalActJoined",
            "originalCompletionJoined",
            "originalWaiterJoined",
            "scopeSettled",
            "cancelJoined",
            "oldDeliveryNotRevived",
            "zeroPeerWire",
            "pairRetained",
            "threeSharesRetained",
            "originalDocumentsRetained",
            "featureAndStoreRetained",
            "preflightOwnerMatched",
        ]
        .iter()
        .any(|field| evidence[field] != true)
    {
        return Err("original late action evidence incomplete".into());
    }
    println!("original late action native witness: {evidence}");
    Ok(())
}

fn checks(chat_stop: bool) -> Vec<&'static str> {
    let mut checks = CHECKS[..6].to_vec();
    checks.extend([
        "original-grant-fresh-observation-no-input",
        "original-action-native-return-held",
        "original-action-interrupted-without-authority",
        "original-action-late-return-rejected-and-joined",
        "original-action-old-delivery-and-shares-retained",
    ]);
    if !chat_stop {
        checks.push("original-action-new-explicit-continue");
    }
    checks.push("original-action-final-stop-keeps-shares");
    checks
}

#[test]
#[ignore = "manual native gate: original click preflight/Host Pause"]
fn existing_task_joint_late_click_pre_pause() {
    scenario("active-late-click-pre-pause", &checks(false));
}
#[test]
#[ignore = "manual native gate: original applied click result/Host Pause"]
fn existing_task_joint_late_click_post_pause() {
    scenario("active-late-click-post-pause", &checks(false));
}
#[test]
#[ignore = "manual native gate: original click preflight/Chat Stop"]
fn existing_task_joint_late_click_pre_chat_stop() {
    scenario("active-late-click-pre-chat-stop", &checks(true));
}
#[test]
#[ignore = "manual native gate: original applied click result/Chat Stop"]
fn existing_task_joint_late_click_post_chat_stop() {
    scenario("active-late-click-post-chat-stop", &checks(true));
}

macro_rules! late_action_test {
    ($name:ident, $scenario:literal, $chat_stop:expr) => {
        #[test]
        #[ignore = "manual native gate: original action return and Host interruption"]
        fn $name() {
            scenario($scenario, &checks($chat_stop));
        }
    };
}

late_action_test!(
    existing_task_joint_late_set_value_pre_pause,
    "active-late-set-value-pre-pause",
    false
);
late_action_test!(
    existing_task_joint_late_set_value_post_pause,
    "active-late-set-value-post-pause",
    false
);
late_action_test!(
    existing_task_joint_late_set_value_pre_chat_stop,
    "active-late-set-value-pre-chat-stop",
    true
);
late_action_test!(
    existing_task_joint_late_set_value_post_chat_stop,
    "active-late-set-value-post-chat-stop",
    true
);
late_action_test!(
    existing_task_joint_late_type_text_pre_pause,
    "active-late-type-text-pre-pause",
    false
);
late_action_test!(
    existing_task_joint_late_type_text_post_pause,
    "active-late-type-text-post-pause",
    false
);
late_action_test!(
    existing_task_joint_late_type_text_pre_chat_stop,
    "active-late-type-text-pre-chat-stop",
    true
);
late_action_test!(
    existing_task_joint_late_type_text_post_chat_stop,
    "active-late-type-text-post-chat-stop",
    true
);
late_action_test!(
    existing_task_joint_late_scroll_pre_pause,
    "active-late-scroll-pre-pause",
    false
);
late_action_test!(
    existing_task_joint_late_scroll_post_pause,
    "active-late-scroll-post-pause",
    false
);
late_action_test!(
    existing_task_joint_late_scroll_pre_chat_stop,
    "active-late-scroll-pre-chat-stop",
    true
);
late_action_test!(
    existing_task_joint_late_scroll_post_chat_stop,
    "active-late-scroll-post-chat-stop",
    true
);
late_action_test!(
    existing_task_joint_late_wait_pre_pause,
    "active-late-wait-pre-pause",
    false
);
late_action_test!(
    existing_task_joint_late_wait_post_pause,
    "active-late-wait-post-pause",
    false
);
late_action_test!(
    existing_task_joint_late_wait_pre_chat_stop,
    "active-late-wait-pre-chat-stop",
    true
);
late_action_test!(
    existing_task_joint_late_wait_post_chat_stop,
    "active-late-wait-post-chat-stop",
    true
);
