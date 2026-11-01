//! Whether a delivered row is still the lease's current commit.
//!
//! The cache or file lock is already held. [`ScopeLease::same_epoch`] may take
//! the lease lock after that, which is the same order as `commit_if_current`.
//! Memory only. No timeout or cancellation.

use gcu_core::scope::ScopeLease;

/// Run id and generation stamped inside the existing commit closure.
pub(crate) type Epoch = (String, u64);

/// `(None, None)` is the unscoped cache. Any other mismatch is hidden.
pub(crate) fn visible(lease: Option<&ScopeLease>, epoch: Option<&Epoch>) -> bool {
    match (lease, epoch) {
        (None, None) => true,
        (Some(lease), Some((run, generation))) => lease.same_epoch(run, *generation),
        _ => false,
    }
}
