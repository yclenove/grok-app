//! Exact CU wire metadata; never use title/kind fallback or generic allow cache.
use super::*;
#[cfg(test)]
mod granted_upload;
#[cfg(test)]
mod pending_status;

fn tool_input<'a>(raw: &'a Value, server: &str) -> Option<(&'a str, &'a Value)> {
    let call = raw.get("toolCall")?.as_object()?;
    let meta = call.get("_meta")?.get("x.ai/tool")?;
    if meta.get("name")?.as_str()? != "use_tool" {
        return None;
    }
    let input = call.get("rawInput")?.as_object()?;
    if input.get("variant")?.as_str()? != "UseTool" {
        return None;
    }
    let name = input.get("tool_name")?.as_str()?;
    let (claimed_server, tool) = name.split_once("__")?;
    if claimed_server != server
        || claimed_server.is_empty()
        || tool.is_empty()
        || !grok_computer_use_core::tools::MODEL_TOOLS.contains(&tool)
    {
        return None;
    }
    Some((tool, input.get("tool_input").unwrap_or(&Value::Null)))
}

fn once_outcome(allowed: bool, options: &Value) -> PermissionOutcome {
    let (id, kind) = if allowed {
        ("allow-once", "allow_once")
    } else {
        ("reject-once", "reject_once")
    };
    if options.as_array().is_some_and(|options| {
        options.iter().any(|option| {
            option.get("optionId").and_then(Value::as_str) == Some(id)
                && option.get("kind").and_then(Value::as_str) == Some(kind)
        })
    }) {
        PermissionOutcome::Selected {
            option_id: id.into(),
        }
    } else {
        PermissionOutcome::Cancelled
    }
}

impl AcpClient {
    pub(crate) fn is_registered_computer_use_call(&self, app_session: &str, raw: &Value) -> bool {
        let slots = self.computer_use_connections.lock();
        slots
            .get(app_session)
            .and_then(|slot| slot.transport.as_ref())
            .is_some_and(|transport| {
                transport
                    .entry
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|server| tool_input(raw, server).is_some())
            })
    }

    pub(crate) fn computer_use_permission(
        &self,
        app_session: &str,
        agent: &str,
        raw: &Value,
        options: &Value,
        active_turn: bool,
    ) -> Option<PermissionOutcome> {
        let slots = self.computer_use_connections.lock();
        let transport = slots.get(app_session)?.transport.as_ref()?.clone();
        let server = transport.entry.get("name")?.as_str()?;
        let (tool, args) = tool_input(raw, server)?;
        let valid_owner = self.is_alive()
            && active_turn
            && raw.get("sessionId").and_then(Value::as_str) == Some(agent)
            && transport.bound_to(agent);
        drop(slots);
        let allowed = valid_owner
            && if matches!(tool, "computer_request_access" | "computer_complete_task") {
                // These gates cannot grant execution. IPC validates request
                // intent or fences the exact original task for completion.
                args.is_object()
            } else {
                transport
                    .broker
                    .preflight_tool_permission(&transport.token, tool, args)
                    .is_ok()
            };
        Some(once_outcome(allowed, options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tool: &str) -> Value {
        // Shape recorded by normal Grok 1.0.46 U00-R, with fixed test ids.
        json!({"sessionId":"fixture-agent","toolCall":{"title":"ignored","rawInput":{"variant":"UseTool","tool_name":format!("grok-computer-use__{tool}"),"tool_input":{}},"_meta":{"x.ai/tool":{"name":"use_tool"}}}})
    }

    #[test]
    fn only_exact_registered_metadata_matches() {
        let raw = fixture("computer_request_access");
        assert_eq!(
            tool_input(&raw, crate::computer_use::MCP_SERVER_NAME)
                .unwrap()
                .0,
            "computer_request_access"
        );
        for pointer in [
            "/toolCall/_meta/x.ai~1tool/name",
            "/toolCall/rawInput/variant",
            "/toolCall/rawInput/tool_name",
        ] {
            let mut invalid = raw.clone();
            *invalid.pointer_mut(pointer).unwrap() = json!("wrong");
            assert!(tool_input(&invalid, crate::computer_use::MCP_SERVER_NAME).is_none());
        }
        assert!(tool_input(&raw, "user-configured-server").is_none());
        assert!(tool_input(
            &fixture("computer_authorize"),
            crate::computer_use::MCP_SERVER_NAME
        )
        .is_none());
        assert!(tool_input(
            &json!({"toolCall":{"title":"grok-computer-use__computer_request_access"}}),
            crate::computer_use::MCP_SERVER_NAME
        )
        .is_none());
        assert!(tool_input(
            &fixture("computer_request_access__extra"),
            crate::computer_use::MCP_SERVER_NAME
        )
        .is_none());
    }

    #[test]
    fn upload_is_registered_only_with_original_cu_wire_metadata() {
        let raw = fixture("computer_upload");
        assert_eq!(
            tool_input(&raw, crate::computer_use::MCP_SERVER_NAME)
                .unwrap()
                .0,
            "computer_upload"
        );
        for pointer in [
            "/toolCall/_meta/x.ai~1tool/name",
            "/toolCall/rawInput/variant",
            "/toolCall/rawInput/tool_name",
        ] {
            let mut forged = raw.clone();
            *forged.pointer_mut(pointer).unwrap() = json!("computer_upload");
            assert!(tool_input(&forged, crate::computer_use::MCP_SERVER_NAME).is_none());
        }
        for name in [
            "other-server__computer_upload",
            "grok-computer-use__computer_upload__extra",
            "grok-computer-use__computer_authorize",
        ] {
            let mut forged = raw.clone();
            forged["toolCall"]["rawInput"]["tool_name"] = json!(name);
            assert!(tool_input(&forged, crate::computer_use::MCP_SERVER_NAME).is_none());
        }
    }

    #[test]
    fn missing_once_option_cancels_without_always_fallback() {
        let options = json!([{"optionId":"always-allow","kind":"allow_always"},{"optionId":"allow-once","kind":"allow_once"},{"optionId":"reject-once","kind":"reject_once"}]);
        assert!(
            matches!(once_outcome(true, &options), PermissionOutcome::Selected { option_id } if option_id == "allow-once")
        );
        assert!(
            matches!(once_outcome(false, &options), PermissionOutcome::Selected { option_id } if option_id == "reject-once")
        );
        assert!(matches!(
            once_outcome(
                true,
                &json!([{"optionId":"always-allow","kind":"allow_always"}])
            ),
            PermissionOutcome::Cancelled
        ));
        assert!(matches!(
            once_outcome(
                true,
                &json!([{"optionId":"allow-once","kind":"allow_always"}])
            ),
            PermissionOutcome::Cancelled
        ));
        assert!(matches!(
            once_outcome(false, &json!([])),
            PermissionOutcome::Cancelled
        ));
    }

    #[tokio::test]
    async fn synthetic_request_channel_allows_gate_but_rejects_execution_replay_and_old_agent() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, _events) = AcpClient::connect_tcp(
            &listener.local_addr().unwrap().to_string(),
            std::env::temp_dir(),
        )
        .await
        .unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        // Deliberate fixture insertion, bypassing production eligibility. This
        // proves handler routing only; localhost TCP is separately rejected.
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
            crate::computer_use::BrokerOptions::default(),
        ));
        let issued = broker.reserve_transport_connection("fixture-app").unwrap();
        broker
            .bind_transport_agent(issued.token(), "fixture-agent")
            .unwrap();
        let transport = Arc::new(RequestTransport {
            history_path: None,
            picker: ParkingMutex::new(None),
            deliveries: ParkingMutex::new(super::super::delivery::DeliveryLedger::default()),
            broker: broker.clone(),
            token: issued.token().into(),
            entry: json!({"name":crate::computer_use::MCP_SERVER_NAME}),
        });
        client.computer_use_connections.lock().insert(
            "fixture-app".into(),
            RequestConnection {
                transport: Some(transport),
                error: None,
            },
        );
        let options = json!([{"optionId":"allow-once","kind":"allow_once"},{"optionId":"reject-once","kind":"reject_once"}]);
        for tool in ["computer_request_access", "computer_complete_task"] {
            let raw = fixture(tool);
            assert!(
                matches!(client.computer_use_permission("fixture-app","fixture-agent",&raw,&options,true),Some(PermissionOutcome::Selected {option_id}) if option_id=="allow-once")
            );
            for (agent, active) in [("old-agent", true), ("fixture-agent", false)] {
                assert!(
                    matches!(client.computer_use_permission("fixture-app",agent,&raw,&options,active),Some(PermissionOutcome::Selected {option_id}) if option_id=="reject-once")
                );
            }
            let mut invalid = raw.clone();
            invalid["toolCall"]["rawInput"]["tool_input"] = Value::Null;
            assert!(
                matches!(client.computer_use_permission("fixture-app","fixture-agent",&invalid,&options,true),Some(PermissionOutcome::Selected {option_id}) if option_id=="reject-once")
            );
        }
        for tool in ["computer_observe", "computer_upload"] {
            let execution = fixture(tool);
            // Recognition keeps upload out of generic permission/allow caches;
            // a reserved transport alone is not an executable TaskGrant.
            assert!(client.is_registered_computer_use_call("fixture-app", &execution));
            for (agent, active) in [
                ("fixture-agent", true),
                ("old-agent", true),
                ("fixture-agent", false),
            ] {
                assert!(
                    matches!(client.computer_use_permission("fixture-app",agent,&execution,&options,active),Some(PermissionOutcome::Selected {option_id}) if option_id=="reject-once")
                );
            }
            assert!(matches!(
                client.computer_use_permission(
                    "fixture-app",
                    "fixture-agent",
                    &execution,
                    &json!([{"optionId":"always-allow","kind":"allow_always"}]),
                    true
                ),
                Some(PermissionOutcome::Cancelled)
            ));
        }
        broker.retire_transport_connection(issued.token());
        for tool in [
            "computer_request_access",
            "computer_complete_task",
            "computer_upload",
        ] {
            assert!(
                matches!(client.computer_use_permission("fixture-app","fixture-agent",&fixture(tool),&options,true),Some(PermissionOutcome::Selected {option_id}) if option_id=="reject-once")
            );
        }
        assert_eq!(broker.executable_grant_count("fixture-app"), 0);
        client.kill().await;
    }
}
