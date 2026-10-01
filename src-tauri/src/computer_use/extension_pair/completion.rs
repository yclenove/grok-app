//! Private fixture admission, not a model tool or production action delivery channel.
use grok_computer_use_core::{broker::ComputerUseBroker, execution::ActionCancellation};
use serde_json::{json, Value};

const SESSION: &str = "extension-completion-owner";
const RUN: &str = "extension-completion-run";

pub(super) fn control(broker: &ComputerUseBroker, value: &Value) -> Result<Value, String> {
    match value["command"].as_str().unwrap_or_default() {
        "completion-observe" => {
            let tab = broker
                .tabs()
                .grant_picker_tab(
                    SESSION,
                    RUN,
                    value["selector"]
                        .as_str()
                        .ok_or("completion fixture candidate missing")?,
                )
                .map_err(|_| "completion fixture grant unavailable")?;
            let observed = broker
                .tabs()
                .observe_existing_document(SESSION, RUN, &tab.tab_id, false)
                .map_err(|_| "completion fixture observation failed")?;
            Ok(json!({"observation":observed,"tabId":tab.tab_id}))
        }
        "completion-offer" => {
            let proof = broker.tabs().offer_existing_completion(
                SESSION,
                RUN,
                value["tabId"]
                    .as_str()
                    .ok_or("completion fixture tab missing")?,
                value["snapshotId"]
                    .as_str()
                    .ok_or("completion fixture snapshot missing")?,
                ActionCancellation::default(),
            )?;
            // The parent/child private pipe carries the proof; it must never enter evidence output.
            Ok(json!({"proof":proof}))
        }
        "completion-cancel" => {
            broker
                .tabs()
                .cancel_actions(RUN)
                .map_err(|_| "completion fixture cancellation failed")?;
            Ok(json!({"ok":true}))
        }
        "completion-state" => Ok(json!({"idle":broker.tabs().existing_operations_idle(RUN),
            "borrowed":broker.tabs().list_for_run(RUN).len()})),
        _ => Err("unsupported completion fixture command".into()),
    }
}
