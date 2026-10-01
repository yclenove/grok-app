//! Authorized fixture loop: shipped choice function, then the Windows broker.
//! The Choice answer names elements from the current observation. Text comes
//! from the caller binding. Stop and a dead target do not reach another window.

use super::{force_foreground, oracle_path, FixtureWindow};
use grok_computer_use_core::adapter::SurfaceKind;
use grok_computer_use_core::choice::{
    accept_step, bind_proposal, decide, AcceptedStep, ActionIdentity, BoundText, ChoiceInput,
    ChoiceStop, ChoiceVerdict, Proposal, TextBinding, DEFAULT_MIN_CONFIDENCE,
};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const SUPPLIED_TEXT: &str = "Ada";

pub fn run_choice_click_text() -> Result<(), String> {
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker};
    use crate::computer_use::protocol::OutcomeKind;
    use crate::computer_use::windows_adapter::WindowsAdapter;

    let disabled =
        ComputerUseBroker::new(Arc::new(WindowsAdapter::new()), BrokerOptions::default());
    match disabled.open_run("off", "off-run") {
        Err(crate::computer_use::error::BrokerError::FeatureDisabled) => {
            println!("gate: feature_default_off");
        }
        other => {
            return Err(format!(
                "disabled computer use must not open a run: {other:?}"
            ))
        }
    }

    let title = format!("GrokCuFixture-choice-{}", std::process::id());
    let clicks_path = oracle_path("grok-cu-fixture-clicks.txt");
    let edit_path = oracle_path("grok-cu-fixture-edit.txt");
    let _ = std::fs::write(&clicks_path, "0");
    let _ = std::fs::write(&edit_path, "");
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));
    force_foreground(fx.hwnd());

    let broker = ComputerUseBroker::new(
        Arc::new(WindowsAdapter::new()),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "grok-cu-choice-{}-{}.lease",
                std::process::id(),
                Uuid::new_v4()
            )),
            ..BrokerOptions::default()
        },
    );
    let result = click_and_type(&broker, &fx, &title, &clicks_path, &edit_path, None);
    fx.close();
    result
}

pub fn run_choice_captured_set_value() -> Result<(), String> {
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker};
    use crate::computer_use::windows_adapter::WindowsAdapter;

    let captured: serde_json::Value = serde_json::from_str(include_str!(
        "../../../computer-use-core/src/choice_live_response.json"
    ))
    .map_err(|e| e.to_string())?;
    let operation = captured["answers"]["operation"]["choice"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if operation != "set_value" {
        return Err(format!("captured body is not set_value: {operation}"));
    }
    println!("choice captured-operation={operation}");
    let title = format!("GrokCuFixture-choice-captured-{}", std::process::id());
    let clicks_path = oracle_path("grok-cu-fixture-clicks.txt");
    let edit_path = oracle_path("grok-cu-fixture-edit.txt");
    let _ = std::fs::write(&clicks_path, "0");
    let _ = std::fs::write(&edit_path, "");
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));
    force_foreground(fx.hwnd());
    let broker = ComputerUseBroker::new(
        Arc::new(WindowsAdapter::new()),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "grok-cu-choice-captured-{}-{}.lease",
                std::process::id(),
                Uuid::new_v4()
            )),
            ..BrokerOptions::default()
        },
    );
    let result = click_and_type(
        &broker,
        &fx,
        &title,
        &clicks_path,
        &edit_path,
        Some(captured),
    );
    fx.close();
    result
}

fn click_and_type(
    broker: &crate::computer_use::broker::ComputerUseBroker,
    fx: &FixtureWindow,
    title: &str,
    clicks_path: &std::path::Path,
    edit_path: &std::path::Path,
    mut injected: Option<serde_json::Value>,
) -> Result<(), String> {
    use crate::computer_use::protocol::OutcomeKind;

    broker
        .open_run("choice-sess", "choice-run")
        .map_err(|e| e.to_string())?;
    let listed = broker
        .list_targets("choice-run")
        .map_err(|e| e.to_string())?;
    let target = listed
        .iter()
        .find(|item| item.title.contains(title))
        .ok_or_else(|| format!("fixture not listed: {title}"))?
        .clone();
    if listed
        .iter()
        .filter(|item| item.title.contains(title))
        .count()
        != 1
    {
        return Err("fixture title matched more than one target".into());
    }
    let generation = broker
        .authorize_target("choice-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    force_foreground(fx.hwnd());
    let obs = broker.observe("choice-run").map_err(|e| e.to_string())?;
    let count_ref = obs
        .nodes
        .iter()
        .find(|node| node.name.eq_ignore_ascii_case("count"))
        .map(|node| node.node_ref.clone())
        .ok_or("observation has no Count control")?;
    let edit_ref = obs
        .nodes
        .iter()
        .find(|node| node.role == "edit")
        .map(|node| node.node_ref.clone())
        .ok_or("observation has no edit control")?;
    let bindings = [TextBinding {
        element_ref: edit_ref.clone(),
        text: SUPPLIED_TEXT.into(),
        mode: BoundText::Replace,
    }];
    let mut current = obs;
    let mut need_click = true;
    let mut need_text = true;
    for step in 0..4 {
        if !need_click && !need_text {
            break;
        }
        let verdict = if let Some(captured) = injected.take() {
            let rebound =
                grok_computer_use_core::choice::rebind_choice_target(captured, "e-name", &edit_ref);
            let input = choice_input(&current, &bindings);
            grok_computer_use_core::choice::choose_with(&input, |_| Ok(rebound))
        } else if jev_configured() {
            let input = choice_input(&current, &bindings);
            grok_computer_use_core::choice::choose(&input)
        } else if need_click {
            scripted_verdict(&current, &bindings, "click", "click_target", &count_ref)?
        } else {
            scripted_verdict(
                &current,
                &bindings,
                "set_value",
                "set_value_target",
                &edit_ref,
            )?
        };
        match accept_step(&verdict, SUPPLIED_TEXT) {
            AcceptedStep::Click { element_ref } if need_click && element_ref == count_ref => {
                let proposal = proposal_of(&verdict)?;
                let request = bind_proposal(
                    &proposal,
                    identity(
                        &format!("choice-click-{step}"),
                        &target.target_id,
                        generation,
                        &current,
                    ),
                )?;
                force_foreground(fx.hwnd());
                let (_, outcome, after) = broker
                    .observe_act_verify(request)
                    .map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(200));
                let clicks = fx.clicks();
                let click_file = std::fs::read_to_string(clicks_path).unwrap_or_default();
                println!(
                    "choice click: outcome={:?} executed={} clicks={clicks} file={}",
                    outcome.kind,
                    outcome.executed,
                    click_file.trim()
                );
                if outcome.kind == OutcomeKind::Rejected || !outcome.executed || clicks != 1 {
                    return Err(format!(
                        "click postcondition clicks={clicks} file={} outcome={outcome:?}",
                        click_file.trim()
                    ));
                }
                if click_file.trim() != "1" {
                    return Err(format!("click file postcondition {}", click_file.trim()));
                }
                current = after;
                need_click = false;
            }
            AcceptedStep::ReplaceText { element_ref, text }
                if need_text && element_ref == edit_ref =>
            {
                let proposal = proposal_of(&verdict)?;
                if text != SUPPLIED_TEXT {
                    return Err("proposal text was not the caller binding".into());
                }
                let request = bind_proposal(
                    &proposal,
                    identity(
                        &format!("choice-text-{step}"),
                        &target.target_id,
                        generation,
                        &current,
                    ),
                )?;
                force_foreground(fx.hwnd());
                let (_, outcome, after) = broker
                    .observe_act_verify(request)
                    .map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(200));
                let edit = fx.edit_text();
                let edit_file = std::fs::read_to_string(edit_path).unwrap_or_default();
                let clicks_during_text = fx.clicks();
                println!(
                    "choice set_value: outcome={:?} executed={} clicks={clicks_during_text} edit={edit:?} file={:?}",
                    outcome.kind,
                    outcome.executed,
                    edit_file.trim()
                );
                if need_click && clicks_during_text != 0 {
                    return Err(format!(
                        "set_value was treated as the Count click: clicks={clicks_during_text}"
                    ));
                }
                if outcome.kind == OutcomeKind::Rejected || !outcome.executed {
                    return Err(format!("set_value did not execute: {outcome:?}"));
                }
                if edit != SUPPLIED_TEXT || edit_file.trim() != SUPPLIED_TEXT {
                    return Err(format!(
                        "text postcondition edit={edit:?} file={:?}",
                        edit_file.trim()
                    ));
                }
                current = after;
                need_text = false;
            }
            other => {
                return Err(format!(
                    "choice step {step} was not the remaining action: {other:?}"
                ));
            }
        }
    }
    if need_click || need_text {
        return Err(format!(
            "choice loop unfinished click={need_click} text={need_text}"
        ));
    }
    println!("gate: windows_choice_click_text");
    Ok(())
}

pub fn run_choice_stop_and_dead() -> Result<(), String> {
    use crate::computer_use::broker::{BrokerOptions, ComputerUseBroker};
    use crate::computer_use::protocol::OutcomeKind;
    use crate::computer_use::windows_adapter::WindowsAdapter;

    let title = format!("GrokCuFixture-choice-stop-{}", std::process::id());
    let clicks_path = oracle_path("grok-cu-fixture-clicks.txt");
    let _ = std::fs::write(&clicks_path, "0");
    let fx = FixtureWindow::spawn(&title)?;
    std::thread::sleep(Duration::from_millis(250));
    force_foreground(fx.hwnd());
    let broker = ComputerUseBroker::new(
        Arc::new(WindowsAdapter::new()),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "grok-cu-choice-stop-{}-{}.lease",
                std::process::id(),
                Uuid::new_v4()
            )),
            ..BrokerOptions::default()
        },
    );
    broker
        .open_run("stop-sess", "stop-run")
        .map_err(|e| e.to_string())?;
    let listed = broker.list_targets("stop-run").map_err(|e| e.to_string())?;
    let target = listed
        .iter()
        .find(|item| item.title.contains(&title))
        .cloned()
        .ok_or("stop fixture not listed")?;
    let generation = broker
        .authorize_target("stop-run", &target.target_id)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("stop-run").map_err(|e| e.to_string())?;
    let count_ref = obs
        .nodes
        .iter()
        .find(|node| node.name.eq_ignore_ascii_case("count"))
        .map(|node| node.node_ref.clone())
        .ok_or("stop observation has no Count")?;
    let click = proposal_for(&obs, &[], "click", "click_target", &count_ref)?;
    let first = bind_proposal(
        &click,
        identity("stop-click", &target.target_id, generation, &obs),
    )?;
    force_foreground(fx.hwnd());
    let (_, first_out, _) = broker
        .observe_act_verify(first)
        .map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(150));
    let setup_clicks = fx.clicks();
    if !first_out.executed || setup_clicks != 1 {
        fx.close();
        return Err(format!(
            "stop setup click failed clicks={setup_clicks} outcome={first_out:?}"
        ));
    }
    broker.request_stop("stop-run").map_err(|e| e.to_string())?;
    let replay = proposal_for(&obs, &[], "click", "click_target", &count_ref)?;
    let second = bind_proposal(
        &replay,
        identity("stop-replay", &target.target_id, generation, &obs),
    )?;
    let replay_out = broker.act(second);
    std::thread::sleep(Duration::from_millis(150));
    let clicks_after_stop = fx.clicks();
    println!(
        "choice stop: outcome={:?} executed={} clicks={clicks_after_stop}",
        replay_out.kind, replay_out.executed
    );
    if replay_out.executed || replay_out.kind != OutcomeKind::Rejected || clicks_after_stop != 1 {
        fx.close();
        return Err(format!(
            "stop replayed input clicks={clicks_after_stop} outcome={replay_out:?}"
        ));
    }

    let other_title = format!("GrokCuFixture-choice-other-{}", std::process::id());
    let other = FixtureWindow::spawn(&other_title)?;
    std::thread::sleep(Duration::from_millis(200));
    let dead_broker = ComputerUseBroker::new(
        Arc::new(WindowsAdapter::new()),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "grok-cu-choice-dead-{}-{}.lease",
                std::process::id(),
                Uuid::new_v4()
            )),
            ..BrokerOptions::default()
        },
    );
    dead_broker
        .open_run("dead-sess", "dead-run")
        .map_err(|e| e.to_string())?;
    let dead_listed = dead_broker
        .list_targets("dead-run")
        .map_err(|e| e.to_string())?;
    let dead_target = dead_listed
        .iter()
        .find(|item| item.title.contains(&title))
        .cloned()
        .ok_or("original fixture missing before close")?;
    let dead_gen = dead_broker
        .authorize_target("dead-run", &dead_target.target_id)
        .map_err(|e| e.to_string())?;
    let dead_obs = dead_broker.observe("dead-run").map_err(|e| e.to_string())?;
    fx.close();
    std::thread::sleep(Duration::from_millis(200));
    let stray = proposal_for(&dead_obs, &[], "click", "click_target", &count_ref)?;
    let stray_request = bind_proposal(
        &stray,
        identity("dead-click", &dead_target.target_id, dead_gen, &dead_obs),
    )?;
    let dead_out = dead_broker.act(stray_request);
    std::thread::sleep(Duration::from_millis(150));
    let other_clicks = other.clicks();
    println!(
        "choice dead: outcome={:?} executed={} distractor={other_clicks}",
        dead_out.kind, dead_out.executed
    );
    other.close();
    if dead_out.executed || dead_out.kind != OutcomeKind::Rejected || other_clicks != 0 {
        return Err(format!(
            "dead target reached another window outcome={dead_out:?} distractor={other_clicks}"
        ));
    }
    println!("gate: windows_choice_stop_and_dead");
    super::run_focus_drift_pauses()
}

fn jev_configured() -> bool {
    std::env::var("TYPESAFE_API_KEY")
        .ok()
        .is_some_and(|key| !key.trim().is_empty())
}

fn choice_input<'a>(
    observation: &'a crate::computer_use::protocol::Observation,
    bindings: &'a [TextBinding],
) -> ChoiceInput<'a> {
    ChoiceInput {
        goal: "Click Count, then replace the edit with the supplied text",
        surface: SurfaceKind::Desktop,
        observation,
        bindings,
        min_confidence: DEFAULT_MIN_CONFIDENCE,
    }
}

fn scripted_verdict(
    observation: &crate::computer_use::protocol::Observation,
    bindings: &[TextBinding],
    operation: &str,
    target_key: &str,
    element_ref: &str,
) -> Result<ChoiceVerdict, String> {
    let input = choice_input(observation, bindings);
    let response = serde_json::json!({
        "answers": {
            "operation": choice_answer(operation),
            target_key: choice_answer(element_ref)
        }
    });
    let verdict = decide(&input, &response);
    match &verdict {
        ChoiceVerdict::Propose(proposal) if proposal.element_ref == element_ref => Ok(verdict),
        ChoiceVerdict::Stop(ChoiceStop::Unoffered) => Err(format!(
            "observed element {element_ref} was not offered for {operation}"
        )),
        ChoiceVerdict::Stop(stop) => Err(format!("decision stopped: {stop:?}")),
        ChoiceVerdict::Propose(proposal) => Err(format!(
            "decision changed target {element_ref} -> {}",
            proposal.element_ref
        )),
    }
}

fn proposal_of(verdict: &ChoiceVerdict) -> Result<Proposal, String> {
    match verdict {
        ChoiceVerdict::Propose(proposal) => Ok(proposal.clone()),
        ChoiceVerdict::Stop(stop) => Err(format!("decision stopped: {stop:?}")),
    }
}

fn proposal_for(
    observation: &crate::computer_use::protocol::Observation,
    bindings: &[TextBinding],
    operation: &str,
    target_key: &str,
    element_ref: &str,
) -> Result<Proposal, String> {
    proposal_of(&scripted_verdict(
        observation,
        bindings,
        operation,
        target_key,
        element_ref,
    )?)
}

fn choice_answer(choice: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "choice",
        "choice": choice,
        "confidence": 0.95,
        "probabilities": { choice: 0.95 }
    })
}

fn identity(
    action_id: &str,
    target_id: &str,
    target_generation: u64,
    observation: &crate::computer_use::protocol::Observation,
) -> ActionIdentity {
    ActionIdentity {
        action_id: action_id.into(),
        run_id: observation.run_id.clone(),
        target_id: target_id.into(),
        target_generation,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
    }
}
