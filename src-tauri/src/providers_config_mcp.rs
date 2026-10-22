//! Read ACP MCP definitions from the same structural tree as main projection.
//! Definitions can contain secrets: never log, Debug or expose the capture.
use super::{invalid, ConfigSnapshot, Table, Value};
use crate::extensions::McpServerDef;
use std::collections::HashMap;

fn string(table: &Table, key: &str) -> Result<Option<String>, String> {
    match table.fields.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(invalid()),
    }
}

fn strings(table: &Table, key: &str) -> Result<Option<HashMap<String, String>>, String> {
    match table.fields.get(key) {
        None => Ok(None),
        Some(Value::Table(table)) => table
            .fields
            .iter()
            .map(|(key, value)| match value {
                Value::String(value) => Ok((key.clone(), value.clone())),
                _ => Err(invalid()),
            })
            .collect::<Result<_, _>>()
            .map(Some),
        _ => Err(invalid()),
    }
}

impl ConfigSnapshot {
    pub(in crate::providers) fn mcp_definitions(&self) -> Result<Vec<McpServerDef>, String> {
        let servers = match self.root.fields.get("mcp_servers") {
            None => return Ok(Vec::new()),
            Some(Value::Table(servers)) => servers,
            _ => return Err(invalid()),
        };
        servers
            .fields
            .iter()
            .map(|(name, value)| {
                let Value::Table(server) = value else {
                    return Err(invalid());
                };
                let args = match server.fields.get("args") {
                    None => None,
                    Some(Value::Array(values)) => Some(
                        values
                            .iter()
                            .map(|value| match value {
                                Value::String(value) => Ok(value.clone()),
                                _ => Err(invalid()),
                            })
                            .collect::<Result<_, _>>()?,
                    ),
                    _ => return Err(invalid()),
                };
                let enabled = match server.fields.get("enabled") {
                    None => None,
                    Some(Value::Atom(value)) if value == "true" => Some(true),
                    Some(Value::Atom(value)) if value == "false" => Some(false),
                    _ => return Err(invalid()),
                };
                Ok(McpServerDef {
                    name: name.clone(),
                    command: string(server, "command")?,
                    args,
                    env: strings(server, "env")?,
                    url: string(server, "url")?,
                    headers: strings(server, "headers")?,
                    transport: string(server, "transport")?,
                    enabled,
                    scope: string(server, "scope")?,
                })
            })
            .collect()
    }
}
