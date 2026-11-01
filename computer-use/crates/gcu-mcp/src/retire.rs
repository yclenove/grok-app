//! Drop delivered images and files when the lease generation is no longer current.
//!
//! Armed from `bind_lease` on whatever runtime is current. A sync caller leaves
//! the flag unset so a later runtime can still start the task. Disk cleanup
//! runs after the store lock is released. The lease lock is only the one
//! [`crate::lease_view::visible`] already takes, and it is not held across
//! unlink.

use gcu_core::scope::ScopeLease;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Start one invalidation task for `lease`.
///
/// Does nothing when no runtime is current, and does not set `flag` in that
/// case. A second call with the flag already set does not start another task.
/// `retire` must not delete files while it holds the lease lock.
pub(crate) fn arm(flag: &AtomicBool, lease: ScopeLease, retire: impl Fn() + Send + Sync + 'static) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    if flag.swap(true, Ordering::SeqCst) {
        return;
    }
    let retire = Arc::new(retire);
    tokio::spawn(async move {
        drive(lease, retire).await;
    });
}

async fn drive(lease: ScopeLease, retire: Arc<dyn Fn() + Send + Sync>) {
    let mut epoch = lease.subscribe();
    loop {
        // Expiry publishes only when the lease is read. A zero remainder is
        // already revoked, including a paused clock that advanced first.
        if lease.remaining() == Some(Duration::ZERO) {
            retire();
            break;
        }
        tokio::select! {
            biased;
            changed = epoch.changed() => {
                if changed.is_err() {
                    retire();
                    break;
                }
            }
            _ = sleep_remaining(&lease), if lease.remaining().is_some_and(|left| !left.is_zero()) => {}
        }
        retire();
        if lease.current().is_none() {
            break;
        }
    }
}

async fn sleep_remaining(lease: &ScopeLease) {
    match lease.remaining() {
        Some(left) if !left.is_zero() => tokio::time::sleep(left).await,
        _ => std::future::pending::<()>().await,
    }
}
