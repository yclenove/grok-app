//! Revoke stops serving delivered bytes. Disk and pixels stay until the next
//! read or until close/drop.
//!
//! `export_after_a_lock_holder_releases_does_not_return_revoked_bytes` fails if
//! export copies ledger rows without `visible`: the holder releases the mutex
//! with the old row still stored, and export is the next locker. Removing
//! `SessionFiles`' drop purge leaves `ran` false; `TempDir`'s later field drop
//! is not that cleanup. Each blocking test has an abort watchdog.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::files::SessionFiles;
use super::image_cache::ImageCache;
use super::lease_view::Epoch;
use gcu_core::scope::{FixtureScope, ScopeLease};
use gcu_host_protocol::messages::Image;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn watch(bound: Duration) -> Watchdog {
    let (tx, rx) = mpsc::channel();
    let thread = thread::spawn(move || {
        if rx.recv_timeout(bound).is_err() {
            eprintln!("artifact cleanup watchdog: blocked past {bound:?}");
            std::process::abort();
        }
    });
    Watchdog {
        tx: Some(tx),
        thread: Some(thread),
    }
}

struct Watchdog {
    tx: Option<mpsc::Sender<()>>,
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

fn lease(run: &str, generation: u64) -> ScopeLease {
    ScopeLease::install(
        FixtureScope::try_new(run, generation, "engine", "desktop", "control", vec![]).unwrap(),
    )
}

fn image(byte: u8) -> Image {
    Image {
        mime: "image/png".into(),
        width: 1,
        height: 1,
        bytes: vec![byte],
    }
}

fn deliver_file(files: &SessionFiles, bytes: &[u8], epoch: Epoch) -> std::path::PathBuf {
    let mut staged = files.stage(bytes, "png").unwrap();
    files.seal(&mut staged).unwrap();
    {
        let mut admission = files.admission(std::slice::from_ref(&staged)).unwrap();
        admission.stamp(Some(epoch));
        admission.commit(std::slice::from_mut(&mut staged));
    }
    staged.path().to_path_buf()
}

fn deliver_image(cache: &ImageCache, byte: u8, epoch: Epoch) {
    let mut admission = cache.admission("shot".into(), image(byte));
    admission.stamp(Some(epoch));
    admission.commit();
}

fn recv(rx: &mpsc::Receiver<()>) {
    rx.recv_timeout(Duration::from_secs(8))
        .expect("handoff timed out");
}

#[test]
fn revoke_then_export_and_get_reject_without_returning_old_bytes() {
    let _watch = watch(Duration::from_secs(8));
    let bound = lease("run", 1);
    let files = SessionFiles::new().unwrap();
    let cache = ImageCache::default();
    files.bind_lease(bound.clone());
    cache.bind_lease(bound.clone());
    let path = deliver_file(&files, b"old-bytes", ("run".into(), 1));
    deliver_image(&cache, 4, ("run".into(), 1));
    bound.revoke();
    assert!(path.exists(), "revoke leaves the file until export");
    assert!(cache.resident("shot"), "revoke leaves pixels until get");
    let exported = files.export().unwrap();
    assert!(!path.exists(), "export must unlink the stale row");
    assert!(
        exported.is_empty(),
        "export returned revoked bytes: {exported:?}"
    );
    let got = cache.get("shot");
    assert!(!cache.resident("shot"), "get must drop the stale row");
    assert!(got.is_none(), "get returned revoked pixels: {got:?}");
    files.close().unwrap();
}

#[test]
fn export_after_a_lock_holder_releases_does_not_return_revoked_bytes() {
    let _watch = watch(Duration::from_secs(10));
    let bound = lease("run", 1);
    let files = Arc::new(SessionFiles::new().unwrap());
    files.bind_lease(bound.clone());
    let path = deliver_file(&files, b"old-bytes", ("run".into(), 1));
    let (holder_ready_tx, holder_ready_rx) = mpsc::channel();
    let (export_waiting_tx, export_waiting_rx) = mpsc::channel();
    let holder = thread::spawn({
        let files = Arc::clone(&files);
        move || {
            let guard = files.lock_area_for_test();
            holder_ready_tx.send(()).unwrap();
            recv(&export_waiting_rx);
            drop(guard);
        }
    });
    recv(&holder_ready_rx);
    bound.revoke();
    assert!(path.exists(), "the holder still owns the unfiltered row");
    files.arm_before_export_lock(move || {
        export_waiting_tx.send(()).unwrap();
    });
    let exporter = thread::spawn({
        let files = Arc::clone(&files);
        move || files.export().unwrap()
    });
    holder.join().unwrap();
    let exported = exporter.join().unwrap();
    assert!(!path.exists(), "export must unlink the stale row");
    assert!(
        exported
            .iter()
            .all(|(_, bytes)| bytes.as_slice() != b"old-bytes"),
        "export copied the row before the visibility check: {exported:?}"
    );
    assert!(exported.is_empty(), "export returned revoked bytes");
    files.close().unwrap();
}

#[test]
fn get_after_a_lock_holder_releases_does_not_return_revoked_pixels() {
    let _watch = watch(Duration::from_secs(10));
    let bound = lease("run", 1);
    let cache = Arc::new(ImageCache::default());
    cache.bind_lease(bound.clone());
    deliver_image(&cache, 9, ("run".into(), 1));
    assert!(cache.resident("shot"));
    let (holder_ready_tx, holder_ready_rx) = mpsc::channel();
    let (get_waiting_tx, get_waiting_rx) = mpsc::channel();
    let holder = thread::spawn({
        let cache = Arc::clone(&cache);
        move || {
            let guard = cache.lock_for_test();
            holder_ready_tx.send(()).unwrap();
            recv(&get_waiting_rx);
            drop(guard);
        }
    });
    recv(&holder_ready_rx);
    bound.revoke();
    cache.arm_before_get_lock(move || {
        get_waiting_tx.send(()).unwrap();
    });
    let getter = thread::spawn({
        let cache = Arc::clone(&cache);
        move || cache.get("shot")
    });
    holder.join().unwrap();
    let got = getter.join().unwrap();
    assert!(!cache.resident("shot"), "get must drop the stale row");
    assert!(
        got.is_none(),
        "get copied pixels before the visibility check"
    );
}

#[test]
fn binding_a_replacement_lease_hides_the_previous_epoch_on_access() {
    let _watch = watch(Duration::from_secs(8));
    let first = lease("run-a", 1);
    let second = lease("run-b", 1);
    let files = SessionFiles::new().unwrap();
    let cache = ImageCache::default();
    files.bind_lease(first.clone());
    cache.bind_lease(first.clone());
    let old_path = deliver_file(&files, b"from-a", ("run-a".into(), 1));
    deliver_image(&cache, 1, ("run-a".into(), 1));
    files.bind_lease(second.clone());
    cache.bind_lease(second.clone());
    assert!(
        old_path.exists(),
        "binding another lease does not delete the previous file"
    );
    assert!(
        cache.resident("shot"),
        "binding another lease does not drop the previous pixels"
    );
    let exported = files.export().unwrap();
    assert!(!old_path.exists(), "export must unlink the previous epoch");
    assert!(exported.is_empty(), "export returned the previous epoch");
    let got = cache.get("shot");
    assert!(!cache.resident("shot"));
    assert!(got.is_none(), "get returned the previous epoch");

    let new_path = deliver_file(&files, b"from-b", ("run-b".into(), 1));
    deliver_image(&cache, 2, ("run-b".into(), 1));
    first.revoke();
    assert!(
        new_path.exists(),
        "the old lease must not delete the new file"
    );
    assert!(cache.resident("shot"));
    let kept = files.export().unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, b"from-b");
    assert_eq!(cache.get("shot").unwrap().bytes, vec![2]);

    second.revoke();
    assert!(new_path.exists(), "revoke leaves the new file until export");
    assert!(
        cache.resident("shot"),
        "revoke leaves the new pixels until get"
    );
    let exported = files.export().unwrap();
    assert!(!new_path.exists());
    assert!(
        exported.is_empty(),
        "export returned bytes from the revoked lease"
    );
    let got = cache.get("shot");
    assert!(!cache.resident("shot"));
    assert!(got.is_none());
    files.close().unwrap();
}

fn observe_purge(files: &SessionFiles) -> (Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>) {
    let ran = Arc::new(AtomicBool::new(false));
    let lock_held = Arc::new(AtomicBool::new(false));
    let files_gone = Arc::new(AtomicBool::new(false));
    files.arm_purge_probe({
        let ran = Arc::clone(&ran);
        let lock_held = Arc::clone(&lock_held);
        let files_gone = Arc::clone(&files_gone);
        move |held, gone| {
            ran.store(true, Ordering::SeqCst);
            lock_held.store(held, Ordering::SeqCst);
            files_gone.store(gone, Ordering::SeqCst);
        }
    });
    (ran, lock_held, files_gone)
}

#[test]
fn close_deletes_every_registered_file_while_the_owner_lock_is_held() {
    let _watch = watch(Duration::from_secs(8));
    let files = SessionFiles::new().unwrap();
    let path_a = files.write(b"alpha", "txt").unwrap();
    let path_b = files.write(b"beta", "bin").unwrap();
    let dir = path_a.parent().unwrap().to_path_buf();
    let (ran, lock_held, files_gone) = observe_purge(&files);
    files.close().unwrap();
    assert!(ran.load(Ordering::SeqCst), "close did not purge");
    assert!(
        lock_held.load(Ordering::SeqCst),
        "close released the owner lock before unlinking"
    );
    assert!(
        files_gone.load(Ordering::SeqCst),
        "close left a registered file"
    );
    assert!(!path_a.exists());
    assert!(!path_b.exists());
    assert!(!dir.exists());
    files.close().unwrap();
}

#[test]
fn drop_deletes_every_registered_file_while_the_owner_lock_is_held() {
    let _watch = watch(Duration::from_secs(8));
    let files = SessionFiles::new().unwrap();
    let path_a = files.write(b"alpha", "txt").unwrap();
    let path_b = files.write(b"beta", "bin").unwrap();
    let dir = path_a.parent().unwrap().to_path_buf();
    let (ran, lock_held, files_gone) = observe_purge(&files);
    drop(files);
    assert!(
        ran.load(Ordering::SeqCst),
        "Drop did not purge; TempDir's field drop is not this cleanup"
    );
    assert!(
        lock_held.load(Ordering::SeqCst),
        "Drop released the owner lock before unlinking"
    );
    assert!(
        files_gone.load(Ordering::SeqCst),
        "Drop left a registered file"
    );
    assert!(!path_a.exists());
    assert!(!path_b.exists());
    assert!(!dir.exists());
}
