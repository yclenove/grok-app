//! Spotlight recent-app cache. The query runs off the discovery worker.
//!
//! The first caller must not block on `MDQuery`. A missing cache reports
//! indexing and returns nothing. A fresh cache is returned as-is. A cache older
//! than 60 seconds is returned immediately while a refresh runs.

use crate::api::{MacApi, SpotlightHit};
use gcu_core::driver::{DriverError, Result};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

/// How long a Spotlight result may be served without a refresh.
pub const CACHE_TTL: Duration = Duration::from_secs(60);
/// Apps used within this window are "recent". Fourteen days.
pub const RECENT_WINDOW: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// What the caller should do with the cache it already has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheUse {
    /// Nothing is stored. Do not block. Indexing is in progress.
    Miss,
    /// The cache is inside [`CACHE_TTL`]. `indexing` is the in-flight refresh.
    Fresh {
        /// True only while a refresh is still running.
        indexing: bool,
    },
    /// The cache is older than [`CACHE_TTL`]. Return it and refresh.
    Stale,
}

/// Pure cache policy. `fetched_at` and `now` are injected so tests do not sleep.
pub fn cache_decision(fetched_at: Option<Instant>, now: Instant, inflight: bool) -> CacheUse {
    match fetched_at {
        None => CacheUse::Miss,
        Some(at) if now.saturating_duration_since(at) < CACHE_TTL => {
            CacheUse::Fresh { indexing: inflight }
        }
        Some(_) => CacheUse::Stale,
    }
}

/// Keep a hit when it was used in the last 14 days. Missing dates are dropped.
/// A timestamp in the future is kept.
pub fn within_recent(last_used: Option<SystemTime>, now: SystemTime) -> bool {
    let Some(last) = last_used else {
        return false;
    };
    match now.duration_since(last) {
        Ok(age) => age <= RECENT_WINDOW,
        Err(_) => true,
    }
}

struct CacheState {
    hits: Vec<SpotlightHit>,
    fetched_at: Option<Instant>,
    issues: Vec<String>,
}

/// Background Spotlight index. Drop requests shutdown; callers never join the query.
pub struct SpotlightIndex {
    state: Arc<Mutex<CacheState>>,
    indexing: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl SpotlightIndex {
    /// Start the query thread. The flag is already true before the first result.
    pub fn start(api: Arc<dyn MacApi>) -> Result<Self> {
        let index = Self {
            state: Arc::new(Mutex::new(CacheState {
                hits: Vec::new(),
                fetched_at: None,
                issues: Vec::new(),
            })),
            indexing: Arc::new(AtomicBool::new(true)),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let state = index.state.clone();
        let indexing = index.indexing.clone();
        let stop = index.stop.clone();
        thread::Builder::new()
            .name("gcu-macos-spotlight".into())
            .spawn(move || loop {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                indexing.store(true, Ordering::Release);
                match api.spotlight_recent() {
                    Ok(hits) => {
                        if let Ok(mut cache) = state.lock() {
                            cache.hits = hits;
                            cache.fetched_at = Some(Instant::now());
                        }
                    }
                    Err(error) => {
                        if let Ok(mut cache) = state.lock() {
                            cache.issues.push(error.to_string());
                            cache.issues.truncate(8);
                        }
                    }
                }
                indexing.store(false, Ordering::Release);
                let started = Instant::now();
                while started.elapsed() < CACHE_TTL {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    thread::park_timeout(Duration::from_millis(200));
                }
            })
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        Ok(index)
    }

    /// Shared flag for [`gcu_core::driver::Driver::discovery_status`].
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.indexing.clone()
    }

    /// Clone the current hits. Does not wait for a query.
    pub fn hits(&self) -> Result<Vec<SpotlightHit>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
            .hits
            .clone())
    }

    /// Metadata-only query failures.
    pub fn issues(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|cache| cache.issues.clone())
            .unwrap_or_else(|_| vec!["Spotlight index unavailable".into()])
    }
}

impl Drop for SpotlightIndex {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
