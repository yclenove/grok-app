//! Platform-independent computer-use sessions, policies, observations, and actions.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

/// Driver interface and portable request types.
pub mod driver;
/// Deterministic node identity hashing.
pub mod fingerprint;
/// Raw accessibility model, without platform bindings.
pub mod model;
/// Tree-quality assessment and safe screenshot attachment policy.
pub mod observe;
/// Authorization, denylist, sensitive-action, and input policy.
pub mod policy;
/// Bounded text and redacted structured observations.
pub mod render;
/// Session lifecycle, serial calls, window locks, and the stop latch.
pub mod session;
/// Event/poll settling with injectable time, bounded cancellation and effect evidence.
pub mod settle;
/// Shared driver contract suite; enabled only for test consumers.
#[cfg(feature = "testing")]
pub mod testing;
/// Pure accessibility tree transforms and stable snapshot-local indices.
pub mod tree;
/// Bounded `wait_for` observation. No input delivery.
pub mod wait;

/// Mandatory screenshot redaction and safe image derivatives.
pub mod redact;
/// Session-local snapshots, stable element indices, and fingerprint-aligned diffs.
pub mod snapshot;

/// Single-attempt action routing, snapshot gates, and bounded session ledger.
pub mod action;
