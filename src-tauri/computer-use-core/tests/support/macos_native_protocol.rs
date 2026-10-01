//! Private owned Cocoa fixture protocol. Never an input or observation backend.
use super::macos_native_pointer::PointerState;
use serde::{Deserialize, Serialize};
use std::io::BufRead;

pub const MAX_REPLY: usize = 32 * 1024;
pub const VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyEvent {
    pub code: u16,
    pub down: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub version: u32,
    pub nonce: String,
    pub id: u64,
    pub pid: u32,
    pub architecture: String,
    pub translated: bool,
    pub window_id: u32,
    pub title: String,
    pub active: bool,
    pub focused: bool,
    pub text: String,
    pub selection: [usize; 2],
    pub keys: Vec<KeyEvent>,
    pub clicks: u32,
    pub pointer: PointerState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn read_line(reader: &mut impl BufRead) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|e| e.to_string())?;
        if available.is_empty() {
            return Err("fixture EOF before complete reply".into());
        }
        let end = available.iter().position(|b| *b == b'\n');
        let n = end.map_or(available.len(), |i| i + 1);
        if bytes.len() + n > MAX_REPLY {
            return Err("fixture reply too large".into());
        }
        bytes.extend_from_slice(&available[..n]);
        reader.consume(n);
        if end.is_some() {
            return Ok(bytes);
        }
    }
}

pub fn parse(
    bytes: &[u8],
    nonce: &str,
    pid: u32,
    id: u64,
    window: Option<u32>,
) -> Result<State, String> {
    if bytes.len() > MAX_REPLY || bytes.last() != Some(&b'\n') {
        return Err("fixture reply is incomplete or oversized".into());
    }
    let state: State = serde_json::from_slice(bytes).map_err(|e| format!("fixture JSON: {e}"))?;
    if state.version != VERSION
        || state.architecture != std::env::consts::ARCH
        || state.translated
        || state.nonce != nonce
        || state.id != id
        || state.pid != pid
        || pid == 0
        || state.window_id == 0
        || state.window_id > i32::MAX as u32
        || window.is_some_and(|w| w != state.window_id)
        || state.title != format!("GrokCuOwned-{nonce}")
        || state.error.is_some()
    {
        return Err("fixture identity, sequence, or command failed".into());
    }
    let count = state.text.encode_utf16().count();
    if state.text.len() > 8192
        || state.keys.len() > 64
        || state.selection[0]
            .checked_add(state.selection[1])
            .is_none_or(|end| end > count)
    {
        return Err("fixture postcondition has invalid bounds".into());
    }
    state.pointer.validate()?;
    Ok(state)
}

pub fn unchanged(before: &State, after: &State) -> Result<(), String> {
    if before.text != after.text
        || before.selection != after.selection
        || before.keys != after.keys
        || before.clicks != after.clicks
        || before.pointer != after.pointer
    {
        Err("rejected action changed the owned fixture".into())
    } else {
        Ok(())
    }
}

pub fn key_pair(state: &State, previous: &[KeyEvent], code: u16) -> Result<(), String> {
    let mut expected = previous.to_vec();
    expected.extend([
        KeyEvent { code, down: true },
        KeyEvent { code, down: false },
    ]);
    if state.keys != expected {
        return Err("owned control did not receive exactly one matching key pair".into());
    }
    Ok(())
}

/// Apple documents ENOENT as a native process on systems without this sysctl.
/// All other errors, malformed sizes and translated/unknown values fail closed.
#[cfg(any(test, target_os = "macos"))]
pub fn native_translation_readback(
    status: i32,
    value: i32,
    size: usize,
    missing: bool,
) -> Result<(), String> {
    if (status == 0 && size == std::mem::size_of::<i32>() && value == 0)
        || (status == -1 && missing)
    {
        Ok(())
    } else {
        Err("translated or unknown probe execution is not native architecture evidence".into())
    }
}
