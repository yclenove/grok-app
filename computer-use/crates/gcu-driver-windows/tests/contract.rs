//! P2-09: portable harness tests and opt-in, external-oracle Windows contracts.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
#[path = "contract/discovery.rs"]
mod discovery;
#[path = "contract/harness.rs"]
mod harness;
#[path = "contract/invoke_target.rs"]
mod invoke_target;
#[path = "contract/landing_plan.rs"]
mod landing_plan;
#[cfg(windows)]
#[path = "contract/live.rs"]
mod live;
#[path = "contract/mouse_matrix.rs"]
mod mouse_matrix;
#[path = "contract/portable.rs"]
mod portable;
#[path = "contract/readiness.rs"]
mod readiness;
#[cfg(windows)]
#[path = "contract/smoke.rs"]
mod smoke;
#[path = "contract/timings.rs"]
mod timings;
#[cfg(windows)]
#[path = "contract/unsafe_mouse_probe.rs"]
mod unsafe_mouse_probe;
