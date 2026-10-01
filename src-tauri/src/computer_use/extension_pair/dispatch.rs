//! Private fixture consent/admission. All action packets and receipts use production HTTP.
use std::sync::Arc;
use std::thread::JoinHandle;

use grok_computer_use_core::{
    broker::ComputerUseBroker,
    browser::extension_action::{ActionOutcome, ExistingActionInput, ExtensionActionCommand},
    error::BrokerError,
    execution::ActionCancellation,
};
use serde_json::{json, Value};

const SESSION: &str = "extension-dispatch-owner";
const RUN: &str = "extension-dispatch-run";

pub(super) const CHECKS: &[&str] = &[
    "execution-clock-real-transactions",
    "production-sw-negotiates",
    "queued-click-once",
    "maximum-unicode-packet",
    "lost-claim-zero-effect",
    "lost-settle-cleanup-only",
    "lost-business-result-no-replay",
    "host-cancel-real-wait",
    "unpair-joins-both-scripts",
    "duplicate-poll-retires-without-effect",
    "lost-poll-retires-without-effect",
    "navigation-during-claim-has-zero-effect",
    "cleanup-storage-failure-is-zero-claim",
    "unknown-action-reload-cleans-live-controller",
    "expired-completion-tombstone-cleans-without-replay",
    "worker-restart-recovers-claimed-wait",
    "worker-restart-recovers-before-claim",
    "worker-restart-recovers-held-claim",
    "worker-restart-recovers-before-injection",
    "worker-restart-recovers-applied-click",
    "worker-restart-recovers-reloaded-document",
    "worker-restart-recovers-navigated-document",
    "worker-restart-recovers-closed-tab",
];

pub(super) const RETENTION_CHECKS: &[&str] = &[
    "execution-clock-real-transactions",
    "production-sw-negotiates",
    "cached-original-remains-occupied",
    "restored-document-cleanup-and-new-click",
];

pub(super) struct DispatchFixture {
    broker: Arc<ComputerUseBroker>,
    pending: Option<JoinHandle<Result<ActionOutcome, BrokerError>>>,
}

impl DispatchFixture {
    pub(super) fn new(broker: Arc<ComputerUseBroker>) -> Self {
        Self {
            broker,
            pending: None,
        }
    }

    pub(super) fn control(&mut self, value: &Value) -> Result<Value, String> {
        match value["command"].as_str().unwrap_or_default() {
            "dispatch-observe" => {
                if self.pending.is_some() {
                    return Err("consume the fixture result first".into());
                }
                let selector = value["selector"]
                    .as_str()
                    .ok_or("fixture selector missing")?;
                let tab = self
                    .broker
                    .tabs()
                    .grant_picker_tab(SESSION, RUN, selector)
                    .map_err(|_| "fixture grant unavailable")?;
                let observed = self
                    .broker
                    .tabs()
                    .observe_existing_document(SESSION, RUN, &tab.tab_id, false)
                    .map_err(|_| "fixture observation unavailable")?;
                Ok(json!({"observation":observed,"tabId":tab.tab_id}))
            }
            "dispatch-enqueue" => {
                if self.pending.is_some() {
                    return Err("fixture request already pending".into());
                }
                let tab_id = value["tabId"].as_str().ok_or("fixture tab missing")?;
                let tab = self
                    .broker
                    .tabs()
                    .existing_target_info(tab_id)
                    .ok_or("fixture grant missing")?;
                if tab.session != SESSION || tab.run_id != RUN {
                    return Err("wrong fixture owner".into());
                }
                let command: ExtensionActionCommand =
                    serde_json::from_value(value["action"].clone())
                        .map_err(|_| "invalid fixture action")?;
                let broker = self.broker.clone();
                self.pending = Some(std::thread::spawn(move || {
                    broker.tabs().execute_existing_action(ExistingActionInput {
                        session: SESSION,
                        run: RUN,
                        tab: &tab.tab_id,
                        grant_generation: tab.generation,
                        document_generation: tab.document_generation,
                        command,
                        cancellation: ActionCancellation::default(),
                    })
                }));
                Ok(json!({"queued":true}))
            }
            "dispatch-result" => {
                let pending = self.pending.as_ref().ok_or("fixture result missing")?;
                if !pending.is_finished() {
                    return Ok(json!({"pending":true}));
                }
                match self.pending.take().ok_or("fixture result missing")?.join() {
                    Ok(Ok(outcome)) => Ok(json!({"pending":false,"ok":true,"outcome":outcome})),
                    Ok(Err(_)) => Ok(json!({"pending":false,"ok":false})),
                    Err(_) => Err("fixture worker panicked".into()),
                }
            }
            "dispatch-cancel" => {
                self.broker
                    .tabs()
                    .cancel_actions(RUN)
                    .map_err(|_| "fixture cancellation failed")?;
                Ok(json!({"ok":true}))
            }
            "dispatch-state" => {
                Ok(json!({"idle":self.broker.tabs().existing_operations_idle(RUN)}))
            }
            "dispatch-expire-completion-tombstones" => {
                self.broker.tabs().expire_completion_tombstones_for_test();
                Ok(json!({"ok":true}))
            }
            _ => Err("unsupported fixture dispatch command".into()),
        }
    }
}

impl Drop for DispatchFixture {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.broker.tabs().cancel_existing_operations(RUN);
            let _ = pending.join();
        }
    }
}
