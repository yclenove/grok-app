//! Retirement follows the lease publish, on the calling thread.
//!
//! Success does not sleep. Each case asserts the path is gone, or the pixels
//! are not resident, before `export` or `get`.
//!
//! Removing `on_invalidated` / `CallbackSlot::arm` leaves the path in place
//! after revoke, rebind, expiry, or a replacement lease. Removing the `fire`
//! call inside `publish` does the same. Invoking the callback while the lease
//! state lock is held is caught by gcu-core
//! `publish_runs_live_callbacks_outside_the_lock`: that hook calls `current`
//! and the watchdog aborts. This ledger test only checks that revoke does not
//! wait on a busy file-area mutex. After `try_lock`, it no longer deadlocks
//! when `fire` still holds the lease lock.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::files::SessionFiles;
use crate::image_cache::ImageCache;
use gcu_core::scope::{Clock, FixtureScope, ScopeLease};
use gcu_host_protocol::messages::Image;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn lease(run: &str, generation: u64) -> ScopeLease {
    ScopeLease::install(
        FixtureScope::try_new(run, generation, "engine", "desktop", "control", vec![]).unwrap(),
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

fn commit_file(files: &SessionFiles, run: &str, generation: u64, bytes: &[u8]) -> PathBuf {
    let mut staged = files.stage(bytes, "png").unwrap();
    files.seal(&mut staged).unwrap();
    {
        let mut admission = files.admission(std::slice::from_ref(&staged)).unwrap();
        admission.stamp(Some((run.to_owned(), generation)));
        admission.commit(std::slice::from_mut(&mut staged));
    }
    staged.path().to_path_buf()
}

fn commit_image(cache: &ImageCache, run: &str, generation: u64, id: &str) {
    let mut admission = cache.admission(id.to_owned(), image());
    admission.stamp(Some((run.to_owned(), generation)));
    admission.commit();
}

struct Delivered {
    files: Arc<SessionFiles>,
    cache: Arc<ImageCache>,
    path: PathBuf,
}

fn deliver(bound: &ScopeLease, run: &str, bytes: &[u8]) -> Delivered {
    let files = Arc::new(SessionFiles::new().unwrap());
    let cache = Arc::new(ImageCache::default());
    files.bind_lease(bound.clone());
    cache.bind_lease(bound.clone());
    let path = commit_file(&files, run, 1, bytes);
    commit_image(&cache, run, 1, "shot");
    assert!(
        path.exists(),
        "the delivered file must exist before invalidation"
    );
    assert!(cache.resident("shot"));
    Delivered { files, cache, path }
}

fn assert_retired(files: &SessionFiles, cache: &ImageCache, path: &Path) {
    assert!(!path.exists(), "file must be gone before export");
    assert!(!cache.resident("shot"), "pixels must be gone before get");
    assert!(files.export().unwrap().is_empty());
    assert!(cache.get("shot").is_none());
}

fn watch(bound: Duration) -> Watchdog {
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = thread::spawn(move || {
        if rx.recv_timeout(bound).is_err() {
            eprintln!("lease callback watchdog: blocked past {bound:?}");
            std::process::abort();
        }
    });
    Watchdog {
        tx: Some(tx),
        thread: Some(thread),
    }
}

struct Watchdog {
    tx: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug)]
struct Manual(AtomicU64);

impl Clock for Manual {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

#[test]
fn revoke_drops_files_and_pixels_before_export_or_get() {
    let bound = lease("run", 1);
    let delivered = deliver(&bound, "run", b"revoke");
    bound.revoke();
    assert_retired(&delivered.files, &delivered.cache, &delivered.path);
    let rejected = bound.commit_if_current(&[("run", 1)], || panic!("revoked commit ran"));
    assert!(rejected.is_err());
    delivered.files.close().unwrap();
}

#[test]
fn rebind_drops_files_and_pixels_before_export_or_get() {
    let bound = lease("run", 1);
    let delivered = deliver(&bound, "run", b"rebind");
    let next = FixtureScope::try_new("run", 2, "engine", "desktop", "control", vec![]).unwrap();
    bound.rebind(next).unwrap();
    assert_retired(&delivered.files, &delivered.cache, &delivered.path);
    delivered.files.close().unwrap();
}

#[test]
fn expiry_on_lease_read_drops_artifacts_before_export_or_get() {
    let clock = Arc::new(Manual(AtomicU64::new(0)));
    let scope = FixtureScope::try_new("run", 1, "engine", "desktop", "control", vec![]).unwrap();
    let bound = ScopeLease::install_with_deadline(scope, clock.clone(), Duration::from_secs(10));
    let delivered = deliver(&bound, "run", b"expiry");
    clock.0.store(10, Ordering::SeqCst);
    assert!(
        delivered.path.exists(),
        "expiry is not published until a lease read"
    );
    assert!(
        delivered.cache.resident("shot"),
        "expiry is not published until a lease read"
    );
    assert_eq!(bound.remaining(), Some(Duration::ZERO));
    assert_retired(&delivered.files, &delivered.cache, &delivered.path);
    delivered.files.close().unwrap();
}

#[test]
fn renew_does_not_retire_delivered_artifacts() {
    let clock = Arc::new(Manual(AtomicU64::new(0)));
    let scope = FixtureScope::try_new("run", 1, "engine", "desktop", "control", vec![]).unwrap();
    let bound = ScopeLease::install_with_deadline(scope, clock.clone(), Duration::from_secs(10));
    let delivered = deliver(&bound, "run", b"renew");
    bound.renew_deadline(Duration::from_secs(30)).unwrap();
    assert!(delivered.path.exists(), "renewal must keep the file");
    assert!(delivered.cache.resident("shot"), "renewal must keep pixels");
    assert_eq!(delivered.files.export().unwrap().len(), 1);
    assert!(delivered.cache.get("shot").is_some());
    delivered.files.close().unwrap();
}

#[test]
fn sync_bind_without_a_runtime_still_retires_before_get_or_export() {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let bound = lease("run", 1);
    let delivered = deliver(&bound, "run", b"sync");
    bound.revoke();
    assert_retired(&delivered.files, &delivered.cache, &delivered.path);
    delivered.files.close().unwrap();
}

#[test]
fn dropped_runtime_then_revoke_still_retires_before_get_or_export() {
    let bound = lease("run", 1);
    let files = Arc::new(SessionFiles::new().unwrap());
    let cache = Arc::new(ImageCache::default());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let path1 = runtime.block_on(async {
        assert!(tokio::runtime::Handle::try_current().is_ok());
        files.bind_lease(bound.clone());
        cache.bind_lease(bound.clone());
        commit_image(&cache, "run", 1, "shot");
        commit_file(&files, "run", 1, b"before-drop")
    });
    drop(runtime);
    assert!(tokio::runtime::Handle::try_current().is_err());
    assert!(path1.exists());
    assert!(cache.resident("shot"));
    files.bind_lease(bound.clone());
    cache.bind_lease(bound.clone());
    let path2 = commit_file(&files, "run", 1, b"after-drop");
    assert!(
        path1.exists(),
        "the same live lease must keep the first file"
    );
    assert!(path2.exists());
    bound.revoke();
    assert!(
        !path1.exists(),
        "file from before the runtime drop must be gone"
    );
    assert!(
        !path2.exists(),
        "file committed after the second bind must be gone"
    );
    assert!(!cache.resident("shot"), "pixels must be gone before get");
    assert!(files.export().unwrap().is_empty());
    assert!(cache.get("shot").is_none());
    let rejected = bound.commit_if_current(&[("run", 1)], || panic!("revoked commit ran"));
    assert!(rejected.is_err());
    files.close().unwrap();
}

#[test]
fn second_bind_of_the_same_live_lease_still_retires_on_revoke() {
    let bound = lease("run", 1);
    let files = Arc::new(SessionFiles::new().unwrap());
    let cache = Arc::new(ImageCache::default());
    files.bind_lease(bound.clone());
    cache.bind_lease(bound.clone());
    let path1 = commit_file(&files, "run", 1, b"first");
    commit_image(&cache, "run", 1, "shot");
    files.bind_lease(bound.clone());
    cache.bind_lease(bound.clone());
    assert!(
        path1.exists(),
        "binding the same live lease must keep its file"
    );
    assert!(cache.resident("shot"));
    let path2 = commit_file(&files, "run", 1, b"second");
    bound.revoke();
    assert!(!path1.exists(), "first file must be gone before export");
    assert!(!path2.exists(), "second file must be gone before export");
    assert!(!cache.resident("shot"), "pixels must be gone before get");
    assert!(files.export().unwrap().is_empty());
    assert!(cache.get("shot").is_none());
    files.close().unwrap();
}

#[test]
fn replacement_lease_bind_retires_only_on_the_new_lease() {
    let first = lease("run-a", 1);
    let files = Arc::new(SessionFiles::new().unwrap());
    let cache = Arc::new(ImageCache::default());
    files.bind_lease(first.clone());
    cache.bind_lease(first.clone());
    let path_a = commit_file(&files, "run-a", 1, b"run-a");
    commit_image(&cache, "run-a", 1, "shot");
    assert!(path_a.exists());
    assert!(cache.resident("shot"));

    let second = lease("run-b", 1);
    files.bind_lease(second.clone());
    cache.bind_lease(second.clone());
    assert!(
        !path_a.exists(),
        "binding a different lease must retire the previous file before export"
    );
    assert!(
        !cache.resident("shot"),
        "binding a different lease must drop the previous pixels before get"
    );

    let path_b = commit_file(&files, "run-b", 1, b"run-b");
    commit_image(&cache, "run-b", 1, "shot");
    assert!(path_b.exists());
    assert!(cache.resident("shot"));
    first.revoke();
    assert!(
        path_b.exists(),
        "the old lease must not delete the new file"
    );
    assert!(
        cache.resident("shot"),
        "the old lease must not drop the new pixels"
    );
    second.revoke();
    assert!(
        !path_b.exists(),
        "the new lease revoke must delete before export"
    );
    assert!(
        !cache.resident("shot"),
        "the new lease revoke must drop pixels before get"
    );
    assert!(files.export().unwrap().is_empty());
    assert!(cache.get("shot").is_none());
    let rejected = second.commit_if_current(&[("run-b", 1)], || panic!("revoked commit ran"));
    assert!(rejected.is_err());
    files.close().unwrap();
}

#[test]
fn dropped_store_then_revoke_does_not_panic() {
    let bound = lease("run", 1);
    let files = Arc::new(SessionFiles::new().unwrap());
    let cache = Arc::new(ImageCache::default());
    files.bind_lease(bound.clone());
    cache.bind_lease(bound.clone());
    let _path = commit_file(&files, "run", 1, b"drop");
    commit_image(&cache, "run", 1, "shot");
    let files_weak = Arc::downgrade(&files);
    let cache_weak = Arc::downgrade(&cache);
    drop(files);
    drop(cache);
    assert!(
        files_weak.upgrade().is_none(),
        "the lease must not keep the file store alive"
    );
    assert!(
        cache_weak.upgrade().is_none(),
        "the lease must not keep the image cache alive"
    );
    let revoked = std::panic::catch_unwind(AssertUnwindSafe(|| bound.revoke()));
    assert!(revoked.is_ok(), "revoke after store drop panicked");
    assert!(bound.current().is_none());
}

#[test]
fn ledger_lock_holder_and_callback_do_not_deadlock() {
    // 10s abort if the callback blocks on the ledger while the holder blocks
    // on the callback. try_lock must not wait. The handshakes fail at 8s.
    // Fire-under-the-lease-lock is the core watchdog test, not this one.
    let _watch = watch(Duration::from_secs(10));
    let bound = lease("run", 1);
    let delivered = deliver(&bound, "run", b"locked");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let entered = Arc::new(Mutex::new(Some(entered_tx)));
    let signal = Arc::clone(&entered);
    delivered.files.set_before_sweep(Arc::new(move || {
        if let Some(tx) = signal.lock().unwrap_or_else(|err| err.into_inner()).take() {
            let _ = tx.send(());
        }
    }));
    let files = Arc::clone(&delivered.files);
    let bound_hold = bound.clone();
    let holder = thread::spawn(move || {
        let guard = files.lock_area_for_test();
        ready_tx.send(()).expect("holder ready send");
        entered_rx
            .recv_timeout(Duration::from_secs(8))
            .expect("callback did not enter before taking the ledger lock");
        // The lease read lock is free here: publish drops it before the callback.
        // This thread then unlocks the ledger and finishes a deferred sweep.
        assert!(bound_hold.current().is_none());
        drop(guard);
    });
    ready_rx
        .recv_timeout(Duration::from_secs(8))
        .expect("holder did not take the ledger lock");
    bound.revoke();
    holder.join().expect("ledger holder panicked");
    assert!(
        !delivered.path.exists(),
        "revoke must delete the file before export"
    );
    assert!(delivered.files.export().unwrap().is_empty());
    delivered.files.close().unwrap();
}
