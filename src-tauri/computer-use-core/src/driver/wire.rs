//! Wire compatibility with Cua's pinned, private SDK worker protocol.
//! No tool names or arguments supplied by a model reach this channel directly.
use serde::{Deserialize, Serialize};
use serde_json::Value;

macro_rules! cua_revision {
    () => {
        "c5a15f3df3b29ffbe774de9f33d632fe75afec75"
    };
}

pub const CUA_REVISION: &str = cua_revision!();
pub const WIRE_VERSION: u32 = 1;
pub const MAX_REQUEST: usize = 64 * 1024;
pub const MAX_RESPONSE: usize = 16 * 1024 * 1024;
pub const MAX_STDERR: usize = 64 * 1024;
pub const BUILD_IDENTITY: &str = concat!(
    "grok-computer-use-core@",
    env!("CARGO_PKG_VERSION"),
    "+",
    cua_revision!()
);

#[derive(Debug, Clone)]
pub struct Handshake {
    pub protocol_version: u32,
    pub build_identity: String,
    pub capabilities: Value,
    pub generation: String,
    pub max_response_size: usize,
}

#[derive(Serialize)]
pub(super) struct RequestCancellation {
    pub armed: bool,
}

#[derive(Serialize)]
pub(super) struct Request<'a> {
    pub protocol_version: u32,
    pub request_id: u64,
    pub generation: &'a str,
    pub run_id: &'a str,
    pub deadline_ms: u64,
    pub cancellation: RequestCancellation,
    pub operation: &'a str,
    pub name: Option<&'a str>,
    pub arguments: Value,
    pub session_handle: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completion {
    NotStarted,
    Completed,
    Unknown,
}

#[derive(Deserialize)]
pub(super) struct Response {
    pub protocol_version: u32,
    pub request_id: u64,
    pub generation: String,
    pub ok: bool,
    pub completion: Completion,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerError {
    pub completion: Completion,
    pub code: String,
    pub message: String,
}

impl WorkerError {
    pub(super) fn new(completion: Completion, code: &str, message: impl ToString) -> Self {
        Self {
            completion,
            code: code.into(),
            message: message.to_string(),
        }
    }
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WorkerError {}

fn reject_identity() -> WorkerError {
    WorkerError::new(
        Completion::NotStarted,
        "incompatible_driver",
        "worker identity or contract does not match the pinned driver",
    )
}

pub(super) fn validate_ready(
    value: &Value,
    pid: u32,
    host: &str,
    generation: &str,
) -> Result<Handshake, WorkerError> {
    let expected = [
        ("contract_version", "0.7.0"),
        ("tools_list_schema_version", "1"),
        ("capability_version", "1"),
        ("mcp_protocol_version", "2025-06-18"),
    ];
    if value["ready"] != true
        || value["pid"] != pid
        || value["host_bundle_id"] != host
        || expected
            .iter()
            .any(|(key, version)| value["metadata"][key] != *version)
    {
        return Err(reject_identity());
    }
    let protocol_version = value["protocol_version"]
        .as_u64()
        .ok_or_else(reject_identity)? as u32;
    if protocol_version != WIRE_VERSION {
        return Err(reject_identity());
    }
    let build_identity = value["build_identity"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(reject_identity)?;
    if build_identity != BUILD_IDENTITY {
        return Err(reject_identity());
    }
    let reported_generation = value["generation"].as_str().ok_or_else(reject_identity)?;
    if reported_generation != generation {
        return Err(reject_identity());
    }
    let max_response_size = value["max_response_size"]
        .as_u64()
        .ok_or_else(reject_identity)? as usize;
    if max_response_size != MAX_RESPONSE {
        return Err(reject_identity());
    }
    let capabilities = value
        .get("capabilities")
        .cloned()
        .ok_or_else(reject_identity)?;
    match capabilities.as_object() {
        Some(map) if !map.is_empty() => {}
        _ => return Err(reject_identity()),
    }
    Ok(Handshake {
        protocol_version,
        build_identity: build_identity.to_string(),
        capabilities,
        generation: generation.to_string(),
        max_response_size,
    })
}
