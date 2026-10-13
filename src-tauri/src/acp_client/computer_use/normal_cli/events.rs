//! Actual CLI reverse requests, with a strict synthetic-only ordinary policy.
use super::*;

#[derive(Default)]
pub(super) struct Counts {
    pub request: u32,
    pub observe_allowed: u32,
    pub observe_rejected: u32,
    pub ordinary: u32,
    pub nonfixture_cancelled: u32,
}

fn fixed_fixture(raw: &Value) -> bool {
    raw.pointer("/toolCall/_meta/x.ai~1tool/name")
        .and_then(Value::as_str)
        == Some("use_tool")
        && raw
            .pointer("/toolCall/rawInput/variant")
            .and_then(Value::as_str)
            == Some("UseTool")
        && raw
            .pointer("/toolCall/rawInput/tool_name")
            .and_then(Value::as_str)
            == Some("consent-fixture__consent_fixed_data")
        && raw
            .pointer("/toolCall/rawInput/tool_input")
            .is_some_and(|args| {
                args.as_object().is_some_and(|object| object.len() == 1)
                    && matches!(args.get("sequence").and_then(Value::as_u64), Some(1 | 2))
            })
}

pub(super) async fn pump(
    client: Arc<AcpClient>,
    mut events: mpsc::UnboundedReceiver<(Option<String>, AcpEvent)>,
    agent: String,
    counts: Arc<ParkingMutex<Counts>>,
) -> Result<(), String> {
    while let Some((owner, event)) = events.recv().await {
        match event {
            AcpEvent::PermissionRequest {
                rpc_id,
                options,
                raw,
                ..
            } => {
                // Actual permission events are untagged at the outer channel.
                // Like the production handler, require the raw request owner;
                // an optional outer tag must agree and never substitutes for it.
                let routed = raw.get("sessionId").and_then(Value::as_str) == Some(&agent)
                    && owner.as_deref().is_none_or(|owner| owner == agent)
                    && client.prompt_rpc_in_flight(&agent);
                let input = raw.pointer("/toolCall/rawInput");
                let safe_id = |value: Option<&Value>| {
                    value
                        .and_then(Value::as_str)
                        .filter(|id| {
                            id.len() <= 100
                                && id.bytes().all(|byte| {
                                    byte.is_ascii_alphanumeric()
                                        || matches!(byte, b'_' | b'-' | b':')
                                })
                        })
                        .map(str::to_owned)
                };
                note(
                    "permission-shape",
                    json!({
                        "exactOwner":routed,
                        "meta":safe_id(raw.pointer("/toolCall/_meta/x.ai~1tool/name")),
                        "variant":safe_id(input.and_then(|value| value.get("variant"))),
                        "tool":safe_id(input.and_then(|value| value.get("tool_name"))),
                        "inputObject":input.and_then(|value| value.get("tool_input")).is_some_and(Value::is_object),
                        "inputKeys":input.and_then(Value::as_object).map(|object| object.keys().cloned().collect::<Vec<_>>()),
                        "argumentKeys":input.and_then(|value| value.get("tool_input")).and_then(Value::as_object).map(|object| object.keys().cloned().collect::<Vec<_>>()),
                    }),
                );
                let cu =
                    client.computer_use_permission(APP_SESSION, &agent, &raw, &options, routed);
                let outcome = if let Some(outcome) = cu {
                    let tool = raw
                        .pointer("/toolCall/rawInput/tool_name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let allowed = matches!(&outcome, PermissionOutcome::Selected { option_id } if option_id == "allow-once");
                    let mut tally = counts.lock();
                    if tool.ends_with("__computer_request_access") {
                        tally.request += 1;
                    } else if tool.ends_with("__computer_observe") {
                        if allowed {
                            tally.observe_allowed += 1;
                        } else {
                            tally.observe_rejected += 1;
                        }
                    }
                    note(
                        "cu-permission",
                        json!({"tool":tool,"allowed":allowed,"exactOwner":routed}),
                    );
                    outcome
                } else if routed && fixed_fixture(&raw) {
                    counts.lock().ordinary += 1;
                    note(
                        "ordinary-once-confirmation",
                        json!({"sequence":raw.pointer("/toolCall/rawInput/tool_input/sequence")}),
                    );
                    // Synthetic once-only confirmation. Not a human decision.
                    if options.as_array().is_some_and(|rows| {
                        rows.iter().any(|row| {
                            row["optionId"] == "allow-once" && row["kind"] == "allow_once"
                        })
                    }) {
                        PermissionOutcome::Selected {
                            option_id: "allow-once".into(),
                        }
                    } else {
                        PermissionOutcome::Cancelled
                    }
                } else {
                    counts.lock().nonfixture_cancelled += 1;
                    note("nonfixture-cancelled", json!({}));
                    PermissionOutcome::Cancelled
                };
                client
                    .respond_permission(rpc_id, outcome)
                    .await
                    .map_err(|_| "permission reply failed")?;
            }
            AcpEvent::AskUserQuestion { rpc_id, .. } => {
                client
                    .respond_ask_user_question(rpc_id, AskUserOutcome::Cancelled)
                    .await
                    .map_err(|_| "unexpected questionnaire reply failed")?;
            }
            AcpEvent::PromptComplete {
                authoritative,
                host_turn_id,
                ..
            } => {
                note(
                    "prompt-complete",
                    json!({"authoritative":authoritative,"hostTurn":host_turn_id}),
                );
            }
            AcpEvent::Error { .. } => note("cli-error-event", json!({})),
            AcpEvent::RetryState {
                attempt, status, ..
            } => {
                note("cli-retry", json!({"attempt":attempt,"status":status}));
            }
            // Do not log model output, credentials, stderr or arbitrary raw values.
            _ => {}
        }
    }
    Ok(())
}

pub(super) async fn close(client: &AcpClient) -> Result<(), String> {
    client.stopped.store(true, Ordering::SeqCst);
    client.retire_computer_use_transports();
    client.reader_alive.store(false, Ordering::SeqCst);
    client.reader_stop.notify_one();
    client.fail_all_pending("probe ended");
    if let Some(mut stdin) = client.stdin.lock().await.take() {
        let _ = stdin.shutdown().await;
    }
    if let Some(mut child) = client.child.lock().await.take() {
        let result = tokio::time::timeout(Duration::from_secs(8), child.wait()).await;
        let (status, killed) = match result {
            Ok(status) => (status, false),
            Err(_) => {
                #[cfg(windows)]
                if let Some(pid) = child.id() {
                    if !crate::process_util::kill_process_tree_async(pid).await {
                        note("tree-kill-failed", json!({"pid":pid}));
                    }
                }
                let _ = child.kill().await;
                (child.wait().await, true)
            }
        };
        let status = status.map_err(|_| "original CLI wait failed")?;
        note(
            "original-cli-joined",
            json!({"exitCode":status.code(),"treeKill":killed}),
        );
    }
    Ok(())
}
