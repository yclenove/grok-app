//! Held original native recovery versus one isolated App owner replacement.
use super::*;
use crate::acp_client::computer_use::DecisionDelivery;
use grok_computer_use_core::broker::{HostExecutionPhase, HostTaskView};
use grok_computer_use_core::task_consent::AuthorityClaim;

type OriginalReader = Arc<dyn Fn() -> Result<HostTaskView, String> + Send + Sync>;
type OriginalDelivery = (AuthorityClaim, DecisionDelivery);

#[derive(Clone, Copy, PartialEq)]
enum Identity {
    Process,
    Agent,
    Acp,
    Removed,
}

fn parse(scenario: &str) -> Option<Identity> {
    Some(match scenario {
        "active-host-resume-process-replaced" => Identity::Process,
        "active-host-resume-agent-replaced" => Identity::Agent,
        "active-host-resume-acp-replaced" => Identity::Acp,
        "active-host-resume-slot-removed" => Identity::Removed,
        _ => return None,
    })
}

pub(in super::super) fn is_scenario(scenario: &str) -> bool {
    parse(scenario).is_some()
}

pub(super) fn is_command(command: &str) -> bool {
    matches!(
        command,
        "host-resume-owner-replace"
            | "host-resume-owner-finish"
            | "host-resume-owner-replay"
            | "host-resume-owner-state"
    )
}

#[derive(Default)]
pub(super) struct RecoveryOwnerProbe {
    endpoint: Option<McpEndpoint>,
    reader: Option<OriginalReader>,
    replacement: Option<TaskTransportFixture>,
    removed: Option<crate::session_manager::types::LiveSession>,
    identity: Option<Identity>,
    joined: bool,
    replay_checked: bool,
}

impl RecoveryOwnerProbe {
    pub(super) fn command(
        &mut self,
        command: &str,
        runtime: &tokio::runtime::Runtime,
        context: &mut Context<'_>,
        race: &mut races::RecoveryRaceProbe,
        original: &OriginalDelivery,
    ) -> Result<Value, String> {
        match command {
            "host-resume-owner-replace" => {
                if self.identity.is_some() || self.endpoint.is_some() || self.reader.is_some() {
                    return Err("original recovery owner replacement repeated".into());
                }
                race.require_original_pending()?;
                let identity =
                    parse(context.scenario).ok_or("fixed recovery owner scenario missing")?;
                // Pin everything before mutation; later readback never follows the current slot.
                let endpoint = context.manager.task_picker_endpoint("view-app")?;
                if !Arc::ptr_eq(&endpoint.acp, &context.fixture.client)
                    || !context.fixture.original_transport_is_current()
                {
                    return Err("original recovery endpoint already changed".into());
                }
                let reader: OriginalReader = Arc::new(context.fixture.original_host_task_reader());
                self.endpoint = Some(endpoint);
                self.reader = Some(reader);
                self.require_original_state(context, original, HostExecutionPhase::Resuming)?;
                if identity == Identity::Acp {
                    self.replacement =
                        Some(runtime.block_on(TaskTransportFixture::new_existing_http()));
                }
                {
                    let mut slot = context.manager.inner.lock();
                    let current = slot.as_mut().ok_or("original recovery slot missing")?;
                    match identity {
                        Identity::Process => {
                            current.process_id = "replacement-recovery-process".into()
                        }
                        Identity::Agent => {
                            current.meta.agent_session_id =
                                Some("replacement-recovery-agent".into())
                        }
                        Identity::Acp => {
                            current.acp = Some(self.replacement.as_ref().unwrap().client.clone())
                        }
                        Identity::Removed => self.removed = slot.take(),
                    }
                }
                self.identity = Some(identity);
                race.require_original_pending()?;
                self.snapshot(context, original)
            }
            "host-resume-owner-finish" => {
                if self.identity.is_none() || self.joined {
                    return Err("original recovery owner join misplaced".into());
                }
                race.reject_with_and_join(runtime, "Computer Use request owner retired")?;
                self.joined = true;
                runtime.block_on(context.fixture.original_continuation_wire_empty())?;
                if let Some(replacement) = self.replacement.as_mut() {
                    runtime.block_on(replacement.original_continuation_wire_empty())?;
                }
                let mut reply = self.snapshot(context, original)?;
                for field in [
                    "originalWorkerJoined",
                    "originalWorkerRetired",
                    "originalWaiterJoined",
                    "zeroOriginalPeerWire",
                    "zeroReplacementPeerWire",
                ] {
                    reply[field] = json!(true);
                }
                reply["exactRejection"] = json!("Computer Use request owner retired");
                Ok(reply)
            }
            "host-resume-owner-replay" => {
                if !self.joined || self.replay_checked {
                    return Err("original recovery owner replay misplaced".into());
                }
                runtime.block_on(
                    context
                        .fixture
                        .consumed_continuation_cannot_restart_recovery(&original.1, false),
                )?;
                if let Some(replacement) = self.replacement.as_mut() {
                    runtime.block_on(replacement.original_continuation_wire_empty())?;
                }
                self.replay_checked = true;
                self.snapshot(context, original)
            }
            "host-resume-owner-state" => self.snapshot(context, original),
            _ => Err("unknown original recovery owner command".into()),
        }
    }

    fn require_original_state(
        &self,
        context: &Context<'_>,
        original: &OriginalDelivery,
        phase: HostExecutionPhase,
    ) -> Result<HostTaskView, String> {
        let state = self
            .reader
            .as_ref()
            .ok_or("original recovery reader missing")?()?;
        let broker = context.fixture.broker();
        if !state.feature_enabled
            || state
                .request
                .as_ref()
                .is_none_or(|row| row.key != *context.key)
            || state.executions.len() != 1
            || state.grants.len() != 1
            || state.executions[0].key != *context.key
            || state.executions[0].run_id != context.approved.run_id
            || state.executions[0].phase != phase
            || state.executions[0].has_fresh_observation
            || broker.executable_grant_count("view-app") != 0
            || broker.claim_continuation("view-app").is_some()
            || broker.validate_continuation(&original.0).is_ok()
            || !original.1.was_admitted()
            || !context.fixture.client.is_alive()
            || !context.fixture.original_transport_is_current()
            || context.approvals.owners() != 0
            || context.approvals.started() != 1
            || context.approvals.settings()["computerUseEnabled"] != true
        {
            return Err(
                "original recovery owner replacement retained authority or lost its pinned state"
                    .into(),
            );
        }
        Ok(state)
    }

    fn witness(&self, context: &Context<'_>) -> Result<Value, String> {
        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or("original recovery endpoint missing")?;
        let identity = self
            .identity
            .ok_or("original recovery replacement missing")?;
        let slot = context.manager.inner.lock();
        let unchanged = slot.as_ref().map(|current| {
            [
                current.app_session_id == "view-app",
                current.process_id == endpoint.process_id,
                current.meta.agent_session_id.as_deref()
                    == Some(endpoint.agent_session_id.as_str()),
                current
                    .acp
                    .as_ref()
                    .is_some_and(|acp| Arc::ptr_eq(acp, &endpoint.acp)),
            ]
        });
        let expected = match identity {
            Identity::Process => Some([true, false, true, true]),
            Identity::Agent => Some([true, true, false, true]),
            Identity::Acp => Some([true, true, true, false]),
            Identity::Removed => None,
        };
        let precise_replacement = match identity {
            Identity::Process => slot
                .as_ref()
                .is_some_and(|current| current.process_id == "replacement-recovery-process"),
            Identity::Agent => slot.as_ref().is_some_and(|current| {
                current.meta.agent_session_id.as_deref() == Some("replacement-recovery-agent")
            }),
            Identity::Acp => slot.as_ref().is_some_and(|current| {
                current.acp.as_ref().is_some_and(|acp| {
                    self.replacement
                        .as_ref()
                        .is_some_and(|replacement| Arc::ptr_eq(acp, &replacement.client))
                })
            }),
            Identity::Removed => self.removed.as_ref().is_some_and(|original| {
                original.app_session_id == "view-app"
                    && original.process_id == endpoint.process_id
                    && original.meta.agent_session_id.as_deref()
                        == Some(endpoint.agent_session_id.as_str())
                    && original
                        .acp
                        .as_ref()
                        .is_some_and(|acp| Arc::ptr_eq(acp, &endpoint.acp))
            }),
        };
        if unchanged != expected
            || !precise_replacement
            || !context.manager.background.lock().is_empty()
            || !context.manager.parked.lock().is_empty()
        {
            return Err("late recovery changed or followed a replacement App slot".into());
        }
        drop(slot);
        let replacement = self
            .replacement
            .as_ref()
            .map(TaskTransportFixture::original_host_task_view)
            .transpose()?;
        if replacement.as_ref().is_some_and(|view| {
            view.request.is_some() || !view.grants.is_empty() || !view.executions.is_empty()
        }) || self.replacement.as_ref().is_some_and(|peer| {
            !peer.client.is_alive()
                || !peer.original_transport_is_current()
                || peer.broker().executable_grant_count("view-app") != 0
                || peer.broker().claim_continuation("view-app").is_some()
        }) {
            return Err("late original recovery granted a replacement ACP peer authority".into());
        }
        Ok(
            json!({"slotExists":slot_exists(identity),"unchanged":unchanged,
            "preciseReplacementRetained":true,"originalAcpAlive":true,"originalTransportRetained":true,
            "removedOriginalRetained":self.removed.is_some(),"replacementPeerExists":self.replacement.is_some(),
            "replacementRequestExists":false,"replacementGrants":0,"replacementExecutions":0,"replacementAvailableClaims":0}),
        )
    }

    fn snapshot(
        &self,
        context: &Context<'_>,
        original: &OriginalDelivery,
    ) -> Result<Value, String> {
        let phase = if self.joined {
            HostExecutionPhase::Paused
        } else {
            HostExecutionPhase::Resuming
        };
        let state = self.require_original_state(context, original, phase)?;
        Ok(
            json!({"state":state,"scenario":context.scenario,"stored":true,"executable":0,"pendingContinuation":false,
            "retainedContinuationRecords":context.fixture.broker().continuation_count("view-app"),
            "originalClaimRetired":true,"originalDeliveryConsumed":true,"ownerWitness":self.witness(context)?,
            "replayChecked":self.replay_checked}),
        )
    }

    pub(super) async fn finish(&mut self) {
        self.removed.take();
        if let Some(replacement) = self.replacement.take() {
            replacement.finish().await;
            println!("original recovery replacement ACP peer actual finish joined independently");
        }
    }
}

fn slot_exists(identity: Identity) -> bool {
    identity != Identity::Removed
}

pub(in super::super) fn validate_evidence(scenario: &str, evidence: &Value) -> Result<(), String> {
    if parse(scenario).is_none()
        || evidence["scenario"] != scenario
        || evidence["nativeCaptures"] != 1
        || evidence["exactRejection"] != "Computer Use request owner retired"
        || [
            "originalDocumentMatched",
            "originalWorkerRetained",
            "originalWaiterPending",
            "originalWorkerJoined",
            "originalWorkerRetired",
            "originalWaiterJoined",
            "originalNativeResultReturned",
            "zeroOriginalPeerWire",
            "zeroReplacementPeerWire",
            "originalClaimRetired",
            "oldDeliveryNotRevived",
            "originalDocumentsRetained",
            "threeSharesRetained",
            "pairRetained",
            "featureAndStoreRetained",
            "replacementSlotUnchanged",
        ]
        .iter()
        .any(|field| evidence[field] != true)
    {
        return Err("original recovery owner replacement evidence incomplete".into());
    }
    println!("original recovery owner replacement witness: {evidence}");
    Ok(())
}

fn owner_checks() -> Vec<&'static str> {
    let mut checks = RECOVERY_CHECKS[..8].to_vec();
    checks.extend([
        "original-recovery-native-return-held",
        "original-recovery-owner-replaced-without-authority",
        "original-recovery-owner-late-return-rejected-and-joined",
        "original-recovery-owner-old-delivery-and-shares-retained",
    ]);
    checks
}

#[test]
#[ignore = "manual native gate: CFT140+, held original recovery versus process replacement"]
fn existing_task_joint_host_resume_owner_process_replacement() {
    scenario("active-host-resume-process-replaced", &owner_checks());
}
#[test]
#[ignore = "manual native gate: CFT140+, held original recovery versus agent replacement"]
fn existing_task_joint_host_resume_owner_agent_replacement() {
    scenario("active-host-resume-agent-replaced", &owner_checks());
}
#[test]
#[ignore = "manual native gate: CFT140+, held original recovery versus independent real ACP Arc"]
fn existing_task_joint_host_resume_owner_acp_replacement() {
    scenario("active-host-resume-acp-replaced", &owner_checks());
}
#[test]
#[ignore = "manual native gate: CFT140+, held original recovery versus removed original slot"]
fn existing_task_joint_host_resume_owner_slot_removed() {
    scenario("active-host-resume-slot-removed", &owner_checks());
}
