//! Deterministic retirement. Sleep is only the failure bound.
//!
//! `revoke_between_sweep_and_terminal_check_still_drops_artifacts` fails if the
//! sweep after a terminal `current()` is removed: the revoke lands while the
//! owners are parked, and the path and pixels are still there when they exit.
//! `zero_remainder_each_store_publishes_expiry_itself` fails if a zero remainder
//! waits on the epoch instead of re-reading `current()`: the clock returns zero
//! without a publish, so that wait never wakes. Each store is alone.
//! `sync_bind_without_a_runtime_still_retires_before_get_or_export` fails if a
//! synchronous bind starts no owner: nothing exits after revoke.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::{DriveHooks, ExitFlag, Park};
use crate::files::SessionFiles;
use crate::image_cache::ImageCache;
use gcu_core::scope::{Clock, FixtureScope, ScopeLease};
use gcu_host_protocol::messages::Image;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn lease(generation: u64) -> ScopeLease {
    ScopeLease::install(
        FixtureScope::try_new("run", generation, "engine", "desktop", "control", vec![]).unwrap(),
    )
}

fn image() -> Image {
    Image {
        mime: "image/png".into(),
        width: 1,
        height: 1,
        bytes: vec![7],
    }
}

fn commit_file(files: &SessionFiles) -> PathBuf {
    let mut staged = files.stage(b"png-bytes", "png").unwrap();
    files.seal(&mut staged).unwrap();
    {
        let mut admission = files.admission(std::slice::from_ref(&staged)).unwrap();
        admission.stamp(Some(("run".into(), 1)));
        admission.commit(std::slice::from_mut(&mut staged));
    }
    staged.path().to_path_buf()
}

fn commit_image(cache: &ImageCache) {
    let mut admission = cache.admission("shot".into(), image());
    admission.stamp(Some(("run".into(), 1)));
    admission.commit();
}

fn hooks(
    after_sweep: Option<Arc<Park>>,
    before_remaining: Option<Arc<Park>>,
    exited: Arc<ExitFlag>,
) -> DriveHooks {
    DriveHooks {
        after_sweep,
        before_remaining,
        exited: Some(exited),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn revoke_between_sweep_and_terminal_check_still_drops_artifacts() {
    let park = Park::new();
    let exited = ExitFlag::new();
    let shared = hooks(Some(Arc::clone(&park)), None, Arc::clone(&exited));
    let files = Arc::new(SessionFiles::new().unwrap());
    let cache = Arc::new(ImageCache::default());
    files.set_retire_hooks(shared.clone());
    cache.set_retire_hooks(shared);
    let bound = lease(1);
    let mut parked = park.subscribe();
    // Spawned tasks do not poll until the first await below, after both commits.
    files.bind_lease(bound.clone());
    cache.bind_lease(bound.clone());
    let path = commit_file(&files);
    commit_image(&cache);
    parked.wait_for(|count| *count >= 2).await.unwrap();
    assert!(path.exists(), "first sweep must keep the delivered file");
    assert!(
        cache.resident("shot"),
        "first sweep must keep the delivered image"
    );
    bound.revoke();
    park.release();
    assert!(
        exited.wait_yielding(2, Duration::from_secs(2)).await,
        "both owners must leave after the parked revoke"
    );
    assert!(
        !path.exists(),
        "terminal sweep must delete the file before export"
    );
    assert!(
        !cache.resident("shot"),
        "terminal sweep must drop pixels before get"
    );
    assert!(files.export().unwrap().is_empty());
    assert!(cache.get("shot").is_none());
    files.close().unwrap();
}

#[derive(Debug)]
struct CrossClock {
    armed: AtomicBool,
    after_arm: AtomicUsize,
}

impl CrossClock {
    fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            after_arm: AtomicUsize::new(0),
        }
    }

    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    fn after_arm(&self) -> usize {
        self.after_arm.load(Ordering::SeqCst)
    }
}

impl Clock for CrossClock {
    fn now(&self) -> Duration {
        if !self.armed.load(Ordering::SeqCst) {
            return Duration::from_millis(1);
        }
        // The first read after arm stays before the deadline, so `expire`
        // inside `remaining` does not publish. The next read is the deadline.
        let sample = self.after_arm.fetch_add(1, Ordering::SeqCst);
        if sample == 0 {
            Duration::from_millis(1)
        } else {
            Duration::from_secs(5)
        }
    }
}

fn deadline_lease(clock: Arc<CrossClock>) -> ScopeLease {
    let scope = FixtureScope::try_new("run", 1, "engine", "desktop", "control", vec![]).unwrap();
    ScopeLease::install_with_deadline(scope, clock, Duration::from_secs(5))
}

#[tokio::test(flavor = "current_thread")]
async fn zero_remainder_each_store_publishes_expiry_itself() {
    zero_remainder_file().await;
    zero_remainder_image().await;
}

async fn zero_remainder_file() {
    let clock = Arc::new(CrossClock::new());
    let park = Park::new();
    let exited = ExitFlag::new();
    let files = Arc::new(SessionFiles::new().unwrap());
    files.set_retire_hooks(hooks(None, Some(Arc::clone(&park)), Arc::clone(&exited)));
    let bound = deadline_lease(Arc::clone(&clock));
    let mut parked = park.subscribe();
    files.bind_lease(bound);
    let path = commit_file(&files);
    parked.wait_for(|count| *count >= 1).await.unwrap();
    assert!(
        path.exists(),
        "file must still be delivered at the remainder sample"
    );
    clock.arm();
    park.release();
    assert!(
        exited.wait_yielding(1, Duration::from_secs(2)).await,
        "file owner must rescan a zero remainder without another store"
    );
    assert!(
        clock.after_arm() >= 2,
        "the straddle pair was not sampled: {}",
        clock.after_arm()
    );
    assert!(!path.exists(), "file owner must delete before export");
    assert!(files.export().unwrap().is_empty());
    files.close().unwrap();
}

async fn zero_remainder_image() {
    let clock = Arc::new(CrossClock::new());
    let park = Park::new();
    let exited = ExitFlag::new();
    let cache = Arc::new(ImageCache::default());
    cache.set_retire_hooks(hooks(None, Some(Arc::clone(&park)), Arc::clone(&exited)));
    let bound = deadline_lease(Arc::clone(&clock));
    let mut parked = park.subscribe();
    cache.bind_lease(bound);
    commit_image(&cache);
    parked.wait_for(|count| *count >= 1).await.unwrap();
    assert!(
        cache.resident("shot"),
        "pixels must still be delivered at the remainder sample"
    );
    clock.arm();
    park.release();
    assert!(
        exited.wait_yielding(1, Duration::from_secs(2)).await,
        "image owner must rescan a zero remainder without another store"
    );
    assert!(
        clock.after_arm() >= 2,
        "the straddle pair was not sampled: {}",
        clock.after_arm()
    );
    assert!(
        !cache.resident("shot"),
        "image owner must drop pixels before get"
    );
    assert!(cache.get("shot").is_none());
}

#[test]
fn sync_bind_without_a_runtime_still_retires_before_get_or_export() {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let exited = ExitFlag::new();
    let shared = hooks(None, None, Arc::clone(&exited));
    let files = Arc::new(SessionFiles::new().unwrap());
    let cache = Arc::new(ImageCache::default());
    files.set_retire_hooks(shared.clone());
    cache.set_retire_hooks(shared);
    let bound = lease(1);
    files.bind_lease(bound.clone());
    cache.bind_lease(bound.clone());
    let path = commit_file(&files);
    commit_image(&cache);
    assert!(path.exists(), "sync owner must not hide a live delivery");
    assert!(cache.resident("shot"));
    bound.revoke();
    assert!(
        exited.wait_blocking(2, Duration::from_secs(5)),
        "sync owners must exit after revoke"
    );
    assert!(
        !path.exists(),
        "sync owner must delete the file before export"
    );
    assert!(
        !cache.resident("shot"),
        "sync owner must drop pixels before get"
    );
    assert!(files.export().unwrap().is_empty());
    assert!(cache.get("shot").is_none());
    files.close().unwrap();
}
