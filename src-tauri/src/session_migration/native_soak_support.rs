use super::*;
use serde::Serialize;

pub(super) struct SoakHome {
    pub root: PathBuf,
    old: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl SoakHome {
    pub fn from_environment() -> Self {
        assert_eq!(std::env::var("GROK_IMPORT_SOAK").as_deref(), Ok("1"));
        let root = PathBuf::from(
            std::env::var_os("GROK_IMPORT_SOAK_ROOT").expect("explicit root required"),
        );
        let name = root.file_name().unwrap().to_str().unwrap();
        let token = name
            .strip_prefix("grok-import-soak-")
            .expect("runner-owned root required");
        uuid::Uuid::parse_str(token).expect("unique run identity required");
        assert_eq!(
            root.parent().unwrap().canonicalize().unwrap(),
            std::env::temp_dir().canonicalize().unwrap()
        );
        let meta = std::fs::symlink_metadata(&root).unwrap();
        assert!(meta.is_dir() && !meta.file_type().is_symlink());
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            assert_eq!(meta.file_attributes() & 0x400, 0, "reparse root refused");
        }
        assert_eq!(
            std::fs::read_to_string(root.join("owner.txt")).unwrap(),
            token
        );
        let lock = crate::paths::APP_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let app = root.join("app-home");
        // Existing directories are refused; never adopt a user's persisted home.
        std::fs::create_dir(&app).expect("fresh app home required");
        let old = std::env::var_os("GROK_APP_HOME");
        std::env::set_var("GROK_APP_HOME", &app);
        let value = Self {
            root,
            old,
            _lock: lock,
        };
        crate::paths::ensure_app_dirs().unwrap();
        value
    }
}

impl Drop for SoakHome {
    fn drop(&mut self) {
        if let Some(old) = &self.old {
            std::env::set_var("GROK_APP_HOME", old);
        } else {
            std::env::remove_var("GROK_APP_HOME");
        }
        // Deliberately retain this owned synthetic root, including on failure.
    }
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Counters {
    pub cycles: u64,
    pub committed_imports: u64,
    pub native_turns: u64,
    pub job_polls: u64,
    pub job_pages: u64,
    pub reader_pages: u64,
    pub reader_searches: u64,
    pub cancels: u64,
    pub retries: u64,
    pub overlap_steps: u64,
}

pub(super) struct Metrics {
    pub started: Instant,
    pub counters: Counters,
    root: PathBuf,
    seconds: u64,
    last: Instant,
    output: std::fs::File,
    latencies: BTreeMap<&'static str, Vec<u128>>,
    publication: crate::session_migration::publish_timing::Capture,
}

impl Metrics {
    pub fn new(root: &Path, seconds: u64) -> Self {
        let output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("workload.jsonl"))
            .unwrap();
        let mut value = Self {
            started: Instant::now(),
            last: Instant::now(),
            root: root.to_owned(),
            seconds,
            counters: Counters::default(),
            output,
            latencies: BTreeMap::new(),
            publication: crate::session_migration::publish_timing::Capture::start(),
        };
        value.write(serde_json::json!({"kind":"start", "pid":std::process::id(),
            "durationSeconds":seconds, "appHome":root.join("app-home"),
            "scope":"synthetic-backend-active-load", "uiHeartbeat":null,
            "lockWait":null, "eventQueue":null, "acp":false}));
        value
    }

    fn write(&mut self, mut value: serde_json::Value) {
        value["at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        value["elapsedMs"] = serde_json::json!(self.started.elapsed().as_millis());
        writeln!(self.output, "{value}").unwrap();
        self.output.flush().unwrap();
    }

    pub fn check_stop(&self) {
        assert!(
            !self.root.join("stop-requested").exists(),
            "runner requested stop; not a completed soak"
        );
    }

    pub fn observe(&mut self, label: &'static str, elapsed: Duration) {
        self.latencies
            .entry(label)
            .or_default()
            .push(elapsed.as_micros());
    }

    pub fn worker_deadline(&mut self, elapsed: Duration) {
        // Preserve the in-flight stage before scoped-thread unwinding waits for
        // the worker. A deadline is still a failed workload, never a completion.
        self.write(
            serde_json::json!({"kind":"deadlineExceeded", "workerElapsedMs":elapsed.as_millis(),
            "counters":self.counters, "publication":self.publication.snapshot(),
            "uiHeartbeat":null, "lockWait":null, "eventQueue":null}),
        );
    }

    fn latency_summary(&self) -> serde_json::Value {
        let values: BTreeMap<_, _> = self
            .latencies
            .iter()
            .map(|(key, samples)| {
                let mut sorted = samples.clone();
                sorted.sort_unstable();
                let percentile =
                    |p: usize| sorted[(sorted.len() * p).div_ceil(100).saturating_sub(1)];
                (
                    *key,
                    serde_json::json!({"count":sorted.len(), "p50Us":percentile(50),
                "p95Us":percentile(95), "p99Us":percentile(99), "maxUs":sorted.last()}),
                )
            })
            .collect();
        serde_json::to_value(values).unwrap()
    }

    pub fn heartbeat(&mut self, job: Option<&JobRecord>, force: bool) {
        if !force && self.last.elapsed() < Duration::from_secs(30) {
            return;
        }
        let database = catalog::catalog_path();
        let file_size = |path: &Path| std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        self.write(
            serde_json::json!({"kind":"progress", "counters":self.counters,
            "job":job.map(|job| serde_json::json!({"id":job.job_id,
                "state":job.state, "version":job.state_version, "progress":job.progress})),
            "catalogBytes":file_size(&database),
            "latencies":self.latency_summary(), "publication":self.publication.snapshot(), "uiHeartbeat":null,
            "lockWait":null, "eventQueue":null}),
        );
        self.last = Instant::now();
    }

    pub fn finish(&mut self) {
        assert!(self.started.elapsed() >= Duration::from_secs(self.seconds));
        let publication = self.publication.snapshot();
        assert_eq!(
            publication["droppedSpans"], 0,
            "publication timing overflow"
        );
        assert!(publication["active"].as_array().unwrap().is_empty());
        for stage in [
            "cancelCheck",
            "destinationDirectory",
            "renameRevision",
            "verifyRevision",
            "publishedReceipt",
            "fingerprintBefore",
            "proofLookup",
            "fullVerification",
            "fingerprintAfter",
            "proofPersist",
            "proofSync",
            "proofPrune",
        ] {
            assert!(
                publication["completed"][stage]["count"]
                    .as_u64()
                    .unwrap_or(0)
                    > 0,
                "publication stage not observed: {stage}"
            );
        }
        self.write(
            serde_json::json!({"kind":"complete", "counters":self.counters,
            "latencies":self.latency_summary(), "publication":publication, "uiHeartbeat":null,
            "lockWait":null, "eventQueue":null, "periodicAppRestarts":0,
            "acceptance":"backend pilot only; not V53 I3 complete"}),
        );
    }
}
