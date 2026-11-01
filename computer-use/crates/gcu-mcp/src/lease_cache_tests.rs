//! Delivered images and files follow the lease generation stamped at commit.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::files::SessionFiles;
use super::image_cache::ImageCache;
use super::lease_view::{visible, Epoch};
use gcu_core::scope::{FixtureScope, Monotonic, ScopeLease};
use gcu_host_protocol::messages::Image;
use std::{sync::Arc, time::Duration};

fn lease(generation: u64) -> ScopeLease {
    ScopeLease::install(
        FixtureScope::try_new("run", generation, "engine", "desktop", "control", vec![]).unwrap(),
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

fn epoch(generation: u64) -> Epoch {
    ("run".into(), generation)
}

#[test]
fn visibility_matrix_hides_every_mismatch() {
    let bound = lease(1);
    assert!(visible(None, None));
    assert!(!visible(None, Some(&epoch(1))));
    assert!(!visible(Some(&bound), None));
    assert!(visible(Some(&bound), Some(&epoch(1))));
    assert!(!visible(Some(&bound), Some(&epoch(2))));
    bound.revoke();
    assert!(!visible(Some(&bound), Some(&epoch(1))));
}

#[test]
fn unscoped_image_and_file_without_an_epoch_stay_readable() {
    let cache = ImageCache::default();
    {
        let mut admission = cache.admission("shot".into(), image(1));
        admission.commit();
    }
    assert_eq!(cache.get("shot").unwrap().bytes, vec![1]);

    let files = SessionFiles::new().unwrap();
    let path = files.write(b"plain", "txt").unwrap();
    let exported = files.export().unwrap();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].0, path);
    assert_eq!(exported[0].1, b"plain");
    files.close().unwrap();
}

#[test]
fn stamped_image_without_a_bound_lease_is_not_readable() {
    let cache = ImageCache::default();
    {
        let mut admission = cache.admission("shot".into(), image(2));
        admission.stamp(Some(epoch(1)));
        admission.commit();
    }
    assert!(cache.get("shot").is_none());
}

#[test]
fn bound_lease_without_a_stamp_is_not_readable() {
    let cache = Arc::new(ImageCache::default());
    cache.bind_lease(lease(1));
    {
        let mut admission = cache.admission("shot".into(), image(3));
        admission.commit();
    }
    assert!(cache.get("shot").is_none());
}

#[test]
fn delivered_image_is_unreadable_after_rebind_and_the_new_generation_is_readable() {
    let bound = lease(1);
    let cache = Arc::new(ImageCache::default());
    cache.bind_lease(bound.clone());
    {
        let mut admission = cache.admission("shot".into(), image(4));
        admission.stamp(Some(epoch(1)));
        admission.commit();
    }
    assert!(cache.get("shot").is_some());
    let next = FixtureScope::try_new("run", 2, "engine", "desktop", "control", vec![]).unwrap();
    bound.rebind(next).unwrap();
    assert!(cache.get("shot").is_none());
    {
        let mut next_admission = cache.admission("shot".into(), image(5));
        next_admission.stamp(Some(epoch(2)));
        next_admission.commit();
    }
    assert_eq!(cache.get("shot").unwrap().bytes, vec![5]);
}

#[tokio::test(start_paused = true)]
async fn delivered_image_is_unreadable_after_lease_expiry() {
    let clock: Arc<dyn gcu_core::scope::Clock> = Arc::new(Monotonic::default());
    let scope = FixtureScope::try_new("run", 1, "engine", "desktop", "control", vec![]).unwrap();
    let bound = ScopeLease::install_with_deadline(scope, clock, Duration::from_secs(3));
    let cache = Arc::new(ImageCache::default());
    cache.bind_lease(bound.clone());
    {
        let mut admission = cache.admission("shot".into(), image(6));
        admission.stamp(Some(epoch(1)));
        admission.commit();
    }
    assert!(cache.get("shot").is_some());
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(3)).await;
    for _ in 0..8 {
        if !cache.resident("shot") {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        !cache.resident("shot"),
        "expiry must drop the pixels before get"
    );
    assert!(cache.get("shot").is_none());
    assert!(!bound.same_epoch("run", 1));
}

#[tokio::test(start_paused = true)]
async fn delivered_file_is_unreadable_after_lease_expiry() {
    let clock: Arc<dyn gcu_core::scope::Clock> = Arc::new(Monotonic::default());
    let scope = FixtureScope::try_new("run", 1, "engine", "desktop", "control", vec![]).unwrap();
    let bound = ScopeLease::install_with_deadline(scope, clock, Duration::from_secs(3));
    let files = Arc::new(SessionFiles::new().unwrap());
    files.bind_lease(bound.clone());
    let mut staged = files.stage(b"png-bytes", "png").unwrap();
    files.seal(&mut staged).unwrap();
    {
        let mut admission = files.admission(std::slice::from_ref(&staged)).unwrap();
        admission.stamp(Some(epoch(1)));
        admission.commit(std::slice::from_mut(&mut staged));
    }
    let path = staged.path.clone();
    assert_eq!(files.export().unwrap().len(), 1);
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(3)).await;
    for _ in 0..8 {
        if !path.exists() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(!path.exists(), "expiry must delete the spill before export");
    assert!(files.export().unwrap().is_empty());
    assert!(!files.contains_bytes(b"png-bytes"));
    files.close().unwrap();
}

#[tokio::test]
async fn rebind_deletes_artifacts_while_the_session_stays_open() {
    let files = Arc::new(SessionFiles::new().unwrap());
    let cache = Arc::new(ImageCache::default());
    let bound = lease(1);
    files.bind_lease(bound.clone());
    cache.bind_lease(bound.clone());
    let mut staged = files.stage(b"png-bytes", "png").unwrap();
    files.seal(&mut staged).unwrap();
    {
        let mut admission = files.admission(std::slice::from_ref(&staged)).unwrap();
        admission.stamp(Some(epoch(1)));
        admission.commit(std::slice::from_mut(&mut staged));
    }
    {
        let mut admission = cache.admission("shot".into(), image(7));
        admission.stamp(Some(epoch(1)));
        admission.commit();
    }
    let path = staged.path.clone();
    assert!(path.exists());
    assert!(cache.resident("shot"));
    let next = FixtureScope::try_new("run", 2, "engine", "desktop", "control", vec![]).unwrap();
    bound.rebind(next).unwrap();
    wait_until_retired(&path, &cache).await;
    assert!(files.export().unwrap().is_empty());
    assert!(cache.get("shot").is_none());
    files.close().unwrap();
}

async fn wait_until_retired(path: &std::path::Path, cache: &ImageCache) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while path.exists() || cache.resident("shot") {
        if tokio::time::Instant::now() >= deadline {
            panic!("invalidation left a delivered artifact");
        }
        tokio::task::yield_now().await;
    }
}
