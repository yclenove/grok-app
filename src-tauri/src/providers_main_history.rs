//! Selected-agent migration into an exclusive, unpublished main generation.
//! This does not authorize cross-home resume; that requires a lifecycle witness.
use super::{execution_home::PendingHome, profile_io};
use crate::{session_execution::ExecutionSource, store::SessionMeta};
use std::{fs, path::PathBuf};

#[derive(Clone)]
pub(crate) struct MainHistoryRequest {
    /// Observed session metadata, never the desired provider's home.
    pub session: SessionMeta,
    /// Actual local connect cwd; the stored linked worktree takes precedence.
    pub cwd: String,
    /// Only used for rows with no execution receipt. Must be explicitly legacy.
    pub legacy_source: ExecutionSource,
}

impl std::fmt::Debug for MainHistoryRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MainHistoryRequest { selected_history: <private> }")
    }
}

fn unavailable() -> String {
    "selected execution history changed or is unavailable".into()
}

struct Directory {
    path: PathBuf,
    canonical: PathBuf,
    identity: [u64; 3],
}

impl Directory {
    fn capture(path: PathBuf) -> Result<Self, String> {
        let file = profile_io::open_node(&path, true)?;
        let captured = Self {
            canonical: path.canonicalize().map_err(|_| unavailable())?,
            identity: profile_io::node_id(&file)?,
            path,
        };
        captured.unchanged()?;
        Ok(captured)
    }

    fn unchanged(&self) -> Result<(), String> {
        if profile_io::node_id(&profile_io::open_node(&self.path, true)?)? != self.identity
            || self.path.canonicalize().map_err(|_| unavailable())? != self.canonical
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

fn cwd_bytes(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    let bytes = profile_io::read_optional_bounded(path, 128 * 1024)?;
    if bytes
        .as_ref()
        .is_some_and(|bytes| bytes.contains(&0) || std::str::from_utf8(bytes).is_err())
    {
        return Err("selected execution workspace metadata is invalid".into());
    }
    Ok(bytes)
}

pub(super) struct Snapshot {
    source: ExecutionSource,
    home: Directory,
    sessions: Directory,
    workspace: Directory,
    relative: PathBuf,
    history: profile_io::Tree,
    cwd: Option<Vec<u8>>,
    cwd_stamp: profile_io::InputStamp,
}

impl Snapshot {
    pub(super) fn witness(
        self,
        copied: Copied,
        request: &MainHistoryRequest,
        destination: ExecutionSource,
    ) -> Result<MigrationWitness, String> {
        self.unchanged()?;
        copied.unchanged()?;
        let ExecutionSource::AppProfile {
            owner_session_id, ..
        } = &destination
        else {
            return Err("history migration requires a published main generation".into());
        };
        if owner_session_id != &request.session.id
            || destination.home()?.as_deref() != Some(copied.home.path.as_path())
        {
            return Err("history migration destination ownership does not match".into());
        }
        let source = self.source;
        let source_proof = HistorySeal {
            directories: [self.home, self.sessions, self.workspace],
            history: self.history.seal(),
            cwd: self.cwd,
            cwd_stamp: self.cwd_stamp,
        };
        let destination_proof = HistorySeal {
            directories: [copied.home, copied.sessions, copied.workspace],
            history: copied.history.seal(),
            cwd: copied.cwd,
            cwd_stamp: copied.cwd_stamp,
        };
        Ok(MigrationWitness {
            origin: Origin::from(&request.session),
            source,
            destination,
            cwd: request
                .session
                .worktree_path
                .as_deref()
                .filter(|path| !path.is_empty())
                .unwrap_or(&request.cwd)
                .to_owned(),
            source_proof,
            destination_proof,
        })
    }

    pub(super) fn capture(request: &MainHistoryRequest, owner: &str) -> Result<Self, String> {
        if request.session.id != owner {
            return Err("selected history does not belong to the requested App session".into());
        }
        let agent = request
            .session
            .agent_session_id
            .as_deref()
            .ok_or("selected history has no observed agent identity")?;
        if crate::cli_sessions::validate_agent_session_id(agent)? != agent
            || agent.contains(':')
            || agent.len() > 256
        {
            return Err("selected history agent identity is invalid".into());
        }
        let source = if let Some(receipt) = &request.session.agent_execution {
            receipt.validate(Some(agent))?;
            receipt.source.clone()
        } else {
            match request.legacy_source {
                ExecutionSource::LegacyShared | ExecutionSource::LegacyIndependent => {
                    request.legacy_source.clone()
                }
                _ => return Err("selected history requires an explicit legacy source".into()),
            }
        };
        let cwd = request
            .session
            .worktree_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .unwrap_or(&request.cwd);
        if cwd.is_empty() || cwd.len() > 128 * 1024 || cwd.contains('\0') {
            return Err("selected history requires its actual local workspace".into());
        }
        let home = Directory::capture(source.home()?.ok_or_else(unavailable)?)?;
        let selected = crate::session_execution::find_in_home(&home.path, agent, Some(cwd))
            .ok_or_else(unavailable)?;
        let sessions = Directory::capture(home.path.join("sessions"))?;
        let workspace = Directory::capture(selected.parent().ok_or_else(unavailable)?.to_owned())?;
        let name = workspace.path.file_name().ok_or_else(unavailable)?;
        if !crate::session_execution::namespace_paths_match(
            &sessions.canonical,
            &home.canonical.join("sessions"),
        ) || !crate::session_execution::namespace_paths_match(
            &workspace.canonical,
            &sessions.canonical.join(name),
        ) {
            return Err(unavailable());
        }
        let relative = PathBuf::from("sessions").join(name).join(agent);
        let path = workspace.path.join(".cwd");
        let cwd_stamp = profile_io::input_stamp(&path)?;
        let cwd = cwd_bytes(&path)?;
        let captured = Self {
            source,
            home,
            sessions,
            workspace,
            relative,
            history: profile_io::Tree::capture_history(&selected)?,
            cwd,
            cwd_stamp,
        };
        captured.unchanged()?;
        Ok(captured)
    }

    pub(super) fn unchanged(&self) -> Result<(), String> {
        if self.source.home()?.as_deref() != Some(self.home.path.as_path()) {
            return Err(unavailable());
        }
        self.home.unchanged()?;
        self.sessions.unchanged()?;
        self.workspace.unchanged()?;
        self.history.unchanged()?;
        let path = self.workspace.path.join(".cwd");
        if profile_io::input_stamp(&path)? != self.cwd_stamp || cwd_bytes(&path)? != self.cwd {
            return Err(unavailable());
        }
        Ok(())
    }

    pub(super) fn copy_into(&self, pending: &PendingHome) -> Result<Copied, String> {
        self.unchanged()?;
        pending.check()?;
        let destination = pending.home().join(&self.relative);
        let workspace = destination.parent().ok_or_else(unavailable)?;
        // Exclusive creation: no merge, overwrite or retry into a partial tree.
        for path in [
            pending.home().join("sessions"),
            workspace.to_owned(),
            destination.clone(),
        ] {
            pending.check()?;
            fs::create_dir(&path).map_err(|_| unavailable())?;
            profile_io::open_node(&path, true)?;
            if !crate::session_execution::namespace_paths_match(
                &path.canonicalize().map_err(|_| unavailable())?,
                &pending
                    .home()
                    .canonicalize()
                    .map_err(|_| unavailable())?
                    .join(
                        path.strip_prefix(pending.home())
                            .map_err(|_| unavailable())?,
                    ),
            ) {
                return Err(unavailable());
            }
        }
        self.history.copy_into(&destination)?;
        if let Some(bytes) = &self.cwd {
            profile_io::write_new(&workspace.join(".cwd"), bytes)?;
        }
        let copied = Copied {
            home: Directory::capture(pending.home().to_owned())?,
            sessions: Directory::capture(pending.home().join("sessions"))?,
            workspace: Directory::capture(workspace.to_owned())?,
            history: profile_io::Tree::capture_history(&destination)?,
            cwd: self.cwd.clone(),
            cwd_stamp: profile_io::input_stamp(&workspace.join(".cwd"))?,
        };
        if !self.history.same_contents(&copied.history) {
            return Err("selected history copy does not match its captured source".into());
        }
        copied.unchanged()?;
        self.unchanged()?;
        pending.check()?;
        Ok(copied)
    }
}

pub(super) struct Copied {
    home: Directory,
    sessions: Directory,
    workspace: Directory,
    history: profile_io::Tree,
    cwd: Option<Vec<u8>>,
    cwd_stamp: profile_io::InputStamp,
}

impl Copied {
    pub(super) fn unchanged(&self) -> Result<(), String> {
        self.home.unchanged()?;
        self.sessions.unchanged()?;
        self.workspace.unchanged()?;
        self.history.unchanged()?;
        let path = self.workspace.path.join(".cwd");
        if profile_io::input_stamp(&path)? != self.cwd_stamp || cwd_bytes(&path)? != self.cwd {
            return Err(unavailable());
        }
        Ok(())
    }
}

struct HistorySeal {
    directories: [Directory; 3],
    history: profile_io::TreeSeal,
    cwd: Option<Vec<u8>>,
    cwd_stamp: profile_io::InputStamp,
}

impl HistorySeal {
    fn unchanged(&self) -> Result<(), String> {
        for directory in &self.directories {
            directory.unchanged()?;
        }
        self.history.unchanged()?;
        let path = self.directories[2].path.join(".cwd");
        if profile_io::input_stamp(&path)? != self.cwd_stamp || cwd_bytes(&path)? != self.cwd {
            return Err(unavailable());
        }
        Ok(())
    }
}

#[derive(PartialEq, Eq)]
struct Context {
    project: Option<String>,
    workspace: Option<String>,
    roots: Option<String>,
    capability: Option<String>,
    worktree: Option<String>,
    branch: Option<String>,
    is_worktree: bool,
}

impl From<&SessionMeta> for Context {
    fn from(meta: &SessionMeta) -> Self {
        Self {
            project: meta.project_id.clone(),
            workspace: meta.workspace_id.clone(),
            roots: meta.workspace_root_snapshot.clone(),
            capability: meta.workspace_capability.clone(),
            worktree: meta.worktree_path.clone(),
            branch: meta.worktree_branch.clone(),
            is_worktree: meta.is_worktree_session,
        }
    }
}

#[derive(PartialEq, Eq)]
struct Origin {
    owner: String,
    agent: Option<String>,
    receipt: Option<crate::session_execution::ExecutionReceipt>,
    context: Context,
    fork: bool,
    rewind: Option<u32>,
}

impl From<&SessionMeta> for Origin {
    fn from(meta: &SessionMeta) -> Self {
        Self {
            owner: meta.id.clone(),
            agent: meta.agent_session_id.clone(),
            receipt: meta.agent_execution.clone(),
            context: Context::from(meta),
            fork: meta.fork_agent_session,
            rewind: meta.fork_rewind_prompt_index,
        }
    }
}

/// Created only by consuming the producer's verified source/destination trees
/// after publication. No arbitrary path/receipt can construct this capability.
pub(crate) struct MigrationWitness {
    origin: Origin,
    source: ExecutionSource,
    destination: ExecutionSource,
    cwd: String,
    source_proof: HistorySeal,
    destination_proof: HistorySeal,
}

impl MigrationWitness {
    pub(crate) fn matches_origin(&self, meta: &SessionMeta) -> bool {
        self.origin == Origin::from(meta)
    }

    pub(crate) fn matches_context(&self, meta: &SessionMeta) -> bool {
        self.origin.owner == meta.id && self.origin.context == Context::from(meta)
    }

    pub(crate) fn destination(&self) -> &ExecutionSource {
        &self.destination
    }

    pub(crate) fn matches_cwd(&self, cwd: &str) -> bool {
        crate::session_execution::workspace_paths_match(&self.cwd, cwd)
    }

    pub(crate) fn matches_first_open(&self, agent: Option<&str>, fork: bool, cwd: &str) -> bool {
        agent == self.origin.agent.as_deref() && fork == self.origin.fork && self.matches_cwd(cwd)
    }

    /// Blocking worker only. Seals retain no media contents and all filesystem
    /// verification happens before the CLI is allowed to mutate its new history.
    pub(crate) fn verify_before_open(&self) -> Result<(), String> {
        if self.source.home()?.as_deref() != Some(self.source_proof.directories[0].path.as_path())
            || self.destination.home()?.as_deref()
                != Some(self.destination_proof.directories[0].path.as_path())
        {
            return Err(unavailable());
        }
        self.source_proof.unchanged()?;
        self.destination_proof.unchanged()
    }
}

#[cfg(test)]
#[path = "providers_main_history_tests.rs"]
mod tests;
