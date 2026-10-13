//! Real MV3/HTTP/MCP/DOM effects via the original App/ACP TaskGrant publication.
//! App gestures, session and ACP peer are synthetic; first request uses real MCP.
use super::*;
use crate::acp_client::computer_use::TaskTransportFixture;
use crate::computer_use::test_support::{
    remove_owned_root, run_fixture_cleanup_watch, ExtensionPeer, FixtureApprovals,
};
use grok_computer_use_core::broker::StopState;
use grok_computer_use_core::task_consent::HostRequestKey;
use serde_json::json;
use std::{sync::atomic::AtomicUsize, time::Instant};
mod publication;
use publication::PublicationProbe;
mod reservation;
use reservation::ReservationProbe;
mod discovery;
mod evidence;
use discovery::DiscoveryProbe;
mod recovery;
use recovery::RecoveryProbe;
mod bootstrap;
mod browser_replacement;
mod capture;
mod chat_stop;
mod connection_replacement;
mod late_actions;
mod owner_replacement;
mod preapproval_lease;
mod publication_terminals;
mod readback;
mod reservation_terminals;
mod terminals;
const CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "new-task-grant-original-app-publication",
    "new-grant-unselected-member-c-has-no-access",
    "new-grant-mcp-click-and-dedupe",
    "new-grant-mcp-cjk-set-and-type",
    "new-grant-mcp-wait-and-scroll",
    "new-grant-mcp-owner-and-stale-rejected",
    "new-grant-member-b-five-actions",
    "new-grant-switch-back-a-retires-old-refs",
    "new-grant-stop-keeps-real-share-and-tab",
];

const REVOKED_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "old-picker-revoked-share-cannot-approve",
    "original-task-zero-authority-unselected-share-retained",
];

const PUBLICATION_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "prepared-publication-unshare-cannot-publish",
    "original-task-zero-authority-unselected-share-retained",
];

const RESERVATION_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "original-owned-reservation-unshare-cannot-prepare",
    "original-task-zero-authority-unselected-share-retained",
];

const DISCOVERY_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "original-owned-discovery-unshare-cannot-return-stale-receipt",
    "original-task-zero-authority-unselected-share-retained",
];

const CONNECTION_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "original-connection-forget-rejects-old-authority",
    "original-task-zero-authority-tabs-retained",
];

const REPAIR_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "new-task-grant-original-app-publication",
    "new-grant-unselected-member-c-has-no-access",
    "original-grant-fresh-observation-no-input",
    "real-source-repair-retires-original-connection",
    "old-grant-denied-before-and-after-fresh-share",
    "old-run-stop-retains-new-connection-shares",
];

const LEASE_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "new-task-grant-original-app-publication",
    "new-grant-unselected-member-c-has-no-access",
    "original-grant-fresh-observation-no-input",
    "original-thirty-second-lease-expires-without-disconnect",
    "old-grant-denied-before-and-after-fresh-share",
    "old-run-stop-retains-new-connection-shares",
];

const RECOVERY_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "new-task-grant-original-app-publication",
    "new-grant-unselected-member-c-has-no-access",
    "original-grant-fresh-observation-no-input",
    "original-host-pause-denies-model-dispatch",
    "original-host-resume-fresh-semantic-observation-once",
    "resumed-old-refs-denied-fresh-click-deduped",
    "resumed-original-group-switch-retains-unselected-isolation",
    "resumed-stop-keeps-real-shares-and-tabs",
];

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_host_pause_resume_keeps_original_group_and_one_continuation() {
    scenario("active-host-resume", RECOVERY_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_active_connection_repair_cannot_revive_original_grant() {
    scenario("active-connection-repair", REPAIR_CHECKS);
}

#[test]
#[ignore = "manual real 30-second lease: explicit Chrome-for-Testing 140+, unchanged product clock"]
fn existing_task_joint_active_lease_expiry_cannot_revive_original_grant() {
    scenario("active-lease-expiry", LEASE_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_connection_forget_before_choice_rejects_original_candidates() {
    scenario("choose-forget", CONNECTION_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_connection_forget_during_reservation_rejects_original_worker() {
    scenario("reservation-forget", CONNECTION_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_connection_forget_before_publication_fences_original_authority() {
    scenario("publication-forget", CONNECTION_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_discovery_active_unshare_rejects_original_owned_snapshot() {
    scenario("discovery-active-unshare", DISCOVERY_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_discovery_inactive_unshare_rejects_original_owned_snapshot() {
    scenario("discovery-inactive-unshare", DISCOVERY_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_source_mv3_and_mcp_use_new_task_grant() {
    scenario("success", CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_unshare_before_choice_rejects_original_candidate() {
    scenario("choose-unshare", REVOKED_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_same_document_reshare_cannot_adopt_original_candidate() {
    scenario("choose-reshare", REVOKED_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_same_url_reload_reshare_cannot_adopt_original_candidate() {
    scenario("choose-reload", REVOKED_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_publication_active_unshare_fences_prepared_authority() {
    scenario("publication-active-unshare", PUBLICATION_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_publication_inactive_unshare_fences_entire_prepared_group() {
    scenario("publication-inactive-unshare", PUBLICATION_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_reservation_active_unshare_rejects_original_owned_worker() {
    scenario("reservation-active-unshare", RESERVATION_CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_reservation_inactive_unshare_rejects_original_owned_group() {
    scenario("reservation-inactive-unshare", RESERVATION_CHECKS);
}

fn scenario(scenario: &'static str, checks: &[&str]) {
    let dormant_bootstrap = scenario == "startup-dormant";
    let preapproval_lease = preapproval_lease::is_scenario(scenario);
    let owner_replacement = owner_replacement::is_scenario(scenario);
    let preapproval_replacement = matches!(
        scenario,
        "choose-connection-repair"
            | "reservation-connection-repair"
            | "publication-connection-repair"
    );
    let browser_lifecycle = matches!(
        scenario,
        "active-browser-reopen" | "active-profile-replacement" | "active-profile-tab-id-collision"
    );
    let active_lifecycle = matches!(
        scenario,
        "active-connection-repair" | "active-lease-expiry" | "active-chat-stop-queued"
    ) || browser_lifecycle
        || recovery::is_scenario(scenario)
        || late_actions::is_scenario(scenario)
        || capture::is_scenario(scenario);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    // Reuse the exact isolated approval-root cleanup contract, not user settings.
    let root = temporary.join(format!("grok-cu-managed-task-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let approvals = Arc::new(FixtureApprovals::new(&root));
    let mut f = runtime.block_on(TaskTransportFixture::new_existing_http());
    let manager = manager_from_client(f.client.clone());
    let broker = f.broker();
    if preapproval_replacement {
        // Re-Pair is an enabled Host operation. Persist the explicit synthetic
        // setup BEFORE the first request; never enable midway to rescue old refs.
        runtime
            .block_on(approvals.enable_before_request(manager.clone(), broker.clone()))
            .unwrap();
    }
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let watcher = runtime.spawn(run_fixture_cleanup_watch(
        broker.clone(),
        stopped,
        Arc::new(AtomicUsize::new(0)),
    ));
    let mut peer = ExtensionPeer::spawn(scenario).unwrap();
    let mut picker = None::<String>;
    let mut request_key = None::<HostRequestKey>;
    let mut approved = None::<grok_computer_use_core::task_consent::CommitOutcome>;
    let mut rejected = false;
    let mut repair_started = false;
    let mut publication = PublicationProbe::default();
    let mut reservation = ReservationProbe::default();
    let mut discovery = DiscoveryProbe::default();
    let mut recovery = RecoveryProbe::default();
    let mut late_action = late_actions::LateActionProbe::default();
    let mut owner = owner_replacement::OwnerReplacementProbe::default();
    let mut terminal = terminals::TerminalProbe::default();
    let mut reservation_terminal = reservation_terminals::ReservationTerminalProbe::default();
    let mut publication_terminal = publication_terminals::PublicationTerminalProbe::default();
    let mut chat_stop = chat_stop::ChatStopProbe::default();
    let original_started = Instant::now();
    let result = (|| -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if Instant::now() >= deadline {
                return Err("native new TaskGrant joint deadline".into());
            }
            let value = peer.next()?;
            if value["event"] == "failed" {
                return Err(format!(
                    "native new TaskGrant failed at {}",
                    value["stage"].as_str().unwrap_or("unknown")
                ));
            }
            if value["event"] == "passed" {
                evidence::validate(&value, scenario, checks, active_lifecycle, rejected)?;
                return Ok(());
            }
            let original_key = request_key.clone();
            let key = || {
                original_key
                    .as_ref()
                    .ok_or("original MCP request not fixed")
            };
            let reply = match value["command"].as_str().unwrap_or_default() {
                "ready" => json!({"ok":true}),
                // Only this private child pipe receives the fixture-created transport token.
                "mcp-binding" => f.native_mcp_binding(),
                "before-request" => {
                    let state = manager.computer_use_task_view("view-app")?;
                    if state.request.is_some()
                        || state.feature_enabled != preapproval_replacement
                        || !state.grants.is_empty()
                        || !state.executions.is_empty()
                        || broker.continuation_count("view-app") != 0
                    {
                        return Err("native fixture pre-created request or authority".into());
                    }
                    json!({"state":state})
                }
                "begin" => {
                    let state = manager.computer_use_task_view("view-app")?;
                    if state.feature_enabled != preapproval_replacement
                        || !state.grants.is_empty()
                        || !state.executions.is_empty()
                    {
                        return Err("request/readback created execution".into());
                    }
                    if request_key.is_some() {
                        return Err("joint fixture repeated original begin".into());
                    }
                    let current = state.request.as_ref().ok_or("first MCP request missing")?;
                    let selected_key = current.key.clone();
                    let started = serde_json::to_value(manager.begin_computer_use_task_picker(
                        "view-app",
                        &selected_key,
                        "existing-tabs",
                    )?)
                    .unwrap();
                    let id = started["pickerId"]
                        .as_str()
                        .ok_or("original picker missing")?
                        .to_owned();
                    let preparation = manager.computer_use_task_picker_preparation(
                        "view-app",
                        &selected_key,
                        &id,
                    )?;
                    request_key = Some(selected_key);
                    picker = Some(id);
                    json!({"state":state,"preparation":preparation})
                }
                "preparation" => {
                    serde_json::to_value(manager.computer_use_task_picker_preparation(
                        "view-app",
                        key()?,
                        picker.as_deref().ok_or("original picker missing")?,
                    )?)
                    .unwrap()
                }
                "confirm" => json!({"ok": manager.confirm_computer_use_task_picker_pairing(
                    "view-app", key()?, picker.as_deref().ok_or("original picker missing")?,
                    value["nonce"].as_str().unwrap_or_default(),
                ).is_ok()}),
                "load" => match runtime.block_on(manager.load_computer_use_task_picker(
                    "view-app",
                    key()?,
                    picker.as_deref().ok_or("original picker missing")?,
                )) {
                    Ok(view) => json!({"ok":true,"view":view}),
                    Err(_) => json!({"ok":false}),
                },
                "start-discovery" => {
                    if !scenario.starts_with("discovery-") {
                        return Err("discovery gate unavailable in this private scenario".into());
                    }
                    discovery.start(
                        &runtime,
                        manager.clone(),
                        key()?,
                        picker.as_deref().ok_or("original picker missing")?,
                        &broker,
                    )?;
                    json!({"started":true})
                }
                "discovery-ready" => discovery.ready(&manager, &broker, &approvals)?,
                "terminal-arm" => {
                    if !terminals::is_scenario(scenario) {
                        return Err("terminal witness unavailable in this scenario".into());
                    }
                    terminal.arm(&broker)?
                }
                "terminal-reject" => {
                    let candidates = serde_json::from_value(value["candidates"].clone())
                        .map_err(|_| "invalid original terminal selection")?;
                    let reply = terminal.reject(
                        &runtime,
                        owner_replacement::OriginalOwner {
                            manager: &manager,
                            fixture: &f,
                            key: key()?,
                        },
                        (
                            picker.as_deref().ok_or("original picker missing")?,
                            candidates,
                        ),
                        &mut discovery,
                        &approvals,
                        scenario,
                    )?;
                    rejected = true;
                    reply
                }
                "terminal-authority" => terminal.authority(
                    owner_replacement::OriginalOwner {
                        manager: &manager,
                        fixture: &f,
                        key: key()?,
                    },
                    &approvals,
                    scenario,
                )?,
                "finish-discovery" => {
                    let outcome = discovery.release_and_join(&runtime)?;
                    rejected = outcome.as_ref().err().map(String::as_str)
                        == Some("Computer Use scope picker unavailable or retired");
                    let state = manager.computer_use_task_view("view-app")?;
                    let owners = crate::computer_use::task_picker::discovery_owners_for_test();
                    let grants = broker.executable_grant_count("view-app");
                    let continuations = broker.continuation_count("view-app");
                    println!("existing discovery authority witness: rejected={rejected}, grants={grants}, continuations={continuations}, discovery_workers={owners}");
                    json!({"rejected":rejected,"at":"original-owned-discovery","state":state,
                        "discoveryWorkers":owners,"ownedWorkers":approvals.owners(),"continuations":continuations,
                        "stored":approvals.settings()["computerUseEnabled"]})
                }
                "approve" => {
                    if scenario != "success" && !active_lifecycle {
                        return Err("negative fixture cannot request a successful approval".into());
                    }
                    if approved.is_some() {
                        return Err("joint fixture repeated approval".into());
                    }
                    let candidates: Vec<String> =
                        serde_json::from_value(value["candidates"].clone())
                            .map_err(|_| "invalid private fixture selection")?;
                    let endpoint = manager.task_picker_endpoint("view-app")?;
                    let outcome = runtime.block_on(f.client.approve_existing_fixture(
                        key()?,
                        picker.as_deref().ok_or("original picker missing")?,
                        candidates,
                        manager.task_approval_pin("view-app", &endpoint),
                        approvals.clone(),
                    ))?;
                    runtime.block_on(approvals.join())?;
                    if approvals.owners() != 0
                        || broker.executable_grant_count("view-app") != 1
                        || broker.continuation_count("view-app") != 1
                    {
                        return Err(
                            "original owned approval/TaskGrant did not publish exactly once".into(),
                        );
                    }
                    // Only the fields consumed by this private fixture cross the child pipe.
                    let reply = json!({
                        "ok":true,
                        "outcome": {
                            "runId": outcome.run_id,
                            "binding": { "targetId": outcome.binding.target_id },
                        },
                        "state":manager.computer_use_task_view("view-app")?,
                    });
                    approved = Some(outcome);
                    reply
                }
                "reject-approval" => {
                    if scenario == "success" || rejected || approved.is_some() {
                        return Err("joint fixture repeated or misplaced rejection".into());
                    }
                    let candidates: Vec<String> =
                        serde_json::from_value(value["candidates"].clone())
                            .map_err(|_| "invalid private fixture selection")?;
                    let endpoint = manager.task_picker_endpoint("view-app")?;
                    let outcome = runtime.block_on(f.client.approve_existing_fixture(
                        key()?,
                        picker.as_deref().ok_or("original picker missing")?,
                        candidates,
                        manager.task_approval_pin("view-app", &endpoint),
                        approvals.clone(),
                    ));
                    runtime.block_on(approvals.join())?;
                    if outcome.as_ref().err().map(String::as_str)
                        != Some("Computer Use selected window instance retired or changed")
                    {
                        return Err("stale share was not rejected by original reservation".into());
                    }
                    let state = manager.computer_use_task_view("view-app")?;
                    if state.request.as_ref().map(|request| &request.key) != Some(key()?)
                        || state.feature_enabled != preapproval_replacement
                        || !state.grants.is_empty()
                        || !state.executions.is_empty()
                        || approvals.owners() != 0
                        || approvals.settings()["computerUseEnabled"] != preapproval_replacement
                        || broker.executable_grant_count("view-app") != 0
                        || broker.continuation_count("view-app") != 0
                    {
                        return Err(
                            "rejected share changed original owner or published authority".into(),
                        );
                    }
                    rejected = true;
                    json!({"rejected":true,"at":"original-reservation","state":state,
                        "ownedWorkers":approvals.owners(),"continuations":0})
                }
                "reservation-terminal-reject" => {
                    let reply = reservation_terminal.reject(
                        &runtime,
                        owner_replacement::OriginalOwner {
                            manager: &manager,
                            fixture: &f,
                            key: key()?,
                        },
                        picker.as_deref().ok_or("original picker missing")?,
                        &mut reservation,
                        &approvals,
                        scenario,
                    )?;
                    rejected = true;
                    reply
                }
                "reservation-terminal-authority" => reservation_terminal.authority(
                    owner_replacement::OriginalOwner {
                        manager: &manager,
                        fixture: &f,
                        key: key()?,
                    },
                    &reservation,
                    &approvals,
                    scenario,
                )?,
                "publication-terminal-reject" => {
                    let reply = publication_terminal.reject(
                        &runtime,
                        owner_replacement::OriginalOwner {
                            manager: &manager,
                            fixture: &f,
                            key: key()?,
                        },
                        picker.as_deref().ok_or("original picker missing")?,
                        &mut publication,
                        &approvals,
                        scenario,
                    )?;
                    rejected = true;
                    reply
                }
                "publication-terminal-authority" => publication_terminal.authority(
                    owner_replacement::OriginalOwner {
                        manager: &manager,
                        fixture: &f,
                        key: key()?,
                    },
                    &publication,
                    &approvals,
                    scenario,
                )?,
                "start-publication" => {
                    if !scenario.starts_with("publication-") {
                        return Err("publication gate unavailable in this private scenario".into());
                    }
                    let candidates: Vec<String> =
                        serde_json::from_value(value["candidates"].clone())
                            .map_err(|_| "invalid private fixture selection")?;
                    let endpoint = manager.task_picker_endpoint("view-app")?;
                    publication.start(
                        &runtime,
                        f.client.clone(),
                        (
                            key()?,
                            picker.as_deref().ok_or("original picker missing")?,
                            candidates,
                        ),
                        manager.task_approval_pin("view-app", &endpoint),
                        approvals.clone(),
                    )?;
                    json!({"started":true})
                }
                "publication-ready" => {
                    let mut ready = publication.ready(&manager, &broker, &approvals)?;
                    if preapproval_lease && ready["ready"] == true {
                        preapproval_lease::attach_lease(&mut ready, &broker);
                    }
                    ready
                }
                "start-reservation" => {
                    if !scenario.starts_with("reservation-") {
                        return Err("reservation gate unavailable in this private scenario".into());
                    }
                    let candidates: Vec<String> =
                        serde_json::from_value(value["candidates"].clone())
                            .map_err(|_| "invalid private fixture selection")?;
                    let endpoint = manager.task_picker_endpoint("view-app")?;
                    let pin = manager.task_approval_pin("view-app", &endpoint);
                    let pin = if owner_replacement {
                        reservation.record_original_pin(pin, f.original_host_task_reader())
                    } else {
                        pin
                    };
                    reservation.start(
                        &runtime,
                        f.client.clone(),
                        (
                            key()?,
                            picker.as_deref().ok_or("original picker missing")?,
                            candidates,
                        ),
                        pin,
                        approvals.clone(),
                        &broker,
                    )?;
                    json!({"started":true})
                }
                "reservation-ready" => {
                    let mut ready = reservation.ready(&manager, &broker, &approvals)?;
                    if reservation_terminals::is_scenario(scenario) && ready["ready"] == true {
                        reservation.capture_original_entry()?;
                        ready["originalRegistryCaptured"] = json!(true);
                    }
                    if preapproval_lease && ready["ready"] == true {
                        preapproval_lease::attach_lease(&mut ready, &broker);
                    }
                    ready
                }
                "finish-reservation" => {
                    let outcome = reservation.release_and_join(&runtime)?;
                    runtime.block_on(approvals.join())?;
                    rejected = outcome.as_ref().err().map(String::as_str)
                        == Some(if owner_replacement {
                            owner_replacement::rejection(scenario)?
                        } else {
                            "Computer Use selected window instance retired or changed"
                        });
                    let state = if owner_replacement {
                        f.original_host_task_view()?
                    } else {
                        manager.computer_use_task_view("view-app")?
                    };
                    let grants = broker.executable_grant_count("view-app");
                    let continuations = broker.continuation_count("view-app");
                    let owners = crate::computer_use::task_picker::reservation_owners_for_test();
                    println!("existing reservation authority witness: rejected={rejected}, grants={grants}, continuations={continuations}, reservation_workers={owners}");
                    let mut reply = json!({"rejected":rejected,"at":"original-owned-reservation","state":state,
                        "ownedWorkers":approvals.owners(),"reservationWorkers":owners,
                        "continuations":continuations,"stored":approvals.settings()["computerUseEnabled"]});
                    if owner_replacement {
                        reply["runId"] = json!(reservation.publication_run());
                        reply["approvalCalls"] = json!(approvals.started());
                        reply["ownerWitness"] = owner.witness(&f)?;
                    }
                    reply
                }
                "replace-publication-owner" => {
                    if owner_replacement::stage(scenario)
                        != Some(owner_replacement::ReplacementStage::Publication)
                        || approved.is_some()
                        || rejected
                    {
                        return Err("original publication owner replacement unavailable".into());
                    }
                    owner.replace(
                        scenario,
                        &runtime,
                        owner_replacement::OriginalOwner {
                            manager: &manager,
                            fixture: &f,
                            key: key()?,
                        },
                        owner_replacement::ReplacementPoint::Publication(&mut publication),
                        &approvals,
                    )?
                }
                "replace-picker-owner" => {
                    let point = match owner_replacement::stage(scenario) {
                        Some(owner_replacement::ReplacementStage::Choose) => {
                            owner_replacement::ReplacementPoint::Choose(
                                picker.as_deref().ok_or("original picker missing")?,
                            )
                        }
                        Some(owner_replacement::ReplacementStage::Reservation) => {
                            owner_replacement::ReplacementPoint::Reservation(&reservation)
                        }
                        _ => return Err("original picker owner replacement unavailable".into()),
                    };
                    if approved.is_some() || rejected {
                        return Err("original picker owner replacement misplaced".into());
                    }
                    owner.replace(
                        scenario,
                        &runtime,
                        owner_replacement::OriginalOwner {
                            manager: &manager,
                            fixture: &f,
                            key: key()?,
                        },
                        point,
                        &approvals,
                    )?
                }
                "reject-original-picker-owner" => {
                    if approved.is_some() || rejected {
                        return Err("original picker owner rejection misplaced".into());
                    }
                    let candidates: Vec<String> =
                        serde_json::from_value(value["candidates"].clone())
                            .map_err(|_| "invalid private fixture selection")?;
                    let reply = owner.reject_choose(
                        &runtime,
                        owner_replacement::OriginalOwner {
                            manager: &manager,
                            fixture: &f,
                            key: key()?,
                        },
                        (
                            picker.as_deref().ok_or("original picker missing")?,
                            candidates,
                        ),
                        &approvals,
                        scenario,
                    )?;
                    rejected = true;
                    reply
                }
                "finish-publication" => {
                    let (run, outcome) = publication.release_and_join(&runtime)?;
                    runtime.block_on(approvals.join())?;
                    rejected = outcome.as_ref().err().map(String::as_str)
                        == Some(if owner_replacement {
                            "Computer Use request owner retired"
                        } else {
                            "Computer Use approval retired before publication"
                        });
                    let state = if owner_replacement {
                        f.original_host_task_view()?
                    } else {
                        manager.computer_use_task_view("view-app")?
                    };
                    let grants = broker.executable_grant_count("view-app");
                    let continuations = broker.continuation_count("view-app");
                    println!("existing publication authority witness: rejected={rejected}, grants={grants}, continuations={continuations}, owned_workers={}", approvals.owners());
                    let mut reply = json!({"rejected":rejected,"at":"original-publication","runId":run,
                        "state":state,"ownedWorkers":approvals.owners(),"continuations":continuations,
                        "stored":approvals.settings()["computerUseEnabled"]});
                    if owner_replacement {
                        reply["ownerWitness"] = owner.witness(&f)?;
                    }
                    reply
                }
                "delivery" => {
                    if late_actions::is_scenario(scenario) || capture::is_late_scenario(scenario) {
                        late_action.retain_delivery(&f)?;
                    }
                    if recovery::is_scenario(scenario) {
                        recovery.retain_delivery(&f)?;
                    }
                    runtime.block_on(f.deliver_managed_continuation_once())?;
                    json!({"ok":true})
                }
                command if chat_stop::is_command(command) => {
                    if scenario != "active-chat-stop-queued" {
                        return Err("original Chat Stop unavailable in this scenario".into());
                    }
                    chat_stop.command(
                        command,
                        &runtime,
                        chat_stop::Context {
                            manager: &manager,
                            fixture: &mut f,
                            approvals: &approvals,
                            key: key()?,
                            approved: approved
                                .as_ref()
                                .ok_or("original Chat Stop approval missing")?,
                        },
                    )?
                }
                command if recovery::is_command(command) => {
                    if !recovery::is_scenario(scenario) {
                        return Err("original Host recovery unavailable in this scenario".into());
                    }
                    recovery.command(
                        command,
                        &runtime,
                        recovery::Context {
                            scenario,
                            manager: &manager,
                            fixture: &mut f,
                            approvals: &approvals,
                            approved: approved.as_ref().ok_or("original approval missing")?,
                            key: key()?,
                        },
                    )?
                }
                command
                    if late_actions::is_command(command)
                        || capture::control_command(command).is_some() =>
                {
                    if !(late_actions::is_scenario(scenario) || capture::is_late_scenario(scenario))
                    {
                        return Err("late action scenario unavailable".into());
                    }
                    late_action.command(
                        capture::control_command(command).unwrap_or(command),
                        &runtime,
                        recovery::Context {
                            scenario,
                            manager: &manager,
                            fixture: &mut f,
                            approvals: &approvals,
                            approved: approved
                                .as_ref()
                                .ok_or("original action approval missing")?,
                            key: key()?,
                        },
                    )?
                }
                "pending-lease-witness" => {
                    if !preapproval_lease
                        || picker.is_none()
                        || approved.is_some()
                        || rejected
                        || repair_started
                    {
                        return Err("pending lease witness unavailable".into());
                    }
                    key()?;
                    // No task-view, candidate read, expire, renewal or registry sweep.
                    let mut witness = json!({});
                    preapproval_lease::attach_lease(&mut witness, &broker);
                    witness
                }
                "lease-witness" => {
                    if !(scenario == "active-lease-expiry" || browser_lifecycle)
                        || approved.is_none()
                    {
                        return Err("original lease witness unavailable".into());
                    }
                    // Does NOT expire the connection. Only the original HTTP
                    // maintenance consumer can change this stored-state witness.
                    json!({"stored":broker.tabs().has_stored_pairing_connection_for_test(),
                        "leaseMs":grok_computer_use_core::pairing::CONNECTION_LEASE.as_millis()})
                }
                "browser-owner-state" => {
                    if !browser_lifecycle || approved.is_none() {
                        return Err("original browser owner witness unavailable".into());
                    }
                    json!({"activeProcesses":peer.active_processes()?,"members":peer.process_members()?})
                }
                "repair-challenge" => {
                    let original_pending = preapproval_replacement
                        && request_key.is_some()
                        && picker.is_some()
                        && approved.is_none()
                        && !rejected;
                    if !(active_lifecycle && approved.is_some() || original_pending)
                        || repair_started
                    {
                        return Err("original fixture cannot start this repair".into());
                    }
                    // Same Broker operations as Host pairing commands; only
                    // the Host gesture is synthetic, not the MV3/HTTP handshake.
                    let challenge = broker.begin_pairing()?;
                    broker.confirm_pairing(&challenge.nonce)?;
                    repair_started = true;
                    json!({"endpoint":f.native_mcp_binding()["endpoint"],
                        "verificationCode":challenge.verification_code})
                }
                "original-owner-authority" => {
                    if !owner_replacement || !rejected || approved.is_some() {
                        return Err("original rejected owner authority witness unavailable".into());
                    }
                    // Fixed original transport, even after the App slot has changed.
                    json!({"state":f.original_host_task_view()?,
                        "grants":broker.executable_grant_count("view-app"),
                        "continuations":broker.continuation_count("view-app"),
                        "ownedWorkers":approvals.owners(),
                        "reservationWorkers":crate::computer_use::task_picker::reservation_owners_for_test(),
                        "stored":approvals.settings()["computerUseEnabled"],
                        "ownerWitness":owner.witness(&f)?})
                }
                "capture-preview" if scenario == "active-capture-success" => {
                    capture::preview(&runtime, &manager)?
                }
                "lifecycle-authority" => readback::authority(
                    &runtime,
                    &mut f,
                    &manager,
                    &broker,
                    &approvals,
                    active_lifecycle && approved.is_some()
                        || preapproval_replacement && repair_started,
                    capture::is_scenario(scenario),
                )?,
                "stopped" | "preparation-after-stop" | "shared-count" => {
                    readback::command(&broker, &value)?
                }
                _ => return Err("unknown native fixture command".into()),
            };
            peer.reply(&reply)?;
        }
    })();
    if let Err(error) = &result {
        // Preserve the original control result BEFORE teardown errors can
        // dominate the final assertion. No arbitrary error payload is logged.
        let control_timeout = error == "native fixture control did not settle";
        let node_reported = error.starts_with("native new TaskGrant failed at ");
        let family = if error == "computer use is disabled" {
            "feature-disabled"
        } else {
            "other"
        };
        println!("native joint original control witness: control_timeout={control_timeout}, node_reported={node_reported}, family={family}, elapsed_ms={}", original_started.elapsed().as_millis());
        println!(
            "native joint original peer witness: {}",
            peer.control_witness()
        );
    }
    // Retain failure profiles; cleanup never drops the original child/Job ownership.
    let discovery_join = discovery.finish(&runtime, &broker);
    let reservation_join = reservation.finish(&runtime, &broker);
    let publication_join = publication.finish(&runtime, &broker, &approvals);
    recovery.fence_pending(&broker);
    late_action.fence_pending(&broker);
    let native_join = peer.finish(result.is_ok());
    let late_action_join = late_action.finish(&runtime);
    let recovery_join = recovery.finish(&runtime, &broker);
    let approval_join = runtime.block_on(approvals.join());
    let _ = stop.send(());
    runtime.block_on(watcher).unwrap();
    drop(manager);
    runtime.block_on(recovery.finish_owners());
    runtime.block_on(owner.finish());
    runtime.block_on(f.finish());
    approvals.reset_feature(&broker);
    if result.is_ok()
        && native_join.is_ok()
        && approval_join.is_ok()
        && publication_join.is_ok()
        && reservation_join.is_ok()
        && discovery_join.is_ok()
        && recovery_join.is_ok()
        && late_action_join.is_ok()
    {
        remove_owned_root(&root, &temporary);
    }
    assert!(
        discovery_join.is_ok(),
        "original discovery owner failed; preserve {root:?}: {discovery_join:?}"
    );
    assert!(
        reservation_join.is_ok(),
        "original reservation owner failed; preserve {root:?}: {reservation_join:?}"
    );
    assert!(
        publication_join.is_ok(),
        "original publication owner failed; preserve {root:?}: {publication_join:?}"
    );
    assert!(
        recovery_join.is_ok(),
        "original recovery owner failed; preserve {root:?}: {recovery_join:?}"
    );
    assert!(
        late_action_join.is_ok(),
        "original action Pause owner failed; preserve {root:?}"
    );
    assert!(
        native_join.is_ok(),
        "native owners failed; preserve {root:?}: {native_join:?}"
    );
    assert!(
        approval_join.is_ok(),
        "original approval owner failed; preserve {root:?}: {approval_join:?}"
    );
    assert!(
        result.is_ok(),
        "native new TaskGrant joint failed; preserve {root:?}: {result:?}"
    );
    assert!(!root.exists());
    evidence::report_success(scenario, checks.len(), dormant_bootstrap);
}
