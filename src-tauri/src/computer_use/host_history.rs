//! Host prompt fingerprints prevent internal continuation turns becoming user
//! prompts in CLI journal reconciliation. This contains no execution authority.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Read, path::PathBuf};

const MAX_RECEIPTS: usize = 4096;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    agent: String,
    sha256: String,
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.trim().as_bytes()))
}

pub(crate) fn path(session: &str) -> Result<PathBuf, String> {
    if session.is_empty()
        || session.len() > 256
        || !session
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("invalid App session identity".into());
    }
    Ok(crate::paths::session_dir(session).join("computer-use-host-turns.json"))
}

fn load(path: &std::path::Path) -> Result<Vec<Receipt>, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Host decision history unavailable".into()),
    };
    if metadata.len() > MAX_FILE_BYTES {
        return Err("Host decision history capacity exceeded".into());
    }
    // Bound the read itself: the file may grow or be replaced after metadata.
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Host decision history unavailable")?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Host decision history unavailable")?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("Host decision history capacity exceeded".into());
    }
    let receipts: Vec<Receipt> =
        serde_json::from_slice(&bytes).map_err(|_| "Host decision history invalid")?;
    if receipts.len() > MAX_RECEIPTS {
        return Err("Host decision history capacity exceeded".into());
    }
    Ok(receipts)
}

pub(crate) fn record(path: &std::path::Path, agent: &str, text: &str) -> Result<(), String> {
    let _guard = crate::store_lock::lock_exclusive(path)?;
    let mut receipts = load(path)?;
    let sha256 = digest(text);
    if receipts
        .iter()
        .any(|receipt| receipt.agent == agent && receipt.sha256 == sha256)
    {
        return Ok(());
    }
    // Fail explicitly rather than evicting an old receipt and importing its
    // Host prompt as a user message after an App restart.
    if receipts.len() == MAX_RECEIPTS {
        return Err("Host decision history capacity exceeded".into());
    }
    receipts.push(Receipt {
        agent: agent.into(),
        sha256,
    });
    // The caller already holds the store lock; the generic atomic writer
    // would recursively acquire the process-wide lock.
    let bytes = serde_json::to_vec_pretty(&receipts)
        .map_err(|_| "Host decision history serialization failed")?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("Host decision history capacity exceeded".into());
    }
    crate::store_lock::write_bytes_replace(path, &bytes)
}

pub(crate) fn fingerprints(session: &str, agent: &str) -> Result<HashSet<String>, String> {
    Ok(load(&path(session)?)?
        .into_iter()
        .filter(|receipt| receipt.agent == agent)
        .map(|receipt| receipt.sha256)
        .collect())
}

pub(crate) fn is_host_turn(fingerprints: &HashSet<String>, text: &str) -> bool {
    fingerprints.contains(&digest(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_new_receipt_is_rejected_without_corrupting_previous_history() {
        let root =
            std::env::temp_dir().join(format!("cu-host-history-limit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("receipts.json");
        record(&path, "original-agent", "original Host decision").unwrap();
        let previous = std::fs::read(&path).unwrap();
        let huge_agent = "x".repeat(MAX_FILE_BYTES as usize);
        assert!(record(&path, &huge_agent, "another Host decision").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        assert_eq!(load(&path).unwrap().len(), 1);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(crate::store_lock::lock_path_for(&path)).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn exact_recorded_body_is_scoped_to_original_agent_and_survives_reload() {
        let root = std::env::temp_dir().join(format!("cu-host-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("receipts.json");
        let text = "[Host Computer Use decision]\nfixed synthetic body";
        record(&path, "agent-one", text).unwrap();
        record(&path, "agent-one", text).unwrap();
        let rows = load(&path).unwrap();
        assert_eq!(rows.len(), 1);
        let hashes = rows
            .iter()
            .filter(|row| row.agent == "agent-one")
            .map(|row| row.sha256.clone())
            .collect();
        assert!(is_host_turn(&hashes, text));
        assert!(!is_host_turn(
            &hashes,
            "[Host Computer Use decision]\na user's different message"
        ));
        assert!(!rows.iter().any(|row| row.agent == "agent-two"));
        let stored = std::fs::read_to_string(&path).unwrap();
        assert!(!stored.contains(text));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(crate::store_lock::lock_path_for(&path)).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }
}
