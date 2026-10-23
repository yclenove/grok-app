//! One attempt's authority, shared by approval waits and screenshot selection.
//!
//! The invalidation receiver is armed before the watched generation is marked
//! seen. A generation that does not match the attempt is not marked seen and
//! does not become the authority for a later read.

use super::{lock, router::ActionRouter};
use crate::{driver::Cancel, session::Session, snapshot::WindowKey};
use gcu_protocol::ErrorCode;
use std::future::Future;
use tokio::sync::watch;

pub(super) async fn wait_cancel<T>(
    router: &ActionRouter,
    session: &Session,
    cancel: Option<&Cancel>,
    work: impl Future<Output = T>,
) -> Result<T, ErrorCode> {
    if session.is_stopped() || session.is_closed() {
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
    }
    router.check_boundary(session).await?;
    let generation = session.turn_generation();
    let mut epoch = match armed_receiver(router, session) {
        Ok(receiver) => receiver,
        Err(code) => {
            if let Some(cancel) = cancel {
                cancel.cancel();
            }
            return Err(code);
        }
    };
    tokio::pin!(work);
    loop {
        tokio::select! {
            biased;
            _ = session.cancellation().cancelled() => {
                if let Some(cancel) = cancel {
                    cancel.cancel();
                }
                return Err(ErrorCode::ApprovalDenied);
            }
            _ = lease_changed(&mut epoch) => {
                if let Some(cancel) = cancel {
                    cancel.cancel();
                    continue;
                }
                return Err(lease_code(session));
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                if session.turn_generation() != generation {
                    if let Some(cancel) = cancel {
                        cancel.cancel();
                    }
                    return Err(ErrorCode::ApprovalDenied);
                }
                if session.is_stopped() {
                    if let Some(cancel) = cancel {
                        cancel.cancel();
                    }
                }
                router.check_boundary(session).await?;
            }
            value = &mut work => {
                if session.is_stopped() || session.is_closed() {
                    if let Some(cancel) = cancel {
                        cancel.cancel();
                    }
                }
                router.check_boundary(session).await?;
                return Ok(value);
            }
        }
    }
}

/// Accept the receiver only when its current generation is the attempt's.
///
/// `borrow_and_update` runs after that comparison. A different generation stays
/// unseen on this receiver when the function returns an error before marking.
fn armed_receiver(
    router: &ActionRouter,
    session: &Session,
) -> Result<Option<watch::Receiver<u64>>, ErrorCode> {
    let Some(lease) = router.lease.as_ref() else {
        return Ok(None);
    };
    let mut receiver = lease.subscribe();
    let expected = crate::scope::expected_epoch()
        .map(|(_, generation)| generation)
        .or_else(|| lease.authorized_current().map(|scope| scope.generation()));
    let Some(expected) = expected else {
        return Err(lease_code(session));
    };
    if *receiver.borrow() != expected {
        return Err(lease_code(session));
    }
    let marked = *receiver.borrow_and_update();
    if marked != expected {
        return Err(lease_code(session));
    }
    Ok(Some(receiver))
}

fn lease_code(session: &Session) -> ErrorCode {
    if session.is_stopped() || session.activity().stop_requested() {
        ErrorCode::StoppedByUser
    } else {
        ErrorCode::AppDenied
    }
}

async fn lease_changed(epoch: &mut Option<watch::Receiver<u64>>) {
    if let Some(receiver) = epoch.as_mut() {
        let _ = receiver.changed().await;
    } else {
        std::future::pending::<()>().await;
    }
}

/// Epoch the task authorized, or the lease generation when no attempt is set.
pub(super) fn current_attempt_epoch(router: &ActionRouter) -> Option<(String, u64)> {
    crate::scope::expected_epoch().or_else(|| {
        router.lease.as_ref().and_then(|lease| {
            lease
                .authorized_current()
                .map(|scope| (scope.run_id().to_owned(), scope.generation()))
        })
    })
}

/// Refuse when the screenshot that will be used belongs to another epoch.
///
/// An empty id means this action did not select a screenshot. A scoped session
/// rejects a missing or different stamp. The current generation is not written
/// onto that screenshot. Memory only. No blocking, timeout, or cancellation.
pub(super) fn screenshot_epoch(
    router: &ActionRouter,
    session: &Session,
    key: &WindowKey,
    screenshot_id: &str,
    expected: Option<(String, u64)>,
) -> Result<(), super::wire::CallError> {
    if router.lease.is_none() || screenshot_id.is_empty() {
        return Ok(());
    }
    let Some(raw) = screenshot_id.strip_prefix("i_") else {
        return Err(super::wire::CallError::ScopeDenied);
    };
    let Ok(id) = raw.parse::<u64>() else {
        return Err(super::wire::CallError::ScopeDenied);
    };
    // Copy the stamp and drop the session lock before any later lease read.
    let stamped = {
        let resources = lock(&session.actions);
        let Some(shot) = resources
            .snapshots
            .get(key, crate::snapshot::SnapshotId(id))
        else {
            return Err(super::wire::CallError::ScopeDenied);
        };
        (shot.scope_run().map(str::to_owned), shot.scope_generation())
    };
    match (expected, stamped.0.as_deref(), stamped.1) {
        (Some((run, generation)), Some(shot_run), Some(shot_generation))
            if shot_run == run && shot_generation == generation =>
        {
            Ok(())
        }
        _ => Err(super::wire::CallError::ScopeDenied),
    }
}
