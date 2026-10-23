//! Plain-text and compact JSON facts for one tool result.
//!
//! Memory only. No file IO, await, timeout, or cancellation.

use gcu_core::batch::BatchReport;
use gcu_protocol::{result::Outcome, ActionResult, AppState, ErrorCode, ToolError};
use serde_json::{json, Value};

pub(crate) fn error(code: ErrorCode) -> ToolError {
    ToolError {
        code,
        message: code.as_str().replace('_', " "),
        hint: "Use fresh state and obtain user authorization when required.".into(),
        retryable: false,
        state: None,
    }
}

pub(super) fn prefix(text: &str, budget: usize) -> &str {
    let mut end = text.len().min(budget);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub(super) fn step_summary(report: &BatchReport) -> String {
    let body = report
        .steps
        .iter()
        .map(|step| format!("{} {} {}", step.i, step.tool, outcome_name(step.outcome)))
        .collect::<Vec<_>>()
        .join("; ");
    format!("Batch: {body}.")
}

fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Done => "done",
        Outcome::Refused => "refused",
        Outcome::Failed => "failed",
        Outcome::Unknown => "unknown",
    }
}

pub(super) fn facts(state: &AppState) -> Value {
    json!({"app_id":state.app.id,"window_id":state.window.id,"snapshot_id":state.snapshot_id,
        "screenshot":state.screenshot,"focused":state.focused,"truncated":state.truncated})
}

pub(super) fn action_facts(action: &ActionResult) -> Value {
    let mut action = action.clone();
    if let Some(gcu_protocol::result::ActionTarget::Element { label, .. }) = &mut action.target {
        *label = prefix(label, 256).into();
    }
    if let Some(gcu_protocol::result::ActionTarget::Coordinates { screenshot_id, .. }) =
        &mut action.target
    {
        *screenshot_id = prefix(screenshot_id, 256).into();
    }
    if let Some(verify) = &mut action.verify {
        verify.expect = prefix(&verify.expect, 256).into();
    }
    action.notes.truncate(4);
    for note in &mut action.notes {
        *note = prefix(note, 256).into();
    }
    json!(action)
}

pub(super) fn append_action(text: &mut String, data: &Value) {
    let Some(action) = data.get("action") else {
        return;
    };
    text.push_str(&format!(
        "\nAction: {} — {} ({}).",
        action["tool"].as_str().unwrap_or(""),
        action["outcome"].as_str().unwrap_or(""),
        action["effect"].as_str().unwrap_or("")
    ));
    if let Some(notes) = action["notes"].as_array() {
        for note in notes {
            if let Some(note) = note.as_str() {
                text.push('\n');
                text.push_str(note);
            }
        }
    }
}
