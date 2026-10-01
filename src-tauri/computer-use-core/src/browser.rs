//! Ownership registry only. This does not establish a browser connection or
//! authenticate an extension; the transport must do both before registration.

pub mod action_ledger;
mod existing_adapter;
pub mod extension_action;
pub mod extension_completion;
pub(crate) mod extension_image;
pub mod extension_protocol;
mod host;
pub mod host_allowlist;
mod managed_adapter;
pub mod observation;
#[cfg(any(test, feature = "test-support"))]
mod recording;
mod request;
mod run_lifecycle;
pub use request::{ManagedRequest, ManagedRequestIdentity};
#[cfg(any(test, feature = "test-support"))]
mod tests_gates;
#[cfg(test)]
mod tests_profile_lifecycle;
#[cfg(test)]
mod tests_request_binding;
#[cfg(test)]
mod tests_worker_error;
mod types;
mod validation;
mod worker;
pub mod worker_env;
pub mod worker_http;

pub use existing_adapter::ExistingBrowserAdapter;
pub use host::{ExistingTabHost, TabAttachment};
pub use managed_adapter::ManagedBrowserAdapter;
pub use observation::{
    model_visible_url, parse_managed_observation, parse_managed_page, parse_worker_success,
};
pub use run_lifecycle::{
    parse_worker_run_state, WorkerRunPhase, WorkerRunRevision, WorkerRunState,
};
pub use types::*;
pub use validation::{is_allowed_navigate_url, navigation_result_allowed};
pub use worker::ManagedBrowserWorker;
pub use worker_env::{csprng_bearer_token, keep_worker_env_key};
pub use worker_http::{
    bounded_loopback_post, bounded_loopback_post_cancellable, require_loopback_base,
    WORKER_HTTP_MAX_BYTES,
};

pub(crate) use validation::validate_browser_action_id;

#[cfg(any(test, feature = "test-support"))]
pub use recording::{RecordingBrowserWorker, RecordingGotoBarrier};
#[cfg(any(test, feature = "test-support"))]
pub use tests_gates::run_browser_gates;
