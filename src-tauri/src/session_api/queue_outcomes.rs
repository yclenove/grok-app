//! Bounded outcome journal, committed atomically with active queue changes.
use super::queue_store::Item;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Started,
    HandedOff,
    Cancelled,
    Reviewed,
    RetryAuthorized,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Outcome {
    pub session_id: String,
    pub item_id: String,
    prompt_hash: String,
    created_at: i64,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_id: Option<String>,
}
fn fingerprint(prompt: &str) -> String {
    format!("{:x}", Sha256::digest(prompt.as_bytes()))
}
impl Outcome {
    pub fn new(item: &Item, status: Status, replacement_id: Option<String>) -> Self {
        Self {
            session_id: item.session_id.clone(),
            item_id: item.item_id.clone(),
            prompt_hash: fingerprint(&item.prompt),
            created_at: item.created_at,
            status,
            replacement_id,
        }
    }
    pub fn matches(&self, session_id: &str, prompt: &str) -> bool {
        self.session_id == session_id && self.prompt_hash == fingerprint(prompt)
    }
    pub fn matches_item(&self, item: &Item) -> bool {
        self.item_id == item.item_id
            && self.created_at == item.created_at
            && self.matches(&item.session_id, &item.prompt)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.session_id.is_empty()
            || self.session_id.trim() != self.session_id
            || self.session_id == "__draft__"
            || self.session_id.starts_with("imp:")
            || self.item_id.is_empty()
            || self.item_id.trim() != self.item_id
            || self.prompt_hash.len() != 64
            || !self
                .prompt_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || match (&self.status, &self.replacement_id) {
                (Status::HandedOff | Status::RetryAuthorized, Some(id)) => {
                    id.is_empty() || id.trim() != id || id == &self.item_id
                }
                (Status::HandedOff | Status::RetryAuthorized, None) => true,
                (_, Some(_)) => true,
                (_, None) => false,
            }
        {
            return Err("invalid queue outcome".into());
        }
        crate::session_migration::reject_imported_history_id(Some(&self.session_id))?;
        Ok(())
    }
}
pub(super) enum Resolved {
    Queued(Item),
    Started,
    Cancelled,
    Unknown,
}
