//! Local Host: approval grants, metadata-only events, persistent Stop.
pub(crate) mod activity;
pub(crate) mod approval;
pub(crate) mod dialog;
pub(crate) mod macos_wire;
pub(crate) mod page;
pub(crate) mod policy_store;

use crate::{exit::Failure, private};
use approval::Approvals;
use gcu_host_api::*;
use policy_store::HostPolicy;
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
        let path = private::state_dir()?.join("approvals.json");
        let host = Self::assemble(config, mode, activity, Some(path));
        host.events.load(&host.summary_path())?;
        Ok(host)
    }
    fn assemble(
        config: PolicyConfig,
        mode: Option<String>,
        activity: Arc<Activity>,
        approvals_path: Option<PathBuf>,
    ) -> Self {
        let events = Arc::new(Events::default());
        let policy = Arc::new(match approvals_path {
            Some(path) => HostPolicy::open(config, path),
            None => HostPolicy::memory(config),
        });
        let host = gcu_mcp::Host {
            approvals: Arc::new(Approvals::new(mode, policy.clone())),
            policy,
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
                physical: Mutex::new(None),
            }),
            None,
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

pub(crate) async fn approvals(args: crate::args::ApprovalArgs) -> Result<u8, Failure> {
    let path = private::state_dir()?.join("approvals.json");
    let locale = gcu_mcp::active_locale();
    match args.action {
        crate::args::ApprovalAction::List { json } => {
            let body = private::work(move || match json {
                true => policy_store::list_json(&path)
                    .map(|rows| serde_json::Value::Array(rows).to_string())
                    .map_err(|_| unusable(locale)),
                false => policy_store::list_text(&path, locale).map_err(|_| unusable(locale)),
            })
            .await?;
            println!("{body}");
            Ok(0)
        }
        crate::args::ApprovalAction::Revoke { key } => {
            let revoke_key = key.clone();
            let removed = private::work(move || {
                policy_store::revoke(&path, &revoke_key).map_err(|_| unusable(locale))
            })
            .await?;
            if removed {
                println!(
                    "{}",
                    gcu_mcp::fill(locale, "approvals.revoked", &[("key", key.as_str())])
                );
                Ok(0)
            } else {
                println!(
                    "{}",
                    gcu_mcp::fill(locale, "approvals.missing", &[("key", key.as_str())])
                );
                Ok(2)
            }
        }
    }
}

fn unusable(locale: &str) -> Failure {
    Failure::arguments(gcu_mcp::text(locale, "approvals.unusable"))
}

#[derive(Default)]
struct Events(Mutex<VecDeque<serde_json::Value>>);
impl Events {
    fn load(&self, path: &std::path::Path) -> Result<(), Failure> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(retention_failure(
                    path,
                    &error,
                    "Cannot inspect call summaries",
                ))
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Failure::arguments("Cannot inspect call summaries"));
        }
        if file_expired(&metadata)
            .map_err(|error| retention_failure(path, &error, "Cannot inspect call summaries"))?
        {
            return remove_summary(path);
        }
        private::check_file(path)?;
        if metadata.len() > 8192 {
            return Ok(());
        }
        let source = std::fs::read(path)
            .map_err(|error| retention_failure(path, &error, "Cannot read call summaries"))?;
        let entries: Vec<Summary> = serde_json::from_slice(&source)
            .map_err(|_| Failure::arguments("Invalid metadata-only call summaries"))?;
        let now = timestamp_ms();
        let mut kept = Vec::new();
        let mut dropped = 0usize;
        for entry in entries
            .into_iter()
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if entry.timestamp_ms > now || now.saturating_sub(entry.timestamp_ms) > retention_ms() {
                dropped += 1;
                continue;
            }
            kept.push(entry);
        }
        if dropped > 0 {
            if kept.is_empty() {
                remove_summary(path)?;
            } else {
                let body = serde_json::to_vec(&kept)
                    .map_err(|_| Failure::arguments("Cannot save metadata-only call summaries"))?;
                private::write_secret(path, &body).map_err(|error| {
                    retention_failure(path, &error, "Cannot save metadata-only call summaries")
                })?;
            }
        }
        let mut recent = self.0.lock().unwrap_or_else(|error| error.into_inner());
        for entry in kept {
            recent.push_back(serde_json::json!({"tool":entry.tool,"outcome":entry.outcome,"duration_ms":entry.duration_ms,"timestamp_ms":entry.timestamp_ms}));
        }
        Ok(())
    }
}

fn retention_ms() -> u64 {
    gcu_host_protocol::log_policy::RETENTION
        .as_secs()
        .saturating_mul(1000)
}

fn file_expired(metadata: &std::fs::Metadata) -> std::io::Result<bool> {
    let modified = metadata.modified()?;
    Ok(modified
        .elapsed()
        .map(|age| age > gcu_host_protocol::log_policy::RETENTION)
        .unwrap_or(false))
}

fn remove_summary(path: &std::path::Path) -> Result<(), Failure> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(retention_failure(
            path,
            &error,
            "Cannot delete expired call summaries",
        )),
    }
}

fn retention_failure(path: &std::path::Path, error: &std::io::Error, message: &str) -> Failure {
    tracing::warn!(
        category = "retention",
        location = %path.display(),
        io_kind = ?error.kind(),
        "call summary retention failed"
    );
    Failure::arguments(message)
}

#[derive(serde::Deserialize, serde::Serialize)]
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
    physical: Mutex<Option<Arc<dyn gcu_mcp::host::PhysicalActivity>>>,
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
            physical: Mutex::new(None),
        })
    }
    /// Native assembly supplies the same optional source used by route B.
    #[allow(dead_code)]
    pub(crate) fn set_physical(&self, physical: Arc<dyn gcu_mcp::host::PhysicalActivity>) {
        *self.physical.lock().unwrap_or_else(|e| e.into_inner()) = Some(physical);
    }
    pub(crate) fn latch(&self) {
        let mut generation = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        *generation = generation.wrapping_add(1);
        self.stopped.store(true, Ordering::Release);
    }
    pub(crate) fn persist_stop(&self) -> Result<(), Failure> {
        let _io = self.io.lock().unwrap_or_else(|e| e.into_inner());
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
        self.physical
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|source| source.ms_since_physical_input())
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
    fn activity_optional_source_forwards_some_and_none_without_changing_stop() {
        struct Source(Option<u64>);
        impl gcu_mcp::host::PhysicalActivity for Source {
            fn ms_since_physical_input(&self) -> Option<u64> {
                self.0
            }
        }
        let directory = tempfile::TempDir::new().unwrap();
        let activity = LocalHost::fixture(directory.path()).activity;
        assert_eq!(activity.ms_since_physical_input(), None);
        activity.set_physical(Arc::new(Source(Some(123))));
        assert_eq!(activity.ms_since_physical_input(), Some(123));
        activity.latch();
        activity.set_physical(Arc::new(Source(None)));
        assert_eq!(activity.ms_since_physical_input(), None);
        assert!(activity.latched_stop());
    }
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

    fn summary(tool: &str, timestamp_ms: u64) -> String {
        format!(
            r#"[{{"tool":"{tool}","outcome":"done","duration_ms":1,"timestamp_ms":{timestamp_ms}}}]"#
        )
    }

    fn write_summary(path: &std::path::Path, body: &str) {
        private::write_secret(path, body.as_bytes()).unwrap();
    }

    #[test]
    fn expired_recent_calls_file_is_unlinked() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("recent-calls.json");
        write_summary(&path, &summary("click", timestamp_ms()));
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(
            std::time::SystemTime::now()
                .checked_sub(
                    gcu_host_protocol::log_policy::RETENTION + std::time::Duration::from_secs(60),
                )
                .unwrap(),
        )
        .unwrap();
        drop(file);
        Events::default()
            .load(&path)
            .map_err(|error| error.message)
            .unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn stale_recent_call_entries_are_removed_from_disk() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("recent-calls.json");
        let fresh = timestamp_ms();
        write_summary(
            &path,
            &format!(
                "[{},{}]",
                summary("click", 1)
                    .trim_start_matches('[')
                    .trim_end_matches(']'),
                summary("scroll", fresh)
                    .trim_start_matches('[')
                    .trim_end_matches(']')
            ),
        );
        let events = Events::default();
        events.load(&path).map_err(|error| error.message).unwrap();
        let kept = std::fs::read_to_string(&path).unwrap();
        assert!(!kept.contains("click"));
        assert!(kept.contains("scroll"));
        let loaded = events.0.lock().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0]["tool"], "scroll");
    }

    #[cfg(unix)]
    #[test]
    fn recent_calls_delete_failure_is_returned() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("recent-calls.json");
        write_summary(&path, &summary("click", timestamp_ms()));
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(
            std::time::SystemTime::now()
                .checked_sub(
                    gcu_host_protocol::log_policy::RETENTION + std::time::Duration::from_secs(60),
                )
                .unwrap(),
        )
        .unwrap();
        drop(file);
        let mut permissions = std::fs::metadata(directory.path()).unwrap().permissions();
        permissions.set_mode(0o555);
        std::fs::set_permissions(directory.path(), permissions).unwrap();
        let result = Events::default().load(&path);
        let mut restore = std::fs::metadata(directory.path()).unwrap().permissions();
        restore.set_mode(0o700);
        std::fs::set_permissions(directory.path(), restore).unwrap();
        assert!(result.is_err());
        assert!(path.exists());
    }
}
