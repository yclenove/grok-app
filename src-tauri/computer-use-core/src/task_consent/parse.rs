//! Strict request parser. The model cannot name a task, run, or revision.
use std::collections::HashSet;

use serde_json::Value;

use crate::adapter::SurfaceKind;
use crate::error::BrokerError;

use super::types::{
    bounded_text, schema, AccessAction, Capability, ParsedAction, MAX_CAPS, MAX_SURFACES,
};

pub(in crate::task_consent) fn parse_action(args: Value) -> Result<ParsedAction, BrokerError> {
    if args.get("taskId").is_some()
        || args.get("task_id").is_some()
        || args.get("runId").is_some()
        || args.get("run_id").is_some()
        || args.get("requestId").is_some()
        || args.get("request_id").is_some()
        || args.get("revision").is_some()
        || args.get("epoch").is_some()
        || args.get("featureEpoch").is_some()
    {
        return Err(schema("request rejected"));
    }
    let action: AccessAction =
        serde_json::from_value(args).map_err(|_| schema("request rejected"))?;
    match action {
        AccessAction::Status => Ok(ParsedAction::Status),
        AccessAction::Cancel => Ok(ParsedAction::Cancel),
        AccessAction::Request {
            purpose,
            surfaces,
            capabilities,
            scope_intent,
        } => {
            let purpose = bounded_text(&purpose)?;
            let scope_intent = bounded_text(&scope_intent)?;
            if !(1..=MAX_SURFACES).contains(&surfaces.len())
                || !(1..=MAX_CAPS).contains(&capabilities.len())
            {
                return Err(schema("request rejected"));
            }
            let mut parsed_surfaces = Vec::with_capacity(surfaces.len());
            for surface in &surfaces {
                parsed_surfaces.push(parse_surface(surface)?);
            }
            if parsed_surfaces.iter().collect::<HashSet<_>>().len() != parsed_surfaces.len() {
                return Err(schema("request rejected"));
            }
            let mut parsed_caps = Vec::with_capacity(capabilities.len());
            for capability in &capabilities {
                parsed_caps.push(parse_capability(capability)?);
            }
            if parsed_caps.iter().collect::<HashSet<_>>().len() != parsed_caps.len() {
                return Err(schema("request rejected"));
            }
            Ok(ParsedAction::Request {
                purpose,
                surfaces: parsed_surfaces,
                capabilities: parsed_caps,
                scope_intent,
            })
        }
    }
}

fn parse_surface(value: &str) -> Result<SurfaceKind, BrokerError> {
    match value.trim() {
        "desktop" => Ok(SurfaceKind::Desktop),
        "managed-browser" => Ok(SurfaceKind::ManagedBrowser),
        "existing-tabs" => Ok(SurfaceKind::ExistingTab),
        "app-webview" => Ok(SurfaceKind::WebView),
        _ => Err(schema("request rejected")),
    }
}

fn parse_capability(value: &str) -> Result<Capability, BrokerError> {
    match value.trim() {
        "observe" => Ok(Capability::Observe),
        "input" => Ok(Capability::Input),
        _ => Err(schema("request rejected")),
    }
}
