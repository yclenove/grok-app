//! Full-history tool capability probe.
//!
//! Our own MCP `tools/list` is not proof the CLI exposed the tools to the
//! Agent. Full mode may send only after Agent-advertised or live-call evidence.

use super::mcp_stdio::SessionMcpPolicy;

pub const IMPORT_HISTORY_SEARCH: &str = "import_history_search";
pub const IMPORT_HISTORY_READ: &str = "import_history_read";

pub const REQUIRED_IMPORT_TOOLS: [&str; 2] = [IMPORT_HISTORY_SEARCH, IMPORT_HISTORY_READ];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteKind {
    Official,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HomeMode {
    Shared,
    Independent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeEvidence {
    /// Host MCP answered `tools/list`. Insufficient for full-mode send.
    SelfToolsList,
    /// Names the CLI/Agent advertised for this session.
    AgentAdvertised,
    /// A live tool call observed on the Agent session.
    LiveAgentCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContinuationMode {
    FullHistoryTools,
    FragmentOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeBlockReason {
    SelfListIsNotCliProof,
    ToolsNotExposed,
    RemoteOrPolicyDisabled,
    ProbeFailed,
}

impl ProbeBlockReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SelfListIsNotCliProof => "HISTORY_TOOLS_UNVERIFIED",
            Self::ToolsNotExposed => "HISTORY_TOOLS_NOT_EXPOSED",
            Self::RemoteOrPolicyDisabled => "HISTORY_TOOLS_REMOTE_DISABLED",
            Self::ProbeFailed => "HISTORY_TOOLS_PROBE_FAILED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeDecision {
    ReadyFull,
    Blocked { reason: ProbeBlockReason },
    FragmentAllowed,
}

impl ProbeDecision {
    #[cfg(test)]
    pub fn allows_full_send(&self) -> bool {
        matches!(self, Self::ReadyFull)
    }

    #[cfg(test)]
    pub fn allows_any_send(&self) -> bool {
        matches!(self, Self::ReadyFull | Self::FragmentAllowed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityProbe {
    pub cli_version: Option<String>,
    pub model_id: Option<String>,
    pub route: RouteKind,
    pub home_mode: HomeMode,
    pub policy: SessionMcpPolicy,
    pub evidence: ProbeEvidence,
    pub advertised_tool_names: Vec<String>,
    pub probe_error: Option<String>,
}

pub fn advertised_exposes(names: &[String], canonical: &str) -> bool {
    names.iter().any(|n| {
        let n = n.trim();
        n == canonical || n.ends_with(&format!("__{canonical}"))
    })
}

pub fn both_import_tools_exposed(names: &[String]) -> bool {
    REQUIRED_IMPORT_TOOLS
        .iter()
        .all(|tool| advertised_exposes(names, tool))
}

/// Names the CLI advertised on `session/update`.
/// Slash `availableCommands` never satisfy the import-history tool gate.
/// Agent-advertised MCP tools come from `_meta.tools` (camelCase or snake_case).
#[cfg(test)]
pub fn extract_advertised_import_tools(params: &serde_json::Value) -> Vec<String> {
    advertised_import_tools_snapshot(params).unwrap_or_default()
}

/// None means this update has no tool snapshot. Some(empty) is an explicit
/// snapshot without our tools and must revoke earlier exposure evidence.
pub fn advertised_import_tools_snapshot(params: &serde_json::Value) -> Option<Vec<String>> {
    let update = params.get("update").unwrap_or(params);
    let metadata = update
        .pointer("/_meta/tools")
        .or_else(|| update.pointer("/_meta/Tools"))
        .or_else(|| params.pointer("/_meta/tools"));
    let mcp = update.get("mcpTools").or_else(|| update.get("mcp_tools"));
    if metadata.is_none() && mcp.is_none() {
        return None;
    }
    let mut names = Vec::new();
    collect_tool_names(metadata, &mut names);
    collect_tool_names(mcp, &mut names);
    names.sort();
    names.dedup();
    Some(names)
}

fn collect_tool_names(value: Option<&serde_json::Value>, names: &mut Vec<String>) {
    let Some(list) = value.and_then(|v| v.as_array()) else {
        return;
    };
    for item in list {
        let name = item
            .get("name")
            .or_else(|| item.get("toolName"))
            .or_else(|| item.get("tool_name"))
            .and_then(|n| n.as_str())
            .or_else(|| item.as_str());
        let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
            continue;
        };
        if REQUIRED_IMPORT_TOOLS
            .iter()
            .any(|tool| advertised_exposes(&[name.to_string()], tool))
        {
            names.push(name.to_string());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg(test)]
pub enum CapabilityState {
    NotRequested,
    Starting,
    McpReady,
    AgentAdvertised,
    LiveVerified,
    Unavailable,
    PolicyBlocked,
}

#[cfg(test)]
pub fn capability_state(
    probe: &CapabilityProbe,
    user_mode: Option<ContinuationMode>,
) -> CapabilityState {
    if !probe.policy.allows_local_import_mcp() {
        return CapabilityState::PolicyBlocked;
    }
    match evaluate_probe(probe, user_mode) {
        ProbeDecision::ReadyFull => match probe.evidence {
            ProbeEvidence::LiveAgentCall => CapabilityState::LiveVerified,
            ProbeEvidence::AgentAdvertised => CapabilityState::AgentAdvertised,
            ProbeEvidence::SelfToolsList => CapabilityState::McpReady,
        },
        ProbeDecision::FragmentAllowed => CapabilityState::McpReady,
        ProbeDecision::Blocked {
            reason: ProbeBlockReason::RemoteOrPolicyDisabled,
        } => CapabilityState::PolicyBlocked,
        ProbeDecision::Blocked { .. } => match probe.evidence {
            ProbeEvidence::SelfToolsList => CapabilityState::McpReady,
            _ => CapabilityState::Unavailable,
        },
    }
}

/// Decide whether the continuation may send in full-history mode.
///
/// Probe failure never silent-downgrades to fragment. Fragment send requires
/// an explicit `ContinuationMode::FragmentOnly`.
pub fn evaluate_probe(
    probe: &CapabilityProbe,
    user_mode: Option<ContinuationMode>,
) -> ProbeDecision {
    if user_mode == Some(ContinuationMode::FragmentOnly) {
        return ProbeDecision::FragmentAllowed;
    }
    if !probe.policy.allows_local_import_mcp() {
        return ProbeDecision::Blocked {
            reason: ProbeBlockReason::RemoteOrPolicyDisabled,
        };
    }
    if let Some(err) = probe.probe_error.as_deref() {
        if !err.trim().is_empty() {
            return ProbeDecision::Blocked {
                reason: ProbeBlockReason::ProbeFailed,
            };
        }
    }
    match probe.evidence {
        ProbeEvidence::SelfToolsList => ProbeDecision::Blocked {
            reason: ProbeBlockReason::SelfListIsNotCliProof,
        },
        ProbeEvidence::AgentAdvertised | ProbeEvidence::LiveAgentCall => {
            if both_import_tools_exposed(&probe.advertised_tool_names) {
                ProbeDecision::ReadyFull
            } else {
                ProbeDecision::Blocked {
                    reason: ProbeBlockReason::ToolsNotExposed,
                }
            }
        }
    }
}

#[cfg(test)]
pub fn redacted_probe_trace(
    probe: &CapabilityProbe,
    decision: &ProbeDecision,
) -> serde_json::Value {
    serde_json::json!({
        "cliVersion": probe.cli_version,
        "modelId": probe.model_id,
        "route": probe.route,
        "homeMode": probe.home_mode,
        "evidence": probe.evidence,
        "advertisedToolNames": probe.advertised_tool_names,
        "decision": decision,
        "ssh": probe.policy.ssh,
        "wsl": probe.policy.wsl,
        "officialAux": probe.policy.official_aux,
        "tcp": probe.policy.tcp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_migration::continuation::AllowedSelection;
    use crate::session_migration::mcp_stdio::{
        compose_session_mcp_servers, handle_method, ImportHistoryGrant, SessionMcpPolicy,
    };
    use serde_json::json;

    fn grant() -> ImportHistoryGrant {
        ImportHistoryGrant {
            token: "tok".into(),
            app_session_id: "session-a".into(),
            import_id: "imp:x".into(),
            revision_id: "rev".into(),
            branch_leaf: "e1".into(),
            allowed_event_ids: vec!["e1".into()],
            allowed_selection_hash: AllowedSelection::for_events(vec!["e1".into()]).hash(),
            selection: Some(AllowedSelection::for_events(vec!["e1".into()])),
        }
    }

    fn local_probe(evidence: ProbeEvidence, names: Vec<String>) -> CapabilityProbe {
        CapabilityProbe {
            cli_version: Some("0.2.33".into()),
            model_id: Some("grok-4".into()),
            route: RouteKind::Official,
            home_mode: HomeMode::Independent,
            policy: SessionMcpPolicy::default(),
            evidence,
            advertised_tool_names: names,
            probe_error: None,
        }
    }

    fn self_list_names() -> Vec<String> {
        let listed = handle_method(&grant(), "tools/list", &json!({})).unwrap();
        listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn self_tools_list_is_not_cli_proof_even_when_names_match() {
        let names = self_list_names();
        assert!(both_import_tools_exposed(&names));
        let probe = local_probe(ProbeEvidence::SelfToolsList, names);
        let decision = evaluate_probe(&probe, Some(ContinuationMode::FullHistoryTools));
        assert_eq!(
            decision,
            ProbeDecision::Blocked {
                reason: ProbeBlockReason::SelfListIsNotCliProof
            }
        );
        assert!(!decision.allows_full_send());
        assert!(!decision.allows_any_send());
    }

    #[test]
    fn agent_advertised_both_tools_is_full_ready() {
        let probe = local_probe(
            ProbeEvidence::AgentAdvertised,
            vec![
                "import-history__import_history_search".into(),
                "import-history__import_history_read".into(),
            ],
        );
        let decision = evaluate_probe(&probe, Some(ContinuationMode::FullHistoryTools));
        assert_eq!(decision, ProbeDecision::ReadyFull);
        assert!(decision.allows_full_send());
    }

    #[test]
    fn live_call_with_canonical_names_is_full_ready() {
        let probe = local_probe(
            ProbeEvidence::LiveAgentCall,
            vec![IMPORT_HISTORY_SEARCH.into(), IMPORT_HISTORY_READ.into()],
        );
        assert_eq!(evaluate_probe(&probe, None), ProbeDecision::ReadyFull);
    }

    #[test]
    fn missing_read_tool_blocks_full_mode() {
        let probe = local_probe(
            ProbeEvidence::AgentAdvertised,
            vec![IMPORT_HISTORY_SEARCH.into()],
        );
        assert_eq!(
            evaluate_probe(&probe, Some(ContinuationMode::FullHistoryTools)),
            ProbeDecision::Blocked {
                reason: ProbeBlockReason::ToolsNotExposed
            }
        );
    }

    #[test]
    fn probe_failure_does_not_silent_downgrade_to_fragment() {
        let mut probe = local_probe(
            ProbeEvidence::AgentAdvertised,
            vec![IMPORT_HISTORY_SEARCH.into(), IMPORT_HISTORY_READ.into()],
        );
        probe.probe_error = Some("cli available_commands timed out".into());
        let blocked = evaluate_probe(&probe, Some(ContinuationMode::FullHistoryTools));
        assert_eq!(
            blocked,
            ProbeDecision::Blocked {
                reason: ProbeBlockReason::ProbeFailed
            }
        );
        assert!(!blocked.allows_any_send());
        let fragment = evaluate_probe(&probe, Some(ContinuationMode::FragmentOnly));
        assert_eq!(fragment, ProbeDecision::FragmentAllowed);
        assert!(!fragment.allows_full_send());
        assert!(fragment.allows_any_send());
    }

    #[test]
    fn remote_and_aux_never_full_ready_without_explicit_fragment() {
        for policy in [
            SessionMcpPolicy {
                ssh: true,
                ..SessionMcpPolicy::default()
            },
            SessionMcpPolicy {
                wsl: true,
                ..SessionMcpPolicy::default()
            },
            SessionMcpPolicy {
                tcp: true,
                ..SessionMcpPolicy::default()
            },
            SessionMcpPolicy {
                official_aux: true,
                ..SessionMcpPolicy::default()
            },
        ] {
            let probe = CapabilityProbe {
                policy,
                ..local_probe(
                    ProbeEvidence::AgentAdvertised,
                    vec![IMPORT_HISTORY_SEARCH.into(), IMPORT_HISTORY_READ.into()],
                )
            };
            assert_eq!(
                evaluate_probe(&probe, None),
                ProbeDecision::Blocked {
                    reason: ProbeBlockReason::RemoteOrPolicyDisabled
                }
            );
            assert_eq!(
                evaluate_probe(&probe, Some(ContinuationMode::FragmentOnly)),
                ProbeDecision::FragmentAllowed
            );
        }
    }

    #[test]
    fn official_custom_shared_independent_matrix_injects_and_is_ready_on_agent_list() {
        let g = grant();
        let combos = [
            (RouteKind::Official, HomeMode::Shared),
            (RouteKind::Official, HomeMode::Independent),
            (RouteKind::Custom, HomeMode::Shared),
            (RouteKind::Custom, HomeMode::Independent),
        ];
        for (route, home) in combos {
            // Custom + official-aux on the *main* shell omits user MCP; import-history
            // must still be synthesized. Shared vs independent does not rewrite ~/.grok.
            let user = json!([]);
            let servers = compose_session_mcp_servers(user, Some(&g), SessionMcpPolicy::default());
            let names: Vec<String> = servers
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.get("name")?.as_str().map(str::to_string))
                .collect();
            assert_eq!(
                names,
                vec!["import-history".to_string()],
                "{route:?} {home:?} must still inject import-history"
            );
            let probe = CapabilityProbe {
                route,
                home_mode: home,
                ..local_probe(
                    ProbeEvidence::AgentAdvertised,
                    vec![IMPORT_HISTORY_SEARCH.into(), IMPORT_HISTORY_READ.into()],
                )
            };
            let decision = evaluate_probe(&probe, Some(ContinuationMode::FullHistoryTools));
            assert_eq!(decision, ProbeDecision::ReadyFull);
            let trace = redacted_probe_trace(&probe, &decision);
            assert!(trace.get("cliVersion").is_some());
            assert_eq!(trace["decision"], json!("readyFull"));
            let dumped = trace.to_string();
            assert!(
                !dumped.contains("tok"),
                "trace must not leak the grant token"
            );
        }
    }

    #[test]
    fn mcp_tools_list_payload_is_not_agent_available_commands() {
        let listed = handle_method(&grant(), "tools/list", &json!({})).unwrap();
        assert!(
            listed.get("tools").is_some(),
            "self list must still describe the two tools"
        );
        let names = extract_advertised_import_tools(&listed);
        assert!(
            names.is_empty(),
            "MCP tools/list must not count as CLI available_commands: {names:?}"
        );
        assert!(!both_import_tools_exposed(&names));
    }

    #[test]
    fn available_commands_are_slash_commands_not_import_tools() {
        let params = json!({
            "sessionId": "agent-1",
            "update": {
                "sessionUpdate": "available_commands_update",
                "availableCommands": [
                    {"name": "import-history__import_history_search", "description": "search"},
                    {"name": "mcp__import-history__import_history_read"},
                    {"name": "bash"}
                ]
            }
        });
        let names = extract_advertised_import_tools(&params);
        assert!(
            names.is_empty(),
            "slash availableCommands must not gate import-history: {names:?}"
        );
    }

    #[test]
    fn arbitrary_top_level_tools_and_partial_meta_do_not_unlock() {
        let self_list_shape = json!({
            "tools": [
                {"name": "import_history_search"},
                {"name": "import_history_read"}
            ]
        });
        assert!(
            extract_advertised_import_tools(&self_list_shape).is_empty(),
            "a generic MCP tools/list response is not Agent advertisement"
        );

        let partial = json!({
            "_meta": {
                "tools": [
                    {"name": "import-history__import_history_search"},
                    {"name": "import-history__not_import_history_read"}
                ]
            }
        });
        let names = extract_advertised_import_tools(&partial);
        assert_eq!(names, vec!["import-history__import_history_search"]);
        assert!(!both_import_tools_exposed(&names));
    }

    #[test]
    fn meta_tools_camel_and_snake_advertise_import_tools() {
        let params = json!({
            "sessionId": "agent-1",
            "update": {
                "sessionUpdate": "tool_status",
                "_meta": {
                    "tools": [
                        {"name": "import-history__import_history_search"},
                        {"tool_name": "import-history__import_history_read"}
                    ]
                }
            }
        });
        let names = extract_advertised_import_tools(&params);
        assert!(both_import_tools_exposed(&names), "{names:?}");
        let snake = json!({
            "_meta": { "tools": ["import_history_search", "import_history_read"] }
        });
        assert!(both_import_tools_exposed(&extract_advertised_import_tools(
            &snake
        )));
    }

    #[test]
    fn capability_state_does_not_silent_downgrade() {
        let probe = local_probe(
            ProbeEvidence::AgentAdvertised,
            vec![IMPORT_HISTORY_SEARCH.into(), IMPORT_HISTORY_READ.into()],
        );
        assert_eq!(
            capability_state(&probe, Some(ContinuationMode::FullHistoryTools)),
            CapabilityState::AgentAdvertised
        );
        let mut blocked = probe.clone();
        blocked.probe_error = Some("timeout".into());
        assert_eq!(
            capability_state(&blocked, Some(ContinuationMode::FullHistoryTools)),
            CapabilityState::Unavailable
        );
        assert_eq!(
            capability_state(&blocked, Some(ContinuationMode::FragmentOnly)),
            CapabilityState::McpReady
        );
    }
}
