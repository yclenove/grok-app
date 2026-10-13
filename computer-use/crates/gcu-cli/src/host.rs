//! Minimal local Host: no approval UI, metadata-only events, persistent Stop.
use crate::{exit::Failure, private};
use async_trait::async_trait;
use gcu_host_api::*;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub(crate) struct LocalHost {
    pub activity: Arc<Activity>,
    events: Arc<Events>,
    pub host: gcu_mcp::Host,
}
impl LocalHost {
    pub(crate) fn new(config: PolicyConfig, mode: Option<String>) -> Result<Self, Failure> {
        let activity = Arc::new(Activity::open()?);
        let host = Self::assemble(config, mode, activity);
        host.events.load(&host.summary_path())?;
        Ok(host)
    }
    fn assemble(config: PolicyConfig, mode: Option<String>, activity: Arc<Activity>) -> Self {
        let events = Arc::new(Events::default());
        let host = gcu_mcp::Host {
            approvals: Arc::new(Approvals { mode }),
            policy: Arc::new(Policy(Mutex::new(config))),
            events: events.clone(),
            activity: activity.clone(),
        };
        Self {
            activity,
            events,
            host,
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture(directory: &std::path::Path) -> Self {
        Self::assemble(
            Default::default(),
            None,
            Arc::new(Activity {
                stopped: AtomicBool::new(directory.join("stopped").exists()),
                marker: directory.join("stopped"),
                generation: Mutex::new(0),
                io: Mutex::new(()),
            }),
        )
    }
    pub(crate) fn summaries(&self) -> serde_json::Value {
        serde_json::json!(*self.events.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub(crate) fn summary_path(&self) -> PathBuf {
        self.activity.marker.with_file_name("recent-calls.json")
    }
    pub(crate) async fn save_summaries(&self) -> Result<(), Failure> {
        let path = self.summary_path();
        let summaries = self.summaries();
        private::work(move || {
            private::write_secret(&path, summaries.to_string().as_bytes())
                .map_err(|_| Failure::internal("Cannot save metadata-only call summaries"))
        })
        .await
    }
}
struct Policy(Mutex<PolicyConfig>);
impl PolicyStore for Policy {
    fn load(&self) -> PolicyConfig {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn always_allowed(&self, id: &AppIdentity) -> bool {
        self.load().always.iter().any(|g| g.grant_eq(id))
    }
    fn refresh(&self) {}
}
struct Approvals {
    mode: Option<String>,
}
impl Approvals {
    fn decision(&self) -> ApprovalDecision {
        #[cfg(all(feature = "eval-autoapprove", debug_assertions))]
        match self.mode.as_deref() {
            Some("auto-allow-turn") => return ApprovalDecision::Turn,
            // The minimal Host has no persistent grants; Always is bounded to this session.
            Some("auto-allow-session" | "auto-allow-always") => return ApprovalDecision::Session,
            _ => (),
        }
        let _ = &self.mode;
        tracing::warn!("Approval denied: the real approval window is not implemented until P4");
        ApprovalDecision::Deny
    }
}
#[async_trait]
impl ApprovalProvider for Approvals {
    async fn request_app(
        &self,
        _: &SessionCtx,
        _: &AppInfo,
        _: Option<&ModelReason>,
        _: Risk,
        cancel: &Cancel,
    ) -> ApprovalDecision {
        if cancel.is_cancelled() {
            ApprovalDecision::Deny
        } else {
            self.decision()
        }
    }
    async fn confirm_action(
        &self,
        _: &SessionCtx,
        _: &AppInfo,
        _: &ActionSummary,
        _: Option<&ImageRef>,
        cancel: &Cancel,
    ) -> bool {
        !cancel.is_cancelled()
            && !matches!(
                self.decision(),
                ApprovalDecision::Deny | ApprovalDecision::Timeout
            )
    }
    async fn handoff(&self, _: &SessionCtx, _: &AppInfo, _: HandoffReason, _: &Cancel) {
        tracing::warn!("User handoff required; use the application directly");
    }
}
#[derive(Default)]
struct Events(Mutex<VecDeque<serde_json::Value>>);
impl Events {
    fn load(&self, path: &std::path::Path) -> Result<(), Failure> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(Failure::arguments("Cannot inspect call summaries")),
        };
        private::check_file(path)?;
        if metadata.len() > 8192
            || metadata
                .modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_none_or(|age| age.as_secs() > 7 * 86400)
        {
            return Ok(());
        }
        let source =
            std::fs::read(path).map_err(|_| Failure::arguments("Cannot read call summaries"))?;
        let entries: Vec<Summary> = serde_json::from_slice(&source)
            .map_err(|_| Failure::arguments("Invalid metadata-only call summaries"))?;
        let mut recent = self.0.lock().unwrap_or_else(|e| e.into_inner());
        for entry in entries
            .into_iter()
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let now = timestamp_ms();
            if entry.timestamp_ms > now || now.saturating_sub(entry.timestamp_ms) > 7 * 86400 * 1000
            {
                continue;
            }
            recent.push_back(serde_json::json!({"tool":entry.tool,"outcome":entry.outcome,"duration_ms":entry.duration_ms,"timestamp_ms":entry.timestamp_ms}));
        }
        Ok(())
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    tool: gcu_protocol::ToolName,
    outcome: gcu_protocol::result::Outcome,
    duration_ms: u64,
    timestamp_ms: u64,
}
fn timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
impl EventSink for Events {
    fn emit(&self, _: &SessionCtx, event: CuEvent) {
        let mut recent = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if recent.len() == 20 {
            recent.pop_front();
        }
        // target and thumbnail are payloads; neither is retained or logged.
        let record = serde_json::json!({"tool":event.tool,"outcome":event.outcome,"duration_ms":event.duration_ms,"timestamp_ms":timestamp_ms()});
        tracing::info!(tool = ?event.tool, outcome = ?event.outcome, duration_ms = event.duration_ms, "action");
        recent.push_back(record);
    }
}
pub(crate) struct Activity {
    stopped: AtomicBool,
    marker: PathBuf,
    generation: Mutex<u64>,
    io: Mutex<()>,
}
impl Activity {
    fn open() -> Result<Self, Failure> {
        let dir = private::state_dir()?;
        let marker = dir.join("stopped");
        let stopped = match std::fs::symlink_metadata(&marker) {
            Ok(_) => {
                private::check_file(&marker)?;
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(Failure::arguments("Cannot read persistent Stop latch")),
        };
        Ok(Self {
            stopped: AtomicBool::new(stopped),
            marker,
            generation: Mutex::new(0),
            io: Mutex::new(()),
        })
    }
    pub(crate) fn latch(&self) {
        let mut generation = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        *generation = generation.wrapping_add(1);
        self.stopped.store(true, Ordering::Release);
    }
    pub(crate) fn persist_stop(&self) -> Result<(), Failure> {
        let _io = self.io.lock().unwrap_or_else(|e| e.into_inner());
        // A delayed persistence worker must not recreate a marker after an
        // authenticated resume has already cleared this Stop.
        if !self.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        private::write_secret(&self.marker, b"stopped\n")
            .map_err(|_| Failure::internal("Stop latched in memory; persistence failed"))
    }
    pub(crate) fn resume(&self) -> Result<(), Failure> {
        let _io = self.io.lock().unwrap_or_else(|e| e.into_inner());
        let generation = *self.generation.lock().unwrap_or_else(|e| e.into_inner());
        match std::fs::remove_file(&self.marker) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => {
                return Err(Failure::internal(
                    "Cannot clear persisted Stop; remains stopped",
                ))
            }
        }
        let current = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        if *current != generation {
            drop(current);
            private::write_secret(&self.marker, b"stopped\n").map_err(|_| {
                Failure::internal("Stop persistence failed; memory remains stopped")
            })?;
            return Err(Failure::arguments(
                "A newer Stop superseded resume; remains stopped",
            ));
        }
        self.clear_stop_by_user();
        Ok(())
    }
}
impl UserActivity for Activity {
    fn ms_since_physical_input(&self) -> Option<u64> {
        None
    }
    fn stop_requested(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
    fn latched_stop(&self) -> bool {
        self.stop_requested()
    }
    fn clear_stop_by_user(&self) {
        self.stopped.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_delayed_persistence_after_resume_preserves_latest_latch() {
        let directory = tempfile::TempDir::new().unwrap();
        let activity = LocalHost::fixture(directory.path()).activity;
        activity.latch();
        activity.persist_stop().ok().unwrap();
        activity.resume().ok().unwrap();
        activity.persist_stop().ok().unwrap();
        assert!(!activity.latched_stop());
        assert!(!directory.path().join("stopped").exists());
        activity.latch();
        activity.persist_stop().ok().unwrap();
        assert!(activity.latched_stop());
        assert!(directory.path().join("stopped").exists());
    }
}
