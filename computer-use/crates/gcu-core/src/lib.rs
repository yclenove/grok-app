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
/// Shared driver contract suite; enabled only for test consumers.
#[cfg(feature = "testing")]
pub mod testing;
