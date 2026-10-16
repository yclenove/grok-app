//! P2-09: portable harness tests and opt-in, external-oracle Windows contracts.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
#[path = "contract/discovery.rs"]
mod discovery;
#[path = "contract/harness.rs"]
mod harness;
#[cfg(windows)]
#[path = "contract/live.rs"]
mod live;
#[path = "contract/portable.rs"]
mod portable;
#[cfg(windows)]
#[path = "contract/smoke.rs"]
mod smoke;
