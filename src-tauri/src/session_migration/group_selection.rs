//! A durable user choice, distinct from proof that a source belongs to a group.
use serde::{Deserialize, Serialize};

use super::errors::ImportError;
use super::scan::CandidateRef;
use super::types::SourceCandidate;

#[path = "group_selection_mutate.rs"]
mod mutate;
#[path = "group_selection_query.rs"]
mod query;
#[path = "group_selection_store.rs"]
mod store;

pub use mutate::{choose_member, clear, freeze_current, invert_many, set_many};
pub(crate) use query::get_owned;
pub(crate) use query::page_refs;
pub use query::{get, membership, page};

pub const GROUP_SELECTION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroupSelectionDomain {
    FixedGroups,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupSelection {
    pub version: u32,
    pub domain: GroupSelectionDomain,
    pub selection_id: String,
    pub scan_id: String,
    pub epoch: u64,
    pub state_version: u64,
    pub selected_count: u32,
    pub choice_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupChoice {
    pub group_id: String,
    pub group_seq: u32,
    pub candidate: SourceCandidate,
    pub selected: bool,
    pub choice_version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupSelectionPage {
    pub selection: GroupSelection,
    pub choices: Vec<GroupChoice>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupSelectionMembership {
    pub selection_id: String,
    pub state_version: u64,
    pub choices: Vec<GroupChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupMemberChange {
    pub selection: GroupSelection,
    pub group_id: String,
    pub previous_candidate_id: Option<String>,
    pub candidate_id: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Choice {
    version: u32,
    group_id: String,
    group_seq: u32,
    reference: CandidateRef,
    selected: bool,
    choice_version: u64,
    seal: String,
}

impl Choice {
    fn public(&self) -> GroupChoice {
        GroupChoice {
            group_id: self.group_id.clone(),
            group_seq: self.group_seq,
            candidate: self.reference.candidate.clone(),
            selected: self.selected,
            choice_version: self.choice_version,
        }
    }
}

pub(crate) fn initialize(
    conn: &rusqlite::Connection,
    scan_id: &str,
    epoch: u64,
) -> Result<(), ImportError> {
    store::initialize(conn, scan_id, epoch)
}
